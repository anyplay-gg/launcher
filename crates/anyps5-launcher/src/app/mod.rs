//! The application: the model, the I/O behind it and the host that drives the screens each frame.
//!
//! | Module | What it is |
//! |---|---|
//! | [`model`] | the state machine (`AppModel.swift`): states, actions, onboarding, library bookkeeping |
//! | [`pipeline`] | the seam to the unlock/launch pipeline of `anyps5-core` |
//! | [`overlay`] | the overlay enum and the hook the panel modules implement |
//! | [`services`] | scan, watcher, size measuring, trophy loading, folder dialog |
//! | [`textures`] | icons and artwork as textures, decoded off the UI thread |
//! | [`preview`] | made-up games, scenes and the `--gallery=home...` screenshots |
//!
//! [`LauncherApp`] is the `eframe::App`. Per frame it applies finished work to the model, turns
//! input into model commands, runs the model's effects and paints: backdrop, game artwork, the
//! onboarding or the home screen, the overlays, the launch splash and the toast.

// Parts of the model, pipeline and overlay API are for the panel modules and the pipeline adapter.
#![allow(dead_code)]

pub mod avatars;
pub mod clock;
pub mod core_pipeline;
mod discord;
pub mod links;
pub mod model;
pub mod online;
pub mod online_local;
pub mod online_screen;
pub mod overlay;
pub mod panels;
pub mod phone;
pub mod pipeline;
pub mod playtime;
pub mod preview;
pub mod profile;
pub mod run_question;
pub mod selfupdate;
pub mod services;
pub mod textures;
pub mod toolkit;
pub mod window_chrome;

use std::path::PathBuf;
use std::time::Duration;

use anyps5_core::discord::Presence;
use anyps5_core::paths::{AppPaths, GameId};
use anyps5_core::settings::Preferences;
use anyps5_core::toolkit::GameToolkit;
use egui::{Context, Rect, Ui, UiBuilder, ViewportCommand};

use crate::screens::{self, ScreenCx};
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::artwork;
use crate::ui::chrome;
use crate::ui::i18n::Key;
use crate::ui::input::InputEvent;

use self::core_pipeline::CorePipeline;
use self::model::{Effect, Model, Routed, ui_accent};
use self::overlay::{Overlay, OverlayCx, OverlayHost};
use self::pipeline::{Pipeline, PipelineContext};
use self::services::Services;
use self::textures::Textures;

/// Everything the host is made of; the real launcher and the previews differ only in these.
pub struct Config {
    pub prefs: Preferences,
    pub paths: AppPaths,
    /// Save the preferences (off for previews and tests).
    pub persist: bool,
    pub pipeline: Box<dyn Pipeline>,
    pub panels: Box<dyn OverlayHost>,
    /// Scan the games folder at start (off when the library is set up by hand).
    pub scan_on_start: bool,
}

impl Config {
    /// The launcher as users run it: preferences from `%APPDATA%`, the core pipeline, and no panels
    /// until a panel module registers its host.
    pub fn from_environment(ctx: &Context) -> Self {
        let paths = AppPaths::from_env()
            .unwrap_or_else(|| AppPaths::under(&std::env::temp_dir().join("anyps5-launcher")));
        let prefs = Preferences::load(&paths);
        #[cfg(target_os = "macos")]
        let prefs = {
            use anyps5_core::platform::{self, Os};
            if let Some(old) = platform::roots(Os::Linux, &platform::process_vars, None) {
                Preferences::load_with_fallback(
                    &paths,
                    &AppPaths {
                        config: old.config,
                        data: old.data,
                        pictures: old.pictures,
                    },
                )
            } else {
                prefs
            }
        };
        crate::startup::mark("preferences loaded");
        // Whatever the last self-update swap left behind (`.old` exes, stale downloads).
        if let Ok(exe) = std::env::current_exe() {
            anyps5_core::selfupdate::cleanup(&exe, &paths, env!("CARGO_PKG_VERSION"));
        }
        crate::startup::mark("update leftovers cleaned");
        let repaint = ctx.clone();
        let wake: std::sync::Arc<dyn Fn() + Send + Sync> =
            std::sync::Arc::new(move || repaint.request_repaint());
        let pipeline = CorePipeline::new(paths.clone(), &prefs, wake);
        crate::startup::mark("toolkit and queue ready");
        Self {
            pipeline: Box::new(pipeline),
            prefs,
            paths,
            persist: true,
            panels: Box::new(panels::PanelHost::new(ctx, false)),
            scan_on_start: true,
        }
    }
}

/// Which preference values the shell has been given, to apply changes once.
#[derive(Default)]
struct Applied {
    reduced_motion: Option<bool>,
}

pub struct LauncherApp {
    pub shell: Shell,
    pub model: Model,
    pub textures: Textures,
    pub services: Services,
    pub pipeline: Box<dyn Pipeline>,
    pub panels: Box<dyn OverlayHost>,
    /// The net service behind `model.online`.
    pub online: online::OnlineService,
    /// The phone server behind `model.phone`.
    pub phone: phone::PhoneService,
    applied: Applied,
    focused: bool,
    /// Overlays that are open or still animating out.
    overlays_shown: Vec<Overlay>,
    /// Artwork behind the home screen that is fading in or out, newest last.
    artwork: Vec<GameId>,
    started: bool,
    /// Play the launch intro and the boot cue on the next frame (real launches only).
    pub intro: bool,
    /// Show the running game on Discord (real launches only; previews and tests never touch it).
    discord: bool,
    presence: Option<Presence>,
    /// The game that runs, as far as Discord is told of it (see `discord.rs`).
    playing: Option<discord::Played>,
    /// What Discord shows right now.
    shown: Option<anyps5_core::discord::Playing>,
    start_screen: screens::start::StartScreen,
    /// The ringing card and the call strip.
    calls: screens::calls::CallsUi,
    links: links::LinkState,
    scan_on_start: bool,
    /// No scan, size or trophy work (previews and the demo have a made-up library).
    pub offline: bool,
    /// Keep the backdrop clock still (screenshots).
    pub freeze_backdrop: bool,
    /// Behave as if the window had focus (the demo, to measure a focused launcher unattended).
    pub assume_focus: bool,
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        let mut app = Self::from_shell(Shell::new(cc), &cc.egui_ctx, config);
        app.intro = true;
        app.discord = true;
        app
    }

    /// With an already built shell (previews use a silent one).
    pub fn from_shell(shell: Shell, ctx: &Context, config: Config) -> Self {
        let simulated = !config.persist;
        let mut model = Model::new(config.prefs, config.paths, config.persist);
        model.toolkit = config.pipeline.toolkit_version();
        model.glibc = config.pipeline.glibc_shortfall();
        let repaint = ctx.clone();
        let wake: std::sync::Arc<dyn Fn() + Send + Sync> =
            std::sync::Arc::new(move || repaint.request_repaint());
        Self {
            phone: phone::PhoneService::new(wake.clone(), simulated, shell.home.clone()),
            online: online::OnlineService::new(wake, simulated),
            shell,
            model,
            textures: Textures::new(ctx),
            services: Services::new(ctx),
            pipeline: config.pipeline,
            panels: config.panels,
            applied: Applied::default(),
            focused: true,
            overlays_shown: Vec::new(),
            artwork: Vec::new(),
            started: false,
            intro: false,
            discord: false,
            presence: None,
            playing: None,
            shown: None,
            start_screen: screens::start::StartScreen::default(),
            calls: screens::calls::CallsUi::default(),
            links: links::LinkState::default(),
            scan_on_start: config.scan_on_start,
            offline: false,
            freeze_backdrop: false,
            assume_focus: false,
        }
    }

    // --- per frame ----------------------------------------------------------------------------

    /// What the games that stay on a release of their own find on this PC (docs/DESIGN.md 3.8).
    fn pinned_toolkits(&mut self) -> std::collections::HashMap<GameId, GameToolkit> {
        let global = self.model.toolkit.clone();
        let pins: Vec<(String, String)> = self
            .model
            .prefs
            .games
            .iter()
            .filter_map(|(id, settings)| Some((id.clone(), settings.toolkit.clone()?)))
            .collect();
        pins.into_iter()
            .map(|(id, pin)| {
                (
                    GameId::from_key(id),
                    self.pipeline.pinned_toolkit(&pin, global.as_deref()),
                )
            })
            .collect()
    }

    /// Everything before painting: finished work, input, effects, preferences.
    pub fn step(&mut self, ctx: &Context) {
        self.shell.input.blocked = self.services.dialog_open();
        self.shell.input.tab_for_egui = crate::screens::calls::on_screen(&self.model.online);
        let frame = self.shell.begin_frame(ctx);
        let now = ctx.clock();
        self.model.tick(now);

        if !self.started {
            self.started = true;
            if self.scan_on_start && self.model.onboarded() {
                self.model.rescan();
            }
            // Signed in on this PC before: the net service comes up with the launcher.
            if self.model.prefs.online {
                self.model.effects.push(Effect::StartOnline);
            }
            self.model.online.voice.load(self.model.prefs.voice.clone());
        }

        for event in self.services.drain() {
            self.model.on_core(event);
        }
        self.model.toolkit = self.pipeline.toolkit_version();
        self.model.glibc = self.pipeline.glibc_shortfall();
        self.model.game_toolkits = self.pinned_toolkits();
        self.model.patches_dir = self.pipeline.patches_dir();
        for event in self.pipeline.poll(now) {
            if let pipeline::PipelineEvent::Exited { game, .. }
            | pipeline::PipelineEvent::StartFailed { game, .. } = &event
            {
                self.online.on_exited(&mut self.model, game);
            }
            self.model.on_pipeline(event);
        }
        self.push_to_talk(ctx);
        for ready in self.online.frame(&mut self.model) {
            self.submit(
                pipeline::PipelineCommand::Play(ready.game),
                Some(&ready.env),
            );
        }
        if let Some(answer) = self.services.poll_folder() {
            self.model.choose_folder(answer);
        }
        self.textures.poll();

        let focused = self.assume_focus || ctx.input(|i| i.viewport().focused).unwrap_or(true);
        if focused && !self.focused {
            self.model.window_focused();
        }
        if self.focused && !focused {
            // Someone else took the front (the player switched to the game by hand): Back no
            // longer means "back to the game".
            self.model.leave_in_game();
        }
        self.focused = focused;

        self.apply_preferences(ctx);
        self.sync_home_button();
        if std::mem::take(&mut self.intro) {
            crate::ui::intro::start(ctx);
            self.shell.play(crate::ui::sound::Cue::Boot);
        }
        for event in frame.events {
            self.handle_event(ctx, event);
        }
        self.poll_links();
        self.run_effects(ctx);
        self.sync_discord();
        self.sync_phone(ctx);
        self.watch_trophies();

        self.services.set_busy(self.model.busy());
        self.save_preferences();
    }

    /// Tells the net service whether the talk key is down. Only a launcher in front sees keys:
    /// when it loses the focus, or the player types somewhere, the key counts as up. A game that
    /// has the keyboard gets no push to talk; voice activity is for playing (docs/DESIGN.md 17).
    fn push_to_talk(&mut self, ctx: &Context) {
        let focused = self.assume_focus || ctx.input(|i| i.viewport().focused).unwrap_or(true);
        let key = crate::screens::online::talk_key(&self.model.online);
        let held = focused
            && !self.shell.input.capture_keys
            && !self.shell.input.blocked
            && !crate::ui::a11y::foreign_focus(ctx)
            && ctx.input(|i| i.key_down(key) && !i.modifiers.any());
        self.model.online.set_talking(held);
    }

    /// Writes the preferences when something changed (atomic, see `Preferences::save`).
    pub fn save_preferences(&mut self) {
        if self.model.online.voice.take_changed() {
            let voice = self.model.online.voice.prefs().clone();
            self.model.edit_prefs(|p| p.voice = voice);
        }
        if self.model.take_prefs_dirty()
            && let Err(error) = self.model.prefs.save(&self.model.paths)
        {
            tracing::warn!("cannot save the preferences: {error}");
        }
    }

    /// The running game's trophy records are polled for the banner while it runs.
    fn watch_trophies(&mut self) {
        if self.offline {
            return;
        }
        let running = self.model.running.clone();
        match running.and_then(|id| self.model.game(&id).cloned()) {
            Some(game) => {
                let root = self.model.prefs.games_folder.clone();
                self.services
                    .watch_trophies(&game, root.as_deref(), &self.model.prefs);
            }
            None => self.services.stop_trophy_watch(),
        }
    }

    /// After painting: the pointer's tick. Silent while a game starts or runs in front and
    /// while the launcher window is not the focused one.
    pub fn finish_frame(&mut self, ctx: &Context) {
        let playing = self.model.running.is_some() || self.model.starting.is_some();
        self.shell.finish_frame(ctx, !playing && self.focused);
    }

    /// Shows the preferences to the shell: language, theme, accent, motion, sounds, music.
    fn apply_preferences(&mut self, ctx: &Context) {
        self.shell.language = self.model.language();
        let theme = self.model.theme();
        let accent = self.model.prefs.accent.map(ui_accent);
        let wanted_accent = accent.unwrap_or_else(|| theme.default_accent());
        if self.shell.backdrop.theme() != theme || self.shell.accent != wanted_accent {
            self.shell.set_theme(theme, accent);
        }
        let reduced = self.model.prefs.reduced_motion;
        if self.applied.reduced_motion != Some(reduced) {
            self.applied.reduced_motion = Some(reduced);
            self.shell.set_reduced_motion(ctx, reduced);
        }
        self.shell.sounds.enabled = self.model.prefs.sounds;
        if !self.model.prefs.discord_presence {
            self.clear_presence();
        }
        // The music fades out while a game starts or runs and while another window has focus.
        let playing = self.model.running.is_some() || self.model.starting.is_some();
        self.shell
            .sounds
            .set_music(self.model.prefs.music && !playing && self.focused);
        self.shell.backdrop.paused =
            self.model.running.is_some() || !self.focused || self.freeze_backdrop;
    }

    /// Tells the input threads about the game and the window (the Home button acts there, not
    /// here: a hidden launcher draws no frames), and takes what the button did since the last
    /// frame: the quick panel opens when it brought the launcher, closes when it sent the player
    /// back to the game.
    fn sync_home_button(&mut self) {
        let model = &self.model;
        let game = model
            .running
            .clone()
            .filter(|_| model.starting.is_none() && model.onboarded());
        let window = game
            .as_ref()
            .and_then(|id| self.pipeline.game_target(id))
            .and_then(|target| target.window)
            .map(|window| window as isize);
        let home = &self.shell.home;
        home.set_game(game.is_some(), window);
        home.set_enabled(model.prefs.ps_button);
        home.set_focused(self.focused);
        if home.take_returned() {
            self.model.leave_in_game();
        }
        if home.take_summoned() {
            self.model.show_in_game();
        }
    }

    /// Takes the game off Discord if it is shown there.
    fn clear_presence(&mut self) {
        if self.shown.take().is_some()
            && let Some(presence) = &self.presence
        {
            presence.clear();
        }
    }

    pub fn handle_event(&mut self, ctx: &Context, event: InputEvent) {
        match event {
            InputEvent::Command(command) => match self.model.handle(command) {
                Routed::Overlay(overlay) => {
                    let mut cx = OverlayCx {
                        model: &mut self.model,
                        shell: &mut self.shell,
                        textures: &mut self.textures,
                        ctx,
                    };
                    self.panels.command(&mut cx, overlay, command);
                }
                Routed::Done => {}
            },
            InputEvent::Tab { backwards } => {
                if !self.model.onboarded() {
                    // The onboarding steps lay their choices out side by side.
                    let command = if backwards {
                        crate::ui::input::Command::Left
                    } else {
                        crate::ui::input::Command::Right
                    };
                    self.model.onboarding_command(command);
                } else if self.model.overlay.is_some() {
                    // A panel is a column of rows: reading order is down and up.
                    let command = if backwards {
                        crate::ui::input::Command::Up
                    } else {
                        crate::ui::input::Command::Down
                    };
                    self.handle_event(ctx, InputEvent::Command(command));
                } else {
                    self.model.tab_home(backwards);
                }
            }
            InputEvent::Scroll(command) => {
                if self.model.onboarded()
                    && self.model.overlay.is_none()
                    && self.model.starting.is_none()
                {
                    let delta = if command == crate::ui::input::Command::Left {
                        -1
                    } else {
                        1
                    };
                    self.model.move_selection(delta);
                }
            }
            InputEvent::OpenSettings => {
                if self.model.onboarded() && self.model.starting.is_none() {
                    self.model.open(Overlay::Settings);
                }
            }
            InputEvent::ToggleFullscreen => {
                let fullscreen = ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
                self.set_fullscreen(ctx, !fullscreen);
            }
            InputEvent::CapturedKey {
                key,
                physical_key,
                modifiers,
            } => {
                let mut cx = OverlayCx {
                    model: &mut self.model,
                    shell: &mut self.shell,
                    textures: &mut self.textures,
                    ctx,
                };
                self.panels
                    .captured_key(&mut cx, key, physical_key, modifiers);
            }
        }
    }

    /// Executes what the model queued.
    fn run_effects(&mut self, ctx: &Context) {
        for _ in 0..4 {
            let effects = std::mem::take(&mut self.model.effects);
            if effects.is_empty() {
                break;
            }
            for effect in effects {
                self.run_effect(ctx, effect);
            }
        }
    }

    fn run_effect(&mut self, ctx: &Context, effect: Effect) {
        let root = self.model.prefs.games_folder.clone();
        match effect {
            Effect::Sound(cue) => self.shell.play(cue),
            Effect::Pipeline(pipeline::PipelineCommand::Play(id)) => {
                // Signed in, the start waits for the network bridge's variables.
                let Some(game) = self.model.game(&id).cloned() else {
                    self.submit(pipeline::PipelineCommand::Play(id), None);
                    return;
                };
                if let Some(env) = self.online.on_play(&mut self.model, &game) {
                    self.submit(pipeline::PipelineCommand::Play(id), Some(&env));
                }
            }
            Effect::Pipeline(command) => self.submit(command, None),
            Effect::StartOnline => self.online.start(&mut self.model),
            Effect::StopOnline => self.online.stop(&mut self.model),
            Effect::RelayOnlyChanged => self.online.relay_only_changed(&mut self.model),
            Effect::ScreenStart => {
                let target = self
                    .model
                    .running
                    .as_ref()
                    .and_then(|game| self.pipeline.game_target(game))
                    .unwrap_or_default();
                self.model.online.screen_start(target.window, target.pids);
            }
            Effect::Rescan if self.offline => self.model.scanning = false,
            Effect::MeasureSize(_)
            | Effect::LoadTrophies { .. }
            | Effect::BackUpSaves(_)
            | Effect::SyncTrophies
                if self.offline => {}
            Effect::Rescan => {
                if let Some(root) = root {
                    if !self.model.busy() {
                        self.pipeline.before_scan(&root);
                    }
                    self.services.rescan(&root);
                }
            }
            Effect::PickGamesFolder => {
                let title = self.model.language().text(Key::PickFolder);
                self.services.pick_folder(title, root.as_deref(), ctx);
            }
            Effect::MeasureSize(id) => {
                if let Some(game) = self.model.game(&id) {
                    self.services.measure(game);
                }
            }
            Effect::LoadTrophies { game, .. } => {
                if let Some(game) = self.model.game(&game) {
                    self.services
                        .load_trophies(game, root.as_deref(), &self.model.prefs);
                }
            }
            Effect::SyncTrophies => {
                if let Some(root) = root {
                    self.services.sync_trophies(&self.model.games, &root);
                }
            }
            Effect::ReloadToolkit => self.pipeline.reload_toolkit(&self.model.prefs),
            Effect::BackUpSaves(id) => {
                if let (Some(game), Some(root)) = (self.model.game(&id), root) {
                    self.services.back_up_saves(game, &root, &self.model.paths);
                }
            }
            Effect::Presence(Some(game)) if self.discord => {
                self.playing = Some(discord::Played::now(game));
            }
            Effect::Presence(_) => {
                self.playing = None;
                self.clear_presence();
            }
            Effect::MinimizeWindow => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
            Effect::RestoreWindow => {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
        }
    }

    /// Hands a command to the pipeline with the game it is about.
    fn submit(
        &mut self,
        command: pipeline::PipelineCommand,
        extra_env: Option<&std::collections::BTreeMap<String, String>>,
    ) {
        let root = self.model.prefs.games_folder.clone();
        let game = self.model.game(command.game()).cloned();
        self.pipeline.submit(
            command,
            &PipelineContext {
                root: root.as_deref(),
                game: game.as_ref(),
                prefs: &self.model.prefs,
                extra_env,
            },
        );
    }

    // --- painting ------------------------------------------------------------------------------

    /// Paints one frame: backdrop, artwork, screen, overlays, splash, toast.
    pub fn paint(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();
        let painter = ui.painter().clone();
        self.shell.paint_backdrop(&ctx, &painter, screen);
        self.paint_artwork(&ctx, screen);

        let onboarded = ctx.animate_to(
            "onboarded",
            f32::from(u8::from(self.model.onboarded())),
            Motion::Fade,
        );
        // Fade the onboarding out, then the home screen in.
        let (show_onboarding, show_home, opacity) = if onboarded < 0.5 {
            (true, false, 1.0 - 2.0 * onboarded)
        } else {
            (false, true, 2.0 * onboarded - 1.0)
        };
        {
            let mut child = ui.new_child(UiBuilder::new().max_rect(screen));
            child.set_opacity(opacity.clamp(0.0, 1.0));
            let mut cx = ScreenCx {
                model: &mut self.model,
                shell: &self.shell,
                textures: &mut self.textures,
                ctx: &ctx,
            };
            if show_onboarding {
                screens::onboarding::show(&mut child, &mut cx);
            } else if show_home {
                screens::home::show(&mut child, &mut cx);
            }
        }

        self.paint_overlays(ui, &ctx);
        self.paint_start(&ctx);
        self.paint_toast(&ctx);
        self.calls.show(&ctx, &mut self.model, &mut self.shell);
        let mut cx = OverlayCx {
            model: &mut self.model,
            shell: &mut self.shell,
            textures: &mut self.textures,
            ctx: &ctx,
        };
        self.panels.frame(&mut cx);
        if let Some(fullscreen) = crate::ui::caption::show(&ctx) {
            self.set_fullscreen(&ctx, fullscreen);
        }
        crate::ui::intro::paint_curtain(&ctx);
        self.run_effects(&ctx);
        self.save_preferences();
        self.schedule_repaints(&ctx);
    }

    /// Switches between filling the screen and a maximised window, and remembers the choice.
    fn set_fullscreen(&mut self, ctx: &Context, fullscreen: bool) {
        enter_fullscreen(ctx, fullscreen);
        self.model.edit_prefs(|p| p.fullscreen = fullscreen);
    }

    /// The launch splash (the panels module's `StartScreen`) follows the model's `starting`.
    fn paint_start(&mut self, ctx: &Context) {
        let starting = self.model.starting.clone();
        if starting.is_some() && !self.start_screen.is_open() {
            self.start_screen.open_at(ctx.clock());
        } else if starting.is_none() && self.start_screen.is_open() {
            self.start_screen.close();
        }
        if !self.start_screen.is_active() {
            return;
        }
        // The last game shown stays for the way out.
        let id = starting.or_else(|| self.model.running.clone());
        let Some(game) = id.and_then(|id| self.model.game(&id).cloned()) else {
            return;
        };
        let icon = game.icon.as_ref().and_then(|path| {
            self.textures
                .get(path, 400)
                .map(|l| crate::ui::chrome::TileImage {
                    texture: l.id,
                    aspect: l.aspect,
                })
        });
        let data = screens::start::StartData {
            game_name: &game.name,
            icon,
            language: self.model.language(),
            accent: self.shell.accent_color(),
            reduced_motion: ctx.reduced_motion(),
        };
        // Back is the model's: it cancels the start and closes the splash.
        self.start_screen.show(&mut self.shell, ctx, &[], &data);
    }

    fn paint_overlays(&mut self, ui: &mut Ui, ctx: &Context) {
        if let Some(open) = self.model.overlay
            && !self.overlays_shown.contains(&open)
        {
            self.overlays_shown.push(open);
        }
        let open = self.model.overlay;
        let mut still_shown = Vec::new();
        for overlay in self.overlays_shown.clone() {
            let openness = ctx.animate_to(
                ("overlay", overlay),
                f32::from(u8::from(open == Some(overlay))),
                Motion::Overlay,
            );
            if openness <= 0.003 && open != Some(overlay) {
                continue;
            }
            still_shown.push(overlay);
            let mut cx = OverlayCx {
                model: &mut self.model,
                shell: &mut self.shell,
                textures: &mut self.textures,
                ctx,
            };
            self.panels
                .ui(ui, &mut cx, overlay, openness.clamp(0.0, 1.0));
        }
        self.overlays_shown = still_shown;
    }

    fn paint_toast(&mut self, ctx: &Context) {
        let visible = self.model.toast_visible();
        let v = ctx.animate_to(
            "toast-visible",
            f32::from(u8::from(visible)),
            Motion::Overlay,
        );
        if let Some(toast) = &self.model.toast {
            chrome::toast(ctx, "launcher-toast", toast.icon, &toast.text, v);
        }
        if visible && let Some(toast) = &self.model.toast {
            let left = (toast.until - ctx.clock()).max(0.0);
            ctx.request_repaint_after(Duration::from_secs_f64(left + 0.02));
        }
    }

    /// The clock of the top bar changes every minute.
    fn schedule_repaints(&self, ctx: &Context) {
        ctx.request_repaint_after(Duration::from_secs_f32(clock::seconds_to_next_minute()));
    }

    /// The selected game's `pic0.png` behind the home screen, cross-faded per selection, drifting.
    fn paint_artwork(&mut self, ctx: &Context, screen: Rect) {
        paint_artwork_layer(
            ctx,
            screen,
            &self.model,
            &mut self.textures,
            &mut self.artwork,
            self.shell.backdrop.paused,
        );
    }
}

/// The selected game's artwork over the backdrop, cross-fading when the game changes, and the
/// state the home screen reads its material from. Shared by the app and the gallery (which draws
/// the real home under its panels).
pub(crate) fn paint_artwork_layer(
    ctx: &Context,
    screen: Rect,
    model: &Model,
    textures: &mut Textures,
    artwork: &mut Vec<GameId>,
    paused: bool,
) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let wanted =
        model.onboarded() && model.prefs.game_artwork && model.overlay != Some(Overlay::Settings);
    let current = if wanted {
        model
            .selected_game()
            .and_then(|g| g.background.clone().map(|path| (g.id.clone(), path)))
    } else {
        None
    };
    if current.is_none() {
        ctx.data_mut(|d| d.remove::<screens::ArtState>(screens::art_state_id()));
    }
    if let Some((id, _)) = &current {
        artwork.retain(|a| a != id);
        artwork.push(id.clone());
    }
    let ppp = ctx.pixels_per_point();
    let max_dim = ((screen.width().max(screen.height()) * ppp * 1.6) as u32).min(2048);
    let reduced = ctx.reduced_motion();
    let mut keep = Vec::new();
    for id in std::mem::take(artwork) {
        let is_current = current.as_ref().is_some_and(|(c, _)| *c == id);
        let Some(path) = model.game(&id).and_then(|g| g.background.clone()) else {
            continue;
        };
        let Some(tex) = textures.get(&path, max_dim) else {
            keep.push(id);
            continue;
        };
        let alpha = ctx.animate_from_to(
            ("artwork", id.as_str().to_owned()),
            0.0,
            f32::from(u8::from(is_current)),
            Motion::Fade,
        );
        if is_current {
            let material = textures.get(&path, textures::MATERIAL);
            let state = material.map(|m| screens::ArtState {
                material: m.id,
                aspect: tex.aspect,
                alpha,
                zoom: artwork::drift_zoom(ctx.clock(), reduced),
            });
            ctx.data_mut(|d| match state {
                Some(state) => {
                    d.insert_temp(screens::art_state_id(), state);
                }
                None => d.remove::<screens::ArtState>(screens::art_state_id()),
            });
        }
        if alpha > 0.003 || is_current {
            artwork::paint_artwork(
                &painter,
                screen,
                tex.id,
                tex.aspect,
                ctx.clock(),
                reduced,
                alpha,
            );
            keep.push(id);
        }
        if alpha > 0.003 && !reduced && !paused {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
    *artwork = keep;
}

/// Fills the screen, or leaves it for a maximised window. A maximised window is restored first:
/// Windows keeps a maximised frameless window's content to the work area, so in fullscreen the
/// taskbar's strip would stay empty.
fn enter_fullscreen(ctx: &Context, fullscreen: bool) {
    if fullscreen {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(false));
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
    } else {
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
        ctx.send_viewport_cmd(ViewportCommand::Maximized(true));
    }
}

/// Runs the launcher. `inbox` brings the arguments of later launches; `arguments` are the ones this
/// launch got (a link or a folder).
/// The app id on Linux: Wayland's app_id and X11's WM_CLASS. It must match the desktop entry's
/// file name, `StartupWMClass` and `Icon` (tools/linux/anyplay-launcher.desktop).
#[cfg(any(not(windows), test))]
const APP_ID: &str = "anyplay-launcher";

pub fn run(inbox: Option<crate::instance::Inbox>, arguments: Vec<String>) -> eframe::Result {
    // Read before the window exists: it opens filling its screen unless the player chose a window.
    let fullscreen = AppPaths::from_env()
        .map(|paths| Preferences::load(&paths).fullscreen)
        .unwrap_or(true);
    let viewport = egui::ViewportBuilder::default()
        .with_title("AnyPlay Launcher")
        // The launcher draws its own caption (ui::caption).
        .with_decorations(false)
        .with_inner_size([1600.0, 900.0])
        .with_min_inner_size([
            crate::ui::theme::MIN_WINDOW.0,
            crate::ui::theme::MIN_WINDOW.1,
        ])
        // Filling the screen starts from a plain window (see `enter_fullscreen`).
        .with_maximized(!fullscreen);
    // X11 and Wayland match the window to its desktop entry by this name.
    #[cfg(not(windows))]
    let viewport = viewport.with_app_id(APP_ID);
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    crate::ui::restrict_backends(&mut options);
    crate::startup::mark("window requested");
    let result = eframe::run_native(
        "AnyPlay Launcher",
        options,
        Box::new(|cc| {
            crate::startup::mark("window and GPU ready");
            let mut app = LauncherApp::new(cc, Config::from_environment(&cc.egui_ctx));
            crate::startup::mark("app built");
            app.attach_links(&cc.egui_ctx, inbox, &arguments);
            Ok(Box::new(App {
                app,
                chrome_applied: false,
                frames: 0,
                start_fullscreen: fullscreen,
            }))
        }),
    );
    match result {
        // No DirectX 12 GPU: start again on Vulkan (the window never opened, nothing is lost).
        Err(error) if crate::ui::can_fall_back_to_vulkan() => {
            tracing::warn!("cannot start on DirectX 12 ({error}); trying Vulkan");
            relaunch_on_vulkan()
        }
        other => other,
    }
}

/// Starts this program again with the Vulkan backend and the same arguments, then ends. This process
/// holds the single instance, so the new one waits for it to end (`--after-update=<pid>`) instead
/// of handing itself over to it.
fn relaunch_on_vulkan() -> eframe::Result {
    let mut args: Vec<std::ffi::OsString> = std::env::args_os()
        .skip(1)
        .filter(|a| {
            !a.to_string_lossy()
                .starts_with(crate::instance::AFTER_UPDATE)
        })
        .collect();
    args.push(format!("{}{}", crate::instance::AFTER_UPDATE, std::process::id()).into());
    let started = std::env::current_exe().and_then(|exe| {
        std::process::Command::new(exe)
            .args(args)
            .env(crate::ui::GPU_ENV, "vulkan")
            .spawn()
    });
    if let Err(error) = started {
        tracing::error!("cannot start again: {error}");
    }
    Ok(())
}

/// The eframe wrapper of [`LauncherApp`].
pub struct App {
    pub app: LauncherApp,
    /// The window frame is styled once, on the first frame (the window exists by then).
    chrome_applied: bool,
    frames: u32,
    /// Fill the screen once the window is shown: asked for at creation, while eframe still keeps
    /// the window hidden, Windows ignores it (and egui would believe it took).
    start_fullscreen: bool,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        if !self.chrome_applied {
            self.chrome_applied = true;
            window_chrome::apply(frame);
        }
        let ctx = ui.ctx().clone();
        self.frames = self.frames.saturating_add(1);
        if self.frames <= 3 {
            crate::startup::mark(&format!("frame {} begins", self.frames));
        }
        if self.start_fullscreen && self.frames == 2 {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
        }
        self.app.step(&ctx);
        self.app.paint(ui);
        self.app.finish_frame(&ctx);
    }

    fn on_exit(&mut self) {
        self.app.save_preferences();
        self.app.panels.exit();
    }
}

/// Where the preview writes its generated game art.
pub fn preview_dir() -> PathBuf {
    std::env::temp_dir().join(format!("anyps5-preview-{}", std::process::id()))
}

#[cfg(test)]
mod desktop_entry_tests {
    #[test]
    fn the_desktop_entry_matches_the_app_id() {
        let entry = include_str!("../../../../tools/linux/anyplay-launcher.desktop");
        let has = |line: String| entry.lines().any(|l| l == line);
        assert!(has(format!("StartupWMClass={}", super::APP_ID)));
        assert!(has(format!("Icon={}", super::APP_ID)));
        assert!(
            entry
                .lines()
                .any(|l| l.starts_with(&format!("Exec={} ", super::APP_ID)))
        );
    }
}

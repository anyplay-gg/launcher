//! The host of the panels: opens every panel where the original does, hands each its data, and
//! turns what the panel reports back into model changes, pipeline work and saved settings.
//!
//! | Overlay | Panel (module `screens`) | Opened from |
//! |---|---|---|
//! | `Settings` | `settings::SettingsPanel` | options button, `Ctrl+,` |
//! | `Library` | `library::LibraryGrid` | library button |
//! | `GameSettings` | `game_settings::GameSettingsPanel` | the "..." button of the hero |
//! | `ConfirmUninstall` | `game_settings::UninstallPanel` | "Uninstall" row of the game settings |
//! | `Saves`, `ConfirmRestore` | `saves::SavesPanel`, `saves::RestorePanel` | "Save Data" row of the game settings |
//! | `Controls`, `Advanced` | `controls::ControlsPanel`, `advanced::AdvancedPanel` | rows of the game settings |
//! | `Trophies` | `trophies::TrophyListPanel` | trophy chip beside Play |
//! | `Log` | `log::LogView` | "Open log" row of the game settings |
//!
//! The panels animate themselves; the host only keeps them in step with `Model::overlay`: a panel
//! opens when its overlay becomes the open one and closes (slides out) when another one takes over.
//! The four side panels share one frame, so changing between them swaps the content without a
//! slide. Commands the model routes here are queued and handed to the panel in `ui`; panels that
//! edit settings get a copy, and the host stores it back and marks the preferences for saving when
//! the panel reports `changed`. Work with I/O (reports, uninstall, toolkit) runs on worker threads
//! and answers through channels polled in [`OverlayHost::frame`].

mod compare;
mod friends;
mod profile;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use anyps5_core::community::{self, CommunityLayout};
use anyps5_core::compat;
use anyps5_core::diagnosis::{self, Diagnosis};
use anyps5_core::gamefiles::GameInput;
use anyps5_core::issue;
use anyps5_core::library::Game;
use anyps5_core::linkreg::{self, Handler as LinkHandler};
use anyps5_core::paths::{AppPaths, GameId};
use anyps5_core::recommended::{self, Catalog, Kind, Plan};
use anyps5_core::report::{self, ReportInput, SystemInfo};
use anyps5_core::saves::{self, Backup, Reason, SaveData, SavesError};
use anyps5_core::settings::{GameSettings, ToolkitChannel};
use anyps5_core::toolkit::{self, GameToolkit};
use anyps5_core::unlock;
use egui::{Context, Ui, ViewportCommand};

use crate::app::clock;
use crate::app::links;
use crate::app::model::UpdateRequest;
use crate::app::model::{Effect, GameState, Model};
use crate::app::overlay::{Overlay, OverlayCx, OverlayHost};
use crate::app::selfupdate::{Restart, SelfUpdateService};
use crate::app::toolkit::{NotesState, ToolkitEvent, ToolkitService};
use crate::screens::advanced::{AdvancedAction, AdvancedData, AdvancedPanel};
use crate::screens::banner::{BannerHost, BannerTrophy};
use crate::screens::controls::{ControlsAction, ControlsData, ControlsPanel};
use crate::screens::game_settings::{
    GameSettingsAction, GameSettingsData, GameSettingsPanel, RecommendedRow, UninstallAction,
    UninstallPanel,
};
use crate::screens::home::tile_overlay;
use crate::screens::ingame::{CloseGamePanel, InGameAction, InGameData, InGamePanel};
use crate::screens::library::{LibraryAction, LibraryData, LibraryEntry, LibraryGrid, StateText};
use crate::screens::log::{LogAction, LogData, LogView, ReviewData};
use crate::screens::recommended::{RecommendedAction, RecommendedData, RecommendedPanel};
use crate::screens::rows::RowId;
use crate::screens::saves::{
    self as saves_screen, RestoreAction, RestorePanel, SavesAction, SavesData, SavesPanel,
};
use crate::screens::settings::{Category, SettingsAction, SettingsEnv, SettingsPanel};
use crate::screens::share_strings::Share;
use crate::screens::strings::Extra;
use crate::screens::toolkit_pin::{MissingAction, MissingData, MissingPanel, Pin, PinChoices};
use crate::screens::trophies::{TrophyAction, TrophyData, TrophyListPanel};
use crate::screens::whatsnew::{NotesView, WhatsNewAction, WhatsNewData, WhatsNewPanel};
use crate::screens::{ToastRequest, state_label};
use crate::ui::chrome::TileImage;
use crate::ui::focus::HomeFocus;
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};

/// The log view keeps this much of the end of a game's log file (DESIGN.md 5.12).
const LOG_TAIL_BYTES: u64 = 60_000;
/// Size of the icons in the library grid.
const LIBRARY_ICON: u32 = 256;

/// What a worker thread of the host reports.
enum Work {
    Report(Result<PathBuf, String>),
    Uninstalled {
        game: GameId,
        result: Result<(), String>,
    },
    /// What a game's files say about its controls.
    GameInput {
        game: GameId,
        input: GameInput,
    },
    /// The report to AnyPS5 of a game, ready to be shown.
    Upstream(issue::Prepared),
    /// The layouts the community shares for a title (none when offline).
    Community {
        title_id: String,
        layouts: Vec<CommunityLayout>,
    },
    /// The recommended settings, as shipped or newer from the network.
    Catalog(Box<Catalog>),
    /// A backup of a game's saves was made (or could not be).
    BackedUp(Result<Backup, SavesError>),
    /// A backup was put back; the backup of what it replaced comes with it.
    Restored(Result<Option<Backup>, SavesError>),
}

/// What the game sheet and the save panels know of the selected game's backups, read when one of
/// them opens and after every backup or restore.
struct SaveView {
    game: GameId,
    /// Newest first.
    backups: Vec<Backup>,
    has_saves: bool,
}

pub struct PanelHost {
    settings: SettingsPanel,
    game_settings: GameSettingsPanel,
    uninstall: UninstallPanel,
    /// The question when the release a game stays on is gone.
    toolkit_missing: MissingPanel,
    /// The quick panel of the running game and the question before it is closed.
    in_game: InGamePanel,
    close_game: CloseGamePanel,
    /// The tags of the releases on this PC, read when the game sheet opens and after a change.
    pin_installed: Option<Vec<String>>,
    controls: ControlsPanel,
    advanced: AdvancedPanel,
    trophies: TrophyListPanel,
    library: LibraryGrid,
    log: LogView,
    recommended: RecommendedPanel,
    saves: SavesPanel,
    friends: crate::screens::friends::FriendsScreen,
    /// The profile screen (`Overlay::Profile`).
    profile: crate::screens::profile::ProfileScreen,
    /// The trophy comparison (`Overlay::Compare`).
    compare: crate::screens::compare::CompareScreen,
    /// The page the Friends screen opens on when the profile screen sent the player there: the
    /// page, and a feed item to put the focus on.
    online_start: Option<(crate::screens::online::Page, Option<u64>)>,
    restore: RestorePanel,
    save_view: Option<SaveView>,
    saves_stale: bool,
    /// The backup the question before a restore is about, and its place in the list.
    pending_restore: Option<(usize, Backup)>,
    /// Its date, kept while the question slides out.
    restore_stamp: String,
    /// The backup to land on when the list opens again after the question.
    saves_return: Option<usize>,
    whats_new: WhatsNewPanel,
    /// The release the notes panel is about, without a leading `v`.
    whats_new_tag: String,
    /// The settings category to open on when coming back from the notes.
    settings_return: Option<usize>,
    /// How many clips the folder held when it was last counted, and when.
    clips_counted: Option<(std::time::Instant, usize)>,
    /// What the catalogue recommends: the shipped one at first, a newer one once fetched.
    catalog: Catalog,
    catalog_asked: bool,
    last_check: Option<std::time::Instant>,
    banners: BannerHost,
    toolkit: ToolkitService,
    /// Commands and raw keys routed here by the model, in order.
    events: Vec<InputEvent>,
    /// The game settings row to open on when coming back from the controls or advanced page.
    return_row: Option<RowId>,
    log_text: String,
    log_diagnosis: Option<Diagnosis>,
    /// The report to AnyPS5 the log view is reviewing; `None` while it shows the log.
    upstream: Option<issue::Prepared>,
    /// Community layouts by title id; a title is looked up once per run. Never filled in the
    /// demo and the tests.
    community: HashMap<String, Vec<CommunityLayout>>,
    online: bool,
    /// Resolutions of a title's patch by title id (reading the patch file once per title).
    resolutions: HashMap<String, Vec<String>>,
    /// The controls the game's files name, by game; `None` while they are being read.
    game_inputs: HashMap<GameId, Option<GameInput>>,
    work_tx: Sender<Work>,
    work_rx: Receiver<Work>,
    ctx: Context,
    checked_at_start: bool,
    /// The launcher's own update: checked at the start and every few hours, swapped in when it
    /// closes or on "Restart Now".
    launcher: SelfUpdateService,
    /// The notes of the staged update as the "What's New" panel shows them, while it shows them.
    launcher_published: Option<anyps5_core::releases::Published>,
    /// The first-start download of the toolkit was tried (once per session; the settings retry).
    toolkit_auto_tried: bool,
    /// Who answers `anyps5://` links, read when the settings open and after a change.
    links: LinkHandler,
    /// The demo and the tests keep the registration in memory and never touch the registry.
    simulated: bool,
}

impl PanelHost {
    /// A host that shows the launcher's update in `state` and never checks (the previews).
    pub fn showing_launcher_update(
        mut self,
        ctx: &Context,
        state: crate::screens::settings::LauncherUpdate,
    ) -> Self {
        self.launcher = SelfUpdateService::showing(ctx, state, None);
        self
    }

    /// `simulated`: the demo and the tests; the toolkit service then never goes online.
    pub fn new(ctx: &Context, simulated: bool) -> Self {
        let (work_tx, work_rx) = channel();
        Self {
            settings: SettingsPanel::default(),
            game_settings: GameSettingsPanel::default(),
            uninstall: UninstallPanel::default(),
            toolkit_missing: MissingPanel::default(),
            in_game: InGamePanel::default(),
            close_game: CloseGamePanel::default(),
            pin_installed: None,
            controls: ControlsPanel::default(),
            advanced: AdvancedPanel::default(),
            trophies: TrophyListPanel::default(),
            library: LibraryGrid::default(),
            log: LogView::default(),
            recommended: RecommendedPanel::default(),
            saves: SavesPanel::default(),
            friends: crate::screens::friends::FriendsScreen::default(),
            profile: crate::screens::profile::ProfileScreen::default(),
            compare: crate::screens::compare::CompareScreen::default(),
            online_start: None,
            restore: RestorePanel::default(),
            save_view: None,
            saves_stale: true,
            pending_restore: None,
            restore_stamp: String::new(),
            saves_return: None,
            whats_new: WhatsNewPanel::default(),
            whats_new_tag: String::new(),
            settings_return: None,
            clips_counted: None,
            catalog: Catalog::embedded(),
            catalog_asked: simulated,
            last_check: None,
            banners: BannerHost::default(),
            toolkit: ToolkitService::new(ctx, simulated),
            events: Vec::new(),
            return_row: None,
            log_text: String::new(),
            log_diagnosis: None,
            upstream: None,
            community: HashMap::new(),
            online: !simulated,
            resolutions: HashMap::new(),
            game_inputs: HashMap::new(),
            work_tx,
            work_rx,
            ctx: ctx.clone(),
            checked_at_start: simulated,
            launcher: SelfUpdateService::new(ctx, simulated),
            launcher_published: None,
            toolkit_auto_tried: simulated,
            links: LinkHandler::Absent,
            simulated,
        }
    }

    /// The events of this frame for the overlay that is open (and none for the others).
    fn take_events(&mut self, wanted: bool) -> Vec<InputEvent> {
        if wanted {
            std::mem::take(&mut self.events)
        } else {
            Vec::new()
        }
    }

    fn resolutions_for(&mut self, game: &Game, patches: Option<&std::path::Path>) -> Vec<String> {
        // A game on a toolkit of its own has that toolkit's patches.
        let key = format!(
            "{}|{}",
            game.title_id,
            patches.map_or_else(String::new, |dir| dir.display().to_string())
        );
        self.resolutions
            .entry(key)
            .or_insert_with(|| {
                patches
                    .map(|dir| anyps5_core::patches::resolution_choices(&game.title_id, dir))
                    .unwrap_or_default()
            })
            .clone()
    }

    // --- settings --------------------------------------------------------------------------------

    fn settings_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        if wanted && !self.settings.is_open() {
            self.refresh_links();
            if let Some(page) = cx.model.settings_page.take() {
                self.settings
                    .open_at(Category::from_link(page) as usize, false);
            } else {
                match self.settings_return.take() {
                    Some(category) => self.settings.open_at(category, true),
                    None => self.settings.open(),
                }
            }
        } else if !wanted && self.settings.is_open() {
            self.settings.close();
        }
        let events = self.take_events(wanted);
        let toolkit_version = cx.model.toolkit.clone();
        let default_screenshots = cx.model.paths.default_screenshots();
        if wanted {
            self.toolkit.load_installed(&cx.model.paths);
        }
        let in_use = Self::toolkits_in_use(cx.model);
        let unused: Vec<toolkit::InstalledRelease> = self
            .toolkit
            .installed
            .iter()
            .flatten()
            .filter(|release| !in_use.contains(&release.tag))
            .cloned()
            .collect();
        // The folder is read at most every two seconds, not on every frame.
        let clips_count = match self.clips_counted {
            Some((at, count)) if at.elapsed().as_secs() < 2 || !wanted => count,
            _ => {
                let folder = cx.model.prefs.screenshots_dir(&cx.model.paths);
                let count = anyps5_core::clips::list_clips(&folder).len();
                self.clips_counted = Some((std::time::Instant::now(), count));
                count
            }
        };
        let env = SettingsEnv {
            games_found: cx.model.games.len(),
            app_version: env!("CARGO_PKG_VERSION"),
            toolkit_version: toolkit_version.as_deref(),
            glibc: cx.model.glibc,
            update: self.toolkit.state.clone(),
            recent: &self.toolkit.recent,
            unused: &unused,
            launcher: self.launcher.state.clone(),
            launcher_notes: self.launcher.notes().is_some(),
            default_screenshots: &default_screenshots,
            links: self.links.clone(),
            cjk_fonts: anyps5_core::fonts::cjk_state(&cx.model.paths, &crate::fonts::BUNDLE),
            phone: &cx.model.phone,
            clips: crate::screens::settings::ClipsEnv {
                count: clips_count,
                capturing: self.settings.clips_capturing(),
            },
        };
        // Theme, accent, language, motion and sounds reach the shell inside the panel
        // (`apply_preferences`), so the backdrop cross-fades while the choice is made.
        let out = self
            .settings
            .show(cx.shell, cx.ctx, &events, &mut cx.model.prefs, &env);
        if out.changed {
            cx.model.mark_prefs_dirty();
        }
        for action in out.actions {
            self.settings_action(cx, action);
        }
    }

    /// The releases that must stay installed: the one in use, the one the pinned channel stays on
    /// and the ones games stay on.
    fn toolkits_in_use(model: &Model) -> Vec<String> {
        let mut tags = model.prefs.kept_toolkit_tags();
        tags.extend(model.toolkit.as_deref().and_then(compat::release_tag));
        tags
    }

    fn settings_action(&mut self, cx: &mut OverlayCx<'_>, action: SettingsAction) {
        let model = &mut *cx.model;
        match action {
            SettingsAction::Close => model.dismiss_overlay(),
            SettingsAction::GamesFolderChosen(_) => {
                model.selected = 0;
                model.rescan();
            }
            SettingsAction::Rescan => model.rescan(),
            SettingsAction::ShowScreenshots => {
                let folder = model.prefs.screenshots_dir(&model.paths);
                let _ = std::fs::create_dir_all(&folder);
                open_folder(&folder);
            }
            SettingsAction::ShowClips => {
                let folder = model.prefs.screenshots_dir(&model.paths);
                let _ = std::fs::create_dir_all(&folder);
                open_folder(&folder);
            }
            SettingsAction::CheckForUpdates => self.check_toolkit(model),
            SettingsAction::InstallToolkit => {
                // The row of the settings shows the download; the toast only confirms the start.
                let pinned = model.prefs.toolkit_pinned.clone();
                if self.toolkit.install(
                    &model.paths,
                    &model.prefs.toolkit_repository,
                    pinned.as_deref(),
                    model.prefs.kept_toolkit_tags(),
                ) {
                    let text = Extra::ToolkitDownloading.text(model.language());
                    model.toast(text, Icon::DOWNLOAD_SIMPLE);
                }
            }
            SettingsAction::DownloadCjkFonts => {
                let repaint = cx.ctx.clone();
                anyps5_core::fonts::download_cjk_in_background(
                    &model.paths,
                    crate::fonts::BUNDLE,
                    false,
                    Some(std::sync::Arc::new(move || repaint.request_repaint())),
                );
                cx.ctx.request_repaint();
            }
            SettingsAction::AdoptToolkitFolder(folder) => self.toolkit.adopt(folder),
            SettingsAction::UseStableToolkit => {
                self.resolutions.clear();
                model.effects.push(Effect::ReloadToolkit);
                self.last_check = None;
                self.check_toolkit(model);
            }
            SettingsAction::PinToolkit(tag) => {
                // Installed already: switch to it. Otherwise fetch it first.
                let installed = model
                    .paths
                    .toolkit_dir()
                    .join(tag.trim_start_matches('v'))
                    .is_dir();
                if installed {
                    self.resolutions.clear();
                    model.effects.push(Effect::ReloadToolkit);
                } else if self.toolkit.install(
                    &model.paths,
                    &model.prefs.toolkit_repository,
                    Some(&tag),
                    model.prefs.kept_toolkit_tags(),
                ) {
                    let text = Extra::ToolkitDownloading.text(model.language());
                    model.toast(text, Icon::DOWNLOAD_SIMPLE);
                }
            }
            SettingsAction::RemoveToolkit(tag) => {
                let in_use = Self::toolkits_in_use(model);
                self.toolkit.remove(&model.paths, &tag, in_use);
            }
            SettingsAction::OpenWhatsNew(tag) => {
                self.launcher_published = None;
                self.toolkit.load_notes(
                    &model.paths,
                    &model.prefs.toolkit_repository,
                    &format!("v{tag}"),
                );
                self.whats_new_tag = tag;
                self.settings_return = Category::ALL.iter().position(|c| *c == Category::Toolkit);
                model.switch_overlay(Overlay::WhatsNew);
            }
            SettingsAction::RestartNow => self.restart_launcher(model, cx.ctx),
            SettingsAction::CancelRestart => self.launcher.cancel_hold(),
            SettingsAction::CheckLauncherUpdate => self.launcher.check_now(),
            SettingsAction::OpenLauncherNotes => self.open_launcher_notes(model),
            SettingsAction::SetLinks(on) => self.set_links(model, on),
            SettingsAction::OpenPage(page) => open_known_url(page),
            SettingsAction::Phone(crate::screens::phone::PhoneAction::OpenDriverPage) => {
                open_known_url(crate::screens::phone::driver_page());
            }
            SettingsAction::Phone(action) => model.phone.commands.push(action),
            SettingsAction::Quit => cx.ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    /// Reads who answers the links from the registry.
    fn refresh_links(&mut self) {
        if !self.simulated {
            self.links = std::env::current_exe()
                .map(|exe| linkreg::status(&links::scheme(), &exe))
                .unwrap_or(LinkHandler::Absent);
        }
    }

    /// Writes or removes the per-user registration of the scheme (and, for the real scheme, of the
    /// folder entry), then reads it back so the row shows what is true.
    fn set_links(&mut self, model: &mut Model, on: bool) {
        if self.simulated {
            self.links = if on {
                LinkHandler::ThisExe
            } else {
                LinkHandler::Absent
            };
            return;
        }
        let scheme = links::scheme();
        let folder_menu = scheme == anyps5_core::links::SCHEME;
        let result = match std::env::current_exe() {
            Ok(exe) if on => linkreg::register(&scheme, &exe, folder_menu),
            Ok(_) => linkreg::unregister(&scheme, folder_menu),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            tracing::warn!("cannot change the link registration: {error}");
            let text = Extra::LinksFailed.text(model.language());
            model.toast(text, Icon::WARNING);
        }
        self.refresh_links();
    }

    /// Asks for the newest release. Only the stable channel follows it: a pinned release or a
    /// local build is left alone.
    fn check_toolkit(&mut self, model: &Model) {
        if model.prefs.toolkit_channel() != ToolkitChannel::Stable {
            return;
        }
        self.last_check = Some(std::time::Instant::now());
        self.toolkit
            .check(&model.paths, &model.prefs.toolkit_repository);
    }

    // --- game settings and its pages -----------------------------------------------------------

    fn game_settings_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let side_next = cx.model.overlay.is_some_and(Overlay::is_side_panel);
        let Some(game) = cx.model.selected_game().cloned() else {
            self.game_settings.dismiss();
            if wanted {
                cx.model.dismiss_overlay();
            }
            return;
        };
        let id = game.id.clone();
        if wanted && !self.game_settings.is_open() {
            self.saves_stale = true;
            self.pin_installed = None;
            self.toolkit
                .load_recent(&cx.model.paths, &cx.model.prefs.toolkit_repository);
        }
        self.refresh_saves(cx.model, &game);
        let mut settings = cx.model.prefs.game_settings(&id);
        let patches = cx.model.patches_dir_for(&id);
        let resolutions = self.resolutions_for(&game, patches.as_deref());
        let pin = PinChoices {
            installed: self
                .pin_installed
                .get_or_insert_with(|| toolkit::installed_tags(&cx.model.paths))
                .clone(),
            recent: self
                .toolkit
                .recent
                .iter()
                .filter_map(|tag| toolkit::pin_tag(tag))
                .collect(),
            recommended: self.toolkit_advice(cx.model, &game),
        };
        let installed = matches!(cx.model.state_of(&game), GameState::Installed { .. });
        let language = cx.model.language();
        let accent = cx.shell.accent_color();
        let plan = self.recommended_plan(cx.model, &game, &settings);
        let recommended = (!plan.is_empty()).then(|| RecommendedRow {
            changes: plan.changes.len(),
            needed: plan.changes.iter().any(|c| c.kind == Kind::Required),
        });
        let unlock_note = self.unlock_note(cx.model, &game);
        let mut data = GameSettingsData {
            title: &game.name,
            settings: &mut settings,
            resolutions: &resolutions,
            installed,
            saves: installed
                .then(|| self.save_view.as_ref().map(|v| v.backups.len()))
                .flatten(),
            recommended,
            unlock_note,
            pin: &pin,
            language,
            accent,
        };
        if wanted && !self.game_settings.is_open() {
            match self.return_row.take() {
                Some(row) => self.game_settings.open_on(&data, row),
                None => self.game_settings.open(),
            }
        } else if !wanted && self.game_settings.is_open() {
            if side_next {
                self.game_settings.dismiss();
            } else {
                self.game_settings.close();
            }
        }
        let out = self
            .game_settings
            .show(cx.shell, cx.ctx, &events, &mut data);
        if out.changed {
            store_game_settings(cx.model, &id, settings);
        }
        for action in out.actions {
            self.game_settings_action(cx, &game, action);
        }
    }

    fn game_settings_action(
        &mut self,
        cx: &mut OverlayCx<'_>,
        game: &Game,
        action: GameSettingsAction,
    ) {
        let model = &mut *cx.model;
        match action {
            GameSettingsAction::Close => model.dismiss_overlay(),
            GameSettingsAction::OpenControls => model.switch_overlay(Overlay::Controls),
            GameSettingsAction::OpenAdvanced => model.switch_overlay(Overlay::Advanced),
            GameSettingsAction::OpenRecommended => model.switch_overlay(Overlay::Recommended),
            GameSettingsAction::AskUninstall => model.switch_overlay(Overlay::ConfirmUninstall),
            GameSettingsAction::OpenSaves => {
                self.saves_stale = true;
                model.switch_overlay(Overlay::Saves);
            }
            GameSettingsAction::Reinstall => {
                model.dismiss_overlay();
                model.unlock_again(&game.id);
            }
            GameSettingsAction::ShowInExplorer => {
                let target = model
                    .prefs
                    .games_folder
                    .as_deref()
                    .map(|root| {
                        anyps5_core::paths::unlocked_copy_dir(root, &game.title_id, &game.id)
                    })
                    .filter(|copy| copy.exists())
                    .unwrap_or_else(|| game.folder.clone());
                if let Err(error) = report::reveal_in_explorer(&target) {
                    tracing::warn!("cannot show {} in Explorer: {error}", target.display());
                }
            }
            GameSettingsAction::OpenLog => {
                self.prepare_log(model, game);
                model.switch_overlay(Overlay::Log);
            }
            GameSettingsAction::ReportToUpstream => self.prepare_upstream(model, game),
            GameSettingsAction::ClearShaderCache => {
                let cache = model.paths.game_cache_dir(&game.title_id);
                let _ = anyps5_core::fsx::remove_tree(&cache);
                let text = model.language().text(Key::CacheCleared);
                model.toast(text, Icon::SPARKLE);
            }
        }
    }

    fn uninstall_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        if wanted && !self.uninstall.is_active() {
            self.uninstall.open();
        } else if !wanted && self.uninstall.is_active() {
            if cx.model.overlay.is_some_and(Overlay::is_side_panel) {
                self.uninstall.dismiss();
            } else {
                self.uninstall.close();
            }
        }
        let events = self.take_events(wanted);
        let (language, accent) = (cx.model.language(), cx.shell.accent_color());
        for action in self
            .uninstall
            .show(cx.shell, cx.ctx, &events, language, accent)
        {
            let model = &mut *cx.model;
            match action {
                // The original closes everything after either answer.
                UninstallAction::Cancel => model.dismiss_overlay(),
                UninstallAction::Confirm => {
                    model.dismiss_overlay();
                    if let (Some(game), Some(root)) = (
                        model.selected_game().cloned(),
                        model.prefs.games_folder.clone(),
                    ) {
                        let backups = model.paths.game_backups_dir(&game.title_id);
                        self.start_uninstall(&game, root, backups, model.language());
                    }
                }
            }
        }
    }

    /// The quick panel of the running game (docs/DESIGN.md 6.6). It ends when the game does.
    fn in_game_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let Some(game) = cx
            .model
            .running
            .clone()
            .and_then(|id| cx.model.game(&id).cloned())
        else {
            self.in_game.close();
            if wanted {
                cx.model.leave_in_game();
            }
            return;
        };
        if wanted && !self.in_game.is_active() {
            self.in_game.open();
        } else if !wanted && self.in_game.is_active() {
            if cx.model.overlay.is_some_and(Overlay::is_side_panel) {
                self.in_game.dismiss();
            } else {
                self.in_game.close();
            }
        }
        let data = InGameData {
            title: &game.name,
            language: cx.model.language(),
            accent: cx.shell.accent_color(),
            online: cx.model.online.is_signed_in(),
        };
        for action in self.in_game.show(cx.shell, cx.ctx, &events, &data) {
            let model = &mut *cx.model;
            match action {
                InGameAction::Resume | InGameAction::Dismiss => model.return_to_game(),
                InGameAction::Trophies => model.open(Overlay::Trophies),
                InGameAction::GameSettings => model.open(Overlay::GameSettings),
                InGameAction::Friends => model.open(Overlay::Friends),
                InGameAction::AskClose => model.switch_overlay(Overlay::ConfirmCloseGame),
                InGameAction::Close | InGameAction::Keep => {}
            }
        }
    }

    /// "Close Game" / "Keep Playing". Closing stops the game; the launcher stays in front.
    fn close_game_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let Some(game) = cx
            .model
            .running
            .clone()
            .and_then(|id| cx.model.game(&id).cloned())
        else {
            self.close_game.close();
            if wanted {
                cx.model.leave_in_game();
            }
            return;
        };
        if wanted && !self.close_game.is_active() {
            self.close_game.open();
        } else if !wanted && self.close_game.is_active() {
            if cx.model.overlay.is_some_and(Overlay::is_side_panel) {
                self.close_game.dismiss();
            } else {
                self.close_game.close();
            }
        }
        let (language, accent) = (cx.model.language(), cx.shell.accent_color());
        for action in self
            .close_game
            .show(cx.shell, cx.ctx, &events, &game.name, language, accent)
        {
            let model = &mut *cx.model;
            match action {
                InGameAction::Close => {
                    model.stop(&game.id);
                    model.leave_in_game();
                }
                InGameAction::Keep => model.switch_overlay(Overlay::InGame),
                _ => {}
            }
        }
    }

    /// The question about a game whose release is gone (docs/DESIGN.md 3.8). It ends when the
    /// release is back, or the game follows the global setting, or the player cancels.
    fn toolkit_missing_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let side_next = cx.model.overlay.is_some_and(Overlay::is_side_panel);
        let asked = cx
            .model
            .toolkit_question
            .clone()
            .and_then(|id| cx.model.game(&id).cloned());
        let tag = asked
            .as_ref()
            .and_then(|game| match cx.model.toolkit_of(&game.id) {
                GameToolkit::Missing { tag } => Some(tag),
                _ => None,
            });
        let (Some(game), Some(tag)) = (asked, tag) else {
            self.toolkit_missing.close();
            cx.model.toolkit_question = None;
            if wanted {
                cx.model.dismiss_overlay();
            }
            return;
        };
        if wanted && !self.toolkit_missing.is_active() {
            self.toolkit_missing.open();
        } else if !wanted && self.toolkit_missing.is_active() {
            if side_next {
                self.toolkit_missing.dismiss();
            } else {
                self.toolkit_missing.close();
            }
        }
        let data = MissingData {
            title: &game.name,
            downloading: self.toolkit.pin_installing.as_deref() == Some(tag.as_str()),
            tag: &tag,
            language: cx.model.language(),
            accent: cx.shell.accent_color(),
        };
        for action in self.toolkit_missing.show(cx.shell, cx.ctx, &events, &data) {
            let model = &mut *cx.model;
            match action {
                MissingAction::Download => {
                    if self.toolkit.install_pinned(
                        &model.paths,
                        &model.prefs.toolkit_repository,
                        &tag,
                        model.prefs.kept_toolkit_tags(),
                    ) {
                        let text = Extra::ToolkitDownloading.text(model.language());
                        model.toast(text, Icon::DOWNLOAD_SIMPLE);
                    }
                }
                MissingAction::Follow => {
                    let mut settings = model.prefs.game_settings(&game.id);
                    settings.toolkit = None;
                    store_game_settings(model, &game.id, settings);
                    model.toolkit_question = None;
                    model.dismiss_overlay();
                }
                MissingAction::Cancel => {
                    model.toolkit_question = None;
                    model.dismiss_overlay();
                }
            }
        }
    }

    // --- save data -----------------------------------------------------------------------------

    /// The save data of `game`: where its copy is and where its backups go.
    fn save_data(model: &Model, game: &Game) -> Option<SaveData> {
        let root = model.prefs.games_folder.as_deref()?;
        let copy = anyps5_core::paths::unlocked_copy_dir(root, &game.title_id, &game.id);
        Some(SaveData::for_title(&model.paths, &game.title_id, copy))
    }

    /// Reads the backups again when something changed or another game is selected.
    fn refresh_saves(&mut self, model: &Model, game: &Game) {
        let current = self.save_view.as_ref().is_some_and(|v| v.game == game.id);
        if current && !self.saves_stale {
            return;
        }
        self.saves_stale = false;
        self.save_view = Self::save_data(model, game).map(|data| SaveView {
            game: game.id.clone(),
            backups: data.list(),
            has_saves: data.exists(),
        });
    }

    fn saves_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let side_next = cx.model.overlay.is_some_and(Overlay::is_side_panel);
        let Some(game) = cx.model.selected_game().cloned() else {
            if wanted {
                cx.model.dismiss_overlay();
            }
            return;
        };
        if wanted && !self.saves.is_active() {
            self.saves_stale = true;
        }
        self.refresh_saves(cx.model, &game);
        let (language, accent) = (cx.model.language(), cx.shell.accent_color());
        let running = cx.model.is_active(&game.id);
        let (backups, has_saves) = self
            .save_view
            .as_ref()
            .map(|v| (v.backups.clone(), v.has_saves))
            .unwrap_or_default();
        let data = SavesData {
            title: &game.name,
            backups: &backups,
            has_saves,
            running,
            keep: saves::KEEP,
            language,
            accent,
        };
        if wanted && !self.saves.is_active() {
            match self.saves_return.take() {
                Some(index) => self.saves.open_backup(&data, index),
                None => self.saves.open(),
            }
        } else if !wanted && self.saves.is_active() {
            if side_next {
                self.saves.dismiss();
            } else {
                self.saves.close();
            }
        }
        for action in self.saves.show(cx.shell, cx.ctx, &events, &data) {
            match action {
                SavesAction::BackUp if !has_saves => {
                    cx.model
                        .toast(Extra::SavesNothing.text(language), Icon::INFO);
                }
                SavesAction::BackUp => self.start_backup(cx.model, &game),
                SavesAction::Ask(_) if running => {
                    cx.model
                        .toast(Extra::SavesRunning.text(language), Icon::WARNING);
                }
                SavesAction::Ask(index) => {
                    if let Some(backup) = backups.get(index) {
                        self.restore_stamp = saves_screen::stamp(backup);
                        self.pending_restore = Some((index, backup.clone()));
                        cx.model.switch_overlay(Overlay::ConfirmRestore);
                    }
                }
                SavesAction::Close => self.back_to_game_settings(cx.model, "saves"),
            }
        }
    }

    fn restore_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        if wanted && !self.restore.is_active() {
            self.restore.open();
        } else if !wanted && self.restore.is_active() {
            if cx.model.overlay.is_some_and(Overlay::is_side_panel) {
                self.restore.dismiss();
            } else {
                self.restore.close();
            }
        }
        let (language, accent) = (cx.model.language(), cx.shell.accent_color());
        let stamp = self.restore_stamp.clone();
        for action in self
            .restore
            .show(cx.shell, cx.ctx, &events, &stamp, language, accent)
        {
            let model = &mut *cx.model;
            let asked = self.pending_restore.take();
            self.saves_return = asked.as_ref().map(|(index, _)| *index);
            if let (RestoreAction::Confirm, Some((_, backup)), Some(game)) =
                (action, asked, model.selected_game().cloned())
            {
                self.start_restore(model, &game, backup);
            }
            // Either answer goes back to the list, where the result shows.
            model.switch_overlay(Overlay::Saves);
        }
    }

    /// Zips the saves on a worker thread.
    fn start_backup(&mut self, model: &mut Model, game: &Game) {
        if model.saves_busy.is_some() {
            let text = Extra::SavesBusy.text(model.language());
            model.toast(text, Icon::INFO);
            return;
        }
        let Some(data) = Self::save_data(model, game) else {
            return;
        };
        model.saves_busy = Some(game.id.clone());
        let (tx, ctx) = (self.work_tx.clone(), self.ctx.clone());
        spawn("backup", move || {
            let _ = tx.send(Work::BackedUp(data.backup(Reason::Manual)));
            ctx.request_repaint();
        });
    }

    /// Puts a backup back on a worker thread, unless the game is running.
    fn start_restore(&mut self, model: &mut Model, game: &Game, backup: Backup) {
        let language = model.language();
        if model.is_active(&game.id) {
            model.toast(Extra::SavesRunning.text(language), Icon::WARNING);
            return;
        }
        if model.saves_busy.is_some() {
            model.toast(Extra::SavesBusy.text(language), Icon::INFO);
            return;
        }
        let Some(data) = Self::save_data(model, game) else {
            return;
        };
        model.saves_busy = Some(game.id.clone());
        let (tx, ctx) = (self.work_tx.clone(), self.ctx.clone());
        spawn("restore", move || {
            // The model checked that the game is not running, and it does not start one while
            // `saves_busy` is set.
            let _ = tx.send(Work::Restored(data.restore(&backup, false)));
            ctx.request_repaint();
        });
    }

    fn controls_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let side_next = cx.model.overlay.is_some_and(Overlay::is_side_panel);
        if !wanted {
            if self.controls.is_active() {
                if side_next {
                    self.controls.dismiss(cx.shell);
                } else {
                    self.controls.close(cx.shell);
                }
            }
        } else if !self.controls.is_active() {
            self.controls.open();
        }
        let Some(game) = cx.model.selected_game().cloned() else {
            if wanted {
                cx.model.dismiss_overlay();
            }
            return;
        };
        if !self.game_inputs.contains_key(&game.id) {
            // Read once per game and run, off the frame: a pak index can take a moment.
            self.game_inputs.insert(game.id.clone(), None);
            let (tx, ctx) = (self.work_tx.clone(), self.ctx.clone());
            let (id, folder) = (game.id.clone(), game.folder.clone());
            spawn("game-input", move || {
                let input = anyps5_core::gamefiles::read_game_input(&folder);
                let _ = tx.send(Work::GameInput { game: id, input });
                ctx.request_repaint();
            });
        }
        if self.online && !self.community.contains_key(&game.title_id) {
            // The gallery of the title, once per run: two small files from GitHub, off the frame.
            self.community.insert(game.title_id.clone(), Vec::new());
            let (tx, ctx) = (self.work_tx.clone(), self.ctx.clone());
            let title_id = game.title_id.clone();
            spawn("community", move || {
                let layouts = community::fetch(&title_id);
                let _ = tx.send(Work::Community { title_id, layouts });
                ctx.request_repaint();
            });
        }
        let mut settings = cx.model.prefs.game_settings(&game.id);
        let shared = self
            .community
            .get(&game.title_id)
            .map_or(&[][..], Vec::as_slice);
        let mut data = ControlsData {
            title: &game.name,
            title_id: &game.title_id,
            settings: &mut settings,
            language: cx.model.language(),
            accent: cx.shell.accent_color(),
            game_input: self.game_inputs.get(&game.id).and_then(Option::as_ref),
            community: shared,
        };
        let out = self.controls.show(cx.shell, cx.ctx, &events, &mut data);
        if out.changed {
            store_game_settings(cx.model, &game.id, settings);
        }
        for action in out.actions {
            match action {
                ControlsAction::Back => self.back_to_game_settings(cx.model, "controls"),
                ControlsAction::Close => cx.model.dismiss_overlay(),
                ControlsAction::Toast(ToastRequest { icon, text }) => cx.model.toast(text, icon),
            }
        }
    }

    fn advanced_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let side_next = cx.model.overlay.is_some_and(Overlay::is_side_panel);
        if !wanted {
            if self.advanced.is_active() {
                if side_next {
                    self.advanced.dismiss();
                } else {
                    self.advanced.close();
                }
            }
        } else if !self.advanced.is_active() {
            self.advanced.open();
        }
        let Some(game) = cx.model.selected_game().cloned() else {
            if wanted {
                cx.model.dismiss_overlay();
            }
            return;
        };
        let mut settings = cx.model.prefs.game_settings(&game.id);
        let patches = cx.model.patches_dir_for(&game.id);
        let resolutions = self.resolutions_for(&game, patches.as_deref());
        let mut data = AdvancedData {
            game: &game,
            settings: &mut settings,
            resolutions: &resolutions,
            language: cx.model.language(),
            accent: cx.shell.accent_color(),
        };
        let out = self.advanced.show(cx.shell, cx.ctx, &events, &mut data);
        if out.changed {
            store_game_settings(cx.model, &game.id, settings);
        }
        for action in out.actions {
            match action {
                AdvancedAction::Back => self.back_to_game_settings(cx.model, "advanced"),
                AdvancedAction::Close => cx.model.dismiss_overlay(),
                AdvancedAction::Toast(ToastRequest { icon, text }) => cx.model.toast(text, icon),
            }
        }
    }

    /// Controls and advanced go back to the game settings on the row that opened them.
    fn back_to_game_settings(&mut self, model: &mut Model, row: RowId) {
        self.return_row = Some(row);
        model.switch_overlay(Overlay::GameSettings);
    }

    /// What the catalogue would change for `game` with its current settings.
    fn recommended_plan(&self, model: &Model, game: &Game, settings: &GameSettings) -> Plan {
        let entries = self.catalog.entries(&game.title_id);
        if entries.is_empty() {
            return Plan::default();
        }
        let toolkit = model
            .toolkit_of(&game.id)
            .version()
            .map(str::to_owned)
            .unwrap_or_default();
        let inherited = |key: &str| std::env::var(key).ok();
        let context = recommended::Context {
            title_id: &game.title_id,
            content_version: &game.version,
            toolkit: &toolkit,
            dump: &game.folder,
            inherited: &inherited,
        };
        recommended::plan(entries, &context, settings)
    }

    /// The release the catalogue recommends for the game (plain, `0.1.2`), when it has a view on
    /// it for the toolkit the game uses now.
    fn toolkit_advice(&self, model: &Model, game: &Game) -> Option<String> {
        let entries = self.catalog.entries(&game.title_id);
        if entries.is_empty() {
            return None;
        }
        let version = model
            .toolkit_of(&game.id)
            .version()
            .map(str::to_owned)
            .unwrap_or_default();
        let inherited = |key: &str| std::env::var(key).ok();
        let context = recommended::Context {
            title_id: &game.title_id,
            content_version: &game.version,
            toolkit: &version,
            dump: &game.folder,
            inherited: &inherited,
        };
        recommended::toolkit_advice(entries, &context)
            .and_then(|advice| toolkit::pin_tag(&advice.tag))
    }

    /// "Unlocked with v0.1.2. v0.1.3 is installed." and why unlocking again is worth it, for a
    /// copy made with another toolkit than the one in use.
    fn unlock_note(&mut self, model: &Model, game: &Game) -> Option<String> {
        let record = game.unlocked_with.as_deref()?;
        let toolkit = model.toolkit_of(&game.id);
        let current = toolkit.version()?;
        if record == current {
            return None;
        }
        let language = model.language();
        let name = |version: &str| {
            compat::release_tag(version).map_or_else(|| version.to_owned(), |t| format!("v{t}"))
        };
        let (from, to) = (compat::release_tag(record), compat::release_tag(current));
        let mut note = match (&from, &to) {
            (Some(a), Some(b)) if a == b => Extra::UnlockEarlierBuild.text(language).to_owned(),
            _ => Extra::UnlockedWith.format(
                language,
                &[Arg::Str(&name(record)), Arg::Str(&name(current))],
            ),
        };
        if let (Some(from), Some(to)) = (from, to)
            && compat::compare_versions(&from, &to).is_some_and(std::cmp::Ordering::is_lt)
        {
            self.toolkit
                .ask_conversion(&model.paths, &model.prefs.toolkit_repository, &from, &to);
            match self.toolkit.conversion_changed(&from, &to) {
                Some(true) => note = format!("{note} {}", Extra::ConversionChanged.text(language)),
                Some(false) => note = format!("{note} {}", Extra::UnlockOptional.text(language)),
                None => {}
            }
        }
        Some(note)
    }

    fn recommended_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let side_next = cx.model.overlay.is_some_and(Overlay::is_side_panel);
        let Some(game) = cx.model.selected_game().cloned() else {
            if wanted {
                cx.model.dismiss_overlay();
            }
            return;
        };
        let settings = cx.model.prefs.game_settings(&game.id);
        let plan = self.recommended_plan(cx.model, &game, &settings);
        let (language, accent) = (cx.model.language(), cx.shell.accent_color());
        if wanted && !self.recommended.is_active() {
            self.recommended.open(&plan, language);
        } else if !wanted && self.recommended.is_active() {
            if side_next {
                self.recommended.dismiss();
            } else {
                self.recommended.close();
            }
        }
        let data = RecommendedData {
            title: &game.name,
            plan: &plan,
            language,
            accent,
        };
        for action in self.recommended.show(cx.shell, cx.ctx, &events, &data) {
            if action == RecommendedAction::Apply {
                let mut applied = settings.clone();
                plan.apply(&mut applied);
                store_game_settings(cx.model, &game.id, applied);
                let text = if plan.needs_unlock() {
                    Extra::RecAppliedUnlock
                } else {
                    Extra::RecApplied
                };
                cx.model.toast(text.text(language), Icon::CHECK_CIRCLE);
            }
            self.back_to_game_settings(cx.model, "recommended");
        }
    }

    fn whats_new_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let language = cx.model.language();
        if wanted && !self.whats_new.is_active() {
            match &self.launcher_published {
                Some(published) => self.whats_new.open_launcher(&published.notes, language),
                None => self.whats_new.open(),
            }
        } else if !wanted && self.whats_new.is_active() {
            self.whats_new.close();
        }
        let launcher = self.launcher_published.is_some();
        let view = match (&self.launcher_published, &self.toolkit.notes) {
            (Some(published), _) => NotesView::Ready(published),
            (None, NotesState::Ready(published)) => NotesView::Ready(published),
            (None, NotesState::Unavailable) => NotesView::Unavailable,
            (None, NotesState::Idle | NotesState::Loading) => NotesView::Loading,
        };
        let tag = match &self.launcher_published {
            Some(published) => published.tag.as_str(),
            None => self.whats_new_tag.as_str(),
        };
        let data = WhatsNewData {
            tag,
            view,
            language,
            launcher,
        };
        let actions = self.whats_new.show(cx.shell, cx.ctx, &events, &data);
        for action in actions {
            match action {
                WhatsNewAction::Close => cx.model.switch_overlay(Overlay::Settings),
                WhatsNewAction::OpenUrl(url) => open_url(&url),
            }
        }
    }

    // --- full-screen panels --------------------------------------------------------------------

    fn trophies_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let game = cx.model.selected_game().cloned();
        let set = game.as_ref().and_then(|g| cx.model.trophies.get(&g.id));
        let (Some(game), Some(set)) = (game, set) else {
            // Nothing to list: never open, and leave a panel that is already open.
            if wanted {
                cx.model.dismiss_overlay();
            }
            if self.trophies.is_active() {
                self.trophies.close();
            }
            return;
        };
        if wanted && !self.trophies.is_active() {
            self.trophies.open();
        } else if !wanted && self.trophies.is_active() {
            self.trophies.close();
        }
        let data = TrophyData {
            game_key: game.id.as_str(),
            game_name: &game.name,
            set,
            language: cx.model.language(),
            accent: cx.shell.accent_color(),
            show_hidden: cx.model.prefs.show_hidden_trophies,
        };
        let actions = self.trophies.show(cx.shell, cx.ctx, &events, &data);
        // The player read a secret: everyone who may see their trophies may, too. Only the first
        // set is the network's.
        let title_id = game.title_id.clone();
        let revealed: Vec<u32> = actions
            .iter()
            .filter_map(|action| match action {
                TrophyAction::Revealed(index) => set.trophies.get(*index),
                TrophyAction::Close => None,
            })
            .filter(|t| t.service_label == 0)
            .map(|t| t.number)
            .collect();
        for number in revealed {
            cx.model.online.reveal_secret(&title_id, number);
        }
        if actions.contains(&TrophyAction::Close) {
            cx.model.dismiss_overlay();
        }
    }

    fn library_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        if wanted && !self.library.is_active() {
            self.library.open(cx.model.selected);
        } else if !wanted && self.library.is_active() {
            self.library.close();
        }
        let events = self.take_events(wanted);
        let language = cx.model.language();
        let accent = cx.shell.accent_color();
        let entries: Vec<LibraryEntry> = cx
            .model
            .games
            .iter()
            .map(|game| {
                let state = cx.model.state_of(game);
                let label = state_label(&state, language);
                let icon = game.icon.as_ref().and_then(|path| {
                    cx.textures.get(path, LIBRARY_ICON).map(|loaded| TileImage {
                        texture: loaded.id,
                        aspect: loaded.aspect,
                    })
                });
                LibraryEntry {
                    name: game.name.clone(),
                    icon,
                    overlay: tile_overlay(&state),
                    state: StateText {
                        icon: label.icon,
                        text: label.text,
                        tint: label.tint,
                    },
                }
            })
            .collect();
        let data = LibraryData {
            entries: &entries,
            language,
            accent,
        };
        for action in self.library.show(cx.shell, cx.ctx, &events, &data) {
            let model = &mut *cx.model;
            match action {
                LibraryAction::Pick(index) => {
                    if index < model.games.len() {
                        model.selected = index;
                        model.focus = HomeFocus::Tiles;
                        model.measure_selected();
                    }
                    model.dismiss_overlay();
                }
                LibraryAction::Close => model.dismiss_overlay(),
            }
        }
    }

    fn log_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        if wanted && !self.log.is_active() {
            self.log.open();
        } else if !wanted && self.log.is_active() {
            self.log.close();
        }
        let events = self.take_events(wanted);
        let game = cx.model.selected_game().cloned();
        let language = cx.model.language();
        let hint = match &self.upstream {
            Some(prepared) if prepared.clipboard.is_some() => format!(
                "{}\n{}",
                Share::ReportHint.text(language),
                Share::ReportPasteHint.text(language)
            ),
            _ => Share::ReportHint.text(language).to_owned(),
        };
        let data = LogData {
            game_name: game.as_ref().map(|g| g.name.as_str()),
            diagnosis: self.log_diagnosis.as_ref(),
            text: self
                .upstream
                .as_ref()
                .map_or(&self.log_text, |p| &p.preview),
            language,
            review: self.upstream.as_ref().map(|_| ReviewData { hint: &hint }),
        };
        let actions = self.log.show(cx.shell, cx.ctx, &events, &data);
        for action in actions {
            match action {
                LogAction::Close => {
                    self.upstream = None;
                    cx.model.dismiss_overlay();
                }
                LogAction::OpenReport => {
                    if let Some(prepared) = self.upstream.take() {
                        open_upstream(cx.ctx, cx.model, &prepared);
                    }
                    cx.model.dismiss_overlay();
                }
                LogAction::MakeReport => {
                    if let Some(game) = &game {
                        self.prepare_upstream(cx.model, game);
                    }
                }
                LogAction::SaveReport => {
                    if let Some(game) = &game {
                        self.make_report(cx.model, game);
                    }
                }
            }
        }
    }

    // --- log and report ------------------------------------------------------------------------

    /// The text of the log view (DESIGN.md 5.12): the failure message, then the end of the log file.
    fn prepare_log(&mut self, model: &Model, game: &Game) {
        self.upstream = None;
        let mut text = String::new();
        if let Some(failure) = model.failures.get(&game.id) {
            text.push_str(failure);
            text.push_str("\n\n");
        }
        let file = model.paths.log_file(&game.title_id);
        if let Ok(bytes) = diagnosis::read_tail(&file, LOG_TAIL_BYTES) {
            text.push_str(&String::from_utf8_lossy(&bytes));
        }
        self.log_text = if text.trim().is_empty() {
            "—".to_owned()
        } else {
            text
        };
        // The sentence above the log only when the game ended on an error.
        self.log_diagnosis = model
            .diagnoses
            .contains_key(&game.id)
            .then(|| Diagnosis::read(&diagnosis::read_last_session(&file), None))
            .flatten();
    }

    /// Everything a report says about `game`; run it on a worker thread (it reads the PC).
    fn report_job(model: &Model, game: &Game) -> impl FnOnce() -> (AppPaths, ReportInput) + use<> {
        let paths: AppPaths = model.paths.clone();
        let settings =
            serde_json::to_value(model.prefs.game_settings(&game.id)).unwrap_or_default();
        let (name, title_id, version) = (
            game.name.clone(),
            game.title_id.clone(),
            game.version.clone(),
        );
        let toolkit_version = model.toolkit_of(&game.id).version().map(str::to_owned);
        move || {
            let session = diagnosis::read_last_session(&paths.log_file(&title_id));
            let input = ReportInput {
                game_name: name,
                title_id,
                version,
                system: SystemInfo::collect(),
                launcher_version: env!("CARGO_PKG_VERSION").to_owned(),
                toolkit_version,
                reason: Diagnosis::read(&session, None).map(|d| d.summary()),
                settings,
                crash: Diagnosis::crash_name(&session, None),
                session,
                now: clock::rfc3339_utc(clock::unix_seconds()),
            };
            (paths, input)
        }
    }

    /// Writes the report on a worker thread, then shows it in Explorer (DESIGN.md 5.13).
    fn make_report(&mut self, model: &Model, game: &Game) {
        let job = Self::report_job(model, game);
        let (tx, ctx) = (self.work_tx.clone(), self.ctx.clone());
        spawn("report", move || {
            let (paths, input) = job();
            let result = report::write_report(&paths, &input).map_err(|e| e.to_string());
            let _ = tx.send(Work::Report(result));
            ctx.request_repaint();
        });
    }

    /// Builds the issue for AnyPS5 on a worker thread; the log view then shows it (DESIGN.md 5.19).
    fn prepare_upstream(&mut self, model: &Model, game: &Game) {
        let job = Self::report_job(model, game);
        let (tx, ctx) = (self.work_tx.clone(), self.ctx.clone());
        spawn("report", move || {
            let (_, input) = job();
            let _ = tx.send(Work::Upstream(issue::prepare(&input)));
            ctx.request_repaint();
        });
    }

    /// Deletes the unlocked copy on a worker thread (a game can be tens of gigabytes). Its saves go
    /// with it, so they are backed up first, and when that fails nothing is removed.
    fn start_uninstall(
        &mut self,
        game: &Game,
        root: PathBuf,
        backups: PathBuf,
        language: Language,
    ) {
        let (tx, ctx) = (self.work_tx.clone(), self.ctx.clone());
        let (title_id, id) = (game.title_id.clone(), game.id.clone());
        spawn("uninstall", move || {
            let copy = anyps5_core::paths::unlocked_copy_dir(&root, &title_id, &id);
            match SaveData::new(copy, backups).backup(Reason::BeforeUninstall) {
                Ok(_) | Err(SavesError::NothingToSave) => {}
                Err(error) => {
                    tracing::warn!("cannot back up the saves before removing: {error}");
                    let result = Err(Extra::UninstallNeedsBackup.text(language).to_owned());
                    let _ = tx.send(Work::Uninstalled { game: id, result });
                    ctx.request_repaint();
                    return;
                }
            }
            let result = unlock::remove_unlocked(&title_id, &id, &root).map_err(|e| e.to_string());
            let _ = tx.send(Work::Uninstalled { game: id, result });
            ctx.request_repaint();
        });
    }

    // --- per frame -----------------------------------------------------------------------------

    fn poll_work(&mut self, model: &mut Model) {
        while let Ok(work) = self.work_rx.try_recv() {
            let language = model.language();
            match work {
                Work::Report(Ok(path)) => {
                    if let Err(error) = report::reveal_in_explorer(&path) {
                        tracing::warn!("cannot show the report: {error}");
                    }
                    model.toast(language.text(Key::ReportSaved), Icon::FILE_TEXT);
                }
                Work::Report(Err(error)) => {
                    tracing::warn!("cannot write the report: {error}");
                    model.toast(language.text(Key::ReportFailed), Icon::WARNING);
                }
                Work::GameInput { game, input } => {
                    self.game_inputs.insert(game, Some(input));
                }
                Work::Upstream(prepared) => {
                    // Only while the player is still in the game's settings or its log.
                    match model.overlay {
                        Some(Overlay::GameSettings) => {
                            self.upstream = Some(prepared);
                            model.switch_overlay(Overlay::Log);
                        }
                        Some(Overlay::Log) => self.upstream = Some(prepared),
                        _ => {}
                    }
                }
                Work::Community { title_id, layouts } => {
                    self.community.insert(title_id, layouts);
                }
                Work::Catalog(catalog) => self.catalog = *catalog,
                Work::BackedUp(result) => {
                    model.saves_busy = None;
                    self.saves_stale = true;
                    match result {
                        Ok(_) => {
                            model.toast(Extra::SavesBackedUp.text(language), Icon::CHECK_CIRCLE)
                        }
                        Err(error) => {
                            let text = saves_failure(&error, false, language);
                            model.toast(text, Icon::WARNING);
                        }
                    }
                }
                Work::Restored(result) => {
                    model.saves_busy = None;
                    self.saves_stale = true;
                    match result {
                        Ok(_) => {
                            // The trophies came back too: the list reads them again.
                            if let Some(game) = model.selected_game().map(|g| g.id.clone()) {
                                model.reload_trophies(&game);
                            }
                            model.toast(Extra::SavesRestored.text(language), Icon::CHECK_CIRCLE);
                        }
                        Err(error) => {
                            let text = saves_failure(&error, true, language);
                            model.toast(text, Icon::WARNING);
                        }
                    }
                }
                Work::Uninstalled { game, result } => match result {
                    Ok(()) => {
                        if let Some(g) = model.games.iter_mut().find(|g| g.id == game) {
                            g.unlocked_with = None;
                        }
                        model.failures.remove(&game);
                        model.trophies.remove(&game);
                        model.toast(language.text(Key::UninstallDone), Icon::TRASH);
                    }
                    Err(error) => {
                        tracing::warn!("cannot uninstall: {error}");
                        model.toast(error, Icon::WARNING);
                    }
                },
            }
        }
    }

    fn poll_toolkit(&mut self, model: &mut Model) {
        for event in self.toolkit.poll() {
            let language = model.language();
            match event {
                ToolkitEvent::Checked(Ok(found)) => {
                    let stamp = clock::rfc3339_utc(clock::unix_seconds());
                    model.edit_prefs(|p| p.toolkit_checked_at = Some(stamp));
                    // Told, not installed: the settings offer the download.
                    if let Some(tag) = found.filter(|_| model.toolkit.is_some()) {
                        let text = Extra::ToolkitAvailable
                            .format(language, &[Arg::Str(&format!("v{tag}"))]);
                        model.toast(text, Icon::DOWNLOAD_SIMPLE);
                    }
                }
                ToolkitEvent::Checked(Err(error)) => tracing::warn!("update check: {error}"),
                ToolkitEvent::PinInstalled(Ok(version)) => {
                    // The global choice stays as it is; the game that was waiting finds its release.
                    self.resolutions.clear();
                    self.pin_installed = None;
                    let text = Extra::ToolkitReady.format(language, &[Arg::Str(&version)]);
                    model.toast(text, Icon::CHECK_CIRCLE);
                }
                ToolkitEvent::PinInstalled(Err(error)) => {
                    tracing::warn!("toolkit install for a game: {error}");
                    model.toast(Extra::ToolkitFailed.text(language), Icon::WARNING);
                }
                ToolkitEvent::Removed(Ok(tag)) => {
                    self.pin_installed = None;
                    let text = Pin::Removed.format(language, &[Arg::Str(&format!("v{tag}"))]);
                    model.toast(text, Icon::TRASH);
                }
                ToolkitEvent::Removed(Err(error)) => {
                    tracing::warn!("toolkit removal: {error}");
                    model.toast(Pin::RemoveFailed.text(language), Icon::WARNING);
                }
                ToolkitEvent::Installed(Ok(version)) => {
                    self.pin_installed = None;
                    self.resolutions.clear();
                    model.edit_prefs(|p| p.toolkit = None);
                    model.effects.push(Effect::ReloadToolkit);
                    let text = Extra::ToolkitReady.format(language, &[Arg::Str(&version)]);
                    model.toast(text, Icon::CHECK_CIRCLE);
                }
                ToolkitEvent::Adopted(Ok((folder, version))) => {
                    self.resolutions.clear();
                    model.edit_prefs(|p| {
                        p.toolkit = Some(folder);
                        p.toolkit_pinned = None;
                    });
                    model.effects.push(Effect::ReloadToolkit);
                    let text = Extra::ToolkitReady.format(language, &[Arg::Str(&version)]);
                    model.toast(text, Icon::CHECK_CIRCLE);
                }
                ToolkitEvent::Installed(Err(error)) => {
                    tracing::warn!("toolkit install: {error}");
                    model.toast(Extra::ToolkitFailed.text(language), Icon::WARNING);
                }
                ToolkitEvent::Adopted(Err(error)) => {
                    tracing::warn!("toolkit folder: {error}");
                    model.toast(Extra::ToolkitFolderInvalid.text(language), Icon::WARNING);
                }
            }
        }
    }

    /// The launcher update, every frame: results in, the state out to the screens, the notice's
    /// buttons in, and a restart that waited for a game goes ahead once the game is over. There is
    /// no toast: the update shows in About and as the notice on the home screen.
    fn poll_launcher(&mut self, cx: &mut OverlayCx<'_>) {
        self.launcher.poll();
        let model = &mut *cx.model;
        if model.update != self.launcher.state {
            model.update.clone_from(&self.launcher.state);
        }
        match model.update_request.take() {
            Some(UpdateRequest::Restart) => self.restart_launcher(model, cx.ctx),
            Some(UpdateRequest::CancelRestart) => self.launcher.cancel_hold(),
            None => {}
        }
        let game_running = model.running.is_some() || model.starting.is_some();
        if self.launcher.hold_released(game_running) {
            cx.ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// "Restart Now". The swap and the new exe follow in `exit`, once the preferences are saved;
    /// a running game is never closed for it, the restart waits (see `SelfUpdateService`).
    fn restart_launcher(&mut self, model: &mut Model, ctx: &Context) {
        let game_running = model.running.is_some() || model.starting.is_some();
        let resume = resume_link(model);
        if self.launcher.request_restart(game_running, resume) == Restart::Now {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// The notes of the staged update in the "What's New" panel.
    fn open_launcher_notes(&mut self, model: &mut Model) {
        let (Some(version), Some(notes)) = (self.launcher.staged_version(), self.launcher.notes())
        else {
            return;
        };
        self.launcher_published = Some(anyps5_core::releases::Published {
            tag: version.to_owned(),
            date: String::new(),
            notes,
        });
        self.settings_return = Category::ALL.iter().position(|c| *c == Category::About);
        model.switch_overlay(Overlay::WhatsNew);
    }

    /// Trophies earned by the running game raise banners (own viewport) and refresh the list.
    fn run_banners(&mut self, cx: &mut OverlayCx<'_>) {
        let earned = std::mem::take(&mut cx.model.trophy_banners);
        let mut refresh: Vec<GameId> = Vec::new();
        for (game, trophy) in earned {
            self.banners.push(BannerTrophy::from_core(&trophy));
            if !refresh.contains(&game) {
                refresh.push(game);
            }
        }
        for game in refresh {
            cx.model.reload_trophies(&game);
        }
        let monitor_width = cx
            .ctx
            .input(|i| i.viewport().monitor_size)
            .map_or(1920.0, |size| size.x);
        let reduced = cx.model.prefs.reduced_motion;
        let language = cx.model.language();
        self.banners
            .show(cx.shell, cx.ctx, reduced, monitor_width, language);
    }
}

impl OverlayHost for PanelHost {
    fn command(&mut self, cx: &mut OverlayCx<'_>, _overlay: Overlay, command: Command) {
        self.events.push(InputEvent::Command(command));
        cx.ctx.request_repaint();
    }

    fn ui(&mut self, _ui: &mut Ui, cx: &mut OverlayCx<'_>, overlay: Overlay, _openness: f32) {
        let wanted = cx.model.overlay == Some(overlay);
        match overlay {
            Overlay::Settings => self.settings_ui(cx, wanted),
            Overlay::GameSettings => self.game_settings_ui(cx, wanted),
            Overlay::ConfirmUninstall => self.uninstall_ui(cx, wanted),
            Overlay::ToolkitMissing => self.toolkit_missing_ui(cx, wanted),
            Overlay::InGame => self.in_game_ui(cx, wanted),
            Overlay::ConfirmCloseGame => self.close_game_ui(cx, wanted),
            Overlay::Saves => self.saves_ui(cx, wanted),
            Overlay::ConfirmRestore => self.restore_ui(cx, wanted),
            Overlay::Controls => self.controls_ui(cx, wanted),
            Overlay::Advanced => self.advanced_ui(cx, wanted),
            Overlay::Trophies => self.trophies_ui(cx, wanted),
            Overlay::Library => self.library_ui(cx, wanted),
            Overlay::Log => self.log_ui(cx, wanted),
            Overlay::Recommended => self.recommended_ui(cx, wanted),
            Overlay::WhatsNew => self.whats_new_ui(cx, wanted),
            Overlay::Friends => self.friends_ui(cx, wanted),
            Overlay::Profile => self.profile_ui(cx, wanted),
            Overlay::Compare => self.compare_ui(cx, wanted),
        }
    }

    fn captured_key(
        &mut self,
        cx: &mut OverlayCx<'_>,
        key: egui::Key,
        physical_key: Option<egui::Key>,
        modifiers: egui::Modifiers,
    ) {
        self.events.push(InputEvent::CapturedKey {
            key,
            physical_key,
            modifiers,
        });
        cx.ctx.request_repaint();
    }

    fn exit(&mut self) {
        if let Err(error) = self.launcher.finish() {
            tracing::warn!("launcher update: {error}");
        }
    }

    fn frame(&mut self, cx: &mut OverlayCx<'_>) {
        // Keys that arrived while no overlay was open belong to nobody.
        if cx.model.overlay.is_none() {
            self.events.clear();
        }
        self.poll_work(cx.model);
        self.poll_toolkit(cx.model);
        let paths = cx.model.paths.clone();
        self.launcher.tick(&paths, cx.model.prefs.check_for_updates);
        self.poll_launcher(cx);
        // No toolkit on this PC: fetch it now, already during the onboarding, so the first unlock
        // needs no visit to the settings. The result shows as a toast (see `poll_toolkit`).
        if !self.toolkit_auto_tried && cx.model.toolkit.is_none() {
            self.toolkit_auto_tried = true;
            let model = &*cx.model;
            self.toolkit.install(
                &model.paths,
                &model.prefs.toolkit_repository,
                model.prefs.toolkit_pinned.as_deref(),
                model.prefs.kept_toolkit_tags(),
            );
        }
        if !self.checked_at_start && cx.model.onboarded() {
            self.checked_at_start = true;
            if cx.model.prefs.check_for_updates {
                self.check_toolkit(cx.model);
            }
        }
        // Once a day while the launcher stays open.
        if cx.model.prefs.check_for_updates
            && self
                .last_check
                .is_some_and(|at| at.elapsed() > std::time::Duration::from_secs(24 * 3600))
        {
            self.check_toolkit(cx.model);
        }
        if cx.model.prefs.toolkit_channel() == ToolkitChannel::Pinned {
            let model = &*cx.model;
            self.toolkit
                .load_recent(&model.paths, &model.prefs.toolkit_repository);
        }
        if !self.catalog_asked && cx.model.onboarded() && cx.model.prefs.check_for_updates {
            self.catalog_asked = true;
            let (tx, ctx, paths) = (
                self.work_tx.clone(),
                self.ctx.clone(),
                cx.model.paths.clone(),
            );
            spawn("recommended settings", move || {
                let catalog = recommended::load(&paths);
                let _ = tx.send(Work::Catalog(Box::new(catalog)));
                ctx.request_repaint();
            });
        }
        self.run_banners(cx);
    }
}

/// What a failed backup or restore says, in one line.
fn saves_failure(error: &SavesError, restoring: bool, language: Language) -> &'static str {
    match error {
        SavesError::NothingToSave => Extra::SavesNothing,
        SavesError::GameRunning => Extra::SavesRunning,
        SavesError::Corrupt => Extra::SavesCorrupt,
        SavesError::Incomplete { kept: true, .. } => Extra::SavesRestoreIncomplete,
        SavesError::Incomplete { kept: false, .. } => Extra::SavesRestoreIncompleteNone,
        SavesError::Io { .. } if restoring => Extra::SavesRestoreFailed,
        SavesError::Io { .. } => Extra::SavesFailed,
    }
    .text(language)
}

/// Stores a game's edited settings and marks the preferences for saving.
fn store_game_settings(model: &mut Model, id: &GameId, settings: GameSettings) {
    model.prefs.set_game_settings(id, settings);
    model.mark_prefs_dirty();
}

/// Opens the issue form of a report in the browser. A report too long for the link is copied
/// first, to be pasted into the form; when the browser does not open, the link is copied instead.
fn open_upstream(ctx: &Context, model: &mut Model, prepared: &issue::Prepared) {
    let language = model.language();
    if let Some(text) = &prepared.clipboard {
        ctx.copy_text(text.clone());
    }
    match issue::open_url(&prepared.url) {
        Ok(()) => {
            let text = if prepared.clipboard.is_some() {
                Share::ReportOpenedPaste
            } else {
                Share::ReportOpened
            };
            model.toast(text.text(language), Icon::CHECK_CIRCLE);
        }
        Err(error) => {
            tracing::warn!("cannot open the browser: {error}");
            ctx.copy_text(prepared.url.clone());
            model.toast(Share::ReportOpenFailed.text(language), Icon::WARNING);
        }
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!("cannot start the {name} thread: {error}");
    }
}

/// Where a restarted launcher should open, as a deep link: the About page when the restart came
/// from the settings, else the game that is selected. `None` with an empty library.
fn resume_link(model: &Model) -> Option<String> {
    if model.overlay == Some(Overlay::Settings) {
        return Some(format!("{}://settings/about", anyps5_core::links::SCHEME));
    }
    let game = model.selected_game()?;
    // The title ID comes from the game's files: it only goes into a link the strict parser accepts
    // as a plain game link.
    let link = format!("{}://game/{}", anyps5_core::links::SCHEME, game.title_id);
    matches!(
        anyps5_core::links::parse(&link),
        Ok(anyps5_core::links::Link::Game { .. })
    )
    .then_some(link)
}
/// A release page of the launcher itself: `.../releases/tag/v` and a version of digits, letters,
/// dots and dashes, nothing else.
fn launcher_release_page(url: &str) -> bool {
    url.strip_prefix("https://github.com/anyplaynet/launcher/releases/tag/v")
        .is_some_and(|version| {
            !version.is_empty()
                && version
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        })
}

/// Opens a page of the toolkit's repository in the browser. Nothing else is opened.
fn open_url(url: &str) {
    if !url.starts_with("https://github.com/boykopovar/AnyPS5/") && !launcher_release_page(url) {
        return;
    }
    if let Err(error) = anyps5_core::platform::open_url(url) {
        tracing::warn!("cannot open {url}: {error}");
    }
}

/// Opens one of the fixed pages the launcher itself points to (the driver for the phone's pad).
fn open_known_url(url: &str) {
    let known = [
        crate::screens::phone::VIGEM_DOWNLOAD,
        crate::screens::phone::UINPUT_GUIDE,
        crate::screens::about_strings::WEBSITE,
        crate::screens::about_strings::DISCORD,
        crate::screens::about_strings::PRIVACY,
        crate::screens::about_strings::TERMS,
        crate::screens::about_strings::INSTALL_GUIDE,
        crate::screens::update_strings::RELEASES,
    ];
    if !known.contains(&url) {
        return;
    }
    if let Err(error) = anyps5_core::platform::open_url(url) {
        tracing::warn!("cannot open {url}: {error}");
    }
}

/// Opens a folder in the file manager.
fn open_folder(folder: &std::path::Path) {
    if let Err(error) = anyps5_core::platform::open_folder(folder) {
        tracing::warn!("cannot open {}: {error}", folder.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::pipeline::SimPipeline;
    use crate::app::preview::{Scene, apply_scene, make_games};
    use crate::app::textures::Textures;
    use crate::app::{Config, LauncherApp};
    use crate::ui::Shell;
    use crate::ui::focus::HomeIntent;
    use crate::ui::theme::Theme;
    use anyps5_core::settings::{Preferences, Theme as CoreTheme};
    use egui::{Pos2, RawInput, Rect, vec2};

    /// A launcher app on a temp folder with the panel host, driven frame by frame.
    struct Rig {
        ctx: Context,
        app: LauncherApp,
        dir: PathBuf,
        time: f64,
    }

    impl Rig {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("anyps5-panels-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let ctx = Context::default();
            let config = Config {
                prefs: Preferences::default(),
                paths: AppPaths::under(&dir.join("appdata")),
                persist: true,
                pipeline: Box::new(SimPipeline::new("0.1.0")),
                panels: Box::new(PanelHost::new(&ctx, true)),
                scan_on_start: false,
            };
            let shell = Shell::for_test(&ctx, Theme::Cosmic);
            let mut app = LauncherApp::from_shell(shell, &ctx, config);
            app.offline = true;
            apply_scene(&mut app.model, Scene::Home, make_games(&dir.join("art")));
            // The scene touched the preferences; start from a clean file.
            app.model.take_prefs_dirty();
            let mut rig = Self {
                ctx,
                app,
                dir,
                time: 1.0,
            };
            rig.frames(3);
            rig
        }

        fn frames(&mut self, count: usize) {
            for _ in 0..count {
                let input = RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 900.0))),
                    time: Some(self.time),
                    ..Default::default()
                };
                self.time += 0.1;
                let ctx = self.ctx.clone();
                let app = &mut self.app;
                let mut output = ctx.run_ui(input, |ui| {
                    app.step(&ctx);
                    app.paint(ui);
                });
                output.textures_delta.clear();
            }
        }

        fn press(&mut self, command: Command) {
            let ctx = self.ctx.clone();
            self.app.handle_event(&ctx, InputEvent::Command(command));
            self.frames(2);
        }

        fn saved(&self) -> Preferences {
            Preferences::load(&self.app.model.paths)
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn cx<'a>(rig: &'a mut Rig, textures: &'a mut Textures) -> OverlayCx<'a> {
        OverlayCx {
            model: &mut rig.app.model,
            shell: &mut rig.app.shell,
            textures,
            ctx: &rig.ctx,
        }
    }

    #[test]
    fn the_quick_panel_of_the_home_button_stays_open_and_acts() {
        let mut rig = Rig::new("in-game");
        let games = make_games(&rig.dir.join("art"));
        apply_scene(&mut rig.app.model, Scene::HomeInGame, games);
        rig.frames(5);
        assert_eq!(rig.app.model.overlay, Some(Overlay::InGame));
        // Close Game asks first.
        let ctx = rig.ctx.clone();
        rig.app.model.switch_overlay(Overlay::ConfirmCloseGame);
        rig.frames(5);
        assert_eq!(rig.app.model.overlay, Some(Overlay::ConfirmCloseGame));
        // The question opens on Keep Playing: Enter does not stop the game.
        rig.app
            .handle_event(&ctx, InputEvent::Command(Command::Select));
        rig.frames(3);
        assert_eq!(rig.app.model.overlay, Some(Overlay::InGame));
        assert!(rig.app.model.running.is_some());
        // Back on the panel returns to the game.
        rig.press(Command::Back);
        assert_eq!(rig.app.model.overlay, None);
        assert!(!rig.app.model.summoned);
    }
    #[test]
    fn every_overlay_opens_and_closes_with_back() {
        let mut rig = Rig::new("overlays");
        for overlay in [
            Overlay::Settings,
            Overlay::Library,
            Overlay::GameSettings,
            Overlay::Controls,
            Overlay::Advanced,
            Overlay::Trophies,
            Overlay::Log,
            Overlay::ConfirmUninstall,
            Overlay::Saves,
            Overlay::ConfirmRestore,
            Overlay::Recommended,
            Overlay::WhatsNew,
        ] {
            rig.app.model.open(overlay);
            rig.frames(3);
            assert_eq!(rig.app.model.overlay, Some(overlay));
            rig.press(Command::Back);
            // Controls and advanced step back to the game settings, the rest close.
            let expected = match overlay {
                Overlay::Controls | Overlay::Advanced | Overlay::Recommended | Overlay::Saves => {
                    Some(Overlay::GameSettings)
                }
                // The question goes back to the list.
                Overlay::ConfirmRestore => Some(Overlay::Saves),
                // The notes go back to the Toolkit page of the settings.
                Overlay::WhatsNew => Some(Overlay::Settings),
                _ => None,
            };
            assert_eq!(rig.app.model.overlay, expected, "back from {overlay:?}");
            // The question is two steps from home: the list, then the game settings.
            let steps = if overlay == Overlay::ConfirmRestore {
                2
            } else {
                1
            };
            for _ in 0..if expected.is_some() { steps } else { 0 } {
                rig.press(Command::Back);
            }
            assert_eq!(rig.app.model.overlay, None);
        }
    }

    #[test]
    fn home_buttons_open_the_panels() {
        let mut rig = Rig::new("home");
        for (intent, overlay) in [
            (HomeIntent::OpenSettings, Overlay::Settings),
            (HomeIntent::OpenLibrary, Overlay::Library),
            (HomeIntent::OpenOnline, Overlay::Friends),
            (HomeIntent::OpenFriends, Overlay::Friends),
            (HomeIntent::OpenGameSettings, Overlay::GameSettings),
            (HomeIntent::OpenTrophies, Overlay::Trophies),
        ] {
            rig.app.model.apply_intent(intent);
            assert_eq!(rig.app.model.overlay, Some(overlay));
            rig.frames(3);
            rig.press(Command::Back);
            assert_eq!(rig.app.model.overlay, None);
        }
    }

    #[test]
    fn theme_change_reaches_the_shell_and_is_saved() {
        let mut rig = Rig::new("theme");
        rig.app.model.open(Overlay::Settings);
        rig.frames(2);
        // Appearance, into its rows (theme row first), one step right.
        rig.press(Command::Select);
        rig.press(Command::Right);
        assert_eq!(rig.app.model.prefs.theme, CoreTheme::Aurora);
        assert_eq!(rig.app.shell.backdrop.theme(), Theme::Aurora);
        rig.frames(2);
        assert_eq!(rig.saved().theme, CoreTheme::Aurora, "written to disk");
    }

    #[test]
    fn game_setting_changes_are_stored_and_saved() {
        let mut rig = Rig::new("game");
        let id = rig.app.model.selected_game().unwrap().id.clone();
        assert!(!rig.app.model.prefs.game_settings(&id).fullscreen);
        rig.app.model.open(Overlay::GameSettings);
        rig.frames(2);
        // The first row is the display mode (no patch, so no resolution row).
        rig.press(Command::Right);
        assert!(rig.app.model.prefs.game_settings(&id).fullscreen);
        rig.frames(2);
        assert!(rig.saved().game_settings(&id).fullscreen);
    }

    #[test]
    fn looking_without_changing_writes_nothing() {
        let mut rig = Rig::new("quiet");
        rig.app.model.open(Overlay::GameSettings);
        rig.frames(4);
        rig.press(Command::Down);
        rig.press(Command::Up);
        rig.press(Command::Back);
        rig.frames(3);
        assert!(!rig.app.model.paths.preferences_file().exists());
    }

    #[test]
    fn controls_row_opens_the_controls_and_back_returns() {
        let mut rig = Rig::new("pages");
        rig.app.model.open(Overlay::GameSettings);
        rig.frames(3);
        // display, size, vsync, controls
        for _ in 0..3 {
            rig.press(Command::Down);
        }
        rig.press(Command::Select);
        assert_eq!(rig.app.model.overlay, Some(Overlay::Controls));
        rig.frames(3);
        rig.press(Command::Back);
        assert_eq!(rig.app.model.overlay, Some(Overlay::GameSettings));
    }

    #[test]
    fn library_pick_selects_the_game_and_closes() {
        let mut rig = Rig::new("library");
        rig.app.model.open(Overlay::Library);
        rig.frames(3);
        let before = rig.app.model.selected;
        rig.press(Command::Right);
        rig.press(Command::Select);
        assert_eq!(rig.app.model.overlay, None);
        assert_eq!(rig.app.model.selected, before + 1);
    }

    #[test]
    fn reinstall_closes_the_panel_and_queues_the_unlock() {
        let mut rig = Rig::new("reinstall");
        let mut host = PanelHost::new(&rig.ctx, true);
        let game = rig.app.model.selected_game().cloned().unwrap();
        rig.app.model.toolkit = Some("0.1.0".into());
        rig.app.model.open(Overlay::GameSettings);
        let mut textures = Textures::new(&rig.ctx);
        host.game_settings_action(
            &mut cx(&mut rig, &mut textures),
            &game,
            GameSettingsAction::Reinstall,
        );
        assert_eq!(rig.app.model.overlay, None);
        assert!(rig.app.model.queue.contains(&game.id));
    }

    #[test]
    fn page_actions_switch_overlays_without_closing() {
        let mut rig = Rig::new("switch");
        let mut host = PanelHost::new(&rig.ctx, true);
        let game = rig.app.model.selected_game().cloned().unwrap();
        let mut textures = Textures::new(&rig.ctx);
        for (action, expected) in [
            (GameSettingsAction::OpenControls, Overlay::Controls),
            (GameSettingsAction::OpenAdvanced, Overlay::Advanced),
            (GameSettingsAction::AskUninstall, Overlay::ConfirmUninstall),
            (GameSettingsAction::OpenLog, Overlay::Log),
        ] {
            rig.app.model.open(Overlay::GameSettings);
            host.game_settings_action(&mut cx(&mut rig, &mut textures), &game, action);
            assert_eq!(rig.app.model.overlay, Some(expected));
        }
        host.back_to_game_settings(&mut rig.app.model, "advanced");
        assert_eq!(rig.app.model.overlay, Some(Overlay::GameSettings));
        assert_eq!(host.return_row, Some("advanced"));
    }

    #[test]
    fn settings_actions_reach_the_model() {
        let mut rig = Rig::new("settings-actions");
        let mut host = PanelHost::new(&rig.ctx, true);
        let mut textures = Textures::new(&rig.ctx);
        rig.app.model.open(Overlay::Settings);
        host.settings_action(&mut cx(&mut rig, &mut textures), SettingsAction::Rescan);
        assert!(rig.app.model.scanning, "a rescan was asked for");
        host.settings_action(&mut cx(&mut rig, &mut textures), SettingsAction::Close);
        assert_eq!(rig.app.model.overlay, None);
    }

    /// Polls the host until a toast shows, and returns its text.
    fn wait_for_toast(host: &mut PanelHost, model: &mut Model) -> String {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            host.poll_work(model);
            if let Some(toast) = &model.toast {
                return toast.text.clone();
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("no toast");
    }

    /// The selected game with an unlocked copy that holds one save.
    fn game_with_save(rig: &mut Rig) -> (Game, PathBuf) {
        let game = rig.app.model.selected_game().cloned().unwrap();
        let root = rig.dir.join("games");
        rig.app.model.prefs.games_folder = Some(root.clone());
        let copy = anyps5_core::paths::unlocked_copy_dir(&root, &game.title_id, &game.id);
        std::fs::create_dir_all(copy.join("_sd")).unwrap();
        std::fs::write(copy.join("_sd/slot0.sav"), b"ONE").unwrap();
        (game, copy)
    }

    #[test]
    fn saves_are_backed_up_and_restored_off_thread_with_a_toast_each() {
        let mut rig = Rig::new("saves");
        let mut host = PanelHost::new(&rig.ctx, true);
        let (game, copy) = game_with_save(&mut rig);
        host.refresh_saves(&rig.app.model, &game);
        assert!(host.save_view.as_ref().unwrap().has_saves);
        assert!(host.save_view.as_ref().unwrap().backups.is_empty());

        host.start_backup(&mut rig.app.model, &game);
        assert_eq!(
            wait_for_toast(&mut host, &mut rig.app.model),
            "Saves backed up"
        );
        host.refresh_saves(&rig.app.model, &game);
        let view = host.save_view.as_ref().unwrap();
        assert_eq!(view.backups.len(), 1);
        let backup = view.backups[0].clone();

        std::fs::write(copy.join("_sd/slot0.sav"), b"TWO").unwrap();
        rig.app.model.toast = None;
        host.start_restore(&mut rig.app.model, &game, backup);
        assert_eq!(
            wait_for_toast(&mut host, &mut rig.app.model),
            "Saves restored"
        );
        assert_eq!(std::fs::read(copy.join("_sd/slot0.sav")).unwrap(), b"ONE");
        // The restore kept what it replaced, so it can be undone.
        host.refresh_saves(&rig.app.model, &game);
        assert_eq!(host.save_view.as_ref().unwrap().backups.len(), 2);
    }

    #[test]
    fn a_running_game_is_not_restored_over() {
        let mut rig = Rig::new("saves-running");
        let mut host = PanelHost::new(&rig.ctx, true);
        let (game, copy) = game_with_save(&mut rig);
        let backup = SaveData::for_title(&rig.app.model.paths, &game.title_id, copy.clone())
            .backup(Reason::Manual)
            .unwrap();
        std::fs::write(copy.join("_sd/slot0.sav"), b"LIVE").unwrap();
        rig.app.model.running = Some(game.id.clone());
        host.start_restore(&mut rig.app.model, &game, backup);
        assert_eq!(
            rig.app.model.toast.as_ref().unwrap().text,
            "Close the game to restore a backup."
        );
        assert_eq!(std::fs::read(copy.join("_sd/slot0.sav")).unwrap(), b"LIVE");
        assert!(rig.app.model.saves_busy.is_none());
    }

    #[test]
    fn uninstall_keeps_a_backup_of_the_saves_it_removes() {
        let mut rig = Rig::new("saves-uninstall");
        let mut host = PanelHost::new(&rig.ctx, true);
        let (game, copy) = game_with_save(&mut rig);
        std::fs::write(copy.join("eboot.bin"), b"x").unwrap();
        let root = rig.dir.join("games");
        let backups = rig.app.model.paths.game_backups_dir(&game.title_id);
        host.start_uninstall(&game, root, backups.clone(), Language::English);
        wait_for_toast(&mut host, &mut rig.app.model);
        assert!(!copy.exists());
        let kept = SaveData::new(copy, backups).list();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].reason, Reason::BeforeUninstall);
    }

    #[test]
    fn a_removal_that_cannot_keep_the_saves_does_not_remove_anything() {
        let mut rig = Rig::new("saves-uninstall-fails");
        let mut host = PanelHost::new(&rig.ctx, true);
        let (game, copy) = game_with_save(&mut rig);
        std::fs::write(copy.join("eboot.bin"), b"x").unwrap();
        let root = rig.dir.join("games");
        // A file where the backup folder should be: no backup can be written.
        let backups = rig.dir.join("not-a-folder");
        std::fs::write(&backups, b"x").unwrap();
        host.start_uninstall(&game, root, backups, Language::English);
        let text = wait_for_toast(&mut host, &mut rig.app.model);
        assert!(text.starts_with("Could not back up the saves"), "{text}");
        assert_eq!(std::fs::read(copy.join("_sd/slot0.sav")).unwrap(), b"ONE");
        assert!(
            rig.app
                .model
                .selected_game()
                .unwrap()
                .unlocked_with
                .is_some()
        );
    }

    #[test]
    fn a_game_is_not_started_while_its_saves_are_being_worked_on() {
        let mut rig = Rig::new("saves-busy");
        let game = rig.app.model.selected_game().cloned().unwrap();
        rig.app.model.saves_busy = Some(game.id.clone());
        rig.app.model.play(&game.id);
        assert!(rig.app.model.starting.is_none());
        assert_eq!(
            rig.app.model.toast.as_ref().unwrap().text,
            "Still working on the saves. Try again in a moment."
        );
    }

    #[test]
    fn uninstall_removes_the_copy_off_thread_and_updates_the_game() {
        let mut rig = Rig::new("uninstall");
        let mut host = PanelHost::new(&rig.ctx, true);
        let game = rig.app.model.selected_game().cloned().unwrap();
        let root = rig.dir.join("games");
        let copy = anyps5_core::paths::unlocked_copy_dir(&root, &game.title_id, &game.id);
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(copy.join("eboot.bin"), b"x").unwrap();
        let backups = rig.app.model.paths.game_backups_dir(&game.title_id);
        host.start_uninstall(&game, root, backups, Language::English);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            host.poll_work(&mut rig.app.model);
            if rig.app.model.toast.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!copy.exists());
        let selected = rig.app.model.selected;
        assert!(rig.app.model.games[selected].unlocked_with.is_none());
    }

    #[test]
    fn only_a_release_page_of_the_launcher_may_be_opened_from_the_notes() {
        let page = "https://github.com/anyplaynet/launcher/releases/tag/v";
        assert!(launcher_release_page(&format!("{page}0.2.5")));
        assert!(launcher_release_page(&format!("{page}0.3.0-rc.1")));
        for bad in [
            page.to_owned(),
            format!("{page}0.2.5/../../x"),
            format!("{page}0.2.5@evil.example"),
            format!("{page}0.2.5?x=1"),
            "https://github.com/anyplaynet/launcher/releases/tag/".to_owned(),
            "https://evil.example/anyplaynet/launcher/releases/tag/v1".to_owned(),
        ] {
            assert!(!launcher_release_page(&bad), "{bad}");
        }
    }
}

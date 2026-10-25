//! The advanced panel (docs/DESIGN.md 5.9): controller sticks, the mouse as the camera, keyboard
//! sticks, the shader cache and sharing a game's settings as a file. Tuning values are the core's
//! [`InputTuning`]; only values that differ from the defaults are kept (the whole group goes back
//! to `None` when it equals them).

use anyps5_core::controls::InputTuning;
use anyps5_core::library::Game;
use anyps5_core::settings::{GameSettings, SharedSettings};
use egui::{Color32, Context};

use crate::screens::format;
use crate::screens::rows::{self, Change, Row};
use crate::screens::side::{self, Output, SideState, SideView};
use crate::screens::{ToastRequest, dialogs};
use crate::ui::Shell;
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;
use crate::ui::sound::Cue;

/// What the host has to do after a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdvancedAction {
    /// Back or options: the panel is gone; open the game settings on the `advanced` row.
    Back,
    /// The scrim was clicked: everything closes.
    Close,
    Toast(ToastRequest),
}

/// The game, borrowed for one frame.
pub struct AdvancedData<'a> {
    pub game: &'a Game,
    pub settings: &'a mut GameSettings,
    /// Resolutions the title's patch offers (an imported resolution must be one of them).
    pub resolutions: &'a [String],
    pub language: Language,
    pub accent: Color32,
}

#[derive(Clone, Copy)]
enum Fmt {
    Percent,
    Times,
    Tenths,
    /// Milliseconds, "Off" at zero.
    MsOff,
    Ms,
}

struct Tune {
    id: &'static str,
    title: Key,
    detail: Key,
    icon: Icon,
    range: (i32, i32),
    step: i32,
    fmt: Fmt,
    get: fn(&InputTuning) -> i32,
    set: fn(&mut InputTuning, i32),
}

const TUNES: [Tune; 12] = [
    Tune {
        id: "leftDeadzone",
        title: Key::LeftDeadzone,
        detail: Key::DeadzoneDetail,
        icon: Icon::GAME_CONTROLLER,
        range: (0, 90),
        step: 2,
        fmt: Fmt::Percent,
        get: |t| t.left_deadzone,
        set: |t, v| t.left_deadzone = v,
    },
    Tune {
        id: "rightDeadzone",
        title: Key::RightDeadzone,
        detail: Key::DeadzoneDetail,
        icon: Icon::GAME_CONTROLLER,
        range: (0, 90),
        step: 2,
        fmt: Fmt::Percent,
        get: |t| t.right_deadzone,
        set: |t, v| t.right_deadzone = v,
    },
    Tune {
        id: "sensitivity",
        title: Key::MouseSensitivity,
        detail: Key::MouseSensitivityDetail,
        icon: Icon::MOUSE,
        range: (1, 100),
        step: 1,
        fmt: Fmt::Times,
        get: |t| t.mouse_sensitivity,
        set: |t, v| t.mouse_sensitivity = v,
    },
    Tune {
        id: "vertical",
        title: Key::MouseVertical,
        detail: Key::MouseVerticalDetail,
        icon: Icon::ARROWS_DOWN_UP,
        range: (1, 100),
        step: 1,
        fmt: Fmt::Times,
        get: |t| t.mouse_vertical_scale,
        set: |t, v| t.mouse_vertical_scale = v,
    },
    Tune {
        id: "curve",
        title: Key::MouseCurve,
        detail: Key::MouseCurveDetail,
        icon: Icon::CHART_LINE_UP,
        range: (2, 50),
        step: 1,
        fmt: Fmt::Tenths,
        get: |t| t.mouse_curve,
        set: |t, v| t.mouse_curve = v,
    },
    Tune {
        id: "offset",
        title: Key::MouseMinimum,
        detail: Key::MouseMinimumDetail,
        icon: Icon::CROSSHAIR,
        range: (0, 90),
        step: 2,
        fmt: Fmt::Percent,
        get: |t| t.mouse_deadzone_offset,
        set: |t, v| t.mouse_deadzone_offset = v,
    },
    Tune {
        id: "maxSpeed",
        title: Key::MouseMaxSpeed,
        detail: Key::MouseMaxSpeedDetail,
        icon: Icon::GAUGE,
        range: (1, 100),
        step: 1,
        fmt: Fmt::Times,
        get: |t| t.mouse_max_speed,
        set: |t, v| t.mouse_max_speed = v,
    },
    Tune {
        id: "smoothing",
        title: Key::MouseSmoothing,
        detail: Key::MouseSmoothingDetail,
        icon: Icon::WAVE_SINE,
        range: (0, 500),
        step: 5,
        fmt: Fmt::MsOff,
        get: |t| t.mouse_smoothing_ms,
        set: |t, v| t.mouse_smoothing_ms = v,
    },
    Tune {
        id: "decay",
        title: Key::MouseDecay,
        detail: Key::MouseDecayDetail,
        icon: Icon::TIMER,
        range: (5, 1000),
        step: 5,
        fmt: Fmt::Ms,
        get: |t| t.mouse_decay_ms,
        set: |t, v| t.mouse_decay_ms = v,
    },
    Tune {
        id: "modifier",
        title: Key::StickModifierScale,
        detail: Key::StickModifierScaleDetail,
        icon: Icon::PERSON_SIMPLE_WALK,
        range: (5, 100),
        step: 5,
        fmt: Fmt::Percent,
        get: |t| t.stick_modifier_scale,
        set: |t, v| t.stick_modifier_scale = v,
    },
    Tune {
        id: "keyboardRamp",
        title: Key::KeyboardRamp,
        detail: Key::KeyboardRampDetail,
        icon: Icon::KEYBOARD,
        range: (0, 2000),
        step: 50,
        fmt: Fmt::MsOff,
        get: |t| t.keyboard_ramp_ms,
        set: |t, v| t.keyboard_ramp_ms = v,
    },
    Tune {
        id: "triggerRamp",
        title: Key::TriggerRamp,
        detail: Key::TriggerRampDetail,
        icon: Icon::RECTANGLE,
        range: (0, 2000),
        step: 50,
        fmt: Fmt::MsOff,
        get: |t| t.trigger_ramp_ms,
        set: |t, v| t.trigger_ramp_ms = v,
    },
];

fn show_value(fmt: Fmt, value: i32, language: Language) -> String {
    match fmt {
        Fmt::Percent => format::percent(value),
        Fmt::Times => format::times(value),
        Fmt::Tenths => format::tenths(value),
        Fmt::MsOff => format::milliseconds(value, language.text(Key::Off)),
        Fmt::Ms => format!("{value} ms"),
    }
}

fn tune_row(tune: &Tune, tuning: &InputTuning, language: Language) -> Row {
    let value = (tune.get)(tuning);
    Row::stepper(
        tune.id,
        language.text(tune.title),
        value,
        tune.range,
        tune.step,
        show_value(tune.fmt, value, language),
    )
    .detail(language.text(tune.detail))
    .icon(tune.icon)
}

/// The rows of the panel (section headers are info rows).
pub fn build_rows(settings: &GameSettings, language: Language) -> Vec<Row> {
    let t = |key: Key| language.text(key);
    let tuning = settings.tuning.unwrap_or_default();
    let tune = |id: &str| {
        let tune = TUNES.iter().find(|x| x.id == id).expect("known tuning row");
        tune_row(tune, &tuning, language)
    };
    vec![
        Row::caption("sticksSection", t(Key::SectionSticks)),
        tune("leftDeadzone"),
        tune("rightDeadzone"),
        Row::caption("mouseSection", t(Key::SectionMouse)),
        tune("sensitivity"),
        tune("vertical"),
        tune("curve"),
        tune("offset"),
        tune("maxSpeed"),
        tune("smoothing"),
        tune("decay"),
        Row::caption("keyboardSection", t(Key::SectionKeyboard)),
        tune("modifier"),
        tune("keyboardRamp"),
        tune("triggerRamp"),
        Row::caption("graphicsSection", t(Key::SectionGraphics)),
        Row::toggle(
            "shaderCache",
            t(Key::ShaderCache),
            settings.shader_cache.unwrap_or(true),
        )
        .detail(t(Key::ShaderCacheDetail))
        .icon(Icon::STACK),
        Row::caption("shareSection", t(Key::SectionShare)),
        Row::command("export", t(Key::ExportSettings))
            .detail(t(Key::ExportSettingsDetail))
            .icon(Icon::EXPORT),
        Row::command("import", t(Key::ImportSettings))
            .detail(t(Key::ImportSettingsDetail))
            .icon(Icon::DOWNLOAD_SIMPLE),
        Row::command("resetAdvanced", t(Key::ResetAdvanced)).icon(Icon::ARROW_COUNTER_CLOCKWISE),
    ]
}

/// Writes one change into the settings. Returns whether anything changed.
pub fn apply_change(settings: &mut GameSettings, change: &Change) -> bool {
    let before = settings.clone();
    match change {
        Change::Stepper { row, value } => {
            if let Some(tune) = TUNES.iter().find(|t| t.id == *row) {
                let mut tuning = settings.tuning.unwrap_or_default();
                (tune.set)(&mut tuning, (*value).clamp(tune.range.0, tune.range.1));
                settings.tuning = (tuning != InputTuning::default()).then_some(tuning);
            }
        }
        Change::Toggle {
            row: "shaderCache",
            on,
        } => {
            settings.shader_cache = (!*on).then_some(false);
        }
        Change::Run {
            row: "resetAdvanced",
        } => {
            settings.tuning = None;
            settings.shader_cache = None;
        }
        _ => {}
    }
    *settings != before
}

/// Writes the game's settings to `path` as an AnyPlay settings file.
pub fn export_settings(
    path: &std::path::Path,
    game: &Game,
    settings: &GameSettings,
) -> std::io::Result<()> {
    std::fs::write(path, SharedSettings::new(game, settings).to_json())
}

/// What importing a file gave.
#[derive(Debug)]
pub struct Imported {
    pub settings: GameSettings,
    /// The name of the game the file was exported from, when it is another title.
    pub other_game: Option<String>,
}

/// Reads a settings file for `game`. A resolution only applies to the title whose patch offers it.
pub fn import_settings(
    path: &std::path::Path,
    game: &Game,
    resolutions: &[String],
) -> Option<Imported> {
    let bytes = std::fs::read(path).ok()?;
    let shared = SharedSettings::read(&bytes).ok()?;
    let mut settings = shared.settings;
    if settings
        .resolution
        .as_ref()
        .is_some_and(|r| !resolutions.contains(r))
    {
        settings.resolution = None;
    }
    let other_game = (shared.title_id != game.title_id).then_some(shared.game);
    Some(Imported {
        settings,
        other_game,
    })
}

/// The advanced panel.
#[derive(Default)]
pub struct AdvancedPanel {
    side: SideState,
}

impl AdvancedPanel {
    /// Opens on the first stepper (the section header above it is not focusable).
    pub fn open(&mut self) {
        self.side.open_at(1);
    }

    /// Opens on a row (previews).
    pub fn open_row(&mut self, row: usize) {
        self.side.open_at(row);
    }

    pub fn close(&mut self) {
        self.side.close();
    }

    pub fn dismiss(&mut self) {
        self.side.dismiss();
    }

    pub fn is_active(&self) -> bool {
        self.side.is_active()
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &mut AdvancedData<'_>,
    ) -> Output<AdvancedAction> {
        let mut out = Output::default();
        if !self.side.is_active() {
            return out;
        }
        if self.side.is_open() {
            let rows = build_rows(data.settings, data.language);
            let driven = rows::drive(shell, &mut self.side.nav, &rows, &side::commands(events));
            for change in &driven.changes {
                self.apply(shell, data, change, &mut out);
            }
            if driven.close {
                // Back and options return to the game settings: no slide.
                self.side.dismiss();
                out.actions.push(AdvancedAction::Back);
                return out;
            }
        }
        let mut rows = build_rows(data.settings, data.language);
        let view = SideView {
            title: &data.game.name,
            subtitle: Some(data.language.text(Key::AdvancedTitle)),
            question: false,
            list_id: "advanced-rows",
            nav: &self.side.nav,
            accent: data.accent,
            language: data.language,
            force: None,
        };
        match side::draw(ctx, self.side.is_open(), &view, &mut rows) {
            Some(drawn) if self.side.is_open() => {
                let (changes, _) =
                    rows::apply_mouse(shell, &mut self.side.nav, &rows, drawn.events);
                for change in &changes {
                    self.apply(shell, data, change, &mut out);
                }
                if drawn.scrim_clicked {
                    shell.play(Cue::Back);
                    self.side.close();
                    out.actions.push(AdvancedAction::Close);
                }
            }
            Some(_) => {}
            None => self.side.finished(),
        }
        out
    }

    fn apply(
        &mut self,
        shell: &mut Shell,
        data: &mut AdvancedData<'_>,
        change: &Change,
        out: &mut Output<AdvancedAction>,
    ) {
        if apply_change(data.settings, change) {
            out.changed = true;
        }
        match change {
            Change::Run { row: "export" } => {
                if let Some(toast) = export_flow(shell, data) {
                    out.actions.push(AdvancedAction::Toast(toast));
                }
            }
            Change::Run { row: "import" } => {
                if let Some((toast, changed)) = import_flow(shell, data) {
                    out.changed |= changed;
                    out.actions.push(AdvancedAction::Toast(toast));
                }
            }
            _ => {}
        }
    }
}

fn export_flow(shell: &mut Shell, data: &AdvancedData<'_>) -> Option<ToastRequest> {
    let language = data.language;
    let name = format!("{} - AnyPlay.json", data.game.name);
    let path = dialogs::save_file(
        shell,
        language.text(Key::ExportSettings),
        &name,
        ("JSON", &["json"]),
        None,
    )?;
    Some(match export_settings(&path, data.game, data.settings) {
        Ok(()) => ToastRequest::new(Icon::EXPORT, language.text(Key::SettingsExported)),
        Err(_) => ToastRequest::warning(language.text(Key::SettingsFileFailed)),
    })
}

fn import_flow(shell: &mut Shell, data: &mut AdvancedData<'_>) -> Option<(ToastRequest, bool)> {
    let language = data.language;
    let path = dialogs::pick_file(
        shell,
        language.text(Key::ImportSettings),
        ("JSON", &["json"]),
        None,
    )?;
    let Some(imported) = import_settings(&path, data.game, data.resolutions) else {
        return Some((
            ToastRequest::warning(language.text(Key::SettingsFileFailed)),
            false,
        ));
    };
    // Which toolkit a game uses is about this PC: a file never changes it.
    let mut incoming = imported.settings;
    incoming.toolkit = data.settings.toolkit.clone();
    let changed = *data.settings != incoming;
    *data.settings = incoming;
    let text = match imported.other_game {
        None => language.text(Key::SettingsImported).to_owned(),
        Some(game) => language.format(Key::SettingsImportedOther, &[Arg::Str(&game)]),
    };
    Some((ToastRequest::new(Icon::DOWNLOAD_SIMPLE, text), changed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyps5_core::paths::GameId;

    fn game(title_id: &str, name: &str) -> Game {
        Game {
            id: GameId::from_key(title_id),
            folder: title_id.into(),
            title_id: title_id.into(),
            name: name.into(),
            names: Default::default(),
            version: "01.000.000".into(),
            content_id: None,
            icon: None,
            background: None,
            music: None,
            executable: anyps5_core::library::ExecutableKind::Elf,
            size: None,
            unlocked_with: None,
        }
    }

    #[test]
    fn the_rows_match_section_5_9() {
        let rows = build_rows(&GameSettings::default(), Language::English);
        let ids: Vec<_> = rows.iter().map(|r| r.id).collect();
        assert_eq!(
            ids,
            [
                "sticksSection",
                "leftDeadzone",
                "rightDeadzone",
                "mouseSection",
                "sensitivity",
                "vertical",
                "curve",
                "offset",
                "maxSpeed",
                "smoothing",
                "decay",
                "keyboardSection",
                "modifier",
                "keyboardRamp",
                "triggerRamp",
                "graphicsSection",
                "shaderCache",
                "shareSection",
                "export",
                "import",
                "resetAdvanced"
            ]
        );
        assert!(!rows[0].kind().focusable(), "headers are not focusable");
    }

    #[test]
    fn defaults_display_like_the_screenshot() {
        let rows = build_rows(&GameSettings::default(), Language::English);
        let text = |id: &str| match &rows.iter().find(|r| r.id == id).unwrap().control {
            rows::Control::Stepper { text, .. } => text.clone(),
            _ => panic!("not a stepper"),
        };
        assert_eq!(text("leftDeadzone"), "0%");
        assert_eq!(text("sensitivity"), "1.0×");
        assert_eq!(text("curve"), "1.0");
        assert_eq!(text("offset"), "20%");
        assert_eq!(text("smoothing"), "10 ms");
        assert_eq!(text("decay"), "30 ms");
        assert_eq!(text("keyboardRamp"), "Off");
    }

    #[test]
    fn only_changed_values_are_kept_and_the_group_resets_to_none() {
        let mut s = GameSettings::default();
        assert!(apply_change(
            &mut s,
            &Change::Stepper {
                row: "sensitivity",
                value: 25
            }
        ));
        assert_eq!(s.tuning.unwrap().mouse_sensitivity, 25);
        // Back to the default: the whole group disappears again.
        assert!(apply_change(
            &mut s,
            &Change::Stepper {
                row: "sensitivity",
                value: 10
            }
        ));
        assert_eq!(s.tuning, None);
        // Values are clamped to the row's range.
        assert!(apply_change(
            &mut s,
            &Change::Stepper {
                row: "leftDeadzone",
                value: 500
            }
        ));
        assert_eq!(s.tuning.unwrap().left_deadzone, 90);
    }

    #[test]
    fn the_shader_cache_toggle_stores_false_only_when_off() {
        let mut s = GameSettings::default();
        assert!(apply_change(
            &mut s,
            &Change::Toggle {
                row: "shaderCache",
                on: false
            }
        ));
        assert_eq!(s.shader_cache, Some(false));
        assert!(apply_change(
            &mut s,
            &Change::Toggle {
                row: "shaderCache",
                on: true
            }
        ));
        assert_eq!(s.shader_cache, None, "on is the default");
    }

    #[test]
    fn reset_clears_tuning_and_cache() {
        let mut s = GameSettings::default();
        apply_change(
            &mut s,
            &Change::Stepper {
                row: "decay",
                value: 200,
            },
        );
        apply_change(
            &mut s,
            &Change::Toggle {
                row: "shaderCache",
                on: false,
            },
        );
        assert!(apply_change(
            &mut s,
            &Change::Run {
                row: "resetAdvanced"
            }
        ));
        assert_eq!((s.tuning, s.shader_cache), (None, None));
        assert!(
            !apply_change(
                &mut s,
                &Change::Run {
                    row: "resetAdvanced"
                }
            ),
            "nothing to reset"
        );
    }

    #[test]
    fn export_then_import_round_trips_and_drops_foreign_resolutions() {
        let dir = std::env::temp_dir().join(format!("anyps5-adv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Stray - AnyPlay.json");
        let stray = game("PPSA02100", "Stray");
        let settings = GameSettings {
            vsync: false,
            resolution: Some("2560x1440".into()),
            ..GameSettings::default()
        };
        export_settings(&file, &stray, &settings).unwrap();

        let same = import_settings(&file, &stray, &["2560x1440".to_owned()]).unwrap();
        assert_eq!(same.settings, settings);
        assert_eq!(same.other_game, None);

        let other = game("PPSA99999", "Other");
        let imported = import_settings(&file, &other, &[]).unwrap();
        assert_eq!(
            imported.settings.resolution, None,
            "this title has no such patch"
        );
        assert!(!imported.settings.vsync);
        assert_eq!(imported.other_game.as_deref(), Some("Stray"));

        std::fs::write(&file, "{\"format\":\"something-else\"}").unwrap();
        assert!(import_settings(&file, &stray, &[]).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

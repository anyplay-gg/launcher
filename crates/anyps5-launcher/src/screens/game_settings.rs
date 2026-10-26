//! The game's settings panel (the "..." menu, docs/DESIGN.md 5.9) and the uninstall
//! confirmation that shares its frame. Values live in the core's [`GameSettings`]; the panel edits
//! them in place, reports `changed` so the host saves, and returns [`GameSettingsAction`]s for
//! everything that is not a value (open the controls, reveal the folder, ...).

use anyps5_core::settings::{ConsoleLanguage, GameSettings, Overlay};
use egui::{Color32, Context};

use crate::screens::dialog;
use crate::screens::rows::{self, Change, Row, RowId};
use crate::screens::saves;
use crate::screens::share_strings::Share;
use crate::screens::side::{self, Frame, Leave, Output, SideState, SideView};
use crate::screens::strings::Extra;
use crate::screens::toolkit_pin::PinChoices;
use crate::ui::Shell;
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;

/// Windowed size range of the stepper (percent of the display).
pub const WINDOW_SIZE: (i32, i32) = (40, 100);
pub const WINDOW_SIZE_STEP: i32 = 5;

/// What the host has to do after a frame of the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameSettingsAction {
    /// The panel is closing (back, options).
    Close,
    OpenControls,
    OpenAdvanced,
    /// The recommended settings of this game, as a list of changes.
    OpenRecommended,
    /// Unlock the game again; the panel closes itself.
    Reinstall,
    ShowInExplorer,
    OpenLog,
    /// Build the report and show it, to save or to send to AnyPS5 on GitHub.
    ReportToUpstream,
    ClearShaderCache,
    /// Open the confirmation ([`UninstallPanel`]).
    AskUninstall,
    /// The backups of the game's saves.
    OpenSaves,
}

/// What the catalogue has for this game, for the row that opens it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecommendedRow {
    /// Settings that would change.
    pub changes: usize,
    /// Some change is needed for the game to start.
    pub needed: bool,
}

/// The game the panel is about, borrowed for one frame.
pub struct GameSettingsData<'a> {
    /// The panel title.
    pub title: &'a str,
    pub settings: &'a mut GameSettings,
    /// Resolutions the title's patch offers; empty hides the row.
    pub resolutions: &'a [String],
    /// The game is unlocked (reinstall and uninstall are offered).
    pub installed: bool,
    /// How many backups of the saves there are; `None` hides the row (nothing unlocked yet).
    pub saves: Option<usize>,
    /// Shown only when the catalogue has something for this game.
    pub recommended: Option<RecommendedRow>,
    /// Why unlocking again is offered ("Unlocked with v0.1.2. v0.1.3 is installed.").
    pub unlock_note: Option<String>,
    /// The releases the game can stay on (docs/DESIGN.md 3.8).
    pub pin: &'a PinChoices,
    pub language: Language,
    pub accent: Color32,
}

/// The game settings panel.
#[derive(Default)]
pub struct GameSettingsPanel {
    side: SideState,
}

impl GameSettingsPanel {
    /// Opens on the first row.
    pub fn open(&mut self) {
        self.side.open_at(0);
    }

    /// Opens on a row (previews).
    pub fn open_row(&mut self, row: usize) {
        self.side.open_at(row);
    }

    /// Opens on the row with this id (coming back from controls or advanced).
    pub fn open_on(&mut self, data: &GameSettingsData<'_>, id: RowId) {
        let rows = build_rows(data);
        self.side.open_at(rows::position(&rows, id).unwrap_or(0));
    }

    pub fn close(&mut self) {
        self.side.close();
    }

    /// Hands the frame to another panel of the family without a slide.
    pub fn dismiss(&mut self) {
        self.side.dismiss();
    }

    pub fn is_open(&self) -> bool {
        self.side.is_open()
    }

    pub fn is_active(&self) -> bool {
        self.side.is_active()
    }

    /// Handles the frame's commands, draws the panel and returns what the host must do. Call every
    /// frame; it draws nothing while the panel is closed.
    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &mut GameSettingsData<'_>,
    ) -> Output<GameSettingsAction> {
        let mut out = Output::default();
        let title = data.title;
        let frame = Frame {
            title,
            subtitle: Some(data.language.text(Key::GameSettings)),
            list_id: "game-settings-rows",
            accent: data.accent,
            language: data.language,
        };
        let mut changed = false;
        self.side.show_rows(
            shell,
            ctx,
            events,
            data,
            &frame,
            Leave::Slide,
            |data| build_rows(data),
            |side, _, data, change, actions| {
                changed |= Self::apply(side, data, change, actions);
            },
            GameSettingsAction::Close,
            &mut out.actions,
        );
        out.changed = changed;
        out
    }

    /// Writes one change and queues the action of a row that ran. Returns whether a value changed.
    fn apply(
        side: &mut SideState,
        data: &mut GameSettingsData<'_>,
        change: &Change,
        actions: &mut Vec<GameSettingsAction>,
    ) -> bool {
        let mut changed = apply_change(data.settings, change, data.resolutions);
        if let Change::Choice {
            row: "toolkit",
            index,
        } = change
        {
            let pin = data
                .pin
                .pin_at(data.settings.toolkit.as_deref(), *index, data.language);
            if pin != data.settings.toolkit {
                data.settings.toolkit = pin;
                changed = true;
            }
        }
        let action = match change.row() {
            "controls" => Some(GameSettingsAction::OpenControls),
            "advanced" => Some(GameSettingsAction::OpenAdvanced),
            "recommended" => Some(GameSettingsAction::OpenRecommended),
            "reinstall" => Some(GameSettingsAction::Reinstall),
            "finder" => Some(GameSettingsAction::ShowInExplorer),
            "log" => Some(GameSettingsAction::OpenLog),
            "upstream" => Some(GameSettingsAction::ReportToUpstream),
            "cache" => Some(GameSettingsAction::ClearShaderCache),
            "uninstall" => Some(GameSettingsAction::AskUninstall),
            "saves" => Some(GameSettingsAction::OpenSaves),
            _ => None,
        };
        if let (Change::Run { .. }, Some(action)) = (change, action) {
            if action == GameSettingsAction::Reinstall {
                side.close();
            }
            actions.push(action);
        }
        changed
    }
}

/// The rows of the panel for this game, in the order of section 5.9.
pub fn build_rows(data: &GameSettingsData<'_>) -> Vec<Row> {
    let t = |key: Key| data.language.text(key).to_owned();
    let s = &*data.settings;
    let caption = |id, key: Extra| Row::caption(id, key.text(data.language));
    let mut rows = vec![caption("screenCaption", Extra::GroupScreen)];
    if !data.resolutions.is_empty() {
        let mut options = vec![t(Key::ResolutionDefault)];
        options.extend(data.resolutions.iter().cloned());
        let selected = s
            .resolution
            .as_ref()
            .and_then(|r| data.resolutions.iter().position(|c| c == r))
            .map_or(0, |i| i + 1);
        rows.push(
            Row::choice("resolution", t(Key::Resolution), options, selected)
                .icon(Icon::FRAME_CORNERS),
        );
    }
    rows.push(
        Row::choice(
            "display",
            t(Key::Display),
            vec![t(Key::Windowed), t(Key::Fullscreen)],
            usize::from(s.fullscreen),
        )
        .icon(Icon::APP_WINDOW),
    );
    if !s.fullscreen {
        let value = s.window_size as i32;
        rows.push(
            Row::stepper(
                "size",
                t(Key::WindowSize),
                value,
                WINDOW_SIZE,
                WINDOW_SIZE_STEP,
                format!("{value}%"),
            )
            .icon(Icon::ARROWS_OUT_SIMPLE),
        );
    }
    rows.push(Row::toggle("vsync", t(Key::Vsync), s.vsync).icon(Icon::WAVE_SINE));
    // Groups, each under a caption: screen, play (recommended to toolkit), maintenance; the
    // removal stands alone at the foot.
    rows.push(caption("playCaption", Extra::GroupPlay).group_start());
    if let Some(r) = data.recommended {
        let detail = if r.needed {
            Extra::RecNeeded.text(data.language).to_owned()
        } else if r.changes == 0 {
            Extra::RecWorthALook.text(data.language).to_owned()
        } else if r.changes == 1 {
            Extra::ChangeOne.text(data.language).to_owned()
        } else {
            Extra::ChangesCount.format(data.language, &[Arg::Int(r.changes as i64)])
        };
        rows.push(
            Row::action("recommended", Extra::RecTitle.text(data.language))
                .detail(detail)
                .icon(Icon::SPARKLE),
        );
    }
    rows.push(Row::action("controls", t(Key::Controls)).icon(Icon::KEYBOARD));
    if let Some(count) = data.saves {
        rows.push(
            Row::action("saves", Extra::SavesTitle.text(data.language))
                .detail(saves::row_detail(count, data.language))
                .icon(Icon::FLOPPY_DISK),
        );
    }
    rows.push(
        Row::action("advanced", t(Key::Advanced))
            .detail(t(Key::AdvancedDetail))
            .icon(Icon::SLIDERS),
    );
    rows.push(
        Row::choice(
            "overlay",
            t(Key::Overlay),
            vec![
                t(Key::OverlayOff),
                t(Key::OverlayTopLeft),
                t(Key::OverlayTopRight),
                t(Key::OverlayBottomLeft),
                t(Key::OverlayBottomRight),
            ],
            Overlay::ALL
                .iter()
                .position(|o| *o == s.overlay)
                .unwrap_or(0),
        )
        .icon(Icon::GAUGE),
    );
    let mut languages = vec![t(Key::FollowLauncher)];
    languages.extend(
        ConsoleLanguage::ALL
            .iter()
            .map(|(_, name)| (*name).to_owned()),
    );
    let selected = s
        .language
        .and_then(|n| {
            ConsoleLanguage::ALL
                .iter()
                .position(|(number, _)| *number == n)
        })
        .map_or(0, |i| i + 1);
    rows.push(
        Row::choice("language", t(Key::GameLanguage), languages, selected).icon(Icon::TRANSLATE),
    );
    // Which toolkit the game is made and started with: the global one, or a release of its own.
    rows.extend(data.pin.row(s.toolkit.as_deref(), data.language));
    rows.push(caption("maintenanceCaption", Extra::GroupMaintenance).group_start());
    if data.installed {
        let mut reinstall =
            Row::command("reinstall", t(Key::Reinstall)).icon(Icon::ARROWS_CLOCKWISE);
        if let Some(note) = &data.unlock_note {
            reinstall = reinstall.detail(note.clone());
        }
        rows.push(reinstall);
    }
    rows.push(Row::command("finder", t(Key::ShowInFinder)).icon(Icon::FOLDER));
    rows.push(Row::command("log", t(Key::OpenLog)).icon(Icon::FILE_TEXT));
    rows.push(
        Row::command("upstream", Share::Report.text(data.language))
            .detail(Share::ReportDetail.text(data.language))
            .icon(Icon::STETHOSCOPE),
    );
    rows.push(Row::command("cache", t(Key::ClearShaderCache)).icon(Icon::BROOM));
    if data.installed {
        rows.push(
            Row::command("uninstall", t(Key::Uninstall))
                .icon(Icon::TRASH)
                .destructive()
                .group_start(),
        );
    }
    rows
}

/// Writes one change into the settings. Returns whether anything changed.
pub fn apply_change(settings: &mut GameSettings, change: &Change, resolutions: &[String]) -> bool {
    let before = settings.clone();
    match change {
        Change::Choice {
            row: "resolution",
            index,
        } => {
            settings.resolution = index
                .checked_sub(1)
                .and_then(|i| resolutions.get(i))
                .cloned();
        }
        Change::Choice {
            row: "display",
            index,
        } => settings.fullscreen = *index == 1,
        Change::Stepper { row: "size", value } => {
            settings.window_size = (*value).clamp(WINDOW_SIZE.0, WINDOW_SIZE.1) as u32;
        }
        Change::Toggle { row: "vsync", on } => settings.vsync = *on,
        Change::Choice {
            row: "overlay",
            index,
        } => {
            settings.overlay = Overlay::ALL.get(*index).copied().unwrap_or(Overlay::Off);
        }
        Change::Choice {
            row: "language",
            index,
        } => {
            settings.language = index
                .checked_sub(1)
                .and_then(|i| ConsoleLanguage::ALL.get(i))
                .map(|(number, _)| *number);
        }
        _ => {}
    }
    *settings != before
}

// ---------------------------------------------------------------------------------------------
// Uninstall confirmation
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UninstallAction {
    Confirm,
    Cancel,
}

/// "Uninstall" / "Cancel" in the panel frame.
#[derive(Default)]
pub struct UninstallPanel {
    side: SideState,
}

impl UninstallPanel {
    /// Opens on Cancel (the original opens on the destructive row): Enter must never remove
    /// an installation by accident.
    pub fn open(&mut self) {
        self.side.open_at(1);
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

    fn handle(&mut self, change: &Change, actions: &mut Vec<UninstallAction>) {
        if let Change::Run { row } = change {
            self.side.close();
            actions.push(if *row == "uninstall" {
                UninstallAction::Confirm
            } else {
                UninstallAction::Cancel
            });
        }
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        language: Language,
        accent: Color32,
    ) -> Vec<UninstallAction> {
        let mut actions = Vec::new();
        if !self.side.is_active() {
            return actions;
        }
        let build = || {
            vec![
                Row::command("uninstall", language.text(Key::Uninstall))
                    .icon(Icon::TRASH)
                    .destructive(),
                Row::command("cancel", language.text(Key::Cancel)).icon(Icon::X),
            ]
        };
        if self.side.is_open() {
            let rows = build();
            let driven = dialog::drive(shell, &mut self.side.nav, &rows, &side::commands(events));
            for change in &driven.changes {
                self.handle(change, &mut actions);
            }
            if driven.close {
                self.side.close();
                actions.push(UninstallAction::Cancel);
            }
        }
        let mut rows = build();
        // The saves go with the copy, so the question says they are kept first.
        let ask = format!(
            "{} {}",
            language.text(Key::UninstallConfirm),
            Extra::UninstallBacksUp.text(language)
        );
        let view = SideView {
            title: language.text(Key::Uninstall),
            subtitle: Some(&ask),
            question: true,
            list_id: "uninstall-rows",
            nav: &self.side.nav,
            accent,
            language,
            force: None,
        };
        match side::draw(ctx, self.side.is_open(), &view, &mut rows) {
            Some(drawn) if self.side.is_open() => {
                let (changes, _) =
                    rows::apply_mouse(shell, &mut self.side.nav, &rows, drawn.events);
                for change in &changes {
                    self.handle(change, &mut actions);
                }
                if drawn.back_clicked {
                    self.side.close();
                    actions.push(UninstallAction::Cancel);
                }
            }
            Some(_) => {}
            None => self.side.finished(),
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(settings: &mut GameSettings) -> GameSettingsData<'_> {
        GameSettingsData {
            title: "Test Game",
            settings,
            resolutions: &[],
            installed: true,
            saves: None,
            recommended: None,
            unlock_note: None,
            pin: &NO_PINS,
            language: Language::English,
            accent: Color32::WHITE,
        }
    }

    static NO_PINS: PinChoices = PinChoices {
        installed: Vec::new(),
        recent: Vec::new(),
        recommended: None,
    };

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.id).collect()
    }

    #[test]
    fn rows_follow_the_order_and_conditions_of_5_9() {
        let mut s = GameSettings::default();
        let rows = build_rows(&data(&mut s));
        assert_eq!(
            ids(&rows),
            [
                "screenCaption",
                "display",
                "size",
                "vsync",
                "playCaption",
                "controls",
                "advanced",
                "overlay",
                "language",
                "maintenanceCaption",
                "reinstall",
                "finder",
                "log",
                "upstream",
                "cache",
                "uninstall"
            ],
            "no resolution row without a patch, no trophies row without a list"
        );
    }

    #[test]
    fn the_save_data_row_sits_in_the_play_group_and_counts_the_backups() {
        let mut s = GameSettings::default();
        let mut d = data(&mut s);
        d.saves = Some(3);
        let rows = build_rows(&d);
        let ids = ids(&rows);
        let at = ids.iter().position(|i| *i == "saves").unwrap();
        assert_eq!((ids[at - 1], ids[at + 1]), ("controls", "advanced"));
        assert_eq!(rows[at].detail.as_deref(), Some("3 backups"));
        assert_eq!(rows[at].control, rows::Control::Action);
        d.saves = Some(0);
        let rows = build_rows(&d);
        let row = rows.iter().find(|r| r.id == "saves").unwrap();
        assert_eq!(row.detail.as_deref(), Some("No backups yet"));
    }

    #[test]
    fn the_recommended_row_and_the_unlock_note_appear_only_with_something_to_say() {
        let mut s = GameSettings::default();
        let mut d = data(&mut s);
        d.recommended = Some(RecommendedRow {
            changes: 2,
            needed: false,
        });
        d.unlock_note = Some("Unlocked with v0.1.2. v0.1.3 is installed.".into());
        let rows = build_rows(&d);
        let ids = ids(&rows);
        let recommended = ids.iter().position(|i| *i == "recommended").unwrap();
        assert_eq!(
            ids[recommended - 1],
            "playCaption",
            "the play group starts with it"
        );
        assert!(rows[recommended - 1].group_start && !rows[recommended].group_start);
        assert_eq!(rows[recommended].detail.as_deref(), Some("2 changes"));
        let reinstall = rows.iter().find(|r| r.id == "reinstall").unwrap();
        assert_eq!(
            reinstall.detail.as_deref(),
            Some("Unlocked with v0.1.2. v0.1.3 is installed.")
        );
        d.recommended = Some(RecommendedRow {
            changes: 1,
            needed: true,
        });
        let rows = build_rows(&d);
        let row = rows.iter().find(|r| r.id == "recommended").unwrap();
        assert_eq!(row.detail.as_deref(), Some("Needed to start"));
        d.recommended = None;
        d.unlock_note = None;
        let rows = build_rows(&d);
        assert!(rows.iter().all(|r| r.id != "recommended"));
        assert!(
            rows.iter()
                .find(|r| r.id == "playCaption")
                .unwrap()
                .group_start
        );
        assert!(
            rows.iter()
                .find(|r| r.id == "reinstall")
                .unwrap()
                .detail
                .is_none()
        );
    }

    #[test]
    fn conditional_rows_appear_and_disappear() {
        let mut s = GameSettings::default();
        let mut d = data(&mut s);
        let resolutions = vec!["1920x1080".to_owned(), "2560x1440".to_owned()];
        d.resolutions = &resolutions;
        d.installed = false;
        let rows = build_rows(&d);
        assert_eq!(rows[1].id, "resolution", "under the first caption");
        assert!(
            rows.iter().all(|r| r.id != "trophies"),
            "the trophy count lives beside Play, not here"
        );
        assert!(
            rows.iter()
                .all(|r| r.id != "reinstall" && r.id != "uninstall")
        );
        d.settings.fullscreen = true;
        assert!(
            build_rows(&d).iter().all(|r| r.id != "size"),
            "no size when fullscreen"
        );
    }

    #[test]
    fn values_land_in_the_settings() {
        let mut s = GameSettings::default();
        let res = vec!["1920x1080".to_owned()];
        assert!(apply_change(
            &mut s,
            &Change::Choice {
                row: "display",
                index: 1
            },
            &res
        ));
        assert!(s.fullscreen);
        assert!(apply_change(
            &mut s,
            &Change::Stepper {
                row: "size",
                value: 55
            },
            &res
        ));
        assert_eq!(s.window_size, 55);
        assert!(apply_change(
            &mut s,
            &Change::Toggle {
                row: "vsync",
                on: false
            },
            &res
        ));
        assert!(!s.vsync);
        assert!(apply_change(
            &mut s,
            &Change::Choice {
                row: "overlay",
                index: 2
            },
            &res
        ));
        assert_eq!(s.overlay, Overlay::TopRight);
        assert!(apply_change(
            &mut s,
            &Change::Choice {
                row: "language",
                index: 4
            },
            &res
        ));
        assert_eq!(s.language, Some(4), "Deutsch is console language 4");
        assert!(apply_change(
            &mut s,
            &Change::Choice {
                row: "language",
                index: 0
            },
            &res
        ));
        assert_eq!(s.language, None, "follow the launcher");
        assert!(apply_change(
            &mut s,
            &Change::Choice {
                row: "resolution",
                index: 1
            },
            &res
        ));
        assert_eq!(s.resolution.as_deref(), Some("1920x1080"));
        assert!(apply_change(
            &mut s,
            &Change::Choice {
                row: "resolution",
                index: 0
            },
            &res
        ));
        assert_eq!(s.resolution, None);
        assert!(
            !apply_change(
                &mut s,
                &Change::Choice {
                    row: "display",
                    index: 1
                },
                &res
            ) || s.fullscreen
        );
        assert!(
            !apply_change(&mut s, &Change::Run { row: "log" }, &res),
            "actions change no value"
        );
    }

    #[test]
    fn the_overlay_choice_lists_the_five_corners_in_core_order() {
        let mut s = GameSettings::default();
        let rows = build_rows(&data(&mut s));
        let overlay = rows.iter().find(|r| r.id == "overlay").unwrap();
        let rows::Control::Choice { options, .. } = &overlay.control else {
            panic!("overlay is a choice");
        };
        assert_eq!(options.len(), Overlay::ALL.len());
    }

    #[test]
    fn the_language_choice_lists_follow_plus_eighteen() {
        let mut s = GameSettings::default();
        let rows = build_rows(&data(&mut s));
        let language = rows.iter().find(|r| r.id == "language").unwrap();
        let rows::Control::Choice { options, selected } = &language.control else {
            panic!("language is a choice");
        };
        assert_eq!(options.len(), 19);
        assert_eq!(*selected, 0);
    }

    #[test]
    fn every_key_the_rows_use_exists_in_both_languages() {
        let mut s = GameSettings::default();
        for language in Language::ALL {
            let mut d = data(&mut s);
            d.language = language;
            for row in build_rows(&d) {
                assert!(!row.title.is_empty());
            }
        }
    }
}

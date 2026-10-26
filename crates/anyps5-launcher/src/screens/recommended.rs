//! "Recommended for this game": what the catalogue would change for one game, as a short list of
//! `current → recommended` and one button. Nothing is applied before that button; a warning
//! (`avoid`) is only text. Same frame as the other side panels.

use anyps5_core::recommended::{Caution, Change, Kind, Plan, Setting};
use anyps5_core::settings::ConsoleLanguage;
use egui::{Color32, Context};

use crate::screens::rows::{self, Row};
use crate::screens::side::{Frame, Leave, SideState};
use crate::screens::strings::Extra;
use crate::ui::Shell;
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecommendedAction {
    /// Write the changes into the game's settings.
    Apply,
    /// Back, or the scrim: return to the game settings.
    Close,
}

/// The game and its plan, borrowed for one frame.
pub struct RecommendedData<'a> {
    pub title: &'a str,
    pub plan: &'a Plan,
    pub language: Language,
    pub accent: Color32,
}

/// The side panel.
#[derive(Default)]
pub struct RecommendedPanel {
    side: SideState,
}

impl RecommendedPanel {
    /// Opens on the button.
    pub fn open(&mut self, plan: &Plan, language: Language) {
        let rows = build_rows(plan, language);
        self.side.open_at(rows.len().saturating_sub(1));
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

    fn handle(side: &mut SideState, change: &rows::Change, actions: &mut Vec<RecommendedAction>) {
        if let rows::Change::Run { row } = change {
            side.close();
            actions.push(if *row == "apply" {
                RecommendedAction::Apply
            } else {
                RecommendedAction::Close
            });
        }
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &RecommendedData<'_>,
    ) -> Vec<RecommendedAction> {
        let mut actions = Vec::new();
        let frame = Frame {
            title: data.title,
            subtitle: Some(Extra::RecTitle.text(data.language)),
            list_id: "recommended-rows",
            accent: data.accent,
            language: data.language,
        };
        self.side.show_rows(
            shell,
            ctx,
            events,
            &mut { data },
            &frame,
            Leave::Slide,
            |data| build_rows(data.plan, data.language),
            |side, _, _, change, actions| Self::handle(side, change, actions),
            RecommendedAction::Close,
            &mut actions,
        );
        actions
    }
}

/// The rows: the changes (needed ones first), the warnings, a hint when the game has to be
/// unlocked again, and last the button (or a way out when there is nothing to apply).
pub fn build_rows(plan: &Plan, language: Language) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut changes: Vec<&Change> = plan.changes.iter().collect();
    changes.sort_by_key(|c| c.kind != Kind::Required);
    // Each reason hangs under its line; the pairs are set apart by a double gap.
    for change in changes {
        let (label, from, to) = describe(change, language);
        let row = Row::info("change", label, Some(format!("{from} → {to}")));
        rows.push(if rows.is_empty() {
            row
        } else {
            row.group_start()
        });
        rows.push(Row::info("why", change.why.clone(), None).attached());
    }
    for caution in &plan.cautions {
        let (label, value) = describe_caution(caution, language);
        let row = Row::info("caution", label, Some(value));
        rows.push(if rows.is_empty() {
            row
        } else {
            row.group_start()
        });
        rows.push(Row::info("why", caution.why.clone(), None).attached());
    }
    if plan.needs_unlock() {
        rows.push(Row::info(
            "unlock",
            Extra::RecUnlockHint.text(language),
            None,
        ));
    }
    if plan.changes.is_empty() {
        rows.push(
            Row::command("close", language.text(Key::Close))
                .icon(Icon::X)
                .group_start(),
        );
    } else {
        rows.push(
            Row::command("apply", Extra::RecApply.text(language))
                .icon(Icon::CHECK)
                .group_start(),
        );
    }
    rows
}

fn on_off(value: &str, language: Language) -> String {
    match value {
        "on" => Extra::RecOn,
        _ => Extra::RecOff,
    }
    .text(language)
    .to_owned()
}

/// The setting's name and the two values, in words.
fn describe(change: &Change, language: Language) -> (String, String, String) {
    let t = |key: Key| language.text(key).to_owned();
    let default = Extra::RecDefault.text(language).to_owned();
    let (label, from, to) = match &change.setting {
        Setting::Env(key) if key == "APS5_PLAYGO_UNLISTED_CHUNKS" => {
            let word = |v: &str| match v {
                "installed" => Extra::RecPlayGoInstalled.text(language).to_owned(),
                _ => Extra::RecPlayGoAbsent.text(language).to_owned(),
            };
            (
                Extra::RecPlayGo.text(language).to_owned(),
                change.current.as_deref().map(word),
                word(&change.proposed),
            )
        }
        Setting::Env(key) => (key.clone(), change.current.clone(), change.proposed.clone()),
        Setting::Relink(flag) => {
            let label = match flag.as_str() {
                "--windows-fixed-base" => Extra::RecImageBase,
                "--module-dir" => Extra::RecModuleFolder,
                _ => Extra::RecExecutable,
            };
            (
                label.text(language).to_owned(),
                change.current.clone(),
                change.proposed.clone(),
            )
        }
        Setting::Vsync => (
            t(Key::Vsync),
            change.current.as_deref().map(|v| on_off(v, language)),
            on_off(&change.proposed, language),
        ),
        Setting::ShaderCache => (
            Extra::RecShaderCache.text(language).to_owned(),
            change.current.as_deref().map(|v| on_off(v, language)),
            on_off(&change.proposed, language),
        ),
        Setting::Overlay => {
            let word = |v: &str| match v {
                "tl" => t(Key::OverlayTopLeft),
                "tr" => t(Key::OverlayTopRight),
                "bl" => t(Key::OverlayBottomLeft),
                "br" => t(Key::OverlayBottomRight),
                _ => t(Key::OverlayOff),
            };
            (
                t(Key::Overlay),
                change.current.as_deref().map(word),
                word(&change.proposed),
            )
        }
        Setting::Language => {
            let word = |v: &str| {
                v.parse::<i32>()
                    .ok()
                    .and_then(|n| ConsoleLanguage::ALL.iter().find(|(number, _)| *number == n))
                    .map_or_else(|| v.to_owned(), |(_, name)| (*name).to_owned())
            };
            (
                t(Key::GameLanguage),
                Some(
                    change
                        .current
                        .as_deref()
                        .map_or_else(|| t(Key::FollowLauncher), word),
                ),
                word(&change.proposed),
            )
        }
    };
    (label, from.unwrap_or(default), to)
}

fn describe_caution(caution: &Caution, language: Language) -> (String, String) {
    match &caution.setting {
        Setting::Env(key) if key == "APS5_TEXTURE_CACHE_MIB" => (
            Extra::RecTextureCache.text(language).to_owned(),
            format!("{} MiB", caution.value),
        ),
        Setting::Env(key) | Setting::Relink(key) => (key.clone(), caution.value.clone()),
        _ => (String::new(), caution.value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(kind: Kind, setting: Setting, current: Option<&str>, proposed: &str) -> Change {
        Change {
            entry: "e".into(),
            kind,
            setting,
            current: current.map(str::to_owned),
            proposed: proposed.to_owned(),
            why: "Because.".into(),
        }
    }

    #[test]
    fn needed_changes_come_first_and_one_button_ends_the_list() {
        let plan = Plan {
            changes: vec![
                change(Kind::Recommended, Setting::Vsync, Some("on"), "off"),
                change(
                    Kind::Required,
                    Setting::Env("APS5_PLAYGO_UNLISTED_CHUNKS".into()),
                    None,
                    "installed",
                ),
            ],
            cautions: vec![],
        };
        let rows = build_rows(&plan, Language::English);
        let ids: Vec<_> = rows.iter().map(|r| r.id).collect();
        assert_eq!(ids, ["change", "why", "change", "why", "apply"]);
        assert_eq!(rows[0].title, "Missing PlayGo Data");
        assert_eq!(
            rows[0].control,
            rows::Control::Info(Some("Default → Report as Installed".into()))
        );
        assert_eq!(rows.iter().filter(|r| r.id == "apply").count(), 1);
    }

    #[test]
    fn a_warning_alone_has_no_apply_button() {
        let plan = Plan {
            changes: vec![],
            cautions: vec![Caution {
                entry: "e".into(),
                setting: Setting::Env("APS5_TEXTURE_CACHE_MIB".into()),
                value: "3072".into(),
                why: "Slower.".into(),
            }],
        };
        let rows = build_rows(&plan, Language::English);
        let ids: Vec<_> = rows.iter().map(|r| r.id).collect();
        assert_eq!(ids, ["caution", "why", "close"]);
        assert_eq!(rows[0].title, "Texture Cache");
        assert_eq!(
            rows[0].control,
            rows::Control::Info(Some("3072 MiB".into()))
        );
    }

    #[test]
    fn a_relinker_flag_says_the_game_must_be_unlocked_again() {
        let plan = Plan {
            changes: vec![change(
                Kind::Recommended,
                Setting::Relink("--windows-fixed-base".into()),
                None,
                "0x400000",
            )],
            cautions: vec![],
        };
        let ids: Vec<_> = build_rows(&plan, Language::Turkish)
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, ["change", "why", "unlock", "apply"]);
    }

    #[test]
    fn languages_are_named_and_the_launcher_default_is_said() {
        let plan = Plan {
            changes: vec![change(Kind::Recommended, Setting::Language, None, "4")],
            cautions: vec![],
        };
        let rows = build_rows(&plan, Language::English);
        let rows::Control::Info(Some(value)) = &rows[0].control else {
            panic!("an info row");
        };
        assert!(value.ends_with("→ Deutsch"), "{value}");
    }
}

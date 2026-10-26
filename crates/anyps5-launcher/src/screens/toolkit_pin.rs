//! A toolkit for one game (docs/DESIGN.md 3.8): the choice in the game sheet, the fact on the home
//! screen, the question when the release a game stays on is gone, and the texts of all three.
//! English and Turkish like the rest.

use std::cmp::Ordering;

use anyps5_core::compat;
use anyps5_core::toolkit::{GameToolkit, compare_tags, pin_tag};
use egui::{Color32, Context};

use crate::screens::rows::{self, Change, Row};
use crate::screens::side::{self, SideState, SideView};
use crate::screens::strings::Extra;
use crate::ui::Shell;
use crate::ui::i18n::{Arg, Key, Language, substitute};
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pin {
    /// Game sheet row.
    Title,
    /// First choice of the row, and a button of the question.
    FollowGlobal,
    /// Choice: `%@` = the release as `PinChoices` labels it.
    Recommended,
    /// A release that is not on this PC: `%@` = `v0.1.2`.
    NotInstalled,
    /// Fact of the game: `%@` = `v0.1.2`.
    Pinned,
    /// The question's sentence: `%@` = `v0.1.2`.
    MissingAsk,
    /// Button of the question: `%@` = `v0.1.2`.
    Download,
    /// Settings: `%@` = `v0.1.2`.
    Remove,
    /// Settings, under the release: `%@` = the size.
    RemoveDetail,
    /// Toast: `%@` = `v0.1.2`.
    Removed,
    RemoveFailed,
}

impl Pin {
    pub const fn english(self) -> &'static str {
        match self {
            Pin::Title => "Toolkit for This Game",
            Pin::FollowGlobal => "Follow Global Setting",
            Pin::Recommended => "Recommended: %@",
            Pin::NotInstalled => "%@ (not installed)",
            Pin::Pinned => "%@ (pinned)",
            Pin::MissingAsk => {
                "This game stays on toolkit %@, which is not installed. Download it again, or let the game follow the global setting."
            }
            Pin::Download => "Download %@",
            Pin::Remove => "Remove %@",
            Pin::RemoveDetail => "%@ · No game uses it",
            Pin::Removed => "%@ removed",
            Pin::RemoveFailed => "Could not remove the toolkit.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            Pin::Title => "Bu Oyun için Araç Seti",
            Pin::FollowGlobal => "Genel Ayarı İzle",
            Pin::Recommended => "Önerilen: %@",
            Pin::NotInstalled => "%@ (kurulu değil)",
            Pin::Pinned => "%@ (sabitlenmiş)",
            Pin::MissingAsk => {
                "Bu oyun %@ araç setinde kalıyor, ama kurulu değil. Yeniden indir ya da oyun genel ayarı izlesin."
            }
            Pin::Download => "%@ İndir",
            Pin::Remove => "%@ Kaldır",
            Pin::RemoveDetail => "%@ · Hiçbir oyun kullanmıyor",
            Pin::Removed => "%@ kaldırıldı",
            Pin::RemoveFailed => "Araç seti kaldırılamadı.",
        }
    }

    pub const fn text(self, language: Language) -> &'static str {
        match language {
            Language::Turkish => self.turkish(),
            Language::English => self.english(),
        }
    }

    pub fn format(self, language: Language, args: &[Arg<'_>]) -> String {
        substitute(self.text(language), args)
    }

    pub const ALL: [Pin; 11] = [
        Pin::Title,
        Pin::FollowGlobal,
        Pin::Recommended,
        Pin::NotInstalled,
        Pin::Pinned,
        Pin::MissingAsk,
        Pin::Download,
        Pin::Remove,
        Pin::RemoveDetail,
        Pin::Removed,
        Pin::RemoveFailed,
    ];
}

// ---------------------------------------------------------------------------------------------
// The choice in the game sheet
// ---------------------------------------------------------------------------------------------

/// What the game sheet offers to stay on. Tags are plain (`0.1.2`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PinChoices {
    /// The releases on this PC.
    pub installed: Vec<String>,
    /// The last releases of the configured repository (empty until fetched).
    pub recent: Vec<String>,
    /// The release the catalogue recommends for this game.
    pub recommended: Option<String>,
}

/// One entry of the choice: the release it pins (`None`: follow the global setting) and its words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinOption {
    pub tag: Option<String>,
    pub label: String,
}

impl PinChoices {
    /// The entries of the choice: follow the global setting; the recommended release when there is
    /// one; then the installed and the recent releases, newest first, and the one the game stays on
    /// even when it is in neither.
    pub fn options(&self, pinned: Option<&str>, language: Language) -> Vec<PinOption> {
        let pinned = pinned.and_then(pin_tag);
        let name = |tag: &str| {
            let plain = format!("v{tag}");
            if self.installed.iter().any(|t| t == tag) {
                plain
            } else {
                Pin::NotInstalled.format(language, &[Arg::Str(&plain)])
            }
        };
        let mut options = vec![PinOption {
            tag: None,
            label: Pin::FollowGlobal.text(language).to_owned(),
        }];
        let recommended = self.recommended.as_deref().and_then(pin_tag);
        if let Some(tag) = &recommended {
            options.push(PinOption {
                label: Pin::Recommended.format(language, &[Arg::Str(&name(tag))]),
                tag: Some(tag.clone()),
            });
        }
        let mut tags: Vec<String> = Vec::new();
        for tag in self
            .installed
            .iter()
            .chain(&self.recent)
            .filter_map(|t| pin_tag(t))
            .chain(pinned.clone())
        {
            if recommended.as_ref() != Some(&tag) && !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        tags.sort_by(|a, b| compare_tags(b, a).unwrap_or(Ordering::Equal));
        options.extend(tags.into_iter().map(|tag| PinOption {
            label: name(&tag),
            tag: Some(tag),
        }));
        options
    }

    /// The index of the game's own choice in [`options`](Self::options).
    pub fn selected(&self, pinned: Option<&str>, language: Language) -> usize {
        let pinned = pinned.and_then(pin_tag);
        self.options(pinned.as_deref(), language)
            .iter()
            .position(|o| o.tag == pinned)
            .unwrap_or(0)
    }

    /// The row of the game sheet. Without a second release to stay on there is nothing to choose,
    /// so there is no row.
    pub fn row(&self, pinned: Option<&str>, language: Language) -> Option<Row> {
        let options = self.options(pinned, language);
        if options.len() < 2 {
            return None;
        }
        let selected = self.selected(pinned, language);
        Some(
            Row::choice(
                "toolkit",
                Pin::Title.text(language),
                options.into_iter().map(|o| o.label).collect(),
                selected,
            )
            .icon(Icon::STACK),
        )
    }

    /// What the game stays on after the choice moved to `index`; `None` follows the global setting.
    pub fn pin_at(&self, pinned: Option<&str>, index: usize, language: Language) -> Option<String> {
        self.options(pinned, language)
            .into_iter()
            .nth(index)
            .and_then(|o| o.tag)
            .map(|tag| format!("v{tag}"))
    }
}

// ---------------------------------------------------------------------------------------------
// The fact on the home screen
// ---------------------------------------------------------------------------------------------

/// The value of the "Toolkit" fact of a game, `None` when there is no toolkit to name.
pub fn fact_value(toolkit: &GameToolkit, language: Language) -> Option<String> {
    match toolkit {
        GameToolkit::Global(None) => None,
        GameToolkit::Global(Some(version)) => Some(match compat::release_tag(version) {
            Some(tag) => format!("v{tag}"),
            None => Extra::ChannelLocal.text(language).to_owned(),
        }),
        GameToolkit::Pinned { tag, .. } => {
            Some(Pin::Pinned.format(language, &[Arg::Str(&format!("v{tag}"))]))
        }
        GameToolkit::Missing { tag } => {
            Some(Pin::NotInstalled.format(language, &[Arg::Str(&format!("v{tag}"))]))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The question
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingAction {
    /// Download the release again.
    Download,
    /// The game follows the global setting from now on (the pin is removed).
    Follow,
    Cancel,
}

/// What the question needs to know.
pub struct MissingData<'a> {
    /// The game's name.
    pub title: &'a str,
    /// The release the game stays on, plain (`0.1.2`).
    pub tag: &'a str,
    /// The release downloads now.
    pub downloading: bool,
    pub language: Language,
    pub accent: Color32,
}

fn missing_rows(data: &MissingData<'_>) -> Vec<Row> {
    let language = data.language;
    let name = format!("v{}", data.tag);
    let mut download = Row::command(
        "download",
        Pin::Download.format(language, &[Arg::Str(&name)]),
    )
    .icon(Icon::DOWNLOAD_SIMPLE);
    if data.downloading {
        download = download.detail(Extra::ToolkitDownloading.text(language));
    }
    vec![
        download,
        Row::command("follow", Pin::FollowGlobal.text(language)).icon(Icon::STACK),
        Row::command("cancel", language.text(Key::Cancel)).icon(Icon::X),
    ]
}

/// "Download v0.1.2" / "Follow Global Setting" / "Cancel" in the side panel frame.
#[derive(Default)]
pub struct MissingPanel {
    side: SideState,
}

impl MissingPanel {
    /// Opens on the download: it is the way on, and it changes nothing about the game.
    pub fn open(&mut self) {
        self.side.open_at(0);
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

    fn handle(&mut self, change: &Change, actions: &mut Vec<MissingAction>) {
        if let Change::Run { row } = change {
            let action = match *row {
                "download" => MissingAction::Download,
                "follow" => MissingAction::Follow,
                _ => MissingAction::Cancel,
            };
            // The download keeps the panel: it shows the progress and closes when the release is there.
            if action != MissingAction::Download {
                self.side.close();
            }
            actions.push(action);
        }
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &MissingData<'_>,
    ) -> Vec<MissingAction> {
        let mut actions = Vec::new();
        if !self.side.is_active() {
            return actions;
        }
        let language = data.language;
        if self.side.is_open() {
            let rows = missing_rows(data);
            let driven = rows::drive(shell, &mut self.side.nav, &rows, &side::commands(events));
            for change in &driven.changes {
                self.handle(change, &mut actions);
            }
            if driven.close {
                self.side.close();
                actions.push(MissingAction::Cancel);
            }
        }
        let mut rows = missing_rows(data);
        let ask = Pin::MissingAsk.format(language, &[Arg::Str(&format!("v{}", data.tag))]);
        let view = SideView {
            title: data.title,
            subtitle: Some(&ask),
            question: true,
            list_id: "toolkit-missing-rows",
            nav: &self.side.nav,
            accent: data.accent,
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
                    actions.push(MissingAction::Cancel);
                }
            }
            Some(_) => {}
            None => self.side.finished(),
        }
        actions
    }
}

// ---------------------------------------------------------------------------------------------
// Unused releases in the settings
// ---------------------------------------------------------------------------------------------

/// The rows that remove an installed release nothing uses. The row ids are fixed text, so the page
/// shows the first few; the next one follows when one is removed.
pub const REMOVE_ROWS: [&str; 6] = [
    "removeToolkit0",
    "removeToolkit1",
    "removeToolkit2",
    "removeToolkit3",
    "removeToolkit4",
    "removeToolkit5",
];

/// A release on this PC that nothing uses, with its size in bytes, for the settings.
pub fn remove_row(index: usize, tag: &str, bytes: u64, language: Language) -> Row {
    let name = format!("v{tag}");
    let size = crate::ui::i18n::byte_text(i64::try_from(bytes).unwrap_or(i64::MAX), language);
    let mut row = Row::command(
        REMOVE_ROWS[index],
        Pin::Remove.format(language, &[Arg::Str(&name)]),
    )
    .detail(Pin::RemoveDetail.format(language, &[Arg::Str(&size)]))
    .icon(Icon::TRASH);
    if index == 0 {
        row = row.group_start();
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: Language = Language::English;

    fn labels(options: &[PinOption]) -> Vec<&str> {
        options.iter().map(|o| o.label.as_str()).collect()
    }

    #[test]
    fn every_text_exists_in_both_languages_with_the_same_placeholders() {
        for pin in Pin::ALL {
            for language in Language::ALL {
                assert!(!pin.text(language).is_empty(), "{pin:?} {language:?}");
            }
            assert_eq!(
                pin.english().matches("%@").count(),
                pin.turkish().matches("%@").count(),
                "{pin:?}"
            );
        }
    }

    #[test]
    fn the_choice_starts_with_following_and_lists_releases_newest_first() {
        let choices = PinChoices {
            installed: vec!["0.1.3".into(), "0.1.1".into()],
            recent: vec!["v0.1.3".into(), "v0.1.2".into(), "v0.1.1".into()],
            recommended: None,
        };
        let options = choices.options(None, EN);
        assert_eq!(
            labels(&options),
            [
                "Follow Global Setting",
                "v0.1.3",
                "v0.1.2 (not installed)",
                "v0.1.1"
            ]
        );
        assert_eq!(options[0].tag, None);
        assert_eq!(options[2].tag.as_deref(), Some("0.1.2"));
        assert_eq!(choices.selected(None, EN), 0);
        assert_eq!(choices.selected(Some("v0.1.1"), EN), 3);
        assert_eq!(choices.pin_at(None, 1, EN).as_deref(), Some("v0.1.3"));
        assert_eq!(choices.pin_at(Some("v0.1.1"), 0, EN), None);
    }

    #[test]
    fn the_recommended_release_comes_second_and_is_not_listed_twice() {
        let choices = PinChoices {
            installed: vec!["0.1.3".into()],
            recent: vec!["v0.1.3".into(), "v0.1.2".into()],
            recommended: Some("v0.1.2".into()),
        };
        let options = choices.options(None, EN);
        assert_eq!(
            labels(&options),
            [
                "Follow Global Setting",
                "Recommended: v0.1.2 (not installed)",
                "v0.1.3"
            ]
        );
        // Choosing it pins the release; the game then shows it as its own choice.
        assert_eq!(choices.pin_at(None, 1, EN).as_deref(), Some("v0.1.2"));
        assert_eq!(choices.selected(Some("v0.1.2"), EN), 1);
    }

    #[test]
    fn a_pin_that_is_in_no_list_stays_visible() {
        let choices = PinChoices {
            installed: vec!["0.1.3".into()],
            ..Default::default()
        };
        let options = choices.options(Some("v0.0.9"), EN);
        assert_eq!(options.last().unwrap().label, "v0.0.9 (not installed)");
        assert_eq!(choices.selected(Some("v0.0.9"), EN), 2);
        // A pin that is not a release is nothing.
        assert_eq!(choices.options(Some("../x"), EN).len(), 2);
    }

    #[test]
    fn there_is_no_row_when_there_is_nothing_to_choose() {
        assert!(PinChoices::default().row(None, EN).is_none());
        let one = PinChoices {
            installed: vec!["0.1.3".into()],
            ..Default::default()
        };
        assert!(one.row(None, EN).is_some());
    }

    #[test]
    fn the_fact_names_the_toolkit_and_whether_it_is_pinned() {
        assert_eq!(fact_value(&GameToolkit::Global(None), EN), None);
        assert_eq!(
            fact_value(&GameToolkit::Global(Some("0.1.3-abc".into())), EN).as_deref(),
            Some("v0.1.3")
        );
        assert_eq!(
            fact_value(&GameToolkit::Global(Some("local-abc".into())), EN).as_deref(),
            Some("Local Build")
        );
        let pinned = GameToolkit::Pinned {
            tag: "0.1.2".into(),
            version: "0.1.2-abc".into(),
        };
        assert_eq!(fact_value(&pinned, EN).as_deref(), Some("v0.1.2 (pinned)"));
        let missing = GameToolkit::Missing {
            tag: "0.1.2".into(),
        };
        assert_eq!(
            fact_value(&missing, EN).as_deref(),
            Some("v0.1.2 (not installed)")
        );
    }

    #[test]
    fn the_question_offers_three_ways_and_opens_on_the_download() {
        let data = MissingData {
            title: "Game",
            tag: "0.1.2",
            downloading: false,
            language: EN,
            accent: Color32::WHITE,
        };
        let rows = missing_rows(&data);
        let ids: Vec<_> = rows.iter().map(|r| r.id).collect();
        assert_eq!(ids, ["download", "follow", "cancel"]);
        assert_eq!(rows[0].title, "Download v0.1.2");
    }
}

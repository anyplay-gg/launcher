//! The quick panel of the running game (docs/DESIGN.md 6.6): what the Home button of a controller
//! or the phone opens in front of the game. A side panel of the usual family with five rows and a
//! confirmation before the game is closed. English and Turkish like the rest.

use egui::Color32;
use egui::Context;

use crate::screens::rows::{self, Change, Row};
use crate::screens::side::{self, SideState, SideView};
use crate::ui::Shell;
use crate::ui::i18n::{Arg, Key, Language, substitute};
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InGameText {
    /// Settings row, and what it does.
    Setting,
    SettingDetail,
    /// The line under the game's name.
    Running,
    BackToGame,
    Friends,
    /// The row that asks, and the title of the question.
    CloseGame,
    /// The question's sentence: `%@` = the game.
    CloseAsk,
    KeepPlaying,
}

impl InGameText {
    pub const fn english(self) -> &'static str {
        match self {
            InGameText::Setting => "Home Button Opens the Launcher",
            InGameText::SettingDetail => {
                "While a game runs, the Home button on your controller or phone brings the launcher in front of it. Press it again to go back to the game."
            }
            InGameText::Running => "The game keeps running behind the launcher.",
            InGameText::BackToGame => "Back to Game",
            InGameText::Friends => "Friends and Party",
            InGameText::CloseGame => "Close Game",
            InGameText::CloseAsk => "Close %@? Anything the game has not saved is lost.",
            InGameText::KeepPlaying => "Keep Playing",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            InGameText::Setting => "Ana Düğme Başlatıcıyı Açar",
            InGameText::SettingDetail => {
                "Bir oyun açıkken denetleyicideki ya da telefondaki Ana Sayfa düğmesi başlatıcıyı oyunun önüne getirir. Oyuna dönmek için tekrar bas."
            }
            InGameText::Running => "Oyun, başlatıcının arkasında çalışmaya devam ediyor.",
            InGameText::BackToGame => "Oyuna Dön",
            InGameText::Friends => "Arkadaşlar ve Parti",
            InGameText::CloseGame => "Oyunu Kapat",
            InGameText::CloseAsk => "%@ kapatılsın mı? Oyunun kaydetmediği ilerleme kaybolur.",
            InGameText::KeepPlaying => "Oynamaya Devam Et",
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

    pub const ALL: [InGameText; 8] = [
        InGameText::Setting,
        InGameText::SettingDetail,
        InGameText::Running,
        InGameText::BackToGame,
        InGameText::Friends,
        InGameText::CloseGame,
        InGameText::CloseAsk,
        InGameText::KeepPlaying,
    ];
}

/// What a row of the panel asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InGameAction {
    /// Back to the game's window.
    Resume,
    Trophies,
    GameSettings,
    Friends,
    /// Ask whether to close the game.
    AskClose,
    /// The panel was dismissed with Back or a click beside it: back to the game as well.
    Dismiss,
    /// The question: close the game now.
    Close,
    /// The question: keep playing, back to the panel.
    Keep,
}

/// The game the panel is about.
pub struct InGameData<'a> {
    pub title: &'a str,
    pub language: Language,
    pub accent: Color32,
    /// The player is signed in: the friends row shows.
    pub online: bool,
}

fn rows_of(data: &InGameData<'_>) -> Vec<Row> {
    let language = data.language;
    let mut rows = vec![
        Row::command("resume", InGameText::BackToGame.text(language)).icon(Icon::PLAY_CIRCLE),
        Row::command("trophies", language.text(Key::Trophies)).icon(Icon::TROPHY),
        Row::command("settings", language.text(Key::GameSettings)).icon(Icon::SLIDERS_HORIZONTAL),
    ];
    if data.online {
        rows.push(Row::command("friends", InGameText::Friends.text(language)).icon(Icon::USERS));
    }
    rows.push(
        Row::command("close", InGameText::CloseGame.text(language))
            .icon(Icon::POWER)
            .destructive(),
    );
    rows
}

/// The panel in the frame of the side family.
#[derive(Default)]
pub struct InGamePanel {
    side: SideState,
}

impl InGamePanel {
    /// Opens on "Back to Game": the Home button pressed twice is the way back.
    pub fn open(&mut self) {
        self.side.open_at(0);
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

    fn handle(&mut self, change: &Change, actions: &mut Vec<InGameAction>) {
        let Change::Run { row } = change else {
            return;
        };
        let action = match *row {
            "resume" => InGameAction::Resume,
            "trophies" => InGameAction::Trophies,
            "settings" => InGameAction::GameSettings,
            "friends" => InGameAction::Friends,
            _ => InGameAction::AskClose,
        };
        // The pages and the question take the frame over without a slide; only the way back to
        // the game slides out.
        if action == InGameAction::Resume {
            self.side.close();
        }
        actions.push(action);
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &InGameData<'_>,
    ) -> Vec<InGameAction> {
        let mut actions = Vec::new();
        if !self.side.is_active() {
            return actions;
        }
        if self.side.is_open() {
            let rows = rows_of(data);
            let driven = rows::drive(shell, &mut self.side.nav, &rows, &side::commands(events));
            for change in &driven.changes {
                self.handle(change, &mut actions);
            }
            if driven.close {
                self.side.close();
                actions.push(InGameAction::Dismiss);
            }
        }
        let mut rows = rows_of(data);
        let running = InGameText::Running.text(data.language);
        let view = SideView {
            title: data.title,
            subtitle: Some(running),
            question: false,
            list_id: "in-game-rows",
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
                    self.handle(change, &mut actions);
                }
                if drawn.scrim_clicked {
                    self.side.close();
                    actions.push(InGameAction::Dismiss);
                }
            }
            Some(_) => {}
            None => self.side.finished(),
        }
        actions
    }
}

/// "Close Game" / "Keep Playing".
#[derive(Default)]
pub struct CloseGamePanel {
    side: SideState,
}

impl CloseGamePanel {
    /// Opens on "Keep Playing": Enter must never end a game by accident.
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

    fn handle(&mut self, change: &Change, actions: &mut Vec<InGameAction>) {
        if let Change::Run { row } = change {
            self.side.close();
            actions.push(if *row == "close" {
                InGameAction::Close
            } else {
                InGameAction::Keep
            });
        }
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        title: &str,
        language: Language,
        accent: Color32,
    ) -> Vec<InGameAction> {
        let mut actions = Vec::new();
        if !self.side.is_active() {
            return actions;
        }
        let build = || {
            vec![
                Row::command("close", InGameText::CloseGame.text(language))
                    .icon(Icon::POWER)
                    .destructive(),
                Row::command("keep", InGameText::KeepPlaying.text(language)).icon(Icon::PLAY),
            ]
        };
        if self.side.is_open() {
            let rows = build();
            let driven = rows::drive(shell, &mut self.side.nav, &rows, &side::commands(events));
            for change in &driven.changes {
                self.handle(change, &mut actions);
            }
            if driven.close {
                self.side.close();
                actions.push(InGameAction::Keep);
            }
        }
        let mut rows = build();
        let ask = InGameText::CloseAsk.format(language, &[Arg::Str(title)]);
        let view = SideView {
            title: InGameText::CloseGame.text(language),
            subtitle: Some(&ask),
            question: true,
            list_id: "close-game-rows",
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
                if drawn.scrim_clicked {
                    self.side.close();
                    actions.push(InGameAction::Keep);
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

    #[test]
    fn every_text_exists_in_both_languages() {
        for text in InGameText::ALL {
            for language in Language::ALL {
                assert!(!text.text(language).is_empty(), "{text:?} {language:?}");
            }
        }
    }

    #[test]
    fn the_names_are_ours() {
        for text in InGameText::ALL {
            for language in Language::ALL {
                let shown = text.text(language);
                assert!(
                    !shown.contains("PlayStation") && !shown.contains("PSN"),
                    "{shown}"
                );
            }
        }
    }

    #[test]
    fn the_question_names_the_game() {
        let ask = InGameText::CloseAsk.format(Language::English, &[Arg::Str("Astro")]);
        assert!(ask.starts_with("Close Astro?"), "{ask}");
    }

    #[test]
    fn the_friends_row_needs_an_account() {
        let mut data = InGameData {
            title: "Astro",
            language: Language::English,
            accent: Color32::WHITE,
            online: false,
        };
        assert_eq!(rows_of(&data).len(), 4);
        data.online = true;
        assert_eq!(rows_of(&data).len(), 5);
    }
}

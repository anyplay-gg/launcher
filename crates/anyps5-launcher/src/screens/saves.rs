//! "Save Data": the backups of one game's saves and trophies, and the confirmation that comes
//! before one is put back. Both are side panels on the game sheet's frame. The panel lists the
//! backups, newest first, under one command; selecting a backup asks, and only the answer to that
//! question replaces anything.

use anyps5_core::saves::{Backup, Reason};
use egui::{Color32, Context};

use crate::app::clock;
use crate::screens::rows::{self, Row};
use crate::screens::side::{self, SideState, SideView};
use crate::screens::strings::Extra;
use crate::ui::Shell;
use crate::ui::i18n::{Arg, Key, Language, byte_text};
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavesAction {
    /// Back up the saves now.
    BackUp,
    /// The backup at this position of the list was selected: ask before restoring it.
    Ask(usize),
    /// Back, or the scrim: return to the game sheet.
    Close,
}

/// The game and its backups, borrowed for one frame.
pub struct SavesData<'a> {
    pub title: &'a str,
    /// Newest first.
    pub backups: &'a [Backup],
    /// The game has saves or trophies to back up.
    pub has_saves: bool,
    /// The game runs: a backup can be made but not put back.
    pub running: bool,
    /// How many backups are kept.
    pub keep: usize,
    pub language: Language,
    pub accent: Color32,
}

/// The side panel.
#[derive(Default)]
pub struct SavesPanel {
    side: SideState,
}

impl SavesPanel {
    /// Opens on the command.
    pub fn open(&mut self) {
        self.side.open_at(0);
    }

    /// Opens on the backup at `index` (coming back from the question about it).
    pub fn open_backup(&mut self, data: &SavesData<'_>, index: usize) {
        let first = rows::position(&build_rows(data), "restore").unwrap_or(0);
        self.side
            .open_at(first + index.min(data.backups.len().saturating_sub(1)));
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

    fn handle(
        &mut self,
        change: &rows::Change,
        data: &SavesData<'_>,
        actions: &mut Vec<SavesAction>,
    ) {
        let rows::Change::Run { row } = change else {
            return;
        };
        match *row {
            "backup" => actions.push(SavesAction::BackUp),
            "restore" => {
                let rows = build_rows(data);
                let first = rows::position(&rows, "restore").unwrap_or(0);
                let index = self.side.nav.row.saturating_sub(first);
                if index < data.backups.len() {
                    actions.push(SavesAction::Ask(index));
                }
            }
            _ => {}
        }
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &SavesData<'_>,
    ) -> Vec<SavesAction> {
        let mut actions = Vec::new();
        if !self.side.is_active() {
            return actions;
        }
        if self.side.is_open() {
            let rows = build_rows(data);
            // One command at a time: a change names the row that was focused when it ran.
            for command in side::commands(events) {
                let driven = rows::drive(shell, &mut self.side.nav, &rows, &[command]);
                for change in &driven.changes {
                    self.handle(change, data, &mut actions);
                }
                if driven.close {
                    self.side.close();
                    actions.push(SavesAction::Close);
                    break;
                }
            }
        }
        let mut rows = build_rows(data);
        let view = SideView {
            title: data.title,
            subtitle: Some(Extra::SavesTitle.text(data.language)),
            question: false,
            list_id: "saves-rows",
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
                    self.handle(change, data, &mut actions);
                }
                if drawn.back_clicked {
                    shell.play(crate::ui::sound::Cue::Close);
                    self.side.close();
                    actions.push(SavesAction::Close);
                }
            }
            Some(_) => {}
            None => self.side.finished(),
        }
        actions
    }
}

/// What made a backup, in a few words.
pub fn reason_text(reason: Reason, language: Language) -> &'static str {
    match reason {
        Reason::Manual => Extra::SavesReasonManual,
        Reason::BeforeUnlock => Extra::SavesReasonUnlock,
        Reason::BeforeRestore => Extra::SavesReasonRestore,
        Reason::BeforeUninstall => Extra::SavesReasonUninstall,
        Reason::AfterPlaying => Extra::SavesReasonPlayed,
    }
    .text(language)
}

/// The date of a backup as the lists show it (local time, like the last toolkit check).
pub fn stamp(backup: &Backup) -> String {
    clock::local_stamp(backup.created)
}

/// What the game sheet's row says: how many backups there are.
pub fn row_detail(count: usize, language: Language) -> String {
    match count {
        0 => Extra::SavesNone.text(language).to_owned(),
        1 => Extra::SavesOne.text(language).to_owned(),
        n => Extra::SavesCount.format(language, &[Arg::Int(n as i64)]),
    }
}

/// The rows: the command, then the backups (or a note that there are none), then what is kept.
pub fn build_rows(data: &SavesData<'_>) -> Vec<Row> {
    let language = data.language;
    let mut rows = Vec::new();
    let mut backup =
        Row::command("backup", Extra::SavesBackUp.text(language)).icon(Icon::FLOPPY_DISK);
    if !data.has_saves {
        backup = backup.detail(Extra::SavesNothing.text(language));
    }
    rows.push(backup);
    if data.backups.is_empty() {
        rows.push(Row::info("empty", Extra::SavesEmpty.text(language), None).group_start());
        return rows;
    }
    rows.push(Row::caption("caption", Extra::SavesCaption.text(language)).group_start());
    if data.running {
        rows.push(Row::info(
            "running",
            Extra::SavesRunning.text(language),
            None,
        ));
    }
    for backup in data.backups {
        let detail = format!(
            "{} · {}",
            byte_text(backup.bytes as i64, language),
            reason_text(backup.reason, language)
        );
        rows.push(
            Row::action("restore", stamp(backup))
                .detail(detail)
                .icon(Icon::CLOCK_COUNTER_CLOCKWISE),
        );
    }
    rows.push(
        Row::info(
            "kept",
            Extra::SavesKept.format(language, &[Arg::Int(data.keep as i64)]),
            None,
        )
        .group_start(),
    );
    rows
}

// ---------------------------------------------------------------------------------------------
// The question before a restore
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreAction {
    Confirm,
    Cancel,
}

/// "Restore" / "Cancel" in the side panel frame, like the removal of a game.
#[derive(Default)]
pub struct RestorePanel {
    side: SideState,
}

impl RestorePanel {
    /// Opens on Cancel: Enter must never replace saves by accident.
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

    fn handle(&mut self, change: &rows::Change, actions: &mut Vec<RestoreAction>) {
        if let rows::Change::Run { row } = change {
            self.side.close();
            actions.push(if *row == "restore" {
                RestoreAction::Confirm
            } else {
                RestoreAction::Cancel
            });
        }
    }

    /// `stamp` is the date of the backup that would be put back.
    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        stamp: &str,
        language: Language,
        accent: Color32,
    ) -> Vec<RestoreAction> {
        let mut actions = Vec::new();
        if !self.side.is_active() {
            return actions;
        }
        let build = || confirm_rows(language);
        if self.side.is_open() {
            let rows = build();
            let driven = rows::drive(shell, &mut self.side.nav, &rows, &side::commands(events));
            for change in &driven.changes {
                self.handle(change, &mut actions);
            }
            if driven.close {
                self.side.close();
                actions.push(RestoreAction::Cancel);
            }
        }
        let mut rows = build();
        let ask = Extra::SavesRestoreAsk.format(language, &[Arg::Str(stamp)]);
        let view = SideView {
            title: Extra::SavesRestore.text(language),
            subtitle: Some(&ask),
            question: true,
            list_id: "restore-rows",
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
                    actions.push(RestoreAction::Cancel);
                }
            }
            Some(_) => {}
            None => self.side.finished(),
        }
        actions
    }
}

pub fn confirm_rows(language: Language) -> Vec<Row> {
    vec![
        Row::command("restore", Extra::SavesRestore.text(language))
            .icon(Icon::ARROW_COUNTER_CLOCKWISE)
            .destructive(),
        Row::command("cancel", language.text(Key::Cancel)).icon(Icon::X),
    ]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn backup(created: i64, reason: Reason, bytes: u64) -> Backup {
        Backup {
            path: PathBuf::from(format!("{created}.zip")),
            created,
            reason,
            bytes,
        }
    }

    fn data<'a>(backups: &'a [Backup], has_saves: bool, running: bool) -> SavesData<'a> {
        SavesData {
            title: "Game",
            backups,
            has_saves,
            running,
            keep: 10,
            language: Language::English,
            accent: Color32::WHITE,
        }
    }

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.id).collect()
    }

    #[test]
    fn without_backups_the_panel_says_what_the_command_does() {
        let rows = build_rows(&data(&[], true, false));
        assert_eq!(ids(&rows), ["backup", "empty"]);
        assert_eq!(rows[0].control, rows::Control::Command);
        assert!(rows[0].detail.is_none());
        assert!(rows[1].title.contains("Back Up Now"));
    }

    #[test]
    fn a_game_without_saves_says_so_on_the_command() {
        let rows = build_rows(&data(&[], false, false));
        assert_eq!(
            rows[0].detail.as_deref(),
            Some("No save data to back up yet")
        );
    }

    #[test]
    fn backups_are_rows_that_open_the_question_newest_first() {
        let list = [
            backup(2_000, Reason::BeforeRestore, 12_300_000),
            backup(1_000, Reason::Manual, 900),
        ];
        let rows = build_rows(&data(&list, true, false));
        assert_eq!(
            ids(&rows),
            ["backup", "caption", "restore", "restore", "kept"]
        );
        assert_eq!(rows[2].control, rows::Control::Action);
        assert_eq!(
            rows[2].detail.as_deref(),
            Some("12.3 MB · Before a restore")
        );
        assert_eq!(rows[3].detail.as_deref(), Some("900 bytes · Manual"));
        assert_eq!(
            rows[4].title,
            "Up to 10 backups are kept. Backups made after playing go first."
        );
    }

    #[test]
    fn a_running_game_gets_a_note_instead_of_a_refusal_later() {
        let list = [backup(1_000, Reason::Manual, 900)];
        let rows = build_rows(&data(&list, true, true));
        assert!(ids(&rows).contains(&"running"));
        assert!(!ids(&build_rows(&data(&list, true, false))).contains(&"running"));
    }

    #[test]
    fn the_question_opens_on_cancel_and_the_destructive_row_is_the_restore() {
        let rows = confirm_rows(Language::English);
        assert_eq!(ids(&rows), ["restore", "cancel"]);
        assert!(rows[0].destructive && !rows[1].destructive);
        let mut panel = RestorePanel::default();
        panel.open();
        assert_eq!(panel.side.nav.row, 1);
    }

    #[test]
    fn the_sheet_row_counts_the_backups() {
        let text = |n| row_detail(n, Language::English);
        assert_eq!(text(0), "No backups yet");
        assert_eq!(text(1), "1 backup");
        assert_eq!(text(7), "7 backups");
        assert_eq!(row_detail(3, Language::Turkish), "3 yedek");
    }
}

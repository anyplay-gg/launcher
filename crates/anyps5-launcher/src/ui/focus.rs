//! Focus model and navigation (docs/DESIGN.md 6.4), pure logic without any egui types so the
//! screens drive it from [`Command`]s and draw focus from its state.
//!
//! * [`HomeFocus`] with [`HomeFocus::navigate`]: the three zones of the home screen.
//! * [`PanelNav`] with [`PanelNav::command`]: the two-level settings panel and the single-level
//!   side panels (`panelCommand`).
//!
//! Every transition also says which sound cue to play, so screens stay free of that rule.

use crate::ui::input::Command;
use crate::ui::sound::Cue;

// ---------------------------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------------------------

/// Where focus sits on the home screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HomeFocus {
    /// Top bar, left to right: [`HomeFocus::LIBRARY`], [`HomeFocus::FRIENDS`],
    /// [`HomeFocus::SETTINGS`], [`HomeFocus::PROFILE`].
    Top(u8),
    /// The tile row.
    #[default]
    Tiles,
    /// The hero action row: 0 = primary, then trophies if present, last = "...".
    Actions(usize),
    /// "How did it run?" below the facts: the four answers (0 to 3), then dismiss (4).
    Ask(usize),
}

/// What the home screen needs to know to navigate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HomeContext {
    /// Number of games in the library.
    pub games: usize,
    /// Index of the selected game.
    pub selected: usize,
    /// The selected game has a trophy list (a trophy button sits between Play and "...").
    pub has_trophies: bool,
    /// "How did it run?" is on screen.
    pub asking: bool,
}

impl HomeContext {
    /// Number of buttons in the action row: primary, trophies (optional), "...".
    pub fn action_count(&self) -> usize {
        if self.has_trophies { 3 } else { 2 }
    }
}

/// What a command on the home screen leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeIntent {
    /// Run the selected game's primary action (play / unlock / retry / cancel / activate window).
    PrimaryAction,
    /// Open the trophy list.
    OpenTrophies,
    /// Open the game settings side panel.
    OpenGameSettings,
    /// Answer "How did it run?" (0 to 3) or dismiss it (4).
    AnswerRun(usize),
    OpenLibrary,
    OpenSettings,
    /// The friends button: the online area on the friends list, or on sign-in while signed out.
    OpenFriends,
    /// The profile: sign-in, or the account's online area.
    OpenOnline,
    /// Select with no games: open the games folder dialog.
    ChooseFolder,
}

/// Result of one command on the home screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HomeStep {
    pub focus: HomeFocus,
    /// New selected game index (clamped, no wrap).
    pub selected: usize,
    pub intent: Option<HomeIntent>,
    /// Every focus change plays `move`; an intent plays its own cue (the screen decides).
    pub sound: Option<Cue>,
}

impl HomeFocus {
    pub const LIBRARY: HomeFocus = HomeFocus::Top(0);
    pub const FRIENDS: HomeFocus = HomeFocus::Top(1);
    pub const SETTINGS: HomeFocus = HomeFocus::Top(2);
    pub const PROFILE: HomeFocus = HomeFocus::Top(3);
    /// The last stop of the top bar.
    const LAST_TOP: u8 = 3;

    /// What a press on stop `index` of the top bar does.
    pub fn top_intent(index: u8) -> HomeIntent {
        match index {
            0 => HomeIntent::OpenLibrary,
            1 => HomeIntent::OpenFriends,
            2 => HomeIntent::OpenSettings,
            _ => HomeIntent::OpenOnline,
        }
    }

    /// Applies a command.
    pub fn navigate(self, command: Command, ctx: HomeContext) -> HomeStep {
        let last_game = ctx.games.saturating_sub(1);
        let mut step = HomeStep {
            focus: self,
            selected: ctx.selected.min(last_game),
            intent: None,
            sound: None,
        };
        let has_games = ctx.games > 0;
        let go = |step: &mut HomeStep, focus: HomeFocus| {
            if step.focus != focus {
                step.focus = focus;
                step.sound = Some(Cue::Move);
            }
        };
        match self {
            HomeFocus::Tiles => match command {
                Command::Left if step.selected > 0 => {
                    step.selected -= 1;
                    step.sound = Some(Cue::Move);
                }
                Command::Right if step.selected < last_game => {
                    step.selected += 1;
                    step.sound = Some(Cue::Move);
                }
                Command::Up => go(&mut step, HomeFocus::SETTINGS),
                Command::Down if has_games => go(&mut step, HomeFocus::Actions(0)),
                Command::Select => {
                    step.intent = Some(if has_games {
                        HomeIntent::PrimaryAction
                    } else {
                        HomeIntent::ChooseFolder
                    });
                }
                Command::Options if has_games => step.intent = Some(HomeIntent::OpenGameSettings),
                _ => {}
            },
            HomeFocus::Top(i) => match command {
                Command::Left => go(&mut step, HomeFocus::Top(i.saturating_sub(1))),
                Command::Right => go(&mut step, HomeFocus::Top((i + 1).min(Self::LAST_TOP))),
                Command::Down | Command::Back => go(&mut step, HomeFocus::Tiles),
                Command::Select => {
                    step.intent = Some(HomeFocus::top_intent(i));
                }
                Command::Options if has_games => step.intent = Some(HomeIntent::OpenGameSettings),
                _ => {}
            },
            HomeFocus::Actions(i) => {
                let last = ctx.action_count() - 1;
                match command {
                    Command::Left => go(&mut step, HomeFocus::Actions(i.saturating_sub(1))),
                    Command::Right => go(&mut step, HomeFocus::Actions((i + 1).min(last))),
                    Command::Down if ctx.asking => go(&mut step, HomeFocus::Ask(0)),
                    Command::Up | Command::Back => go(&mut step, HomeFocus::Tiles),
                    Command::Select => {
                        step.intent = Some(if i == 0 {
                            HomeIntent::PrimaryAction
                        } else if i == 1 && ctx.has_trophies {
                            HomeIntent::OpenTrophies
                        } else {
                            HomeIntent::OpenGameSettings
                        });
                    }
                    Command::Options if has_games => {
                        step.intent = Some(HomeIntent::OpenGameSettings)
                    }
                    _ => {}
                }
            }
            HomeFocus::Ask(i) => match command {
                Command::Left => go(&mut step, HomeFocus::Ask(i.saturating_sub(1))),
                Command::Right => go(&mut step, HomeFocus::Ask((i + 1).min(ASK_LAST))),
                Command::Up | Command::Back => go(&mut step, HomeFocus::Actions(0)),
                Command::Select => step.intent = Some(HomeIntent::AnswerRun(i)),
                Command::Options if has_games => step.intent = Some(HomeIntent::OpenGameSettings),
                _ => {}
            },
        }
        step
    }
}

/// The last stop of the question line: its dismiss button (four answers come before).
pub const ASK_LAST: usize = 4;

// ---------------------------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------------------------

/// How a panel row behaves (5.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// Static text: not focusable.
    Info,
    Choice,
    Toggle,
    Stepper,
    Action,
    Text,
    Keys,
}

impl RowKind {
    pub fn focusable(self) -> bool {
        self != RowKind::Info
    }

    /// Left/right change the value.
    pub fn adjustable(self) -> bool {
        matches!(self, RowKind::Choice | RowKind::Toggle | RowKind::Stepper)
    }
}

/// What a command in a panel asks the screen to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelEffect {
    None,
    /// Change the value of `row` by one step (`-1` / `+1`; a toggle: -1 = off, +1 = on).
    Adjust {
        row: usize,
        direction: i8,
    },
    /// Run the row (action), cycle it (choice: next, wrapping), toggle it, step a stepper by +step
    /// (wrapping to min), start a capture (keys).
    Activate {
        row: usize,
    },
    /// Close the panel (or, for controls/advanced, return to the game settings).
    Close,
}

/// Result of one command in a panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelStep {
    pub effect: PanelEffect,
    pub sound: Option<Cue>,
}

/// Navigation state of a panel. Multi-category panels (settings) have a category level and a row
/// level; single-category panels (game settings, controls, advanced, confirm) are always at row level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelNav {
    pub categories: usize,
    pub category: usize,
    pub row: usize,
    pub in_rows: bool,
}

impl PanelNav {
    /// Settings: starts at the category level with category 0.
    pub fn settings(categories: usize) -> Self {
        Self {
            categories,
            category: 0,
            row: 0,
            in_rows: false,
        }
    }

    /// A single-category panel opened at `row`.
    pub fn single(row: usize) -> Self {
        Self {
            categories: 1,
            category: 0,
            row,
            in_rows: true,
        }
    }

    fn multi(&self) -> bool {
        self.categories > 1
    }

    fn first_focusable(rows: &[RowKind]) -> usize {
        rows.iter().position(|k| k.focusable()).unwrap_or(0)
    }

    /// Applies a command. `rows` are the row kinds of the current category.
    pub fn command(&mut self, command: Command, rows: &[RowKind]) -> PanelStep {
        let step = |effect, sound| PanelStep { effect, sound };
        if !self.in_rows {
            // Category level.
            return match command {
                Command::Up | Command::Down => {
                    let next = if command == Command::Up {
                        self.category.saturating_sub(1)
                    } else {
                        (self.category + 1).min(self.categories.saturating_sub(1))
                    };
                    if next != self.category {
                        self.category = next;
                        self.row = 0;
                        step(PanelEffect::None, Some(Cue::Move))
                    } else {
                        step(PanelEffect::None, None)
                    }
                }
                Command::Right | Command::Select => {
                    self.in_rows = true;
                    self.row = Self::first_focusable(rows);
                    step(PanelEffect::None, Some(Cue::Move))
                }
                Command::Back | Command::Options => step(PanelEffect::Close, Some(Cue::Back)),
                Command::Left => step(PanelEffect::None, None),
            };
        }
        // Row level.
        let kind = rows.get(self.row).copied();
        match command {
            Command::Up | Command::Down => {
                let found = if command == Command::Up {
                    (0..self.row).rev().find(|&i| rows[i].focusable())
                } else {
                    (self.row + 1..rows.len()).find(|&i| rows[i].focusable())
                };
                match found {
                    Some(i) => {
                        self.row = i;
                        step(PanelEffect::None, Some(Cue::Move))
                    }
                    None => step(PanelEffect::None, None),
                }
            }
            Command::Left | Command::Right => {
                let direction: i8 = if command == Command::Left { -1 } else { 1 };
                match kind {
                    Some(k) if k.adjustable() => step(
                        PanelEffect::Adjust {
                            row: self.row,
                            direction,
                        },
                        Some(Cue::Move),
                    ),
                    // A row that cannot be adjusted: left leaves the row level (settings only).
                    _ if command == Command::Left && self.multi() => {
                        self.in_rows = false;
                        step(PanelEffect::None, Some(Cue::Back))
                    }
                    _ => step(PanelEffect::None, None),
                }
            }
            Command::Select => match kind {
                Some(k) if k.focusable() => {
                    let sound = match k {
                        RowKind::Action | RowKind::Keys => Cue::Select,
                        _ => Cue::Move,
                    };
                    step(PanelEffect::Activate { row: self.row }, Some(sound))
                }
                _ => step(PanelEffect::None, None),
            },
            Command::Back => {
                if self.multi() {
                    self.in_rows = false;
                    step(PanelEffect::None, Some(Cue::Back))
                } else {
                    step(PanelEffect::Close, Some(Cue::Back))
                }
            }
            Command::Options => step(PanelEffect::Close, Some(Cue::Back)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(games: usize, selected: usize, has_trophies: bool) -> HomeContext {
        HomeContext {
            games,
            selected,
            has_trophies,
            asking: false,
        }
    }

    #[test]
    fn the_question_line_is_below_the_actions_and_only_while_asked() {
        let mut c = ctx(3, 0, true);
        let down = HomeFocus::Actions(0).navigate(Command::Down, c);
        assert_eq!(down.focus, HomeFocus::Actions(0), "no question, no stop");
        c.asking = true;
        let down = HomeFocus::Actions(2).navigate(Command::Down, c);
        assert_eq!(down.focus, HomeFocus::Ask(0));
        let right = HomeFocus::Ask(4).navigate(Command::Right, c);
        assert_eq!(right.focus, HomeFocus::Ask(4), "dismiss is the last stop");
        let left = HomeFocus::Ask(1).navigate(Command::Left, c);
        assert_eq!(left.focus, HomeFocus::Ask(0));
        let select = HomeFocus::Ask(2).navigate(Command::Select, c);
        assert_eq!(select.intent, Some(HomeIntent::AnswerRun(2)));
        let up = HomeFocus::Ask(3).navigate(Command::Up, c);
        assert_eq!(up.focus, HomeFocus::Actions(0));
    }

    #[test]
    fn tiles_move_selection_clamped_without_wrap() {
        let c = ctx(3, 0, false);
        let s = HomeFocus::Tiles.navigate(Command::Left, c);
        assert_eq!((s.selected, s.sound), (0, None), "no wrap, no sound");
        let s = HomeFocus::Tiles.navigate(Command::Right, c);
        assert_eq!((s.selected, s.sound), (1, Some(Cue::Move)));
        let s = HomeFocus::Tiles.navigate(Command::Right, ctx(3, 2, false));
        assert_eq!(s.selected, 2);
    }

    #[test]
    fn tiles_vertical_and_actions() {
        let c = ctx(3, 1, true);
        assert_eq!(
            HomeFocus::Tiles.navigate(Command::Up, c).focus,
            HomeFocus::SETTINGS
        );
        assert_eq!(
            HomeFocus::Tiles.navigate(Command::Down, c).focus,
            HomeFocus::Actions(0)
        );
        assert_eq!(
            HomeFocus::Tiles.navigate(Command::Select, c).intent,
            Some(HomeIntent::PrimaryAction)
        );
        assert_eq!(
            HomeFocus::Tiles.navigate(Command::Options, c).intent,
            Some(HomeIntent::OpenGameSettings)
        );
        assert_eq!(HomeFocus::Tiles.navigate(Command::Back, c).intent, None);
    }

    #[test]
    fn top_bar_navigation() {
        let c = ctx(3, 1, false);
        let nav = |from: HomeFocus, command| from.navigate(command, c);
        // Left to right: library, friends, settings, profile; no wrap at either end.
        let order = [
            HomeFocus::LIBRARY,
            HomeFocus::FRIENDS,
            HomeFocus::SETTINGS,
            HomeFocus::PROFILE,
        ];
        for pair in order.windows(2) {
            assert_eq!(nav(pair[0], Command::Right).focus, pair[1]);
            assert_eq!(nav(pair[1], Command::Left).focus, pair[0]);
        }
        let end = nav(HomeFocus::LIBRARY, Command::Left);
        assert_eq!((end.focus, end.sound), (HomeFocus::LIBRARY, None));
        let end = nav(HomeFocus::PROFILE, Command::Right);
        assert_eq!((end.focus, end.sound), (HomeFocus::PROFILE, None));
        for (focus, intent) in [
            (HomeFocus::LIBRARY, HomeIntent::OpenLibrary),
            (HomeFocus::FRIENDS, HomeIntent::OpenFriends),
            (HomeFocus::SETTINGS, HomeIntent::OpenSettings),
            (HomeFocus::PROFILE, HomeIntent::OpenOnline),
        ] {
            assert_eq!(nav(focus, Command::Select).intent, Some(intent));
            assert_eq!(nav(focus, Command::Down).focus, HomeFocus::Tiles);
            assert_eq!(nav(focus, Command::Back).focus, HomeFocus::Tiles);
            assert_eq!(nav(focus, Command::Up).focus, focus, "nothing above");
        }
    }

    #[test]
    fn action_row_with_and_without_trophies() {
        let with = ctx(2, 0, true);
        assert_eq!(
            HomeFocus::Actions(0).navigate(Command::Right, with).focus,
            HomeFocus::Actions(1)
        );
        assert_eq!(
            HomeFocus::Actions(2).navigate(Command::Right, with).focus,
            HomeFocus::Actions(2)
        );
        assert_eq!(
            HomeFocus::Actions(1).navigate(Command::Select, with).intent,
            Some(HomeIntent::OpenTrophies)
        );
        assert_eq!(
            HomeFocus::Actions(2).navigate(Command::Select, with).intent,
            Some(HomeIntent::OpenGameSettings)
        );
        let without = ctx(2, 0, false);
        assert_eq!(
            HomeFocus::Actions(1)
                .navigate(Command::Right, without)
                .focus,
            HomeFocus::Actions(1)
        );
        assert_eq!(
            HomeFocus::Actions(1)
                .navigate(Command::Select, without)
                .intent,
            Some(HomeIntent::OpenGameSettings)
        );
        assert_eq!(
            HomeFocus::Actions(0)
                .navigate(Command::Select, without)
                .intent,
            Some(HomeIntent::PrimaryAction)
        );
        assert_eq!(
            HomeFocus::Actions(1).navigate(Command::Up, with).focus,
            HomeFocus::Tiles
        );
        assert_eq!(
            HomeFocus::Actions(1).navigate(Command::Back, with).focus,
            HomeFocus::Tiles
        );
        assert_eq!(
            HomeFocus::Actions(1).navigate(Command::Down, with).focus,
            HomeFocus::Actions(1)
        );
    }

    #[test]
    fn empty_library_select_chooses_a_folder() {
        let c = ctx(0, 0, false);
        assert_eq!(
            HomeFocus::Tiles.navigate(Command::Select, c).intent,
            Some(HomeIntent::ChooseFolder)
        );
        assert_eq!(
            HomeFocus::Tiles.navigate(Command::Down, c).focus,
            HomeFocus::Tiles
        );
        assert_eq!(HomeFocus::Tiles.navigate(Command::Options, c).intent, None);
    }

    use RowKind::*;
    const ROWS: [RowKind; 6] = [Info, Choice, Toggle, Info, Action, Text];

    #[test]
    fn settings_start_at_the_category_level() {
        let mut nav = PanelNav::settings(6);
        assert!(!nav.in_rows);
        let s = nav.command(Command::Down, &ROWS);
        assert_eq!((nav.category, nav.row, s.sound), (1, 0, Some(Cue::Move)));
        let s = nav.command(Command::Up, &ROWS);
        assert_eq!(nav.category, 0);
        let s2 = nav.command(Command::Up, &ROWS);
        assert_eq!(
            (nav.category, s.sound, s2.sound),
            (0, Some(Cue::Move), None),
            "clamped"
        );
        let s = nav.command(Command::Right, &ROWS);
        assert!(nav.in_rows);
        assert_eq!(nav.row, 1, "first focusable row");
        assert_eq!(s.sound, Some(Cue::Move));
    }

    #[test]
    fn category_level_back_closes() {
        let mut nav = PanelNav::settings(6);
        assert_eq!(
            nav.command(Command::Back, &ROWS),
            PanelStep {
                effect: PanelEffect::Close,
                sound: Some(Cue::Back)
            }
        );
        assert_eq!(
            nav.command(Command::Options, &ROWS).effect,
            PanelEffect::Close
        );
    }

    #[test]
    fn rows_skip_info_and_do_not_wrap() {
        let mut nav = PanelNav::settings(6);
        nav.command(Command::Select, &ROWS);
        assert_eq!(nav.row, 1);
        nav.command(Command::Down, &ROWS);
        assert_eq!(nav.row, 2);
        nav.command(Command::Down, &ROWS);
        assert_eq!(nav.row, 4, "row 3 is info");
        nav.command(Command::Down, &ROWS);
        assert_eq!(nav.row, 5);
        let s = nav.command(Command::Down, &ROWS);
        assert_eq!((nav.row, s.sound), (5, None));
        nav.command(Command::Up, &ROWS);
        nav.command(Command::Up, &ROWS);
        nav.command(Command::Up, &ROWS);
        assert_eq!(nav.row, 1);
        let s = nav.command(Command::Up, &ROWS);
        assert_eq!((nav.row, s.sound), (1, None), "row 0 is info");
    }

    #[test]
    fn left_right_adjust_or_leave() {
        let mut nav = PanelNav::settings(6);
        nav.command(Command::Select, &ROWS); // row 1: choice
        assert_eq!(
            nav.command(Command::Right, &ROWS).effect,
            PanelEffect::Adjust {
                row: 1,
                direction: 1
            }
        );
        assert_eq!(
            nav.command(Command::Left, &ROWS).effect,
            PanelEffect::Adjust {
                row: 1,
                direction: -1
            }
        );
        nav.row = 4; // action: not adjustable
        assert_eq!(nav.command(Command::Right, &ROWS).effect, PanelEffect::None);
        let s = nav.command(Command::Left, &ROWS);
        assert!(
            !nav.in_rows,
            "left on a non-adjustable row leaves the row level"
        );
        assert_eq!(s.sound, Some(Cue::Back));
    }

    #[test]
    fn select_activates_with_the_matching_sound() {
        let mut nav = PanelNav::settings(6);
        nav.command(Command::Select, &ROWS);
        let s = nav.command(Command::Select, &ROWS);
        assert_eq!(
            s,
            PanelStep {
                effect: PanelEffect::Activate { row: 1 },
                sound: Some(Cue::Move)
            }
        );
        nav.row = 4;
        let s = nav.command(Command::Select, &ROWS);
        assert_eq!(
            s,
            PanelStep {
                effect: PanelEffect::Activate { row: 4 },
                sound: Some(Cue::Select)
            }
        );
    }

    #[test]
    fn back_leaves_rows_in_settings_and_closes_single_panels() {
        let mut nav = PanelNav::settings(6);
        nav.command(Command::Select, &ROWS);
        let s = nav.command(Command::Back, &ROWS);
        assert!(!nav.in_rows);
        assert_eq!(s.effect, PanelEffect::None);

        let mut single = PanelNav::single(1);
        assert_eq!(
            single.command(Command::Back, &ROWS).effect,
            PanelEffect::Close
        );
        assert_eq!(
            single.command(Command::Options, &ROWS).effect,
            PanelEffect::Close
        );
        // Left on a non-adjustable row of a single panel does nothing.
        single.row = 4;
        assert_eq!(
            single.command(Command::Left, &ROWS).effect,
            PanelEffect::None
        );
        assert!(single.in_rows);
    }
}

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
    /// The launcher update notice after the title: its buttons from the left.
    Update(u8),
    /// The Play Together card at the top centre: its buttons from the left (the quiet answer
    /// first, the primary one last). The pad reaches it from the library button, to the left.
    Card(u8),
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
    /// The selected game can be played together: a Play Together button sits after Play.
    pub together: bool,
    /// "How did it run?" is on screen.
    pub asking: bool,
    /// Buttons of the launcher update notice (0 = no notice).
    pub notice: u8,
    /// Buttons of the Play Together card (0 = no card).
    pub card: u8,
}

impl HomeContext {
    /// Number of buttons in the action row: primary, Play Together (optional), trophies
    /// (optional), "...".
    pub fn action_count(&self) -> usize {
        2 + usize::from(self.has_trophies) + usize::from(self.together)
    }

    /// What a press on button `index` of the action row does.
    pub fn action_intent(&self, index: usize) -> HomeIntent {
        let mut slot = 1;
        if index == 0 {
            return HomeIntent::PrimaryAction;
        }
        if self.together {
            if index == slot {
                return HomeIntent::PlayTogether;
            }
            slot += 1;
        }
        if self.has_trophies && index == slot {
            return HomeIntent::OpenTrophies;
        }
        HomeIntent::OpenGameSettings
    }
}

/// What a command on the home screen leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeIntent {
    /// Run the selected game's primary action (play / unlock / retry / cancel / activate window).
    PrimaryAction,
    /// Start the selected game with a Play Together session.
    PlayTogether,
    /// Open the trophy list.
    OpenTrophies,
    /// Open the game settings side panel.
    OpenGameSettings,
    /// Answer "How did it run?" (0 to 3) or dismiss it (4).
    AnswerRun(usize),
    /// A button of the launcher update notice (0 = the main one).
    UpdateAction(usize),
    /// A button of the Play Together card, in the order it is drawn.
    CardAction(usize),
    OpenLibrary,
    /// The captures button: the screenshots and clips.
    OpenCaptures,
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
    pub const CAPTURES: HomeFocus = HomeFocus::Top(1);
    pub const FRIENDS: HomeFocus = HomeFocus::Top(2);
    pub const SETTINGS: HomeFocus = HomeFocus::Top(3);
    pub const PROFILE: HomeFocus = HomeFocus::Top(4);
    /// The last stop of the top bar.
    const LAST_TOP: u8 = 4;

    /// What a press on stop `index` of the top bar does.
    pub fn top_intent(index: u8) -> HomeIntent {
        match index {
            0 => HomeIntent::OpenLibrary,
            1 => HomeIntent::OpenCaptures,
            2 => HomeIntent::OpenFriends,
            3 => HomeIntent::OpenSettings,
            _ => HomeIntent::OpenOnline,
        }
    }

    /// Every stop of the home screen in reading order: the top bar from left to right, the tile
    /// row (one stop, the arrows pick the tile), the action row and, while asked, the question.
    pub fn reading_order(ctx: HomeContext) -> Vec<HomeFocus> {
        let mut order: Vec<HomeFocus> = (0..ctx.card).map(HomeFocus::Card).collect();
        order.extend((0..=Self::LAST_TOP).map(HomeFocus::Top));
        order.push(HomeFocus::Tiles);
        if ctx.games > 0 {
            order.extend((0..ctx.action_count()).map(HomeFocus::Actions));
            if ctx.asking {
                order.extend((0..=ASK_LAST).map(HomeFocus::Ask));
            }
        }
        order
    }

    /// Tab (or Shift+Tab with `backwards`): the next or previous stop in reading order. There is no
    /// wrap, so the last stop stays put like the arrows do at an end.
    pub fn tab(self, backwards: bool, ctx: HomeContext) -> HomeStep {
        let order = Self::reading_order(ctx);
        // A stop that has gone (the action row shrank, the question was answered) counts as the
        // nearest one that is left.
        let tiles = order
            .iter()
            .position(|f| *f == HomeFocus::Tiles)
            .unwrap_or(0);
        let here = order
            .iter()
            .position(|f| *f == self)
            .unwrap_or_else(|| match self {
                HomeFocus::Actions(_) | HomeFocus::Ask(_) => order
                    .iter()
                    .rposition(|f| matches!(f, HomeFocus::Actions(_)))
                    .unwrap_or(tiles),
                _ => tiles,
            });
        let there = if backwards {
            here.saturating_sub(1)
        } else {
            (here + 1).min(order.len() - 1)
        };
        HomeStep {
            focus: order[there],
            selected: ctx.selected.min(ctx.games.saturating_sub(1)),
            intent: None,
            sound: (order[there] != self).then_some(Cue::Move),
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
                Command::Left if i == 0 && ctx.notice > 0 => {
                    go(&mut step, HomeFocus::Update(ctx.notice - 1));
                }
                Command::Left if i == 0 && ctx.card > 0 => {
                    go(&mut step, HomeFocus::Card(ctx.card - 1));
                }
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
                        step.intent = Some(ctx.action_intent(i));
                    }
                    Command::Options if has_games => {
                        step.intent = Some(HomeIntent::OpenGameSettings)
                    }
                    _ => {}
                }
            }
            HomeFocus::Update(i) => match command {
                Command::Left if i == 0 && ctx.card > 0 => {
                    go(&mut step, HomeFocus::Card(ctx.card - 1));
                }
                Command::Left => go(&mut step, HomeFocus::Update(i.saturating_sub(1))),
                Command::Right if i + 1 < ctx.notice => go(&mut step, HomeFocus::Update(i + 1)),
                Command::Right => go(&mut step, HomeFocus::Top(0)),
                Command::Down | Command::Back => go(&mut step, HomeFocus::Tiles),
                Command::Select => step.intent = Some(HomeIntent::UpdateAction(usize::from(i))),
                Command::Options if has_games => step.intent = Some(HomeIntent::OpenGameSettings),
                _ => {}
            },
            HomeFocus::Card(i) => match command {
                Command::Left => go(&mut step, HomeFocus::Card(i.saturating_sub(1))),
                Command::Right if i + 1 < ctx.card => go(&mut step, HomeFocus::Card(i + 1)),
                Command::Right if ctx.notice > 0 => go(&mut step, HomeFocus::Update(0)),
                Command::Right => go(&mut step, HomeFocus::Top(0)),
                Command::Down => go(&mut step, HomeFocus::Tiles),
                Command::Select => step.intent = Some(HomeIntent::CardAction(usize::from(i))),
                Command::Options if has_games => step.intent = Some(HomeIntent::OpenGameSettings),
                _ => {}
            },
            HomeFocus::Ask(i) => match command {
                Command::Left => go(&mut step, HomeFocus::Ask(i.saturating_sub(1))),
                Command::Right => go(&mut step, HomeFocus::Ask((i + 1).min(ASK_LAST))),
                Command::Up | Command::Back => go(&mut step, HomeFocus::Actions(0)),
                Command::Select => step.intent = Some(HomeIntent::AnswerRun(i)),
                Command::Options if has_games => step.intent = Some(HomeIntent::OpenGameSettings),
                _ => {}
            },
        }
        // A direction that went nowhere has met an edge: a soft bump, so the press is answered.
        if command.is_direction()
            && step.intent.is_none()
            && step.sound.is_none()
            && step.focus == self
            && step.selected == ctx.selected.min(last_game)
        {
            step.sound = Some(Cue::Bump);
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

    pub fn first_focusable(rows: &[RowKind]) -> usize {
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
                        step(PanelEffect::None, Some(Cue::Bump))
                    }
                }
                Command::Right | Command::Select => {
                    self.in_rows = true;
                    self.row = Self::first_focusable(rows);
                    step(PanelEffect::None, Some(Cue::Move))
                }
                Command::Back | Command::Options => step(PanelEffect::Close, Some(Cue::Close)),
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
                    None => step(PanelEffect::None, Some(Cue::Bump)),
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
                    step(PanelEffect::Close, Some(Cue::Close))
                }
            }
            Command::Options => step(PanelEffect::Close, Some(Cue::Close)),
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
            together: false,
            asking: false,
            notice: 0,
            card: 0,
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
    fn the_pad_reaches_every_button_of_the_card() {
        let mut c = ctx(3, 0, true);
        c.card = 2;
        // Left of the library button: the card's primary (last) button, then its quiet one.
        let left = HomeFocus::LIBRARY.navigate(Command::Left, c);
        assert_eq!(left.focus, HomeFocus::Card(1));
        let left = HomeFocus::Card(1).navigate(Command::Left, c);
        assert_eq!(left.focus, HomeFocus::Card(0));
        let select = HomeFocus::Card(0).navigate(Command::Select, c);
        assert_eq!(select.intent, Some(HomeIntent::CardAction(0)));
        // Right walks back to the top bar; Down leaves for the tiles.
        let right = HomeFocus::Card(0).navigate(Command::Right, c);
        assert_eq!(right.focus, HomeFocus::Card(1));
        let right = HomeFocus::Card(1).navigate(Command::Right, c);
        assert_eq!(right.focus, HomeFocus::LIBRARY);
        let down = HomeFocus::Card(1).navigate(Command::Down, c);
        assert_eq!(down.focus, HomeFocus::Tiles);
        // With an update notice too, the notice sits between the card and the top bar.
        c.notice = 2;
        let left = HomeFocus::Update(0).navigate(Command::Left, c);
        assert_eq!(left.focus, HomeFocus::Card(1));
        let right = HomeFocus::Card(1).navigate(Command::Right, c);
        assert_eq!(right.focus, HomeFocus::Update(0));
        // No card, no stop.
        c.card = 0;
        c.notice = 0;
        let left = HomeFocus::LIBRARY.navigate(Command::Left, c);
        assert_eq!(left.focus, HomeFocus::LIBRARY);
    }

    #[test]
    fn tab_walks_the_home_screen_in_reading_order() {
        let mut c = ctx(3, 1, true);
        let mut at = HomeFocus::LIBRARY;
        let mut seen = vec![at];
        for _ in 0..12 {
            let step = at.tab(false, c);
            if step.focus == at {
                break;
            }
            assert_eq!(step.sound, Some(Cue::Move));
            assert_eq!(step.selected, 1, "Tab never changes the tile");
            at = step.focus;
            seen.push(at);
        }
        assert_eq!(
            seen,
            [
                HomeFocus::Top(0),
                HomeFocus::Top(1),
                HomeFocus::Top(2),
                HomeFocus::Top(3),
                HomeFocus::Top(4),
                HomeFocus::Tiles,
                HomeFocus::Actions(0),
                HomeFocus::Actions(1),
                HomeFocus::Actions(2),
            ]
        );
        let end = at.tab(false, c);
        assert_eq!((end.focus, end.sound), (at, None), "no wrap at the end");
        // Shift+Tab goes back the same way, and stops at the first stop.
        assert_eq!(HomeFocus::Actions(0).tab(true, c).focus, HomeFocus::Tiles);
        assert_eq!(HomeFocus::Tiles.tab(true, c).focus, HomeFocus::PROFILE);
        assert_eq!(HomeFocus::LIBRARY.tab(true, c).focus, HomeFocus::LIBRARY);
        // The question line joins while it is asked.
        c.asking = true;
        assert_eq!(HomeFocus::Actions(2).tab(false, c).focus, HomeFocus::Ask(0));
        assert_eq!(HomeFocus::Ask(4).tab(false, c).focus, HomeFocus::Ask(4));
        // A stop that has gone is replaced by the nearest one: Tab from a third action when only two
        // are left goes to nothing further, Shift+Tab to the one before.
        let two = ctx(3, 0, false);
        assert_eq!(
            HomeFocus::Actions(2).tab(true, two).focus,
            HomeFocus::Actions(0)
        );
        assert_eq!(
            HomeFocus::Ask(1).tab(false, two).focus,
            HomeFocus::Actions(1)
        );
        // No games: the top bar and the one tile stop (choose a folder).
        let empty = ctx(0, 0, false);
        assert_eq!(HomeFocus::PROFILE.tab(false, empty).focus, HomeFocus::Tiles);
        assert_eq!(HomeFocus::Tiles.tab(false, empty).focus, HomeFocus::Tiles);
    }

    #[test]
    fn tiles_move_selection_clamped_without_wrap() {
        let c = ctx(3, 0, false);
        let s = HomeFocus::Tiles.navigate(Command::Left, c);
        assert_eq!(
            (s.selected, s.sound),
            (0, Some(Cue::Bump)),
            "no wrap, a bump"
        );
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
        // Left to right: library, captures, friends, settings, profile; no wrap at either end.
        let order = [
            HomeFocus::LIBRARY,
            HomeFocus::CAPTURES,
            HomeFocus::FRIENDS,
            HomeFocus::SETTINGS,
            HomeFocus::PROFILE,
        ];
        for pair in order.windows(2) {
            assert_eq!(nav(pair[0], Command::Right).focus, pair[1]);
            assert_eq!(nav(pair[1], Command::Left).focus, pair[0]);
        }
        let end = nav(HomeFocus::LIBRARY, Command::Left);
        assert_eq!(
            (end.focus, end.sound),
            (HomeFocus::LIBRARY, Some(Cue::Bump))
        );
        let end = nav(HomeFocus::PROFILE, Command::Right);
        assert_eq!(
            (end.focus, end.sound),
            (HomeFocus::PROFILE, Some(Cue::Bump))
        );
        for (focus, intent) in [
            (HomeFocus::LIBRARY, HomeIntent::OpenLibrary),
            (HomeFocus::CAPTURES, HomeIntent::OpenCaptures),
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
            (0, Some(Cue::Move), Some(Cue::Bump)),
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
                sound: Some(Cue::Close)
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
        assert_eq!((nav.row, s.sound), (5, Some(Cue::Bump)));
        nav.command(Command::Up, &ROWS);
        nav.command(Command::Up, &ROWS);
        nav.command(Command::Up, &ROWS);
        assert_eq!(nav.row, 1);
        let s = nav.command(Command::Up, &ROWS);
        assert_eq!((nav.row, s.sound), (1, Some(Cue::Bump)), "row 0 is info");
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

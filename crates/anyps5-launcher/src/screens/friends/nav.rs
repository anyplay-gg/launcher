//! Where the focus goes on the Friends screen. The screen has two zones: the rail of sections on
//! the left and the page beside it. Up and down walk the rail and show the section they stop on;
//! right (or select) crosses to the page; left on the page crosses back to the rail once the row
//! under the focus has nothing left to step through. Pure, so the rules are tested without a
//! window.

use crate::ui::input::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Rail,
    Page,
}

/// The rail and the page: which zone has the focus, and which entry of the rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Focus {
    pub zone: Zone,
    pub rail: usize,
}

/// What a command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing happens (an edge).
    Stay,
    /// The focus moved. A move inside the rail also shows the section it stopped on.
    Moved(Focus),
    /// The entry of the rail was pressed (an action such as Your Profile; a section is entered).
    Press(usize),
    /// The page takes the command.
    Page(Command),
    /// Leave the screen.
    Leave,
}

/// The step `command` takes from `focus`. `rail_len` entries stand in the rail, the first
/// `sections` of them are sections. `page_keeps_left` is whether the row under the focus still has
/// something to step through to the left.
pub fn step(
    focus: Focus,
    command: Command,
    rail_len: usize,
    sections: usize,
    page_keeps_left: bool,
) -> Step {
    match focus.zone {
        Zone::Rail => match command {
            Command::Up => match focus.rail.checked_sub(1) {
                Some(rail) => Step::Moved(Focus { rail, ..focus }),
                None => Step::Stay,
            },
            Command::Down if focus.rail + 1 < rail_len => Step::Moved(Focus {
                rail: focus.rail + 1,
                ..focus
            }),
            Command::Down | Command::Left => Step::Stay,
            // An entry that is an action stays in the rail; a section opens onto its page.
            Command::Right | Command::Select if focus.rail >= sections => {
                if command == Command::Select {
                    Step::Press(focus.rail)
                } else {
                    Step::Stay
                }
            }
            Command::Right | Command::Select => Step::Moved(Focus {
                zone: Zone::Page,
                ..focus
            }),
            Command::Back | Command::Options => Step::Leave,
        },
        Zone::Page => match command {
            Command::Left if !page_keeps_left => Step::Moved(Focus {
                zone: Zone::Rail,
                ..focus
            }),
            other => Step::Page(other),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAIL: usize = 6;
    const SECTIONS: usize = 5;

    fn rail(at: usize) -> Focus {
        Focus {
            zone: Zone::Rail,
            rail: at,
        }
    }

    fn page(at: usize) -> Focus {
        Focus {
            zone: Zone::Page,
            rail: at,
        }
    }

    fn go(focus: Focus, command: Command) -> Step {
        step(focus, command, RAIL, SECTIONS, false)
    }

    #[test]
    fn up_and_down_walk_the_rail_and_stop_at_its_ends() {
        assert_eq!(go(rail(0), Command::Down), Step::Moved(rail(1)));
        assert_eq!(go(rail(4), Command::Down), Step::Moved(rail(5)));
        assert_eq!(go(rail(5), Command::Down), Step::Stay);
        assert_eq!(go(rail(1), Command::Up), Step::Moved(rail(0)));
        assert_eq!(go(rail(0), Command::Up), Step::Stay);
    }

    #[test]
    fn right_and_select_enter_the_page_from_a_section() {
        assert_eq!(go(rail(2), Command::Right), Step::Moved(page(2)));
        assert_eq!(go(rail(2), Command::Select), Step::Moved(page(2)));
        assert_eq!(go(rail(2), Command::Left), Step::Stay);
    }

    #[test]
    fn an_action_of_the_rail_is_pressed_and_never_entered() {
        assert_eq!(go(rail(5), Command::Select), Step::Press(5));
        assert_eq!(go(rail(5), Command::Right), Step::Stay);
    }

    #[test]
    fn left_on_the_page_returns_to_the_rail_unless_the_row_still_steps() {
        assert_eq!(go(page(1), Command::Left), Step::Moved(rail(1)));
        assert_eq!(
            step(page(1), Command::Left, RAIL, SECTIONS, true),
            Step::Page(Command::Left)
        );
    }

    #[test]
    fn the_page_takes_everything_else() {
        for command in [
            Command::Up,
            Command::Down,
            Command::Right,
            Command::Select,
            Command::Back,
            Command::Options,
        ] {
            assert_eq!(go(page(0), command), Step::Page(command));
        }
    }

    #[test]
    fn back_and_options_leave_from_the_rail() {
        assert_eq!(go(rail(3), Command::Back), Step::Leave);
        assert_eq!(go(rail(3), Command::Options), Step::Leave);
    }

    #[test]
    fn a_rail_without_entries_leads_nowhere() {
        assert_eq!(step(rail(0), Command::Down, 0, 0, false), Step::Stay);
        assert_eq!(step(rail(0), Command::Up, 0, 0, false), Step::Stay);
    }
}

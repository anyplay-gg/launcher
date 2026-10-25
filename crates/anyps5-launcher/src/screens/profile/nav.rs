//! Where the focus goes on the profile screen. The screen has a row of buttons under the name and
//! two columns below it (games on the left, trophies and activity on the right). Up and down walk
//! the column, left and right cross to the other column at the same height, and up from the top of
//! a column reaches the buttons. Pure, so the rules are tested without a window.

use crate::ui::input::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    /// The buttons under the name, side by side.
    Header,
    Left,
    Right,
}

/// A place the focus can stand: its column and the middle of its height on the page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub column: Column,
    pub y: f32,
}

/// The slot the focus moves to, `None` when the command leads nowhere (an edge).
pub fn step(current: usize, command: Command, slots: &[Slot]) -> Option<usize> {
    let here = *slots.get(current)?;
    let next_in = |column: Column| slots.iter().position(|s| s.column == column);
    let last_in = |column: Column| slots.iter().rposition(|s| s.column == column);
    // The slot of `column` whose height is nearest to the current one.
    let level = |column: Column| {
        slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.column == column)
            .min_by(|(_, a), (_, b)| {
                (a.y - here.y)
                    .abs()
                    .partial_cmp(&(b.y - here.y).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(n, _)| n)
    };
    let beside = |n: Option<usize>, column: Column| {
        n.filter(|n| slots.get(*n).is_some_and(|s| s.column == column))
    };
    match command {
        Command::Up => match here.column {
            Column::Header => None,
            Column::Left => {
                beside(current.checked_sub(1), Column::Left).or(next_in(Column::Header))
            }
            Column::Right => {
                beside(current.checked_sub(1), Column::Right).or(last_in(Column::Header))
            }
        },
        Command::Down => match here.column {
            Column::Header => next_in(Column::Left).or(next_in(Column::Right)),
            column => beside(Some(current + 1), column),
        },
        Command::Left => match here.column {
            Column::Header => beside(current.checked_sub(1), Column::Header),
            Column::Left => None,
            Column::Right => level(Column::Left),
        },
        Command::Right => match here.column {
            Column::Header => beside(Some(current + 1), Column::Header),
            Column::Left => level(Column::Right),
            Column::Right => None,
        },
        Command::Select | Command::Back | Command::Options => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(column: Column, y: f32) -> Slot {
        Slot { column, y }
    }

    /// Two buttons, three games, two trophies and two activity rows.
    fn page() -> Vec<Slot> {
        vec![
            slot(Column::Header, 200.0),
            slot(Column::Header, 200.0),
            slot(Column::Left, 400.0),
            slot(Column::Left, 480.0),
            slot(Column::Left, 560.0),
            slot(Column::Right, 400.0),
            slot(Column::Right, 470.0),
            slot(Column::Right, 600.0),
            slot(Column::Right, 700.0),
        ]
    }

    #[test]
    fn the_buttons_walk_sideways_and_down_leads_into_the_games() {
        let slots = page();
        assert_eq!(step(0, Command::Right, &slots), Some(1));
        assert_eq!(step(1, Command::Right, &slots), None);
        assert_eq!(step(1, Command::Left, &slots), Some(0));
        assert_eq!(step(0, Command::Left, &slots), None);
        assert_eq!(step(0, Command::Down, &slots), Some(2));
        assert_eq!(step(1, Command::Down, &slots), Some(2));
        assert_eq!(step(0, Command::Up, &slots), None);
    }

    #[test]
    fn a_column_walks_up_and_down_and_stops_at_its_end() {
        let slots = page();
        assert_eq!(step(2, Command::Down, &slots), Some(3));
        assert_eq!(step(4, Command::Down, &slots), None);
        assert_eq!(step(7, Command::Up, &slots), Some(6));
        assert_eq!(step(8, Command::Down, &slots), None);
    }

    #[test]
    fn up_from_the_top_of_a_column_reaches_the_buttons() {
        let slots = page();
        assert_eq!(
            step(2, Command::Up, &slots),
            Some(0),
            "games: the first button"
        );
        assert_eq!(
            step(5, Command::Up, &slots),
            Some(1),
            "trophies: the last button"
        );
    }

    #[test]
    fn crossing_lands_on_the_row_at_the_same_height() {
        let slots = page();
        assert_eq!(step(3, Command::Right, &slots), Some(6));
        assert_eq!(step(4, Command::Right, &slots), Some(7));
        assert_eq!(step(8, Command::Left, &slots), Some(4));
        assert_eq!(step(2, Command::Left, &slots), None);
        assert_eq!(step(5, Command::Right, &slots), None);
    }

    #[test]
    fn a_page_without_a_column_keeps_the_focus_in_the_other() {
        // A friend without games: buttons and the right column only.
        let slots = vec![
            slot(Column::Header, 200.0),
            slot(Column::Right, 400.0),
            slot(Column::Right, 480.0),
        ];
        assert_eq!(step(0, Command::Down, &slots), Some(1));
        assert_eq!(step(1, Command::Left, &slots), None);
        assert_eq!(step(1, Command::Up, &slots), Some(0));
        // No buttons at all: up stays on the first row.
        let slots = vec![slot(Column::Left, 400.0)];
        assert_eq!(step(0, Command::Up, &slots), None);
    }

    #[test]
    fn other_commands_and_a_stale_index_lead_nowhere() {
        let slots = page();
        assert_eq!(step(2, Command::Select, &slots), None);
        assert_eq!(step(2, Command::Back, &slots), None);
        assert_eq!(step(99, Command::Down, &slots), None);
        assert_eq!(step(0, Command::Down, &[]), None);
    }
}

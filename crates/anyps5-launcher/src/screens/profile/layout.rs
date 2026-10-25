//! Which section stands in which column, and how wide the columns are. The three sections are
//! games, trophies and activity; a section that has nothing to show is not there, and the rest
//! close ranks so the page never has a hole.

use super::nav::Column;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Games,
    Trophies,
    Activity,
}

/// Gap between the two columns, in dp.
pub const COLUMN_GAP: f32 = 64.0;
/// Share of the width the left column takes when there are two.
const LEFT_SHARE: f32 = 0.45;
/// The widest a single column grows.
const SINGLE_MAX: f32 = 760.0;

/// The sections that are shown, each with its column. Three sections: games on the left, the other
/// two stacked on the right. Two: one in each column. One: on the left.
pub fn placements(games: bool, trophies: bool, activity: bool) -> Vec<(Kind, Column)> {
    let shown: Vec<Kind> = [
        (Kind::Games, games),
        (Kind::Trophies, trophies),
        (Kind::Activity, activity),
    ]
    .into_iter()
    .filter_map(|(kind, on)| on.then_some(kind))
    .collect();
    match shown.as_slice() {
        [] => Vec::new(),
        [one] => vec![(*one, Column::Left)],
        [a, b] => vec![(*a, Column::Left), (*b, Column::Right)],
        [a, rest @ ..] => {
            let mut out = vec![(*a, Column::Left)];
            out.extend(rest.iter().map(|k| (*k, Column::Right)));
            out
        }
    }
}

/// `(left, right)` widths and the x of the right column for a page `total` wide.
pub fn columns(total: f32, two: bool) -> (f32, f32, f32) {
    if two {
        let left = ((total - COLUMN_GAP) * LEFT_SHARE).floor();
        let right = total - COLUMN_GAP - left;
        (left, right, left + COLUMN_GAP)
    } else {
        let width = total.min(SINGLE_MAX);
        (width, 0.0, width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_sections_put_games_on_the_left_and_stack_the_rest() {
        assert_eq!(
            placements(true, true, true),
            vec![
                (Kind::Games, Column::Left),
                (Kind::Trophies, Column::Right),
                (Kind::Activity, Column::Right),
            ]
        );
    }

    #[test]
    fn two_sections_take_one_column_each() {
        assert_eq!(
            placements(false, true, true),
            vec![
                (Kind::Trophies, Column::Left),
                (Kind::Activity, Column::Right)
            ]
        );
        assert_eq!(
            placements(true, false, true),
            vec![(Kind::Games, Column::Left), (Kind::Activity, Column::Right)]
        );
    }

    #[test]
    fn one_section_stands_on_the_left_and_none_leaves_nothing() {
        assert_eq!(
            placements(false, false, true),
            vec![(Kind::Activity, Column::Left)]
        );
        assert!(placements(false, false, false).is_empty());
    }

    #[test]
    fn the_columns_add_up_to_the_width() {
        let (left, right, x) = columns(1420.0, true);
        assert_eq!(left + COLUMN_GAP + right, 1420.0);
        assert_eq!(x, left + COLUMN_GAP);
        assert!(
            left < right,
            "the trophies and activity get the wider column"
        );
        let (single, none, _) = columns(1420.0, false);
        assert_eq!((single, none), (760.0, 0.0));
        assert_eq!(columns(600.0, false).0, 600.0);
    }
}

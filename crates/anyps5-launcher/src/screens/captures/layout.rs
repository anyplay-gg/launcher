//! Where the captures sit: groups of tiles, one group per game, every group a header over rows of
//! equal tiles. Pure arithmetic with no drawing, so the focus moves and the scrolling can be tested
//! with a thousand captures and no window.
//!
//! A capture's *place* is its position in the flat list: the groups one after the other, each
//! group newest first. The arrows move through places; this module knows which place is above,
//! below, before and after another.

use crate::ui::input::Command;

/// One row of tiles, with the header of its group when it is the group's first row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub section: usize,
    /// The place of the first tile in this row.
    pub first: usize,
    /// How many tiles the row holds.
    pub len: usize,
    /// Top of the band (its header if it has one), from the top of the content.
    pub top: f32,
    /// Top of the tiles.
    pub tiles_top: f32,
    pub height: f32,
    pub header: bool,
}

/// The measures a layout is made from, in dp.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub header: f32,
    pub tile_row: f32,
    pub row_gap: f32,
    pub section_gap: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub columns: usize,
    pub bands: Vec<Band>,
    pub height: f32,
}

impl Layout {
    /// The rows for groups of `sizes` tiles, `columns` to a row.
    pub fn build(sizes: &[usize], columns: usize, m: Metrics) -> Layout {
        let columns = columns.max(1);
        let mut bands = Vec::new();
        let mut y = 0.0;
        let mut place = 0;
        for (section, &size) in sizes.iter().enumerate() {
            if size == 0 {
                continue;
            }
            if section > 0 && !bands.is_empty() {
                y += m.section_gap;
            }
            let rows = size.div_ceil(columns);
            for row in 0..rows {
                let first = place + row * columns;
                let len = columns.min(place + size - first);
                let header = row == 0;
                let top = y;
                let tiles_top = if header { y + m.header } else { y };
                let bottom = tiles_top + m.tile_row;
                bands.push(Band {
                    section,
                    first,
                    len,
                    top,
                    tiles_top,
                    height: bottom - top,
                    header,
                });
                y = bottom + if row + 1 < rows { m.row_gap } else { 0.0 };
            }
            place += size;
        }
        Layout {
            columns,
            bands,
            height: y,
        }
    }

    /// The band holding a place.
    pub fn band_of(&self, place: usize) -> Option<usize> {
        let at = self.bands.partition_point(|b| b.first + b.len <= place);
        self.bands.get(at).filter(|b| b.first <= place).map(|_| at)
    }

    /// The bands that touch the content between `from` and `to`.
    pub fn visible(&self, from: f32, to: f32) -> std::ops::Range<usize> {
        let start = self.bands.partition_point(|b| b.top + b.height < from);
        let end = self.bands.partition_point(|b| b.top <= to);
        start..end.max(start)
    }

    /// Where the tile at `place` is, within its row: `(x offset in tiles, band)`.
    pub fn column_of(&self, place: usize) -> Option<(usize, usize)> {
        let band = self.band_of(place)?;
        Some((place - self.bands[band].first, band))
    }

    /// `(top, height)` of every band, for the scroll that follows the focus.
    pub fn geometry(&self) -> Vec<(f32, f32)> {
        self.bands.iter().map(|b| (b.top, b.height)).collect()
    }
}

/// Where an arrow takes the focus from `place` among `total` tiles in `layout`, or `None` at an
/// end. Left and right step one place and cross from one group to the next. Up and down keep the
/// column, and into a shorter row they land on its last tile.
pub fn step(layout: &Layout, place: usize, total: usize, command: Command) -> Option<usize> {
    let (column, band) = layout.column_of(place)?;
    match command {
        Command::Left => place.checked_sub(1),
        Command::Right => (place + 1 < total).then_some(place + 1),
        Command::Down => {
            let next = layout.bands.get(band + 1)?;
            Some(next.first + column.min(next.len - 1))
        }
        Command::Up => {
            let previous = layout.bands.get(band.checked_sub(1)?)?;
            Some(previous.first + column.min(previous.len - 1))
        }
        _ => None,
    }
}

/// As many columns as fit `width` for tiles of `tile` wide with `gap` between, at least two.
pub fn columns_for(width: f32, tile: f32, gap: f32) -> usize {
    (((width + gap) / (tile + gap)).floor().max(0.0) as usize).max(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: Metrics = Metrics {
        header: 50.0,
        tile_row: 200.0,
        row_gap: 20.0,
        section_gap: 30.0,
    };

    #[test]
    fn rows_fill_left_to_right_and_headers_sit_on_a_groups_first_row() {
        let layout = Layout::build(&[5, 2], 4, M);
        let rows: Vec<_> = layout
            .bands
            .iter()
            .map(|b| (b.section, b.first, b.len, b.header))
            .collect();
        assert_eq!(rows, [(0, 0, 4, true), (0, 4, 1, false), (1, 5, 2, true)]);
        assert_eq!(layout.bands[0].top, 0.0);
        assert_eq!(layout.bands[0].tiles_top, 50.0);
        assert_eq!(layout.bands[1].top, 270.0, "after a row and a gap");
        // Group 1 starts after the first group's last row and the group gap.
        assert_eq!(layout.bands[2].top, 470.0 + 30.0);
        assert_eq!(layout.height, 500.0 + 50.0 + 200.0);
    }

    #[test]
    fn an_empty_group_leaves_no_trace() {
        let layout = Layout::build(&[0, 3, 0], 4, M);
        assert_eq!(layout.bands.len(), 1);
        assert_eq!(layout.bands[0].section, 1);
        assert_eq!(layout.bands[0].top, 0.0, "no gap before the first group");
        assert_eq!(Layout::build(&[], 4, M).height, 0.0);
    }

    #[test]
    fn arrows_walk_the_places_across_groups() {
        let sizes = [5, 2];
        let layout = Layout::build(&sizes, 4, M);
        let total = 7;
        let go = |from, c| step(&layout, from, total, c);
        assert_eq!(go(0, Command::Right), Some(1));
        assert_eq!(go(3, Command::Right), Some(4), "wraps to the next row");
        assert_eq!(go(4, Command::Right), Some(5), "into the next group");
        assert_eq!(go(6, Command::Right), None);
        assert_eq!(go(0, Command::Left), None);
        assert_eq!(go(5, Command::Left), Some(4));
        // Down keeps the column; into a shorter row it lands on the last tile.
        assert_eq!(go(0, Command::Down), Some(4));
        assert_eq!(go(3, Command::Down), Some(4), "the row has one tile");
        assert_eq!(go(4, Command::Down), Some(5), "into the next group");
        assert_eq!(go(6, Command::Down), None);
        assert_eq!(go(5, Command::Up), Some(4));
        assert_eq!(go(6, Command::Up), Some(4), "column 1 of a one-tile row");
        assert_eq!(go(2, Command::Up), None, "the top");
        assert_eq!(go(0, Command::Select), None);
    }

    #[test]
    fn every_place_has_a_band_and_a_thousand_are_cheap() {
        let sizes = [400, 350, 250];
        let layout = Layout::build(&sizes, 6, M);
        for place in [0, 5, 6, 399, 400, 749, 750, 999] {
            let band = layout.band_of(place).unwrap();
            let b = layout.bands[band];
            assert!(b.first <= place && place < b.first + b.len, "{place}");
        }
        assert_eq!(layout.band_of(1000), None);
        // Only the rows on screen are visited.
        let seen = layout.visible(2000.0, 2900.0);
        assert!(seen.len() < 8, "{seen:?}");
        assert!(seen.start > 0 && seen.end < layout.bands.len());
        let all = layout.visible(0.0, layout.height);
        assert_eq!(all, 0..layout.bands.len());
    }

    #[test]
    fn tiles_per_row_follow_the_width() {
        assert_eq!(columns_for(1000.0, 280.0, 20.0), 3);
        assert_eq!(columns_for(1500.0, 280.0, 20.0), 5);
        assert_eq!(columns_for(100.0, 280.0, 20.0), 2, "never fewer than two");
    }
}

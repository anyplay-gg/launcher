//! What the launcher keeps of the trophy comparisons it asked for, and the one small computation
//! behind the level figure: how far a player is from the next level (docs/DESIGN.md 17, "Trophy
//! compare"). The cloud sends every number; nothing is computed here but a fraction.

use super::api::{AccountId, TrophyCompare, TrophyLevel};

/// A comparison with one account in one title, or why there is none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompareEntry {
    /// The last answer. Kept while a newer one is on its way.
    pub data: Option<TrophyCompare>,
    /// The other player's trophies are hidden from the player (or the account is gone): the cloud
    /// answers `not_found`.
    pub hidden: bool,
    /// The last attempt failed (and there is no answer to show).
    pub failed: bool,
}

/// The key of a comparison: the other account and the title.
pub type CompareKey = (AccountId, String);

/// How far the player is on the way from their level to the next, 0 to 1. The highest level has
/// no next one and counts as full.
pub fn level_progress(level: &TrophyLevel) -> f32 {
    let Some(to) = level.to else {
        return 1.0;
    };
    let span = to.saturating_sub(level.from);
    if span == 0 {
        return 1.0;
    }
    let done = level.points.saturating_sub(level.from).min(span);
    done as f32 / span as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_the_share_of_the_level_s_points() {
        let level = TrophyLevel::of(72 + 42);
        assert_eq!(level.level, 2);
        // Level 2 spans 72 to 156: 84 points, 42 done.
        assert!((level_progress(&level) - 0.5).abs() < 1e-6);
        assert_eq!(level_progress(&TrophyLevel::of(0)), 0.0);
    }

    #[test]
    fn the_highest_level_is_full() {
        assert_eq!(level_progress(&TrophyLevel::of(70_000)), 1.0);
    }
}

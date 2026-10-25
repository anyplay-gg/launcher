//! Which comparison the trophy compare screen is on (docs/DESIGN.md 17, "Trophy compare"). The
//! screen is a full-screen overlay opened from a game row of a player's profile; Back returns to
//! that profile, on the row it was left from.

use anyps5_core::online::api::AccountId;

use crate::app::model::Model;
use crate::app::overlay::Overlay;
use crate::ui::sound::Cue;

/// The other player and the game (a title ID) the screen compares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub account: AccountId,
    pub title_id: String,
}

impl Model {
    /// Opens the comparison with `account` in a game. The profile stays what it was, so Back finds
    /// its way home.
    pub fn open_compare(&mut self, account: AccountId, title_id: String) {
        self.compare = Some(Target { account, title_id });
        self.switch_overlay(Overlay::Compare);
    }

    /// Back from the comparison, to the profile it was opened from.
    pub fn close_compare(&mut self) {
        if self.profile.is_some() {
            self.sound(Cue::Back);
            self.profile_returning = true;
            self.switch_overlay(Overlay::Profile);
        } else {
            self.sound(Cue::Close);
            self.dismiss_overlay();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::profile::Back;
    use anyps5_core::paths::AppPaths;
    use anyps5_core::settings::Preferences;

    fn model() -> Model {
        let prefs = Preferences {
            onboarded: true,
            ..Preferences::default()
        };
        Model::new(
            prefs,
            AppPaths::under(std::path::Path::new("C:/none")),
            false,
        )
    }

    #[test]
    fn back_from_the_comparison_leads_to_the_profile_it_came_from() {
        let mut m = model();
        m.open_profile(AccountId(2), Back::Friends);
        m.open_compare(AccountId(2), "GAME00001".into());
        assert_eq!(m.overlay, Some(Overlay::Compare));
        assert_eq!(
            m.compare,
            Some(Target {
                account: AccountId(2),
                title_id: "GAME00001".into()
            })
        );
        m.close_compare();
        assert_eq!(m.overlay, Some(Overlay::Profile));
        assert!(m.profile_returning, "the profile keeps its focus");
        assert_eq!(m.profile.map(|t| t.account), Some(AccountId(2)));
    }

    #[test]
    fn back_from_the_comparison_is_a_step_back_and_a_close_only_without_a_profile() {
        let mut m = model();
        m.open_profile(AccountId(2), Back::Friends);
        m.open_compare(AccountId(2), "GAME00001".into());
        m.effects.clear();
        m.close_compare();
        assert!(matches!(
            m.effects.as_slice(),
            [crate::app::model::Effect::Sound(Cue::Back)]
        ));
        let mut m = model();
        m.open_compare(AccountId(2), "GAME00001".into());
        m.effects.clear();
        m.close_compare();
        assert!(matches!(
            m.effects.as_slice(),
            [crate::app::model::Effect::Sound(Cue::Close)]
        ));
    }

    #[test]
    fn without_a_profile_back_closes() {
        let mut m = model();
        m.open_compare(AccountId(2), "GAME00001".into());
        m.close_compare();
        assert_eq!(m.overlay, None);
    }
}

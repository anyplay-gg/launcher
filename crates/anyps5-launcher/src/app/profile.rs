//! Which profile the profile screen is on and where Back leads (docs/DESIGN.md 17, "Profile
//! screen"). The screen is a full-screen overlay; it remembers where the player came from so Back
//! returns there: Home (the profile button), or the online panel on the page it was left on (a
//! friend's page, the feed, Home).

use anyps5_core::online::api::AccountId;

use crate::app::model::Model;
use crate::app::overlay::Overlay;
use crate::ui::sound::Cue;

/// Where Back from the profile screen leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Back {
    Home,
    /// The online panel, on the page it was left on.
    Online,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub account: AccountId,
    pub back: Back,
}

impl Model {
    /// Opens the profile screen on `account`. The caller plays the cue that fits its own press.
    pub fn open_profile(&mut self, account: AccountId, back: Back) {
        self.profile = Some(Target { account, back });
        self.switch_overlay(Overlay::Profile);
    }

    /// The profile button of the top bar: the player's own profile once signed in, the sign-in
    /// page of the online panel before.
    pub fn open_profile_button(&mut self) {
        match self.online.account.as_ref().map(|a| a.id) {
            Some(me) => {
                self.open_profile(me, Back::Home);
                self.sound(Cue::Open);
            }
            None => self.open(Overlay::Online),
        }
    }

    /// Back from the profile screen, to where it was opened from.
    pub fn close_profile(&mut self) {
        match self.profile.map(|t| t.back) {
            Some(Back::Online) => {
                self.online_resume = true;
                self.switch_overlay(Overlay::Online);
            }
            _ => self.dismiss_overlay(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyps5_core::paths::AppPaths;
    use anyps5_core::settings::Preferences;

    fn model_with_games(_count: usize) -> Model {
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
    fn the_button_opens_the_own_profile_once_signed_in_and_sign_in_before() {
        let mut m = model_with_games(1);
        m.open_profile_button();
        assert_eq!(
            m.overlay,
            Some(Overlay::Online),
            "signed out: the sign-in page"
        );
        m.dismiss_overlay();
        m.online.account = Some(anyps5_core::online::SignedIn {
            id: AccountId(7),
            handle: "Mira".into(),
            email: String::new(),
            appear_offline: false,
        });
        m.open_profile_button();
        assert_eq!(m.overlay, Some(Overlay::Profile));
        assert_eq!(
            m.profile,
            Some(Target {
                account: AccountId(7),
                back: Back::Home
            })
        );
    }

    #[test]
    fn back_leads_where_the_profile_was_opened_from() {
        let mut m = model_with_games(1);
        m.open_profile(AccountId(2), Back::Home);
        m.close_profile();
        assert_eq!(m.overlay, None);
        assert!(!m.online_resume);
        m.open_profile(AccountId(2), Back::Online);
        m.close_profile();
        assert_eq!(m.overlay, Some(Overlay::Online));
        assert!(m.online_resume, "the panel comes back on the page it left");
    }
}

//! Which profile the profile screen is on and where Back leads (docs/DESIGN.md 17, "Profile
//! screen"). The screen is a full-screen overlay; it remembers where the player came from so Back
//! returns there: Home (the profile button), or the Friends screen on the page it was left on (a
//! friend's page, the activity, Home).

use anyps5_core::online::api::AccountId;

use crate::app::model::Model;
use crate::app::overlay::Overlay;
use crate::ui::sound::Cue;

/// Where Back from the profile screen leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Back {
    Home,
    /// The Friends screen, on the page it was left on.
    Friends,
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
    /// page of the Friends screen before.
    pub fn open_profile_button(&mut self) {
        match self.online.account.as_ref().map(|a| a.id) {
            Some(me) => {
                self.open_profile(me, Back::Home);
                self.sound(Cue::Select);
            }
            None => self.open(Overlay::Friends),
        }
    }

    /// Back from the profile screen, to where it was opened from.
    pub fn close_profile(&mut self) {
        match self.profile.map(|t| t.back) {
            Some(Back::Friends) => {
                self.online_resume = true;
                self.switch_overlay(Overlay::Friends);
            }
            _ => self.dismiss_overlay(),
        }
    }

    /// The profile screen sends the player on to the Friends screen: Back from there leads here.
    pub fn open_friends_from_profile(&mut self) {
        self.friends_back = self.profile;
        self.switch_overlay(Overlay::Friends);
    }

    /// Back from the first page of the Friends screen: to the profile it was opened from, else
    /// home.
    pub fn close_friends(&mut self) {
        match self.friends_back.take() {
            Some(target) => {
                self.profile = Some(target);
                self.switch_overlay(Overlay::Profile);
            }
            None => self.dismiss_overlay(),
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
            Some(Overlay::Friends),
            "signed out: the sign-in page"
        );
        m.dismiss_overlay();
        m.online.account = Some(anyps5_core::online::SignedIn {
            id: AccountId(7),
            handle: "Mira".into(),
            email: String::new(),
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
        m.open_profile(AccountId(2), Back::Friends);
        m.close_profile();
        assert_eq!(m.overlay, Some(Overlay::Friends));
        assert!(m.online_resume, "the screen comes back on the page it left");
    }
}

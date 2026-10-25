//! The join code behind the Join button on the player's Discord profile (docs/DESIGN.md 7.4).
//!
//! The cloud issues a code that stands for the player's account; its link is what the button
//! opens ([ADR 0022] there). This module only decides *when to ask* for a code and *when to give
//! it back*; [`super::Online`] sends the commands and feeds the answers in.
//!
//! - A code is asked for once there is something to join (a session or a party) and again a
//!   little before it runs out.
//! - A cloud that answers `not_found` has nothing joinable: no code, no button, and no new
//!   question until the session or party is another one.
//! - The code is revoked when the game ends, the player signs out or the setting goes off.
//!
//! The code and its link are secrets of a sort (whoever has them can ask for an invitation), so
//! nothing here prints them, `Debug` included.
//!
//! [ADR 0022]: https://github.com/anyplaynet/network/blob/main/docs/adr/0022-a-join-link-asks-it-never-joins.md

use std::fmt;

use super::api;

/// A new code is asked for when the one held has less than this long to live.
pub const REFRESH_BEFORE_SECS: u64 = 10 * 60;
/// The least time between two questions, so a flapping session cannot use up the cloud's limit
/// of ten new codes an hour.
pub const MIN_GAP_SECS: u64 = 30;
/// The wait after a `rate_limited` answer.
pub const RATE_LIMITED_WAIT_SECS: u64 = 15 * 60;

/// What [`JoinOffer::step`] wants done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Nothing,
    /// Send `join_code`.
    Ask,
    /// Send `join_code_revoke`.
    Revoke,
}

#[derive(Clone, PartialEq, Eq)]
struct Issued {
    url: String,
    expires: u64,
}

/// The join code of the player, as far as the launcher knows.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct JoinOffer {
    issued: Option<Issued>,
    /// A `join_code` waits for its answer, asked while this was offered (a session or party ID).
    asking: Option<Option<String>>,
    /// The offer for which the cloud said it has nothing to join.
    refused: Option<String>,
    /// No question before this time (seconds since the epoch).
    quiet_until: u64,
}

impl fmt::Debug for JoinOffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JoinOffer")
            .field("held", &self.issued.is_some())
            .field("asking", &self.asking.is_some())
            .finish_non_exhaustive()
    }
}

impl JoinOffer {
    /// What to do now. `active` is whether a code should exist at all (presence and the join
    /// button are on, the player plays online); `offer` identifies what could be joined (the
    /// session, else the party), none when there is nothing. `now` is seconds since the epoch.
    pub fn step(&mut self, active: bool, offer: Option<&str>, now: u64) -> Step {
        if !active {
            self.refused = None;
            return if self.issued.take().is_some() {
                Step::Revoke
            } else {
                Step::Nothing
            };
        }
        let Some(offer) = offer else {
            self.refused = None;
            return Step::Nothing;
        };
        if self.asking.is_some() || self.refused.as_deref() == Some(offer) || now < self.quiet_until
        {
            return Step::Nothing;
        }
        let fresh = self
            .issued
            .as_ref()
            .is_some_and(|i| i.expires > now + REFRESH_BEFORE_SECS);
        if fresh {
            return Step::Nothing;
        }
        self.asking = Some(Some(offer.to_owned()));
        self.quiet_until = now + MIN_GAP_SECS;
        Step::Ask
    }

    /// The cloud issued a code.
    pub fn issued(&mut self, code: api::JoinCode) {
        self.asking = None;
        self.refused = None;
        self.issued = Some(Issued {
            url: code.url,
            expires: code.expires,
        });
    }

    /// The question failed. `nothing_to_join` is the cloud's `not_found`; `rate_limited` makes the
    /// next question wait a quarter of an hour.
    pub fn failed(&mut self, nothing_to_join: bool, rate_limited: bool, now: u64) {
        let offer = self.asking.take().flatten();
        if nothing_to_join {
            self.refused = offer;
            self.issued = None;
        }
        if rate_limited {
            self.quiet_until = now + RATE_LIMITED_WAIT_SECS;
        }
    }

    /// The link for the button: the code's address, while the code is alive.
    pub fn url(&self, now: u64) -> Option<&str> {
        self.issued
            .as_ref()
            .filter(|i| i.expires > now)
            .map(|i| i.url.as_str())
    }

    /// The player signs out: whether a code is held that must go back first. It is forgotten
    /// either way.
    pub fn take_for_sign_out(&mut self) -> bool {
        let held = self.issued.is_some();
        *self = JoinOffer::default();
        held
    }

    /// Whether a code is held or asked for.
    pub fn is_open(&self) -> bool {
        self.issued.is_some() || self.asking.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(expires: u64) -> api::JoinCode {
        api::JoinCode {
            code: "k3x9m2pqra".into(),
            url: "https://anyplaynet.com/join/k3x9m2pqra".into(),
            expires,
        }
    }

    #[test]
    fn a_code_is_asked_for_once_there_is_something_to_join() {
        let mut offer = JoinOffer::default();
        assert_eq!(offer.step(true, None, 100), Step::Nothing);
        assert_eq!(offer.step(true, Some("s1"), 100), Step::Ask);
        assert_eq!(offer.step(true, Some("s1"), 101), Step::Nothing, "it waits");
        offer.issued(code(100 + 7200));
        assert_eq!(offer.step(true, Some("s1"), 200), Step::Nothing);
        assert_eq!(
            offer.url(200).map(|u| u.ends_with("k3x9m2pqra")),
            Some(true)
        );
    }

    #[test]
    fn a_code_is_renewed_shortly_before_it_ends() {
        let mut offer = JoinOffer::default();
        assert_eq!(offer.step(true, Some("s1"), 0), Step::Ask);
        offer.issued(code(7200));
        let late = 7200 - REFRESH_BEFORE_SECS;
        assert_eq!(offer.step(true, Some("s1"), late - 1), Step::Nothing);
        assert_eq!(offer.step(true, Some("s1"), late), Step::Ask);
    }

    #[test]
    fn nothing_to_join_is_not_asked_again_until_the_offer_changes() {
        let mut offer = JoinOffer::default();
        assert_eq!(offer.step(true, Some("party1"), 0), Step::Ask);
        offer.failed(true, false, 0);
        assert_eq!(offer.url(0), None);
        assert_eq!(offer.step(true, Some("party1"), 1000), Step::Nothing);
        assert_eq!(
            offer.step(true, Some("s2"), 1000),
            Step::Ask,
            "another offer"
        );
    }

    #[test]
    fn questions_keep_a_distance_and_a_limit_waits_longer() {
        let mut offer = JoinOffer::default();
        assert_eq!(offer.step(true, Some("a"), 0), Step::Ask);
        offer.failed(false, false, 0);
        assert_eq!(offer.step(true, Some("a"), MIN_GAP_SECS - 1), Step::Nothing);
        assert_eq!(offer.step(true, Some("a"), MIN_GAP_SECS), Step::Ask);
        offer.failed(false, true, 100);
        assert_eq!(
            offer.step(true, Some("a"), 100 + RATE_LIMITED_WAIT_SECS - 1),
            Step::Nothing
        );
        assert_eq!(
            offer.step(true, Some("a"), 100 + RATE_LIMITED_WAIT_SECS),
            Step::Ask
        );
    }

    #[test]
    fn the_code_goes_back_when_it_is_no_longer_wanted() {
        let mut offer = JoinOffer::default();
        assert_eq!(
            offer.step(false, None, 0),
            Step::Nothing,
            "nothing to give back"
        );
        assert_eq!(offer.step(true, Some("s"), 0), Step::Ask);
        offer.issued(code(7200));
        assert_eq!(offer.step(false, Some("s"), 10), Step::Revoke);
        assert_eq!(offer.step(false, Some("s"), 11), Step::Nothing, "once");
        assert_eq!(offer.url(11), None);
    }

    #[test]
    fn a_code_that_arrives_after_the_game_ended_is_given_back_at_once() {
        let mut offer = JoinOffer::default();
        assert_eq!(offer.step(true, Some("s"), 0), Step::Ask);
        assert_eq!(
            offer.step(false, Some("s"), 1),
            Step::Nothing,
            "still asking"
        );
        assert!(offer.is_open());
        offer.issued(code(7200));
        assert_eq!(offer.step(false, None, 2), Step::Revoke);
    }

    #[test]
    fn signing_out_gives_a_held_code_back_first() {
        let mut offer = JoinOffer::default();
        assert!(!offer.take_for_sign_out());
        assert_eq!(offer.step(true, Some("s"), 0), Step::Ask);
        offer.issued(code(7200));
        assert!(offer.take_for_sign_out());
        assert!(!offer.is_open());
    }

    #[test]
    fn an_expired_code_shows_no_link() {
        let mut offer = JoinOffer::default();
        assert_eq!(offer.step(true, Some("s"), 0), Step::Ask);
        offer.issued(code(500));
        assert!(offer.url(499).is_some());
        assert_eq!(offer.url(500), None);
    }

    #[test]
    fn debug_output_never_holds_the_code() {
        let mut offer = JoinOffer::default();
        offer.step(true, Some("s"), 0);
        offer.issued(code(7200));
        let text = format!("{offer:?}");
        assert!(
            !text.contains("k3x9m2pqra") && !text.contains("anyplaynet"),
            "{text}"
        );
    }
}

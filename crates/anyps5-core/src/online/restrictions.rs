//! What limits the player's account, as the launcher shows it.
//!
//! The net service tells the launcher about mutes (the `restrictions` event) and about a ban (the
//! `banned` event), and a failed command can carry the restriction that stopped it. This is the
//! pure state behind that: which restrictions are in force at a given time, and which of them
//! stops a given action. The panel only reads it.

use anyplay_control::Restriction;

/// What a player may be stopped from doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    /// Sending a friend request.
    FriendRequest,
    /// Writing to someone who is not a friend.
    Stranger,
    /// Writing to friends, a group or the party.
    Message,
}

/// The restrictions of the signed-in account.
#[derive(Debug, Default)]
pub struct Restrictions {
    /// The mutes the net service listed, at most one for each kind.
    held: Vec<Restriction>,
    /// What a refused command taught: the action it was, or `None` for every action.
    learned: Vec<(Option<Blocked>, Restriction)>,
    /// The ban. It outlives the sign-out it causes, up to the next sign-in.
    banned: Option<Restriction>,
    /// The action numbers of the held mutes, to tell a new one from one already known.
    seen: Vec<u64>,
    /// Whether the service listed at least once since sign-in.
    listed: bool,
}

/// True while `restriction` runs at `now` (Unix seconds).
pub fn in_force(restriction: &Restriction, now: u64) -> bool {
    restriction.until.is_none_or(|until| now < until)
}

/// Of two restrictions of one kind, the one that lasts longer (no end lasts longest).
fn longer(a: Restriction, b: Restriction) -> Restriction {
    match (a.until, b.until) {
        (None, _) => a,
        (_, None) => b,
        (Some(x), Some(y)) => {
            if y > x {
                b
            } else {
                a
            }
        }
    }
}

/// The restrictions with one for each kind: the one that lasts longest.
fn one_of_each_kind(list: Vec<Restriction>) -> Vec<Restriction> {
    let mut kept: Vec<Restriction> = Vec::new();
    for restriction in list {
        match kept.iter().position(|k| k.kind == restriction.kind) {
            Some(index) => {
                let old = kept.remove(index);
                kept.insert(index, longer(old, restriction));
            }
            None => kept.push(restriction),
        }
    }
    kept
}

impl Restrictions {
    /// Keeps the mutes the service listed. Returns the ones that are new since the service last
    /// listed, none for the first list: those the player may already know of.
    pub fn list(&mut self, list: Vec<Restriction>) -> Vec<Restriction> {
        let first = !self.listed;
        self.listed = true;
        let numbers: Vec<u64> = list.iter().filter_map(|r| r.action).collect();
        let fresh = list
            .iter()
            .filter(|r| r.action.is_none_or(|n| !self.seen.contains(&n)))
            .cloned()
            .collect();
        self.seen = numbers;
        self.held = one_of_each_kind(list);
        // What the service lists is the truth about mutes: a mute learned from a refusal that the
        // list does not hold is over.
        self.learned.retain(|(_, r)| r.kind != Restriction::MUTED);
        if first { Vec::new() } else { fresh }
    }

    /// Remembers what a refused command told. `blocked` is the action it was, when the launcher
    /// knows it. A ban is kept as the ban.
    pub fn learn(&mut self, blocked: Option<Blocked>, restriction: &Restriction) {
        if restriction.kind == Restriction::BANNED {
            self.ban(restriction.clone());
            return;
        }
        // A mute limits everything; a new account's limit belongs to the action that met it.
        let target = if restriction.kind == Restriction::MUTED {
            None
        } else {
            blocked
        };
        if restriction.kind == Restriction::NEW_ACCOUNT && target.is_none() {
            return;
        }
        self.learned
            .retain(|(t, r)| !(*t == target && r.kind == restriction.kind));
        self.learned.push((target, restriction.clone()));
    }

    /// Keeps the ban the account got.
    pub fn ban(&mut self, restriction: Restriction) {
        self.banned = Some(restriction);
    }

    /// The account signed in: a ban of before is over or will be met again.
    pub fn signed_in(&mut self) {
        self.banned = None;
    }

    /// The account signed out: what belonged to it goes, the ban stays.
    pub fn signed_out(&mut self) {
        self.held.clear();
        self.learned.clear();
        self.seen.clear();
        self.listed = false;
    }

    /// The ban, while the account is signed out because of it.
    pub fn banned(&self) -> Option<&Restriction> {
        self.banned.as_ref()
    }

    /// The restrictions in force at `now`, one for each kind: the mutes the service listed, then
    /// what refusals taught that the list does not already say.
    pub fn active(&self, now: u64) -> Vec<&Restriction> {
        let mut active: Vec<&Restriction> = self.held.iter().filter(|r| in_force(r, now)).collect();
        for (_, learned) in &self.learned {
            if in_force(learned, now) && !active.iter().any(|a| a.kind == learned.kind) {
                active.push(learned);
            }
        }
        active
    }

    /// The restriction in force at `now` that stops `what`, if any.
    pub fn blocking(&self, what: Blocked, now: u64) -> Option<&Restriction> {
        let held = self
            .held
            .iter()
            .find(|r| r.kind == Restriction::MUTED && in_force(r, now));
        held.or_else(|| {
            self.learned
                .iter()
                .filter(|(_, r)| in_force(r, now))
                .find(|(target, _)| match target {
                    None => true,
                    Some(Blocked::Stranger) => what == Blocked::Stranger,
                    Some(target) => *target == what,
                })
                .map(|(_, r)| r)
        })
    }

    /// The restriction of `kind` in force at `now`, for its page.
    pub fn of_kind(&self, kind: &str, now: u64) -> Option<&Restriction> {
        if kind == Restriction::BANNED {
            return self.banned.as_ref();
        }
        self.active(now).into_iter().find(|r| r.kind == kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mute(action: u64, until: Option<u64>) -> Restriction {
        Restriction {
            action: Some(action),
            rule: Some("spam".into()),
            reason: Some("Links in every message.".into()),
            ..Restriction::new(Restriction::MUTED, until)
        }
    }

    #[test]
    fn a_mute_stops_every_action_until_it_ends() {
        let mut restrictions = Restrictions::default();
        restrictions.list(vec![mute(1, Some(1000))]);
        for what in [Blocked::FriendRequest, Blocked::Stranger, Blocked::Message] {
            assert_eq!(
                restrictions.blocking(what, 999).map(|r| r.action),
                Some(Some(1))
            );
            assert_eq!(restrictions.blocking(what, 1000), None, "over at its end");
        }
        assert!(restrictions.active(1000).is_empty());
    }

    #[test]
    fn a_mute_for_good_has_no_end() {
        let mut restrictions = Restrictions::default();
        restrictions.list(vec![mute(1, None)]);
        assert!(restrictions.blocking(Blocked::Message, u64::MAX).is_some());
    }

    #[test]
    fn of_two_mutes_the_one_that_lasts_longer_shows() {
        let mut restrictions = Restrictions::default();
        restrictions.list(vec![
            mute(1, Some(500)),
            mute(2, Some(900)),
            mute(3, Some(700)),
        ]);
        let active = restrictions.active(0);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].until, Some(900));
        restrictions.list(vec![mute(1, Some(500)), mute(4, None)]);
        assert_eq!(restrictions.active(0)[0].until, None);
    }

    #[test]
    fn only_a_mute_that_arrives_later_is_new() {
        let mut restrictions = Restrictions::default();
        // The first list holds what the player may know of already.
        assert!(restrictions.list(vec![mute(1, Some(500))]).is_empty());
        assert!(restrictions.list(vec![mute(1, Some(500))]).is_empty());
        let fresh = restrictions.list(vec![mute(1, Some(500)), mute(2, Some(900))]);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].action, Some(2));
        // A mute that was lifted and comes back with another number is new again.
        assert!(restrictions.list(Vec::new()).is_empty());
        assert_eq!(restrictions.list(vec![mute(3, Some(900))]).len(), 1);
    }

    #[test]
    fn a_new_accounts_limit_belongs_to_the_action_that_met_it() {
        let mut restrictions = Restrictions::default();
        let limit = Restriction::new(Restriction::NEW_ACCOUNT, Some(2000));
        restrictions.learn(Some(Blocked::FriendRequest), &limit);
        assert!(restrictions.blocking(Blocked::FriendRequest, 100).is_some());
        assert!(restrictions.blocking(Blocked::Stranger, 100).is_none());
        assert!(restrictions.blocking(Blocked::Message, 100).is_none());
        assert_eq!(restrictions.blocking(Blocked::FriendRequest, 2000), None);

        restrictions.learn(Some(Blocked::Stranger), &limit);
        assert!(restrictions.blocking(Blocked::Stranger, 100).is_some());
        assert!(restrictions.blocking(Blocked::Message, 100).is_none());
        // A refusal from a command the launcher cannot place teaches nothing about an action.
        let mut other = Restrictions::default();
        other.learn(None, &limit);
        assert!(other.active(0).is_empty());
    }

    #[test]
    fn a_mute_learned_from_a_refusal_goes_when_the_list_does_not_hold_it() {
        let mut restrictions = Restrictions::default();
        restrictions.learn(
            Some(Blocked::Message),
            &Restriction::new(Restriction::MUTED, Some(5000)),
        );
        assert!(restrictions.blocking(Blocked::FriendRequest, 0).is_some());
        restrictions.list(Vec::new());
        assert!(restrictions.blocking(Blocked::FriendRequest, 0).is_none());
    }

    #[test]
    fn the_list_says_more_than_a_refusal() {
        let mut restrictions = Restrictions::default();
        restrictions.learn(None, &Restriction::new(Restriction::MUTED, Some(5000)));
        assert_eq!(restrictions.active(0)[0].reason, None);
        restrictions.list(vec![mute(1, Some(5000))]);
        let active = restrictions.active(0);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].reason.as_deref(), Some("Links in every message."));
    }

    #[test]
    fn a_ban_outlives_the_sign_out_and_ends_at_the_next_sign_in() {
        let mut restrictions = Restrictions::default();
        restrictions.list(vec![mute(1, Some(500))]);
        let ban = Restriction::new(Restriction::BANNED, Some(9000));
        restrictions.learn(None, &ban);
        assert_eq!(restrictions.banned(), Some(&ban));
        restrictions.signed_out();
        assert_eq!(restrictions.banned(), Some(&ban));
        assert!(restrictions.active(0).is_empty());
        assert_eq!(restrictions.of_kind("banned", 0), Some(&ban));
        restrictions.signed_in();
        assert_eq!(restrictions.banned(), None);
    }

    #[test]
    fn signing_out_forgets_the_mutes_and_the_first_list_is_quiet_again() {
        let mut restrictions = Restrictions::default();
        restrictions.list(vec![mute(1, Some(500))]);
        restrictions.signed_out();
        assert!(restrictions.active(0).is_empty());
        assert!(restrictions.list(vec![mute(2, Some(500))]).is_empty());
    }
}

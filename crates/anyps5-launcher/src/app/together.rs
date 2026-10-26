//! Play Together on the launcher's side of the model (docs/DESIGN.md 17, "Play Together").
//!
//! Everything the player does about it goes through here and ends in one of two things: a game that
//! starts (hosting, or joining a session), or a [`Blocker`] the card in front of the player says in
//! words. The pure rules (the table, the join check) are `anyps5_core::play_together`; the net
//! service's answers come back through [`OnlineService`](super::online::OnlineService), which hands
//! them on to the functions here.
//!
//! The model keeps four small things: the blocker on show, the offers the player closed, the host
//! plan the next start of a game carries, and the session the player hosts.

use std::collections::BTreeSet;

use anyps5_core::library::Game;
use anyps5_core::online::api::{AccountId, Invitation, SessionKind, VersionRule};
use anyps5_core::online::control::Host;
use anyps5_core::online::{Command, Join, Purpose, Status};
use anyps5_core::paths::GameId;
use anyps5_core::play_together::{self, Blocker, Offer, Owned, World};

use crate::app::model::{Effect, GameState, Model};
use crate::app::online::build;

/// State of Play Together between frames.
#[derive(Debug, Default)]
pub struct Together {
    /// Why the last press did not go ahead. The card shows it until it is closed.
    pub blocked: Option<Blocker>,
    /// Offers the player said "decline" to: an invitation's or a session's ID.
    dismissed: BTreeSet<String>,
    /// Play Together was pressed: the next start of this game opens a session.
    pub host: Option<(GameId, Host)>,
    /// The session this launcher's running game opened.
    pub hosting: Option<String>,
    /// The offer the last join was for: what "Try Anyway" joins when the versions do not fit.
    attempt: Option<Offer>,
}

/// What the card in front of the player shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Card {
    /// "Mira is playing Minecraft. Join?"
    Offer {
        /// What "decline" remembers.
        key: String,
        offer: Offer,
        /// The invitation behind it, if one.
        invitation: Option<String>,
    },
    /// Why a join or a hosting did not go ahead.
    Blocked(Blocker),
}

/// What the player did on the card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Join(Offer),
    /// "Decline": the key, and the invitation to decline.
    Decline {
        key: String,
        invitation: Option<String>,
    },
    /// Close the blocker.
    Close,
    /// The blocker's remedy.
    Fix(Fix),
    /// "Try Anyway" on a version blocker: join the last offer although the versions do not fit.
    TryAnyway,
}

/// What mends a blocker from the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    SignIn,
    SwitchOn,
}

impl Fix {
    /// The remedy of a blocker, if the card can do it.
    pub fn of(blocker: &Blocker) -> Option<Fix> {
        match blocker {
            Blocker::SignedOut => Some(Fix::SignIn),
            Blocker::OnlineOff => Some(Fix::SwitchOn),
            _ => None,
        }
    }
}

/// Does the blocker offer "Try Anyway" beside its primary action?
pub fn can_try_anyway(blocker: &Blocker) -> bool {
    matches!(blocker, Blocker::WrongVersion { .. })
}

impl Together {
    pub fn block(&mut self, blocker: Blocker) {
        self.blocked = Some(blocker);
    }

    /// The player is joining or said "decline": the offer stays off the card.
    pub fn dismiss(&mut self, key: &str) {
        self.dismissed.insert(key.to_owned());
    }

    /// The host plan for a start of `game`, once.
    pub fn take_host(&mut self, game: &GameId) -> Option<Host> {
        match self.host.take() {
            Some((id, host)) if &id == game => Some(host),
            _ => None,
        }
    }

    /// The game ended (or never started): nothing is hosted and no plan waits.
    pub fn game_ended(&mut self) {
        self.hosting = None;
        self.host = None;
    }
}

// ---------------------------------------------------------------------------------------------
// The player's world

/// What Play Together needs to know about the player, for `title_id`.
pub fn world(model: &Model, title_id: &str) -> World {
    let running = model.online.status == Status::Running;
    let owned = model
        .games
        .iter()
        .find(|g| g.title_id == title_id)
        .map(|g| Owned {
            version: g.version.clone(),
            unlocked: matches!(model.state_of(g), GameState::Installed { .. }),
        });
    let busy = model.starting.is_some() || model.running.is_some();
    // Signed in on this PC, but the net service is not running: online play is off, not the
    // account. The check names that one.
    if model.prefs.online && !running {
        return World {
            signed_in: true,
            online: true,
            online_play: false,
            owned,
            build: build(model),
            busy,
        };
    }
    World {
        signed_in: model.online.is_signed_in(),
        online: model.online.is_online(),
        online_play: true,
        owned,
        build: build(model),
        busy,
    }
}

// ---------------------------------------------------------------------------------------------
// Hosting

/// The selected game, when Play Together applies to its page: it can be played together and
/// starts now (an unlocked game that is not running).
pub fn hostable(model: &Model) -> Option<&Game> {
    let game = model.selected_game()?;
    play_together::lan_title(&game.title_id)?;
    matches!(model.state_of(game), GameState::Installed { .. }).then_some(game)
}

/// Play Together was pressed on a game's page or in the party: start the game, with a session.
pub fn host_game(model: &mut Model, id: &GameId) {
    let Some(game) = model.game(id).cloned() else {
        return;
    };
    let in_party = model.online.social.party.is_some();
    let world = world(model, &game.title_id);
    match play_together::host(&game.title_id, &game.name, in_party, &world) {
        Err(blocker) => model.together.block(blocker),
        Ok(plan) => {
            model.together.blocked = None;
            model.together.host = Some((id.clone(), plan));
            model.play(id);
            // `play` refuses while something else is starting: no plan is left for a later Play.
            if model.starting.as_ref() != Some(id) {
                model.together.host = None;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Joining

/// A friend's session: from their row or their profile.
pub fn join_friend(model: &mut Model, friend: AccountId) {
    let Some(friend) = model.online.friend(friend) else {
        return;
    };
    match Offer::of_friend(friend) {
        Ok(offer) => join(model, offer),
        Err(blocker) => model.together.block(blocker),
    }
}

/// An invitation the player accepted.
pub fn join_invitation(model: &mut Model, invitation: &Invitation) {
    join(model, Offer::of_invitation(invitation));
}

/// The local checks, then the net service's. The answer comes back to
/// [`OnlineService`](super::online::OnlineService), which starts the game.
pub fn join(model: &mut Model, offer: Offer) {
    join_with(model, offer, false);
}

/// [`join`], or with `try_anyway` the same join that skips the version check on purpose: the
/// player read that the versions do not fit and chose to go on. The net service and the cloud are
/// told with an explicit flag, and the launcher's log keeps the choice. Everything else still holds.
fn join_with(model: &mut Model, offer: Offer, try_anyway: bool) {
    let world = world(model, &offer.title_id);
    let rule = play_together::version_rule(&offer.title_id);
    model.together.attempt = Some(offer.clone());
    let accepted = if try_anyway { VersionRule::Any } else { rule };
    if let Err(blocker) = play_together::check_as(&offer, &world, accepted) {
        model.together.block(blocker);
        return;
    }
    model.together.blocked = None;
    let have = world.owned.map(|o| o.version).unwrap_or_default();
    if try_anyway {
        tracing::info!(
            session = %offer.session,
            title_id = %offer.title_id,
            host_version = %offer.version,
            own_version = %have,
            ?rule,
            "Play Together: joining although the versions do not fit (Try Anyway)"
        );
    }
    let command = Command::CheckJoin {
        session: offer.session.clone(),
        title_id: offer.title_id.clone(),
        version: have.clone(),
        build: world.build.clone(),
        version_rule: rule,
        override_version: try_anyway,
    };
    model.online.send(
        Purpose::CheckJoin(Join {
            session: offer.session,
            title_id: offer.title_id,
            title: offer.title,
            version: have,
            build: world.build,
            friend: offer.host,
            rule,
            override_version: try_anyway,
        }),
        command,
    );
}

// ---------------------------------------------------------------------------------------------
// The card

/// What the card shows now: a blocker first, else an offer to join.
///
/// Offers are the Play Together invitations and the party's friends who play in a Play Together
/// session the player is not in. An invitation to any other kind of session keeps its place in the
/// panel's Home page.
pub fn card(model: &Model) -> Option<Card> {
    if let Some(blocker) = &model.together.blocked {
        return Some(Card::Blocked(blocker.clone()));
    }
    let own = model.online.session.as_ref().map(|s| s.id.as_str());
    let open = |session: &str| Some(session) != own && !model.together.dismissed.contains(session);
    for invitation in &model.online.invitations {
        if invitation.kind == SessionKind::Lan && open(&invitation.session) {
            return Some(Card::Offer {
                key: invitation.session.clone(),
                offer: Offer::of_invitation(invitation),
                invitation: Some(invitation.id.clone()),
            });
        }
    }
    let party = model.online.social.party.as_ref()?;
    let me = model.online.account.as_ref().map(|a| a.id);
    for member in &party.members {
        if Some(member.account.id) == me {
            continue;
        }
        let Some(friend) = model.online.friend(member.account.id) else {
            continue;
        };
        if let Ok(offer) = Offer::of_friend(friend)
            && offer.kind == SessionKind::Lan
            && open(&offer.session)
        {
            return Some(Card::Offer {
                key: offer.session.clone(),
                offer,
                invitation: None,
            });
        }
    }
    None
}

/// Back (Esc, the pad's back button) with the card on screen: a blocker closes, an offer is put away
/// (an invitation is not declined by it). True when the card took the press.
pub fn escape(model: &mut Model) -> bool {
    let action = match card(model) {
        Some(Card::Blocked(_)) => Action::Close,
        Some(Card::Offer { key, .. }) => {
            // Back only puts the card away; an invitation stays in the Friends screen to answer.
            model.together.dismiss(&key);
            return true;
        }
        None => return false,
    };
    apply(model, action);
    true
}

/// Carries out what the player did on the card.
pub fn apply(model: &mut Model, action: Action) {
    match action {
        Action::Join(offer) => join(model, offer),
        Action::Decline { key, invitation } => {
            model.together.dismiss(&key);
            if let Some(id) = invitation {
                model.online.send(
                    Purpose::AnswerInvitation {
                        id: id.clone(),
                        accept: false,
                    },
                    Command::AnswerInvitation { id, accept: false },
                );
            }
        }
        Action::Close => model.together.blocked = None,
        Action::TryAnyway => match model.together.attempt.clone() {
            Some(offer) => join_with(model, offer, true),
            None => model.together.blocked = None,
        },
        Action::Fix(fix) => {
            model.together.blocked = None;
            match fix {
                Fix::SignIn => model.apply_intent(crate::ui::focus::HomeIntent::OpenOnline),
                Fix::SwitchOn => model.effects.push(Effect::StartOnline),
            }
        }
    }
}

/// The game a join starts: the one in the library with the join's title ID.
pub fn game_of<'a>(model: &'a Model, join: &Join) -> Option<&'a Game> {
    model.games.iter().find(|g| g.title_id == join.title_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_plan_is_taken_once_and_only_by_its_game() {
        let mut together = Together::default();
        let plan = Host {
            max_members: 8,
            visibility: anyps5_core::online::api::Visibility::Friends,
            party: false,
        };
        together.host = Some((GameId::from_key("a"), plan.clone()));
        assert_eq!(together.take_host(&GameId::from_key("b")), None);
        together.host = Some((GameId::from_key("a"), plan.clone()));
        assert_eq!(together.take_host(&GameId::from_key("a")), Some(plan));
        assert_eq!(together.take_host(&GameId::from_key("a")), None);
    }

    #[test]
    fn only_two_blockers_have_a_remedy_on_the_card() {
        assert_eq!(Fix::of(&Blocker::SignedOut), Some(Fix::SignIn));
        assert_eq!(Fix::of(&Blocker::OnlineOff), Some(Fix::SwitchOn));
        assert_eq!(Fix::of(&Blocker::Full), None);
        assert_eq!(Fix::of(&Blocker::Busy), None);
    }
}

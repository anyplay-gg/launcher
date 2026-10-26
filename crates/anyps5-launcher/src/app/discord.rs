//! The Discord profile while a game runs (docs/DESIGN.md 7.4): what to show, from the running
//! game, the player's session or party and the join code the network issued.
//!
//! Every frame [`LauncherApp::sync_discord`] works out what the profile should say and sends it
//! when it differs from what Discord shows. The join code is asked for and given back by
//! [`Online::sync_join_offer`](anyps5_core::online::Online::sync_join_offer); here it only
//! becomes a button.

use std::time::SystemTime;

use anyps5_core::discord::{self, Online as OnAnyPlay, Playing, Presence};
use anyps5_core::online::{Online, unix_now};
use anyps5_core::settings::Preferences;

use crate::app::LauncherApp;

/// The game Discord is told about, and since when.
pub struct Played {
    game: String,
    since: SystemTime,
}

impl Played {
    pub fn now(game: String) -> Self {
        Self {
            game,
            since: SystemTime::now(),
        }
    }
}

/// What the profile should show, or none when nothing is to be sent.
///
/// - Presence off: nothing, buttons included.
/// - Signed in and online with a session or party: "on AnyPlay", the party, and the join link
///   when the button is on and the network issued a code.
/// - Otherwise the launcher's plain line.
pub fn wanted(
    played: Option<&Played>,
    prefs: &Preferences,
    online: &Online,
    now: u64,
) -> Option<Playing> {
    let played = played.filter(|_| prefs.discord_presence)?;
    let party = online.social.party.as_ref();
    let on_anyplay = online.is_online() && (online.session.is_some() || party.is_some());
    let online = on_anyplay.then(|| OnAnyPlay {
        party: party.map(|p| (p.members.len(), p.max_members)),
        join_url: prefs
            .discord_join_button
            .then(|| online.join.url(now))
            .flatten()
            .map(str::to_owned),
    });
    Some(Playing {
        game: played.game.clone(),
        since_unix: discord::unix_seconds(played.since),
        online,
    })
}

impl LauncherApp {
    /// Every frame: keeps the join code and the profile in step with the game and the network.
    /// Previews, the demo and tests (`discord` off) never come here with a game and send nothing.
    pub(super) fn sync_discord(&mut self) {
        let playing = self.discord && self.playing.is_some();
        let button =
            playing && self.model.prefs.discord_presence && self.model.prefs.discord_join_button;
        let _ = self.model.online.sync_join_offer(button);
        let wanted = if self.discord {
            wanted(
                self.playing.as_ref(),
                &self.model.prefs,
                &self.model.online,
                unix_now(),
            )
        } else {
            None
        };
        let Some(wanted) = wanted else {
            self.clear_presence();
            return;
        };
        if self.shown.as_ref() == Some(&wanted) {
            return;
        }
        let cache = self.model.paths.cache_dir().join("discord-games.json");
        self.presence
            .get_or_insert_with(|| Presence::spawn(cache))
            .playing(wanted.clone());
        self.shown = Some(wanted);
    }
}

#[cfg(test)]
mod tests {
    use anyps5_core::online::api::{AccountId, JoinCode, Party, PartyMember};
    use anyps5_core::online::{Cloud, Event, Line, Output, Purpose, SignedIn};

    use super::*;

    /// The preferences of a player who turned the status on: it is off for a new profile.
    fn presence_on() -> Preferences {
        Preferences {
            discord_presence: true,
            ..Preferences::default()
        }
    }

    fn played() -> Played {
        Played::now("Hollow Orchard".into())
    }

    fn signed_in(online: &mut Online) {
        online.apply(Line::Output(Output::Event(Event::State {
            cloud: Cloud::Online,
            account: Some(SignedIn {
                id: AccountId(1),
                handle: "Alec".into(),
                email: "a@example.org".into(),
                appear_offline: false,
            }),
            pending: None,
        })));
        online.take_outbox();
    }

    fn party(members: usize, max: u16) -> Party {
        Party {
            id: "party-1".into(),
            leader: AccountId(1),
            members: (0..members)
                .map(|n| PartyMember {
                    account: anyps5_core::online::api::Account {
                        id: AccountId(n as u64 + 1),
                        handle: format!("p{n}"),
                    },
                    joined: 0,
                    endpoint: String::new(),
                    relay: None,
                    voice: false,
                })
                .collect(),
            max_members: max,
            conversation: "c".into(),
            created: 0,
            revision: 1,
            discord: None,
        }
    }

    fn issue(online: &mut Online) {
        let id = online.sync_join_offer(true).expect("a question for a code");
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1, "one question for a code: {lines:?}");
        online.apply(Line::Output(Output::Answer(
            anyps5_core::online::control::Answer::ok(
                id,
                JoinCode {
                    code: "k3x9m2pqra".into(),
                    url: "https://anyplaynet.com/join/k3x9m2pqra".into(),
                    expires: unix_now() + 7200,
                },
            ),
        )));
    }

    #[test]
    fn without_a_network_it_is_todays_line() {
        let prefs = presence_on();
        let online = Online::default();
        let shown = wanted(Some(&played()), &prefs, &online, unix_now()).unwrap();
        assert_eq!(shown.game, "Hollow Orchard");
        assert!(shown.online.is_none());
    }

    #[test]
    fn presence_off_sends_nothing_at_all() {
        let prefs = Preferences {
            discord_presence: false,
            ..presence_on()
        };
        let mut online = Online::default();
        signed_in(&mut online);
        assert!(wanted(Some(&played()), &prefs, &online, unix_now()).is_none());
        assert!(wanted(None, &presence_on(), &online, unix_now()).is_none());
    }

    #[test]
    fn signed_in_without_a_session_or_party_is_still_the_plain_line() {
        let mut online = Online::default();
        signed_in(&mut online);
        let shown = wanted(Some(&played()), &presence_on(), &online, unix_now()).unwrap();
        assert!(shown.online.is_none());
    }

    #[test]
    fn a_party_shows_its_size_and_the_code_makes_the_button() {
        let prefs = presence_on();
        let mut online = Online::default();
        signed_in(&mut online);
        online.social.apply_party(Some(party(2, 4)));
        let before = wanted(Some(&played()), &prefs, &online, unix_now()).unwrap();
        let on = before.online.expect("online with a party");
        assert_eq!(on.party, Some((2, 4)));
        assert_eq!(on.join_url, None, "no code yet");
        issue(&mut online);
        let after = wanted(Some(&played()), &prefs, &online, unix_now())
            .and_then(|p| p.online)
            .unwrap();
        assert_eq!(
            after.join_url.as_deref(),
            Some("https://anyplaynet.com/join/k3x9m2pqra")
        );
    }

    #[test]
    fn the_button_setting_takes_the_link_away_but_keeps_the_text() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.social.apply_party(Some(party(2, 4)));
        issue(&mut online);
        let prefs = Preferences {
            discord_join_button: false,
            ..presence_on()
        };
        let shown = wanted(Some(&played()), &prefs, &online, unix_now())
            .and_then(|p| p.online)
            .unwrap();
        assert_eq!(shown.join_url, None);
        assert_eq!(shown.party, Some((2, 4)));
    }

    #[test]
    fn the_code_goes_back_when_the_game_ends_or_the_setting_goes_off() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.social.apply_party(Some(party(1, 4)));
        issue(&mut online);
        assert_eq!(
            online.sync_join_offer(true),
            None,
            "a held code is not asked for again"
        );
        let id = online.sync_join_offer(false).expect("given back");
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("join_code_revoke"), "{lines:?}");
        assert_eq!(online.sync_join_offer(false), None, "once");
        assert!(online.is_waiting(|p| *p == Purpose::JoinCodeRevoke));
        assert!(id > 0);
    }

    #[test]
    fn nothing_to_join_means_no_buttons_and_no_new_question() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.social.apply_party(Some(party(1, 4)));
        let id = online.sync_join_offer(true).expect("asked");
        assert_eq!(online.take_outbox().len(), 1);
        online.apply(Line::Output(Output::Answer(
            anyps5_core::online::control::Answer::error(
                id,
                anyps5_core::online::Failure::new("not_found", "nothing to join"),
            ),
        )));
        assert_eq!(online.sync_join_offer(true), None, "not asked again");
        let shown = wanted(Some(&played()), &presence_on(), &online, unix_now())
            .and_then(|p| p.online)
            .unwrap();
        assert_eq!(shown.join_url, None);
    }

    #[test]
    fn signing_out_forgets_the_code() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.social.apply_party(Some(party(1, 4)));
        issue(&mut online);
        online.apply(Line::Output(Output::Event(Event::State {
            cloud: Cloud::Online,
            account: None,
            pending: None,
        })));
        assert_eq!(online.join.url(unix_now()), None);
    }
}

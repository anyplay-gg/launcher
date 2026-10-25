//! Calls: a party of two that rings (network ADR 0026, `spec/cloud-api.md` "Calls").
//!
//! A call is an ordinary party with voice on. Calling a friend sends a party invitation with the
//! `call` flag; the friend's launcher rings while that invitation lives (30 seconds), accepting
//! joins the party, declining or letting it run out ends it. "Add to call" is one more call
//! invitation into the same party. A call nobody answered is kept by the cloud as a missed call
//! until the callee has seen it.
//!
//! This module holds what the launcher adds to the party and voice state: who the player is
//! calling, whether a party is a call, the missed calls, and the commands.

use super::api::{self, Account, AccountId, CallEnd, MissedCall, PartyInvitation};
use super::voice::VoiceProvider;
use super::{Command, Notice, Online, Purpose};

/// What the launcher knows about calls.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Calls {
    /// The calls nobody answered, newest first, until the player has seen them.
    pub missed: Vec<MissedCall>,
    /// The friend the player called and who has not answered yet.
    pub ringing: Option<Account>,
    /// The player's party is a call: they placed one that was answered, or answered one.
    pub active: bool,
}

impl Calls {
    /// The missed call from this account, if the player has not seen it.
    pub fn missed_from(&self, account: AccountId) -> Option<&MissedCall> {
        self.missed.iter().find(|m| m.from.id == account)
    }

    fn put_missed(&mut self, call: MissedCall) {
        self.missed.retain(|m| m.from.id != call.from.id);
        self.missed.insert(0, call);
    }

    fn forget_missed(&mut self, account: AccountId) {
        self.missed.retain(|m| m.from.id != account);
    }
}

/// Where a call stands, for the surface that shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallState<'a> {
    /// No call.
    Idle,
    /// The player called and waits for an answer.
    Calling(&'a Account),
    /// The party is a call with at least one other member.
    Connected,
}

impl Online {
    /// The call that rings for the player now: a call invitation that has not run out.
    pub fn incoming_call(&self, now: u64) -> Option<&PartyInvitation> {
        self.social
            .party_invitations
            .iter()
            .filter(|i| i.call && i.expires > now)
            .min_by_key(|i| i.expires)
    }

    /// Whether the player is on a call, calling, or neither.
    pub fn call_state(&self) -> CallState<'_> {
        if let Some(account) = &self.calls.ringing {
            return CallState::Calling(account);
        }
        let others = self
            .social
            .party
            .as_ref()
            .is_some_and(|p| p.members.len() > 1);
        if others && self.calls.active {
            CallState::Connected
        } else {
            CallState::Idle
        }
    }

    /// The other members of the call, for the indicator.
    pub fn call_members(&self) -> Vec<&Account> {
        let me = self.account.as_ref().map(|a| a.id);
        self.social
            .party
            .as_ref()
            .map(|p| {
                p.members
                    .iter()
                    .filter(|m| Some(m.account.id) != me)
                    .map(|m| &m.account)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Calls a friend: voice goes on, and the friend's launcher rings. In a party the player
    /// leads, this adds the friend to the call. Fails (returns false) when the player is in a
    /// party they do not lead.
    pub fn start_call(&mut self, friend: &Account) -> bool {
        if let (Some(party), Some(me)) = (&self.social.party, self.account.as_ref())
            && party.leader != me.id
        {
            return false;
        }
        self.use_voice_for_call();
        self.calls.ringing = Some(friend.clone());
        self.calls.forget_missed(friend.id);
        self.send(
            Purpose::Call(friend.id),
            Command::PartyInvite {
                accounts: vec![friend.id],
                call: true,
            },
        );
        self.send(
            Purpose::CallsSeen,
            Command::CallsSeen {
                account: Some(friend.id),
            },
        );
        true
    }

    /// Answers the ringing call: joins the party, with voice on.
    pub fn accept_call(&mut self, invitation: &str) {
        let from = self
            .social
            .party_invitations
            .iter()
            .find(|i| i.id == invitation)
            .map(|i| i.from.id);
        self.use_voice_for_call();
        self.calls.active = true;
        if let Some(from) = from {
            self.calls.forget_missed(from);
        }
        self.send(
            Purpose::PartyAnswer {
                id: invitation.to_owned(),
                accept: true,
            },
            Command::PartyAnswer {
                id: invitation.to_owned(),
                accept: true,
            },
        );
    }

    /// Turns the ringing call down.
    pub fn decline_call(&mut self, invitation: &str) {
        self.social.drop_party_invitation(invitation);
        self.send(
            Purpose::PartyAnswer {
                id: invitation.to_owned(),
                accept: false,
            },
            Command::PartyAnswer {
                id: invitation.to_owned(),
                accept: false,
            },
        );
    }

    /// Leaves the call (the party), or stops calling.
    pub fn hang_up(&mut self) {
        self.calls.ringing = None;
        self.calls.active = false;
        self.send(Purpose::PartyLeave, Command::PartyLeave);
    }

    /// Marks the missed calls from `account` as seen, here and in the cloud.
    pub fn calls_seen(&mut self, account: AccountId) {
        if self.calls.missed_from(account).is_some() {
            self.calls.forget_missed(account);
            self.send(
                Purpose::CallsSeen,
                Command::CallsSeen {
                    account: Some(account),
                },
            );
        }
    }

    /// Mutes or unmutes the player's own microphone.
    pub fn set_mic_mute(&mut self, muted: bool) {
        if self.voice.muted != muted {
            self.voice.muted = muted;
            self.send(Purpose::Voice, Command::MicMute { muted });
        }
    }

    /// Stops or resumes hearing everyone. The service silences the microphone while deafened.
    pub fn set_deafen(&mut self, deafened: bool) {
        if self.voice.deafened != deafened {
            self.voice.deafened = deafened;
            self.send(Purpose::Voice, Command::Deafen { deafened });
        }
    }

    /// The leader names (or, with the empty string, removes) the party's Discord link.
    pub fn set_party_link(&mut self, url: &str) {
        self.send(
            Purpose::PartyLink,
            Command::PartyLink {
                url: url.trim().to_owned(),
            },
        );
    }

    /// Voice goes on for a call, unless the player's voice is on Discord.
    fn use_voice_for_call(&mut self) {
        if self.voice.provider() == VoiceProvider::AnyPlay && !self.voice.enabled() {
            self.set_voice(true);
        }
    }

    /// An answer to `Purpose::Call`: nobody rings when the cloud sent no invitation.
    pub(super) fn call_placed(
        &mut self,
        friend: AccountId,
        value: &serde_json::Value,
        notices: &mut Vec<Notice>,
    ) {
        let sent = serde_json::from_value::<api::Invited>(value.clone())
            .map(|i| i.sent.contains(&friend))
            .unwrap_or(true);
        if !sent && let Some(account) = self.calls.ringing.take_if(|a| a.id == friend) {
            notices.push(Notice::CallUnreachable(account));
        }
    }

    /// A call the player placed failed outright.
    pub(super) fn call_failed(&mut self, friend: AccountId) {
        self.calls.ringing.take_if(|a| a.id == friend);
    }

    pub(super) fn missed_calls(&mut self, list: Vec<MissedCall>) {
        self.calls.missed = list;
    }

    pub(super) fn missed_call(&mut self, call: MissedCall) -> Notice {
        self.calls.put_missed(call.clone());
        Notice::MissedCall(call)
    }

    pub(super) fn missed_call_gone(&mut self, account: AccountId) {
        self.calls.forget_missed(account);
    }

    /// A call the player placed ended unanswered. The player's party of one is dropped.
    pub(super) fn call_ended(&mut self, account: AccountId, reason: CallEnd) -> Option<Notice> {
        let callee = self.calls.ringing.take_if(|a| a.id == account)?;
        self.calls.active = false;
        let alone = self
            .social
            .party
            .as_ref()
            .is_some_and(|p| p.members.len() <= 1);
        if alone {
            self.send(Purpose::PartyLeave, Command::PartyLeave);
        }
        Some(Notice::CallEnded(callee, reason))
    }

    /// The party changed: a callee who joined ends the ringing, a party that ended ends the call.
    pub(super) fn call_party_changed(&mut self) {
        match &self.social.party {
            None => {
                self.calls.ringing = None;
                self.calls.active = false;
            }
            Some(party) => {
                if let Some(callee) = &self.calls.ringing
                    && party.members.iter().any(|m| m.account.id == callee.id)
                {
                    self.calls.ringing = None;
                    self.calls.active = true;
                }
                if party.members.len() <= 1 && self.calls.ringing.is_none() {
                    self.calls.active = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{Event, Line, Output, SignedIn, control};

    fn event(event: Event) -> Line {
        Line::Output(Output::Event(event))
    }

    fn account(id: u64, handle: &str) -> Account {
        Account {
            id: AccountId(id),
            handle: handle.into(),
        }
    }

    fn me() -> SignedIn {
        SignedIn {
            id: AccountId(1),
            handle: "Alec".into(),
            email: "a@example.org".into(),
        }
    }

    fn signed_in() -> Online {
        let mut online = Online::default();
        online.apply(event(Event::State {
            cloud: control::Cloud::Online,
            account: Some(me()),
            pending: None,
        }));
        online.take_outbox();
        online
    }

    fn party(members: &[(u64, &str)], leader: u64) -> api::Party {
        api::Party {
            id: "p".into(),
            leader: AccountId(leader),
            members: members
                .iter()
                .map(|(id, handle)| api::PartyMember {
                    account: account(*id, handle),
                    joined: 0,
                    endpoint: String::new(),
                    relay: None,
                    voice: true,
                })
                .collect(),
            max_members: 8,
            conversation: "c".into(),
            created: 0,
            revision: 1,
            discord: None,
        }
    }

    fn ring(online: &mut Online, id: &str, expires: u64) {
        online.apply(event(Event::PartyInvitation {
            invitation: PartyInvitation {
                id: id.into(),
                from: account(2, "Lumen"),
                party: "p".into(),
                members: 1,
                expires,
                call: true,
            },
        }));
    }

    #[test]
    fn a_call_invitation_rings_until_it_runs_out() {
        let mut online = signed_in();
        ring(&mut online, "i", 100);
        assert_eq!(online.incoming_call(99).map(|i| i.id.as_str()), Some("i"));
        assert!(online.incoming_call(100).is_none());
        online.apply(event(Event::PartyInvitationGone { id: "i".into() }));
        assert!(online.incoming_call(1).is_none());
    }

    #[test]
    fn an_ordinary_party_invitation_does_not_ring() {
        let mut online = signed_in();
        online.apply(event(Event::PartyInvitation {
            invitation: PartyInvitation {
                id: "i".into(),
                from: account(2, "Lumen"),
                party: "p".into(),
                members: 2,
                expires: 100,
                call: false,
            },
        }));
        assert!(online.incoming_call(1).is_none());
    }

    #[test]
    fn calling_turns_voice_on_and_rings_the_friend() {
        let mut online = signed_in();
        assert!(online.start_call(&account(2, "Lumen")));
        let sent = online.take_outbox();
        assert!(sent.iter().any(|l| l.contains("\"do\":\"voice\"")));
        assert!(
            sent.iter()
                .any(|l| l.contains("\"do\":\"party_invite\"") && l.contains("\"call\":true"))
        );
        assert!(matches!(online.call_state(), CallState::Calling(a) if a.handle == "Lumen"));
        assert!(online.voice.enabled());
    }

    #[test]
    fn on_discord_a_call_leaves_the_microphone_alone() {
        let mut online = signed_in();
        online.set_voice_provider(VoiceProvider::Discord);
        online.take_outbox();
        online.start_call(&account(2, "Lumen"));
        let sent = online.take_outbox();
        assert!(!sent.iter().any(|l| l.contains("\"do\":\"voice\"")));
        assert!(!online.voice.enabled());
    }

    #[test]
    fn the_callee_joining_makes_the_party_a_call() {
        let mut online = signed_in();
        online.start_call(&account(2, "Lumen"));
        online.apply(event(Event::Party {
            party: Some(party(&[(1, "Alec")], 1)),
        }));
        assert!(matches!(online.call_state(), CallState::Calling(_)));
        online.apply(event(Event::Party {
            party: Some({
                let mut p = party(&[(1, "Alec"), (2, "Lumen")], 1);
                p.revision = 2;
                p
            }),
        }));
        assert_eq!(online.call_state(), CallState::Connected);
        assert_eq!(online.call_members().len(), 1);
        online.apply(event(Event::Party { party: None }));
        assert_eq!(online.call_state(), CallState::Idle);
    }

    #[test]
    fn a_call_nobody_answered_drops_the_empty_party() {
        let mut online = signed_in();
        online.start_call(&account(2, "Lumen"));
        online.apply(event(Event::Party {
            party: Some(party(&[(1, "Alec")], 1)),
        }));
        online.take_outbox();
        let notices = online.apply(event(Event::CallEnded {
            account: AccountId(2),
            reason: CallEnd::NoAnswer,
        }));
        assert!(
            matches!(&notices[..], [Notice::CallEnded(a, CallEnd::NoAnswer)] if a.id == AccountId(2))
        );
        assert_eq!(online.call_state(), CallState::Idle);
        assert!(
            online
                .take_outbox()
                .iter()
                .any(|l| l.contains("\"do\":\"party_leave\""))
        );
    }

    #[test]
    fn an_empty_answer_means_nobody_rings() {
        let mut online = signed_in();
        online.start_call(&account(2, "Lumen"));
        let line = online.take_outbox().remove(1);
        let id: u64 = serde_json::from_str::<serde_json::Value>(&line).unwrap()["id"]
            .as_u64()
            .unwrap();
        let notices = online.apply(Line::Output(Output::Answer(control::Answer::ok(
            id,
            api::Invited { sent: vec![] },
        ))));
        assert!(
            notices
                .iter()
                .any(|n| matches!(n, Notice::CallUnreachable(_)))
        );
        assert_eq!(online.call_state(), CallState::Idle);
    }

    #[test]
    fn missed_calls_are_kept_until_seen() {
        let mut online = signed_in();
        let call = |at| MissedCall {
            from: account(2, "Lumen"),
            at,
        };
        online.apply(event(Event::MissedCalls(api::MissedCalls {
            missed: vec![call(5)],
        })));
        let notices = online.apply(event(Event::MissedCall { call: call(9) }));
        assert!(matches!(&notices[..], [Notice::MissedCall(c)] if c.at == 9));
        assert_eq!(online.calls.missed.len(), 1, "one per caller");
        assert_eq!(online.calls.missed_from(AccountId(2)).unwrap().at, 9);
        online.calls_seen(AccountId(2));
        assert!(online.calls.missed.is_empty());
        assert!(
            online
                .take_outbox()
                .iter()
                .any(|l| l.contains("\"do\":\"calls_seen\"") && l.contains("\"account\":2"))
        );
        // Seeing nothing sends nothing.
        online.calls_seen(AccountId(2));
        assert!(online.take_outbox().is_empty());
    }

    #[test]
    fn only_the_leader_adds_to_a_call() {
        let mut online = signed_in();
        online.apply(event(Event::Party {
            party: Some(party(&[(2, "Lumen"), (1, "Alec")], 2)),
        }));
        assert!(!online.start_call(&account(3, "Cy")));
        assert!(online.take_outbox().is_empty());
    }

    #[test]
    fn mute_and_deafen_go_out_once_per_change() {
        let mut online = signed_in();
        online.set_mic_mute(true);
        online.set_mic_mute(true);
        online.set_deafen(true);
        let sent = online.take_outbox();
        assert_eq!(sent.len(), 2);
        assert!(sent[0].contains("\"do\":\"mic_mute\""));
        assert!(sent[1].contains("\"do\":\"deafen\""));
    }
}

//! Share screen as the launcher knows it (network `spec/control-protocol.md`, "Share screen").
//! A player shows the game they play to their party and the members choose to watch: nothing
//! comes back, no input crosses. The share worker, a process the net service starts, captures,
//! encodes, decodes and draws; the launcher neither decodes nor draws video. It chooses (start,
//! stop, watch, leave) and says in words what the `screen` event reports.
//!
//! - [`Screen`] is what the service reported: whether sharing and watching are possible at all,
//!   the stream the player shares, the streams offered to them and the one they watch.
//! - [`ScreenCommand`] is what the player can ask for; [`Online::screen_send`] sends it.
//! - [`ScreenNotice`] is what the player should hear of when the party's streams change under
//!   them: an offer, a stream that ended.
//!
//! Nothing here shows while `can_share` and `can_watch` are both false and there is nothing to
//! show ([`Screen::visible`]); a build without capture never mentions the feature.

use anyplay_control::{Command, ScreenHosting, ScreenOffer, ScreenSource, ScreenWatching};

use super::api::AccountId;
use super::{Notice, Online, Purpose};

/// What the player asks of their stream or the one they watch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenCommand {
    /// Starts sharing the game that runs.
    Start,
    /// Ends the player's stream.
    Stop,
    /// Watches the stream of this member.
    Watch(AccountId),
    /// Stops watching.
    Leave,
}

/// Something about the party's streams the player should hear of, whatever they are looking at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenNotice {
    /// A member started sharing.
    Offered { account: AccountId, title: String },
    /// The stream the player watched is gone, and they did not leave it themselves.
    WatchEnded { account: AccountId, title: String },
    /// The player's own stream ended without their stopping it (the game closed, the party
    /// ended).
    ShareEnded,
}

/// What the net service reported about share screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    can_share: bool,
    can_watch: bool,
    hosting: Option<ScreenHosting>,
    offers: Vec<ScreenOffer>,
    watching: Option<ScreenWatching>,
    /// The player asked to stop; the end of the stream is no news to them.
    stopping: bool,
    /// The player asked to leave; the end of the stream is no news to them.
    leaving: bool,
}

impl Screen {
    /// Forgets everything: signed out, or the service ended.
    pub fn reset(&mut self) {
        *self = Screen::default();
    }

    /// True when anything about share screen belongs on the screen: a button may show, or there
    /// is a stream to show or to offer. False in a build without capture and decoding, for a
    /// player who has no party, and while nothing is shared.
    pub fn visible(&self) -> bool {
        self.can_share
            || self.can_watch
            || self.hosting.is_some()
            || self.watching.is_some()
            || !self.offers.is_empty()
    }

    /// The Share Screen button may show: a game runs in a party and the worker can capture, and
    /// the player is not sharing yet.
    pub fn can_start(&self) -> bool {
        self.can_share && self.hosting.is_none()
    }

    /// The worker can decode and show a stream.
    pub fn can_watch(&self) -> bool {
        self.can_watch
    }

    /// The stream the player shares.
    pub fn hosting(&self) -> Option<&ScreenHosting> {
        self.hosting.as_ref()
    }

    /// How many members watch the player's stream.
    pub fn watchers(&self) -> usize {
        self.hosting
            .as_ref()
            .map_or(0, |h| h.viewers.iter().filter(|v| v.watching).count())
    }

    /// The streams members offer, the one the player watches left out.
    pub fn offers(&self) -> impl Iterator<Item = &ScreenOffer> {
        let watched = self.watching.as_ref().map(|w| w.account);
        self.offers
            .iter()
            .filter(move |offer| Some(offer.account) != watched)
    }

    /// The stream the player watches.
    pub fn watching(&self) -> Option<&ScreenWatching> {
        self.watching.as_ref()
    }

    /// Takes over one `screen` event and tells what changed that the player should hear of.
    pub fn changed(
        &mut self,
        can_share: bool,
        can_watch: bool,
        hosting: Option<ScreenHosting>,
        offers: Vec<ScreenOffer>,
        watching: Option<ScreenWatching>,
    ) -> Vec<ScreenNotice> {
        let mut notices = Vec::new();
        for offer in &offers {
            if !self.offers.iter().any(|o| o.account == offer.account) {
                notices.push(ScreenNotice::Offered {
                    account: offer.account,
                    title: offer.title.clone(),
                });
            }
        }
        if let (Some(was), None) = (&self.watching, &watching)
            && !self.leaving
        {
            notices.push(ScreenNotice::WatchEnded {
                account: was.account,
                title: was.title.clone(),
            });
        }
        if self.hosting.is_some() && hosting.is_none() && !self.stopping {
            notices.push(ScreenNotice::ShareEnded);
        }
        if watching.is_none() {
            self.leaving = false;
        }
        if hosting.is_none() {
            self.stopping = false;
        }
        self.can_share = can_share;
        self.can_watch = can_watch;
        self.hosting = hosting;
        self.offers = offers;
        self.watching = watching;
        notices
    }

    /// A stop or a leave failed: the stream did not end by the player's choice after all.
    pub fn forget_expectations(&mut self) {
        self.stopping = false;
        self.leaving = false;
    }
}

impl Online {
    /// Sends what the player asked of share screen.
    pub fn screen_send(&mut self, command: ScreenCommand) {
        match command {
            ScreenCommand::Start => {
                self.send(
                    Purpose::ScreenStart,
                    Command::ScreenStart {
                        source: ScreenSource::Game,
                    },
                );
            }
            ScreenCommand::Stop => {
                self.screen.stopping = self.screen.hosting.is_some();
                self.send(Purpose::ScreenStop, Command::ScreenStop);
            }
            ScreenCommand::Watch(account) => {
                self.send(
                    Purpose::ScreenWatch(account),
                    Command::ScreenWatch { account },
                );
            }
            ScreenCommand::Leave => {
                self.screen.leaving = self.screen.watching.is_some();
                self.send(Purpose::ScreenLeave, Command::ScreenLeave);
            }
        }
    }

    pub(super) fn screen_event(
        &mut self,
        can_share: bool,
        can_watch: bool,
        hosting: Option<ScreenHosting>,
        offers: Vec<ScreenOffer>,
        watching: Option<ScreenWatching>,
    ) -> Vec<Notice> {
        self.screen
            .changed(can_share, can_watch, hosting, offers, watching)
            .into_iter()
            .map(Notice::Screen)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use anyplay_control::{Link, ScreenPhase, ScreenStats, ScreenViewer};

    use super::*;
    use crate::online::{Event, Line, Output};

    fn offer(id: u64, title: &str) -> ScreenOffer {
        ScreenOffer {
            account: AccountId(id),
            title: title.into(),
            link: None,
        }
    }

    fn hosting(viewers: Vec<ScreenViewer>) -> ScreenHosting {
        ScreenHosting {
            phase: ScreenPhase::Live,
            title: "Astro Bot".into(),
            codec: None,
            stats: ScreenStats::default(),
            viewers,
        }
    }

    fn viewer(id: u64, watching: bool) -> ScreenViewer {
        ScreenViewer {
            account: AccountId(id),
            watching,
            link: Some(Link::Direct),
            rtt_ms: Some(24),
            loss_percent: 0,
        }
    }

    fn watching(id: u64, phase: ScreenPhase) -> ScreenWatching {
        ScreenWatching {
            account: AccountId(id),
            title: "Astro Bot".into(),
            phase,
            link: Some(Link::Relay),
            rtt_ms: Some(80),
            stats: ScreenStats::default(),
            shown_fps: 0,
        }
    }

    fn event(
        can_share: bool,
        can_watch: bool,
        hosting: Option<ScreenHosting>,
        offers: Vec<ScreenOffer>,
        watching: Option<ScreenWatching>,
    ) -> Line {
        Line::Output(Output::Event(Event::Screen {
            can_share,
            can_watch,
            hosting,
            offers,
            watching,
        }))
    }

    #[test]
    fn nothing_shows_while_nothing_is_possible_and_nothing_runs() {
        let mut online = Online::default();
        assert!(!online.screen.visible());
        online.apply(event(false, false, None, vec![], None));
        assert!(!online.screen.visible());
        assert!(!online.screen.can_start());
    }

    #[test]
    fn a_stream_shows_even_where_sharing_is_not_possible() {
        let mut online = Online::default();
        online.apply(event(false, false, None, vec![offer(2, "Astro Bot")], None));
        assert!(online.screen.visible());
        assert!(!online.screen.can_watch());
        online.apply(event(false, false, None, vec![], None));
        assert!(!online.screen.visible());
        online.apply(event(true, false, None, vec![], None));
        assert!(online.screen.visible() && online.screen.can_start());
    }

    #[test]
    fn sharing_ends_the_start_button_and_counts_those_who_watch() {
        let mut online = Online::default();
        online.apply(event(
            true,
            true,
            Some(hosting(vec![viewer(2, true), viewer(3, false)])),
            vec![],
            None,
        ));
        assert!(!online.screen.can_start());
        assert_eq!(online.screen.watchers(), 1);
        assert_eq!(online.screen.hosting().map(|h| h.viewers.len()), Some(2));
    }

    #[test]
    fn a_new_offer_is_news_once() {
        let mut online = Online::default();
        let notices = online.apply(event(false, true, None, vec![offer(2, "Astro Bot")], None));
        assert_eq!(
            notices,
            vec![Notice::Screen(ScreenNotice::Offered {
                account: AccountId(2),
                title: "Astro Bot".into()
            })]
        );
        let again = online.apply(event(false, true, None, vec![offer(2, "Astro Bot")], None));
        assert!(again.is_empty());
    }

    #[test]
    fn the_stream_watched_is_not_offered_again() {
        let mut online = Online::default();
        online.apply(event(
            false,
            true,
            None,
            vec![offer(2, "Astro Bot"), offer(3, "Gran Turismo")],
            Some(watching(2, ScreenPhase::Live)),
        ));
        let rest: Vec<_> = online.screen.offers().map(|o| o.account).collect();
        assert_eq!(rest, vec![AccountId(3)]);
        assert_eq!(
            online.screen.watching().map(|w| w.phase),
            Some(ScreenPhase::Live)
        );
    }

    #[test]
    fn a_stream_that_ends_under_the_viewer_is_told_but_leaving_is_not() {
        let mut online = Online::default();
        online.apply(event(
            false,
            true,
            None,
            vec![offer(2, "Astro Bot")],
            Some(watching(2, ScreenPhase::Live)),
        ));
        let ended = online.apply(event(false, true, None, vec![], None));
        assert_eq!(
            ended,
            vec![Notice::Screen(ScreenNotice::WatchEnded {
                account: AccountId(2),
                title: "Astro Bot".into()
            })]
        );

        online.apply(event(
            false,
            true,
            None,
            vec![offer(2, "Astro Bot")],
            Some(watching(2, ScreenPhase::Connecting)),
        ));
        online.screen_send(ScreenCommand::Leave);
        let left = online.apply(event(false, true, None, vec![offer(2, "Astro Bot")], None));
        assert!(left.is_empty());
        // The next end is news again.
        online.apply(event(
            false,
            true,
            None,
            vec![],
            Some(watching(2, ScreenPhase::Live)),
        ));
        assert_eq!(
            online.apply(event(false, true, None, vec![], None)).len(),
            1
        );
    }

    #[test]
    fn the_player_hears_when_their_own_stream_ends_by_itself_only() {
        let mut online = Online::default();
        online.apply(event(true, true, Some(hosting(vec![])), vec![], None));
        let ended = online.apply(event(false, true, None, vec![], None));
        assert_eq!(ended, vec![Notice::Screen(ScreenNotice::ShareEnded)]);

        online.apply(event(true, true, Some(hosting(vec![])), vec![], None));
        online.screen_send(ScreenCommand::Stop);
        assert!(
            online
                .apply(event(true, true, None, vec![], None))
                .is_empty()
        );
    }

    #[test]
    fn a_refused_stop_does_not_hide_the_next_end() {
        let mut online = Online::default();
        online.apply(event(true, true, Some(hosting(vec![])), vec![], None));
        online.screen_send(ScreenCommand::Stop);
        online.screen.forget_expectations();
        let ended = online.apply(event(true, true, None, vec![], None));
        assert_eq!(ended.len(), 1);
    }

    #[test]
    fn the_commands_go_out_as_the_protocol_writes_them() {
        let mut online = Online::default();
        online.screen_send(ScreenCommand::Start);
        online.screen_send(ScreenCommand::Stop);
        online.screen_send(ScreenCommand::Watch(AccountId(7)));
        online.screen_send(ScreenCommand::Leave);
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("screen_start") && lines[0].contains("game"));
        assert!(lines[1].contains("screen_stop"));
        assert!(lines[2].contains("screen_watch") && lines[2].contains('7'));
        assert!(lines[3].contains("screen_leave"));
        assert!(online.is_waiting(|p| *p == Purpose::ScreenWatch(AccountId(7))));
    }

    #[test]
    fn signing_out_forgets_the_streams() {
        let mut online = Online::default();
        online.apply(event(
            true,
            true,
            Some(hosting(vec![viewer(2, true)])),
            vec![offer(3, "Astro Bot")],
            None,
        ));
        online.screen.reset();
        assert!(!online.screen.visible());
    }
}

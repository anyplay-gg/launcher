//! The one virtual pad of LudusCtrl and who holds it. The UDP receiver (the app) and a browser
//! client both feed the pad through this slot and never hold the pad themselves, so this PC never
//! shows two controllers for LudusCtrl.
//!
//! The rules: one holder at a time; a holder silent for the watchdog loses the pad, which goes to
//! neutral; the pad is unplugged after a while without a holder. Between phones of the app, a new
//! session takes over at once and a continued one only after the holder has been silent for
//! 0.3 s. A browser cannot claim the pad while the app is linked, and while a browser holds it the
//! app's packets are not applied.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use super::{PadTarget, wire::Id, wire::PadLayout};
use crate::remote::vpad::PadState;

/// A holder silent this long has left: the pad goes to neutral and the slot is free.
pub const WATCHDOG: Duration = Duration::from_millis(1500);
/// Another phone takes over the pad once the holder has been silent this long.
pub const PEER_SWITCH: Duration = Duration::from_millis(300);
/// With nobody holding the pad this long, the virtual controller is unplugged.
pub const UNPLUG_AFTER: Duration = Duration::from_secs(10);

/// Who feeds the pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// A phone of the LudusCtrl app over UDP, by its phone id.
    App(Id),
    /// A browser client, by its connection id.
    Browser(u64),
}

/// Why a browser may not take the pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The app is linked over UDP (`busy:app`).
    BusyApp,
    /// Another browser holds it and sent within the watchdog (`busy`).
    Busy,
}

/// The holder as the status shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holding {
    pub owner: Owner,
    /// Where its last packet came from (it follows Wi-Fi roaming).
    pub source: SocketAddr,
    pub layout: PadLayout,
    /// A controller is connected to the phone or the browser sees one; without it the pad is
    /// held at neutral.
    pub controller: bool,
    /// A pad target exists and its last write worked.
    pub pad_live: bool,
}

struct Holder {
    owner: Owner,
    source: SocketAddr,
    last: Instant,
    layout: PadLayout,
    controller: bool,
}

#[derive(Default)]
struct Inner {
    pad: Option<Arc<dyn PadTarget>>,
    holder: Option<Holder>,
    /// The last write to the pad worked.
    pad_ok: bool,
    /// Something was written since the last unplug.
    plugged: bool,
    free_since: Option<Instant>,
}

impl Inner {
    /// Writes to the pad; a failed write releases it until one works again.
    fn write(&mut self, state: &PadState) -> bool {
        let Some(pad) = self.pad.clone() else {
            self.pad_ok = false;
            return false;
        };
        let ok = pad.write(state);
        if !ok && self.pad_ok {
            pad.release();
        }
        self.pad_ok = ok;
        self.plugged |= ok;
        ok
    }

    fn feed(
        &mut self,
        state: &PadState,
        layout: PadLayout,
        controller: bool,
        now: Instant,
    ) -> bool {
        if let Some(holder) = &mut self.holder {
            holder.last = now;
            holder.layout = layout;
            holder.controller = controller;
        }
        self.free_since = None;
        self.write(if controller { state } else { &NEUTRAL })
    }

    fn let_go(&mut self, now: Instant) {
        self.holder = None;
        self.free_since = Some(now);
        if self.plugged {
            self.write(&NEUTRAL);
        }
    }

    fn unplug(&mut self) {
        if let Some(pad) = &self.pad {
            pad.release();
        }
        self.plugged = false;
        self.pad_ok = false;
    }
}

const NEUTRAL: PadState = PadState {
    buttons: 0,
    left_trigger: 0,
    right_trigger: 0,
    lx: 0,
    ly: 0,
    rx: 0,
    ry: 0,
};

#[derive(Default)]
pub struct PadSlot {
    inner: Mutex<Inner>,
}

impl PadSlot {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().expect("ludusctrl pad slot")
    }

    /// The pad the slot writes to (`None`: nowhere, never live). The old one is released.
    pub fn set_target(&self, pad: Option<Arc<dyn PadTarget>>) {
        let mut inner = self.lock();
        if let Some(old) = std::mem::replace(&mut inner.pad, pad) {
            old.release();
        }
        inner.plugged = false;
        inner.pad_ok = false;
    }

    pub fn has_target(&self) -> bool {
        self.lock().pad.is_some()
    }

    pub fn holding(&self) -> Option<Holding> {
        let inner = self.lock();
        inner.holder.as_ref().map(|h| Holding {
            owner: h.owner,
            source: h.source,
            layout: h.layout,
            controller: h.controller,
            pad_live: inner.pad.is_some() && inner.pad_ok,
        })
    }

    /// An authenticated, accepted packet of phone `phone`. `Some(pad_live)` when it was applied;
    /// `None` when another phone or a browser holds the pad.
    #[allow(clippy::too_many_arguments)]
    pub fn app_input(
        &self,
        phone: Id,
        source: SocketAddr,
        new_session: bool,
        state: &PadState,
        layout: PadLayout,
        controller: bool,
        now: Instant,
    ) -> Option<bool> {
        let mut inner = self.lock();
        let take = match &mut inner.holder {
            Some(holder) if holder.owner == Owner::App(phone) => {
                holder.source = source;
                false
            }
            Some(Holder {
                owner: Owner::Browser(_),
                ..
            }) => return None,
            Some(holder)
                if !new_session && now.saturating_duration_since(holder.last) < PEER_SWITCH =>
            {
                return None;
            }
            _ => true,
        };
        if take {
            // A new session (the person pressed Start link) or a silent holder: take over.
            inner.holder = Some(Holder {
                owner: Owner::App(phone),
                source,
                last: now,
                layout,
                controller,
            });
        }
        Some(inner.feed(state, layout, controller, now))
    }

    /// A browser asks for the pad. A stale browser holder is taken over.
    pub fn claim_browser(
        &self,
        connection: u64,
        peer: SocketAddr,
        now: Instant,
    ) -> Result<(), Refusal> {
        let mut inner = self.lock();
        match &inner.holder {
            Some(Holder {
                owner: Owner::App(_),
                ..
            }) => return Err(Refusal::BusyApp),
            Some(holder)
                if holder.owner != Owner::Browser(connection)
                    && now.saturating_duration_since(holder.last) < WATCHDOG =>
            {
                return Err(Refusal::Busy);
            }
            _ => {}
        }
        inner.holder = Some(Holder {
            owner: Owner::Browser(connection),
            source: peer,
            last: now,
            layout: PadLayout::Unknown,
            controller: false,
        });
        inner.free_since = None;
        Ok(())
    }

    /// A browser's packet. `Some(pad_live)` when this connection holds the pad, `None` when it
    /// no longer does (the watchdog dropped it: `evicted`).
    pub fn browser_input(
        &self,
        connection: u64,
        state: &PadState,
        layout: PadLayout,
        controller: bool,
        now: Instant,
    ) -> Option<bool> {
        let mut inner = self.lock();
        if inner.holder.as_ref().map(|h| h.owner) != Some(Owner::Browser(connection)) {
            return None;
        }
        Some(inner.feed(state, layout, controller, now))
    }

    /// Lets go of the pad if `owner` holds it (a closed connection, a removed phone). True if
    /// it did.
    pub fn release(&self, owner: Owner, now: Instant) -> bool {
        let mut inner = self.lock();
        if inner.holder.as_ref().map(|h| h.owner) != Some(owner) {
            return false;
        }
        inner.let_go(now);
        true
    }

    /// At least every 0.25 s. True when the holder was dropped.
    pub fn tick(&self, now: Instant) -> bool {
        let mut inner = self.lock();
        let silent = inner
            .holder
            .as_ref()
            .is_some_and(|h| now.saturating_duration_since(h.last) >= WATCHDOG);
        if silent {
            inner.let_go(now);
        }
        if inner.holder.is_none()
            && inner.plugged
            && inner
                .free_since
                .is_none_or(|at| now.saturating_duration_since(at) >= UNPLUG_AFTER)
        {
            inner.unplug();
        }
        silent
    }

    /// The server stops: nobody holds the pad and it is unplugged.
    pub fn stop(&self) {
        let mut inner = self.lock();
        inner.holder = None;
        if inner.plugged {
            inner.unplug();
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    #[derive(Default)]
    pub struct Pad {
        pub writes: Mutex<Vec<PadState>>,
        pub releases: Mutex<u32>,
        pub fail: Mutex<bool>,
    }

    impl PadTarget for Pad {
        fn write(&self, state: &PadState) -> bool {
            self.writes.lock().unwrap().push(*state);
            !*self.fail.lock().unwrap()
        }
        fn release(&self) {
            *self.releases.lock().unwrap() += 1;
        }
    }

    impl Pad {
        pub fn last(&self) -> Option<PadState> {
            self.writes.lock().unwrap().last().copied()
        }
        pub fn count(&self) -> usize {
            self.writes.lock().unwrap().len()
        }
        pub fn released(&self) -> u32 {
            *self.releases.lock().unwrap()
        }
    }

    pub fn slot() -> (PadSlot, Arc<Pad>) {
        let slot = PadSlot::default();
        let pad = Arc::new(Pad::default());
        slot.set_target(Some(pad.clone()));
        (slot, pad)
    }

    fn at(port: u16) -> SocketAddr {
        SocketAddr::from(([192, 168, 1, 20], port))
    }

    fn pressed(buttons: u16) -> PadState {
        PadState { buttons, ..NEUTRAL }
    }

    const A: Id = [1; 8];
    const B: Id = [2; 8];

    fn app(slot: &PadSlot, phone: Id, new: bool, buttons: u16, now: Instant) -> Option<bool> {
        slot.app_input(
            phone,
            at(1),
            new,
            &pressed(buttons),
            PadLayout::Xbox,
            true,
            now,
        )
    }

    #[test]
    fn phones_take_turns_by_the_rules() {
        let (slot, pad) = slot();
        let t = Instant::now();
        assert_eq!(app(&slot, A, true, 1, t), Some(true));
        assert_eq!(
            app(&slot, B, true, 2, t),
            Some(true),
            "a new session takes over"
        );
        let soon = t + Duration::from_millis(299);
        assert_eq!(app(&slot, A, false, 1, soon), None, "B was active");
        assert_eq!(pad.last().unwrap().buttons, 2);
        let later = t + PEER_SWITCH;
        assert_eq!(
            app(&slot, A, false, 1, later),
            Some(true),
            "B was silent for 0.3 s"
        );
        assert_eq!(slot.holding().unwrap().owner, Owner::App(A));
    }

    #[test]
    fn the_watchdog_lets_go_and_the_pad_is_unplugged_later() {
        let (slot, pad) = slot();
        let t = Instant::now();
        app(&slot, A, true, 0x1000, t);
        assert!(!slot.tick(t + Duration::from_millis(1499)));
        assert!(slot.tick(t + WATCHDOG));
        assert_eq!(pad.last(), Some(NEUTRAL));
        assert!(slot.holding().is_none());
        assert_eq!(pad.released(), 0);
        slot.tick(t + WATCHDOG + UNPLUG_AFTER);
        slot.tick(t + WATCHDOG + UNPLUG_AFTER);
        assert_eq!(pad.released(), 1, "once");
    }

    #[test]
    fn no_controller_holds_the_pad_at_neutral() {
        let (slot, pad) = slot();
        let t = Instant::now();
        slot.app_input(A, at(1), true, &pressed(1), PadLayout::Unknown, false, t);
        assert_eq!(pad.last(), Some(NEUTRAL));
        assert!(!slot.holding().unwrap().controller);
    }

    #[test]
    fn a_failing_pad_is_released_once_and_not_live() {
        let (slot, pad) = slot();
        let t = Instant::now();
        assert_eq!(app(&slot, A, true, 0, t), Some(true));
        *pad.fail.lock().unwrap() = true;
        assert_eq!(app(&slot, A, false, 1, t), Some(false));
        assert_eq!(app(&slot, A, false, 2, t), Some(false));
        assert_eq!(pad.released(), 1);
        assert!(!slot.holding().unwrap().pad_live);
        *pad.fail.lock().unwrap() = false;
        assert_eq!(app(&slot, A, false, 3, t), Some(true));
        slot.set_target(None);
        assert_eq!(
            app(&slot, A, false, 4, t),
            Some(false),
            "no pad, never live"
        );
    }

    #[test]
    fn a_browser_waits_for_the_app_and_the_app_for_a_browser() {
        let (slot, pad) = slot();
        let t = Instant::now();
        app(&slot, A, true, 1, t);
        assert_eq!(slot.claim_browser(7, at(9), t), Err(Refusal::BusyApp));
        slot.tick(t + WATCHDOG);
        let t = t + WATCHDOG;
        assert_eq!(slot.claim_browser(7, at(9), t), Ok(()));
        assert_eq!(app(&slot, A, true, 1, t), None, "a browser holds the pad");
        assert_eq!(
            slot.browser_input(7, &pressed(8), PadLayout::PlayStation, true, t),
            Some(true)
        );
        assert_eq!(pad.last().unwrap().buttons, 8);
        assert_eq!(slot.claim_browser(8, at(10), t), Err(Refusal::Busy));
        assert_eq!(
            slot.browser_input(8, &pressed(9), PadLayout::Unknown, true, t),
            None
        );
        // A stale browser is taken over by the next one.
        let stale = t + WATCHDOG;
        assert_eq!(slot.claim_browser(8, at(10), stale), Ok(()));
        assert_eq!(
            slot.browser_input(7, &pressed(9), PadLayout::Unknown, true, stale),
            None
        );
        assert!(slot.release(Owner::Browser(8), stale));
        assert!(!slot.release(Owner::Browser(8), stale));
        assert_eq!(pad.last(), Some(NEUTRAL));
    }

    #[test]
    fn stop_unplugs() {
        let (slot, pad) = slot();
        app(&slot, A, true, 1, Instant::now());
        slot.stop();
        assert_eq!(pad.released(), 1);
        assert!(slot.holding().is_none());
    }
}

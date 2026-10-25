//! The phone's pad as an operating-system controller: ViGEmBus on Windows, uinput on Linux.
//!
//! [`VirtualPad`] is a [`PadSink`]: hand it to `RemoteServer::set_pad_sink` while a game runs and
//! the phone's buttons, sticks and triggers appear as an Xbox-360-style gamepad that any game
//! (and SDL, XInput) sees. The device is plugged in with the first input and removed when the
//! session ends. Nothing is ever installed: [`status`] says whether the driver or the device
//! node is there, and the launcher tells the player what to do when it is not.

// Without a backend (macOS) the plumbing around it has nothing to carry.
#![cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]

mod map;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

use std::sync::{Arc, Mutex};

pub use map::{PadState, axis, map_packet, xbutton};

use super::{InputPacket, OutputPacket, PadSink, PadTarget};

#[cfg(target_os = "linux")]
use self::linux as backend;
#[cfg(windows)]
use self::windows as backend;

/// Whether this PC can give the phone a virtual controller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VirtualPadStatus {
    /// The driver (Windows) or `/dev/uinput` (Linux) is there and usable.
    Ready,
    /// ViGEmBus is not installed (Windows), or `/dev/uinput` does not exist (Linux).
    DriverMissing,
    /// The device exists but this user may not use it; the text is the system's reason.
    NoAccess(String),
    /// No backend for this operating system.
    Unsupported,
}

#[derive(Debug, thiserror::Error)]
pub enum VirtualPadError {
    #[error("no virtual controller driver: {0}")]
    Driver(String),
    #[error("this system has no virtual controller support")]
    Unsupported,
}

/// What a platform backend does; one controller at a time.
trait Backend: Send {
    fn plug(&mut self) -> Result<(), String>;
    fn update(&mut self, state: &PadState) -> Result<(), String>;
    fn unplug(&mut self);
}

type Feedback = Arc<dyn Fn(OutputPacket) + Send + Sync>;

struct Inner {
    backend: Box<dyn Backend>,
    plugged: bool,
    last: Option<PadState>,
    /// A plug-in or update failed: say so once, not for every packet.
    failed: bool,
}

pub struct VirtualPad {
    inner: Mutex<Inner>,
}

/// Looks at the system without creating anything.
pub fn status() -> VirtualPadStatus {
    #[cfg(any(target_os = "linux", windows))]
    {
        backend::status()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        VirtualPadStatus::Unsupported
    }
}

impl VirtualPad {
    /// Opens the driver. No controller exists until the first input. `feedback` receives rumble
    /// as `large_motor`/`small_motor` of an [`OutputPacket`] where the backend has it (ViGEmBus
    /// does; uinput does not yet).
    pub fn open(feedback: Option<Feedback>) -> Result<VirtualPad, VirtualPadError> {
        #[cfg(any(target_os = "linux", windows))]
        {
            let backend = backend::open(feedback)?;
            Ok(VirtualPad::with_backend(backend))
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = feedback;
            Err(VirtualPadError::Unsupported)
        }
    }

    fn with_backend(backend: Box<dyn Backend>) -> VirtualPad {
        VirtualPad {
            inner: Mutex::new(Inner {
                backend,
                plugged: false,
                last: None,
                failed: false,
            }),
        }
    }

    /// Takes the controller away now.
    pub fn release(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.release();
        }
    }
}

impl Inner {
    fn release(&mut self) {
        if self.plugged {
            self.backend.unplug();
            self.plugged = false;
        }
        self.last = None;
        self.failed = false;
    }

    /// Plugs in if needed and writes the state. True when the controller holds it.
    fn apply(&mut self, state: PadState) -> bool {
        if !self.plugged {
            match self.backend.plug() {
                Ok(()) => self.plugged = true,
                Err(error) => {
                    if !std::mem::replace(&mut self.failed, true) {
                        tracing::warn!("virtual controller could not be plugged in: {error}");
                    }
                    return false;
                }
            }
        }
        if self.last == Some(state) {
            return true;
        }
        match self.backend.update(&state) {
            Ok(()) => {
                self.last = Some(state);
                true
            }
            Err(error) => {
                // The driver may not be ready for a moment after the plug-in; try again with the
                // next packet.
                if !std::mem::replace(&mut self.failed, true) {
                    tracing::warn!("virtual controller update failed: {error}");
                }
                false
            }
        }
    }
}

impl PadSink for VirtualPad {
    fn input(&self, packet: &InputPacket) {
        let state = map_packet(packet);
        if let Ok(mut inner) = self.inner.lock() {
            inner.apply(state);
        }
    }

    fn ended(&self) {
        self.release();
    }
}

/// LudusCtrl's pad is already an XInput state.
impl PadTarget for VirtualPad {
    fn write(&self, state: &PadState) -> bool {
        self.inner
            .lock()
            .map(|mut inner| inner.apply(*state))
            .unwrap_or(false)
    }

    fn release(&self) {
        VirtualPad::release(self);
    }
}

impl Drop for VirtualPad {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::button;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Log {
        plugs: AtomicUsize,
        unplugs: AtomicUsize,
        updates: Mutex<Vec<PadState>>,
        fail_plug: bool,
    }

    struct Fake(Arc<Log>);

    impl Backend for Fake {
        fn plug(&mut self) -> Result<(), String> {
            if self.0.fail_plug {
                return Err("no".into());
            }
            self.0.plugs.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn update(&mut self, state: &PadState) -> Result<(), String> {
            self.0.updates.lock().unwrap().push(*state);
            Ok(())
        }
        fn unplug(&mut self) {
            self.0.unplugs.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn pad(log: &Arc<Log>) -> VirtualPad {
        VirtualPad::with_backend(Box::new(Fake(log.clone())))
    }

    fn packet(buttons: u32) -> InputPacket {
        InputPacket {
            buttons,
            ..InputPacket::neutral(1)
        }
    }

    #[test]
    fn plugs_in_lazily_sends_changes_only_and_unplugs() {
        let log = Arc::new(Log::default());
        let pad = pad(&log);
        assert_eq!(log.plugs.load(Ordering::SeqCst), 0);
        pad.input(&packet(0));
        pad.input(&packet(0));
        pad.input(&packet(button::CROSS));
        pad.input(&packet(button::CROSS));
        assert_eq!(log.plugs.load(Ordering::SeqCst), 1);
        let updates = log.updates.lock().unwrap().clone();
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[1].buttons, xbutton::A);
        pad.ended();
        assert_eq!(log.unplugs.load(Ordering::SeqCst), 1);
        // A new session plugs in again and sends its first state even if it equals the last.
        pad.input(&packet(button::CROSS));
        assert_eq!(log.plugs.load(Ordering::SeqCst), 2);
        assert_eq!(log.updates.lock().unwrap().len(), 3);
        drop(pad);
        assert_eq!(log.unplugs.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn ended_without_input_and_a_failing_plug_do_nothing_bad() {
        let log = Arc::new(Log {
            fail_plug: true,
            ..Log::default()
        });
        let pad = pad(&log);
        pad.ended();
        pad.input(&packet(0));
        pad.input(&packet(1));
        assert!(log.updates.lock().unwrap().is_empty());
        drop(pad);
        assert_eq!(log.unplugs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_ludusctrl_state_goes_through_as_it_is() {
        let log = Arc::new(Log::default());
        let pad = pad(&log);
        let state = PadState {
            buttons: xbutton::A | xbutton::GUIDE,
            lx: -32768,
            ry: 32767,
            right_trigger: 9,
            ..PadState::default()
        };
        assert!(PadTarget::write(&pad, &state));
        assert!(PadTarget::write(&pad, &state), "unchanged is still held");
        assert_eq!(*log.updates.lock().unwrap(), vec![state]);
        PadTarget::release(&pad);
        assert_eq!(log.unplugs.load(Ordering::SeqCst), 1);
        let failing = Arc::new(Log {
            fail_plug: true,
            ..Log::default()
        });
        assert!(!PadTarget::write(
            &super::VirtualPad::with_backend(Box::new(Fake(failing))),
            &state
        ));
    }

    #[test]
    fn status_does_not_panic() {
        let _ = status();
    }
}

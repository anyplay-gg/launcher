//! The one virtual controller of Phone as Controller. The AnyPS5 Remote server and LudusCtrl both
//! drive it, so with both on the system still sees one controller (docs/DESIGN.md 18). It is
//! opened on first use and kept while the launcher runs; each server plugs it in with its first
//! input and unplugs it when its phone leaves. Rumble goes back to the Remote phone, the only
//! one that can take it.

use std::sync::{Arc, Mutex, Weak};

use anyps5_core::remote::{OutputPacket, RemoteServer, VirtualPad, VirtualPadError};

type Feedback = Arc<dyn Fn(OutputPacket) + Send + Sync>;
type Open = Box<dyn Fn(Feedback) -> Result<VirtualPad, VirtualPadError> + Send + Sync>;

pub struct SharedPad {
    open: Open,
    pad: Mutex<Option<Arc<VirtualPad>>>,
    rumble_to: Arc<Mutex<Option<Weak<RemoteServer>>>>,
}

impl SharedPad {
    pub fn new() -> Arc<SharedPad> {
        SharedPad::with_open(Box::new(|feedback| VirtualPad::open(Some(feedback))))
    }

    pub fn with_open(open: Open) -> Arc<SharedPad> {
        Arc::new(SharedPad {
            open,
            pad: Mutex::new(None),
            rumble_to: Arc::default(),
        })
    }

    /// The controller, opened the first time it is asked for. `None` without a driver; the
    /// next call tries again (the player may have installed it meanwhile).
    pub fn get(&self) -> Option<Arc<VirtualPad>> {
        let mut pad = self.pad.lock().ok()?;
        if let Some(pad) = pad.as_ref() {
            return Some(pad.clone());
        }
        let target = self.rumble_to.clone();
        let feedback: Feedback = Arc::new(move |packet| {
            let server = target
                .lock()
                .ok()
                .and_then(|t| t.as_ref().and_then(Weak::upgrade));
            if let Some(server) = server {
                server.send_feedback(&packet);
            }
        });
        match (self.open)(feedback) {
            Ok(opened) => {
                let opened = Arc::new(opened);
                *pad = Some(opened.clone());
                Some(opened)
            }
            Err(error) => {
                tracing::warn!("no virtual controller: {error}");
                None
            }
        }
    }

    /// Where the game's rumble goes: the Remote server's phone, or nowhere.
    pub fn rumble_to(&self, server: Option<Weak<RemoteServer>>) {
        if let Ok(mut target) = self.rumble_to.lock() {
            *target = server;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn the_pad_is_opened_once_and_shared() {
        static OPENED: AtomicUsize = AtomicUsize::new(0);
        let shared = SharedPad::with_open(Box::new(|_| {
            OPENED.fetch_add(1, Ordering::SeqCst);
            Ok(VirtualPad::inert())
        }));
        let first = shared.get().unwrap();
        let second = shared.get().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(OPENED.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn without_a_driver_it_tries_again_next_time() {
        static TRIED: AtomicUsize = AtomicUsize::new(0);
        let shared = SharedPad::with_open(Box::new(|_| {
            TRIED.fetch_add(1, Ordering::SeqCst);
            Err(VirtualPadError::Unsupported)
        }));
        assert!(shared.get().is_none() && shared.get().is_none());
        assert_eq!(TRIED.load(Ordering::SeqCst), 2);
    }
}

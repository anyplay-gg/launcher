//! ViGEmBus through the `vigem-client` crate (MIT): a virtual Xbox 360 controller. The driver is
//! the player's to install; this only talks to it when it is there.

use std::sync::Arc;

use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};

use super::{Backend, Feedback, PadState, VirtualPadError, VirtualPadStatus};
use crate::remote::OutputPacket;

pub fn status() -> VirtualPadStatus {
    match Client::connect() {
        Ok(_) => VirtualPadStatus::Ready,
        Err(_) => VirtualPadStatus::DriverMissing,
    }
}

pub fn open(feedback: Option<Feedback>) -> Result<Box<dyn Backend>, VirtualPadError> {
    let client = Client::connect().map_err(|e| VirtualPadError::Driver(e.to_string()))?;
    Ok(Box::new(Vigem {
        client: Arc::new(client),
        target: None,
        feedback,
    }))
}

struct Vigem {
    client: Arc<Client>,
    target: Option<Xbox360Wired<Arc<Client>>>,
    feedback: Option<Feedback>,
}

impl Backend for Vigem {
    fn plug(&mut self) -> Result<(), String> {
        let mut target = Xbox360Wired::new(self.client.clone(), TargetId::XBOX360_WIRED);
        target.plugin().map_err(|e| e.to_string())?;
        target.wait_ready().map_err(|e| e.to_string())?;
        if let Some(feedback) = self.feedback.clone() {
            // The thread ends by itself when the target is unplugged (the request is aborted).
            match target.request_notification() {
                Ok(request) => {
                    request.spawn_thread(move |_, note| {
                        feedback(OutputPacket {
                            large_motor: note.large_motor,
                            small_motor: note.small_motor,
                            ..OutputPacket::default()
                        });
                    });
                }
                Err(error) => tracing::debug!("no rumble from the virtual controller: {error}"),
            }
        }
        self.target = Some(target);
        Ok(())
    }

    fn update(&mut self, state: &PadState) -> Result<(), String> {
        let target = self.target.as_mut().ok_or("not plugged in")?;
        target
            .update(&XGamepad {
                buttons: XButtons(state.buttons),
                left_trigger: state.left_trigger,
                right_trigger: state.right_trigger,
                thumb_lx: state.lx,
                thumb_ly: state.ly,
                thumb_rx: state.rx,
                thumb_ry: state.ry,
            })
            .map_err(|e| e.to_string())
    }

    fn unplug(&mut self) {
        // Dropping the target unplugs it.
        self.target = None;
    }
}

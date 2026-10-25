//! Gamepad polling on its own thread (docs/DESIGN.md 6.3). The thread only translates `gilrs`
//! events into [`PadMessage`]s; repeat timing lives in `ui::input` so it uses the UI clock.
//!
//! D-pad and left stick are directions (stick axis above 0.55, otherwise that axis counts as 0;
//! the dominant axis wins). A / Cross = select, B / Circle = back, Y / Triangle = options,
//! Menu / Start = options. The Home / Guide button (`Button::Mode`) never becomes a command: it
//! switches between the game and the launcher (`ui::home_button`), and while a game is in front
//! it and Create (which saves a clip) are the only buttons that count.

use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use gilrs::{Axis, Button, EventType, Gamepad, GamepadId, Gilrs, MappingSource};

use crate::ui::home_button::HomeButton;
use crate::ui::input::{Command, PadMessage};

/// Stick deflection that makes a direction.
pub const STICK_THRESHOLD: f32 = 0.55;
/// Deflection below which a held stick direction lets go. Between the two nothing changes, so a
/// thumb resting near the threshold does not flutter between "right" and "nothing".
pub const STICK_RELEASE: f32 = 0.38;
/// A held direction gives way to the other axis only when that one is this much stronger, so a
/// thumb near the diagonal does not jump between "right" and "up".
pub const STICK_SWITCH: f32 = 1.25;

/// Direction for an (x, y) pair with y up: dominant axis wins, ties and zero give `None`.
pub fn direction(x: f32, y: f32) -> Option<Command> {
    if x.abs() > y.abs() {
        Some(if x > 0.0 {
            Command::Right
        } else {
            Command::Left
        })
    } else if y != 0.0 {
        Some(if y > 0.0 { Command::Up } else { Command::Down })
    } else {
        None
    }
}

fn stick_axis(v: f32) -> f32 {
    if v.abs() > STICK_THRESHOLD { v } else { 0.0 }
}

/// The left stick as a direction, with hysteresis: it engages at [`STICK_THRESHOLD`], lets go at
/// [`STICK_RELEASE`] and changes axis only for a clearly stronger other axis.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stick {
    held: Option<Command>,
}

impl Stick {
    /// The direction for a stick at (x, y), y up.
    pub fn update(&mut self, x: f32, y: f32) -> Option<Command> {
        let fresh = || direction(stick_axis(x), stick_axis(y));
        self.held = match self.held {
            None => fresh(),
            Some(held) => {
                // How far the stick still leans the held way, and how far across it.
                let (along, across) = match held {
                    Command::Right => (x, y),
                    Command::Left => (-x, y),
                    Command::Up => (y, x),
                    _ => (-y, x),
                };
                if along >= STICK_RELEASE {
                    if across.abs() >= STICK_THRESHOLD && across.abs() > along * STICK_SWITCH {
                        fresh()
                    } else {
                        Some(held)
                    }
                } else {
                    fresh()
                }
            }
        };
        self.held
    }
}

/// Held directional input from the D-pad and the left stick.
#[derive(Debug, Default)]
struct Directions {
    dpad: (f32, f32),
    stick: (f32, f32),
    stick_state: Stick,
}

impl Directions {
    /// The D-pad wins when it is pressed, else the stick. Call after every axis event.
    fn current(&mut self) -> Option<Command> {
        let stick = self.stick_state.update(self.stick.0, self.stick.1);
        if self.dpad != (0.0, 0.0) {
            direction(self.dpad.0, self.dpad.1)
        } else {
            stick
        }
    }
}

/// Starts the poll loop and returns the receiving end. A missing gamepad backend is logged once
/// and is not fatal (the receiver then simply stays silent).
pub fn spawn(ctx: egui::Context, home: Arc<HomeButton>) -> Receiver<PadMessage> {
    let (tx, rx) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("gamepad".into())
        .spawn(move || poll(&ctx, &tx, &home));
    if let Err(err) = spawned {
        tracing::warn!("gamepad thread not started: {err}");
    }
    rx
}

/// How long to wait before the backend is started again after it failed or panicked.
const RESTART_AFTER: Duration = Duration::from_secs(5);

/// What the pad layer reads through: Windows.Gaming.Input on Windows (gilrs' default; it only
/// delivers input while a window of ours has focus), evdev on Linux, IOKit HID on macOS.
const BACKEND: &str = if cfg!(windows) {
    "WGI"
} else if cfg!(target_os = "linux") {
    "evdev"
} else {
    "HID"
};

/// Sony's USB vendor id and the DualSense (0CE6) and DualSense Edge (0DF2) product ids. SDL's
/// controller database that gilrs bundles has a mapping for both.
const SONY: u16 = 0x054C;
const DUALSENSE: [u16; 2] = [0x0CE6, 0x0DF2];

/// Whether a vendor and product id name a DualSense.
fn is_dualsense(vendor: Option<u16>, product: Option<u16>) -> bool {
    vendor == Some(SONY) && product.is_some_and(|p| DUALSENSE.contains(&p))
}

/// The line the log gets for a pad that was found: who it is, which mapping it got and through
/// which backend it is read, so a "my controller is not detected" report can be answered from
/// `launcher.log` alone.
fn describe(
    name: &str,
    vendor: Option<u16>,
    product: Option<u16>,
    uuid: &str,
    mapping: Option<&str>,
    source: MappingSource,
) -> String {
    let id = |v: Option<u16>| v.map_or_else(|| "????".to_owned(), |v| format!("{v:04X}"));
    let mapped = match (source, mapping) {
        (MappingSource::SdlMappings, Some(map)) => format!("mapped \"{map}\""),
        (MappingSource::SdlMappings, None) => "mapped".to_owned(),
        _ => "no SDL mapping (default layout)".to_owned(),
    };
    let kind = if is_dualsense(vendor, product) {
        " [DualSense]"
    } else {
        ""
    };
    format!(
        "gamepad found: \"{name}\"{kind} vid {} pid {} uuid {uuid}, {mapped}, backend {BACKEND}",
        id(vendor),
        id(product)
    )
}

fn uuid_text(bytes: [u8; 16]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn log_pad(pad: &Gamepad<'_>) {
    let line = describe(
        pad.os_name(),
        pad.vendor_id(),
        pad.product_id(),
        &uuid_text(pad.uuid()),
        pad.map_name(),
        pad.mapping_source(),
    );
    if pad.mapping_source() == MappingSource::SdlMappings {
        tracing::info!("{line}");
    } else {
        tracing::warn!("{line}");
    }
}

fn log_all(gilrs: &Gilrs) {
    let mut count = 0;
    for (_, pad) in gilrs.gamepads() {
        log_pad(&pad);
        count += 1;
    }
    if count == 0 {
        tracing::info!(
            "no gamepad found at start (backend {BACKEND}); pads plugged in later are logged"
        );
    }
}

fn log_connected(gilrs: &Gilrs, id: GamepadId) {
    if let Some(pad) = gilrs.connected_gamepad(id) {
        log_pad(&pad);
    }
}

/// Runs the pad layer for as long as the launcher lives. The backend is started again when it
/// could not start (one odd device on the machine can fail it) or when it panicked, because a
/// dead pad thread would otherwise leave the launcher without a controller until it is restarted.
fn poll(ctx: &egui::Context, tx: &Sender<PadMessage>, home: &HomeButton) {
    let mut attempt = 0u32;
    loop {
        let run = panic::catch_unwind(AssertUnwindSafe(|| session(ctx, tx, home, &mut attempt)));
        if run.is_err() {
            tracing::warn!("gamepad thread panicked; starting it again");
        }
        thread::sleep(RESTART_AFTER);
    }
}

/// One run of the backend; returns only when it could not start.
fn session(ctx: &egui::Context, tx: &Sender<PadMessage>, home: &HomeButton, attempt: &mut u32) {
    let mut gilrs = match Gilrs::new() {
        Ok(gilrs) => gilrs,
        Err(err) => {
            // Once, then about once a minute.
            if (*attempt).is_multiple_of(12) {
                tracing::warn!("gamepad backend unavailable ({BACKEND}): {err}; trying again");
            }
            *attempt += 1;
            return;
        }
    };
    *attempt = 0;
    log_all(&gilrs);
    let mut dirs = Directions::default();
    let mut last = None;
    loop {
        while let Some(event) = gilrs.next_event() {
            let send = |message| {
                crate::ui::latency::pad_event_read();
                if tx.send(message).is_ok() {
                    ctx.request_repaint();
                }
            };
            // With a game in front the launcher hears the Home button and nothing else; the held
            // directions are kept up to date but not passed on.
            let mine = home.accepts_pad();
            match event.event {
                EventType::ButtonPressed(Button::Mode, _) => {
                    home.press();
                }
                // Create on a DualSense, Share on a DualShock, View or Back on an Xbox pad: it
                // saves a clip, which is only ever wanted while a game is in front.
                EventType::ButtonPressed(Button::Select, _) => send(PadMessage::Create),
                EventType::ButtonPressed(..) if !mine => {}
                EventType::ButtonPressed(button, _) => match button {
                    Button::South => send(PadMessage::Button(Command::Select)),
                    Button::East => send(PadMessage::Button(Command::Back)),
                    Button::North | Button::Start => send(PadMessage::Button(Command::Options)),
                    Button::DPadUp => dirs.dpad.1 = 1.0,
                    Button::DPadDown => dirs.dpad.1 = -1.0,
                    Button::DPadLeft => dirs.dpad.0 = -1.0,
                    Button::DPadRight => dirs.dpad.0 = 1.0,
                    _ => {}
                },
                EventType::ButtonReleased(button, _) => match button {
                    Button::DPadUp | Button::DPadDown => dirs.dpad.1 = 0.0,
                    Button::DPadLeft | Button::DPadRight => dirs.dpad.0 = 0.0,
                    _ => {}
                },
                EventType::AxisChanged(axis, value, _) => match axis {
                    Axis::LeftStickX => dirs.stick.0 = value,
                    Axis::LeftStickY => dirs.stick.1 = value,
                    Axis::DPadX => dirs.dpad.0 = value,
                    Axis::DPadY => dirs.dpad.1 = value,
                    _ => {}
                },
                EventType::Connected => log_connected(&gilrs, event.id),
                EventType::Disconnected => {
                    tracing::info!("gamepad removed");
                    dirs = Directions::default();
                }
                _ => {}
            }
            let now = if mine { dirs.current() } else { None };
            if now != last {
                last = now;
                send(PadMessage::Direction(now));
            }
        }
        // 4 ms (a pad reports at 125 to 250 Hz) puts the wait for a press at 2 ms on average and
        // costs no measurable CPU; with none connected there is nothing to poll for, and a plug-in
        // is noticed within a tenth of a second.
        let idle = gilrs.gamepads().next().is_none();
        thread::sleep(Duration::from_millis(if idle { 100 } else { 4 }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dualsense_is_known_by_its_ids() {
        assert!(is_dualsense(Some(0x054C), Some(0x0CE6)));
        assert!(is_dualsense(Some(0x054C), Some(0x0DF2)));
        assert!(!is_dualsense(Some(0x054C), Some(0x09CC)), "a DualShock 4");
        assert!(!is_dualsense(Some(0x045E), Some(0x0CE6)));
        assert!(!is_dualsense(None, None));
    }

    #[test]
    fn the_log_line_says_who_what_mapping_and_backend() {
        let line = describe(
            "PS5 Controller",
            Some(0x054C),
            Some(0x0CE6),
            "030000004c050000e60c000000000000",
            Some("PS5 Controller"),
            MappingSource::SdlMappings,
        );
        assert!(line.contains("[DualSense]"), "{line}");
        assert!(line.contains("vid 054C pid 0CE6"), "{line}");
        assert!(line.contains("mapped \"PS5 Controller\""), "{line}");
        assert!(line.contains(&format!("backend {BACKEND}")), "{line}");
        let line = describe("Pad", None, None, "00", None, MappingSource::Driver);
        assert!(line.contains("vid ???? pid ????"), "{line}");
        assert!(line.contains("no SDL mapping"), "{line}");
        assert!(!line.contains("DualSense"), "{line}");
    }

    #[test]
    fn the_uuid_is_written_as_sdl_writes_it() {
        let mut bytes = [0u8; 16];
        bytes[0] = 0x03;
        bytes[4] = 0x4c;
        bytes[5] = 0x05;
        assert_eq!(
            uuid_text(bytes),
            format!("030000004c050000{}", "0".repeat(16))
        );
    }

    #[test]
    fn dominant_axis_wins() {
        assert_eq!(direction(1.0, 0.2), Some(Command::Right));
        assert_eq!(direction(-0.8, 0.3), Some(Command::Left));
        assert_eq!(direction(0.1, 0.9), Some(Command::Up));
        assert_eq!(direction(0.0, -1.0), Some(Command::Down));
        assert_eq!(direction(0.0, 0.0), None);
    }

    #[test]
    fn stick_below_the_threshold_is_ignored() {
        let mut dirs = Directions {
            stick: (0.5, 0.0),
            ..Directions::default()
        };
        assert_eq!(dirs.current(), None);
        let mut dirs = Directions {
            stick: (0.6, 0.5),
            ..Directions::default()
        };
        assert_eq!(
            dirs.current(),
            Some(Command::Right),
            "the weak axis counts as 0"
        );
    }

    #[test]
    fn dpad_beats_stick() {
        let mut dirs = Directions {
            dpad: (0.0, -1.0),
            stick: (1.0, 0.0),
            ..Directions::default()
        };
        assert_eq!(dirs.current(), Some(Command::Down));
    }

    #[test]
    fn a_stick_near_the_threshold_does_not_flutter() {
        let mut stick = Stick::default();
        assert_eq!(stick.update(0.50, 0.0), None);
        assert_eq!(stick.update(0.56, 0.0), Some(Command::Right));
        // Wobbling between the two thresholds keeps the direction ...
        for x in [0.50, 0.45, 0.52, 0.41, 0.54] {
            assert_eq!(stick.update(x, 0.0), Some(Command::Right), "{x}");
        }
        // ... and it lets go only below the release level.
        assert_eq!(stick.update(0.37, 0.0), None);
        // From rest the old threshold applies again.
        assert_eq!(stick.update(0.50, 0.0), None);
    }

    #[test]
    fn a_stick_near_the_diagonal_does_not_jump_between_axes() {
        let mut stick = Stick::default();
        assert_eq!(stick.update(0.9, 0.2), Some(Command::Right));
        // Up is a little stronger than right now, but not by a quarter.
        assert_eq!(stick.update(0.70, 0.75), Some(Command::Right));
        assert_eq!(stick.update(0.65, 0.80), Some(Command::Right));
        // Clearly up: it switches.
        assert_eq!(stick.update(0.40, 0.90), Some(Command::Up));
        // And back, again only for a clear lead.
        assert_eq!(stick.update(0.75, 0.70), Some(Command::Up));
        assert_eq!(stick.update(0.95, 0.30), Some(Command::Right));
    }

    #[test]
    fn a_flick_through_the_middle_changes_direction_at_once() {
        let mut stick = Stick::default();
        assert_eq!(stick.update(0.9, 0.0), Some(Command::Right));
        assert_eq!(stick.update(-0.9, 0.0), Some(Command::Left));
        assert_eq!(stick.update(0.0, 0.9), Some(Command::Up));
        assert_eq!(stick.update(0.0, -0.9), Some(Command::Down));
    }
}

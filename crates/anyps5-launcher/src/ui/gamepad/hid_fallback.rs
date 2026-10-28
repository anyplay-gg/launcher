//! A DualSense or DualShock 4 that the system's pad API does not list, read straight from its HID
//! reports (docs/DESIGN.md 6.9).
//!
//! On Windows the launcher reads pads through Windows.Gaming.Input (gilrs). A DualSense on a
//! cable can be missing from it: the Controllers page then says that no controller is connected,
//! while DS4Windows, which hands out a virtual Xbox pad, makes it work. This module is the
//! way around that. It looks at gilrs' list and does nothing while gilrs lists any pad. When
//! gilrs has listed none for [`GRACE`], it opens the Sony pads through `hidapi` (without exclusive
//! access, and only reading: no report is ever written), turns their input reports into the same
//! button and axis changes gilrs would deliver, and feeds them to the same rules (`press_effect`,
//! the held directions, the Controllers page, the Home button). It lets go of the pads as soon as
//! gilrs lists one, so a pad is never counted twice.
//!
//! The reports are generic device facts, from the public sources of the Linux kernel
//! (`drivers/hid/hid-playstation.c`) and SDL (`SDL_hidapi_ps5.c`, `SDL_hidapi_ps4.c`). The
//! common part starts after the report id (USB), one byte later (DualSense, Bluetooth) or two
//! bytes later (DualShock 4, Bluetooth). Its layout, relative to its start:
//!
//! | Pad | Sticks (LX LY RX RY) | Buttons 0, 1, 2 | Triggers L2, R2 |
//! |---|---|---|---|
//! | DualSense | 0 to 3 | 7, 8, 9 | 4, 5 |
//! | DualShock 4 | 0 to 3 | 4, 5, 6 | 7, 8 |
//! | Short Bluetooth report (id 0x01, 10 bytes) | 0 to 3 | 4, 5, 6 | none |
//!
//! Buttons 0: the hat switch in the low nibble (0 up, then clockwise, 8 none), Square 0x10,
//! Cross 0x20, Circle 0x40, Triangle 0x80. Buttons 1: L1, R1, L2, R2, Create or Share, Options,
//! L3, R3 from bit 0 up. Buttons 2: the PS button is bit 0.

#![cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gilrs::{Axis, Button};

use super::{Directions, PressEffect, press_effect};

use crate::ui::home_button::HomeButton;
use crate::ui::input::{Command, PadMessage};
use crate::ui::lightbar::Kind;
use crate::ui::nav_bindings;
use crate::ui::pad_hub::{self, PadInfo};
use crate::ui::ps_button::crc32;
use crate::ui::talk;

/// How long gilrs must list no pad before the pads are read directly: a pad that was just plugged
/// in, or the backend that is still starting, gets time to show up there first.
const GRACE: Duration = Duration::from_secs(3);

/// Pad numbers of this module on the Controllers page start here, far from gilrs' own.
const ID_BASE: usize = 1_000_000;

/// gilrs lists at least one pad. Written by the gilrs thread.
static GILRS_HAS_PAD: AtomicBool = AtomicBool::new(false);

/// The pads this module reads, for the list the Controllers page shows next to gilrs' pads.
static PADS: Mutex<Vec<PadInfo>> = Mutex::new(Vec::new());

/// The gilrs thread: whether gilrs lists a pad right now.
pub fn gilrs_has_pad(has: bool) {
    GILRS_HAS_PAD.store(has, Ordering::Relaxed);
}

/// The pads read here, to be added to the list gilrs publishes.
pub fn pads() -> Vec<PadInfo> {
    PADS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The buttons a report can change, in the order of [`State::down`].
const BUTTONS: [Button; 17] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftTrigger2,
    Button::RightTrigger2,
    Button::Select,
    Button::Start,
    Button::LeftThumb,
    Button::RightThumb,
    Button::Mode,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
];

/// What one input report says, in the terms of gilrs: sticks from -1 to 1 with y up, triggers from
/// 0 to 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct State {
    down: [bool; 17],
    left: (f32, f32),
    right: (f32, f32),
    l2: f32,
    r2: f32,
}

/// The report layouts, by where the fields sit.
#[derive(Clone, Copy)]
enum Shape {
    Short,
    DualSense,
    DualShock,
}

/// A Bluetooth input report ends in a CRC-32 over a 0xA1 byte (the HID input header) and the rest.
fn crc_ok(report: &[u8]) -> bool {
    let Some(split) = report.len().checked_sub(4) else {
        return false;
    };
    let (body, tail) = report.split_at(split);
    let stored = u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]);
    crc32(crc32(0, &[0xA1]), body) == stored
}

/// A stick byte (0 to 255, centre 127.5) as -1 to 1.
fn axis(byte: u8) -> f32 {
    (f32::from(byte) - 127.5) / 127.5
}

/// The state an input report of a pad of this `kind` holds; `None` for a report that is not one
/// of the layouts above (an output echo, a different mode, a damaged Bluetooth packet).
fn decode(kind: Kind, report: &[u8]) -> Option<State> {
    let (start, shape) = match (kind, report.first().copied()?, report.len()) {
        (_, 0x01, 10) => (1, Shape::Short),
        (Kind::DualSense, 0x01, 64) => (1, Shape::DualSense),
        (Kind::DualSense, 0x31, 78) if crc_ok(report) => (2, Shape::DualSense),
        (Kind::DualShock4, 0x01, 64..) => (1, Shape::DualShock),
        (Kind::DualShock4, 0x11..=0x19, 78..) if report[1] & 0x80 != 0 && crc_ok(&report[..78]) => {
            (3, Shape::DualShock)
        }
        _ => return None,
    };
    let (b0, b1, b2, triggers) = match shape {
        Shape::Short => (start + 4, start + 5, start + 6, None),
        Shape::DualSense => (
            start + 7,
            start + 8,
            start + 9,
            Some((start + 4, start + 5)),
        ),
        Shape::DualShock => (
            start + 4,
            start + 5,
            start + 6,
            Some((start + 7, start + 8)),
        ),
    };
    let at = |i: usize| report.get(i).copied();
    let (b0, b1, b2) = (at(b0)?, at(b1)?, at(b2)?);
    let (lx, ly, rx, ry) = (at(start)?, at(start + 1)?, at(start + 2)?, at(start + 3)?);
    let (l2, r2) = match triggers {
        Some((l, r)) => (f32::from(at(l)?) / 255.0, f32::from(at(r)?) / 255.0),
        None => (
            f32::from(u8::from(b1 & 0x04 != 0)),
            f32::from(u8::from(b1 & 0x08 != 0)),
        ),
    };
    let hat = b0 & 0x0F;
    Some(State {
        down: [
            b0 & 0x20 != 0,
            b0 & 0x40 != 0,
            b0 & 0x10 != 0,
            b0 & 0x80 != 0,
            b1 & 0x01 != 0,
            b1 & 0x02 != 0,
            b1 & 0x04 != 0,
            b1 & 0x08 != 0,
            b1 & 0x10 != 0,
            b1 & 0x20 != 0,
            b1 & 0x40 != 0,
            b1 & 0x80 != 0,
            b2 & 0x01 != 0,
            matches!(hat, 7 | 0 | 1),
            matches!(hat, 3..=5),
            matches!(hat, 5..=7),
            matches!(hat, 1..=3),
        ],
        left: (axis(lx), -axis(ly)),
        right: (axis(rx), -axis(ry)),
        l2,
        r2,
    })
}

/// Where the changes go: the same places the gilrs thread sends them to.
struct Feed<'a> {
    ctx: &'a egui::Context,
    tx: &'a Sender<PadMessage>,
    home: &'a HomeButton,
    dirs: Directions,
    last: Option<Command>,
}

impl Feed<'_> {
    fn send(&self, message: PadMessage) {
        crate::ui::latency::pad_event_read();
        if self.tx.send(message).is_ok() {
            self.ctx.request_repaint();
        }
    }

    /// Everything that changed from `prev` to `now` on pad number `pad`.
    fn apply(&mut self, pad: usize, prev: &State, now: &State) {
        let bindings = nav_bindings::current();
        // With a game in front the launcher hears the Home button and Create and nothing else;
        // while a binding waits for a button the pad only lights the page.
        let mine = self.home.accepts_pad() && !nav_bindings::capturing();
        for (i, button) in BUTTONS.iter().copied().enumerate() {
            if prev.down[i] == now.down[i] {
                continue;
            }
            let down = now.down[i];
            pad_hub::note_button(pad, button, down);
            talk::shared().gilrs_button(button, down);
            if !down {
                match button {
                    Button::DPadUp | Button::DPadDown => self.dirs.dpad.1 = 0.0,
                    Button::DPadLeft | Button::DPadRight => self.dirs.dpad.0 = 0.0,
                    _ => {}
                }
                continue;
            }
            pad_hub::note_press(button);
            let effect = press_effect(button, mine, &bindings);
            // The talk button belongs to talking: a launcher button it shares gives way.
            let effect = if matches!(effect, PressEffect::Command(_) | PressEffect::Tab(_))
                && talk::shared().button().is_some_and(|b| b.gilrs() == button)
            {
                PressEffect::Nothing
            } else {
                effect
            };
            match effect {
                PressEffect::Home => {
                    self.home.press();
                }
                PressEffect::Create => self.send(PadMessage::Create),
                PressEffect::Command(command) => self.send(PadMessage::Button(command)),
                PressEffect::Tab(backwards) => self.send(PadMessage::Tab { backwards }),
                PressEffect::Dpad(Command::Up) => self.dirs.dpad.1 = 1.0,
                PressEffect::Dpad(Command::Down) => self.dirs.dpad.1 = -1.0,
                PressEffect::Dpad(Command::Left) => self.dirs.dpad.0 = -1.0,
                PressEffect::Dpad(_) => self.dirs.dpad.0 = 1.0,
                PressEffect::Nothing => {}
            }
        }
        let moved = |a: f32, b: f32| (a - b).abs() > 0.004;
        for (which, before, after) in [
            (Axis::LeftStickX, prev.left.0, now.left.0),
            (Axis::LeftStickY, prev.left.1, now.left.1),
            (Axis::RightStickX, prev.right.0, now.right.0),
            (Axis::RightStickY, prev.right.1, now.right.1),
            (Axis::LeftZ, prev.l2, now.l2),
            (Axis::RightZ, prev.r2, now.r2),
        ] {
            if moved(before, after) {
                pad_hub::note_axis(pad, which, after);
            }
        }
        self.dirs.stick = now.left;
        self.dirs.stick_state.set_zone(nav_bindings::deadzone());
        let direction = if mine { self.dirs.current() } else { None };
        if direction != self.last {
            self.last = direction;
            self.send(PadMessage::Direction(direction));
        }
        if pad_hub::watched() {
            self.ctx.request_repaint();
        }
    }
}

/// Starts the reader thread. It idles while gilrs lists a pad.
#[cfg(any(windows, target_os = "linux"))]
pub fn spawn(ctx: egui::Context, tx: Sender<PadMessage>, home: Arc<HomeButton>) {
    let spawned = std::thread::Builder::new()
        .name("hid-pad".into())
        .spawn(move || run(&ctx, &tx, &home));
    if let Err(err) = spawned {
        tracing::warn!("HID pad thread not started: {err}");
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn spawn(_ctx: egui::Context, _tx: Sender<PadMessage>, _home: Arc<HomeButton>) {}

#[cfg(any(windows, target_os = "linux"))]
use io::run;

#[cfg(any(windows, target_os = "linux"))]
mod io {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::Sender;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{Feed, GILRS_HAS_PAD, GRACE, ID_BASE, PADS, State, decode};

    use crate::ui::gamepad::{ActivePad, Directions, Turn};
    use crate::ui::home_button::HomeButton;
    use crate::ui::input::PadMessage;
    use crate::ui::lightbar::{Kind, kind_of};
    use crate::ui::pad_hub::{self, Link, PadInfo};
    use crate::ui::pad_info::{self, Battery, Family};

    /// How often the list of devices is looked at again: while no pad is open (a plug-in is
    /// waited for), and while one is (only a second pad is).
    const RESCAN: Duration = Duration::from_secs(2);
    const RESCAN_OPEN: Duration = Duration::from_secs(10);

    struct Open {
        slot: usize,
        key: String,
        kind: Kind,
        product: u16,
        link: Link,
        device: hidapi::HidDevice,
        /// The state of the last report; `None` until the first one, which only sets it, so a
        /// button already held when the pad is opened is not a press.
        prev: Option<State>,
    }

    fn name_of(kind: Kind) -> &'static str {
        match kind {
            Kind::DualSense => "DualSense Wireless Controller",
            Kind::DualShock4 => "DualShock 4 Wireless Controller",
        }
    }

    /// Tells the Controllers page and the top bar which pads are read here.
    fn publish(open: &[Open], active: Option<usize>) {
        let infos: Vec<PadInfo> = open
            .iter()
            .map(|pad| PadInfo {
                id: ID_BASE + pad.slot,
                name: name_of(pad.kind).to_owned(),
                vendor: Some(0x054C),
                product: Some(pad.product),
                family: Family::PlayStation,
                link: pad.link,
                battery: Battery::Unknown,
                backend: "HID".to_owned(),
                active: active == Some(pad.slot),
                dualsense: true,
                rumble: false,
            })
            .collect();
        if infos.is_empty() {
            // gilrs listed a pad (or the pad went away): take only this module's pads off the page.
            let mut rest = pad_hub::snapshot().pads;
            rest.retain(|p| p.id < ID_BASE);
            *PADS.lock().unwrap_or_else(|e| e.into_inner()) = Vec::new();
            pad_hub::publish_pads(rest);
            return;
        }
        pad_info::set_active(Family::PlayStation);
        let mut all = pad_hub::snapshot().pads;
        all.retain(|p| p.id < ID_BASE);
        all.extend(infos.iter().cloned());
        *PADS.lock().unwrap_or_else(|e| e.into_inner()) = infos;
        pad_hub::publish_pads(all);
    }

    /// Looks at the HID devices: opens the Sony pads not open yet and forgets the ones gone.
    fn rescan(
        api: &mut Option<hidapi::HidApi>,
        open: &mut Vec<Open>,
        next_slot: &mut usize,
        reported: &mut Vec<String>,
    ) -> Result<(), String> {
        let api = match api {
            Some(api) => {
                api.refresh_devices().map_err(|e| e.to_string())?;
                api
            }
            none => none.insert(hidapi::HidApi::new().map_err(|e| e.to_string())?),
        };
        let mut present = Vec::new();
        for info in api.device_list() {
            let Some(kind) = kind_of(info.vendor_id(), info.product_id()) else {
                continue;
            };
            // The gamepad collection (generic desktop, game pad); hidraw reports no usage.
            if info.usage_page() != 0 && (info.usage_page() != 0x01 || info.usage() != 0x05) {
                continue;
            }
            let key = info.path().to_string_lossy().into_owned();
            present.push(key.clone());
            if open.iter().any(|pad| pad.key == key) {
                continue;
            }
            match api.open_path(info.path()) {
                Ok(device) => {
                    let link = match info.bus_type() {
                        hidapi::BusType::Bluetooth => Link::Bluetooth,
                        hidapi::BusType::Usb => Link::Usb,
                        _ => Link::Unknown,
                    };
                    tracing::info!(
                        "gamepad found: \"{}\" vid 054C pid {:04X} ({link:?}), read through HID \
                         because the system's pad API lists none",
                        name_of(kind),
                        info.product_id()
                    );
                    open.push(Open {
                        slot: *next_slot,
                        key,
                        kind,
                        product: info.product_id(),
                        link,
                        device,
                        prev: None,
                    });
                    *next_slot += 1;
                }
                Err(err) => {
                    if !reported.contains(&key) {
                        tracing::info!("HID pad: cannot open {key}: {err}");
                        reported.push(key);
                    }
                }
            }
        }
        open.retain(|pad| present.contains(&pad.key));
        reported.retain(|key| present.contains(key));
        Ok(())
    }

    pub fn run(ctx: &egui::Context, tx: &Sender<PadMessage>, home: &HomeButton) {
        let mut api: Option<hidapi::HidApi> = None;
        let mut open: Vec<Open> = Vec::new();
        let mut next_slot = 0usize;
        let mut reported: Vec<String> = Vec::new();
        let mut api_reported = false;
        let mut no_pad_since: Option<Instant> = None;
        let mut last_scan: Option<Instant> = None;
        let mut active = ActivePad::<usize>::default();
        let mut feed = Feed {
            ctx,
            tx,
            home,
            dirs: Directions::default(),
            last: None,
        };
        let mut buf = [0u8; 256];
        loop {
            if GILRS_HAS_PAD.load(Ordering::Relaxed) {
                // gilrs has a pad: it is the one that counts. Let go of ours.
                if !open.is_empty() {
                    open.clear();
                    active = ActivePad::default();
                    feed.dirs = Directions::default();
                    feed.last = None;
                    publish(&open, None);
                }
                no_pad_since = None;
                last_scan = None;
                thread::sleep(Duration::from_millis(250));
                continue;
            }
            let since = *no_pad_since.get_or_insert_with(Instant::now);
            if since.elapsed() < GRACE {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            let every = if open.is_empty() { RESCAN } else { RESCAN_OPEN };
            if last_scan.is_none_or(|at| at.elapsed() >= every) {
                last_scan = Some(Instant::now());
                let before = open.len();
                match rescan(&mut api, &mut open, &mut next_slot, &mut reported) {
                    Ok(()) => api_reported = false,
                    Err(err) => {
                        if !api_reported {
                            api_reported = true;
                            tracing::info!("HID pad reader unavailable: {err}");
                        }
                    }
                }
                if !open.is_empty() || before != 0 {
                    if active
                        .id
                        .is_none_or(|id| !open.iter().any(|p| p.slot == id))
                    {
                        active = ActivePad::default();
                    }
                    publish(&open, active.id);
                }
            }
            if open.is_empty() {
                if pad_hub::snapshot().pads.iter().any(|p| p.id >= ID_BASE) {
                    publish(&open, None);
                    pad_info::clear();
                }
                thread::sleep(Duration::from_millis(250));
                continue;
            }
            let mut republish = false;
            open.retain_mut(|pad| {
                loop {
                    match pad.device.read_timeout(&mut buf, 0) {
                        Ok(0) => return true,
                        Ok(len) => {
                            let Some(state) = decode(pad.kind, &buf[..len]) else {
                                continue;
                            };
                            let Some(prev) = pad.prev.replace(state) else {
                                continue;
                            };
                            if prev == state {
                                continue;
                            }
                            match active.turn(pad.slot, Instant::now()) {
                                Turn::Same => {}
                                Turn::Switched => republish = true,
                                Turn::Echo => continue,
                            }
                            feed.apply(ID_BASE + pad.slot, &prev, &state);
                        }
                        Err(err) => {
                            tracing::info!("gamepad removed (HID): {err}");
                            republish = true;
                            return false;
                        }
                    }
                }
            });
            if republish {
                if active
                    .id
                    .is_some_and(|id| !open.iter().any(|p| p.slot == id))
                {
                    active = ActivePad::default();
                    feed.dirs = Directions::default();
                    feed.last = None;
                }
                publish(&open, active.id);
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DualSense USB report: sticks centred, nothing pressed.
    fn dualsense_usb() -> Vec<u8> {
        let mut r = vec![0u8; 64];
        r[0] = 0x01;
        r[1..5].fill(128);
        r[8] = 0x08; // hat: none
        r
    }

    fn dualshock_usb() -> Vec<u8> {
        let mut r = vec![0u8; 64];
        r[0] = 0x01;
        r[1..5].fill(128);
        r[5] = 0x08;
        r
    }

    fn with_crc(mut r: Vec<u8>) -> Vec<u8> {
        let split = r.len() - 4;
        let crc = crc32(crc32(0, &[0xA1]), &r[..split]);
        r[split..].copy_from_slice(&crc.to_le_bytes());
        r
    }

    fn down(state: &State, button: Button) -> bool {
        let i = BUTTONS.iter().position(|b| *b == button).unwrap();
        state.down[i]
    }

    #[test]
    fn a_resting_dualsense_has_nothing_down_and_the_sticks_near_the_middle() {
        let s = decode(Kind::DualSense, &dualsense_usb()).unwrap();
        assert!(s.down.iter().all(|d| !d));
        assert!(s.left.0.abs() < 0.01 && s.left.1.abs() < 0.01);
        assert_eq!((s.l2, s.r2), (0.0, 0.0));
    }

    #[test]
    fn dualsense_buttons_sit_where_the_kernel_puts_them() {
        let mut r = dualsense_usb();
        r[8] = 0x20 | 0x10 | 0x08; // Cross and Square, hat none
        r[9] = 0x01 | 0x20; // L1, Options
        r[10] = 0x01; // PS
        let s = decode(Kind::DualSense, &r).unwrap();
        assert!(down(&s, Button::South) && down(&s, Button::West));
        assert!(!down(&s, Button::East) && !down(&s, Button::North));
        assert!(down(&s, Button::LeftTrigger) && down(&s, Button::Start));
        assert!(down(&s, Button::Mode));
        assert!(!down(&s, Button::Select));
    }

    #[test]
    fn the_hat_makes_the_dpad_with_diagonals() {
        let mut r = dualsense_usb();
        for (hat, up, right, below, left) in [
            (0u8, true, false, false, false),
            (1, true, true, false, false),
            (2, false, true, false, false),
            (3, false, true, true, false),
            (4, false, false, true, false),
            (5, false, false, true, true),
            (6, false, false, false, true),
            (7, true, false, false, true),
            (8, false, false, false, false),
        ] {
            r[8] = hat;
            let s = decode(Kind::DualSense, &r).unwrap();
            assert_eq!(down(&s, Button::DPadUp), up, "hat {hat}");
            assert_eq!(down(&s, Button::DPadRight), right, "hat {hat}");
            assert_eq!(down(&s, Button::DPadDown), below, "hat {hat}");
            assert_eq!(down(&s, Button::DPadLeft), left, "hat {hat}");
        }
    }

    #[test]
    fn the_left_stick_has_y_up_and_the_triggers_are_analog() {
        let mut r = dualsense_usb();
        r[1] = 255; // right
        r[2] = 0; // pushed up: the report counts down from the top
        r[5] = 255; // L2 fully
        r[6] = 51; // R2 a fifth
        let s = decode(Kind::DualSense, &r).unwrap();
        assert!(s.left.0 > 0.99, "{:?}", s.left);
        assert!(s.left.1 > 0.99, "{:?}", s.left);
        assert_eq!(s.l2, 1.0);
        assert!((s.r2 - 0.2).abs() < 0.001);
    }

    #[test]
    fn a_dualshock_4_has_its_own_offsets() {
        let mut r = dualshock_usb();
        r[5] = 0x40 | 0x08; // Circle
        r[6] = 0x02 | 0x10; // R1, Share
        r[7] = 0x01; // PS
        r[8] = 200;
        let s = decode(Kind::DualShock4, &r).unwrap();
        assert!(down(&s, Button::East) && !down(&s, Button::South));
        assert!(down(&s, Button::RightTrigger) && down(&s, Button::Select));
        assert!(down(&s, Button::Mode));
        assert!((s.l2 - 200.0 / 255.0).abs() < 0.001);
    }

    #[test]
    fn bluetooth_reports_start_later_and_need_their_signature() {
        let mut r = vec![0u8; 78];
        r[0] = 0x31;
        r[2..6].fill(128);
        r[2 + 7] = 0x28; // Cross, hat none
        let good = with_crc(r.clone());
        let s = decode(Kind::DualSense, &good).unwrap();
        assert!(down(&s, Button::South));
        let mut bad = good;
        bad[20] ^= 0xFF;
        assert!(decode(Kind::DualSense, &bad).is_none(), "damaged packet");

        let mut r = vec![0u8; 78];
        r[0] = 0x11;
        r[1] = 0xC0;
        r[3..7].fill(128);
        r[3 + 4] = 0x48; // Circle, hat none
        let s = decode(Kind::DualShock4, &with_crc(r)).unwrap();
        assert!(down(&s, Button::East));
    }

    #[test]
    fn the_short_bluetooth_report_works_for_both() {
        let mut r = vec![0u8; 10];
        r[0] = 0x01;
        r[1..5].fill(128);
        r[5] = 0x28; // Cross, hat none
        r[6] = 0x04; // L2 as a bit
        for kind in [Kind::DualSense, Kind::DualShock4] {
            let s = decode(kind, &r).unwrap();
            assert!(down(&s, Button::South) && down(&s, Button::LeftTrigger2));
            assert_eq!(s.l2, 1.0);
        }
    }

    #[test]
    fn other_reports_are_not_input() {
        assert!(decode(Kind::DualSense, &[]).is_none());
        assert!(decode(Kind::DualSense, &[0x02; 63]).is_none());
        // A DualSense report the length of a DualShock 4 one.
        let mut r = dualsense_usb();
        r.push(0);
        assert!(decode(Kind::DualSense, &r).is_none());
    }
}

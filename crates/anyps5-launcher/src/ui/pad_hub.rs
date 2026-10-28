//! What the pad thread knows about every connected controller, for the Controllers page
//! (docs/DESIGN.md 6.10): the list (name, link, battery, backend) and the live state of the pad
//! that spoke last (buttons, sticks, triggers) that the button test draws.
//!
//! The pad thread writes ([`publish_pads`], [`note`], [`note_press`]); the UI reads
//! ([`snapshot`]). One mutex held for a few field writes, so neither side waits for the other.
//! Nothing here touches a device.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use gilrs::Button;

use crate::ui::pad_info::{Battery, Family};

/// Every button the test shows, with the name the preferences and the page use for it. The
/// names are the PlayStation ones the launcher's hints use, so a binding reads the same on every
/// pad (`South` is Cross on a DualSense and A on an Xbox pad).
pub const BUTTONS: [(&str, Button); 17] = [
    ("South", Button::South),
    ("East", Button::East),
    ("West", Button::West),
    ("North", Button::North),
    ("L1", Button::LeftTrigger),
    ("R1", Button::RightTrigger),
    ("L2", Button::LeftTrigger2),
    ("R2", Button::RightTrigger2),
    ("Create", Button::Select),
    ("Start", Button::Start),
    ("Home", Button::Mode),
    ("L3", Button::LeftThumb),
    ("R3", Button::RightThumb),
    ("Up", Button::DPadUp),
    ("Down", Button::DPadDown),
    ("Left", Button::DPadLeft),
    ("Right", Button::DPadRight),
];

/// The name of a button, `None` for one the test does not show.
pub fn button_name(button: Button) -> Option<&'static str> {
    BUTTONS
        .iter()
        .find(|(_, b)| *b == button)
        .map(|(name, _)| *name)
}

/// The button a name stands for.
pub fn button_from_name(name: &str) -> Option<Button> {
    BUTTONS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

/// Where a button sits in [`BUTTONS`] (and in [`Live::down`]).
pub fn index_of(name: &str) -> Option<usize> {
    BUTTONS.iter().position(|(n, _)| *n == name)
}

/// How a pad is connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Usb,
    Bluetooth,
    /// The system does not say (a pad behind a wireless adapter, or a platform without the
    /// information).
    Unknown,
}

/// One connected pad.
#[derive(Clone, Debug, PartialEq)]
pub struct PadInfo {
    pub id: usize,
    pub name: String,
    pub vendor: Option<u16>,
    pub product: Option<u16>,
    pub family: Family,
    pub link: Link,
    pub battery: Battery,
    /// What reads it: `gilrs (WGI)`, with `HID` added where the launcher can talk to the pad's
    /// reports (a DualSense or a DualShock 4).
    pub backend: String,
    /// It is the pad the launcher listens to.
    pub active: bool,
    /// A DualSense or a DualShock 4: the light bar can be set. (The name is older than the
    /// DualShock 4 support.)
    pub dualsense: bool,
    /// The pad has a rumble motor the launcher can drive.
    pub rumble: bool,
}

/// The live state of the pad that spoke last.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Live {
    /// Bit `i` is [`BUTTONS`]`[i]`.
    pub down: u32,
    pub left: (f32, f32),
    pub right: (f32, f32),
    pub l2: f32,
    pub r2: f32,
    /// The pad it belongs to.
    pub pad: Option<usize>,
}

impl Live {
    pub fn is_down(&self, name: &str) -> bool {
        index_of(name).is_some_and(|i| self.down & (1 << i) != 0)
    }
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub pads: Vec<PadInfo>,
    pub live: Live,
    /// Counts button presses of any pad, so the page can tell a new one.
    pub press_seq: u64,
    /// The button of the last press.
    pub last_press: Option<&'static str>,
}

static HUB: Mutex<Snapshot> = Mutex::new(Snapshot {
    pads: Vec::new(),
    live: Live {
        down: 0,
        left: (0.0, 0.0),
        right: (0.0, 0.0),
        l2: 0.0,
        r2: 0.0,
        pad: None,
    },
    press_seq: 0,
    last_press: None,
});

static WATCHED: AtomicBool = AtomicBool::new(false);

fn hub() -> std::sync::MutexGuard<'static, Snapshot> {
    HUB.lock().unwrap_or_else(|e| e.into_inner())
}

/// A copy of everything, for one frame.
pub fn snapshot() -> Snapshot {
    hub().clone()
}

/// The page is open and wants a repaint with every pad event.
pub fn set_watched(watched: bool) {
    WATCHED.store(watched, Ordering::Relaxed);
}

pub fn watched() -> bool {
    WATCHED.load(Ordering::Relaxed)
}

/// The pad thread: the list of pads.
pub fn publish_pads(pads: Vec<PadInfo>) {
    let mut hub = hub();
    if let Some(live) = hub.live.pad
        && !pads.iter().any(|p| p.id == live)
    {
        hub.live = Live::default();
    }
    hub.pads = pads;
}

/// The pad thread: a button changed on pad `pad`.
pub fn note_button(pad: usize, button: Button, down: bool) {
    let Some(index) = button_name(button).and_then(index_of) else {
        return;
    };
    let mut hub = hub();
    if hub.live.pad != Some(pad) {
        hub.live = Live {
            pad: Some(pad),
            ..Live::default()
        };
    }
    if down {
        hub.live.down |= 1 << index;
    } else {
        hub.live.down &= !(1 << index);
    }
}

/// The pad thread: a press of a button, for a binding that waits for one.
pub fn note_press(button: Button) {
    if let Some(name) = button_name(button) {
        let mut hub = hub();
        hub.press_seq += 1;
        hub.last_press = Some(name);
    }
}

/// The pad thread: an axis moved on pad `pad`.
pub fn note_axis(pad: usize, axis: gilrs::Axis, value: f32) {
    use gilrs::Axis;
    let mut hub = hub();
    if hub.live.pad != Some(pad) {
        hub.live = Live {
            pad: Some(pad),
            ..Live::default()
        };
    }
    let live = &mut hub.live;
    match axis {
        Axis::LeftStickX => live.left.0 = value,
        Axis::LeftStickY => live.left.1 = value,
        Axis::RightStickX => live.right.0 = value,
        Axis::RightStickY => live.right.1 = value,
        Axis::LeftZ => live.l2 = value.clamp(0.0, 1.0),
        Axis::RightZ => live.r2 = value.clamp(0.0, 1.0),
        _ => {}
    }
}

/// The pad thread: an analog trigger changed.
pub fn note_trigger(pad: usize, button: Button, value: f32) {
    let mut hub = hub();
    if hub.live.pad != Some(pad) {
        hub.live = Live {
            pad: Some(pad),
            ..Live::default()
        };
    }
    match button {
        Button::LeftTrigger2 => hub.live.l2 = value.clamp(0.0, 1.0),
        Button::RightTrigger2 => hub.live.r2 = value.clamp(0.0, 1.0),
        _ => {}
    }
}

/// How a bus the system names becomes a [`Link`].
#[cfg(any(windows, target_os = "linux"))]
fn link_of(bus: hidapi::BusType) -> Link {
    match bus {
        hidapi::BusType::Usb => Link::Usb,
        hidapi::BusType::Bluetooth => Link::Bluetooth,
        _ => Link::Unknown,
    }
}

/// The links of the HID devices by vendor and product id, as the system lists them.
#[cfg(any(windows, target_os = "linux"))]
pub fn hid_links() -> Vec<(u16, u16, Link)> {
    let Ok(api) = hidapi::HidApi::new() else {
        return Vec::new();
    };
    api.device_list()
        .map(|d| (d.vendor_id(), d.product_id(), link_of(d.bus_type())))
        .collect()
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn hid_links() -> Vec<(u16, u16, Link)> {
    Vec::new()
}

/// The link of a pad: what the system's HID list says when the pad is listed once, else the
/// power state (a pad on a cable reports `Wired`).
pub fn link_for(
    vendor: Option<u16>,
    product: Option<u16>,
    wired: bool,
    links: &[(u16, u16, Link)],
) -> Link {
    if let (Some(v), Some(p)) = (vendor, product) {
        let mut found = links
            .iter()
            .filter(|(lv, lp, link)| *lv == v && *lp == p && *link != Link::Unknown)
            .map(|(_, _, link)| *link);
        if let Some(first) = found.next()
            && found.all(|l| l == first)
        {
            return first;
        }
    }
    if wired { Link::Usb } else { Link::Unknown }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_button_has_one_name_and_a_bit() {
        assert!(BUTTONS.len() <= 32);
        for (i, (name, button)) in BUTTONS.iter().enumerate() {
            assert_eq!(button_name(*button), Some(*name));
            assert_eq!(button_from_name(name), Some(*button));
            assert_eq!(index_of(name), Some(i));
        }
        assert_eq!(button_name(Button::Unknown), None);
    }

    #[test]
    fn a_pad_listed_once_has_the_link_the_system_names() {
        let links = [
            (0x054C, 0x0CE6, Link::Bluetooth),
            (0x045E, 0x028E, Link::Usb),
        ];
        assert_eq!(
            link_for(Some(0x054C), Some(0x0CE6), false, &links),
            Link::Bluetooth
        );
        assert_eq!(
            link_for(Some(0x045E), Some(0x028E), false, &links),
            Link::Usb
        );
    }

    #[test]
    fn two_listings_that_disagree_fall_back_to_the_power_state() {
        let links = [
            (0x054C, 0x0CE6, Link::Bluetooth),
            (0x054C, 0x0CE6, Link::Usb),
        ];
        assert_eq!(
            link_for(Some(0x054C), Some(0x0CE6), true, &links),
            Link::Usb
        );
        assert_eq!(
            link_for(Some(0x054C), Some(0x0CE6), false, &links),
            Link::Unknown
        );
        assert_eq!(link_for(None, None, false, &links), Link::Unknown);
    }

    #[test]
    fn live_state_follows_the_pad_that_spoke_last() {
        let mut live = Live::default();
        live.down |= 1 << index_of("South").unwrap();
        assert!(live.is_down("South"));
        assert!(!live.is_down("East"));
        assert!(!live.is_down("Nope"));
    }
}

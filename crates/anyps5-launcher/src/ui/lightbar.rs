//! The light bar colour of a DualSense or a DualShock 4 (docs/DESIGN.md 6.10), set from the
//! Controllers page.
//!
//! This is the one place the launcher writes to a pad's HID output, and only because the player
//! asked on that page, only for a DualSense or a DualShock 4, and only while no game runs: a game owns the pad
//! while it is in front, and the next report it sends replaces the colour anyway. The caller
//! checks "no game"; [`set`] does the writing. The report layouts are generic device facts from
//! the public sources of the Linux kernel (`drivers/hid/hid-playstation.c`, the
//! `dualsense_output_report_*` structs and `dualsense_output_worker`) and SDL
//! (`src/joystick/hidapi/SDL_hidapi_ps5.c`, `DS5EffectsState_t`).
//!
//! The common part of the output report is 47 bytes: `valid_flag0` (0), `valid_flag1` (1),
//! rumble and audio fields up to `valid_flag2` (38), `lightbar_setup` (41) and the colour at 44,
//! 45 and 46. `valid_flag1` bit 2 says the colour is meant; `valid_flag2` bit 1 with
//! `lightbar_setup` 2 turns off the blue fade-out a pad shows after it has been paired. Only the
//! lightbar bits are set, so rumble and the player lamps are left as they are.
//!
//! | Link | Report id | Length | Common part starts at |
//! |---|---|---|---|
//! | USB | 0x02 | 63 | 1 |
//! | Bluetooth | 0x31 | 78 | 3 (sequence in the high nibble of byte 1, tag 0x10 at byte 2, CRC-32 in the last four) |
//!
//! The Bluetooth CRC is CRC-32 over a 0xA2 byte and the report without its last four bytes.
//!
//! A DualShock 4 (kernel `dualshock4_output_report_*`, SDL `SDL_hidapi_ps4.c`) has a shorter
//! common part of 10 bytes: `valid_flag0` (0), `valid_flag1` (1), a reserved byte, the two rumble
//! motors (3, 4) and the colour at 5, 6 and 7. `valid_flag0` bit 1 says the colour is meant; the
//! rumble motors stay off the report's valid flags, so a game's rumble is not touched. There is no
//! pairing fade-out to switch off.
//!
//! | Link | Report id | Length | Common part starts at |
//! |---|---|---|---|
//! | USB | 0x05 | 32 | 1 |
//! | Bluetooth | 0x11 | 78 | 3 (0xC0 at byte 1: HID output plus CRC, 0 at byte 2, CRC-32 in the last four) |

#![cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]

use crate::ui::pad_hub::Link;
use crate::ui::ps_button::crc32;

const USB_ID: u8 = 0x02;
const USB_LEN: usize = 63;
const BT_ID: u8 = 0x31;
const BT_LEN: usize = 78;
const BT_TAG: u8 = 0x10;
const BT_CRC_SEED: u8 = 0xA2;

/// Offsets inside the common part.
const FLAG1: usize = 1;
const FLAG2: usize = 38;
const SETUP: usize = 41;
const RED: usize = 44;

const FLAG1_LIGHTBAR: u8 = 1 << 2;

/// The same, for a DualShock 4.
const DS4_USB_ID: u8 = 0x05;
const DS4_USB_LEN: usize = 32;
const DS4_BT_ID: u8 = 0x11;
const DS4_BT_LEN: usize = 78;
const DS4_BT_HW_CONTROL: u8 = 0xC0;
const DS4_FLAG0: usize = 0;
const DS4_RED: usize = 5;
const DS4_FLAG0_LIGHTBAR: u8 = 1 << 1;

/// Sony's USB vendor id.
const SONY: u16 = 0x054C;

/// The two families of pads whose light bar the launcher can set; their output reports differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    DualSense,
    DualShock4,
}

/// The pad a vendor and product id stand for. DualSense 0CE6 and Edge 0DF2; DualShock 4 05C4,
/// 09CC (v2) and 0BA0 (Sony's USB adapter).
pub fn kind_of(vendor: u16, product: u16) -> Option<Kind> {
    match (vendor, product) {
        (SONY, 0x0CE6 | 0x0DF2) => Some(Kind::DualSense),
        (SONY, 0x05C4 | 0x09CC | 0x0BA0) => Some(Kind::DualShock4),
        _ => None,
    }
}
const FLAG2_LIGHTBAR_SETUP: u8 = 1 << 1;
const SETUP_LIGHT_OUT: u8 = 1 << 1;

/// The colours the page offers, as `(name, RRGGBB)`. The first is the pad's own.
pub const COLOURS: [(&str, [u8; 3]); 7] = [
    ("Blue", [0x00, 0x30, 0xFF]),
    ("Red", [0xFF, 0x1A, 0x1A]),
    ("Green", [0x12, 0xD6, 0x4A]),
    ("Purple", [0x9A, 0x3C, 0xFF]),
    ("Orange", [0xFF, 0x7A, 0x00]),
    ("White", [0xFF, 0xFF, 0xFF]),
    ("Off", [0x00, 0x00, 0x00]),
];

/// The most degrees of hue the custom colour has: a whole turn would end where it began.
pub const HUE_MAX: u16 = 355;

/// A colour from hue (degrees, 0 to 360), saturation and value (0 to 1).
pub fn from_hsv(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let h = hue.rem_euclid(360.0) / 60.0;
    let c = value * saturation;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = value - c;
    [r + m, g + m, b + m].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The custom colour for a hue (degrees) and a brightness (percent): the pure hue at that
/// brightness. No custom colour is ever one of the presets.
pub fn custom(hue: u16, brightness: u8) -> [u8; 3] {
    from_hsv(f32::from(hue), 1.0, f32::from(brightness) / 100.0)
}

/// The hue (degrees, in steps of 5) and brightness (percent, in steps of 5, at least 10) a colour
/// starts the custom sliders at: its own hue and brightness. Off starts at full red.
pub fn custom_of(rgb: [u8; 3]) -> (u16, u8) {
    let [r, g, b] = rgb.map(|v| f32::from(v) / 255.0);
    let top = r.max(g).max(b);
    let delta = top - r.min(g).min(b);
    if top <= 0.0 {
        return (0, 100);
    }
    let hue = if delta <= 0.0 {
        0.0
    } else if top == r {
        ((g - b) / delta).rem_euclid(6.0) * 60.0
    } else if top == g {
        ((b - r) / delta + 2.0) * 60.0
    } else {
        ((r - g) / delta + 4.0) * 60.0
    };
    let hue = ((hue / 5.0).round() as u16 * 5).min(HUE_MAX);
    let brightness = ((top * 100.0 / 5.0).round() as u8 * 5).clamp(10, 100);
    (hue, brightness)
}

/// Whether the colour the preferences hold is one of the presets.
pub fn is_preset(stored: Option<&str>) -> bool {
    stored
        .and_then(parse)
        .is_none_or(|rgb| COLOURS.iter().any(|(_, c)| *c == rgb))
}

/// `RRGGBB` as the preferences keep it.
pub fn hex(rgb: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

pub fn parse(text: &str) -> Option<[u8; 3]> {
    if text.len() != 6 || !text.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&text[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// The index in [`COLOURS`] of the colour the preferences hold, or of the pad's own. A colour that
/// is none of them (a custom one) is the entry after the last.
pub fn index_of(stored: Option<&str>) -> usize {
    match stored.and_then(parse) {
        None => 0,
        Some(rgb) => COLOURS
            .iter()
            .position(|(_, c)| *c == rgb)
            .unwrap_or(COLOURS.len()),
    }
}

/// A report that makes the light bar `rgb`, for a pad on `link`. `seq` is the Bluetooth
/// sequence number (0 to 15). With `setup` the report also turns off the pairing fade-out.
pub fn report(link: Link, rgb: [u8; 3], setup: bool, seq: u8) -> Vec<u8> {
    let (mut report, start) = match link {
        Link::Bluetooth => {
            let mut r = vec![0u8; BT_LEN];
            r[0] = BT_ID;
            r[1] = (seq & 0x0F) << 4;
            r[2] = BT_TAG;
            (r, 3)
        }
        _ => {
            let mut r = vec![0u8; USB_LEN];
            r[0] = USB_ID;
            (r, 1)
        }
    };
    if setup {
        report[start + FLAG2] |= FLAG2_LIGHTBAR_SETUP;
        report[start + SETUP] = SETUP_LIGHT_OUT;
    } else {
        report[start + FLAG1] |= FLAG1_LIGHTBAR;
        report[start + RED..start + RED + 3].copy_from_slice(&rgb);
    }
    if link == Link::Bluetooth {
        let split = BT_LEN - 4;
        let crc = crc32(crc32(0, &[BT_CRC_SEED]), &report[..split]);
        report[split..].copy_from_slice(&crc.to_le_bytes());
    }
    report
}

/// A report that makes the light bar of a DualShock 4 `rgb`, for a pad on `link`. Only the
/// colour flag is set, so rumble is left alone.
pub fn report_ds4(link: Link, rgb: [u8; 3]) -> Vec<u8> {
    let (mut report, start) = match link {
        Link::Bluetooth => {
            let mut r = vec![0u8; DS4_BT_LEN];
            r[0] = DS4_BT_ID;
            r[1] = DS4_BT_HW_CONTROL;
            (r, 3)
        }
        _ => {
            let mut r = vec![0u8; DS4_USB_LEN];
            r[0] = DS4_USB_ID;
            (r, 1)
        }
    };
    report[start + DS4_FLAG0] |= DS4_FLAG0_LIGHTBAR;
    report[start + DS4_RED..start + DS4_RED + 3].copy_from_slice(&rgb);
    if link == Link::Bluetooth {
        let split = DS4_BT_LEN - 4;
        let crc = crc32(crc32(0, &[BT_CRC_SEED]), &report[..split]);
        report[split..].copy_from_slice(&crc.to_le_bytes());
    }
    report
}

/// Sets the light bar of every DualSense and DualShock 4 the system lists. Returns how many took it. The
/// caller has checked that no game runs.
#[cfg(any(windows, target_os = "linux"))]
pub fn set(rgb: [u8; 3]) -> Result<usize, String> {
    use hidapi::{BusType, HidApi};
    let api = HidApi::new().map_err(|e| e.to_string())?;
    let mut done = 0;
    let mut last_error = None;
    for info in api.device_list() {
        let Some(kind) = kind_of(info.vendor_id(), info.product_id()) else {
            continue;
        };
        let link = match info.bus_type() {
            BusType::Bluetooth => Link::Bluetooth,
            _ => Link::Usb,
        };
        let device = match info.open_device(&api) {
            Ok(device) => device,
            Err(err) => {
                last_error = Some(err.to_string());
                continue;
            }
        };
        let sent = match kind {
            Kind::DualSense => device
                .write(&report(link, rgb, true, 0))
                .and_then(|_| device.write(&report(link, rgb, false, 1))),
            Kind::DualShock4 => device.write(&report_ds4(link, rgb)),
        };
        match sent {
            Ok(_) => done += 1,
            Err(err) => last_error = Some(err.to_string()),
        }
    }
    match (done, last_error) {
        (0, Some(err)) => Err(err),
        _ => Ok(done),
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn set(_rgb: [u8; 3]) -> Result<usize, String> {
    Err("not supported on this system".to_owned())
}

/// What the writer thread has still to put on the pad: only ever the latest colour.
static LATEST: std::sync::Mutex<Option<[u8; 3]>> = std::sync::Mutex::new(None);
static WRITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// [`set`] without waiting for it: the colour goes to the pads on a thread of its own, and while a
/// write is still going a newer colour replaces an older one that has not been written yet (a
/// slider drag asks for many). The caller has checked that no game runs.
pub fn set_soon(rgb: [u8; 3]) {
    use std::sync::atomic::Ordering;
    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(rgb);
    if WRITING.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("lightbar".into())
        .spawn(|| {
            loop {
                let next = LATEST.lock().unwrap_or_else(|e| e.into_inner()).take();
                match next {
                    Some(rgb) => {
                        if let Err(err) = set(rgb) {
                            tracing::warn!("light bar not set: {err}");
                        }
                    }
                    None => {
                        WRITING.store(false, Ordering::Release);
                        // A colour that arrived after the last look: write it, unless another
                        // thread has taken the job over already.
                        let waiting = LATEST.lock().unwrap_or_else(|e| e.into_inner()).is_some();
                        if !waiting || WRITING.swap(true, Ordering::AcqRel) {
                            break;
                        }
                    }
                }
            }
        });
    if let Err(err) = spawned {
        WRITING.store(false, Ordering::Release);
        tracing::warn!("light bar thread not started: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_usb_report_has_the_colour_where_the_kernel_puts_it() {
        let r = report(Link::Usb, [1, 2, 3], false, 0);
        assert_eq!(r.len(), 63);
        assert_eq!(r[0], 0x02);
        // Common part starts at 1: flag1 at 2, colour at 45, 46 and 47.
        assert_eq!(r[2], 0x04);
        assert_eq!(&r[45..48], [1, 2, 3]);
        // Nothing else is switched on: no rumble, no audio, no player lamps.
        assert_eq!(r[1], 0);
        assert_eq!(r[39], 0);
        assert!(r[48..].iter().all(|b| *b == 0));
    }

    #[test]
    fn the_setup_report_turns_the_fade_out_off_and_sets_no_colour() {
        let r = report(Link::Usb, [9, 9, 9], true, 0);
        assert_eq!(r[1 + 38], 0x02);
        assert_eq!(r[1 + 41], 0x02);
        assert_eq!(r[2], 0);
        assert_eq!(&r[45..48], [0, 0, 0]);
    }

    #[test]
    fn a_bluetooth_report_is_signed_and_numbered() {
        let r = report(Link::Bluetooth, [10, 20, 30], false, 5);
        assert_eq!(r.len(), 78);
        assert_eq!(r[0], 0x31);
        assert_eq!(r[1], 0x50, "sequence in the high nibble");
        assert_eq!(r[2], 0x10);
        // Common part starts at 3: flag1 at 4, colour at 47, 48 and 49.
        assert_eq!(r[4], 0x04);
        assert_eq!(&r[47..50], [10, 20, 30]);
        let crc = u32::from_le_bytes([r[74], r[75], r[76], r[77]]);
        assert_eq!(crc, crc32(crc32(0, &[0xA2]), &r[..74]));
        // A different colour is a different signature.
        let other = report(Link::Bluetooth, [10, 20, 31], false, 5);
        assert_ne!(&r[74..], &other[74..]);
    }

    #[test]
    fn the_pads_are_told_apart_by_their_ids() {
        assert_eq!(kind_of(0x054C, 0x0CE6), Some(Kind::DualSense));
        assert_eq!(kind_of(0x054C, 0x0DF2), Some(Kind::DualSense));
        for product in [0x05C4, 0x09CC, 0x0BA0] {
            assert_eq!(kind_of(0x054C, product), Some(Kind::DualShock4));
        }
        assert_eq!(kind_of(0x045E, 0x09CC), None);
        assert_eq!(kind_of(0x054C, 0x0001), None);
    }

    #[test]
    fn a_dualshock_usb_report_has_the_colour_where_the_kernel_puts_it() {
        let r = report_ds4(Link::Usb, [1, 2, 3]);
        assert_eq!(r.len(), 32);
        assert_eq!(r[0], 0x05);
        // Common part starts at 1: flag0 at 1, colour at 6, 7 and 8.
        assert_eq!(r[1], 0x02);
        assert_eq!(&r[6..9], [1, 2, 3]);
        // No rumble, no flash, nothing else switched on.
        assert_eq!(r[2], 0);
        assert_eq!(&r[4..6], [0, 0]);
        assert!(r[9..].iter().all(|b| *b == 0));
    }

    #[test]
    fn a_dualshock_bluetooth_report_is_signed() {
        let r = report_ds4(Link::Bluetooth, [10, 20, 30]);
        assert_eq!(r.len(), 78);
        assert_eq!(r[0], 0x11);
        assert_eq!(r[1], 0xC0);
        // Common part starts at 3: flag0 at 3, colour at 8, 9 and 10.
        assert_eq!(r[3], 0x02);
        assert_eq!(&r[8..11], [10, 20, 30]);
        let crc = u32::from_le_bytes([r[74], r[75], r[76], r[77]]);
        assert_eq!(crc, crc32(crc32(0, &[0xA2]), &r[..74]));
        let other = report_ds4(Link::Bluetooth, [10, 20, 31]);
        assert_ne!(&r[74..], &other[74..]);
    }

    #[test]
    fn a_custom_colour_comes_back_as_the_sliders_that_made_it() {
        // Every place the hue slider and the brightness slider can stop (the pointer snaps to 5).
        for hue in (0..=HUE_MAX).step_by(5) {
            for brightness in (10..=100u8).step_by(5) {
                let rgb = custom(hue, brightness);
                assert_eq!(custom_of(rgb), (hue, brightness), "{hue} {brightness}");
                // And none of them passes for a preset or for Off.
                assert!(
                    !COLOURS.iter().any(|(_, c)| *c == rgb),
                    "{hue} {brightness} {rgb:?}"
                );
                assert_eq!(index_of(Some(&hex(rgb))), COLOURS.len());
                assert!(!is_preset(Some(&hex(rgb))));
            }
        }
    }

    #[test]
    fn the_pure_hues_are_where_they_should_be() {
        assert_eq!(from_hsv(0.0, 1.0, 1.0), [255, 0, 0]);
        assert_eq!(from_hsv(120.0, 1.0, 1.0), [0, 255, 0]);
        assert_eq!(from_hsv(240.0, 1.0, 0.5), [0, 0, 128]);
        assert_eq!(custom_of([0, 0, 0]), (0, 100), "Off starts at red");
    }

    #[test]
    fn colours_round_trip_through_the_preferences() {
        for (i, (_, rgb)) in COLOURS.iter().enumerate() {
            assert_eq!(parse(&hex(*rgb)), Some(*rgb));
            assert_eq!(index_of(Some(&hex(*rgb))), i);
        }
        assert_eq!(index_of(None), 0);
        assert_eq!(index_of(Some("nonsense")), 0);
        assert!(is_preset(None));
        assert_eq!(parse("12345"), None);
        assert_eq!(parse("GGGGGG"), None);
    }
}

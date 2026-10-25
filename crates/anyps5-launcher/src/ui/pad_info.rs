//! What the launcher knows about the controller in use: which family it is (the hints name its
//! buttons), whether one is connected and how full its battery is (the top bar shows it).
//!
//! The pad thread writes ([`set_active`], [`set_battery`], [`clear`]); the UI reads. Everything is
//! an atomic, so neither side waits for the other.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// The make of a pad, as far as its buttons are named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// DualSense, DualShock: Cross, Circle, Square, Triangle.
    PlayStation,
    /// Xbox pads and the virtual Xbox pad of Steam Input and DS4Windows: A, B, X, Y.
    Xbox,
    /// Switch pads: the same names as Xbox, but A and B (and X and Y) swap places.
    Nintendo,
    /// Anything else (8BitDo and the like): named like an Xbox pad.
    Other,
}

const SONY: u16 = 0x054C;
const MICROSOFT: u16 = 0x045E;
const NINTENDO: u16 = 0x057E;

impl Family {
    /// The family of a pad from its USB ids and the name the system gives it.
    pub fn classify(vendor: Option<u16>, name: &str) -> Family {
        let lower = name.to_ascii_lowercase();
        match vendor {
            Some(SONY) => return Family::PlayStation,
            Some(MICROSOFT) => return Family::Xbox,
            Some(NINTENDO) => return Family::Nintendo,
            _ => {}
        }
        if ["dualsense", "dualshock", "ps5 ", "ps4 ", "playstation"]
            .iter()
            .any(|n| lower.contains(n))
        {
            Family::PlayStation
        } else if lower.contains("xbox") || lower.contains("xinput") {
            Family::Xbox
        } else if lower.contains("switch") || lower.contains("joy-con") {
            Family::Nintendo
        } else {
            Family::Other
        }
    }

    /// The letter that stands for a PlayStation face button on this family (`None` where the
    /// hints keep the PlayStation shape). Cross is the bottom button, Circle the right one,
    /// Square the left one and Triangle the top one.
    pub fn face_letter(self, glyph: char) -> Option<char> {
        let (bottom, right, left, top) = match self {
            Family::PlayStation => return None,
            Family::Xbox | Family::Other => ('A', 'B', 'X', 'Y'),
            Family::Nintendo => ('B', 'A', 'Y', 'X'),
        };
        match glyph {
            '✕' => Some(bottom),
            '○' => Some(right),
            '□' => Some(left),
            '△' => Some(top),
            _ => None,
        }
    }

    fn code(self) -> u8 {
        match self {
            Family::PlayStation => 0,
            Family::Xbox => 1,
            Family::Nintendo => 2,
            Family::Other => 3,
        }
    }

    fn from_code(code: u8) -> Family {
        match code {
            1 => Family::Xbox,
            2 => Family::Nintendo,
            3 => Family::Other,
            _ => Family::PlayStation,
        }
    }
}

/// What a battery reading says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Battery {
    /// Nothing is known, or the pad has no battery of its own.
    Unknown,
    /// Percent full, and whether it is charging.
    Level { percent: u8, charging: bool },
}

const UNKNOWN: u8 = 255;

static CONNECTED: AtomicBool = AtomicBool::new(false);
static FAMILY: AtomicU8 = AtomicU8::new(0);
static PERCENT: AtomicU8 = AtomicU8::new(UNKNOWN);
static CHARGING: AtomicBool = AtomicBool::new(false);

/// A pad is connected and is the one in use.
pub fn set_active(family: Family) {
    FAMILY.store(family.code(), Ordering::Relaxed);
    CONNECTED.store(true, Ordering::Relaxed);
}

/// No pad is connected.
pub fn clear() {
    CONNECTED.store(false, Ordering::Relaxed);
    PERCENT.store(UNKNOWN, Ordering::Relaxed);
}

pub fn set_battery(battery: Battery) {
    match battery {
        Battery::Unknown => PERCENT.store(UNKNOWN, Ordering::Relaxed),
        Battery::Level { percent, charging } => {
            CHARGING.store(charging, Ordering::Relaxed);
            PERCENT.store(percent.min(100), Ordering::Relaxed);
        }
    }
}

pub fn connected() -> bool {
    CONNECTED.load(Ordering::Relaxed)
}

/// The family of the pad in use; PlayStation until one was seen.
pub fn family() -> Family {
    Family::from_code(FAMILY.load(Ordering::Relaxed))
}

pub fn battery() -> Battery {
    match PERCENT.load(Ordering::Relaxed) {
        UNKNOWN => Battery::Unknown,
        percent => Battery::Level {
            percent,
            charging: CHARGING.load(Ordering::Relaxed),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_are_told_apart_by_vendor_then_by_name() {
        assert_eq!(
            Family::classify(Some(0x054C), "PS5 Controller"),
            Family::PlayStation
        );
        assert_eq!(
            Family::classify(Some(0x045E), "Xbox Wireless Controller"),
            Family::Xbox
        );
        assert_eq!(
            Family::classify(Some(0x057E), "Pro Controller"),
            Family::Nintendo
        );
        // Steam Input and DS4Windows hand out a virtual Xbox 360 pad.
        assert_eq!(
            Family::classify(Some(0x045E), "Controller (XBOX 360 For Windows)"),
            Family::Xbox
        );
        // No ids (some backends): the name decides.
        assert_eq!(
            Family::classify(None, "Wireless Controller DualShock 4"),
            Family::PlayStation
        );
        assert_eq!(
            Family::classify(Some(0x2DC8), "8BitDo Pro 2"),
            Family::Other
        );
        assert_eq!(Family::classify(None, "Xbox Controller"), Family::Xbox);
    }

    #[test]
    fn hints_name_the_buttons_of_the_pad_in_hand() {
        assert_eq!(Family::PlayStation.face_letter('✕'), None);
        assert_eq!(Family::Xbox.face_letter('✕'), Some('A'));
        assert_eq!(Family::Xbox.face_letter('○'), Some('B'));
        assert_eq!(Family::Xbox.face_letter('△'), Some('Y'));
        assert_eq!(Family::Xbox.face_letter('□'), Some('X'));
        // A Switch pad has its A on the right and its B at the bottom.
        assert_eq!(Family::Nintendo.face_letter('✕'), Some('B'));
        assert_eq!(Family::Nintendo.face_letter('○'), Some('A'));
        assert_eq!(Family::Xbox.face_letter('x'), None);
    }

    #[test]
    fn the_state_round_trips() {
        clear();
        assert!(!connected());
        set_active(Family::Nintendo);
        assert!(connected());
        assert_eq!(family(), Family::Nintendo);
        set_battery(Battery::Level {
            percent: 140,
            charging: true,
        });
        assert_eq!(
            battery(),
            Battery::Level {
                percent: 100,
                charging: true
            }
        );
        clear();
        assert_eq!(battery(), Battery::Unknown);
        set_active(Family::PlayStation);
        clear();
    }
}

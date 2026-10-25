//! A virtual Xbox-360-style gamepad through `/dev/uinput`, written with `libc` only.
//!
//! The device claims the identity of a wired Xbox 360 controller (vendor 0x045e, product 0x028e)
//! so SDL and games pick the mapping they already know. The axes follow the `xpad` driver: sticks
//! -32768..32767 with up negative, triggers 0..255, the d-pad on `ABS_HAT0X/Y`.
//! No rumble yet: that needs the force-feedback upload handshake on the uinput fd.

use std::{
    fs::{File, OpenOptions},
    io::{self, ErrorKind, Write},
    mem,
    os::fd::AsRawFd,
};

use super::{Backend, Feedback, PadState, VirtualPadError, VirtualPadStatus, xbutton};

const DEVICE: &str = "/dev/uinput";
const NAME: &str = "AnyPlay Phone Controller";

// Event types and codes of <linux/input-event-codes.h>.
const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;
const SYN_REPORT: u16 = 0;
const BTN_SOUTH: u16 = 0x130;
const BTN_EAST: u16 = 0x131;
const BTN_NORTH: u16 = 0x133;
const BTN_WEST: u16 = 0x134;
const BTN_TL: u16 = 0x136;
const BTN_TR: u16 = 0x137;
const BTN_SELECT: u16 = 0x13a;
const BTN_START: u16 = 0x13b;
const BTN_MODE: u16 = 0x13c;
const BTN_THUMBL: u16 = 0x13d;
const BTN_THUMBR: u16 = 0x13e;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_Z: u16 = 0x02;
const ABS_RX: u16 = 0x03;
const ABS_RY: u16 = 0x04;
const ABS_RZ: u16 = 0x05;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;
const BUS_USB: u16 = 0x03;

// ioctl numbers of <linux/uinput.h>, built like the kernel's _IOC (x86, ARM, RISC-V layout).
const fn ioc(dir: u32, nr: u32, size: usize) -> libc::c_ulong {
    ((dir << 30) | ((size as u32) << 16) | ((b'U' as u32) << 8) | nr) as libc::c_ulong
}
const IOC_NONE: u32 = 0;
const IOC_WRITE: u32 = 1;
const UI_DEV_CREATE: libc::c_ulong = ioc(IOC_NONE, 1, 0);
const UI_DEV_DESTROY: libc::c_ulong = ioc(IOC_NONE, 2, 0);
const UI_DEV_SETUP: libc::c_ulong = ioc(IOC_WRITE, 3, mem::size_of::<libc::uinput_setup>());
const UI_ABS_SETUP: libc::c_ulong = ioc(IOC_WRITE, 4, mem::size_of::<libc::uinput_abs_setup>());
const UI_SET_EVBIT: libc::c_ulong = ioc(IOC_WRITE, 100, mem::size_of::<libc::c_int>());
const UI_SET_KEYBIT: libc::c_ulong = ioc(IOC_WRITE, 101, mem::size_of::<libc::c_int>());
const UI_SET_ABSBIT: libc::c_ulong = ioc(IOC_WRITE, 103, mem::size_of::<libc::c_int>());

pub fn status() -> VirtualPadStatus {
    match OpenOptions::new().write(true).open(DEVICE) {
        Ok(_) => VirtualPadStatus::Ready,
        Err(e) if e.kind() == ErrorKind::NotFound => VirtualPadStatus::DriverMissing,
        Err(e) => VirtualPadStatus::NoAccess(e.to_string()),
    }
}

pub fn open(_feedback: Option<Feedback>) -> Result<Box<dyn Backend>, VirtualPadError> {
    let file = OpenOptions::new()
        .write(true)
        .open(DEVICE)
        .map_err(|e| VirtualPadError::Driver(format!("{DEVICE}: {e}")))?;
    Ok(Box::new(Uinput {
        file,
        created: false,
    }))
}

struct Uinput {
    file: File,
    created: bool,
}

fn check(result: libc::c_int) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn ioctl_int(file: &File, request: libc::c_ulong, value: libc::c_int) -> io::Result<()> {
    // SAFETY: the fd is open for the whole call, and the uinput requests used with this helper
    // take their argument by value (an int).
    check(unsafe { libc::ioctl(file.as_raw_fd(), request, value) })
}

fn ioctl_ptr<T>(file: &File, request: libc::c_ulong, value: &T) -> io::Result<()> {
    // SAFETY: the fd is open for the whole call, `value` is a live, fully initialised repr(C)
    // struct of the size encoded in `request`, and the kernel only reads from it.
    check(unsafe { libc::ioctl(file.as_raw_fd(), request, std::ptr::from_ref(value)) })
}

fn ioctl_none(file: &File, request: libc::c_ulong) -> io::Result<()> {
    // SAFETY: the fd is open for the whole call; these requests take no argument.
    check(unsafe { libc::ioctl(file.as_raw_fd(), request) })
}

const KEYS: [u16; 11] = [
    BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_WEST, BTN_TL, BTN_TR, BTN_SELECT, BTN_START, BTN_MODE,
    BTN_THUMBL, BTN_THUMBR,
];

/// The axes with (code, minimum, maximum, fuzz, flat), as `xpad` declares them.
const AXES: [(u16, i32, i32, i32, i32); 8] = [
    (ABS_X, -32768, 32767, 16, 128),
    (ABS_Y, -32768, 32767, 16, 128),
    (ABS_RX, -32768, 32767, 16, 128),
    (ABS_RY, -32768, 32767, 16, 128),
    (ABS_Z, 0, 255, 0, 0),
    (ABS_RZ, 0, 255, 0, 0),
    (ABS_HAT0X, -1, 1, 0, 0),
    (ABS_HAT0Y, -1, 1, 0, 0),
];

impl Uinput {
    fn create(&self) -> io::Result<()> {
        let f = &self.file;
        ioctl_int(f, UI_SET_EVBIT, EV_KEY.into())?;
        for key in KEYS {
            ioctl_int(f, UI_SET_KEYBIT, key.into())?;
        }
        ioctl_int(f, UI_SET_EVBIT, EV_ABS.into())?;
        for (code, min, max, fuzz, flat) in AXES {
            ioctl_int(f, UI_SET_ABSBIT, code.into())?;
            // SAFETY: plain integers; all-zero is a valid value.
            let mut setup: libc::uinput_abs_setup = unsafe { mem::zeroed() };
            setup.code = code;
            setup.absinfo.minimum = min;
            setup.absinfo.maximum = max;
            setup.absinfo.fuzz = fuzz;
            setup.absinfo.flat = flat;
            ioctl_ptr(f, UI_ABS_SETUP, &setup)?;
        }
        // SAFETY: plain integers and a byte array; all-zero is valid.
        let mut setup: libc::uinput_setup = unsafe { mem::zeroed() };
        setup.id = libc::input_id {
            bustype: BUS_USB,
            vendor: 0x045e,
            product: 0x028e,
            version: 0x0110,
        };
        for (slot, byte) in setup.name.iter_mut().zip(NAME.bytes()) {
            *slot = byte as libc::c_char;
        }
        ioctl_ptr(f, UI_DEV_SETUP, &setup)?;
        ioctl_none(f, UI_DEV_CREATE)
    }

    fn emit(&mut self, events: &[(u16, u16, i32)]) -> io::Result<()> {
        let mut bytes = Vec::new();
        for &(kind, code, value) in events.iter().chain(&[(EV_SYN, SYN_REPORT, 0)]) {
            bytes.extend_from_slice(&encode_event(kind, code, value));
        }
        // One write: the kernel takes the whole report.
        self.file.write_all(&bytes)
    }
}

/// One `struct input_event` as bytes; the kernel stamps the time itself.
fn encode_event(kind: u16, code: u16, value: i32) -> Vec<u8> {
    // SAFETY: input_event is plain integers; all-zero is valid.
    let mut event: libc::input_event = unsafe { mem::zeroed() };
    event.type_ = kind;
    event.code = code;
    event.value = value;
    // SAFETY: reading the bytes of a live, initialised repr(C) struct of plain integers (any
    // padding was zeroed above).
    unsafe {
        std::slice::from_raw_parts(
            std::ptr::from_ref(&event).cast::<u8>(),
            mem::size_of::<libc::input_event>(),
        )
    }
    .to_vec()
}

/// The full report for a state: every key and axis, so the first one after a plug-in sets all.
fn events_for(state: &PadState) -> Vec<(u16, u16, i32)> {
    let b = state.buttons;
    let key = |code: u16, bit: u16| (EV_KEY, code, i32::from(b & bit != 0));
    let hat = |minus: u16, plus: u16| i32::from(b & plus != 0) - i32::from(b & minus != 0);
    vec![
        key(BTN_SOUTH, xbutton::A),
        key(BTN_EAST, xbutton::B),
        key(BTN_NORTH, xbutton::X),
        key(BTN_WEST, xbutton::Y),
        key(BTN_TL, xbutton::LB),
        key(BTN_TR, xbutton::RB),
        key(BTN_SELECT, xbutton::BACK),
        key(BTN_START, xbutton::START),
        key(BTN_THUMBL, xbutton::L3),
        key(BTN_THUMBR, xbutton::R3),
        (EV_ABS, ABS_X, state.lx.into()),
        // evdev has down positive, like xpad: the state's up-positive axes flip.
        (EV_ABS, ABS_Y, i32::from(state.ly.saturating_neg())),
        (EV_ABS, ABS_RX, state.rx.into()),
        (EV_ABS, ABS_RY, i32::from(state.ry.saturating_neg())),
        (EV_ABS, ABS_Z, state.left_trigger.into()),
        (EV_ABS, ABS_RZ, state.right_trigger.into()),
        (EV_ABS, ABS_HAT0X, hat(xbutton::LEFT, xbutton::RIGHT)),
        (EV_ABS, ABS_HAT0Y, hat(xbutton::UP, xbutton::DOWN)),
    ]
}

impl Backend for Uinput {
    fn plug(&mut self) -> Result<(), String> {
        self.create().map_err(|e| format!("{DEVICE}: {e}"))?;
        self.created = true;
        Ok(())
    }

    fn update(&mut self, state: &PadState) -> Result<(), String> {
        self.emit(&events_for(state)).map_err(|e| e.to_string())
    }

    fn unplug(&mut self) {
        if mem::take(&mut self.created) {
            let _ = ioctl_none(&self.file, UI_DEV_DESTROY);
        }
    }
}

impl Drop for Uinput {
    fn drop(&mut self) {
        self.unplug();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_linux_uinput_h() {
        assert_eq!(UI_DEV_CREATE, 0x5501);
        assert_eq!(UI_DEV_DESTROY, 0x5502);
        assert_eq!(UI_DEV_SETUP, 0x405c_5503);
        assert_eq!(UI_ABS_SETUP, 0x401c_5504);
        assert_eq!(UI_SET_EVBIT, 0x4004_5564);
        assert_eq!(UI_SET_KEYBIT, 0x4004_5565);
        assert_eq!(UI_SET_ABSBIT, 0x4004_5567);
    }

    #[test]
    fn an_event_is_the_kernels_layout() {
        let bytes = encode_event(EV_ABS, ABS_RZ, -2);
        let size = mem::size_of::<libc::input_event>();
        assert_eq!(bytes.len(), size);
        let tail = &bytes[size - 8..];
        assert_eq!(&tail[0..2], &EV_ABS.to_ne_bytes());
        assert_eq!(&tail[2..4], &ABS_RZ.to_ne_bytes());
        assert_eq!(&tail[4..8], &(-2i32).to_ne_bytes());
    }

    #[test]
    fn a_state_becomes_keys_axes_and_hat() {
        let state = PadState {
            buttons: xbutton::A | xbutton::LB | xbutton::UP | xbutton::RIGHT,
            left_trigger: 9,
            right_trigger: 255,
            lx: -5,
            ly: 32767,
            rx: 7,
            ry: -32768,
        };
        let events = events_for(&state);
        let get = |kind: u16, code: u16| {
            events
                .iter()
                .find(|e| e.0 == kind && e.1 == code)
                .unwrap()
                .2
        };
        assert_eq!(get(EV_KEY, BTN_SOUTH), 1);
        assert_eq!(get(EV_KEY, BTN_TL), 1);
        assert_eq!(get(EV_KEY, BTN_EAST), 0);
        assert_eq!(get(EV_ABS, ABS_X), -5);
        assert_eq!(get(EV_ABS, ABS_Y), -32767);
        assert_eq!(get(EV_ABS, ABS_RY), 32767);
        assert_eq!(get(EV_ABS, ABS_Z), 9);
        assert_eq!(get(EV_ABS, ABS_RZ), 255);
        assert_eq!(get(EV_ABS, ABS_HAT0X), 1);
        assert_eq!(get(EV_ABS, ABS_HAT0Y), -1);
        assert_eq!(events.len(), 18);
    }

    #[test]
    fn struct_sizes_match_the_encoded_ioctl_sizes() {
        assert_eq!(mem::size_of::<libc::uinput_setup>(), 92);
        assert_eq!(mem::size_of::<libc::uinput_abs_setup>(), 28);
    }

    /// Needs write access to /dev/uinput: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn a_device_can_be_created_and_fed() {
        use crate::remote::{InputPacket, PadSink, button};
        let pad = super::super::VirtualPad::open(None).expect("/dev/uinput is usable");
        let mut packet = InputPacket::neutral(1);
        packet.buttons = button::CROSS;
        pad.input(&packet);
        pad.ended();
    }
}

//! The PS button of a Sony pad, read from the HID reports while a game runs (docs/DESIGN.md 6.6).
//!
//! Windows.Gaming.Input hands a pad to the focused window only, so with a game in front gilrs
//! never sees a DualSense or DualShock 4 press its PS button. This module opens those pads
//! through `hidapi`, without exclusive access and only reading (hidapi opens a read/write handle;
//! nothing is ever written), and watches one bit of their input reports. Its whole
//! interface is [`spawn`]: it presses the same [`HomeButton`] the Guide button of an Xbox pad
//! presses (which debounces presses that several sources see at once).
//!
//! It reads only while a game runs and the preference is on, closes every device otherwise, and
//! never writes a report: rumble and light bar belong to the game. A pad that cannot be opened
//! (Linux without read access to its hidraw node, a pad another tool holds exclusively) is
//! logged once and left to the other sources.
//!
//! Report layouts are generic device facts, taken from the public sources of the Linux kernel
//! (`drivers/hid/hid-playstation.c`) and SDL (`src/joystick/hidapi/SDL_hidapi_ps5.c`,
//! `SDL_hidapi_ps4.c`). The PS button is bit 0 of the third button byte of the common part of a
//! report; only where that part starts differs:
//!
//! | Pad and link | Report id | Length | Third button byte at |
//! |---|---|---|---|
//! | DualSense, USB | 0x01 | 64 | 10 |
//! | DualSense, Bluetooth | 0x31 | 78 | 11 (CRC-32 in the last four bytes) |
//! | DualShock 4, USB | 0x01 | 64 (or more) | 7 |
//! | DualShock 4, Bluetooth | 0x11 to 0x19 | 78 or more | 9 (first byte after the id has 0x80 set; CRC-32) |
//! | either, Bluetooth before it is switched to the full report | 0x01 | 10 | 7 |
//!
//! The report id is the first byte of what `hidapi` returns on Windows and Linux, for these pads.

#![cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::ui::home_button::HomeButton;

/// Sony's USB vendor id.
const SONY: u16 = 0x054C;

/// The two families that differ in their report layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Model {
    DualSense,
    DualShock4,
}

/// The pad a vendor and product id stand for. DualSense 0CE6 and Edge 0DF2; DualShock 4 05C4,
/// 09CC (v2) and 0BA0 (Sony's USB adapter).
fn model_of(vendor: u16, product: u16) -> Option<Model> {
    match (vendor, product) {
        (SONY, 0x0CE6 | 0x0DF2) => Some(Model::DualSense),
        (SONY, 0x05C4 | 0x09CC | 0x0BA0) => Some(Model::DualShock4),
        _ => None,
    }
}

/// CRC-32 (zlib) of the bytes, continuing from `seed`.
fn crc32(seed: u32, bytes: &[u8]) -> u32 {
    let mut crc = !seed;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

/// A Bluetooth report ends in a CRC-32 over a 0xA1 byte (the HID input header) and the rest.
fn crc_ok(report: &[u8]) -> bool {
    let Some(split) = report.len().checked_sub(4) else {
        return false;
    };
    let (body, tail) = report.split_at(split);
    let stored = u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]);
    crc32(crc32(0, &[0xA1]), body) == stored
}

/// Whether the PS button is down in this input report; `None` for a report that is not one of
/// the layouts above (an output echo, a different mode, a damaged Bluetooth packet).
fn ps_down(model: Model, report: &[u8]) -> Option<bool> {
    let at = match (model, report.first().copied()?, report.len()) {
        // The short Bluetooth report both pads send until they are switched to the full one.
        (_, 0x01, 10) => 7,
        (Model::DualSense, 0x01, 64) => 10,
        (Model::DualShock4, 0x01, 64..) => 7,
        (Model::DualSense, 0x31, 78) if crc_ok(report) => 11,
        (Model::DualShock4, 0x11..=0x19, 78..)
            if report[1] & 0x80 != 0 && crc_ok(&report[..78]) =>
        {
            9
        }
        _ => return None,
    };
    Some(report[at] & 0x01 != 0)
}

// ---------------------------------------------------------------------------------------------
// The watcher: devices, edges and what is logged. It knows nothing of hidapi.
// ---------------------------------------------------------------------------------------------

/// One open pad, read without waiting.
trait Reader {
    /// The next report into `buf`: `Ok(0)` when there is none, `Err` when the pad is gone.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, String>;
}

/// A pad found by a scan and opened.
struct Found {
    key: String,
    model: Model,
    reader: Box<dyn Reader>,
}

/// What a scan saw: every matching pad by key, the new ones that opened, and the ones that did not.
#[derive(Default)]
struct Scan {
    present: Vec<String>,
    opened: Vec<Found>,
    failed: Vec<(String, String)>,
}

/// Finds Sony pads. `open` lists the keys already open, which a scan leaves alone.
trait Backend {
    fn scan(&mut self, open: &[String]) -> Result<Scan, String>;
}

/// How often the list of pads is looked at again while a game runs.
const RESCAN: Duration = Duration::from_secs(3);
/// The same while a pad is open: it going away shows in its reads, so only a second pad is waited for.
const RESCAN_OPEN: Duration = Duration::from_secs(10);

struct Open {
    key: String,
    model: Model,
    reader: Box<dyn Reader>,
    /// The PS bit of the last report; `None` until the first one, which only sets it, so a button
    /// already held when the pad is opened is not a press.
    down: Option<bool>,
}

struct Watcher<B> {
    backend: B,
    pads: Vec<Open>,
    last_scan: Option<Instant>,
    /// Keys whose failure was logged, so a pad that cannot be opened (or keeps failing) is reported once.
    reported: Vec<String>,
    /// The backend's own failure was logged.
    backend_reported: bool,
}

impl<B: Backend> Watcher<B> {
    fn new(backend: B) -> Self {
        Self {
            backend,
            pads: Vec::new(),
            last_scan: None,
            reported: Vec::new(),
            backend_reported: false,
        }
    }

    /// One round. With `active` off every pad is closed. Returns how many presses (edges of the
    /// PS bit) the reports of this round held.
    fn tick(&mut self, active: bool, now: Instant) -> usize {
        if !active {
            self.pads.clear();
            self.last_scan = None;
            return 0;
        }
        // Often while no pad is open (a plug-in is looked for), seldom once one is.
        let every = if self.pads.is_empty() {
            RESCAN
        } else {
            RESCAN_OPEN
        };
        if self.last_scan.is_none_or(|at| now >= at + every) {
            self.last_scan = Some(now);
            self.rescan();
        }
        let mut presses = 0;
        let mut buf = [0u8; 256];
        let reported = &mut self.reported;
        self.pads.retain_mut(|pad| {
            loop {
                match pad.reader.read(&mut buf) {
                    Ok(0) => break true,
                    Ok(len) => {
                        if let Some(down) = ps_down(pad.model, &buf[..len]) {
                            presses += usize::from(down && pad.down == Some(false));
                            pad.down = Some(down);
                        }
                    }
                    Err(error) => {
                        if !reported.contains(&pad.key) {
                            tracing::info!("PS button reader: {} closed: {error}", pad.key);
                            reported.push(pad.key.clone());
                        }
                        break false;
                    }
                }
            }
        });
        presses
    }

    fn rescan(&mut self) {
        let open: Vec<String> = self.pads.iter().map(|pad| pad.key.clone()).collect();
        let scan = match self.backend.scan(&open) {
            Ok(scan) => scan,
            Err(error) => {
                if !self.backend_reported {
                    self.backend_reported = true;
                    tracing::info!(
                        "PS button reader unavailable ({error}); the PS button is read through gilrs only"
                    );
                }
                return;
            }
        };
        self.backend_reported = false;
        self.pads.retain(|pad| scan.present.contains(&pad.key));
        self.reported.retain(|key| scan.present.contains(key));
        for (key, error) in scan.failed {
            if !self.reported.contains(&key) {
                tracing::info!(
                    "PS button reader: cannot open {key}: {error}; the PS button is read through gilrs only{}",
                    HINT
                );
                self.reported.push(key);
            }
        }
        for found in scan.opened {
            tracing::info!(
                "PS button reader: watching {} ({:?})",
                found.key,
                found.model
            );
            self.pads.push(Open {
                key: found.key,
                model: found.model,
                reader: found.reader,
                down: None,
            });
        }
    }
}

/// Appended to the log line of a pad that did not open.
const HINT: &str = if cfg!(target_os = "linux") {
    " (Linux needs read access to its /dev/hidraw node; see docs/INSTALL.md)"
} else {
    ""
};

// ---------------------------------------------------------------------------------------------
// hidapi
// ---------------------------------------------------------------------------------------------

#[cfg(any(windows, target_os = "linux"))]
mod hid {
    use super::{Backend, Found, Reader, Scan, model_of};

    struct Device(hidapi::HidDevice);

    impl Reader for Device {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
            // No waiting; reports only come in, nothing is ever written.
            self.0.read_timeout(buf, 0).map_err(|e| e.to_string())
        }
    }

    pub struct Hid {
        api: Option<hidapi::HidApi>,
    }

    impl Hid {
        pub fn new() -> Self {
            Self { api: None }
        }
    }

    impl Backend for Hid {
        fn scan(&mut self, open: &[String]) -> Result<Scan, String> {
            let api = match &mut self.api {
                Some(api) => {
                    api.refresh_devices().map_err(|e| e.to_string())?;
                    api
                }
                none => none.insert(hidapi::HidApi::new().map_err(|e| e.to_string())?),
            };
            let mut scan = Scan::default();
            for info in api.device_list() {
                let Some(model) = model_of(info.vendor_id(), info.product_id()) else {
                    continue;
                };
                // The gamepad collection (generic desktop, game pad); hidraw reports no usage.
                if info.usage_page() != 0 && (info.usage_page() != 0x01 || info.usage() != 0x05) {
                    continue;
                }
                let key = info.path().to_string_lossy().into_owned();
                scan.present.push(key.clone());
                if open.contains(&key) {
                    continue;
                }
                match api.open_path(info.path()) {
                    Ok(device) => scan.opened.push(Found {
                        key,
                        model,
                        reader: Box::new(Device(device)),
                    }),
                    Err(error) => scan.failed.push((key, error.to_string())),
                }
            }
            Ok(scan)
        }
    }
}

/// Starts the reader thread. It idles (one look at the state every 250 ms) until a game runs.
/// Other platforms have no reader: macOS reads the pad through IOKit HID, which delivers the
/// PS button to the background.
#[cfg(any(windows, target_os = "linux"))]
pub fn spawn(home: Arc<HomeButton>) {
    let spawned = std::thread::Builder::new()
        .name("ps-button".into())
        .spawn(move || {
            let mut watcher = Watcher::new(hid::Hid::new());
            loop {
                let active = home.game_running() && home.enabled();
                for _ in 0..watcher.tick(active, Instant::now()) {
                    home.press();
                }
                let wait = if active && !watcher.pads.is_empty() {
                    8
                } else {
                    250
                };
                std::thread::sleep(Duration::from_millis(wait));
            }
        });
    if let Err(error) = spawned {
        tracing::warn!("PS button thread not started: {error}");
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn spawn(_home: Arc<HomeButton>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    // ---- synthetic reports ---------------------------------------------------------------

    /// A Bluetooth report with its CRC-32 filled in.
    fn sealed(mut report: Vec<u8>) -> Vec<u8> {
        let end = report.len() - 4;
        let crc = crc32(crc32(0, &[0xA1]), &report[..end]);
        report[end..].copy_from_slice(&crc.to_le_bytes());
        report
    }

    fn dualsense_usb(ps: bool) -> Vec<u8> {
        let mut r = vec![0u8; 64];
        r[0] = 0x01;
        r[8] = 0x08; // d-pad neutral
        r[9] = 0x30; // R2 and L2 shoulder bits: other buttons must not matter
        r[10] = u8::from(ps) | 0x06; // touchpad and mute set
        r
    }

    fn dualsense_bt(ps: bool) -> Vec<u8> {
        let mut r = vec![0u8; 78];
        r[0] = 0x31;
        r[1] = 0x10;
        r[11] = u8::from(ps) | 0x02;
        sealed(r)
    }

    fn dualshock_usb(ps: bool) -> Vec<u8> {
        let mut r = vec![0u8; 64];
        r[0] = 0x01;
        r[5] = 0x88; // d-pad neutral and triangle
        r[7] = u8::from(ps) | 0xFC; // frame counter in the high six bits
        r
    }

    fn dualshock_bt(ps: bool) -> Vec<u8> {
        let mut r = vec![0u8; 78];
        r[0] = 0x11;
        r[1] = 0xC0;
        r[9] = u8::from(ps) | 0x40;
        sealed(r)
    }

    fn short_bt(ps: bool) -> Vec<u8> {
        let mut r = vec![0u8; 10];
        r[0] = 0x01;
        r[7] = u8::from(ps) | 0x02;
        r
    }

    #[test]
    fn crc32_is_the_zlib_one() {
        assert_eq!(crc32(0, b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(crc32(0, b"1234"), b"56789"), 0xCBF4_3926, "continues");
    }

    #[test]
    fn the_ps_bit_is_read_in_every_layout() {
        type Make = fn(bool) -> Vec<u8>;
        let cases: [(Model, Make); 5] = [
            (Model::DualSense, dualsense_usb),
            (Model::DualSense, dualsense_bt),
            (Model::DualShock4, dualshock_usb),
            (Model::DualShock4, dualshock_bt),
            (Model::DualShock4, short_bt),
        ];
        for (model, make) in cases {
            assert_eq!(ps_down(model, &make(true)), Some(true), "{model:?} down");
            assert_eq!(ps_down(model, &make(false)), Some(false), "{model:?} up");
        }
        // The short report is the same for both pads.
        assert_eq!(ps_down(Model::DualSense, &short_bt(true)), Some(true));
    }

    #[test]
    fn a_known_dualsense_usb_report_byte_for_byte() {
        // Idle, sticks centred, d-pad neutral; PS (bit 0) and touchpad (bit 1) of byte 10.
        let mut r = [0u8; 64];
        r[..12].copy_from_slice(&[
            0x01, 0x80, 0x80, 0x80, 0x80, 0x00, 0x00, 0x5A, 0x08, 0x00, 0x01, 0x00,
        ]);
        assert_eq!(ps_down(Model::DualSense, &r), Some(true));
        r[10] = 0x02;
        assert_eq!(
            ps_down(Model::DualSense, &r),
            Some(false),
            "touchpad is not PS"
        );
    }

    #[test]
    fn the_other_pads_layout_is_not_read() {
        // A DualShock 4 USB report is 64 bytes too; the model decides where the bit is.
        let ds4 = dualshock_usb(true);
        assert_eq!(ps_down(Model::DualShock4, &ds4), Some(true));
        assert_eq!(
            ps_down(Model::DualSense, &ds4),
            Some(false),
            "byte 10 is empty there"
        );
    }

    #[test]
    fn damaged_or_foreign_reports_say_nothing() {
        assert_eq!(ps_down(Model::DualSense, &[]), None);
        assert_eq!(ps_down(Model::DualSense, &[0x01; 12]), None, "odd length");
        assert_eq!(ps_down(Model::DualSense, &[0x05; 64]), None, "output echo");
        let mut bad = dualsense_bt(true);
        bad[20] ^= 1;
        assert_eq!(ps_down(Model::DualSense, &bad), None, "CRC");
        let mut bad = dualshock_bt(true);
        bad[40] ^= 1;
        assert_eq!(ps_down(Model::DualShock4, &bad), None, "CRC");
        let mut nodata = dualshock_bt(true);
        nodata[1] = 0x00;
        let nodata = sealed(nodata);
        assert_eq!(
            ps_down(Model::DualShock4, &nodata),
            None,
            "no HID data flag"
        );
        // A report of the Bluetooth id on the wrong pad.
        assert_eq!(ps_down(Model::DualShock4, &dualsense_bt(true)), None);
    }

    #[test]
    fn only_sony_pads_are_taken() {
        assert_eq!(model_of(SONY, 0x0CE6), Some(Model::DualSense));
        assert_eq!(model_of(SONY, 0x0DF2), Some(Model::DualSense));
        assert_eq!(model_of(SONY, 0x05C4), Some(Model::DualShock4));
        assert_eq!(model_of(SONY, 0x09CC), Some(Model::DualShock4));
        assert_eq!(model_of(SONY, 0x0BA0), Some(Model::DualShock4));
        assert_eq!(model_of(SONY, 0x0001), None);
        assert_eq!(model_of(0x045E, 0x0CE6), None);
    }

    // ---- the watcher ---------------------------------------------------------------------

    type Queue = Rc<RefCell<VecDeque<Result<Vec<u8>, String>>>>;

    struct Fake(Queue);

    impl Reader for Fake {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
            match self.0.borrow_mut().pop_front() {
                None => Ok(0),
                Some(Ok(report)) => {
                    buf[..report.len()].copy_from_slice(&report);
                    Ok(report.len())
                }
                Some(Err(error)) => Err(error),
            }
        }
    }

    /// What a scan finds: pads by key, with a queue (opens) or none (cannot be opened).
    type Pads = Result<Vec<(&'static str, Option<Queue>)>, String>;

    /// One scripted answer per scan.
    struct Script {
        scans: VecDeque<Pads>,
        count: usize,
    }

    impl Backend for Script {
        fn scan(&mut self, open: &[String]) -> Result<Scan, String> {
            self.count += 1;
            let pads = self.scans.pop_front().unwrap_or(Ok(Vec::new()))?;
            let mut scan = Scan::default();
            for (key, queue) in pads {
                scan.present.push(key.to_owned());
                if open.iter().any(|k| k == key) {
                    continue;
                }
                match queue {
                    Some(queue) => scan.opened.push(Found {
                        key: key.to_owned(),
                        model: Model::DualSense,
                        reader: Box::new(Fake(queue)),
                    }),
                    None => scan
                        .failed
                        .push((key.to_owned(), "permission denied".into())),
                }
            }
            Ok(scan)
        }
    }

    fn queue(reports: Vec<Vec<u8>>) -> Queue {
        Rc::new(RefCell::new(reports.into_iter().map(Ok).collect()))
    }

    fn watcher(scans: Vec<Pads>) -> Watcher<Script> {
        Watcher::new(Script {
            scans: scans.into(),
            count: 0,
        })
    }

    #[test]
    fn a_held_button_presses_once_and_again_after_release() {
        let q = queue(vec![]);
        let mut w = watcher(vec![Ok(vec![("a", Some(q.clone()))])]);
        let t = Instant::now();
        assert_eq!(w.tick(true, t), 0, "opened, nothing read");
        q.borrow_mut().push_back(Ok(dualsense_usb(false)));
        // Down for ten reports (a held button repeats), up, down again.
        for _ in 0..10 {
            q.borrow_mut().push_back(Ok(dualsense_usb(true)));
        }
        assert_eq!(w.tick(true, t), 1, "edge only");
        q.borrow_mut().push_back(Ok(dualsense_usb(true)));
        assert_eq!(w.tick(true, t), 0, "still held");
        q.borrow_mut().push_back(Ok(dualsense_usb(false)));
        q.borrow_mut().push_back(Ok(dualsense_usb(true)));
        assert_eq!(w.tick(true, t), 1, "a second press");
    }

    #[test]
    fn a_press_and_release_inside_one_round_still_counts() {
        let q = queue(vec![
            dualsense_usb(false),
            dualsense_usb(true),
            dualsense_usb(false),
        ]);
        let mut w = watcher(vec![Ok(vec![("a", Some(q))])]);
        assert_eq!(w.tick(true, Instant::now()), 1);
    }

    #[test]
    fn reports_of_other_kinds_do_not_end_a_press() {
        let q = queue(vec![dualsense_usb(false), dualsense_usb(true)]);
        let mut w = watcher(vec![Ok(vec![("a", Some(q.clone()))])]);
        let t = Instant::now();
        assert_eq!(w.tick(true, t), 1);
        q.borrow_mut().push_back(Ok(vec![0x05; 64]));
        q.borrow_mut().push_back(Ok(dualsense_usb(true)));
        assert_eq!(
            w.tick(true, t),
            0,
            "garbage between reports of a held button"
        );
    }

    #[test]
    fn nothing_is_open_or_read_without_a_game() {
        let q = queue(vec![dualsense_usb(false), dualsense_usb(true)]);
        let mut w = watcher(vec![Ok(vec![("a", Some(q.clone()))])]);
        assert_eq!(w.tick(false, Instant::now()), 0);
        assert_eq!(w.backend.count, 0, "no scan");
        assert_eq!(q.borrow().len(), 2, "no read");
        // The game starts: the press that was waiting is read now, from a fresh state.
        assert_eq!(w.tick(true, Instant::now()), 1);
        // The game ends: every pad is closed, and the next game opens them again.
        assert_eq!(w.tick(false, Instant::now()), 0);
        assert!(w.pads.is_empty());
    }

    #[test]
    fn a_pad_that_cannot_be_opened_falls_back_and_is_reported_once() {
        let mut w = watcher(vec![
            Ok(vec![("a", None)]),
            Ok(vec![("a", None)]),
            Ok(vec![("a", None)]),
        ]);
        let t = Instant::now();
        for round in 0..3 {
            assert_eq!(w.tick(true, t + RESCAN * round), 0);
        }
        assert_eq!(w.backend.count, 3, "it keeps looking");
        assert!(w.pads.is_empty());
        assert_eq!(w.reported, ["a"], "one report, however often it looks");
    }

    #[test]
    fn a_failure_is_reported_again_after_the_pad_was_gone_in_between() {
        let mut w = watcher(vec![
            Ok(vec![("a", None)]),
            Ok(vec![]),
            Ok(vec![("a", None)]),
        ]);
        let t = Instant::now();
        w.tick(true, t);
        assert_eq!(w.reported, ["a"]);
        w.tick(true, t + RESCAN);
        assert!(w.reported.is_empty(), "unplugged");
        w.tick(true, t + RESCAN * 2);
        assert_eq!(w.reported, ["a"]);
    }

    #[test]
    fn a_missing_backend_is_logged_once_and_tried_again() {
        let mut w = watcher(vec![Err("no hid".into()), Err("no hid".into()), Ok(vec![])]);
        let t = Instant::now();
        w.tick(true, t);
        assert!(w.backend_reported);
        w.tick(true, t + RESCAN);
        assert!(w.backend_reported);
        w.tick(true, t + RESCAN * 2);
        assert!(!w.backend_reported, "it works again");
    }

    #[test]
    fn a_pad_that_goes_away_is_dropped_and_found_again() {
        let first = queue(vec![]);
        first.borrow_mut().push_back(Err("unplugged".into()));
        let second = queue(vec![dualsense_usb(false), dualsense_usb(true)]);
        let mut w = watcher(vec![
            Ok(vec![("a", Some(first))]),
            Ok(vec![("a", Some(second))]),
        ]);
        let t = Instant::now();
        assert_eq!(w.tick(true, t), 0);
        assert!(w.pads.is_empty(), "a failed read closes the pad");
        assert_eq!(w.tick(true, t + RESCAN), 1, "found again at the next scan");
    }

    #[test]
    fn two_pads_are_watched_apart() {
        let (a, b) = (queue(vec![]), queue(vec![]));
        let mut w = watcher(vec![Ok(vec![
            ("a", Some(a.clone())),
            ("b", Some(b.clone())),
        ])]);
        let t = Instant::now();
        w.tick(true, t);
        a.borrow_mut().push_back(Ok(dualsense_usb(false)));
        b.borrow_mut().push_back(Ok(dualsense_usb(false)));
        w.tick(true, t);
        a.borrow_mut().push_back(Ok(dualsense_usb(true)));
        b.borrow_mut().push_back(Ok(dualsense_usb(true)));
        assert_eq!(w.tick(true, t), 2, "each pad's own edge");
    }

    #[test]
    fn a_button_already_held_when_the_pad_opens_is_not_a_press() {
        let q = queue(vec![dualsense_usb(true), dualsense_usb(true)]);
        let mut w = watcher(vec![Ok(vec![("a", Some(q.clone()))])]);
        let t = Instant::now();
        assert_eq!(w.tick(true, t), 0);
        q.borrow_mut().push_back(Ok(dualsense_usb(false)));
        q.borrow_mut().push_back(Ok(dualsense_usb(true)));
        assert_eq!(w.tick(true, t), 1, "the next press is one");
    }

    #[test]
    fn a_pad_that_keeps_failing_is_logged_once() {
        let bad = || {
            let q = queue(vec![]);
            q.borrow_mut().push_back(Err("broken".into()));
            q
        };
        let mut w = watcher(vec![
            Ok(vec![("a", Some(bad()))]),
            Ok(vec![("a", Some(bad()))]),
        ]);
        let t = Instant::now();
        w.tick(true, t);
        assert_eq!(w.reported, ["a"]);
        w.tick(true, t + RESCAN);
        assert_eq!(w.reported, ["a"], "not again");
        assert!(w.pads.is_empty());
    }

    #[test]
    fn a_second_pad_is_waited_for_longer_than_a_first() {
        let q = queue(vec![]);
        let mut w = watcher(vec![Ok(vec![("a", Some(q))]), Ok(vec![])]);
        let t = Instant::now();
        w.tick(true, t);
        w.tick(true, t + RESCAN);
        assert_eq!(w.backend.count, 1, "a pad is open");
        w.tick(true, t + RESCAN_OPEN);
        assert_eq!(w.backend.count, 2);
    }

    #[test]
    fn the_lengths_and_ids_of_the_layouts_are_exact() {
        for len in [63, 65, 77, 79] {
            let mut r = vec![0u8; len];
            r[0] = 0x01;
            r[10] = 1;
            assert_eq!(
                ps_down(Model::DualSense, &r),
                None,
                "DualSense 0x01 of {len}"
            );
        }
        // Any of 0x11 to 0x19 is a DualShock 4 Bluetooth report; the larger ones are longer.
        for id in 0x11..=0x19u8 {
            let mut r = vec![0u8; if id > 0x11 { 100 } else { 78 }];
            r[0] = id;
            r[1] = 0x80;
            r[9] = 1;
            let sealed78 = sealed(r[..78].to_vec());
            r[..78].copy_from_slice(&sealed78);
            assert_eq!(ps_down(Model::DualShock4, &r), Some(true), "id {id:#x}");
        }
        assert_eq!(ps_down(Model::DualShock4, &[0x10; 78]), None);
        // A DualShock 4 on a stack that sends more than 64 bytes on report 0x01.
        let mut r = vec![0u8; 128];
        r[0] = 0x01;
        r[7] = 1;
        assert_eq!(ps_down(Model::DualShock4, &r), Some(true));
    }

    #[test]
    fn the_watcher_scans_every_few_seconds_not_every_round() {
        let mut w = watcher(vec![]);
        let t = Instant::now();
        for ms in [0, 8, 16, 1000, 2900] {
            w.tick(true, t + Duration::from_millis(ms));
        }
        assert_eq!(w.backend.count, 1);
        w.tick(true, t + RESCAN);
        assert_eq!(w.backend.count, 2);
    }
}

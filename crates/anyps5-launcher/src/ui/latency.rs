//! How long a controller press takes to reach the interface. The gamepad thread stamps each event
//! it reads ([`pad_event_read`]); the frame that picks the event up calls [`pad_event_seen`] and,
//! when it has built its output, [`frame_done`]. The stamps are kept as a running summary and
//! logged at `debug` level (`RUST_LOG=anyplay_launcher::ui::latency=debug`) every 40 events, so a
//! slow poll or a late repaint shows up with numbers instead of as a feeling.
//!
//! What this cannot see: the pad's own report rate, the operating system's input stack before the
//! thread reads the event, and the display's scan-out after the frame is submitted.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Nanoseconds since the epoch of the oldest unseen pad event, 0 for none.
static PENDING: AtomicU64 = AtomicU64::new(0);
/// Nanoseconds since the epoch at which the UI picked the event up, 0 for none.
static SEEN: AtomicU64 = AtomicU64::new(0);
static SAMPLES: Summary = Summary::new();

fn now_ns() -> u64 {
    // +1 so that "now" is never the "none" value 0.
    epoch().elapsed().as_nanos() as u64 + 1
}

/// The gamepad thread read an event (call before handing it to the UI).
pub fn pad_event_read() {
    // Keep the oldest unseen stamp: a burst is measured from its first event.
    let _ = PENDING.compare_exchange(0, now_ns(), Ordering::AcqRel, Ordering::Acquire);
}

/// The UI took an event off the channel.
pub fn pad_event_seen() {
    let pending = PENDING.load(Ordering::Acquire);
    if pending != 0 {
        SEEN.store(now_ns(), Ordering::Release);
    }
}

/// The frame that took the event has built its output; records the two intervals.
pub fn frame_done() {
    let pending = PENDING.swap(0, Ordering::AcqRel);
    let seen = SEEN.swap(0, Ordering::AcqRel);
    if pending == 0 || seen == 0 || seen < pending {
        return;
    }
    let done = now_ns();
    let wake = Duration::from_nanos(seen - pending);
    let frame = Duration::from_nanos(done.saturating_sub(seen));
    if let Some(line) = SAMPLES.add(wake, frame) {
        tracing::debug!("{line}");
    }
}

/// A running summary of the two intervals.
struct Summary {
    count: AtomicU64,
    wake_sum_us: AtomicU64,
    wake_max_us: AtomicU64,
    frame_sum_us: AtomicU64,
    frame_max_us: AtomicU64,
}

impl Summary {
    const REPORT_EVERY: u64 = 40;

    const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            wake_sum_us: AtomicU64::new(0),
            wake_max_us: AtomicU64::new(0),
            frame_sum_us: AtomicU64::new(0),
            frame_max_us: AtomicU64::new(0),
        }
    }

    /// Adds a sample; every 40th returns the summary line and starts over.
    fn add(&self, wake: Duration, frame: Duration) -> Option<String> {
        let (wake, frame) = (wake.as_micros() as u64, frame.as_micros() as u64);
        self.wake_sum_us.fetch_add(wake, Ordering::Relaxed);
        self.wake_max_us.fetch_max(wake, Ordering::Relaxed);
        self.frame_sum_us.fetch_add(frame, Ordering::Relaxed);
        self.frame_max_us.fetch_max(frame, Ordering::Relaxed);
        let count = self.count.fetch_add(1, Ordering::Relaxed) + 1;
        if count < Self::REPORT_EVERY {
            return None;
        }
        self.count.store(0, Ordering::Relaxed);
        let take = |a: &AtomicU64| a.swap(0, Ordering::Relaxed);
        Some(format!(
            "pad input, {count} events: thread to frame start {:.1} ms avg, {:.1} ms max; frame start to frame built {:.1} ms avg, {:.1} ms max",
            take(&self.wake_sum_us) as f64 / count as f64 / 1000.0,
            take(&self.wake_max_us) as f64 / 1000.0,
            take(&self.frame_sum_us) as f64 / count as f64 / 1000.0,
            take(&self.frame_max_us) as f64 / 1000.0,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_reports_every_fortieth_sample() {
        let s = Summary::new();
        let mut lines = 0;
        for i in 0..80 {
            let wake = Duration::from_micros(1000 + i);
            let line = s.add(wake, Duration::from_micros(500));
            if let Some(line) = line {
                lines += 1;
                assert!(line.contains("40 events"), "{line}");
                assert!(line.contains("ms avg"), "{line}");
            }
        }
        assert_eq!(lines, 2);
    }

    #[test]
    fn a_stamp_travels_from_the_thread_to_the_frame() {
        // The statics are shared with other tests: only check that the round trip clears them.
        pad_event_read();
        std::thread::sleep(Duration::from_millis(2));
        pad_event_seen();
        frame_done();
        assert_eq!(PENDING.load(Ordering::Acquire), 0);
        assert_eq!(SEEN.load(Ordering::Acquire), 0);
    }
}

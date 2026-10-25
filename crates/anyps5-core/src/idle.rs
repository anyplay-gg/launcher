//! Whether the PC has nothing better to do than prepare a game: the gate of the unlocks the
//! launcher starts on its own (`UnlockRequest::background`).
//!
//! [`Meter`] reads the machine (power, free memory, processor load); [`Gate`] turns a [`Reading`]
//! into "go on" or the reason to stand still. The gate is pure and tested; the meter is the thin
//! part that asks the system. A reading the system cannot give is left out, and a missing
//! number never holds anything back.

/// Free memory a prepared game needs before it starts (the relinker maps the whole executable).
pub const START_FREE_RAM: u64 = 4 << 30;
/// A prepared game under way stands still below this: the PC is short of memory for real.
pub const STOP_FREE_RAM: u64 = 400 << 20;
/// ... and goes on again above this.
pub const RESUME_FREE_RAM: u64 = 1 << 30;
/// A prepared game starts only while the processors are at most this busy (0..=1). Its own load
/// is not held against it once it runs.
pub const START_LOAD: f32 = 0.60;

/// What the machine looks like right now.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    /// Running on the battery (a laptop that is not plugged in).
    pub on_battery: bool,
    /// Memory that programs can use at once, in bytes.
    pub free_ram: Option<u64>,
    /// Share of the processors' time spent working since the last reading, 0..=1.
    pub load: Option<f32>,
}

/// Why prepared games stand still.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// A game starts or runs; it gets the whole PC.
    GameActive,
    /// The PC runs on its battery.
    Battery,
    /// Too little memory is free.
    Memory,
    /// The processors are busy with something else.
    Busy,
}

/// Decides, with a little hysteresis, whether prepared games may run.
#[derive(Debug, Default)]
pub struct Gate {
    why: Option<Why>,
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    /// The reason to hold, or `None` to go on. `working`: a prepared game is under way (it
    /// holds memory and uses a processor itself, which must not stop it).
    pub fn update(&mut self, reading: &Reading, game_active: bool, working: bool) -> Option<Why> {
        let memory_short = |below: u64| reading.free_ram.is_some_and(|free| free < below);
        self.why = if game_active {
            Some(Why::GameActive)
        } else if reading.on_battery {
            Some(Why::Battery)
        } else if (!working && memory_short(START_FREE_RAM))
            || (working && self.why == Some(Why::Memory) && memory_short(RESUME_FREE_RAM))
            || (working && memory_short(STOP_FREE_RAM))
        {
            Some(Why::Memory)
        } else if !working && reading.load.is_some_and(|load| load > START_LOAD) {
            Some(Why::Busy)
        } else {
            None
        };
        self.why
    }

    pub fn why(&self) -> Option<Why> {
        self.why
    }
}

/// Reads the machine. Keep one and call [`sample`](Self::sample) every few seconds: the load is
/// the difference to the call before.
#[derive(Debug, Default)]
pub struct Meter {
    /// Idle and total processor time at the last reading.
    cpu: Option<(u64, u64)>,
}

impl Meter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sample(&mut self) -> Reading {
        let times = system::cpu_times();
        let load = match (self.cpu, times) {
            (Some((idle_then, total_then)), Some((idle, total))) if total > total_then => {
                let busy = (total - total_then).saturating_sub(idle.saturating_sub(idle_then));
                Some((busy as f32 / (total - total_then) as f32).clamp(0.0, 1.0))
            }
            _ => None,
        };
        self.cpu = times.or(self.cpu);
        Reading {
            on_battery: system::on_battery(),
            free_ram: system::free_ram(),
            load,
        }
    }
}

/// `MemAvailable` of `/proc/meminfo`, in bytes.
pub fn parse_meminfo(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kilobytes: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kilobytes * 1024)
}

/// The first line of `/proc/stat` as (idle, total) jiffies; idle includes the wait for I/O.
pub fn parse_proc_stat(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().next()?;
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let numbers: Vec<u64> = fields.map_while(|f| f.parse().ok()).collect();
    if numbers.len() < 4 {
        return None;
    }
    let idle = numbers[3] + numbers.get(4).copied().unwrap_or(0);
    // `guest` and `guest_nice` are counted in user and nice already.
    let total: u64 = numbers.iter().take(8).sum();
    Some((idle, total))
}

/// Whether the power supplies of `/sys/class/power_supply` say the PC runs on its battery: a
/// battery is discharging and no mains supply is online.
pub fn parse_power_supplies(supplies: &[(String, String)]) -> bool {
    // (type, value) pairs: for "Mains" the value is `online`, for "Battery" the `status`.
    let mains_online = supplies
        .iter()
        .any(|(kind, value)| kind.eq_ignore_ascii_case("Mains") && value.trim() == "1");
    let discharging = supplies.iter().any(|(kind, value)| {
        kind.eq_ignore_ascii_case("Battery") && value.trim().eq_ignore_ascii_case("Discharging")
    });
    discharging && !mains_online
}

#[cfg(windows)]
mod system {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows::Win32::System::Threading::GetSystemTimes;

    fn ticks(time: FILETIME) -> u64 {
        (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
    }

    pub fn cpu_times() -> Option<(u64, u64)> {
        let (mut idle, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: three valid out pointers.
        unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
        // The kernel time includes the idle time.
        Some((ticks(idle), ticks(kernel) + ticks(user)))
    }

    pub fn free_ram() -> Option<u64> {
        let mut status = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: `dwLength` is set, the pointer is valid.
        unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
        Some(status.ullAvailPhys)
    }

    pub fn on_battery() -> bool {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: a valid out pointer.
        if unsafe { GetSystemPowerStatus(&mut status) }.is_err() {
            return false;
        }
        // 0 is offline; 1 online; 255 unknown (a desktop without a battery says 1 or 255).
        status.ACLineStatus == 0
    }
}

#[cfg(target_os = "linux")]
mod system {
    use super::{parse_meminfo, parse_power_supplies, parse_proc_stat};

    pub fn cpu_times() -> Option<(u64, u64)> {
        parse_proc_stat(&std::fs::read_to_string("/proc/stat").ok()?)
    }

    pub fn free_ram() -> Option<u64> {
        parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
    }

    pub fn on_battery() -> bool {
        let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else {
            return false;
        };
        let supplies: Vec<(String, String)> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let kind = std::fs::read_to_string(path.join("type")).ok()?;
                let kind = kind.trim().to_string();
                let value = if kind.eq_ignore_ascii_case("Mains") {
                    std::fs::read_to_string(path.join("online")).ok()?
                } else {
                    std::fs::read_to_string(path.join("status")).ok()?
                };
                Some((kind, value))
            })
            .collect();
        parse_power_supplies(&supplies)
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod system {
    pub fn cpu_times() -> Option<(u64, u64)> {
        None
    }

    pub fn free_ram() -> Option<u64> {
        None
    }

    pub fn on_battery() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(free_gib: f64, load: f32) -> Reading {
        Reading {
            on_battery: false,
            free_ram: Some((free_gib * (1u64 << 30) as f64) as u64),
            load: Some(load),
        }
    }

    #[test]
    fn an_idle_pc_with_memory_goes_on() {
        let mut gate = Gate::new();
        assert_eq!(gate.update(&reading(8.0, 0.1), false, false), None);
    }

    #[test]
    fn a_running_game_or_the_battery_holds_everything() {
        let mut gate = Gate::new();
        assert_eq!(
            gate.update(&reading(8.0, 0.1), true, true),
            Some(Why::GameActive)
        );
        let battery = Reading {
            on_battery: true,
            ..reading(8.0, 0.1)
        };
        assert_eq!(gate.update(&battery, false, true), Some(Why::Battery));
        assert_eq!(gate.update(&reading(8.0, 0.1), false, true), None);
    }

    #[test]
    fn a_new_unlock_needs_memory_and_a_quiet_processor() {
        let mut gate = Gate::new();
        assert_eq!(
            gate.update(&reading(3.0, 0.1), false, false),
            Some(Why::Memory)
        );
        assert_eq!(
            gate.update(&reading(8.0, 0.9), false, false),
            Some(Why::Busy)
        );
        assert_eq!(gate.update(&reading(4.5, 0.55), false, false), None);
    }

    #[test]
    fn a_running_unlock_does_not_stop_for_its_own_memory_and_load() {
        let mut gate = Gate::new();
        // It took memory and a processor itself: 1.2 GiB left, load high. It goes on.
        assert_eq!(gate.update(&reading(1.2, 0.95), false, true), None);
    }

    #[test]
    fn real_memory_pressure_stops_it_and_it_resumes_with_room() {
        let mut gate = Gate::new();
        assert_eq!(
            gate.update(&reading(0.2, 0.2), false, true),
            Some(Why::Memory)
        );
        // Still short of the resume level: stays stopped (no flapping).
        assert_eq!(
            gate.update(&reading(0.7, 0.2), false, true),
            Some(Why::Memory)
        );
        assert_eq!(gate.update(&reading(1.5, 0.2), false, true), None);
    }

    #[test]
    fn a_missing_number_holds_nothing() {
        let mut gate = Gate::new();
        let blind = Reading::default();
        assert_eq!(gate.update(&blind, false, false), None);
        assert_eq!(gate.update(&blind, false, true), None);
    }

    #[test]
    fn linux_files_are_read() {
        assert_eq!(
            parse_meminfo("MemTotal: 100 kB\nMemAvailable:    2048 kB\n"),
            Some(2048 * 1024)
        );
        assert_eq!(parse_meminfo("MemTotal: 1 kB\n"), None);
        assert_eq!(
            parse_proc_stat("cpu  10 0 20 100 5 0 0 0 0 0\ncpu0 1 1 1 1\n"),
            Some((105, 135))
        );
        assert_eq!(parse_proc_stat("intr 1 2 3"), None);
        let discharging = vec![
            ("Mains".to_string(), "0\n".to_string()),
            ("Battery".to_string(), "Discharging\n".to_string()),
        ];
        assert!(parse_power_supplies(&discharging));
        let plugged = vec![
            ("Mains".to_string(), "1\n".to_string()),
            ("Battery".to_string(), "Discharging\n".to_string()),
        ];
        assert!(!parse_power_supplies(&plugged));
        assert!(!parse_power_supplies(&[]));
    }

    #[test]
    fn the_meter_reads_this_machine_without_failing() {
        let mut meter = Meter::new();
        let first = meter.sample();
        assert_eq!(first.load, None, "a load needs two readings");
        let second = meter.sample();
        if let Some(load) = second.load {
            assert!((0.0..=1.0).contains(&load));
        }
    }
}

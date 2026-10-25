//! Small text formatters the panels share: trophy dates, the start timer, tuning values and paths.

use std::path::Path;

use crate::ui::i18n::Language;

const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_TR: [&str; 12] = [
    "Oca", "Şub", "Mar", "Nis", "May", "Haz", "Tem", "Ağu", "Eyl", "Eki", "Kas", "Ara",
];

/// Year, month (1..=12) and day for a count of days since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A trophy's earned date the way `DateFormatter(dateStyle: .medium, timeStyle: .short)` writes it:
/// `Oct 17, 2026 at 9:14 PM` in English, `17 Eki 2026 21:14` in Turkish. `offset_seconds` is the
/// local UTC offset.
pub fn date_text(unix_micros: i64, offset_seconds: i32, language: Language) -> String {
    let local = unix_micros.div_euclid(1_000_000) + i64::from(offset_seconds);
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    let second_of_day = local.rem_euclid(86_400);
    let (hour, minute) = (
        (second_of_day / 3600) as u32,
        (second_of_day % 3600 / 60) as u32,
    );
    match language {
        Language::English => {
            let (h12, suffix) = match hour {
                0 => (12, "AM"),
                1..=11 => (hour, "AM"),
                12 => (12, "PM"),
                _ => (hour - 12, "PM"),
            };
            format!(
                "{} {day}, {year} at {h12}:{minute:02} {suffix}",
                MONTHS_EN[month as usize - 1]
            )
        }
        Language::Turkish => format!(
            "{day} {} {year} {hour:02}:{minute:02}",
            MONTHS_TR[month as usize - 1]
        ),
    }
}

/// The local UTC offset at `unix_micros`, 0 when it cannot be read.
pub fn local_offset_seconds(unix_micros: i64) -> i32 {
    let secs = unix_micros.div_euclid(1_000_000);
    time::OffsetDateTime::from_unix_timestamp(secs)
        .ok()
        .and_then(|utc| time::UtcOffset::local_offset_at(utc).ok())
        .map_or(0, |offset| offset.whole_seconds())
}

/// [`date_text`] in the local time zone.
pub fn local_date_text(unix_micros: i64, language: Language) -> String {
    date_text(unix_micros, local_offset_seconds(unix_micros), language)
}

/// The time of a message, on a 24-hour clock: `14:32` on the day of `now`, else the day first,
/// `Oct 22, 14:32` in English and `22 Eki 14:32` in Turkish. `offset_seconds` is the local UTC
/// offset.
pub fn message_time(unix: i64, now: i64, offset_seconds: i32, language: Language) -> String {
    let local = unix + i64::from(offset_seconds);
    let today = (now + i64::from(offset_seconds)).div_euclid(86_400);
    let days = local.div_euclid(86_400);
    let second_of_day = local.rem_euclid(86_400);
    let clock = format!(
        "{:02}:{:02}",
        second_of_day / 3600,
        second_of_day % 3600 / 60
    );
    if days == today {
        return clock;
    }
    let (_, month, day) = civil_from_days(days);
    match language {
        Language::English => format!("{} {day}, {clock}", MONTHS_EN[month as usize - 1]),
        Language::Turkish => format!("{day} {} {clock}", MONTHS_TR[month as usize - 1]),
    }
}

/// The clock time of a moment: `14:32`. `offset_seconds` is the local UTC offset.
pub fn clock_text(unix: i64, offset_seconds: i32) -> String {
    let second_of_day = (unix + i64::from(offset_seconds)).rem_euclid(86_400);
    format!(
        "{:02}:{:02}",
        second_of_day / 3600,
        second_of_day % 3600 / 60
    )
}

/// The local day a moment falls on, as a number that only compares.
pub fn day_number(unix: i64, offset_seconds: i32) -> i64 {
    (unix + i64::from(offset_seconds)).div_euclid(86_400)
}

/// What a day separator in a conversation says: Today, Yesterday, else the day (with its year
/// when it is not this year).
pub fn day_label(unix: i64, now: i64, offset_seconds: i32, language: Language) -> String {
    let days = day_number(unix, offset_seconds);
    let today = day_number(now, offset_seconds);
    match (today - days, language) {
        (0, Language::English) => "Today".to_owned(),
        (0, Language::Turkish) => "Bugün".to_owned(),
        (1, Language::English) => "Yesterday".to_owned(),
        (1, Language::Turkish) => "Dün".to_owned(),
        _ => {
            let year =
                |u: i64| civil_from_days((u + i64::from(offset_seconds)).div_euclid(86_400)).0;
            if year(unix) == year(now) {
                short_date(unix, offset_seconds, language)
            } else {
                day_text(unix, offset_seconds, language)
            }
        }
    }
}

/// The day with its year: `Oct 17, 2026` in English and `17 Eki 2026` in Turkish. `unix` is in
/// seconds, `offset_seconds` is the local UTC offset.
pub fn day_text(unix: i64, offset_seconds: i32, language: Language) -> String {
    let local = unix + i64::from(offset_seconds);
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    match language {
        Language::English => format!("{} {day}, {year}", MONTHS_EN[month as usize - 1]),
        Language::Turkish => format!("{day} {} {year}", MONTHS_TR[month as usize - 1]),
    }
}

/// The day without a year, for times too old to tell in hours: `Oct 21` in English and `21 Eki` in
/// Turkish. `unix` is in seconds, `offset_seconds` is the local UTC offset.
pub fn short_date(unix: i64, offset_seconds: i32, language: Language) -> String {
    let local = unix + i64::from(offset_seconds);
    let (_, month, day) = civil_from_days(local.div_euclid(86_400));
    match language {
        Language::English => format!("{} {day}", MONTHS_EN[month as usize - 1]),
        Language::Turkish => format!("{day} {}", MONTHS_TR[month as usize - 1]),
    }
}

/// A count with the language's thousands separator: `65,340` in English, `65.340` in Turkish.
pub fn grouped(n: u64, language: Language) -> String {
    let digits = n.to_string();
    let separator = match language {
        Language::English => ',',
        Language::Turkish => '.',
    };
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(c);
    }
    out
}

/// `m:ss` of the start timer.
pub fn elapsed_text(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// `NN%`
pub fn percent(value: i32) -> String {
    format!("{value}%")
}

/// Tenths with a times sign: `1.0×`.
pub fn times(tenths: i32) -> String {
    format!("{:.1}×", f64::from(tenths) / 10.0)
}

/// Tenths: `1.0`.
pub fn tenths(tenths: i32) -> String {
    format!("{:.1}", f64::from(tenths) / 10.0)
}

/// Milliseconds, with `off_label` for zero.
pub fn milliseconds(value: i32, off_label: &str) -> String {
    if value == 0 {
        off_label.to_owned()
    } else {
        format!("{value} ms")
    }
}

/// Writes the user's folder as `~` (`C:\Users\me\Pictures\AnyPS5` becomes `~\Pictures\AnyPS5`).
pub fn abbreviate_path(path: &Path, home: Option<&Path>) -> String {
    let full = path.display().to_string();
    if let Some(home) = home
        && let Ok(rest) = path.strip_prefix(home)
    {
        let rest = rest.display().to_string();
        return if rest.is_empty() {
            "~".to_owned()
        } else {
            format!("~{}{rest}", std::path::MAIN_SEPARATOR)
        };
    }
    full
}

/// The user's profile folder, if the environment names it.
pub fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(Into::into)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_day_is_written_with_its_year() {
        use super::day_text;
        use crate::ui::i18n::Language::{English, Turkish};
        // 2026-10-17 12:00 UTC.
        assert_eq!(day_text(1_792_238_400, 0, English), "Oct 17, 2026");
        assert_eq!(day_text(1_792_238_400, 0, Turkish), "17 Eki 2026");
        // Late in the evening UTC it is already the next day at +3.
        assert_eq!(
            day_text(1_792_238_400 + 11 * 3600, 3 * 3600, English),
            "Oct 18, 2026"
        );
    }

    #[test]
    fn counts_are_grouped_by_thousands() {
        use super::grouped;
        use crate::ui::i18n::Language::{English, Turkish};
        assert_eq!(grouped(0, English), "0");
        assert_eq!(grouped(999, English), "999");
        assert_eq!(grouped(1_240, English), "1,240");
        assert_eq!(grouped(65_340, Turkish), "65.340");
        assert_eq!(grouped(1_000_000, English), "1,000,000");
    }

    use super::*;

    // 2026-10-17 21:14:00 UTC
    const OCT_17: i64 = 1_792_271_640 * 1_000_000;

    #[test]
    fn civil_dates_are_right() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_513), (2026, 3, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29), "leap day");
    }

    #[test]
    fn english_dates_read_like_the_original() {
        assert_eq!(
            date_text(OCT_17, 0, Language::English),
            "Oct 17, 2026 at 9:14 PM"
        );
        // Midnight and noon are 12, not 0.
        assert_eq!(
            date_text(
                OCT_17 + 2 * 3600 * 1_000_000 + 46 * 60 * 1_000_000,
                0,
                Language::English
            ),
            "Oct 18, 2026 at 12:00 AM"
        );
        assert_eq!(
            date_text(
                OCT_17 - 9 * 3600 * 1_000_000 - 14 * 60 * 1_000_000,
                0,
                Language::English
            ),
            "Oct 17, 2026 at 12:00 PM"
        );
    }

    #[test]
    fn the_offset_moves_the_day() {
        assert_eq!(
            date_text(OCT_17, 3 * 3600, Language::English),
            "Oct 18, 2026 at 12:14 AM"
        );
    }

    #[test]
    fn turkish_dates_use_month_names_and_24_hours() {
        assert_eq!(date_text(OCT_17, 0, Language::Turkish), "17 Eki 2026 21:14");
    }

    #[test]
    fn a_message_shows_the_day_only_when_it_is_not_today() {
        let noon = 1_792_670_400; // 2026-10-22 12:00 UTC
        let en = Language::English;
        assert_eq!(message_time(noon + 150, noon, 0, en), "12:02");
        assert_eq!(message_time(noon - 86_400, noon, 0, en), "Oct 21, 12:00");
        assert_eq!(
            message_time(noon - 86_400, noon, 0, Language::Turkish),
            "21 Eki 12:00"
        );
        // Half past eleven in the evening, UTC, is already tomorrow two hours east.
        let late = noon + 11 * 3600 + 1800;
        assert_eq!(message_time(late, late, 7200, en), "01:30");
        assert_eq!(message_time(noon, late, 7200, en), "Oct 22, 14:00");
    }

    #[test]
    fn the_timer_counts_minutes() {
        assert_eq!(elapsed_text(0), "0:00");
        assert_eq!(elapsed_text(75), "1:15");
        assert_eq!(elapsed_text(3600), "60:00");
    }

    #[test]
    fn tuning_values_format_like_the_original() {
        assert_eq!(percent(20), "20%");
        assert_eq!(times(10), "1.0×");
        assert_eq!(times(25), "2.5×");
        assert_eq!(tenths(10), "1.0");
        assert_eq!(milliseconds(0, "Off"), "Off");
        assert_eq!(milliseconds(30, "Off"), "30 ms");
    }

    #[cfg(not(windows))]
    #[test]
    fn the_home_folder_becomes_a_tilde_on_unix() {
        let home = Path::new("/home/me");
        assert_eq!(
            abbreviate_path(Path::new("/home/me/Pictures/AnyPS5"), Some(home)),
            "~/Pictures/AnyPS5"
        );
        assert_eq!(
            abbreviate_path(Path::new("/mnt/games"), Some(home)),
            "/mnt/games"
        );
        assert_eq!(abbreviate_path(home, Some(home)), "~");
    }

    #[cfg(windows)]
    #[test]
    fn the_home_folder_becomes_a_tilde() {
        let home = Path::new("C:\\Users\\me");
        assert_eq!(
            abbreviate_path(Path::new("C:\\Users\\me\\Pictures\\AnyPS5"), Some(home)),
            "~\\Pictures\\AnyPS5"
        );
        assert_eq!(
            abbreviate_path(Path::new("D:\\Games"), Some(home)),
            "D:\\Games"
        );
        assert_eq!(abbreviate_path(home, Some(home)), "~");
    }
}

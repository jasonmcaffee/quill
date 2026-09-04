//! The instant, as text, and how long ago something was.
//!
//! The board writes an ISO 8601 instant in UTC into every timestamp column, so this is where that text
//! is made and read. It is a small amount of calendar arithmetic rather than a dependency, for the
//! reason `services::store` writes `name = value` by hand: nothing here needs a format library, and a
//! board file that can be read and corrected in a text editor is fitting in a text editor.
//!
//! **Every function takes the instant rather than reading the clock**, except [`now`] itself. That is
//! what makes the watchdog, the scheduler and the store testable with a fixed instant, which is the
//! arrangement `dock::regions` and `unluminate_dap`'s state machine already use.

use std::time::{SystemTime, UNIX_EPOCH};

/// The instant, as `2026-08-29T22:49:26Z`.
///
/// A clock that has gone behind 1970 gives the epoch rather than an error, because a board that
/// refused to write a comment because the machine's clock was wrong would be worse than a comment with
/// an odd date on it.
pub fn now() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map(|since| since.as_secs()).unwrap_or(0);
    from_unix(seconds as i64)
}

/// An instant from a count of seconds since 1970, as ISO 8601 in UTC.
pub fn from_unix(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (time / 3600, (time % 3600) / 60, time % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The seconds since 1970 an ISO 8601 instant names, or `None` when it is not one.
///
/// Only the shape this module writes is read: `YYYY-MM-DDTHH:MM:SSZ`, and a trailing fraction or offset
/// is ignored rather than being a refusal, because a row edited by hand should still be read.
pub fn to_unix(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    // The separators are checked, because `2026x08x29` parsed perfectly well before and meant nothing.
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let number = |from: usize, to: usize| -> Option<i64> {
        let part = text.get(from..to)?;
        // `parse` accepts a sign and whitespace, which in a fixed width field means `2026-+8-29` reads as
        // August. Every character has to be a digit.
        match part.bytes().all(|byte| byte.is_ascii_digit()) {
            true => part.parse::<i64>().ok(),
            false => None,
        }
    };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    if !(1..=12).contains(&month) || !(1..=days_in(year, month)).contains(&day) {
        return None;
    }
    // 60 is a leap second, which no clock here writes and which SQLite would not either.
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let instant = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    Some(instant - offset_seconds(text)?)
}

/// How many seconds ahead of UTC the text says it is, which is what has to be taken off to get UTC.
///
/// `Z` and `+00:00` are zero. A real offset is read rather than ignored: reading `2026-08-29T22:00:00+02:00`
/// as UTC would put it two hours later than it happened, and the board's own writer never produces one, so
/// this is for a row somebody edited or copied out of PostgreSQL. Text with no offset at all is read as UTC,
/// which is what SQLite's own functions assume.
fn offset_seconds(text: &str) -> Option<i64> {
    let rest = text.get(19..)?;
    // A fraction of a second, which PostgreSQL writes and which changes nothing here.
    let rest = match rest.strip_prefix('.') {
        Some(fraction) => {
            let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
            fraction.get(digits..)?
        }
        None => rest,
    };
    let rest = rest.trim();
    if rest.is_empty() || rest.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let ahead = match rest.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits: Vec<u8> = rest.bytes().skip(1).filter(u8::is_ascii_digit).collect();
    if digits.len() != 4 {
        return None;
    }
    let read = |pair: &[u8]| -> Option<i64> { std::str::from_utf8(pair).ok()?.parse().ok() };
    let hours = read(&digits[0..2])?;
    let minutes = read(&digits[2..4])?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(ahead * (hours * 3600 + minutes * 60))
}

/// How many days that month of that year has, so `2026-02-31` is refused rather than read as March.
fn days_in(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => match is_a_leap_year(year) {
            true => 29,
            false => 28,
        },
        _ => 0,
    }
}

/// The Gregorian rule in full: every fourth year, except every hundredth, except every four hundredth.
fn is_a_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// How many minutes there are between two instants, or 0 when either is not one.
pub fn minutes_between(earlier: &str, later: &str) -> i64 {
    match (to_unix(earlier), to_unix(later)) {
        (Some(from), Some(to)) => (to - from).max(0) / 60,
        _ => 0,
    }
}

/// `4m ago`, `3h ago`, `2d ago`, or `just now`.
///
/// What a card and a comment show. Short, because it sits beside a ticket key in a 420 point column and
/// `4 minutes ago` would push the counts off the end.
pub fn relative(then: &str, now: &str) -> String {
    let minutes = minutes_between(then, now);
    match minutes {
        0 => "just now".to_owned(),
        1..=59 => format!("{minutes}m ago"),
        60..=1439 => format!("{}h ago", minutes / 60),
        _ => format!("{}d ago", minutes / 1440),
    }
}

/// Days since 1970 from a civil date. Howard Hinnant's algorithm, which is exact and has no branches
/// for leap years because the era arithmetic already accounts for them.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The civil date a count of days since 1970 names. The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_and_a_known_instant_round_trip() {
        assert_eq!(from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(to_unix("1970-01-01T00:00:00Z"), Some(0));
        // 2026-08-29T22:49:26Z, which is the instant the ticket this was written for was last touched.
        let known = "2026-08-29T22:49:26Z";
        let seconds = to_unix(known).expect("a known instant");
        assert_eq!(from_unix(seconds), known);
    }

    #[test]
    fn a_leap_day_and_a_century_are_both_right() {
        // 2000 is a leap year and 1900 is not, which is the case a naive rule gets wrong.
        assert_eq!(from_unix(to_unix("2000-02-29T12:00:00Z").expect("a leap day")), "2000-02-29T12:00:00Z");
        assert_eq!(from_unix(to_unix("2024-12-31T23:59:59Z").expect("new year's eve")), "2024-12-31T23:59:59Z");
        assert_eq!(from_unix(to_unix("2100-03-01T00:00:00Z").expect("after a non leap century")), "2100-03-01T00:00:00Z");
    }

    #[test]
    fn every_day_of_a_year_round_trips() {
        // Two years of days, each one written and read back, which is what proves the arithmetic rather
        // than a handful of dates that happen to work.
        let start = to_unix("2025-01-01T00:00:00Z").expect("the start");
        for day in 0..730 {
            let seconds = start + day * 86_400;
            let text = from_unix(seconds);
            assert_eq!(to_unix(&text), Some(seconds), "{text} did not read back");
        }
    }

    #[test]
    fn text_that_is_not_an_instant_is_refused_rather_than_read_as_the_epoch() {
        for refused in [
            "",
            "yesterday",
            "2026-13-01T00:00:00Z",   // there is no thirteenth month
            "2026-08-00T00:00:00Z",   // there is no zeroth day
            "2026-02-30T00:00:00Z",   // February never has thirty days
            "2026-02-29T00:00:00Z",   // and 2026 is not a leap year
            "2026-04-31T00:00:00Z",   // April has thirty
            "2026-08-29T24:00:00Z",   // there is no twenty-fourth hour
            "2026-08-29T22:60:00Z",   // nor a sixtieth minute
            "2026-08-29T22:49:60Z",   // nor a sixtieth second
            "2026x08x29T22:49:26Z",   // the separators are checked
            "2026-08-29 22:49:26",    // and the `T` is one of them
            "2026-+8-29T22:49:26Z",   // a sign in a fixed width field is not a number
            "2026-08-29T22:49:26+2",  // a partial offset says nothing
            "2026-08-29T22:49:26*",   // and neither does that
        ] {
            assert_eq!(to_unix(refused), None, "`{refused}` should be refused");
        }
        // The leap day rule in full, in both directions.
        assert!(to_unix("2024-02-29T00:00:00Z").is_some(), "2024 is a leap year");
        assert!(to_unix("2000-02-29T00:00:00Z").is_some(), "2000 is, because of the four hundred rule");
        assert_eq!(to_unix("1900-02-29T00:00:00Z"), None, "1900 is not, because of the hundred rule");
    }

    #[test]
    fn a_fraction_is_ignored_and_an_offset_is_taken_off() {
        // PostgreSQL writes microseconds and an offset. A board file copied out of the application being
        // replaced should read, and an offset read as UTC would put the instant hours from where it was.
        assert_eq!(to_unix("2026-08-29T22:49:26.188Z"), to_unix("2026-08-29T22:49:26Z"));
        assert_eq!(to_unix("2026-08-29T22:49:26.188456+00:00"), to_unix("2026-08-29T22:49:26Z"));
        assert_eq!(to_unix("2026-08-29T22:49:26+00:00"), to_unix("2026-08-29T22:49:26Z"));
        assert_eq!(
            to_unix("2026-08-29T22:49:26+02:00"),
            to_unix("2026-08-29T20:49:26Z"),
            "two hours ahead of UTC is two hours earlier in UTC"
        );
        assert_eq!(
            to_unix("2026-08-29T22:49:26-0600"),
            to_unix("2026-08-30T04:49:26Z"),
            "and six behind is six later, with or without the colon"
        );
        assert_eq!(
            to_unix("2026-08-29T22:49:26"),
            to_unix("2026-08-29T22:49:26Z"),
            "no offset at all is read as UTC, which is what SQLite's own functions assume"
        );
    }

    #[test]
    fn how_long_ago_is_said_in_the_largest_unit_that_fits() {
        let now = "2026-08-29T12:00:00Z";
        assert_eq!(relative("2026-08-29T11:59:30Z", now), "just now");
        assert_eq!(relative("2026-08-29T11:56:00Z", now), "4m ago");
        assert_eq!(relative("2026-08-29T09:00:00Z", now), "3h ago");
        assert_eq!(relative("2026-08-27T12:00:00Z", now), "2d ago");
        // A clock that has gone backwards says `just now` rather than a negative age.
        assert_eq!(relative("2026-08-30T12:00:00Z", now), "just now");
    }

    #[test]
    fn the_clock_writes_something_the_reader_can_read() {
        let instant = now();
        assert_eq!(instant.len(), 20, "{instant} is not an ISO 8601 instant");
        assert!(to_unix(&instant).is_some(), "{instant} did not read back");
        assert!(instant.ends_with('Z'), "{instant} is not in UTC");
    }
}

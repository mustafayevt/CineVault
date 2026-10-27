//! A small calendar date-time with the `yyyy-MM-dd` style pattern letters templates use.
//!
//! The engine has no dependency on a calendar crate: values carry a UTC instant in milliseconds
//! and the UTC offset to show it in, and `format` renders English names only.

use std::fmt::Write as _;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// The pattern used when a template asks for `;date` without a pattern.
pub const DEFAULT_DATE_PATTERN: &str = "dd.MM.yyyy HH:mm:ss Z";

/// An instant plus the UTC offset it is shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    /// Milliseconds since 1970-01-01T00:00:00Z.
    pub unix_ms: i64,
    /// Offset of the display zone from UTC in minutes (east is positive).
    pub offset_minutes: i32,
}

/// The broken-down local fields of a [`DateTime`].
struct Fields {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
    /// 0 = Sunday.
    weekday: usize,
    day_of_year: u32,
}

impl DateTime {
    /// Builds a date-time from local fields in the zone `offset_minutes`. Returns `None` for
    /// fields out of range.
    pub fn from_local(
        year: i64,
        month: u32,
        day: u32,
        time: (u32, u32, u32),
        offset_minutes: i32,
    ) -> Option<DateTime> {
        let (hour, minute, second) = time;
        if !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
            || hour > 23
            || minute > 59
            || second > 59
        {
            return None;
        }
        let days = days_from_civil(year, month, day);
        let local_secs = days * 86_400 + i64::from(hour * 3600 + minute * 60 + second);
        let unix_secs = local_secs - i64::from(offset_minutes) * 60;
        Some(DateTime {
            unix_ms: unix_secs * 1000,
            offset_minutes,
        })
    }

    /// Parses `yyyy-MM-dd` or `yyyy-MM-dd HH:mm:ss` as a local time in the given zone.
    pub fn parse(text: &str, offset_minutes: i32) -> Option<DateTime> {
        let text = text.trim();
        let (date, time) = match text.split_once([' ', 'T']) {
            Some((d, t)) => (d, Some(t)),
            None => (text, None),
        };
        let mut parts = date.split('-');
        let year: i64 = parts.next()?.parse().ok()?;
        let month: u32 = parts.next()?.parse().ok()?;
        let day: u32 = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        let mut clock = (0, 0, 0);
        if let Some(time) = time {
            let mut parts = time.split(':');
            clock.0 = parts.next()?.parse().ok()?;
            clock.1 = parts.next()?.parse().ok()?;
            clock.2 = parts.next().map_or(Some(0), |s| s.parse().ok())?;
        }
        DateTime::from_local(year, month, day, clock, offset_minutes)
    }

    fn fields(&self) -> Fields {
        let local_ms = self.unix_ms + i64::from(self.offset_minutes) * 60_000;
        let days = local_ms.div_euclid(86_400_000);
        let ms_of_day = local_ms.rem_euclid(86_400_000);
        let (year, month, day) = civil_from_days(days);
        let secs = ms_of_day / 1000;
        Fields {
            year,
            month,
            day,
            hour: (secs / 3600) as u32,
            minute: (secs % 3600 / 60) as u32,
            second: (secs % 60) as u32,
            millis: (ms_of_day % 1000) as u32,
            weekday: (days + 4).rem_euclid(7) as usize,
            day_of_year: (days - days_from_civil(year, 1, 1)) as u32 + 1,
        }
    }

    /// Renders the date with a `yyyy-MM-dd` style pattern. Returns `None` for a pattern that
    /// uses an unsupported letter, so callers can decide what an invalid pattern shows.
    pub fn format(&self, pattern: &str) -> Option<String> {
        let f = self.fields();
        let chars: Vec<char> = pattern.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '\'' {
                if chars.get(i + 1) == Some(&'\'') {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\'' {
                        if chars.get(i + 1) == Some(&'\'') {
                            out.push('\'');
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    out.push(chars[i]);
                    i += 1;
                }
                i += 1;
                continue;
            }
            if !c.is_ascii_alphabetic() {
                out.push(c);
                i += 1;
                continue;
            }
            let mut n = 1;
            while chars.get(i + n) == Some(&c) {
                n += 1;
            }
            i += n;
            self.field(&mut out, &f, c, n)?;
        }
        Some(out)
    }

    fn field(&self, out: &mut String, f: &Fields, letter: char, count: usize) -> Option<()> {
        let pad = |out: &mut String, v: i64| {
            let _ = write!(out, "{v:0count$}");
        };
        match letter {
            'y' | 'Y' => {
                if count == 2 {
                    let _ = write!(out, "{:02}", f.year.rem_euclid(100));
                } else {
                    pad(out, f.year);
                }
            }
            'M' | 'L' => match count {
                1 | 2 => pad(out, i64::from(f.month)),
                3 => out.push_str(&MONTHS[f.month as usize - 1][..3]),
                _ => out.push_str(MONTHS[f.month as usize - 1]),
            },
            'd' => pad(out, i64::from(f.day)),
            'D' => pad(out, i64::from(f.day_of_year)),
            'H' => pad(out, i64::from(f.hour)),
            'k' => pad(out, if f.hour == 0 { 24 } else { i64::from(f.hour) }),
            'K' => pad(out, i64::from(f.hour % 12)),
            'h' => pad(
                out,
                if f.hour.is_multiple_of(12) {
                    12
                } else {
                    i64::from(f.hour % 12)
                },
            ),
            'm' => pad(out, i64::from(f.minute)),
            's' => pad(out, i64::from(f.second)),
            // The millisecond count, not a fraction.
            'S' => pad(out, i64::from(f.millis)),
            'a' => out.push_str(if f.hour < 12 { "AM" } else { "PM" }),
            'E' => {
                let name = DAYS[f.weekday];
                out.push_str(if count >= 4 { name } else { &name[..3] });
            }
            'u' => pad(out, if f.weekday == 0 { 7 } else { f.weekday as i64 }),
            'Z' => out.push_str(&self.offset_text(false, false)),
            'X' => match count {
                1 => out.push_str(&self.offset_text(true, true).replace(":00", "")),
                2 => out.push_str(&self.offset_text(false, true)),
                _ => out.push_str(&self.offset_text(true, true)),
            },
            'z' => {
                if self.offset_minutes == 0 {
                    out.push_str("UTC");
                } else {
                    out.push_str("GMT");
                    out.push_str(&self.offset_text(true, false));
                }
            }
            _ => return None,
        }
        Some(())
    }

    /// `+0100` (RFC 822) or `+01:00` (ISO 8601); with `zulu` an offset of zero is `Z`.
    fn offset_text(&self, colon: bool, zulu: bool) -> String {
        if zulu && self.offset_minutes == 0 {
            return "Z".to_owned();
        }
        let sign = if self.offset_minutes < 0 { '-' } else { '+' };
        let abs = self.offset_minutes.unsigned_abs();
        if colon {
            format!("{sign}{:02}:{:02}", abs / 60, abs % 60)
        } else {
            format!("{sign}{:02}{:02}", abs / 60, abs % 60)
        }
    }
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_the_default_pattern() {
        let d = DateTime::parse("2024-01-01 00:00:00", 0).unwrap();
        assert_eq!(
            d.format(DEFAULT_DATE_PATTERN).unwrap(),
            "01.01.2024 00:00:00 +0000"
        );
    }

    #[test]
    fn formats_names_and_zones() {
        let d = DateTime::parse("2024-02-29 13:05:09", 120).unwrap();
        assert_eq!(
            d.format("EEEE, d MMMM yyyy h:mm a 'at' XXX Z").unwrap(),
            "Thursday, 29 February 2024 1:05 PM at +02:00 +0200"
        );
        assert_eq!(d.format("EEE MMM yy").unwrap(), "Thu Feb 24");
        assert_eq!(d.format("yyyy-MM-dd").unwrap(), "2024-02-29");
    }

    #[test]
    fn rejects_unsupported_letters_and_bad_dates() {
        let d = DateTime::parse("2024-02-29", 0).unwrap();
        assert!(d.format("yyyy Q").is_none());
        assert!(DateTime::parse("2023-02-29", 0).is_none());
        assert!(DateTime::parse("nope", 0).is_none());
    }

    #[test]
    fn handles_dates_before_the_epoch() {
        let d = DateTime::parse("1969-12-31 23:59:59", 0).unwrap();
        assert_eq!(d.unix_ms, -1000);
        assert_eq!(
            d.format("yyyy-MM-dd HH:mm:ss").unwrap(),
            "1969-12-31 23:59:59"
        );
    }
}

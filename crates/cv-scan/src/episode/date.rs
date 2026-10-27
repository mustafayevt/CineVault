//! Aired dates in episode names: `2020-05-17`, `2020.05.17` and `17-05-2020`.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

/// A calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AiredDate {
    pub year: u32,
    pub month: u32,
    pub day: u32,
}

impl fmt::Display for AiredDate {
    /// `yyyy-MM-dd`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl AiredDate {
    /// Builds a date from the numbers in a name. Values out of range roll over like a lenient
    /// calendar: month 13 is January of the next year, day 45 of January is 14 February.
    pub fn new_lenient(year: u32, month: u32, day: u32) -> AiredDate {
        let months = i64::from(year) * 12 + i64::from(month) - 1;
        let (year, month) = (months.div_euclid(12), months.rem_euclid(12) + 1);
        let days = days_from_civil(year, month, 1) + i64::from(day) - 1;
        let (year, month, day) = civil_from_days(days);
        AiredDate {
            year: u32::try_from(year).unwrap_or(0),
            month,
            day,
        }
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse of [`days_from_civil`]: (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

struct Patterns {
    year_first: Regex,
    day_first: Regex,
}

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    let re = |p: &str| Regex::new(p).expect("static date pattern");
    Patterns {
        year_first: re(r"(?:^|[^0-9])(\d{4})[.\-](\d{2})[.\-](\d{2})(?:[^0-9]|$)"),
        day_first: re(r"(?:^|[^0-9])(\d{2})[.\-](\d{2})[.\-](\d{4})(?:[^0-9]|$)"),
    }
});

/// Finds the first date in the text (`yyyy-mm-dd` before `dd-mm-yyyy`) and returns it with the
/// text where the date is replaced by a space.
pub(super) fn extract(text: &str) -> (Option<AiredDate>, String) {
    let p = &*PATTERNS;
    if let Some(c) = p.year_first.captures(text) {
        let number = |i: usize| c[i].parse::<u32>().unwrap_or(0);
        let date = AiredDate::new_lenient(number(1), number(2), number(3));
        return (Some(date), replace_span(text, &c, 1, 3));
    }
    if let Some(c) = p.day_first.captures(text) {
        let number = |i: usize| c[i].parse::<u32>().unwrap_or(0);
        let date = AiredDate::new_lenient(number(3), number(2), number(1));
        return (Some(date), replace_span(text, &c, 1, 3));
    }
    (None, text.to_owned())
}

fn replace_span(text: &str, c: &regex::Captures<'_>, first: usize, last: usize) -> String {
    let (start, end) = match (c.get(first), c.get(last)) {
        (Some(a), Some(b)) => (a.start(), b.end()),
        _ => return text.to_owned(),
    };
    format!("{} {}", &text[..start], &text[end..])
}

/// Removes every date from the text for the episode title: `yyyy-mm-dd`, `yyyy.mm.dd` and
/// `dd-mm-yyyy`. A `dd.mm.yyyy` date stays.
pub(super) fn remove_for_title(text: &str) -> String {
    static TITLE: LazyLock<(Regex, Regex)> = LazyLock::new(|| {
        (
            Regex::new(r"(^|[^0-9])\d{4}[.\-]\d{2}[.\-]\d{2}([^0-9]|$)").expect("static pattern"),
            Regex::new(r"(^|[^0-9])\d{2}-\d{2}-\d{4}([^0-9]|$)").expect("static pattern"),
        )
    });
    let once = TITLE.0.replace_all(text, "$1 $2");
    TITLE.1.replace_all(&once, "$1 $2").into_owned()
}

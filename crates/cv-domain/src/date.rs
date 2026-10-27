//! A validated calendar date, serialised as `yyyy-MM-dd`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Error returned when a string is not a valid `yyyy-MM-dd` date.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid calendar date: {0:?}")]
pub struct DateParseError(pub String);

/// A proleptic Gregorian calendar date without a time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CalendarDate {
    year: i32,
    month: u8,
    day: u8,
}

impl CalendarDate {
    /// Builds a date, returning `None` when the year, month or day is out of range.
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        if !(0..=9999).contains(&year) || !(1..=12).contains(&month) {
            return None;
        }
        if day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Self { year, month, day })
    }

    pub fn year(&self) -> i32 {
        self.year
    }

    pub fn month(&self) -> u8 {
        self.month
    }

    pub fn day(&self) -> u8 {
        self.day
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

impl fmt::Display for CalendarDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for CalendarDate {
    type Err = DateParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || DateParseError(s.to_owned());
        let mut parts = s.trim().split('-');
        let (y, m, d) = (parts.next(), parts.next(), parts.next());
        if parts.next().is_some() {
            return Err(err());
        }
        let (y, m, d) = (y.ok_or_else(err)?, m.ok_or_else(err)?, d.ok_or_else(err)?);
        if y.len() != 4 || m.len() != 2 || d.len() != 2 {
            return Err(err());
        }
        let year = y.parse().map_err(|_| err())?;
        let month = m.parse().map_err(|_| err())?;
        let day = d.parse().map_err(|_| err())?;
        Self::new(year, month, day).ok_or_else(err)
    }
}

impl Serialize for CalendarDate {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CalendarDate {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_as_iso_string() {
        let d = CalendarDate::new(1979, 5, 25).unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"1979-05-25\"");
        assert_eq!(serde_json::from_str::<CalendarDate>(&json).unwrap(), d);
    }

    #[test]
    fn rejects_invalid_dates() {
        for bad in [
            "2023-02-29",
            "2023-13-01",
            "2023-00-10",
            "23-01-01",
            "2023-1-1",
            "x",
            "",
        ] {
            assert!(bad.parse::<CalendarDate>().is_err(), "{bad}");
        }
        assert!("2024-02-29".parse::<CalendarDate>().is_ok());
    }
}

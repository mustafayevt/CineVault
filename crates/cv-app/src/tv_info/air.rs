//! Air information of a show: when it started, when the last episode aired, what airs next and,
//! for a show that has stopped, when it ended.

use cv_domain::{AiredStatus, CalendarDate};

/// The air date of one known episode of a show, owned or missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpisodeAir {
    pub season: u32,
    pub date: CalendarDate,
}

/// What the show page says about a show's run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AirInfo {
    /// The date the provider gave, else the earliest episode.
    pub first_aired: Option<CalendarDate>,
    /// The latest episode that has aired (today counts as aired).
    pub last_aired: Option<CalendarDate>,
    /// The earliest episode that has not aired yet.
    pub next_aired: Option<CalendarDate>,
    /// The date of the last episode of an ended or cancelled show.
    pub end_date: Option<CalendarDate>,
    /// The year of `end_date`, `0` while the show runs or when no date is known.
    pub end_year: u16,
}

/// Derives the air information of a show.
///
/// Specials (season 0) are ignored while the show has regular episodes, because a special long
/// after the finale does not move the end of the show; a show with only specials uses them.
/// `first_aired` is the provider's date of the show, used before any episode date.
#[must_use]
pub fn air_info(
    status: AiredStatus,
    first_aired: Option<CalendarDate>,
    episodes: &[EpisodeAir],
    today: CalendarDate,
) -> AirInfo {
    let regular = episodes.iter().any(|e| e.season > 0);
    let dates: Vec<CalendarDate> = episodes
        .iter()
        .filter(|e| !regular || e.season > 0)
        .map(|e| e.date)
        .collect();

    let last_aired = dates.iter().copied().filter(|d| *d <= today).max();
    let next_aired = dates.iter().copied().filter(|d| *d > today).min();
    let stopped = matches!(status, AiredStatus::Ended | AiredStatus::Canceled);
    // A stopped show ends with its last known episode.
    let end_date = if stopped {
        dates.iter().copied().max()
    } else {
        None
    };
    AirInfo {
        first_aired: first_aired.or_else(|| dates.iter().copied().min()),
        last_aired,
        next_aired,
        end_date,
        end_year: end_date
            .and_then(|d| u16::try_from(d.year()).ok())
            .unwrap_or(0),
    }
}

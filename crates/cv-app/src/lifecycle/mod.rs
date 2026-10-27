//! The title lifecycle: three stages and the flags on top of them (spec
//! `title-lifecycle`).
//!
//! Every movie, show, season and episode is in exactly one stage:
//!
//! * **New**: found by a scan; it has no provider ID yet, or its details are not in place.
//! * **Scraped**: it has a provider ID and its details came from a provider or from an NFO file.
//!   A rating source that found nothing, or an extra that is off, does not keep it from being
//!   scraped: the extras only feed the explanation lines.
//! * **Renamed**: scraped, and the folder and file names match the active naming profile (the
//!   cached organize state is tidy).
//!
//! On top of its stage a title carries zero or more flags: Needs review (an uncertain match with
//! its reason), Not matched (a scrape was tried and
//! found no confident match, with its reason), Rename pending, Locked, Missing (file or source
//! offline) and Working (a job is touching it now). A locked title is never Rename pending.
//!
//! "Not matched" has one meaning everywhere: the poster caption, the table's Stage cell, Home and
//! the library filter all read this flag. It needs a scrape that was tried: the stored outcome of
//! the `scrape` step ([`Facts::scrape_miss`]), or a search that found nothing waiting in the review
//! queue. A title no scrape was tried for has no flag at all: it is just New, "not scraped yet".
//! Only the titles whose search found nothing and that wait in the review queue are
//! also counted by Needs review ([`Lifecycle::in_review`]).
//!
//! [`derive`] is a pure function from [`Facts`] to a [`Lifecycle`]; [`read`] gathers the facts of
//! a list of titles from the library in a few queries; [`lifecycle_counts`] counts a library type
//! in one grouped SQL pass using the same rules, so the sidebar, Home, Needs review, Organize and
//! Activity cannot disagree.

mod counts;
mod lines;
mod read;
mod service;

use cv_db::repo::step_outcome::{STEP_SCRAPE, StepOutcome, StepStatus};
use cv_domain::{Availability, ItemKind};

pub use counts::{LifecycleCounts, lifecycle_counts};
pub use lines::{Line, LineLink, LinePart, LineStatus, LineStep, changes_text, lines_of};
pub use read::{LifecycleContext, read};
pub use service::{LifecycleService, RatingsGuard, Working, WorkingGuard};

/// The stage of a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    New,
    Scraped,
    Renamed,
}

impl Stage {
    /// The stable text used in IPC and filters.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Scraped => "scraped",
            Self::Renamed => "renamed",
        }
    }
}

/// What can be true of a title on top of its stage, in the order the meter shows the main one:
/// work in progress first, then what needs the user, then what is merely pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FlagKind {
    Working,
    NeedsReview,
    NotMatched,
    Missing,
    Locked,
    RenamePending,
}

impl FlagKind {
    /// The stable text used in IPC and filters.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::NeedsReview => "needs-review",
            Self::NotMatched => "not-matched",
            Self::Missing => "missing",
            Self::Locked => "locked",
            Self::RenamePending => "rename-pending",
        }
    }
}

/// A flag with its reason (empty when there is nothing to add).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    pub kind: FlagKind,
    pub reason: String,
}

impl Flag {
    fn new(kind: FlagKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            reason: reason.into(),
        }
    }
}

/// The cached organize state of a title, with what the explanation needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Organize {
    /// Not computed yet for the active profile.
    Checking,
    Tidy,
    /// A rename would change something; the text says how much ("3 changes").
    Pending(String),
    /// A rename would collide; the text says how.
    Conflict(String),
    Locked,
    /// It cannot be planned now; the text says why.
    Unplanned(String),
}

/// A match waiting for a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub reason: String,
    /// The search found nothing: the title could not be matched.
    pub no_candidates: bool,
}

/// Which extras the settings switch on, so an extra that is off reads "off", not "missing".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extras {
    pub ratings: bool,
    pub collections: bool,
}

impl Default for Extras {
    fn default() -> Self {
        Self {
            ratings: true,
            collections: false,
        }
    }
}

/// The provider a title was scraped from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    /// The provider's name, for example `TMDB`.
    pub label: String,
    pub id: String,
}

/// Everything the lifecycle of one title is derived from.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    pub kind: ItemKind,
    pub title: String,
    pub year: u16,
    /// When the scan added the title, epoch milliseconds.
    pub added_ms: i64,
    /// When the title was last written, epoch milliseconds (the scrape time, best known).
    pub updated_ms: i64,
    /// The title has a provider ID (not just a collection ID).
    pub has_provider_id: bool,
    /// A scraper wrote the details.
    pub scraped_by_provider: bool,
    /// An NFO file is listed for the title.
    pub has_nfo: bool,
    pub provider: Option<Provider>,
    pub locked: bool,
    pub availability: Availability,
    pub review: Option<Review>,
    pub organize: Organize,
    /// A job is touching the title now.
    pub working: bool,
    /// What each after-scrape step did for the title.
    pub outcomes: Vec<StepOutcome>,
    pub extras: Extras,
    /// Where the title lives, as the user names it: its library folder's label and the folders
    /// below it (`Archive › Heat (1995)`). `None` when not known; the Scan line then names the
    /// library type and the title.
    pub location: Option<String>,
}

impl Facts {
    /// Facts of a title with nothing known, for tests and for the grouped counts.
    #[must_use]
    pub fn new(kind: ItemKind) -> Self {
        Self {
            kind,
            title: String::new(),
            year: 0,
            added_ms: 0,
            updated_ms: 0,
            has_provider_id: false,
            scraped_by_provider: false,
            has_nfo: false,
            provider: None,
            locked: false,
            availability: Availability::Online,
            review: None,
            organize: Organize::Checking,
            working: false,
            outcomes: Vec::new(),
            extras: Extras::default(),
            location: None,
        }
    }

    /// The title is matched to a provider ID and its details are in place, fetched online or
    /// read from an NFO file that has a provider ID.
    #[must_use]
    pub fn is_scraped(&self) -> bool {
        self.has_provider_id && (self.scraped_by_provider || self.has_nfo)
    }

    /// The stored outcome of the title's last scrape when it was tried and found no confident
    /// match (nothing found, only candidates that could not decide, no key, or the provider
    /// failed). `None` when no scrape was tried, or the last one matched.
    #[must_use]
    pub fn scrape_miss(&self) -> Option<&StepOutcome> {
        self.outcomes
            .iter()
            .find(|o| o.step == STEP_SCRAPE && o.status.is_scrape_miss())
    }

    /// The file or the library folder cannot be reached.
    #[must_use]
    pub fn is_missing(&self) -> bool {
        matches!(
            self.availability,
            Availability::Missing | Availability::Offline
        )
    }
}

/// What a title no scrape was tried for says in place of a match (the Scrape line's label).
pub const NOT_SCRAPED_YET: &str = "Not scraped yet";

/// The reason of the Not matched flag of a title whose last scrape found no confident match: the
/// stored reason, or a plain sentence for the outcome when none was stored.
#[must_use]
pub fn scrape_miss_reason(miss: &StepOutcome) -> String {
    let reason = miss.reason.trim();
    if !reason.is_empty() {
        return reason.to_owned();
    }
    match miss.status {
        StepStatus::NeedsKey => "The provider needs a key".to_owned(),
        StepStatus::Failed => "The scrape failed".to_owned(),
        _ => "No match found".to_owned(),
    }
}

/// The stage and flags of a title, with the explanation lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Lifecycle {
    pub stage: Stage,
    /// In the order of [`FlagKind`]: the first is the main flag.
    pub flags: Vec<Flag>,
    /// One line per step: Scan, Scrape and Rename.
    pub lines: Vec<Line>,
    /// Needs review counts the title: an uncertain match or a search that found nothing. The
    /// Needs review inbox lists exactly these.
    pub in_review: bool,
}

impl Lifecycle {
    /// The flag the meter shows beside the stage word.
    #[must_use]
    pub fn main_flag(&self) -> Option<&Flag> {
        self.flags.first()
    }

    /// Whether the title carries a flag of this kind.
    #[must_use]
    pub fn has(&self, kind: FlagKind) -> bool {
        self.flags.iter().any(|f| f.kind == kind)
    }
}

/// The stage of a title.
#[must_use]
pub fn stage_of(facts: &Facts) -> Stage {
    if !facts.is_scraped() {
        Stage::New
    } else if facts.organize == Organize::Tidy {
        Stage::Renamed
    } else {
        Stage::Scraped
    }
}

/// The flags of a title, main flag first.
#[must_use]
pub fn flags_of(facts: &Facts) -> Vec<Flag> {
    let mut flags = Vec::new();
    if facts.working {
        flags.push(Flag::new(FlagKind::Working, ""));
    }
    // An uncertain match.
    if let Some(review) = facts.review.as_ref().filter(|r| !r.no_candidates) {
        flags.push(Flag::new(FlagKind::NeedsReview, review.reason.clone()));
    }
    if let Some(review) = facts.review.as_ref().filter(|r| r.no_candidates) {
        flags.push(Flag::new(FlagKind::NotMatched, review.reason.clone()));
    } else if let (None, false, Some(miss)) =
        (&facts.review, facts.has_provider_id, facts.scrape_miss())
    {
        // A scrape was tried, found no confident match and no decision waits (the review was
        // set aside, or the provider failed). A title nobody tried to scrape has no flag.
        flags.push(Flag::new(FlagKind::NotMatched, scrape_miss_reason(miss)));
    }
    if facts.is_missing() {
        flags.push(Flag::new(
            FlagKind::Missing,
            if facts.availability == Availability::Offline {
                "The library folder cannot be reached."
            } else {
                "The files are missing."
            },
        ));
    }
    if facts.locked {
        flags.push(Flag::new(FlagKind::Locked, ""));
    } else if facts.is_scraped() {
        // A locked title is skipped by Organize, so it is never "Rename pending".
        match &facts.organize {
            Organize::Pending(changes) => {
                flags.push(Flag::new(FlagKind::RenamePending, changes.clone()));
            }
            Organize::Conflict(why) => {
                flags.push(Flag::new(FlagKind::RenamePending, why.clone()));
            }
            _ => {}
        }
    }
    flags
}

/// Whether Needs review counts the title: a match waiting for a decision (uncertain, or a search
/// that found nothing).
#[must_use]
pub fn in_review(facts: &Facts, flags: &[Flag]) -> bool {
    facts.review.is_some() || flags.iter().any(|f| f.kind == FlagKind::NeedsReview)
}

/// The lifecycle of a title: stage, flags and the explanation lines.
#[must_use]
pub fn derive(facts: &Facts) -> Lifecycle {
    let flags = flags_of(facts);
    Lifecycle {
        stage: stage_of(facts),
        in_review: in_review(facts, &flags),
        flags,
        lines: lines_of(facts),
    }
}

#[cfg(test)]
mod tests;

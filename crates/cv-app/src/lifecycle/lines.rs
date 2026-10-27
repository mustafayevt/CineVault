//! The explanation lines of a stage meter: one line per step saying what happened and when, or
//! what happens next.
//!
//! A line is structured so the screen can format the date and show a status glyph next to each
//! part: `Scraped`, a date, then parts such as `TMDB 949`, `artwork saved`, `ratings 2 of 5`,
//! `collection saved`. The words are fixed here, in the three verbs of the app (Scan, Scrape,
//! Rename).

use cv_db::repo::step_outcome::{
    STEP_ARTWORK, STEP_COLLECTIONS, STEP_RATINGS, STEP_SCRAPE, StepOutcome, StepStatus,
};
use cv_domain::ItemKind;

use super::{Facts, NOT_SCRAPED_YET, Organize, scrape_miss_reason};
use crate::wording::counted;

/// The step a line is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineStep {
    Scan,
    Scrape,
    Rename,
}

impl LineStep {
    /// The stable text used in IPC.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Scrape => "scrape",
            Self::Rename => "rename",
        }
    }
}

/// The status of a line or a part, in the shared status vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineStatus {
    Done,
    Waiting,
    Missing,
    NeedsKey,
    InProgress,
    Off,
}

impl LineStatus {
    /// The stable text used in IPC.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Waiting => "waiting",
            Self::Missing => "missing",
            Self::NeedsKey => "needs-key",
            Self::InProgress => "in-progress",
            Self::Off => "off",
        }
    }
}

/// Where a line can send the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineLink {
    /// Preview the pending rename in Organize.
    OrganizePreview,
    /// Resolve a conflict in Organize.
    OrganizeResolve,
}

impl LineLink {
    /// The stable text used in IPC.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OrganizePreview => "organize-preview",
            Self::OrganizeResolve => "organize-resolve",
        }
    }
}

/// One part of a line, with the status of what it names when it names a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinePart {
    pub text: String,
    pub status: Option<LineStatus>,
}

impl LinePart {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            status: None,
        }
    }

    fn with(text: impl Into<String>, status: LineStatus) -> Self {
        Self {
            text: text.into(),
            status: Some(status),
        }
    }
}

/// One line of the explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub step: LineStep,
    pub status: LineStatus,
    /// What happened, or what is next: `Scanned`, `Scraped`, `Renamed: no`.
    pub label: String,
    /// When it happened, epoch milliseconds.
    pub at_ms: Option<i64>,
    pub parts: Vec<LinePart>,
    pub link: Option<LineLink>,
}

/// The three lines of a title: Scan, Scrape and Rename.
#[must_use]
pub fn lines_of(facts: &Facts) -> Vec<Line> {
    vec![scan_line(facts), scrape_line(facts), rename_line(facts)]
}

fn library_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Movie | ItemKind::Set => "Movies",
        ItemKind::Show | ItemKind::Season | ItemKind::Episode => "TV shows",
    }
}

fn scan_line(facts: &Facts) -> Line {
    let name = if facts.year > 0 {
        format!("{} ({})", facts.title, facts.year)
    } else {
        facts.title.clone()
    };
    // Where the title is now, by its library folder's label, as the Location line says it: a
    // move to another folder or a rename shows here at once.
    let place = facts
        .location
        .clone()
        .unwrap_or_else(|| format!("{} \u{203a} {name}", library_name(facts.kind)));
    Line {
        step: LineStep::Scan,
        status: LineStatus::Done,
        label: "Scanned".to_owned(),
        at_ms: (facts.added_ms > 0).then_some(facts.added_ms),
        parts: vec![LinePart::plain(place)],
        link: None,
    }
}

fn outcome<'a>(facts: &'a Facts, step: &str) -> Option<&'a StepOutcome> {
    facts.outcomes.iter().find(|o| o.step == step)
}

fn status_of(status: StepStatus) -> LineStatus {
    match status {
        StepStatus::Done | StepStatus::Unchanged => LineStatus::Done,
        StepStatus::Skipped => LineStatus::Off,
        StepStatus::NeedsKey => LineStatus::NeedsKey,
        StepStatus::NotFound | StepStatus::Failed => LineStatus::Missing,
    }
}

fn scrape_line(facts: &Facts) -> Line {
    if !facts.is_scraped() {
        return unscraped_line(facts);
    }
    // An earlier scrape that found nothing is not when this one happened.
    let at = facts
        .outcomes
        .iter()
        .filter(|o| !(o.step == STEP_SCRAPE && o.status.is_scrape_miss()))
        .map(|o| o.at_ms)
        .min()
        .or((facts.updated_ms > 0).then_some(facts.updated_ms));
    let mut parts = Vec::new();
    if let Some(provider) = &facts.provider {
        parts.push(LinePart::plain(format!(
            "{} {}",
            provider.label, provider.id
        )));
    }
    parts.push(artwork_part(facts));
    parts.push(ratings_part(facts));
    if facts.kind == ItemKind::Movie {
        parts.extend(collection_part(facts));
    }
    Line {
        step: LineStep::Scrape,
        status: if facts.working {
            LineStatus::InProgress
        } else {
            LineStatus::Done
        },
        label: "Scraped".to_owned(),
        at_ms: at,
        parts,
        link: None,
    }
}

fn unscraped_line(facts: &Facts) -> Line {
    let miss = facts.scrape_miss();
    let (status, label, parts) = match &facts.review {
        _ if facts.working => (LineStatus::InProgress, "Scraping now", Vec::new()),
        Some(review) if review.no_candidates => (
            LineStatus::Missing,
            "Not matched",
            vec![LinePart::plain(review.reason.clone())],
        ),
        Some(review) => (
            LineStatus::Waiting,
            NOT_SCRAPED_YET,
            vec![LinePart::plain(format!("needs review: {}", review.reason))],
        ),
        None => match miss.filter(|_| !facts.has_provider_id) {
            // A scrape was tried and found no confident match; nothing waits for a decision.
            Some(miss) => (
                status_of(miss.status),
                "Not matched",
                vec![LinePart::plain(scrape_miss_reason(miss))],
            ),
            None => (LineStatus::Waiting, NOT_SCRAPED_YET, Vec::new()),
        },
    };
    // When the scrape that found nothing ran.
    let at_ms = if label == "Not matched" {
        miss.map(|m| m.at_ms)
    } else {
        None
    };
    Line {
        step: LineStep::Scrape,
        status,
        label: label.to_owned(),
        at_ms,
        parts,
        link: None,
    }
}

/// The word for an extra that has an outcome, or that is off, or that has not run yet.
fn extra_part(facts: &Facts, step: &str, name: &str, on: bool) -> LinePart {
    match outcome(facts, step) {
        Some(row) => {
            let word = match row.status {
                StepStatus::Done => "saved".to_owned(),
                StepStatus::Unchanged => "complete".to_owned(),
                StepStatus::Skipped => "skipped".to_owned(),
                StepStatus::NeedsKey => "needs a key".to_owned(),
                StepStatus::NotFound => "not found".to_owned(),
                StepStatus::Failed => "failed".to_owned(),
            };
            LinePart::with(format!("{name} {word}"), status_of(row.status))
        }
        None if !on => LinePart::with(format!("{name} off"), LineStatus::Off),
        None if facts.working => {
            LinePart::with(format!("{name} fetching\u{2026}"), LineStatus::InProgress)
        }
        None => LinePart::with(format!("{name} waiting"), LineStatus::Waiting),
    }
}

fn artwork_part(facts: &Facts) -> LinePart {
    extra_part(facts, STEP_ARTWORK, "artwork", true)
}

/// A collection is only mentioned once the step ran: most movies belong to none.
fn collection_part(facts: &Facts) -> Option<LinePart> {
    outcome(facts, STEP_COLLECTIONS)?;
    Some(extra_part(
        facts,
        STEP_COLLECTIONS,
        "collection",
        facts.extras.collections,
    ))
}

/// `ratings 2 of 5` from the per-source outcomes, or the step's word when no source was asked.
fn ratings_part(facts: &Facts) -> LinePart {
    let prefix = format!("{STEP_RATINGS}:");
    let sources: Vec<&StepOutcome> = facts
        .outcomes
        .iter()
        .filter(|o| o.step.starts_with(&prefix))
        .collect();
    if sources.is_empty() {
        return extra_part(facts, STEP_RATINGS, "ratings", facts.extras.ratings);
    }
    let found = sources
        .iter()
        .filter(|o| o.status == StepStatus::Done)
        .count();
    let status = if found == sources.len() {
        LineStatus::Done
    } else if sources.iter().any(|o| o.status == StepStatus::Failed) {
        LineStatus::Missing
    } else if sources.iter().any(|o| o.status == StepStatus::NeedsKey) {
        LineStatus::NeedsKey
    } else {
        LineStatus::Waiting
    };
    LinePart::with(format!("ratings {found} of {}", sources.len()), status)
}

fn rename_line(facts: &Facts) -> Line {
    let line = |status, label: &str, parts: Vec<LinePart>, link| Line {
        step: LineStep::Rename,
        status,
        label: label.to_owned(),
        at_ms: None,
        parts,
        link,
    };
    if !facts.is_scraped() {
        return line(LineStatus::Off, "Renamed after scraping", Vec::new(), None);
    }
    if facts.locked {
        return line(
            LineStatus::Off,
            "Renamed: skipped",
            vec![LinePart::plain("locked")],
            None,
        );
    }
    match &facts.organize {
        Organize::Checking => line(
            LineStatus::InProgress,
            "Renamed: checking\u{2026}",
            Vec::new(),
            None,
        ),
        Organize::Tidy => line(
            LineStatus::Done,
            "Renamed",
            vec![LinePart::plain("the names match the naming style")],
            None,
        ),
        Organize::Locked => line(
            LineStatus::Off,
            "Renamed: skipped",
            vec![LinePart::plain("locked")],
            None,
        ),
        Organize::Pending(changes) => {
            let parts = vec![
                LinePart::plain(format!("{changes} pending in Organize")),
                LinePart::plain("nothing moves until you apply it"),
            ];
            line(
                LineStatus::Waiting,
                "Renamed: no",
                parts,
                Some(LineLink::OrganizePreview),
            )
        }
        Organize::Conflict(why) => {
            let parts = vec![LinePart::plain(format!("conflict: {why}"))];
            line(
                LineStatus::Missing,
                "Renamed: no",
                parts,
                Some(LineLink::OrganizeResolve),
            )
        }
        Organize::Unplanned(why) => line(
            LineStatus::Missing,
            "Renamed: no",
            vec![LinePart::plain(why.clone())],
            None,
        ),
    }
}

/// A short count sentence for the changes of a pending rename: `1 change`, `3 changes`.
#[must_use]
pub fn changes_text(count: usize) -> String {
    counted(count.max(1), "change", "changes")
}

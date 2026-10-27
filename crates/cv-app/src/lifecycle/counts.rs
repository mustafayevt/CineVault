//! The pipeline counts of a library type, in one grouped SQL pass.
//!
//! The sidebar badges, Home, Needs review, Organize, the library filter chips and Activity all
//! read this one result, so no two places can show different numbers for the same set. The
//! rules are those of [`super::derive`]: each group of titles that share every relevant fact is
//! turned into [`Facts`] and asked for its stage and flags.

use cv_db::Database;
use cv_db::repo::lifecycle::{ReviewGroup, lifecycle_groups};
use cv_db::repo::organize_state::OrganizeStateKind;
use cv_db::repo::step_outcome::{STEP_SCRAPE, StepOutcome};
use cv_domain::ItemKind;

use super::{Facts, FlagKind, Organize, Review, Stage, flags_of, in_review, stage_of};

/// How many titles of one library type are in each stage and carry each flag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LifecycleCounts {
    pub total: u32,
    pub new: u32,
    pub scraped: u32,
    pub renamed: u32,
    /// Uncertain matches and titles whose search found nothing.
    pub needs_review: u32,
    /// Matches that need a decision (candidates exist).
    pub uncertain: u32,
    /// Titles a scrape was tried for that found no confident match (the Not matched flag): the
    /// search found nothing, or the last scrape's stored outcome is a miss and no decision waits.
    pub not_matched: u32,
    /// New titles waiting for their first scrape: no match, no flag and nothing in Needs review.
    pub not_scraped: u32,
    /// The titles of `not_matched` whose search found nothing: Needs review counts these.
    pub could_not_match: u32,
    /// The most common reason among the titles that are not matched, with its count.
    pub not_matched_top_reason: Option<(String, u32)>,
    pub working: u32,
    pub rename_pending: u32,
    pub locked: u32,
    pub missing: u32,
    /// Titles whose organize state is still being computed.
    pub checking: u32,
}

/// The pipeline counts of the movies or the shows of the library.
///
/// `profile_key` selects the cached organize state of the active naming profile. `working` is the number of titles a job is touching now; it lives in memory, so the
/// caller passes it in.
///
/// # Errors
/// The library cannot be read.
pub fn lifecycle_counts(
    db: &Database,
    kind: ItemKind,
    profile_key: &str,
    working: u32,
) -> Result<LifecycleCounts, cv_db::DbError> {
    let profile_key = profile_key.to_owned();
    let groups = db.read(move |conn| lifecycle_groups(conn, kind, &profile_key))?;
    let mut counts = LifecycleCounts {
        working,
        ..LifecycleCounts::default()
    };
    let mut reasons: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for group in groups {
        let mut facts = Facts::new(kind);
        facts.has_provider_id = group.has_id;
        facts.scraped_by_provider = group.scraped;
        facts.locked = group.locked;
        facts.availability = if group.missing {
            cv_domain::Availability::Missing
        } else {
            cv_domain::Availability::Online
        };
        facts.review = match group.review {
            ReviewGroup::None => None,
            ReviewGroup::Uncertain => Some(Review {
                reason: String::new(),
                no_candidates: false,
            }),
            ReviewGroup::NotMatched => Some(Review {
                reason: group.not_matched_reason.clone(),
                no_candidates: true,
            }),
        };
        if let Some((status, reason)) = &group.scrape_miss {
            facts.outcomes.push(StepOutcome {
                item: cv_domain::ItemId(0),
                step: STEP_SCRAPE.to_owned(),
                status: *status,
                reason: reason.clone(),
                at_ms: 0,
            });
        }
        facts.organize = match group.organize {
            None => Organize::Checking,
            Some(OrganizeStateKind::Tidy) => Organize::Tidy,
            Some(OrganizeStateKind::Pending) => Organize::Pending(String::new()),
            Some(OrganizeStateKind::Conflict) => Organize::Conflict(String::new()),
            Some(OrganizeStateKind::Locked) => Organize::Locked,
            Some(OrganizeStateKind::Unplanned) => Organize::Unplanned(String::new()),
        };

        let n = group.count;
        counts.total += n;
        match stage_of(&facts) {
            Stage::New => counts.new += n,
            Stage::Scraped => counts.scraped += n,
            Stage::Renamed => counts.renamed += n,
        }
        if facts.is_scraped() && facts.organize == Organize::Checking {
            counts.checking += n;
        }
        let flags = flags_of(&facts);
        let has = |kind| flags.iter().any(|f| f.kind == kind);
        let uncertain = group.review == ReviewGroup::Uncertain;
        let could_not_match = group.review == ReviewGroup::NotMatched;
        if uncertain {
            counts.uncertain += n;
        }
        if could_not_match {
            counts.could_not_match += n;
        }
        if let Some(flag) = flags.iter().find(|f| f.kind == FlagKind::NotMatched) {
            counts.not_matched += n;
            *reasons.entry(flag.reason.clone()).or_default() += n;
        } else if stage_of(&facts) == Stage::New && !in_review(&facts, &flags) {
            counts.not_scraped += n;
        }
        // One title counts once, however many reasons it has: the rule of `Lifecycle::in_review`.
        if in_review(&facts, &flags) {
            counts.needs_review += n;
        }
        if has(FlagKind::RenamePending) {
            counts.rename_pending += n;
        }
        if has(FlagKind::Locked) {
            counts.locked += n;
        }
        if has(FlagKind::Missing) {
            counts.missing += n;
        }
    }
    counts.not_matched_top_reason = reasons
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)));
    Ok(counts)
}

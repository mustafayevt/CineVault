//! Automatic matching decisions: accept a confident candidate or queue the item for review.
//!
//! [`resolve_match`] takes candidates ranked by `cv-match`, applies the acceptance rules
//! (threshold, tie rejection, and the same rule for TV shows) and then either
//!
//! - scrapes the top candidate through the one scrape write path
//!   ([`cv_db::repo::apply_scrape`] with the user's field selection, so item locks, field locks and
//!   do-not-overwrite are honoured), records the provider and language, completes the missing
//!   provider IDs and closes any pending review row; or
//! - writes a review-queue row with the reason, the candidates and the score breakdown, and never
//!   waits for the user.
//!
//! An item the user ignored earlier is left alone. Accepting a chosen candidate (inbox, bulk
//! accept, force best match) uses [`accept_candidate`] directly, without the rules.
//!
//! Every scrape stores how it ended as the item's `scrape` step outcome, in the same transaction
//! as its review row or its details ([`record_scrape`]): `done` once a match is written,
//! `not_found` when the search found no confident match, `needs_key` when the provider has no
//! key. That is how the lifecycle tells a title no scrape was tried for (just New) from one a
//! scrape found nothing for (Not matched), the same after a restart.

pub mod batch;
pub mod search;

use std::collections::BTreeMap;
use std::sync::Arc;

use cv_db::repo::review::{self, EnqueueOutcome, NewReview};
use cv_db::repo::step_outcome::{STEP_SCRAPE, StepStatus, record_step_outcome};
use cv_db::repo::{self, get_item, update_item};
use cv_db::{Database, DbError};
use cv_domain::{Item, ItemId, ItemKind, ProviderIds, ScrapeOutcome};
use cv_match::{
    AcceptRules, Candidate, Decision, ReviewDraft, ReviewReason, ScoredCandidate, decide,
};
use cv_providers::completion::AddedId;
use cv_providers::model::{MediaKind, RequestOptions, SearchHit};
use cv_providers::{
    AnimeCrossReference, MetadataProvider, ProviderError, UNIVERSAL_ID, complete_ids,
};

use serde_json::Value;

use crate::anime_crosswalk::AnimeCrosswalk;
use crate::episode_scrape::scrape_after_show;
use crate::scrape_settings::ScrapeSettings;

/// Called with the item after a scrape was written to it and its provider IDs were completed.
/// The app uses it to start the after-scrape sync; matching knows nothing of sync.
#[derive(Clone)]
pub struct ScrapedHook(pub std::sync::Arc<dyn Fn(ItemId) + Send + Sync>);

impl std::fmt::Debug for ScrapedHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScrapedHook")
    }
}

/// Everything a matching decision needs besides the item and its candidates.
#[derive(Debug, Clone, Default)]
pub struct MatchSettings {
    /// The auto-accept threshold.
    pub rules: AcceptRules,
    /// The fields a scrape may write, and the do-not-overwrite mode.
    pub scrape: ScrapeSettings,
    /// Language and country of the provider requests.
    pub request: RequestOptions,
    /// Told about every item a scrape was written to.
    pub on_scraped: Option<ScrapedHook>,
    /// The anime ID cross-reference, loaded on the first anime scrape. `None` completes IDs
    /// from the providers only.
    pub anime: Option<AnimeCrosswalk>,
}

/// The settings key of the auto-accept threshold: a number from 0 to 1, or a percentage from 1 to
/// 100. Missing or not a number means [`cv_match::DEFAULT_THRESHOLD`].
pub const THRESHOLD_KEY: &str = "matching.auto-accept-threshold";

impl MatchSettings {
    /// Reads the matching, scrape and language settings; anything missing takes its default.
    ///
    /// | Key | Meaning |
    /// |---|---|
    /// | `matching.auto-accept-threshold` | confidence at or above which the top candidate is accepted |
    /// | `metadata.language`, `metadata.fallback-language` | provider request languages |
    /// | `metadata.country` | country code for certifications (default `US`) |
    /// | `metadata.local-release-date` | use that country's release date instead of the original one (default off) |
    ///
    /// plus the scrape keys of [`ScrapeSettings`].
    pub fn from_values(values: &BTreeMap<String, Value>) -> Self {
        let mut request = RequestOptions::default();
        for (key, target) in [
            ("metadata.language", &mut request.language),
            ("metadata.fallback-language", &mut request.fallback_language),
        ] {
            if let Some(text) = values.get(key).and_then(Value::as_str)
                && !text.trim().is_empty()
            {
                text.trim().clone_into(target);
            }
        }
        if let Some(code) = values.get("metadata.country").and_then(Value::as_str)
            && !code.trim().is_empty()
        {
            request.country = code.trim().to_ascii_uppercase();
        }
        request.local_release_date = values
            .get("metadata.local-release-date")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let threshold = values.get(THRESHOLD_KEY).and_then(Value::as_f64);
        Self {
            rules: threshold.map_or_else(AcceptRules::default, |t| {
                AcceptRules::new(if t > 1.0 { t / 100.0 } else { t })
            }),
            scrape: ScrapeSettings::from_values(values),
            request,
            on_scraped: None,
            anime: None,
        }
    }

    /// The anime cross-reference for a title with `ids`, when it is an anime and the
    /// cross-reference can be loaded.
    pub async fn anime_for(&self, ids: &ProviderIds) -> Option<Arc<AnimeCrossReference>> {
        self.anime.as_ref()?.for_title(ids).await
    }

    /// Tells the hook, if there is one, that a scrape was written to `item`.
    pub fn notify_scraped(&self, item: ItemId) {
        if let Some(hook) = &self.on_scraped {
            (hook.0)(item);
        }
    }
}

/// A candidate that was scraped into the item.
#[derive(Debug, Clone, PartialEq)]
pub struct Accepted {
    pub candidate: ScoredCandidate,
    /// What the scrape wrote, kept and could not write. `blocked` is set when the item is locked
    /// or of another kind; the review row stays open then.
    pub scrape: ScrapeOutcome,
    /// Provider IDs that ID completion added.
    pub ids_added: Vec<AddedId>,
}

/// What [`resolve_match`] did with an item.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchResolution {
    /// The top candidate passed the rules and was scraped.
    Accepted(Box<Accepted>),
    /// The item waits in the Needs review inbox.
    Queued {
        review_id: i64,
        reason: ReviewReason,
        /// `true` when the item was already pending and its row was refreshed.
        refreshed: bool,
    },
    /// The user ignored the item earlier ("not a catalogued title"); nothing was done.
    Ignored,
}

/// Why a matching decision could not be applied.
#[derive(Debug, thiserror::Error)]
pub enum MatchError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("item {0} does not exist")]
    ItemNotFound(i64),
    #[error("only movies and shows are matched online, not {0:?}")]
    UnsupportedKind(ItemKind),
    #[error("no provider `{0}` is available to scrape the chosen candidate")]
    NoProvider(String),
}

/// A candidate from a search hit. Runtime and original language are not part of a hit; fill them
/// from the provider's details (see [`with_details`]) before scoring when evidence matters.
pub fn candidate_from_hit(hit: &SearchHit) -> Candidate {
    Candidate {
        provider: hit.provider.clone(),
        title: hit.title.clone(),
        original_title: hit.original_title.clone(),
        year: hit.year,
        has_poster: hit.poster_url.as_deref().is_some_and(|u| !u.is_empty()),
        poster_url: hit.poster_url.clone().filter(|u| !u.is_empty()),
        ids: hit.ids.clone(),
        runtime_minutes: None,
        original_language: None,
        exact_id: hit.exact_id,
        popularity: hit.popularity,
        votes: hit.votes,
        director: None,
    }
}

/// Adds the runtime from the provider's details of a candidate.
pub fn with_details(mut candidate: Candidate, details: &cv_domain::Item) -> Candidate {
    let minutes = match &details.detail {
        cv_domain::ItemDetail::Movie(m) => m.runtime_minutes,
        cv_domain::ItemDetail::Show(s) => s.runtime_minutes,
        _ => 0,
    };
    candidate.runtime_minutes = (minutes > 0).then_some(minutes);
    // Kept with the candidate, so tied candidates in Needs review show who directed them.
    candidate.director = search::director_of(details).or(candidate.director);
    candidate
}

fn media_kind(kind: ItemKind) -> Result<MediaKind, MatchError> {
    match kind {
        ItemKind::Movie => Ok(MediaKind::Movie),
        ItemKind::Show => Ok(MediaKind::Show),
        other => Err(MatchError::UnsupportedKind(other)),
    }
}

fn item_kind(db: &Database, item_id: ItemId) -> Result<ItemKind, MatchError> {
    let item = db
        .read(|c| get_item(c, item_id))?
        .ok_or(MatchError::ItemNotFound(item_id.0))?;
    Ok(item.kind())
}

/// Decides what to do with `ranked` candidates (best first, from `cv_match::rank_candidates`) and
/// does it.
///
/// A provider failure while scraping an accepted candidate is returned as an error and nothing is
/// queued: the item is simply not matched yet, and the caller (the match job) can retry.
///
/// # Errors
/// Fails on database errors, provider errors and unsupported item kinds.
pub async fn resolve_match(
    db: &Database,
    item_id: ItemId,
    ranked: &[ScoredCandidate],
    providers: &[&dyn MetadataProvider],
    settings: &MatchSettings,
) -> Result<MatchResolution, MatchError> {
    media_kind(item_kind(db, item_id)?)?;
    if db.read(|c| review::is_dismissed(c, item_id))? {
        return Ok(MatchResolution::Ignored);
    }
    match decide(ranked, &settings.rules) {
        Decision::Accept { .. } => {
            let accepted = accept_candidate(db, item_id, &ranked[0], providers, settings).await?;
            Ok(MatchResolution::Accepted(Box::new(accepted)))
        }
        Decision::Review(reason) => match queue_for_review(db, item_id, reason, ranked)? {
            EnqueueOutcome::Queued(review_id) => Ok(MatchResolution::Queued {
                review_id,
                reason,
                refreshed: false,
            }),
            EnqueueOutcome::Refreshed(review_id) => Ok(MatchResolution::Queued {
                review_id,
                reason,
                refreshed: true,
            }),
            EnqueueOutcome::Dismissed => Ok(MatchResolution::Ignored),
        },
    }
}

/// Writes the review-queue row of an item: the reason, the best score, up to five candidates and
/// the score breakdown, and flags the item `needs_review`.
///
/// # Errors
/// Fails when the item does not exist or the database fails.
pub fn queue_for_review(
    db: &Database,
    item_id: ItemId,
    reason: ReviewReason,
    ranked: &[ScoredCandidate],
) -> Result<EnqueueOutcome, MatchError> {
    let draft = ReviewDraft::new(reason, ranked);
    let new = NewReview {
        item_id,
        reason: draft.reason.text(),
        best_score: draft.best_score,
        candidates: serde_json::to_value(&draft.candidates).map_err(DbError::from)?,
        score_breakdown: draft
            .top_breakdown()
            .map(serde_json::to_value)
            .transpose()
            .map_err(DbError::from)?,
    };
    Ok(db.write(move |tx| queue_in(tx, &new, StepStatus::NotFound))?)
}

/// Writes the review row and stores the scrape as tried and not matched with the row's reason
/// (`status`), unless the item was ignored (then nothing is written).
fn queue_in(
    tx: &rusqlite::Connection,
    new: &NewReview,
    status: StepStatus,
) -> Result<EnqueueOutcome, DbError> {
    let outcome = review::enqueue(tx, new)?;
    if outcome != EnqueueOutcome::Dismissed {
        record_scrape(tx, new.item_id, status, &new.reason)?;
    }
    Ok(outcome)
}

/// Stores how the item's scrape ended (its `scrape` step outcome), replacing the last one: `done`
/// when a match was written, otherwise a miss with its reason (see `StepStatus::is_scrape_miss`).
///
/// # Errors
/// The database fails.
pub fn record_scrape(
    tx: &rusqlite::Connection,
    item_id: ItemId,
    status: StepStatus,
    reason: &str,
) -> Result<(), DbError> {
    record_step_outcome(
        tx,
        item_id,
        STEP_SCRAPE,
        status,
        reason,
        crate::step_outcomes::now_ms(),
    )
}

/// Queues an item whose scraping provider has no key: the inbox says "No TMDB key" (with the
/// provider's name), lists no candidates and flags the item `needs_review`.
///
/// # Errors
/// Fails when the item does not exist or the database fails.
pub fn queue_without_key(
    db: &Database,
    item_id: ItemId,
    provider: &str,
) -> Result<EnqueueOutcome, MatchError> {
    let new = NewReview {
        item_id,
        reason: format!("No {provider} key"),
        best_score: None,
        candidates: serde_json::Value::Array(Vec::new()),
        score_breakdown: None,
    };
    Ok(db.write(move |tx| queue_in(tx, &new, StepStatus::NeedsKey))?)
}

/// Scrapes `candidate` into the item, whatever its score: the write path of an automatic match,
/// a picked candidate, a bulk accept and force best match.
///
/// The scrape honours the item lock, the field locks and do-not-overwrite. When it is not blocked,
/// the item's last scraper and language are recorded, missing provider IDs are completed, the
/// pending review row (if any) is closed as accepted and the item's review state is resolved.
/// A blocked scrape leaves the review row open.
///
/// # Errors
/// Fails when the item is not a movie or show, no provider can scrape the candidate, the
/// provider fails, or the database fails.
pub async fn accept_candidate(
    db: &Database,
    item_id: ItemId,
    candidate: &ScoredCandidate,
    providers: &[&dyn MetadataProvider],
    settings: &MatchSettings,
) -> Result<Accepted, MatchError> {
    let kind = item_kind(db, item_id)?;
    let media = media_kind(kind)?;
    let key = &candidate.candidate.provider;
    let provider = providers
        .iter()
        .find(|p| p.id() == key)
        .ok_or_else(|| MatchError::NoProvider(key.clone()))?;

    let scraped = provider
        .details(media, &candidate.candidate.ids, &settings.request)
        .await?;
    let scrape = apply_scraped(db, item_id, kind, scraped, provider.id(), settings)?;

    let mut ids_added = Vec::new();
    if scrape.blocked.is_none() {
        // The universal provider is built from the others; asking it too would repeat them.
        let askable: Vec<&dyn MetadataProvider> = providers
            .iter()
            .copied()
            .filter(|p| p.id() != UNIVERSAL_ID)
            .collect();
        if crate::collections::wanted_after_scrape(settings, kind) {
            crate::collections::assign_scraped_movie(db, &askable, item_id, &settings.request)
                .await;
        }
        let known = known_ids(db, item_id)?;
        let anime = settings.anime_for(&known).await;
        let done = complete_ids(media, &known, &askable, anime.as_deref(), &settings.request).await;
        if !done.added.is_empty() {
            store_completed_ids(db, item_id, done.ids)?;
        }
        ids_added = done.added;
        if kind == ItemKind::Show {
            scrape_after_show(db, item_id, *provider, settings).await;
        }
        settings.notify_scraped(item_id);
    }
    Ok(Accepted {
        candidate: candidate.clone(),
        scrape,
        ids_added,
    })
}

/// The database half of a scrape: writes `scraped` into the item through the scrape write path
/// (item lock, field locks, do-not-overwrite), then, unless it was blocked, records the scraper
/// and language and closes the item's pending review row.
///
/// Synchronous, so a job calls it on a blocking thread.
pub(crate) fn apply_scraped(
    db: &Database,
    item_id: ItemId,
    kind: ItemKind,
    scraped: Item,
    provider_id: &str,
    settings: &MatchSettings,
) -> Result<ScrapeOutcome, DbError> {
    let provider_id = provider_id.to_owned();
    let settings = settings.clone();
    db.write(move |tx| apply_scraped_in(tx, item_id, kind, &scraped, &provider_id, &settings))
}

/// [`apply_scraped`] inside a transaction the caller holds, so a change of match can empty the
/// old title's data in the same transaction as the write: a refused or failed write then leaves
/// the item as it was.
pub(crate) fn apply_scraped_in(
    tx: &rusqlite::Connection,
    item_id: ItemId,
    kind: ItemKind,
    scraped: &Item,
    provider_id: &str,
    settings: &MatchSettings,
) -> Result<ScrapeOutcome, DbError> {
    let options = settings.scrape.options_for(kind);
    let language = &settings.request.language;
    let outcome = repo::apply_scrape(tx, item_id, scraped, &options)?;
    if outcome.blocked.is_none() {
        let Some(mut item) = get_item(tx, item_id)? else {
            return Err(DbError::ItemNotFound(item_id.0));
        };
        if item.core.last_scraper_id != provider_id || &item.core.last_scrape_language != language {
            item.core.last_scraper_id = provider_id.to_owned();
            item.core.last_scrape_language.clone_from(language);
            update_item(tx, &item)?;
        }
        review::close_accepted(tx, item_id)?;
        // A match was written: the title is no longer Not matched.
        record_scrape(tx, item_id, StepStatus::Done, "")?;
    }
    Ok(outcome)
}

/// The provider IDs the item has now.
pub(crate) fn known_ids(db: &Database, item_id: ItemId) -> Result<ProviderIds, MatchError> {
    Ok(db
        .read(|c| get_item(c, item_id))?
        .ok_or(MatchError::ItemNotFound(item_id.0))?
        .core
        .ids)
}

/// Adds the IDs that ID completion found, never replacing one the item has.
pub(crate) fn store_completed_ids(
    db: &Database,
    item_id: ItemId,
    ids: ProviderIds,
) -> Result<(), DbError> {
    db.write(move |tx| {
        let Some(mut item) = get_item(tx, item_id)? else {
            return Err(DbError::ItemNotFound(item_id.0));
        };
        for (key, value) in ids.iter() {
            if !item.core.ids.contains(key) {
                item.core.ids.set(key, value);
            }
        }
        update_item(tx, &item)
    })
}

/// Ignores an item: "not a catalogued title". It leaves the inbox and is never queued again.
/// Returns the review row that records it (the Undo target for `review::reopen`).
///
/// # Errors
/// Fails when the item does not exist or the database fails.
pub fn ignore_item(db: &Database, item_id: ItemId) -> Result<i64, MatchError> {
    Ok(db.write(move |tx| review::dismiss(tx, item_id))?)
}

/// The stored candidates of a review row, best first.
pub fn candidates_of(
    row: &review::ReviewRow,
) -> Result<Vec<cv_match::ReviewCandidate>, MatchError> {
    Ok(serde_json::from_value(row.candidates.clone()).map_err(DbError::from)?)
}

#[cfg(test)]
mod tests;

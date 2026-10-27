//! Finding candidates at a provider: the search shared by batch matching, the review inbox's
//! manual search and the search-and-scrape panel.
//!
//! The rules (spec: *ID-first identification*, *Query normalisation*):
//!
//! 1. An ID the item has, or one typed as a shortcut (`tt…`, `tmdb:…`, a provider URL), is looked
//!    up directly and the hit counts as an exact match.
//! 2. Otherwise the normalised title and year are searched; when that finds nothing and the title
//!    starts with a four digit number, the search runs once more without it.
//! 3. The hits are scored with `cv-match`, best first.

use cv_domain::ProviderIds;
use cv_match::{Candidate, LocalEvidence, NormalizedQuery, ScoredCandidate, rank_candidates};
use cv_providers::model::{MediaKind, RequestOptions, SearchHit, SearchQuery};
use cv_providers::{MetadataProvider, ProviderError};

use super::batch::DETAIL_LOOKUPS;
use super::{candidate_from_hit, with_details};

/// What [`find_hits`] found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FoundHits {
    pub hits: Vec<SearchHit>,
    /// The hit came from an ID lookup, with no text search.
    pub by_id: bool,
    /// How many text searches were sent to the provider.
    pub searches: u64,
}

fn search_query(media: MediaKind, query: &NormalizedQuery) -> SearchQuery {
    SearchQuery {
        kind: media,
        title: query.text.clone(),
        year: query.year,
    }
}

/// The hits for `query`. `known` holds the IDs the item already has; they are tried first, with
/// the query's own ID shortcut added.
///
/// # Errors
/// Fails with the provider's error; a title the provider does not know is an empty result.
pub async fn find_hits(
    provider: &dyn MetadataProvider,
    media: MediaKind,
    query: &NormalizedQuery,
    known: &ProviderIds,
    request: &RequestOptions,
) -> Result<FoundHits, ProviderError> {
    let mut ids = known.clone();
    if let Some(shortcut) = &query.id {
        ids.set(shortcut.provider, &shortcut.id);
    }
    let can_look_up = provider
        .capabilities()
        .lookup_ids
        .iter()
        .any(|key| ids.contains(key));
    if can_look_up {
        match provider.lookup(media, &ids, request).await {
            Ok(Some(mut hit)) => {
                hit.exact_id = true;
                return Ok(FoundHits {
                    hits: vec![hit],
                    by_id: true,
                    searches: 0,
                });
            }
            Ok(None) | Err(ProviderError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }
    if query.id.is_some() {
        // The query was an ID and the provider has no such title: a text search for the ID's
        // digits would find nothing useful.
        return Ok(FoundHits::default());
    }
    let mut searches = 1;
    let mut hits = provider
        .search(&search_query(media, query), request)
        .await?;
    if hits.is_empty()
        && let Some(retry) = query.retry_without_leading_year()
    {
        searches += 1;
        hits = provider
            .search(&search_query(media, &retry), request)
            .await?;
    }
    Ok(FoundHits {
        hits,
        by_id: false,
        searches,
    })
}

/// Scores `hits` against `query` and the local evidence, best first.
pub fn rank_hits(
    query: &NormalizedQuery,
    hits: &[SearchHit],
    evidence: &LocalEvidence,
) -> Vec<ScoredCandidate> {
    let candidates = hits.iter().map(candidate_from_hit).collect();
    rank_candidates(query, candidates, evidence)
}

/// A candidate with what the search and the provider's details add: the plot, the runtime (inside
/// the candidate) and the director.
#[derive(Debug, Clone, PartialEq)]
pub struct Detailed {
    pub scored: ScoredCandidate,
    pub director: Option<String>,
    /// The plot the search returned, when it has one.
    pub overview: Option<String>,
    /// The IDs the provider's details know that the search result did not carry (a TMDB search
    /// result has no IMDb ID, its details do). Empty for results whose details were not read.
    /// They let results from different providers be recognised as the same title.
    pub more_ids: ProviderIds,
}

/// Searches, scores and fills the runtime and director of the best [`DETAIL_LOOKUPS`] results
/// from the provider's details (the other results keep what the search gave). A candidate whose
/// details fail keeps its search data.
///
/// # Errors
/// Fails with the provider's error from the search itself.
pub async fn ranked_search(
    provider: &dyn MetadataProvider,
    media: MediaKind,
    query: &NormalizedQuery,
    known: &ProviderIds,
    evidence: &LocalEvidence,
    request: &RequestOptions,
) -> Result<Vec<Detailed>, ProviderError> {
    let found = find_hits(provider, media, query, known, request).await?;
    let ranked = rank_hits(query, &found.hits, evidence);
    let mut candidates: Vec<Candidate> = ranked.into_iter().map(|s| s.candidate).collect();
    let mut directors: Vec<(ProviderIds, String)> = Vec::new();
    let mut known_ids: Vec<(ProviderIds, ProviderIds)> = Vec::new();
    for candidate in candidates.iter_mut().take(DETAIL_LOOKUPS) {
        match provider.details(media, &candidate.ids, request).await {
            Ok(details) => {
                known_ids.push((candidate.ids.clone(), details.core.ids.clone()));
                if let Some(director) = director_of(&details) {
                    directors.push((candidate.ids.clone(), director));
                }
                *candidate = with_details(candidate.clone(), &details);
            }
            Err(error) => tracing::debug!(%error, "candidate details unavailable"),
        }
    }
    // Ranking again with the runtimes can reorder; keep each director with its candidate.
    let ranked = rank_candidates(query, candidates, evidence);
    Ok(ranked
        .into_iter()
        .map(|scored| {
            let director = directors
                .iter()
                .find(|(ids, _)| *ids == scored.candidate.ids)
                .map(|(_, director)| director.clone());
            let overview = found
                .hits
                .iter()
                .find(|hit| hit.ids == scored.candidate.ids)
                .map(|hit| hit.overview.trim().to_owned())
                .filter(|text| !text.is_empty());
            let more_ids = known_ids
                .iter()
                .find(|(ids, _)| *ids == scored.candidate.ids)
                .map(|(own, details)| {
                    let mut more = ProviderIds::new();
                    for (key, value) in details.iter() {
                        if !own.contains(key) {
                            more.set(key, value);
                        }
                    }
                    more
                })
                .unwrap_or_default();
            Detailed {
                scored,
                director,
                overview,
                more_ids,
            }
        })
        .collect())
}

/// The first director credit of a scraped item.
pub fn director_of(item: &cv_domain::Item) -> Option<String> {
    item.core
        .credits
        .iter()
        .find(|c| c.kind == cv_domain::CreditKind::Director)
        .map(|c| c.person.name.clone())
        .filter(|name| !name.trim().is_empty())
}

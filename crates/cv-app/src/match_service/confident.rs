//! The Scrape page's additions (spec `title-matching`): the search-only pass for New titles, the
//! current match as a scored candidate, and "Accept all confident".
//!
//! * **Search only.** [`MatchService::search_only`] searches the default provider (then the
//!   fallback chain) for a title and scores the results against the file. It writes nothing: no
//!   title, NFO, artwork, stage or review row changes. The ranked result is kept in memory so
//!   Accept all confident acts on the candidates the page showed.
//! * **Current match.** [`MatchService::current_match`] fetches the title the item is linked to
//!   by its TMDB or IMDb ID and scores it like the other candidates, for the match picker to pin.
//! * **Accept all confident.** [`MatchService::accept_confident`] scrapes every title whose top
//!   candidate is confident (`cv_match::is_confident`: 90 percent and not tied) with that
//!   candidate. It only runs when the user asks; each accepted title returns the token that
//!   `undo_scrape` takes back.

use cv_db::repo::review;
use cv_domain::{ItemId, ProviderIds};
use cv_match::{ScoredCandidate, is_confident, normalize_query};
use cv_providers::model::SearchQuery;

use super::history::label_of;
use super::{
    BULK_PARALLELISM, CandidateView, EvidenceContext, MatchService, MatchServiceError,
    ServiceResult, accept_one, candidates_of, detailed_view, folder_name, local_evidence,
    media_kind, request_for,
};
use crate::history::snapshot::TitleSnapshot;
use crate::matching::search::ranked_search;

/// How many searched titles the service remembers for Accept all confident.
const SEARCHED_KEEP: usize = 2000;

/// What the search-only pass found for one title.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchOnly {
    /// Best first.
    pub candidates: Vec<CandidateView>,
    /// The top candidate scores 90 percent or more and is not tied (Accept all confident takes it).
    pub confident: bool,
}

/// A title that Accept all confident scraped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfidentAccepted {
    pub item: ItemId,
    /// Pass to `undo_scrape` to put the title back as it was; `None` when it cannot be undone or
    /// when the title waited in the review queue (then `review_id` is its Undo).
    pub undo: Option<i64>,
    /// The review row the title was accepted from; `reopen` puts both the row and the title back.
    pub review_id: Option<i64>,
}

/// What Accept all confident did with the titles it was given.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfidentOutcome {
    pub accepted: Vec<ConfidentAccepted>,
    /// Titles with no confident top candidate (or ignored ones): left as they were.
    pub skipped: Vec<ItemId>,
    /// Titles whose scrape failed or was refused (locked): left as they were.
    pub failed: Vec<ItemId>,
    /// The one History entry that records the accepted titles; its Undo takes them all back.
    pub history_entry: Option<i64>,
}

impl MatchService {
    /// Searches for a New title and scores the results, writing nothing.
    ///
    /// `query` is the title text, with its year in brackets when it has one; an ID or link works
    /// too. An empty text uses the item's own title and year.
    ///
    /// # Errors
    /// The item is gone or not a movie or show, no provider is ready, or the provider fails.
    pub async fn search_only(
        &self,
        item_id: ItemId,
        query: &str,
        language: &str,
    ) -> ServiceResult<SearchOnly> {
        let item = self.item(item_id)?;
        let media = media_kind(item.kind())?;
        let context = self.context();
        let provider = context
            .providers
            .default_for(media)
            .cloned()
            .ok_or(MatchServiceError::NoProvider)?;
        let request = request_for(&context.settings, language);
        let text = if query.trim().is_empty() {
            own_query(&item)
        } else {
            query.trim().to_owned()
        };
        let mut found = self
            .search_item(&item, provider.as_ref(), media, &text, &request)
            .await?;
        if found.is_empty()
            && let Some((fallback, _)) = context
                .providers
                .fallback()
                .search_after(
                    provider.id(),
                    &SearchQuery {
                        kind: media,
                        title: normalize_query(&text).text,
                        year: None,
                    },
                    &request,
                )
                .await
        {
            found = self
                .search_item(&item, fallback.as_ref(), media, &text, &request)
                .await?;
        }
        let ranked: Vec<ScoredCandidate> = found.iter().map(|d| d.scored.clone()).collect();
        let confident = is_confident(&ranked);
        self.remember_search(item_id, ranked);
        let evidence = EvidenceContext::of(&item);
        Ok(SearchOnly {
            candidates: found
                .iter()
                .map(|d| detailed_view(d, &evidence, &context.providers))
                .collect(),
            confident,
        })
    }

    /// The title the item is linked to, as a candidate scored like the search results, for the
    /// match picker to pin first. `None` when the item has no TMDB or IMDb ID the default
    /// provider can look up, or the provider does not know it.
    ///
    /// # Errors
    /// The item is gone or not a movie or show, or the provider fails.
    pub async fn current_match(
        &self,
        item_id: ItemId,
        language: &str,
    ) -> ServiceResult<Option<CandidateView>> {
        let item = self.item(item_id)?;
        let media = media_kind(item.kind())?;
        let context = self.context();
        let Some(provider) = context.providers.default_for(media).cloned() else {
            return Ok(None);
        };
        let lookups = provider.capabilities().lookup_ids;
        let mut known = ProviderIds::new();
        for (key, value) in item.core.ids.iter() {
            if lookups.contains(&key) {
                known.set(key, value);
            }
        }
        if known.is_empty() {
            return Ok(None);
        }
        let request = request_for(&context.settings, language);
        let query = normalize_query(&own_query(&item));
        let evidence = local_evidence(&item);
        let found = match ranked_search(
            provider.as_ref(),
            media,
            &query,
            &known,
            &evidence,
            &request,
        )
        .await
        {
            Ok(found) => found,
            Err(error) => {
                self.fail(provider.id(), &error);
                return Err(MatchServiceError::Provider(error));
            }
        };
        let local = EvidenceContext::of(&item);
        Ok(found
            .iter()
            .find(|d| d.scored.candidate.exact_id)
            .map(|d| detailed_view(d, &local, &context.providers)))
    }

    /// Scrapes every given title that has a confident top candidate with that candidate (spec
    /// `title-matching` "Confident means 90 percent and clear"). The candidates are the ones the
    /// search-only pass found, else the ones of the title's review row, else a search now. A
    /// title without a confident candidate is skipped; a failed or locked one is reported and
    /// left as it was.
    ///
    /// # Errors
    /// The database cannot be read. Per-title failures are in the outcome.
    pub async fn accept_confident(&self, ids: &[ItemId]) -> ServiceResult<ConfidentOutcome> {
        let mut outcome = ConfidentOutcome::default();
        let mut work = Vec::new();
        for id in ids {
            match self.confident_top(*id).await {
                Ok(Some(top)) => work.push((*id, top)),
                Ok(None) => outcome.skipped.push(*id),
                Err(_) => outcome.failed.push(*id),
            }
        }
        let context = self.context();
        let mut queue = work.into_iter();
        let mut running = tokio::task::JoinSet::new();
        // The titles that were accepted, each as it was before, with its new name.
        let mut recorded: Vec<(TitleSnapshot, String)> = Vec::new();
        loop {
            while running.len() < BULK_PARALLELISM {
                let Some((id, top)) = queue.next() else {
                    break;
                };
                let context = context.clone();
                // A title that waits in the review queue is undone by reopening its row.
                let row = self
                    .db
                    .read(|c| review::pending_for_item(c, id))
                    .ok()
                    .flatten()
                    .map(|row| row.id);
                // The title is copied for the History entry first; when the candidate is another
                // title than the one it has, its NFO and artwork files go to the bin.
                let snapshot = match self.snapshot_for_match(id, Some(&top.candidate.ids), row) {
                    Ok(snapshot) => snapshot,
                    Err(_) => {
                        outcome.failed.push(id);
                        continue;
                    }
                };
                let (token, before) = match row {
                    Some(_) => (None, self.before_scrape(id)),
                    None => (self.keep_scrape_snapshot(id), None),
                };
                running.spawn(async move {
                    let done = accept_one(&context, id, &top).await;
                    (id, top, (token, row, before, snapshot), done)
                });
            }
            let Some(joined) = running.join_next().await else {
                break;
            };
            let Ok((id, top, (token, row, before, snapshot), done)) = joined else {
                continue;
            };
            match done {
                Ok(accepted) if accepted.scrape.blocked.is_none() => {
                    self.forget_search(id);
                    if let Some(row) = row {
                        self.keep_snapshot(row, before);
                    }
                    if let Some(snapshot) = snapshot {
                        recorded
                            .push((snapshot, label_of(&top.candidate.title, top.candidate.year)));
                    }
                    outcome.accepted.push(ConfidentAccepted {
                        item: id,
                        undo: token,
                        review_id: row,
                    });
                }
                other => {
                    if let Some(token) = token {
                        self.forget_scrape(token);
                    }
                    if let Some(snapshot) = &snapshot {
                        self.unbin_snapshot(snapshot);
                    }
                    if let Err(MatchServiceError::Provider(error)) = &other {
                        self.fail(&top.candidate.provider, error);
                    }
                    outcome.failed.push(id);
                }
            }
        }
        let count = recorded.len();
        outcome.history_entry = self.record_match(
            if count == 1 {
                "Accepted 1 confident match".to_owned()
            } else {
                format!("Accepted {count} confident matches")
            },
            if count == 1 {
                "1 title".to_owned()
            } else {
                format!("{count} titles")
            },
            recorded,
        );
        Ok(outcome)
    }

    /// The top candidate of a title when it is confident.
    async fn confident_top(&self, item_id: ItemId) -> ServiceResult<Option<ScoredCandidate>> {
        if self.db.read(|c| review::is_dismissed(c, item_id))? {
            return Ok(None);
        }
        let ranked = match self.searched(item_id) {
            Some(ranked) => ranked,
            None => match self.db.read(|c| review::pending_for_item(c, item_id))? {
                Some(row) => candidates_of(&row)?
                    .into_iter()
                    .map(ScoredCandidate::from)
                    .collect(),
                None => {
                    self.search_only(item_id, "", "").await?;
                    self.searched(item_id).unwrap_or_default()
                }
            },
        };
        Ok(is_confident(&ranked)
            .then(|| ranked.into_iter().next())
            .flatten())
    }

    fn remember_search(&self, item_id: ItemId, ranked: Vec<ScoredCandidate>) {
        let mut searched = self
            .searched
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if searched.len() >= SEARCHED_KEEP {
            searched.clear();
        }
        searched.insert(item_id, ranked);
    }

    fn searched(&self, item_id: ItemId) -> Option<Vec<ScoredCandidate>> {
        self.searched
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&item_id)
            .cloned()
    }

    fn forget_search(&self, item_id: ItemId) {
        self.searched
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&item_id);
    }
}

/// The text a title is searched with when none is typed: its title, with the year in brackets.
fn own_query(item: &cv_domain::Item) -> String {
    let title = if item.core.title.trim().is_empty() {
        folder_name(&item.core.path)
    } else {
        item.core.title.trim().to_owned()
    };
    if item.core.year > 0 {
        format!("{title} ({})", item.core.year)
    } else {
        title
    }
}

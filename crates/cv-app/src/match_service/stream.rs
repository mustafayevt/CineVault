//! Streaming scrape candidates: the Scrape workspace asks every
//! enabled metadata provider for the title at once and shows each provider's hits as it answers.
//!
//! * **Parallel.** Each provider is a [`ScrapeSource`] that runs its own search, with the usual
//!   scoring ([`crate::matching::search::ranked_search`]); a slow provider never holds back a
//!   quick one. A provider without its key is a source that answers "needs a key" at once.
//! * **Merged.** The hits of different providers that share an IMDb, TMDB or TVDB ID are one
//!   entry (union-find over the IDs, including the IDs the provider's details know). The entry
//!   keeps the highest confidence and lists every provider that has the title with its own ID.
//!   An entry has a stable `id` for the whole search: when a later provider adds to an entry that
//!   was sent already, the entry is sent again under the same `id` and the page replaces it; an
//!   entry that a later hit joined into another is named in `absorbs` and the page drops it.
//! * **Marked.** An entry that is linked to another library item says which one.
//! * **Explained.** When every provider has answered and no entry was found,
//!   [`ScrapeSummary::nothing_found`] says why: no provider is set up, no key, offline, or nothing
//!   found.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use cv_domain::{ItemId, ItemKind, ProviderIds, id_key};
use cv_match::normalize_query;
use cv_providers::model::{MediaKind, RequestOptions};
use cv_providers::{MetadataProvider, UNIVERSAL_ID};

use super::{
    CandidateView, EvidenceContext, LibraryLink, MatchProviders, MatchService, MatchServiceError,
    ServiceResult, detailed_view, local_evidence, media_kind, request_for,
};
use crate::artwork::stream::SourceAnswer;
use crate::matching::search::ranked_search;

/// The ID kinds that join two hits into one entry.
const JOINING_IDS: [&str; 3] = [id_key::IMDB, id_key::TMDB, id_key::TVDB];

/// How many of one provider's hits, the best first, a search lists.
pub const MAX_HITS_PER_SOURCE: usize = 25;

/// One provider that has an entry, with its own ID for it.
#[derive(Debug, Clone, PartialEq)]
pub struct HitProvider {
    pub provider: String,
    /// The provider's own ID for the title.
    pub provider_id: String,
    /// The confidence this provider's hit has, 0 to 1.
    pub confidence: f64,
}

/// One title, possibly found by several providers.
#[derive(Debug, Clone, PartialEq)]
pub struct MergedHit {
    /// Stable for the whole search; sending an entry again with the same `id` replaces it.
    pub id: String,
    /// The hit that leads the entry, with the entry's confidence (the highest of its providers):
    /// the most confident hit of a provider that answers in the scrape's language, else the
    /// most confident one. Choosing the entry scrapes from this hit's provider.
    pub candidate: CandidateView,
    /// Every provider that has it, the leading one first, then the most confident.
    pub providers: Vec<HitProvider>,
    /// The joining IDs the providers gave, such as `imdb:tt0069293`.
    pub ids: Vec<String>,
    /// The library item, other than the one being scraped, that is linked to this title.
    pub in_library: Option<LibraryLink>,
    /// Entries sent earlier that this one now includes; the page drops them.
    pub absorbs: Vec<String>,
}

impl MergedHit {
    /// The entry's confidence: the highest of its providers.
    #[must_use]
    pub fn confidence(&self) -> f64 {
        self.candidate.confidence
    }
}

/// What one source answered.
#[derive(Debug, Clone, PartialEq)]
pub enum ScrapeAnswer {
    /// The entries this provider's hits made or joined, the most confident first.
    Hits(Vec<MergedHit>),
    /// The provider needs a key the user has not set.
    NeedsKey,
    /// The provider's saved key was rejected.
    KeyRejected,
    /// The provider could not answer; `retryable` says whether asking again can help.
    Failed { reason: String, retryable: bool },
}

/// Why a search found nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NothingFound {
    /// No metadata provider handles this kind of title.
    NoProvider,
    /// Every provider needs a key that is not set (or a saved key was rejected).
    NoKey,
    /// No provider could be reached.
    Offline,
    /// The providers answered and know no such title.
    NotFound,
}

impl NothingFound {
    /// A stable word for the page to branch on.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NoProvider => "no-provider",
            Self::NoKey => "no-key",
            Self::Offline => "offline",
            Self::NotFound => "nothing-found",
        }
    }

    /// The sentence the Scrape workspace shows.
    #[must_use]
    pub fn message(self, query: &str) -> String {
        match self {
            Self::NoProvider => {
                "No metadata provider handles this kind of title. Check Settings, Scraping and providers."
                    .to_owned()
            }
            Self::NoKey => {
                "No provider can be asked yet: add a key in Settings, Scraping and providers.".to_owned()
            }
            Self::Offline => {
                "The providers could not be reached. Check the connection and search again."
                    .to_owned()
            }
            Self::NotFound => format!(
                "No provider knows a title called \u{201c}{}\u{201d}. Try a different search.",
                query.trim()
            ),
        }
    }
}

/// How a source ended, for the summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    Answered,
    NeedsKey,
    Unreachable,
    Other,
}

/// The shared state of one search: the merged entries and how each source ended.
#[derive(Default)]
struct Shared {
    merger: Mutex<Merger>,
    ended: Mutex<Vec<(String, Ended)>>,
}

/// What a finished search can say about itself.
#[derive(Clone)]
pub struct ScrapeSummary {
    shared: Arc<Shared>,
    sources: usize,
    query: String,
}

impl ScrapeSummary {
    /// How many entries the search has found so far.
    #[must_use]
    pub fn found(&self) -> usize {
        self.shared
            .merger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries
            .len()
    }

    /// Why nothing was found, when it was not. Only meaningful once every source has answered.
    #[must_use]
    pub fn nothing_found(&self) -> Option<(NothingFound, String)> {
        if self.found() > 0 {
            return None;
        }
        let ended = self
            .shared
            .ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let any = |wanted: Ended| ended.iter().any(|(_, e)| *e == wanted);
        let reason = if self.sources == 0 {
            NothingFound::NoProvider
        } else if any(Ended::Answered) {
            NothingFound::NotFound
        } else if any(Ended::Unreachable) {
            NothingFound::Offline
        } else if any(Ended::NeedsKey) {
            NothingFound::NoKey
        } else {
            NothingFound::NotFound
        };
        Some((reason, reason.message(&self.query)))
    }
}

/// The plan of a streaming scrape search.
pub struct ScrapeCandidateStream {
    /// The text searched for.
    pub query: String,
    /// The providers to ask, in the order of preference.
    pub sources: Vec<ScrapeSource>,
    summary: ScrapeSummary,
}

impl std::fmt::Debug for ScrapeCandidateStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrapeCandidateStream")
            .field("query", &self.query)
            .field("sources", &self.sources)
            .finish_non_exhaustive()
    }
}

impl ScrapeCandidateStream {
    /// What the search says about itself once every source has answered.
    #[must_use]
    pub fn summary(&self) -> ScrapeSummary {
        self.summary.clone()
    }
}

/// One provider of a scrape search.
#[derive(Clone)]
pub struct ScrapeSource {
    /// The provider's key (`tmdb`, `omdb`, ...).
    pub key: String,
    /// The provider's name as the interface shows it.
    pub name: String,
    how: Arc<Asks>,
}

impl ScrapeSource {
    /// Asks the provider. Called once per start and once per retry.
    pub async fn fetch(&self) -> ScrapeAnswer {
        let answer = self.how.ask(&self.key).await;
        let ended = match &answer {
            ScrapeAnswer::Hits(_) => Ended::Answered,
            ScrapeAnswer::NeedsKey | ScrapeAnswer::KeyRejected => Ended::NeedsKey,
            ScrapeAnswer::Failed {
                retryable: true, ..
            } => Ended::Unreachable,
            ScrapeAnswer::Failed { .. } => Ended::Other,
        };
        let mut all = self
            .how
            .shared
            .ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        all.retain(|(key, _)| *key != self.key);
        all.push((self.key.clone(), ended));
        answer
    }
}

impl std::fmt::Debug for ScrapeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrapeSource")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

/// What a source needs to ask its provider.
struct Asks {
    service: Arc<MatchService>,
    /// `None` for a provider that has no key yet.
    provider: Option<Arc<dyn MetadataProvider>>,
    providers: MatchProviders,
    media: MediaKind,
    item: ItemId,
    query: String,
    evidence: EvidenceContext,
    local: cv_match::LocalEvidence,
    request: RequestOptions,
    shared: Arc<Shared>,
}

impl Asks {
    async fn ask(&self, key: &str) -> ScrapeAnswer {
        let Some(provider) = &self.provider else {
            return ScrapeAnswer::NeedsKey;
        };
        let query = normalize_query(&self.query);
        let found = ranked_search(
            provider.as_ref(),
            self.media,
            &query,
            &ProviderIds::new(),
            &self.local,
            &self.request,
        )
        .await;
        let found = match found {
            Ok(found) => found,
            Err(error) => {
                return match SourceAnswer::of_error(&error) {
                    SourceAnswer::NeedsKey => ScrapeAnswer::NeedsKey,
                    SourceAnswer::KeyRejected => ScrapeAnswer::KeyRejected,
                    SourceAnswer::Failed { reason, retryable } => {
                        ScrapeAnswer::Failed { reason, retryable }
                    }
                    SourceAnswer::Candidates(_) => ScrapeAnswer::Hits(Vec::new()),
                };
            }
        };
        // A provider that answers in another language than the scrape's (OFDb's German plots
        // for an English library) never leads an entry another provider also returned.
        let serves_language = provider.serves_language(&self.request.language);
        let mut incoming = Vec::with_capacity(found.len().min(MAX_HITS_PER_SOURCE));
        // The best hits only: one site that lists hundreds of loose hits would flood the list.
        for detailed in found.iter().take(MAX_HITS_PER_SOURCE) {
            let view = detailed_view(detailed, &self.evidence, &self.providers);
            let mut keys = joining_keys(&detailed.scored.candidate.ids);
            keys.extend(joining_keys(&detailed.more_ids));
            let in_library = self
                .service
                .linked_item(
                    provider.as_ref(),
                    &detailed.scored.candidate.ids,
                    &view.provider_id,
                    self.item,
                )
                .unwrap_or_else(|error| {
                    tracing::debug!(%error, "library link not read");
                    None
                });
            incoming.push(Incoming {
                view,
                keys,
                in_library,
                serves_language,
            });
        }
        let hits = self
            .shared
            .merger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .add(key, incoming);
        ScrapeAnswer::Hits(hits)
    }
}

/// The joining IDs of `ids` as `key:value`.
fn joining_keys(ids: &ProviderIds) -> Vec<String> {
    JOINING_IDS
        .iter()
        .filter_map(|key| {
            ids.get(key)
                .map(|value| format!("{key}:{}", value.to_ascii_lowercase()))
        })
        .collect()
}

// ---- the merge ---------------------------------------------------------------------------------

/// A hit on its way into the merge.
struct Incoming {
    view: CandidateView,
    keys: Vec<String>,
    in_library: Option<LibraryLink>,
    /// The provider answers in the scrape's language.
    serves_language: bool,
}

struct Member {
    view: CandidateView,
    serves_language: bool,
}

struct EntryState {
    id: String,
    members: Vec<Member>,
    ids: BTreeSet<String>,
    in_library: Option<LibraryLink>,
    /// Entries merged into this one since it was last sent.
    absorbed: Vec<String>,
}

impl EntryState {
    fn hit(&mut self) -> MergedHit {
        let mut members: Vec<&Member> = self.members.iter().collect();
        members.sort_by(|a, b| {
            b.serves_language
                .cmp(&a.serves_language)
                .then(b.view.rank.total_cmp(&a.view.rank))
        });
        let mut best = members[0].view.clone();
        best.confidence = members
            .iter()
            .map(|m| m.view.confidence)
            .fold(best.confidence, f64::max);
        best.rank = members
            .iter()
            .map(|m| m.view.rank)
            .fold(best.rank, f64::max);
        let providers = members
            .iter()
            .map(|m| HitProvider {
                provider: m.view.provider.clone(),
                provider_id: m.view.provider_id.clone(),
                confidence: m.view.confidence,
            })
            .collect();
        MergedHit {
            id: self.id.clone(),
            candidate: best,
            providers,
            ids: self.ids.iter().cloned().collect(),
            in_library: self.in_library.clone(),
            absorbs: std::mem::take(&mut self.absorbed),
        }
    }
}

/// The merged entries of a search (union-find over the joining IDs, kept incrementally).
#[derive(Default)]
struct Merger {
    entries: Vec<EntryState>,
    next: usize,
}

impl Merger {
    /// Adds the hits of `provider` and returns the entries they made or joined, the most
    /// confident first. A provider asked again replaces its earlier hits.
    fn add(&mut self, provider: &str, hits: Vec<Incoming>) -> Vec<MergedHit> {
        let mut touched: Vec<String> = Vec::new();
        for hit in hits {
            let joined: Vec<usize> = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    hit.keys.iter().any(|key| entry.ids.contains(key))
                        || entry.members.iter().any(|m| {
                            m.view.provider == provider
                                && m.view.provider_id == hit.view.provider_id
                        })
                })
                .map(|(index, _)| index)
                .collect();
            let at = if let Some(&first) = joined.first() {
                // Later entries the hit bridges are folded into the oldest one.
                for &other in joined.iter().skip(1).rev() {
                    let gone = self.entries.remove(other);
                    let keep = &mut self.entries[first];
                    keep.members.extend(gone.members);
                    keep.ids.extend(gone.ids);
                    keep.in_library = keep.in_library.take().or(gone.in_library);
                    keep.absorbed.push(gone.id.clone());
                    keep.absorbed.extend(gone.absorbed);
                    touched.retain(|id| *id != gone.id);
                }
                first
            } else {
                self.next += 1;
                self.entries.push(EntryState {
                    id: format!("hit-{}", self.next),
                    members: Vec::new(),
                    ids: BTreeSet::new(),
                    in_library: None,
                    absorbed: Vec::new(),
                });
                self.entries.len() - 1
            };
            let entry = &mut self.entries[at];
            entry.members.retain(|m| {
                !(m.view.provider == hit.view.provider
                    && m.view.provider_id == hit.view.provider_id)
            });
            entry.members.push(Member {
                view: hit.view,
                serves_language: hit.serves_language,
            });
            entry.ids.extend(hit.keys);
            entry.in_library = entry.in_library.take().or(hit.in_library);
            if !touched.contains(&entry.id) {
                touched.push(entry.id.clone());
            }
        }
        let mut out: Vec<MergedHit> = self
            .entries
            .iter_mut()
            .filter(|entry| touched.contains(&entry.id))
            .map(EntryState::hit)
            .collect();
        out.sort_by(|a, b| b.candidate.rank.total_cmp(&a.candidate.rank));
        out
    }
}

// ---- the service -------------------------------------------------------------------------------

impl MatchService {
    /// Plans a streaming scrape search for `item` as a `kind` of title: every enabled metadata
    /// provider that handles the kind is a source, and so is every one that waits for a key (it
    /// answers "needs a key" at once). `query` is the typed text, an ID shortcut or a provider
    /// URL; blank searches for the item's own title. `language` overrides the metadata language
    /// when it is not blank.
    ///
    /// # Errors
    /// The item is gone or is not a movie or a show, or `kind` is not a kind of title.
    pub fn scrape_candidate_stream(
        self: &Arc<Self>,
        item_id: ItemId,
        query: &str,
        kind: ItemKind,
        language: &str,
    ) -> ServiceResult<ScrapeCandidateStream> {
        let item = self.item(item_id)?;
        // Only movies and shows are scraped; the search itself may be as the other kind.
        media_kind(item.kind())?;
        let media = media_kind(kind)?;
        let text = if query.trim().is_empty() {
            item.core.title.clone()
        } else {
            query.trim().to_owned()
        };
        if normalize_query(&text).text.is_empty() && normalize_query(&text).id.is_none() {
            return Err(MatchServiceError::Invalid(
                "Type a title or an ID to search for.".to_owned(),
            ));
        }
        let context = self.context();
        let request = request_for(&context.settings, language);
        let shared = Arc::new(Shared::default());
        let waiting = self.providers_waiting_for_a_key();
        let ready: Vec<Arc<dyn MetadataProvider>> = context
            .providers
            .supporting(media)
            .into_iter()
            .filter(|p| p.id() != UNIVERSAL_ID)
            .cloned()
            .collect();
        let local = local_evidence(&item);
        let asks = |provider: Option<Arc<dyn MetadataProvider>>| {
            Arc::new(Asks {
                service: Arc::clone(self),
                provider,
                providers: context.providers.clone(),
                media,
                item: item_id,
                query: text.clone(),
                evidence: EvidenceContext::of(&item),
                local: local.clone(),
                request: request.clone(),
                shared: Arc::clone(&shared),
            })
        };
        let mut sources: Vec<ScrapeSource> = ready
            .into_iter()
            .map(|provider| ScrapeSource {
                key: provider.id().to_owned(),
                name: provider.name().to_owned(),
                how: asks(Some(provider)),
            })
            .collect();
        // Providers that wait for a key are listed after the ready ones.
        for provider in self.providers.registry().metadata_providers() {
            let caps = provider.capabilities();
            let handles = match media {
                MediaKind::Movie => caps.movies,
                MediaKind::Show => caps.shows,
            };
            if handles && waiting.contains(&provider.id()) {
                sources.push(ScrapeSource {
                    key: provider.id().to_owned(),
                    name: provider.name().to_owned(),
                    how: asks(None),
                });
            }
        }
        let summary = ScrapeSummary {
            shared,
            sources: sources.len(),
            query: text.clone(),
        };
        Ok(ScrapeCandidateStream {
            query: text,
            sources,
            summary,
        })
    }

    /// The keys of the providers that are not ready for lack of a credential value.
    fn providers_waiting_for_a_key(&self) -> Vec<&'static str> {
        self.providers
            .list()
            .map(|statuses| {
                statuses
                    .into_iter()
                    .filter(|status| !status.active && !status.missing.is_empty())
                    .map(|status| status.provider)
                    .collect()
            })
            .unwrap_or_default()
    }
}

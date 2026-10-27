//! Artwork candidates for the streaming picker.
//!
//! [`ArtworkService::candidate_stream`] plans a search for one artwork type of an item: which
//! providers offer the type, and the image the item has now. Each provider is then asked on its
//! own through [`CandidateSource::fetch`], so a slow provider never holds back a quick one:
//!
//! * a provider that does not offer the type (a clear-art search leaves TMDB out) is not in the
//!   plan at all;
//! * a provider whose key the user has not set is in the plan as a source that answers
//!   [`SourceAnswer::NeedsKey`] without any request;
//! * an answer lists the provider's own best images first, already downloaded into the thumbnail
//!   cache so the interface draws them from `cv://img`;
//! * the image the item has now is pinned first, apart from the providers' lists.

use std::sync::Arc;

use async_trait::async_trait;
use cv_artwork::download::config::ArtworkConfig;
use cv_artwork::download::{candidate_kind, picker_slots};
use cv_artwork::frame_source::file_url;
use cv_artwork::quality::quality;
use cv_artwork::select::rank;
use cv_artwork::thumb::{ThumbCache, ThumbSize};
use cv_domain::{MediaFileType, ProviderIds};
use cv_providers::artwork::presort;
use cv_providers::model::{MediaKind, RequestOptions};
use cv_providers::{ArtworkProvider, ProviderError, SourcedArtwork};

use super::{ArtworkError, ArtworkService, MAX_PICKER_CANDIDATES, PickerCandidate};
use crate::matching::MatchSettings;

/// One artwork type of an item for the picker's type strip.
#[derive(Debug, Clone)]
pub struct SlotCount {
    pub slot: MediaFileType,
    /// How many images the providers offer for it.
    pub count: u32,
    /// The image the item has now for it.
    pub current: Option<PickerCandidate>,
}

/// The artwork types of a movie collection.
const COLLECTION_SLOTS: [MediaFileType; 2] = [MediaFileType::Poster, MediaFileType::Fanart];

/// The IDs a provider needs for the images of a collection: its TMDB ID as `tmdbSet`.
fn collection_ids(set: &cv_domain::Item) -> Option<ProviderIds> {
    let id = set.core.ids.get(cv_domain::id_key::TMDB)?;
    let mut ids = ProviderIds::new();
    ids.set(cv_domain::id_key::TMDB_SET, id);
    Some(ids)
}

/// How many of the candidates are of `kind`, as many as the picker lists for one provider.
fn counted(found: &[cv_providers::model::ArtworkCandidate], kind: MediaFileType) -> u32 {
    let n = found
        .iter()
        .filter(|c| c.kind == kind)
        .count()
        .min(MAX_PICKER_CANDIDATES);
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The `provider` of the image the item has now.
pub const CURRENT_PROVIDER: &str = "current";

/// The `reason` of a failed answer whose provider is only unavailable right now (it asked for a
/// human check): the interface says so quietly and does not offer Retry. The same word as the
/// `unavailable` provider state of the workspaces.
pub const UNAVAILABLE: &str = "unavailable";

/// What one source of candidates answered.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceAnswer {
    /// Its images, best fit first (possibly none).
    Candidates(Vec<PickerCandidate>),
    /// The provider needs a key the user has not set.
    NeedsKey,
    /// The provider saved key was rejected.
    KeyRejected,
    /// The provider could not answer; `retryable` says whether asking again can help.
    Failed { reason: String, retryable: bool },
}

impl SourceAnswer {
    /// The answer a provider error comes to.
    #[must_use]
    pub fn of_error(error: &ProviderError) -> Self {
        match error.root() {
            ProviderError::MissingCredentials { .. } => Self::NeedsKey,
            ProviderError::Auth { .. } => Self::KeyRejected,
            // The title is not there: nothing to offer, not a failure.
            ProviderError::NotFound(_) => Self::Candidates(Vec::new()),
            // A site that asks for a human check cannot be reached by a program: not a failure to
            // retry or to show in red, only "unavailable right now".
            _ if error.is_human_check() => Self::Failed {
                reason: UNAVAILABLE.to_owned(),
                retryable: false,
            },
            ProviderError::RateLimited { .. } | ProviderError::Request { .. } => Self::Failed {
                reason: error.to_string(),
                retryable: true,
            },
            _ => Self::Failed {
                reason: error.to_string(),
                retryable: false,
            },
        }
    }
}

/// Something that answers with candidates when asked. The online providers are one kind; the
/// video frame grabs are another.
#[async_trait]
pub trait CandidateFetch: Send + Sync {
    async fn fetch(&self) -> SourceAnswer;

    /// Like [`fetch`](Self::fetch), but hands the images over in batches as their thumbnails
    /// arrive: every batch but the last goes to `early`, the last one is the answer. A source
    /// that has nothing to hand over early answers as `fetch` does.
    async fn fetch_progressive(&self, early: &EarlyBatch<'_>) -> SourceAnswer {
        let _ = early;
        self.fetch().await
    }
}

/// Receives the earlier batches of a progressive fetch, best fit first.
pub type EarlyBatch<'a> = dyn Fn(Vec<PickerCandidate>) + Send + Sync + 'a;

/// How many images the first batch of a progressive fetch holds: what one round of thumbnail
/// downloads brings in, so the first tiles appear as soon as the first round is done.
pub const FIRST_BATCH: usize = 6;

/// How many images each later batch holds.
pub const LATER_BATCH: usize = 12;

/// One provider of a candidate search.
#[derive(Clone)]
pub struct CandidateSource {
    /// The provider's key (`tmdb`, `fanart`, `frames`).
    pub key: String,
    /// The provider's name as the interface shows it.
    pub name: String,
    /// Whether the work is CPU work (frame grabs), not a network request.
    pub cpu: bool,
    how: Arc<dyn CandidateFetch>,
}

impl CandidateSource {
    #[must_use]
    pub fn new(
        key: impl Into<String>,
        name: impl Into<String>,
        cpu: bool,
        how: Arc<dyn CandidateFetch>,
    ) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            cpu,
            how,
        }
    }

    /// Asks the source. Called once per start and once per retry.
    pub async fn fetch(&self) -> SourceAnswer {
        self.how.fetch().await
    }

    /// Asks the source and hands over its images in batches as they are ready (see
    /// [`CandidateFetch::fetch_progressive`]).
    pub async fn fetch_progressive(&self, early: &EarlyBatch<'_>) -> SourceAnswer {
        self.how.fetch_progressive(early).await
    }
}

impl std::fmt::Debug for CandidateSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CandidateSource")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

/// The plan of a candidate search.
#[derive(Debug, Clone)]
pub struct CandidateStream {
    pub title: String,
    /// `0` when unknown.
    pub year: u16,
    /// The artwork types the item can have, in listing order.
    pub slots: Vec<MediaFileType>,
    /// The image the item has now for the type, pinned first. `None` when it has none (or the
    /// type is an extra, which is set as a whole).
    pub current: Option<PickerCandidate>,
    /// The providers to ask, in the order of their default ranking.
    pub sources: Vec<CandidateSource>,
}

/// What an online provider is asked for: the images of a title, or of one season of a show.
#[derive(Clone, Copy)]
enum Scope {
    Title(MediaKind),
    /// The season number; the IDs are the show's.
    Season(u32),
    /// A movie collection; the IDs hold its `tmdbSet`.
    Collection,
}

/// An online provider asked for one slot of one title or season.
struct Online {
    service: Arc<ArtworkService>,
    provider: Arc<dyn ArtworkProvider>,
    scope: Scope,
    kind: MediaFileType,
    ids: ProviderIds,
    options: RequestOptions,
    config: ArtworkConfig,
}

impl Online {
    /// The provider's images for the slot in the order they are offered: the ones the automatic
    /// selection would take first, in its order, then the rest by the provider's own score.
    async fn ordered(&self) -> Result<Vec<SourcedArtwork>, SourceAnswer> {
        let answer = match self.scope {
            Scope::Title(media) => self.provider.artwork(media, &self.ids, &self.options).await,
            Scope::Season(number) => {
                self.provider
                    .season_artwork(&self.ids, number, &self.options)
                    .await
            }
            Scope::Collection => {
                self.provider
                    .collection_artwork(&self.ids, &self.options)
                    .await
            }
        };
        let found = match answer {
            Ok(found) => found,
            Err(error) => return Err(SourceAnswer::of_error(&error)),
        };
        let mut found: Vec<_> = found.into_iter().filter(|c| c.kind == self.kind).collect();
        presort(&mut found, &self.options);
        let sourced: Vec<SourcedArtwork> = found
            .into_iter()
            .map(|candidate| SourcedArtwork {
                provider: self.provider.id(),
                candidate,
            })
            .collect();
        let ranked = rank(&sourced, self.kind, &self.config.selection);
        let listed: std::collections::HashSet<&str> =
            ranked.iter().map(|c| c.candidate.url.as_str()).collect();
        let mut rest: Vec<&SourcedArtwork> = sourced
            .iter()
            .filter(|c| !listed.contains(c.candidate.url.as_str()))
            .collect();
        rest.sort_by(|a, b| quality(&b.candidate).total_cmp(&quality(&a.candidate)));
        let mut seen = std::collections::HashSet::new();
        Ok(ranked
            .iter()
            .copied()
            .chain(rest)
            .filter(|c| seen.insert(c.candidate.url.as_str()))
            .take(MAX_PICKER_CANDIDATES)
            .cloned()
            .collect())
    }

    /// Downloads the thumbnails of `batch` and returns the candidates that have one, in order.
    async fn candidates_of(&self, batch: &[SourcedArtwork]) -> Vec<PickerCandidate> {
        let urls = batch.iter().map(|c| c.candidate.url.clone()).collect();
        let previews = self.service.previews(urls).await;
        batch
            .iter()
            .filter_map(|c| {
                let preview = previews.get(&c.candidate.url)?;
                Some(PickerCandidate::of(
                    c,
                    self.kind,
                    &self.config.selection,
                    preview.key.clone(),
                    (preview.width, preview.height),
                ))
            })
            .collect()
    }
}

#[async_trait]
impl CandidateFetch for Online {
    async fn fetch(&self) -> SourceAnswer {
        match self.ordered().await {
            Ok(ordered) => SourceAnswer::Candidates(self.candidates_of(&ordered).await),
            Err(answer) => answer,
        }
    }

    async fn fetch_progressive(&self, early: &EarlyBatch<'_>) -> SourceAnswer {
        let ordered = match self.ordered().await {
            Ok(ordered) => ordered,
            Err(answer) => return answer,
        };
        // The best images are fetched first and handed over as soon as their round is done; the
        // order of the batches is the order of the list, so nothing arrives above what is shown.
        let mut rest: &[SourcedArtwork] = &ordered;
        let mut size = FIRST_BATCH;
        let mut last = Vec::new();
        while !rest.is_empty() {
            let (batch, tail) = rest.split_at(size.min(rest.len()));
            rest = tail;
            size = LATER_BATCH;
            let found = self.candidates_of(batch).await;
            if rest.is_empty() {
                last = found;
            } else if !found.is_empty() {
                early(found);
            }
        }
        SourceAnswer::Candidates(last)
    }
}

/// A provider whose key is not set: it answers at once and makes no request.
struct Keyless;

#[async_trait]
impl CandidateFetch for Keyless {
    async fn fetch(&self) -> SourceAnswer {
        SourceAnswer::NeedsKey
    }
}

impl ArtworkService {
    /// Plans a candidate search for `slot` of an item: the providers that offer the type, in the
    /// order of their default ranking, and the image the item has now.
    ///
    /// # Errors
    /// The item cannot be used for artwork, or cannot have that type.
    pub async fn candidate_stream(
        self: &Arc<Self>,
        item: cv_domain::ItemId,
        slot: MediaFileType,
    ) -> Result<CandidateStream, ArtworkError> {
        if let Some(season) = self.season_stream(item, slot).await? {
            return Ok(season);
        }
        if let Some(episode) = self.episode_stream(item, slot).await? {
            return Ok(episode);
        }
        if let Some(collection) = self.collection_stream(item, slot).await? {
            return Ok(collection);
        }
        let loaded = self.load(item).await?;
        let slots = picker_slots(&loaded.target);
        if !slots.contains(&slot) {
            return Err(ArtworkError::UnknownSlot);
        }
        let kind = candidate_kind(slot);
        let current = if kind == slot {
            self.current_image(&loaded, slot).await
        } else {
            None
        };
        let options: RequestOptions =
            MatchSettings::from_values(&self.settings.snapshot().values).request;
        let config = self.config();
        let mut sources = Vec::new();
        // Nothing to look up without an ID.
        if !loaded.item.core.ids.is_empty() {
            let waiting = self.providers_waiting_for_a_key();
            let inactive = self.inactive_providers();
            for provider in self
                .providers
                .registry()
                .artwork_providers_for(loaded.media)
                .into_iter()
                .filter(|p| p.offers(loaded.media, kind))
            {
                let key = provider.id().to_owned();
                let name = provider.name().to_owned();
                if inactive.contains(&provider.id()) {
                    // Without a key it answers "needs a key"; switched off otherwise.
                    if waiting.contains(&provider.id()) {
                        sources.push(CandidateSource::new(key, name, false, Arc::new(Keyless)));
                    }
                    continue;
                }
                sources.push(CandidateSource::new(
                    key,
                    name,
                    false,
                    Arc::new(Online {
                        service: Arc::clone(self),
                        provider,
                        scope: Scope::Title(loaded.media),
                        kind,
                        ids: loaded.item.core.ids.clone(),
                        options: options.clone(),
                        config: config.clone(),
                    }),
                ));
            }
        }
        // Frames come last: they are made on the CPU lane and rank below every online image.
        sources.extend(self.frames_source(&loaded.item, kind));
        Ok(CandidateStream {
            title: loaded.item.core.title.clone(),
            year: loaded.item.core.year,
            slots,
            current,
            sources,
        })
    }

    /// The plan for a season: one of its images from the providers that serve season images of
    /// that type, asked with the show's IDs. The slots are the plain names (`Poster` stands for
    /// the season poster). `None` when `item` is not a season.
    async fn season_stream(
        self: &Arc<Self>,
        item: cv_domain::ItemId,
        slot: MediaFileType,
    ) -> Result<Option<CandidateStream>, ArtworkError> {
        let Some(season) = self.season_context(item).await? else {
            return Ok(None);
        };
        let config = self.config();
        let slots = super::seasons::season_slots(&config.names.tv);
        let kind = super::seasons::season_kind(slot).ok_or(ArtworkError::UnknownSlot)?;
        if !slots
            .iter()
            .any(|s| super::seasons::season_kind(*s) == Some(kind))
        {
            return Err(ArtworkError::UnknownSlot);
        }
        let current = self.current_file(&season.item, kind).await;
        let options: RequestOptions =
            MatchSettings::from_values(&self.settings.snapshot().values).request;
        let show = &season.show.item;
        let mut sources = Vec::new();
        if !show.core.ids.is_empty() {
            let waiting = self.providers_waiting_for_a_key();
            let inactive = self.inactive_providers();
            for provider in self
                .providers
                .registry()
                .artwork_providers_for(MediaKind::Show)
                .into_iter()
                .filter(|p| p.offers_season(kind))
            {
                let key = provider.id().to_owned();
                let name = provider.name().to_owned();
                if inactive.contains(&provider.id()) {
                    if waiting.contains(&provider.id()) {
                        sources.push(CandidateSource::new(key, name, false, Arc::new(Keyless)));
                    }
                    continue;
                }
                sources.push(CandidateSource::new(
                    key,
                    name,
                    false,
                    Arc::new(Online {
                        service: Arc::clone(self),
                        provider,
                        scope: Scope::Season(season.number),
                        kind,
                        ids: show.core.ids.clone(),
                        options: options.clone(),
                        config: config.clone(),
                    }),
                ));
            }
        }
        Ok(Some(CandidateStream {
            title: super::seasons::season_title(show, season.number),
            year: show.core.year,
            slots,
            current,
            sources,
        }))
    }

    /// The plan for a movie collection: its poster or fanart from the providers that serve
    /// collection images, asked with the collection ID at TMDB. The chosen image is written to
    /// the artwork locations of the collection. `None` when `item` is not a collection.
    async fn collection_stream(
        self: &Arc<Self>,
        item: cv_domain::ItemId,
        slot: MediaFileType,
    ) -> Result<Option<CandidateStream>, ArtworkError> {
        if !self.is_set(item).await {
            return Ok(None);
        }
        if !COLLECTION_SLOTS.contains(&slot) {
            return Err(ArtworkError::UnknownSlot);
        }
        let set = self.load_set(item).await?.item;
        let current = self.current_file(&set, slot).await;
        let options: RequestOptions =
            MatchSettings::from_values(&self.settings.snapshot().values).request;
        let config = self.config();
        let mut sources = Vec::new();
        if let Some(ids) = collection_ids(&set) {
            let waiting = self.providers_waiting_for_a_key();
            let inactive = self.inactive_providers();
            for provider in self
                .providers
                .registry()
                .artwork_providers_for(MediaKind::Movie)
                .into_iter()
                .filter(|p| p.offers_collection(slot))
            {
                let key = provider.id().to_owned();
                let name = provider.name().to_owned();
                if inactive.contains(&provider.id()) {
                    if waiting.contains(&provider.id()) {
                        sources.push(CandidateSource::new(key, name, false, Arc::new(Keyless)));
                    }
                    continue;
                }
                sources.push(CandidateSource::new(
                    key,
                    name,
                    false,
                    Arc::new(Online {
                        service: Arc::clone(self),
                        provider,
                        scope: Scope::Collection,
                        kind: slot,
                        ids: ids.clone(),
                        options: options.clone(),
                        config: config.clone(),
                    }),
                ));
            }
        }
        Ok(Some(CandidateStream {
            title: set.core.title.clone(),
            year: 0,
            slots: COLLECTION_SLOTS.to_vec(),
            current,
            sources,
        }))
    }

    /// How many images each artwork type of an item has at the online providers, and the image
    /// it has now for the type, without downloading a candidate thumbnail: the picker's type
    /// strip shows these while it searches only the type on screen. Each provider is asked once
    /// for all types; a provider that fails counts nothing. The types are in the order of the
    /// picker.
    ///
    /// # Errors
    /// [`ArtworkError::Unsupported`] for an item whose images do not come from the providers of a
    /// title (a season or an episode): the picker then searches each type itself.
    pub async fn candidate_counts(
        self: &Arc<Self>,
        item: cv_domain::ItemId,
    ) -> Result<Vec<SlotCount>, ArtworkError> {
        let options: RequestOptions =
            MatchSettings::from_values(&self.settings.snapshot().values).request;
        let inactive = self.inactive_providers();
        if self.is_set(item).await {
            let set = self.load_set(item).await?.item;
            let mut counts = COLLECTION_SLOTS.map(|slot| (slot, 0_u32));
            if let Some(ids) = collection_ids(&set) {
                for provider in self
                    .providers
                    .registry()
                    .artwork_providers_for(MediaKind::Movie)
                {
                    if inactive.contains(&provider.id()) {
                        continue;
                    }
                    let Ok(found) = provider.collection_artwork(&ids, &options).await else {
                        continue;
                    };
                    for (slot, count) in &mut counts {
                        if provider.offers_collection(*slot) {
                            *count += counted(&found, *slot);
                        }
                    }
                }
            }
            let mut listed = Vec::new();
            for (slot, count) in counts {
                listed.push(SlotCount {
                    slot,
                    count,
                    current: self.current_file(&set, slot).await,
                });
            }
            return Ok(listed);
        }
        if self.season_context(item).await?.is_some() {
            return Err(ArtworkError::Unsupported);
        }
        let loaded = self.load(item).await?;
        let slots = picker_slots(&loaded.target);
        let mut counts: Vec<(MediaFileType, u32)> = slots.iter().map(|slot| (*slot, 0)).collect();
        // Nothing to look up without an ID.
        let asks = !loaded.item.core.ids.is_empty();
        for provider in self
            .providers
            .registry()
            .artwork_providers_for(loaded.media)
            .into_iter()
            .filter(|_| asks)
        {
            if inactive.contains(&provider.id()) {
                continue;
            }
            let Ok(found) = provider
                .artwork(loaded.media, &loaded.item.core.ids, &options)
                .await
            else {
                continue;
            };
            for (slot, count) in &mut counts {
                let kind = candidate_kind(*slot);
                if provider.offers(loaded.media, kind) {
                    *count += counted(&found, kind);
                }
            }
        }
        let mut listed = Vec::new();
        for slot in slots {
            let count = counts
                .iter()
                .find(|(s, _)| *s == slot)
                .map_or(0, |(_, n)| *n);
            // An extra (fanart, thumbs) is set as a whole: it has no single current image.
            let current = if candidate_kind(slot) == slot {
                self.current_image(&loaded, slot).await
            } else {
                None
            };
            listed.push(SlotCount {
                slot,
                count,
                current,
            });
        }
        Ok(listed)
    }

    /// The plan for an episode: its thumb, from the frames of its video. `None` when `item` is
    /// not an episode.
    async fn episode_stream(
        self: &Arc<Self>,
        item: cv_domain::ItemId,
        slot: MediaFileType,
    ) -> Result<Option<CandidateStream>, ArtworkError> {
        let db = Arc::clone(&self.db);
        let found =
            tokio::task::spawn_blocking(move || db.read(|c| cv_db::repo::get_item(c, item)))
                .await
                .map_err(|_| ArtworkError::Worker)??;
        let Some(episode) = found else {
            return Ok(None);
        };
        if episode.kind() != cv_domain::ItemKind::Episode {
            return Ok(None);
        }
        if slot != MediaFileType::Thumb {
            return Err(ArtworkError::UnknownSlot);
        }
        let current = self.current_file(&episode, slot).await;
        let sources = self.frames_source(&episode, slot).into_iter().collect();
        Ok(Some(CandidateStream {
            title: episode.core.title.clone(),
            year: 0,
            slots: vec![MediaFileType::Thumb],
            current,
            sources,
        }))
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

    /// The keys of the providers that are not ready to be asked.
    fn inactive_providers(&self) -> Vec<&'static str> {
        self.providers
            .list()
            .map(|statuses| {
                statuses
                    .into_iter()
                    .filter(|status| !status.active)
                    .map(|status| status.provider)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The item's present image of `slot` in its artwork folder, with its thumbnail.
    async fn current_image(
        &self,
        loaded: &super::Loaded,
        slot: MediaFileType,
    ) -> Option<PickerCandidate> {
        self.current_file_in(&loaded.item, slot, Some(&loaded.target.dir))
            .await
    }

    /// The image an item has now for `slot`, wherever its files are.
    async fn current_file(
        &self,
        item: &cv_domain::Item,
        slot: MediaFileType,
    ) -> Option<PickerCandidate> {
        self.current_file_in(item, slot, None).await
    }

    async fn current_file_in(
        &self,
        item: &cv_domain::Item,
        slot: MediaFileType,
        folder: Option<&std::path::Path>,
    ) -> Option<PickerCandidate> {
        let path = item
            .core
            .media_files
            .iter()
            .filter(|f| {
                f.kind == slot && folder.is_none_or(|dir| std::path::Path::new(&f.directory) == dir)
            })
            .map(cv_domain::MediaFile::path)
            .find(|p| p.is_file())?;
        let thumbs: Arc<ThumbCache> = Arc::clone(&self.thumbs);
        let source = path.clone();
        let ensured =
            tokio::task::spawn_blocking(move || thumbs.ensure(&source, &[ThumbSize::Grid]))
                .await
                .ok()?
                .ok()?;
        Some(PickerCandidate {
            provider: CURRENT_PROVIDER.to_owned(),
            url: file_url(&path),
            language: None,
            width: ensured.meta.source_width,
            height: ensured.meta.source_height,
            score: 0,
            best: 0,
            votes: 0,
            rating: 0,
            likes: 0,
            low_quality: false,
            key: ensured.key,
        })
    }
}

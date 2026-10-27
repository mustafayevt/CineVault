//! Details, episode lists in every ordering, seasons, artwork and trailers.

use cv_domain::{EpisodeGroup, Item};

use super::api::{ArtworkType, Envelope, EpisodePage, EpisodeRecord, Extended, SeasonExtended};
use super::client::Params;
use super::convert::{
    Context, aired_season, artwork_candidates, episode_item, group_for_slug, movie_item,
    season_item, set_number, show_item, slug_for_group,
};
use super::language::language_chain;
use super::{DISPLAY_NAME, TvdbProvider, TvdbSeason};
use crate::error::ProviderError;
use crate::model::{ArtworkCandidate, MediaKind, RequestOptions};

/// Episode lists stop after this many pages (500 episodes each).
const MAX_PAGES: u32 = 50;

fn not_found() -> ProviderError {
    ProviderError::NotFound(DISPLAY_NAME.to_owned())
}

/// Whether an episode has both a name and a plot.
fn complete(record: &EpisodeRecord) -> bool {
    use super::api::Text;
    !record.name.text().trim().is_empty() && !record.overview.text().trim().is_empty()
}

/// Fills an episode's empty name and plot from the same episode in another language.
fn fill(target: &mut EpisodeRecord, source: &EpisodeRecord) {
    use super::api::Text;
    if target.name.text().trim().is_empty() {
        target.name.clone_from(&source.name);
    }
    if target.overview.text().trim().is_empty() {
        target.overview.clone_from(&source.overview);
    }
}

impl TvdbProvider {
    fn chain(options: &RequestOptions) -> Vec<String> {
        language_chain(&options.language, &options.fallback_language)
    }

    /// The artwork types, or `None` when they cannot be read (artwork then falls back to the
    /// record's main image).
    async fn types_or_none(
        &self,
        options: &RequestOptions,
    ) -> Option<std::sync::Arc<Vec<ArtworkType>>> {
        match self.artwork_types(options).await {
            Ok(types) => Some(types),
            Err(error) => {
                tracing::debug!(%error, "TheTVDB artwork types are unavailable");
                None
            }
        }
    }

    // --- series and movies ---------------------------------------------------------------

    fn extended_path(kind: MediaKind, id: u64) -> String {
        match kind {
            MediaKind::Movie => format!("movies/{id}/extended"),
            MediaKind::Show => format!("series/{id}/extended"),
        }
    }

    fn extended_params() -> Params {
        vec![("meta", "translations".to_owned())]
    }

    /// The full record of a series or movie, with its translations.
    pub(super) async fn extended(
        &self,
        kind: MediaKind,
        id: u64,
        options: &RequestOptions,
    ) -> Result<Extended, ProviderError> {
        self.get(
            &Self::extended_path(kind, id),
            Self::extended_params(),
            options,
        )
        .await
    }

    /// Like [`Self::extended`], but a missing record is `Ok(None)`.
    pub(super) async fn extended_optional(
        &self,
        kind: MediaKind,
        id: u64,
        options: &RequestOptions,
    ) -> Result<Option<Extended>, ProviderError> {
        match self.extended(kind, id, options).await {
            Ok(ext) => Ok(Some(ext)),
            Err(ProviderError::NotFound(_)) => Ok(None),
            Err(other) => Err(other),
        }
    }

    pub(super) async fn fetch_details(
        &self,
        kind: MediaKind,
        id: u64,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let ext = self.extended(kind, id, options).await?;
        let chain = Self::chain(options);
        let types = self.types_or_none(options).await;
        let candidates = types
            .as_deref()
            .map(|t| artwork_candidates(&ext.artworks, t, &chain))
            .unwrap_or_default();
        let ctx = Context {
            options,
            chain: &chain,
        };
        Ok(match kind {
            MediaKind::Movie => movie_item(&ctx, &ext, &candidates),
            MediaKind::Show => show_item(&ctx, &ext, &candidates),
        })
    }

    pub(super) async fn fetch_artwork(
        &self,
        kind: MediaKind,
        id: u64,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let ext = self.extended(kind, id, options).await?;
        let types = self.artwork_types(options).await?;
        Ok(artwork_candidates(
            &ext.artworks,
            &types,
            &Self::chain(options),
        ))
    }

    // --- episodes ------------------------------------------------------------------------

    /// Every page of `series/{id}/episodes/{slug}[/{language}]`.
    async fn episode_pages(
        &self,
        id: u64,
        slug: &str,
        language: Option<&str>,
        options: &RequestOptions,
    ) -> Result<Vec<EpisodeRecord>, ProviderError> {
        let path = match language {
            Some(language) => format!("series/{id}/episodes/{slug}/{language}"),
            None => format!("series/{id}/episodes/{slug}"),
        };
        let mut all = Vec::new();
        for page in 0..MAX_PAGES {
            let answer: Envelope<EpisodePage> = self
                .envelope(&path, vec![("page", page.to_string())], options)
                .await?;
            let batch = answer.data.map(|d| d.episodes).unwrap_or_default();
            let empty = batch.is_empty();
            all.extend(batch);
            let more = answer.links.is_some_and(|l| !l.next.is_blank());
            if empty || !more {
                break;
            }
        }
        Ok(all)
    }

    /// An episode list in the preferred language. Episodes without a name or plot there are
    /// filled from the fallback language and then English, one extra request per language.
    async fn localised_pages(
        &self,
        id: u64,
        slug: &str,
        options: &RequestOptions,
    ) -> Result<Vec<EpisodeRecord>, ProviderError> {
        let chain = Self::chain(options);
        let mut records = self
            .episode_pages(id, slug, chain.first().map(String::as_str), options)
            .await?;
        for language in chain.iter().skip(1) {
            if records.iter().all(complete) {
                break;
            }
            match self.episode_pages(id, slug, Some(language), options).await {
                Ok(other) => {
                    for record in &mut records {
                        if let Some(source) = other.iter().find(|o| o.id == record.id) {
                            fill(record, source);
                        }
                    }
                }
                Err(ProviderError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(records)
    }

    /// The aired episode list as TheTVDB serves it, or an empty list when the show has none.
    /// `official` is the aired order; some shows only have `default`.
    async fn aired_pages(
        &self,
        id: u64,
        localised: bool,
        options: &RequestOptions,
    ) -> Result<Vec<EpisodeRecord>, ProviderError> {
        for slug in ["official", "default"] {
            let result = if localised {
                self.localised_pages(id, slug, options).await
            } else {
                self.episode_pages(id, slug, None, options).await
            };
            match result {
                Ok(records) if !records.is_empty() => return Ok(records),
                Ok(_) | Err(ProviderError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Vec::new())
    }

    /// Every episode of a show in one ordering. Items carry the aired number when the show has
    /// one for the episode, plus the number in the requested ordering (season types `dvd`,
    /// `absolute`, `alternate`, `regional`, `altdvd` and `alttwo`). An absolute list the show
    /// lacks is built from the absolute numbers of the aired list when those exist.
    pub(super) async fn fetch_episode_list(
        &self,
        id: u64,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        let slug = slug_for_group(ordering).ok_or_else(not_found)?;
        let aired_group = EpisodeGroup::aired();

        if slug == "official" {
            let records = self.aired_pages(id, true, options).await?;
            let mut rows: Vec<(u32, u32, Item)> = Vec::new();
            for record in &records {
                let (Some(season), Some(number)) = (record.season_number, record.number) else {
                    continue;
                };
                let mut item = episode_item(record);
                set_number(&mut item, &aired_group, season, number);
                rows.push((season, number, item));
            }
            if rows.is_empty() {
                return Err(not_found());
            }
            rows.sort_by_key(|(season, number, _)| (*season, *number));
            return Ok(rows.into_iter().map(|(_, _, item)| item).collect());
        }

        let group = group_for_slug(slug).ok_or_else(not_found)?;
        let primary = match self.localised_pages(id, slug, options).await {
            Ok(records) => records,
            Err(ProviderError::NotFound(_)) => Vec::new(),
            Err(error) => return Err(error),
        };
        // Aired numbers only need the default-language list.
        let aired = self.aired_pages(id, false, options).await?;

        let mut rows: Vec<(u32, u32, Item)> = Vec::new();
        if primary.is_empty() && slug == "absolute" {
            for record in &aired {
                let Some(absolute) = record.absolute_number.filter(|n| *n > 0) else {
                    continue;
                };
                let mut item = episode_item(record);
                if let (Some(season), Some(number)) = (record.season_number, record.number) {
                    set_number(&mut item, &aired_group, season, number);
                }
                set_number(&mut item, &group, 1, absolute);
                rows.push((1, absolute, item));
            }
        } else {
            for record in &primary {
                let (Some(season), Some(number)) = (record.season_number, record.number) else {
                    continue;
                };
                let mut item = episode_item(record);
                if let Some(original) = aired.iter().find(|a| a.id == record.id)
                    && let (Some(s), Some(n)) = (original.season_number, original.number)
                {
                    set_number(&mut item, &aired_group, s, n);
                }
                set_number(&mut item, &group, season, number);
                rows.push((season, number, item));
            }
        }
        if rows.is_empty() {
            return Err(not_found());
        }
        rows.sort_by_key(|(season, number, _)| (*season, *number));
        Ok(rows.into_iter().map(|(_, _, item)| item).collect())
    }

    // --- seasons -------------------------------------------------------------------------

    /// The record of an aired season of a show: its `seasons/{id}` ID and the show's record.
    async fn season_record(
        &self,
        show_id: u64,
        number: u32,
        options: &RequestOptions,
    ) -> Result<SeasonExtended, ProviderError> {
        let show = self.extended(MediaKind::Show, show_id, options).await?;
        let season = aired_season(&show.seasons, number).ok_or_else(not_found)?;
        let season_id = season.id.as_u64().ok_or_else(not_found)?;
        self.get(
            &format!("seasons/{season_id}/extended"),
            Self::extended_params(),
            options,
        )
        .await
    }

    pub(super) async fn fetch_season(
        &self,
        show_id: u64,
        number: u32,
        options: &RequestOptions,
    ) -> Result<TvdbSeason, ProviderError> {
        let record = self.season_record(show_id, number, options).await?;
        let chain = Self::chain(options);
        let types = self.types_or_none(options).await;
        let candidates = types
            .as_deref()
            .map(|t| artwork_candidates(&record.artwork, t, &chain))
            .unwrap_or_default();
        let ctx = Context {
            options,
            chain: &chain,
        };
        let season = season_item(&ctx, &record, &candidates);

        let aired_group = EpisodeGroup::aired();
        let mut rows: Vec<(u32, Item)> = Vec::new();
        for episode in self.aired_pages(show_id, true, options).await? {
            if episode.season_number != Some(number) {
                continue;
            }
            let Some(episode_number) = episode.number else {
                continue;
            };
            let mut item = episode_item(&episode);
            set_number(&mut item, &aired_group, number, episode_number);
            rows.push((episode_number, item));
        }
        rows.sort_by_key(|(n, _)| *n);
        Ok(TvdbSeason {
            season,
            episodes: rows.into_iter().map(|(_, item)| item).collect(),
        })
    }

    pub(super) async fn fetch_season_artwork(
        &self,
        show_id: u64,
        number: u32,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let record = self.season_record(show_id, number, options).await?;
        let types = self.artwork_types(options).await?;
        Ok(artwork_candidates(
            &record.artwork,
            &types,
            &Self::chain(options),
        ))
    }
}

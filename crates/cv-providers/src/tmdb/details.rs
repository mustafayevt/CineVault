//! Details, episodes, collections, images, trailers and ratings.

use std::collections::BTreeMap;

use cv_domain::{
    EpisodeGroup, EpisodeOrdering, Item, ItemDetail, MediaFileType, Rating, Ratings, id_key,
};
use tokio::task::JoinSet;

use super::api::{
    CollectionDetails, Episode, EpisodeGroupDetails, Images, MovieDetails, SeasonDetails, Text,
    TvDetails,
};
use super::client::{Params, language_params};
use super::convert::{
    Context, RATING_ID, episode_item, map_episode_groups, missing_movie, movie_item, season_item,
    show_item,
};
use super::language::Locale;
use super::media::{self, Slot};
use super::{DISPLAY_NAME, TmdbCollection, TmdbProvider, TmdbSeason};
use crate::error::ProviderError;
use crate::model::{ArtworkCandidate, MediaKind, RequestOptions};

const MOVIE_APPEND: &str = "credits,keywords,release_dates,translations,external_ids,videos";
const SHOW_APPEND: &str =
    "translations,credits,external_ids,content_ratings,keywords,episode_groups,videos";

/// The video languages a details request asks for: the preferred one, English and videos with
/// no language, so the trailer list is not empty for a language TMDB has no trailer in.
fn video_languages(options: &RequestOptions) -> (&'static str, String) {
    let language = cv_domain::language_code(&options.language);
    let mut list = Vec::new();
    if !language.is_empty() && language != "en" {
        list.push(language);
    }
    list.push("en".to_owned());
    list.push("null".to_owned());
    ("include_video_language", list.join(","))
}

fn not_found() -> ProviderError {
    ProviderError::NotFound(DISPLAY_NAME.to_owned())
}

/// Whether the fallback language is a different request from the preferred one.
fn has_distinct_fallback(options: &RequestOptions) -> bool {
    match (
        Locale::parse(&options.language),
        Locale::parse(&options.fallback_language),
    ) {
        (Some(preferred), Some(fallback)) => preferred != fallback,
        _ => false,
    }
}

/// The options for a second request in the fallback language.
fn fallback_options(options: &RequestOptions) -> RequestOptions {
    RequestOptions {
        language: options.fallback_language.clone(),
        ..options.clone()
    }
}

impl TmdbProvider {
    pub(super) async fn fetch_movie(
        &self,
        id: u64,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let mut params = language_params(options);
        params.push(("append_to_response", MOVIE_APPEND.to_owned()));
        params.push(video_languages(options));
        let details: MovieDetails = self.get(&format!("movie/{id}"), params, options).await?;
        let image_base = self.image_base(options).await;
        Ok(movie_item(
            &Context {
                image_base: &image_base,
                options,
            },
            &details,
        ))
    }

    pub(super) async fn fetch_show(
        &self,
        id: u64,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let details = self.show_response(id, options).await?;
        let image_base = self.image_base(options).await;
        Ok(show_item(
            &Context {
                image_base: &image_base,
                options,
            },
            &details,
        ))
    }

    async fn show_response(
        &self,
        id: u64,
        options: &RequestOptions,
    ) -> Result<TvDetails, ProviderError> {
        let mut params = language_params(options);
        params.push(("append_to_response", SHOW_APPEND.to_owned()));
        params.push(video_languages(options));
        self.get(&format!("tv/{id}"), params, options).await
    }

    // --- seasons and episodes ------------------------------------------------------------

    pub(super) async fn fetch_season(
        &self,
        show_id: u64,
        season: u32,
        options: &RequestOptions,
    ) -> Result<TmdbSeason, ProviderError> {
        let mut details = self.season_response(show_id, season, options).await?;

        // Episodes the preferred language has no name or plot for are filled from the fallback
        // language, in one extra request for the whole season.
        let incomplete = details
            .episodes
            .iter()
            .any(|e| e.name.text().trim().is_empty() || e.overview.text().trim().is_empty());
        if incomplete && has_distinct_fallback(options) {
            let fallback = fallback_options(options);
            if let Ok(other) = self.season_response(show_id, season, &fallback).await {
                for episode in &mut details.episodes {
                    if let Some(source) = other
                        .episodes
                        .iter()
                        .find(|o| o.episode_number == episode.episode_number)
                    {
                        fill_episode(episode, source);
                    }
                }
                if details.overview.text().trim().is_empty() {
                    details.overview = other.overview;
                }
            }
        }

        let image_base = self.image_base(options).await;
        let ctx = Context {
            image_base: &image_base,
            options,
        };
        Ok(TmdbSeason {
            season: season_item(&ctx, &details),
            episodes: details
                .episodes
                .iter()
                .map(|e| episode_item(&ctx, e, None))
                .collect(),
        })
    }

    async fn season_response(
        &self,
        show_id: u64,
        season: u32,
        options: &RequestOptions,
    ) -> Result<SeasonDetails, ProviderError> {
        let mut params = language_params(options);
        params.push(("append_to_response", "external_ids".to_owned()));
        self.get(&format!("tv/{show_id}/season/{season}"), params, options)
            .await
    }

    pub(super) async fn fetch_episode(
        &self,
        show_id: u64,
        season: u32,
        episode: u32,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let mut details = self
            .episode_response(show_id, season, episode, options)
            .await?;
        if (details.name.text().trim().is_empty() || details.overview.text().trim().is_empty())
            && has_distinct_fallback(options)
            && let Ok(other) = self
                .episode_response(show_id, season, episode, &fallback_options(options))
                .await
        {
            fill_episode(&mut details, &other);
        }
        let image_base = self.image_base(options).await;
        Ok(episode_item(
            &Context {
                image_base: &image_base,
                options,
            },
            &details,
            None,
        ))
    }

    async fn episode_response(
        &self,
        show_id: u64,
        season: u32,
        episode: u32,
        options: &RequestOptions,
    ) -> Result<Episode, ProviderError> {
        let mut params = language_params(options);
        params.push(("append_to_response", "external_ids".to_owned()));
        self.get(
            &format!("tv/{show_id}/season/{season}/episode/{episode}"),
            params,
            options,
        )
        .await
    }

    /// Every episode of a show in one ordering. The aired list reads each season; any other
    /// ordering reads the show's matching TMDB episode group. Items carry the aired number when
    /// TMDB gives one, plus the number in the requested ordering.
    pub(super) async fn fetch_episode_list(
        &self,
        show_id: u64,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        let show = self.show_response(show_id, options).await?;
        if ordering.ordering == EpisodeOrdering::Aired {
            return self.aired_episodes(show_id, &show, options).await;
        }

        let groups = map_episode_groups(&show.episode_groups.results);
        let chosen = groups
            .iter()
            .find(|(group, _)| group == ordering)
            .or_else(|| {
                // A stored DVD or absolute ordering may carry a group name; the show has one of each.
                matches!(
                    ordering.ordering,
                    EpisodeOrdering::Dvd | EpisodeOrdering::Absolute
                )
                .then(|| groups.iter().find(|(g, _)| g.ordering == ordering.ordering))
                .flatten()
            })
            .ok_or_else(not_found)?;
        let (group, group_id) = chosen;

        let details: EpisodeGroupDetails = self
            .get(
                &format!("tv/episode_group/{group_id}"),
                language_params(options),
                options,
            )
            .await?;
        let image_base = self.image_base(options).await;
        let ctx = Context {
            image_base: &image_base,
            options,
        };
        let mut items = Vec::new();
        for season in &details.groups {
            for (index, episode) in season.episodes.iter().enumerate() {
                let number = episode
                    .order
                    .map_or_else(|| index as u32 + 1, |order| order + 1);
                items.push(episode_item(
                    &ctx,
                    episode,
                    Some((group, season.order, number)),
                ));
            }
        }
        Ok(items)
    }

    async fn aired_episodes(
        &self,
        show_id: u64,
        show: &TvDetails,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        let mut tasks = JoinSet::new();
        for season in &show.seasons {
            let provider = self.clone();
            let options = options.clone();
            let number = season.season_number;
            tasks.spawn(async move {
                (
                    number,
                    provider.fetch_season(show_id, number, &options).await,
                )
            });
        }
        let mut by_season: BTreeMap<u32, Vec<Item>> = BTreeMap::new();
        while let Some(joined) = tasks.join_next().await {
            let (number, result) = joined.map_err(|e| ProviderError::Other(e.to_string()))?;
            match result {
                Ok(season) => {
                    by_season.insert(number, season.episodes);
                }
                // A season TMDB lists but cannot return has no episodes to offer.
                Err(ProviderError::NotFound(_)) => {}
                Err(other) => return Err(other),
            }
        }
        Ok(by_season.into_values().flatten().collect())
    }

    // --- collections ---------------------------------------------------------------------

    pub(super) async fn fetch_collection(
        &self,
        id: u64,
        options: &RequestOptions,
    ) -> Result<TmdbCollection, ProviderError> {
        let mut details: CollectionDetails = self
            .get(
                &format!("collection/{id}"),
                language_params(options),
                options,
            )
            .await?;
        if details.overview.text().trim().is_empty()
            && has_distinct_fallback(options)
            && let Ok(other) = self
                .get::<CollectionDetails>(
                    &format!("collection/{id}"),
                    language_params(&fallback_options(options)),
                    options,
                )
                .await
        {
            details.overview = other.overview;
        }

        let image_base = self.image_base(options).await;
        let ctx = Context {
            image_base: &image_base,
            options,
        };
        let mut item = Item::set(details.name.text().trim());
        item.core.plot = details.overview.text().trim().to_owned();
        item.core.ids.set(id_key::TMDB_SET, details.id.to_string());
        if let Some(url) = ctx.image("original", &details.poster_path) {
            item.core.artwork_urls.insert(MediaFileType::Poster, url);
        }
        if let Some(url) = ctx.image("original", &details.backdrop_path) {
            item.core.artwork_urls.insert(MediaFileType::Fanart, url);
        }
        if let ItemDetail::Set(set) = &mut item.detail {
            set.english_title = String::new();
        }
        let movies = details
            .parts
            .iter()
            .filter(|p| p.id != 0)
            .map(|part| missing_movie(part, &ctx))
            .collect();
        Ok(TmdbCollection { item, movies })
    }

    // --- images, trailers, ratings -------------------------------------------------------

    /// Reads an `images` endpoint and returns the candidates of `slots`, in that order.
    pub(super) async fn images_for(
        &self,
        path: &str,
        options: &RequestOptions,
        slots: &[Slot],
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let preferred = Locale::parse(&options.language).unwrap_or(Locale {
            language: "en".to_owned(),
            country: None,
        });
        let mut languages = vec![preferred.language.clone()];
        if preferred.language != "en" {
            languages.push("en".to_owned());
        }
        languages.push("null".to_owned());
        let params: Params = vec![("include_image_language", languages.join(","))];

        let images: Images = self.get(path, params, options).await?;
        let image_base = self.image_base(options).await;
        Ok(slots
            .iter()
            .flat_map(|slot| media::candidates(&image_base, &images, *slot, &preferred))
            .collect())
    }

    pub(super) async fn fetch_ratings(
        &self,
        kind: MediaKind,
        id: u64,
        options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        let (average, votes) = match kind {
            MediaKind::Movie => {
                let d: MovieDetails = self
                    .get(&format!("movie/{id}"), Vec::new(), options)
                    .await?;
                (d.vote_average, d.vote_count)
            }
            MediaKind::Show => {
                let d: TvDetails = self.get(&format!("tv/{id}"), Vec::new(), options).await?;
                (d.vote_average, d.vote_count)
            }
        };
        let mut ratings = Ratings::new();
        if let Some(average) = average.filter(|a| *a > 0.0) {
            ratings.insert(Rating::new(RATING_ID, average, votes.unwrap_or(0), 10));
        }
        Ok(ratings)
    }
}

/// Fills an episode's empty name and plot from the same episode in another language.
fn fill_episode(target: &mut Episode, source: &Episode) {
    if target.name.text().trim().is_empty() {
        target.name.clone_from(&source.name);
    }
    if target.overview.text().trim().is_empty() {
        target.overview.clone_from(&source.overview);
    }
}

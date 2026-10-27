//! The title side of Torrent search: finding a movie or show on TMDB, reading its details, and
//! listing a show's seasons and episodes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Datelike;
use cv_domain::{
    CalendarDate, EpisodeGroup, Genre, Item, ItemDetail, MediaFileType, ProviderIds, id_key,
};
use cv_match::query::{IdShortcut, MediaHint, NormalizedQuery, normalize_query};
use cv_match::{Candidate, popularity_bonus, title_similarity, year_penalty};
use cv_nfo::certification::{CertificationStyle, format_certification};
use cv_providers::model::{MediaKind as ProviderKind, SearchHit, SearchQuery};
use cv_providers::{MetadataProvider, ProviderError};
use cv_template::tokens::genre_label;
use cv_torrent::model::{MediaKind, Scope, TitleIds, TorrentQuery};

use super::library::{LibraryHolding, LibraryLink, holding_of, library_copy, linked_title};
use super::{TorrentService, TorrentServiceError};
use crate::anime_crosswalk::is_anime;
use crate::matching::MatchSettings;

/// How many title hits a search lists.
const MAX_HITS: usize = 12;
/// How long a show's episode list is reused between the season and episode pickers.
const EPISODE_LIST_TTL: Duration = Duration::from_secs(10 * 60);

/// A movie or show that matches what was typed.
#[derive(Debug, Clone, PartialEq)]
pub struct TitleHit {
    pub kind: MediaKind,
    pub tmdb_id: u64,
    pub title: String,
    pub original_title: String,
    pub year: Option<u16>,
    pub overview: String,
    pub poster_url: Option<String>,
    pub imdb_id: Option<String>,
    /// 0 to 1: how well the title fits what was typed, less a penalty for another year, plus a
    /// small share for its votes. The best match is first.
    pub confidence: f64,
    pub in_library: Option<LibraryLink>,
}

/// What a confirmed title is, for the header card and the release search.
#[derive(Debug, Clone, PartialEq)]
pub struct TitleDetails {
    pub kind: MediaKind,
    pub ids: TitleIds,
    pub title: String,
    pub original_title: Option<String>,
    pub alternative_titles: Vec<String>,
    pub year: Option<u16>,
    pub runtime_minutes: Option<u32>,
    pub anime: bool,
    pub poster_url: Option<String>,
    pub library_item: Option<LibraryLink>,
    /// The TMDB backdrop, for the header band.
    pub backdrop_url: Option<String>,
    /// `R`, `TV-MA`; none when TMDB gives none.
    pub certification: Option<String>,
    /// The TMDB rating on a ten-point scale.
    pub rating: Option<f32>,
    /// The genres as words (`Science Fiction`).
    pub genres: Vec<String>,
    /// The plot.
    pub overview: Option<String>,
    /// What the library holds of the title, for the header.
    pub holding: Option<LibraryHolding>,
}

/// A season of a show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonInfo {
    pub number: u16,
    pub name: String,
    pub episode_count: u32,
    pub air_date: Option<String>,
}

/// An episode of a season.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeInfo {
    pub number: u16,
    pub name: String,
    pub air_date: Option<String>,
}

/// One row of a show's episode list.
#[derive(Debug, Clone)]
pub(super) struct EpisodeRow {
    season: u16,
    episode: u16,
    name: String,
    air_date: Option<CalendarDate>,
}

pub(super) type EpisodeCache =
    std::sync::Mutex<std::collections::HashMap<u64, (Instant, Arc<Vec<EpisodeRow>>)>>;

fn provider_kind(kind: MediaKind) -> ProviderKind {
    match kind {
        MediaKind::Movie => ProviderKind::Movie,
        MediaKind::Show => ProviderKind::Show,
    }
}

fn year_of(year: u16) -> Option<u16> {
    (year > 0).then_some(year)
}

fn date_text(date: Option<CalendarDate>) -> Option<String> {
    date.map(|d| d.to_string())
}

/// Anime as far as TMDB says: an anime site ID, or a Japanese production in the Animation genre.
fn looks_like_anime(item: &Item) -> bool {
    if is_anime(&item.core.ids) {
        return true;
    }
    let animation = match &item.detail {
        ItemDetail::Movie(movie) => movie.genres.contains(&Genre::Animation),
        ItemDetail::Show(show) => show.genres.contains(&Genre::Animation),
        _ => false,
    };
    animation && item.core.original_language.eq_ignore_ascii_case("ja")
}

fn clean(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// A certification as people read it: the stored token `US_PG13` is `PG-13`, `US_TV_MA` is
/// `TV-MA`. Text that is no token stays as it is.
fn certification_text(token: &str) -> Option<String> {
    let token = clean(token)?;
    let label = format_certification(&token, CertificationStyle::Short);
    Some(if label.trim().is_empty() {
        token
    } else {
        label
    })
}

impl TorrentService {
    fn tmdb(&self) -> Result<Arc<dyn MetadataProvider>, TorrentServiceError> {
        self.providers
            .registry()
            .metadata_providers()
            .iter()
            .find(|p| p.id() == "tmdb")
            .cloned()
            .ok_or(TorrentServiceError::NoMetadataProvider)
    }

    fn request_options(&self) -> cv_providers::model::RequestOptions {
        MatchSettings::from_values(&self.settings.values()).request
    }

    /// Movies or shows that match what was typed, best first.
    ///
    /// ID shortcuts work as in the Scrape search: `tt0063350`, `tmdb:123`, a TMDB or IMDb
    /// address, and `Title (Year)`. A year at the end without brackets (`Heat 1995`) counts as
    /// the year when a title of that name and year exists, so the 1995 movie outranks a 2015 show
    /// called Heat. The hits are ordered by how well they fit, less a penalty for another year,
    /// plus a small share for their votes (the rule of the Scrape search).
    ///
    /// # Errors
    /// [`TorrentServiceError::Torrent`] with `NoticeNotAccepted` until the Torrent search notice is accepted (nothing
    /// is contacted), or the provider's error (no key, rate limit, unreachable).
    pub async fn title_hits(
        &self,
        kind: MediaKind,
        text: &str,
    ) -> Result<Vec<TitleHit>, TorrentServiceError> {
        self.ensure_accepted()?;
        let provider = self.tmdb()?;
        let options = self.request_options();
        let query = normalize_query(text);
        let bare_year = trailing_year(&query);

        let hits: Vec<SearchHit> = if let Some(shortcut) = &query.id {
            let wrong_kind = matches!(
                (&shortcut.hint, kind),
                (Some(MediaHint::Show), MediaKind::Movie)
                    | (Some(MediaHint::Movie), MediaKind::Show)
            );
            if wrong_kind {
                Vec::new()
            } else {
                let mut ids = ProviderIds::new();
                ids.set(shortcut_key(shortcut), &shortcut.id);
                provider
                    .lookup(provider_kind(kind), &ids, &options)
                    .await?
                    .into_iter()
                    .collect()
            }
        } else {
            if query.text.trim().chars().count() < 2 {
                return Ok(Vec::new());
            }
            let search = |query: &NormalizedQuery| {
                let request = SearchQuery {
                    kind: provider_kind(kind),
                    title: query.text.clone(),
                    year: query.year,
                };
                let provider = Arc::clone(&provider);
                let options = options.clone();
                async move { provider.search(&request, &options).await }
            };
            match &bare_year {
                // "Heat 1995": the title with that year first. When no title of that name and
                // year comes back ("Wonder Woman 1984"), the whole text is searched too.
                Some(split) => {
                    let mut hits = search(split).await?;
                    let named = hits
                        .iter()
                        .any(|hit| fit(split, hit) >= NAMED_FIT && Some(hit.year) == split.year);
                    if !named {
                        for hit in search(&query).await? {
                            let id = hit.ids.get(id_key::TMDB).map(str::to_owned);
                            if !hits
                                .iter()
                                .any(|h| h.ids.get(id_key::TMDB).map(str::to_owned) == id)
                            {
                                hits.push(hit);
                            }
                        }
                    }
                    hits
                }
                None => search(&query).await?,
            }
        };

        let mut rows: Vec<TitleHit> = hits
            .into_iter()
            .filter_map(|hit| {
                let tmdb_id = hit.ids.get_u64(id_key::TMDB)?;
                let confidence = if hit.exact_id {
                    1.0
                } else {
                    // The better of the two readings of a trailing year, then a small share for
                    // how well known the title is, so of two of one name the likelier is first.
                    let mut fitness = fit(&query, &hit);
                    if let Some(split) = &bare_year {
                        fitness = fitness.max(fit(split, &hit));
                    }
                    let known = popularity_bonus(&Candidate {
                        votes: hit.votes,
                        popularity: hit.popularity,
                        ..Candidate::default()
                    });
                    (FIT_SHARE * fitness + known).clamp(0.0, 1.0)
                };
                Some((tmdb_id, hit, confidence))
            })
            .map(|(tmdb_id, hit, confidence)| {
                let in_library = linked_title(&self.db, kind, tmdb_id).unwrap_or_else(|error| {
                    tracing::debug!(%error, "library link not read");
                    None
                });
                TitleHit {
                    kind,
                    tmdb_id,
                    title: hit.title,
                    original_title: hit.original_title,
                    year: year_of(hit.year),
                    overview: hit.overview,
                    poster_url: hit.poster_url,
                    imdb_id: hit.ids.get(id_key::IMDB).map(str::to_owned),
                    confidence,
                    in_library,
                }
            })
            .collect();
        // The best fit first; ties keep the provider's order (its own ranking, popularity).
        rows.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
        rows.truncate(MAX_HITS);
        Ok(rows)
    }

    /// The details of a confirmed title: IDs, titles in other languages, year, runtime, whether
    /// it is anime, and the library title with this TMDB ID.
    ///
    /// # Errors
    /// As [`Self::title_hits`]; a TMDB ID that does not exist is
    /// [`TorrentServiceError::NotFound`].
    pub async fn title_details(
        &self,
        kind: MediaKind,
        tmdb_id: u64,
    ) -> Result<TitleDetails, TorrentServiceError> {
        self.ensure_accepted()?;
        let provider = self.tmdb()?;
        let mut ids = ProviderIds::new();
        ids.set(id_key::TMDB, tmdb_id.to_string());
        let item = provider
            .details(provider_kind(kind), &ids, &self.request_options())
            .await?;

        let (english, runtime, certification, genres) = match &item.detail {
            ItemDetail::Movie(movie) => (
                &movie.english_title,
                movie.runtime_minutes,
                &movie.certification,
                &movie.genres,
            ),
            ItemDetail::Show(show) => (
                &show.english_title,
                show.runtime_minutes,
                &show.certification,
                &show.genres,
            ),
            _ => return Err(TorrentServiceError::NotFound),
        };
        let certification = certification_text(&certification.0);
        let genres: Vec<String> = genres.iter().map(genre_label).collect();
        let title = item.core.title.trim().to_owned();
        let original_title = clean(&item.core.original_title).filter(|o| *o != title);
        let mut alternative_titles = Vec::new();
        if let Some(english) = clean(english)
            && english != title
            && Some(&english) != original_title.as_ref()
        {
            alternative_titles.push(english);
        }
        let library_item = linked_title(&self.db, kind, tmdb_id)
            .map_err(|e| TorrentServiceError::Library(e.to_string()))?;
        // What the library holds is a nicety of the header: a read failure leaves it out.
        let holding = library_item.as_ref().and_then(|link| {
            holding_of(&self.db, link).unwrap_or_else(|error| {
                tracing::debug!(%error, "library copy not read");
                None
            })
        });
        let rating = item
            .core
            .ratings
            .on_ten_scale(cv_domain::rating_key::TMDB)
            .filter(|value| *value > 0.0);
        Ok(TitleDetails {
            kind,
            ids: TitleIds {
                tmdb: Some(tmdb_id),
                imdb: item.core.ids.get(id_key::IMDB).map(str::to_owned),
                tvdb: item.core.ids.get_u64(id_key::TVDB),
            },
            title,
            original_title,
            alternative_titles,
            year: year_of(item.core.year),
            runtime_minutes: (runtime > 0).then_some(runtime),
            anime: looks_like_anime(&item),
            poster_url: item.core.artwork_urls.get(&MediaFileType::Poster).cloned(),
            library_item,
            backdrop_url: item.core.artwork_urls.get(&MediaFileType::Fanart).cloned(),
            certification,
            rating,
            genres,
            overview: clean(&item.core.plot),
            holding,
        })
    }

    /// The query to search releases for a confirmed title and scope: the title's names, year,
    /// runtime, whether it is anime and the copy the library holds.
    ///
    /// # Errors
    /// [`TorrentServiceError::Library`] when the library cannot be read.
    pub fn build_query(
        &self,
        details: &TitleDetails,
        scope: Scope,
    ) -> Result<TorrentQuery, TorrentServiceError> {
        self.ensure_accepted()?;
        let scope = match details.kind {
            MediaKind::Movie => Scope::Movie,
            MediaKind::Show => match scope {
                Scope::Movie => Scope::Show,
                other => other,
            },
        };
        let library_copy = library_copy(&self.db, details.library_item.as_ref(), scope)
            .map_err(|e| TorrentServiceError::Library(e.to_string()))?;
        Ok(TorrentQuery {
            kind: details.kind,
            ids: details.ids.clone(),
            title: details.title.clone(),
            original_title: details.original_title.clone(),
            alternative_titles: details.alternative_titles.clone(),
            year: details.year,
            scope,
            runtime_minutes: details.runtime_minutes,
            anime: details.anime,
            library_copy,
        })
    }

    /// A show's episodes in aired order, kept for ten minutes.
    async fn episode_rows(
        &self,
        tmdb_id: u64,
    ) -> Result<Arc<Vec<EpisodeRow>>, TorrentServiceError> {
        if let Some((at, rows)) = self
            .episodes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&tmdb_id)
            && at.elapsed() < EPISODE_LIST_TTL
        {
            return Ok(Arc::clone(rows));
        }
        let provider = self.tmdb()?;
        let mut ids = ProviderIds::new();
        ids.set(id_key::TMDB, tmdb_id.to_string());
        let items = provider
            .episodes(&ids, &EpisodeGroup::aired(), &self.request_options())
            .await?;
        let mut rows: Vec<EpisodeRow> = items
            .iter()
            .filter_map(|item| {
                let ItemDetail::Episode(fields) = &item.detail else {
                    return None;
                };
                let number = fields.numbers.display(&EpisodeGroup::aired())?;
                Some(EpisodeRow {
                    season: u16::try_from(number.season).ok()?,
                    episode: u16::try_from(number.episode).ok()?,
                    name: item.core.title.trim().to_owned(),
                    air_date: fields.first_aired,
                })
            })
            .collect();
        rows.sort_by_key(|r| (r.season, r.episode));
        let rows = Arc::new(rows);
        self.episodes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(tmdb_id, (Instant::now(), Arc::clone(&rows)));
        Ok(rows)
    }

    /// The seasons of a show: number, name, episode count and the day the first episode aired.
    /// Specials (season 0) come last.
    ///
    /// # Errors
    /// As [`Self::title_hits`].
    pub async fn seasons(&self, tmdb_id: u64) -> Result<Vec<SeasonInfo>, TorrentServiceError> {
        self.ensure_accepted()?;
        let rows = self.episode_rows(tmdb_id).await?;
        let mut seasons: Vec<SeasonInfo> = Vec::new();
        for row in rows.iter() {
            match seasons.last_mut() {
                Some(last) if last.number == row.season => {
                    last.episode_count += 1;
                    if last.air_date.is_none() {
                        last.air_date = date_text(row.air_date);
                    }
                }
                _ => seasons.push(SeasonInfo {
                    number: row.season,
                    name: if row.season == 0 {
                        "Specials".to_owned()
                    } else {
                        format!("Season {}", row.season)
                    },
                    episode_count: 1,
                    air_date: date_text(row.air_date),
                }),
            }
        }
        // Season 0 sorts first by number; the picker wants it last.
        if seasons.first().is_some_and(|s| s.number == 0) {
            seasons.rotate_left(1);
        }
        Ok(seasons)
    }

    /// The episodes of one season.
    ///
    /// # Errors
    /// As [`Self::title_hits`].
    pub async fn episodes(
        &self,
        tmdb_id: u64,
        season: u16,
    ) -> Result<Vec<EpisodeInfo>, TorrentServiceError> {
        self.ensure_accepted()?;
        let rows = self.episode_rows(tmdb_id).await?;
        Ok(rows
            .iter()
            .filter(|r| r.season == season)
            .map(|r| EpisodeInfo {
                number: r.episode,
                name: r.name.clone(),
                air_date: date_text(r.air_date),
            })
            .collect())
    }
}

/// How much of a hit's confidence is its fit; the rest (at most `cv_match`'s popularity weight,
/// 0.1) is how well known it is.
const FIT_SHARE: f64 = 0.9;
/// The fit at which a hit counts as the title that was typed.
const NAMED_FIT: f64 = 0.85;

/// How well a hit fits a query: the title (or the original title) less a penalty for another year.
fn fit(query: &NormalizedQuery, hit: &SearchHit) -> f64 {
    let similarity = title_similarity(&query.text, &hit.title)
        .max(title_similarity(&query.text, &hit.original_title));
    (similarity - year_penalty(query.year, hit.year)).clamp(0.0, 1.0)
}

/// `Heat 1995` read as the title `Heat` of 1995: a last word of four digits that is a plausible
/// release year (not later than two years from now), after some other text. `None` when the query
/// already has a year, is an ID shortcut, or does not end in a year (`Blade Runner 2049`).
fn trailing_year(query: &NormalizedQuery) -> Option<NormalizedQuery> {
    if query.id.is_some() || query.year.is_some() {
        return None;
    }
    let (rest, last) = query.text.trim().rsplit_once(' ')?;
    if last.len() != 4 || !last.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: u16 = last.parse().ok()?;
    let latest = u16::try_from(chrono::Utc::now().year() + 2).unwrap_or(u16::MAX);
    let rest = rest.trim();
    ((1880..=latest).contains(&year) && rest.chars().count() >= 2).then(|| NormalizedQuery {
        text: rest.to_owned(),
        year: Some(year),
        id: None,
    })
}

fn shortcut_key(shortcut: &IdShortcut) -> &'static str {
    shortcut.provider
}

impl From<ProviderError> for TorrentServiceError {
    fn from(error: ProviderError) -> Self {
        match error.root() {
            ProviderError::MissingCredentials { .. } => Self::NeedsKey,
            ProviderError::Auth { .. } => Self::KeyRejected,
            ProviderError::NotFound(_) => Self::NotFound,
            ProviderError::RateLimited { .. } | ProviderError::Request { .. } => Self::Provider {
                message: error.to_string(),
                retryable: true,
            },
            _ => Self::Provider {
                message: error.to_string(),
                retryable: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(text: &str) -> NormalizedQuery {
        normalize_query(text)
    }

    #[test]
    fn certifications_read_as_people_know_them() {
        assert_eq!(certification_text("US_PG13").as_deref(), Some("PG-13"));
        assert_eq!(certification_text("US_TV_MA").as_deref(), Some("TV-MA"));
        assert_eq!(certification_text("R").as_deref(), Some("R"));
        assert_eq!(certification_text("  "), None);
    }

    #[test]
    fn a_trailing_year_is_read_as_the_year() {
        let split = trailing_year(&query("Heat 1995")).expect("a year");
        assert_eq!(split.text, "Heat");
        assert_eq!(split.year, Some(1995));
    }

    #[test]
    fn a_number_that_is_no_plausible_year_stays_in_the_title() {
        assert!(trailing_year(&query("Blade Runner 2049")).is_none());
        assert!(trailing_year(&query("1917")).is_none(), "nothing before it");
        assert!(
            trailing_year(&query("Heat (1995)")).is_none(),
            "already has a year"
        );
        assert!(trailing_year(&query("Apollo 13")).is_none());
        assert!(trailing_year(&query("tt0113277")).is_none());
    }

    fn hit(title: &str, year: u16) -> SearchHit {
        SearchHit {
            provider: "tmdb".to_owned(),
            ids: ProviderIds::new(),
            title: title.to_owned(),
            original_title: String::new(),
            year,
            overview: String::new(),
            poster_url: None,
            exact_id: false,
            popularity: 0.0,
            votes: 0,
        }
    }

    #[test]
    fn the_year_puts_the_1995_movie_above_the_2015_show() {
        let split = trailing_year(&query("heat 1995")).unwrap();
        let movie = fit(&split, &hit("Heat", 1995));
        let show = fit(&split, &hit("HEAT", 2015));
        assert!(movie > show, "{movie} > {show}");
        assert!(movie >= NAMED_FIT);
    }

    #[test]
    fn votes_break_a_tie_between_two_titles_of_one_name() {
        let plain = query("Heat");
        let score = |year, votes| {
            FIT_SHARE * fit(&plain, &hit("Heat", year))
                + popularity_bonus(&Candidate {
                    votes,
                    ..Candidate::default()
                })
        };
        let known = score(1995, 7000);
        let obscure = score(2015, 12);
        assert!(known > obscure);
        assert!(known <= 1.0);
    }
}

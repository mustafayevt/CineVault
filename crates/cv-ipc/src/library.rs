//! Typed commands that feed the library views: windows of cards, keyset pages and counts.
//!
//! The rules live in `cv-db` (filters, sort keys, keyset and offset paging) and `cv-artwork` (poster
//! keys); this module reads one page, adds what a card shows and converts to the DTOs the UI sees.
//! Cards carry an image *key*, never image bytes: the UI builds a
//! `cv://img/<key>/grid` URL, and the ThumbHash and ambient colour travel with the card so a tile can
//! paint before its thumbnail arrives.
//!
//! # Windows and pages
//!
//! - [`list_library_window`] returns rows `offset..offset + limit`. The grid asks for the windows that
//!   cover what is on screen, so dragging the scrollbar to row 15,000 is one request.
//! - [`list_library_rows`] returns the same rows with every column the table view can show.
//!
//! Both answer with the `total` for the query, which sizes the scroll area before any row is loaded.
//! Reads run on a blocking thread and never write.

use std::sync::Arc;

use cv_app::hero_audio::hero_audio;
use cv_app::lifecycle::LifecycleService;
use cv_artwork::library_source::LibrarySources;
use cv_artwork::thumb::{ThumbCache, ThumbKey};
use cv_db::repo::facet::facet_values;
use cv_db::repo::filter::{Filter, IntRange, MatchMode, TitleColumn};
use cv_db::repo::graphics::{VideoFacts, card_file_names, unmatched_among, video_facts_for};
use cv_db::repo::people::{PersonRole, search_people};
use cv_db::repo::table_facts::table_facts_for;
use cv_db::repo::{
    Direction, ItemFlag, ItemQuery, ItemSummary, SortField, SortKey, count_items, count_new,
    list_items_at,
};
use cv_db::{Database, DbError};
use cv_domain::{CreditKind, ItemId, ItemKind, RatingPreference};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::library_facets::{
    FacetValueDto, FilterDefDto, LibraryFacetDto, certification_label, filter_catalogue,
    labelled_values, parse_filter_id,
};
use crate::lifecycle::{LifecycleDto, lifecycles_of};

/// The most rows one request returns; a bigger ask is shortened, never refused.
const MAX_ROWS: u32 = 500;

/// Which library a listing shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKindDto {
    Movies,
    Shows,
    Collections,
}

impl From<LibraryKindDto> for ItemKind {
    fn from(kind: LibraryKindDto) -> Self {
        match kind {
            LibraryKindDto::Movies => Self::Movie,
            LibraryKindDto::Shows => Self::Show,
            LibraryKindDto::Collections => Self::Set,
        }
    }
}

/// A column a listing can be sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySortFieldDto {
    Title,
    SortTitle,
    Year,
    DateAdded,
    ReleaseDate,
    Runtime,
    OriginalTitle,
    Filename,
    Path,
    Top250,
    Certification,
    Edition,
    MediaSource,
    /// The label of the library folder the title lives on ("NAS 4K").
    Source,
    /// The pipeline stage: New, Scraped, then Renamed.
    Stage,
    Watched,
    IsNew,
    MovieSet,
    Rating,
    Votes,
    UserRating,
    ImdbRating,
    TmdbRating,
    TraktRating,
    RottenTomatoesRating,
    RottenTomatoesAudienceRating,
    MetacriticRating,
    LetterboxdRating,
    MyAnimeListRating,
    RogerEbertRating,
    TvdbRating,
    VideoHeight,
    AspectRatio,
    AspectRatio2,
    VideoCodec,
    VideoBitrate,
    Hdr,
    /// Resolution rank of the main video, then HDR; titles without one last.
    Quality,
    /// The folder the title's folder sits in ("D:\Movies").
    Folder,
    FileSize,
    TotalFileSize,
    FileModified,
    AudioStreams,
    SubtitleStreams,
    Trailers,
    Images,
    Metadata,
    Seasons,
    Episodes,
    MissingEpisodes,
    MoviesInSet,
    MissingMovies,
    AiredStatus,
}

impl From<LibrarySortFieldDto> for SortField {
    fn from(field: LibrarySortFieldDto) -> Self {
        match field {
            LibrarySortFieldDto::Title => Self::Title,
            LibrarySortFieldDto::SortTitle => Self::SortTitle,
            LibrarySortFieldDto::Year => Self::Year,
            LibrarySortFieldDto::DateAdded => Self::DateAdded,
            LibrarySortFieldDto::ReleaseDate => Self::ReleaseDate,
            LibrarySortFieldDto::Runtime => Self::Runtime,
            LibrarySortFieldDto::OriginalTitle => Self::OriginalTitle,
            LibrarySortFieldDto::Filename => Self::Filename,
            LibrarySortFieldDto::Path => Self::Path,
            LibrarySortFieldDto::Top250 => Self::Top250,
            LibrarySortFieldDto::Certification => Self::Certification,
            LibrarySortFieldDto::Edition => Self::Edition,
            LibrarySortFieldDto::MediaSource => Self::MediaSource,
            LibrarySortFieldDto::Source => Self::Source,
            LibrarySortFieldDto::Stage => Self::Stage,
            LibrarySortFieldDto::Watched => Self::Watched,
            LibrarySortFieldDto::IsNew => Self::IsNew,
            LibrarySortFieldDto::MovieSet => Self::MovieSet,
            LibrarySortFieldDto::Rating => Self::Rating,
            LibrarySortFieldDto::Votes => Self::Votes,
            LibrarySortFieldDto::UserRating => Self::UserRating,
            LibrarySortFieldDto::ImdbRating => Self::ImdbRating,
            LibrarySortFieldDto::TmdbRating => Self::TmdbRating,
            LibrarySortFieldDto::TraktRating => Self::TraktRating,
            LibrarySortFieldDto::RottenTomatoesRating => Self::RottenTomatoesRating,
            LibrarySortFieldDto::RottenTomatoesAudienceRating => Self::RottenTomatoesAudienceRating,
            LibrarySortFieldDto::MetacriticRating => Self::MetacriticRating,
            LibrarySortFieldDto::LetterboxdRating => Self::LetterboxdRating,
            LibrarySortFieldDto::MyAnimeListRating => Self::MyAnimeListRating,
            LibrarySortFieldDto::RogerEbertRating => Self::RogerEbertRating,
            LibrarySortFieldDto::TvdbRating => Self::TvdbRating,
            LibrarySortFieldDto::VideoHeight => Self::VideoHeight,
            LibrarySortFieldDto::AspectRatio => Self::AspectRatio,
            LibrarySortFieldDto::AspectRatio2 => Self::AspectRatio2,
            LibrarySortFieldDto::VideoCodec => Self::VideoCodec,
            LibrarySortFieldDto::VideoBitrate => Self::VideoBitrate,
            LibrarySortFieldDto::Hdr => Self::Hdr,
            LibrarySortFieldDto::Quality => Self::Quality,
            LibrarySortFieldDto::Folder => Self::Folder,
            LibrarySortFieldDto::FileSize => Self::FileSize,
            LibrarySortFieldDto::TotalFileSize => Self::TotalFileSize,
            LibrarySortFieldDto::FileModified => Self::FileModified,
            LibrarySortFieldDto::AudioStreams => Self::AudioStreams,
            LibrarySortFieldDto::SubtitleStreams => Self::SubtitleStreams,
            LibrarySortFieldDto::Trailers => Self::Trailers,
            LibrarySortFieldDto::Images => Self::Images,
            LibrarySortFieldDto::Metadata => Self::Metadata,
            LibrarySortFieldDto::Seasons => Self::Seasons,
            LibrarySortFieldDto::Episodes => Self::Episodes,
            LibrarySortFieldDto::MissingEpisodes => Self::MissingEpisodes,
            LibrarySortFieldDto::MoviesInSet => Self::MoviesInSet,
            LibrarySortFieldDto::MissingMovies => Self::MissingMovies,
            LibrarySortFieldDto::AiredStatus => Self::AiredStatus,
        }
    }
}

/// One sort column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LibrarySortKeyDto {
    pub field: LibrarySortFieldDto,
    /// True for descending order.
    pub descending: bool,
}

/// The field a filter chip tests. The quick search (`hdr:dv year:2010..2019 -watched`) and, later,
/// the filter picker produce these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LibraryChipFieldDto {
    // Flags: no values and no range.
    Watched,
    New,
    /// What Needs review counts: uncertain matches, titles whose search found nothing and
    /// automatic-rename conflicts (the lifecycle's rule, not the stored review state).
    Review,
    Locked,
    /// No provider match: the lifecycle's Not matched flag.
    NotMatched,
    /// Scraped titles whose names a rename would change: the lifecycle's Rename pending flag.
    RenamePending,
    /// Files or library folder out of reach: the lifecycle's Missing flag.
    Missing,
    // Ranges: `min` and `max`, inclusive, either may be open.
    Year,
    Runtime,
    Top250,
    /// The preferred rating (or its fallback) on the 0 to 10 scale.
    Rating,
    ImdbRating,
    TmdbRating,
    RottenTomatoesRating,
    MetacriticRating,
    UserRating,
    // Value lists: any one of `values` matches.
    Resolution,
    Hdr,
    Codec,
    AudioLanguage,
    SubtitleLanguage,
    Genre,
    Actor,
    Director,
    Writer,
    Certification,
    Edition,
    /// The release source (Blu-ray, DVD, ...).
    Source,
    /// The label of the library folder the title lives on ("NAS 4K").
    LibrarySource,
    /// The pipeline stage: `new`, `scraped` or `renamed`.
    Stage,
    // Words a title contains: any one of `values` matches.
    Title,
    OriginalTitle,
    SortTitle,
}

/// One filter chip: a field, its value(s) and whether it is negated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct LibraryChipDto {
    pub field: LibraryChipFieldDto,
    /// `true` for "not": the chip keeps what the filter would leave out.
    pub negated: bool,
    /// The values of a list field; any one matches. Empty for flags and ranges.
    pub values: Vec<String>,
    /// Lower bound of a range field, inclusive; `None` is open.
    pub min: Option<f64>,
    /// Upper bound of a range field, inclusive; `None` is open.
    pub max: Option<f64>,
}

/// What to list: a library, its sort, an optional quick-search text and filter chips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct LibraryQueryDto {
    pub kind: LibraryKindDto,
    /// Sort columns in priority order; empty means title ascending.
    pub sort: Vec<LibrarySortKeyDto>,
    /// Quick-search text; empty lists everything.
    pub search: String,
    /// Filter chips; an item must satisfy every one. Empty lists everything.
    pub filters: Vec<LibraryChipDto>,
    /// Chips from the full filter catalogue (`library_filter_catalogue`), also all required.
    #[serde(default)]
    #[specta(optional)]
    pub facets: Vec<LibraryFacetDto>,
    /// The preferred rating source (`imdb`, `tmdb`, ...). The "rating" column, the rating sort and
    /// the `rating` chip use it, falling back to the next available source for a title without
    /// one. Omitted means the default source.
    #[serde(default)]
    #[specta(optional)]
    pub rating_source: Option<String>,
}

/// The frame heights the `resolution:` search filter reads for a label: bands of the height, so
/// "480p" is everything below 576p and a cropped film still counts as its release.
fn resolution_band(label: &str) -> Option<IntRange> {
    match label.trim().to_ascii_lowercase().as_str() {
        "2160p" => Some(IntRange::at_least(1500)),
        "1080p" => Some(IntRange::between(760, 1499)),
        "720p" => Some(IntRange::between(600, 759)),
        "576p" => Some(IntRange::between(500, 599)),
        "480p" => Some(IntRange::between(1, 499)),
        _ => None,
    }
}

/// A whole-number range from typed bounds: a fractional bound rounds inwards.
#[allow(clippy::cast_possible_truncation)]
fn whole_range(min: Option<f64>, max: Option<f64>) -> IntRange {
    IntRange {
        min: min.map(|v| v.ceil() as i64),
        max: max.map(|v| v.floor() as i64),
    }
}

fn any_of(mut filters: Vec<Filter>) -> Option<Filter> {
    match filters.len() {
        0 => None,
        1 => filters.pop(),
        _ => Some(Filter::or(filters)),
    }
}

/// What the lifecycle chips (`stage`, `review`, `not_matched`, `rename_pending`, `missing`) read
/// besides the library: the naming profile of the organize-state cache. The pipeline counts read
/// the same one, so a count and its filter agree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ChipContext {
    /// The active naming profile; empty reads the profile of the newest computation.
    pub profile_key: String,
}

impl ChipContext {
    fn lifecycle(&self, flag: &str) -> Filter {
        Filter::Lifecycle {
            flags: vec![flag.to_owned()],
            profile_key: self.profile_key.clone(),
        }
    }
}

impl LibraryChipDto {
    fn has_bound(&self) -> bool {
        self.min.is_some() || self.max.is_some()
    }

    /// The condition of a list chip, or `None` when it has no usable value.
    fn list(&self, build: impl FnOnce(Vec<String>) -> Filter) -> Option<Filter> {
        let values: Vec<String> = self
            .values
            .iter()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .collect();
        if values.is_empty() {
            None
        } else {
            Some(build(values))
        }
    }

    fn person(&self, kind: CreditKind) -> Option<Filter> {
        self.list(|names| {
            let people = names
                .into_iter()
                .map(|name| Filter::Person {
                    name,
                    kind: Some(kind),
                })
                .collect();
            any_of(people).unwrap_or_default()
        })
    }

    fn title(&self, column: TitleColumn) -> Option<Filter> {
        self.list(|terms| Filter::TitleContains { column, terms })
    }

    fn rating(&self, sources: &[&str]) -> Option<Filter> {
        if !self.has_bound() {
            return None;
        }
        any_of(
            sources
                .iter()
                .map(|source| Filter::Rating {
                    source: (*source).to_owned(),
                    min: self.min,
                    max: self.max,
                })
                .collect(),
        )
    }

    fn range(&self, build: impl FnOnce(IntRange) -> Filter) -> Option<Filter> {
        self.has_bound()
            .then(|| build(whole_range(self.min, self.max)))
    }

    /// The condition of the chip, or `None` when it restricts nothing (a list without values, a
    /// range without bounds).
    fn to_filter(&self, preference: &RatingPreference, context: &ChipContext) -> Option<Filter> {
        use LibraryChipFieldDto as F;
        let flag = match self.field {
            F::Watched => Some(ItemFlag::Watched),
            F::New => Some(ItemFlag::New),
            F::Locked => Some(ItemFlag::Locked),
            _ => None,
        };
        if let Some(flag) = flag {
            // A negated flag is the flag turned off, so items without a value still match.
            return Some(Filter::Flag {
                flag,
                value: !self.negated,
            });
        }
        let positive = match self.field {
            F::Review => Some(context.lifecycle("needs-review")),
            F::NotMatched => Some(context.lifecycle("not-matched")),
            F::RenamePending => Some(context.lifecycle("rename-pending")),
            F::Missing => Some(context.lifecycle("missing")),
            F::Year => self.range(|range| Filter::Year { range }),
            F::Runtime => self.range(|range| Filter::RuntimeMinutes { range }),
            F::Top250 => self.range(|range| Filter::Top250 { range }),
            F::Rating => self.has_bound().then(|| Filter::PreferredRating {
                preference: preference.clone(),
                min: self.min,
                max: self.max,
            }),
            F::ImdbRating => self.rating(&["imdb"]),
            F::TmdbRating => self.rating(&["tmdb"]),
            F::RottenTomatoesRating => self.rating(&["tomatometerallcritics"]),
            F::MetacriticRating => self.rating(&["metacritic"]),
            F::UserRating => self.rating(&["user"]),
            F::Resolution => any_of(
                self.values
                    .iter()
                    .filter_map(|label| resolution_band(label))
                    .map(|range| Filter::VideoHeight { range })
                    .collect(),
            ),
            F::Hdr => self.list(|values| Filter::Hdr {
                values,
                mode: MatchMode::Any,
            }),
            F::Codec => self.list(|values| Filter::VideoCodec { values }),
            F::AudioLanguage => self.list(|values| Filter::AudioLanguage { values }),
            F::SubtitleLanguage => self.list(|values| Filter::SubtitleLanguage { values }),
            F::Genre => self.list(|names| Filter::Genre {
                names,
                mode: MatchMode::Any,
            }),
            F::Actor => self.person(CreditKind::Actor),
            F::Director => self.person(CreditKind::Director),
            F::Writer => self.person(CreditKind::Writer),
            F::Certification => self.list(|values| Filter::Certification { values }),
            F::Edition => self.list(|values| Filter::Edition { values }),
            F::Source => self.list(|values| Filter::MediaSource { values }),
            F::LibrarySource => self.list(|values| Filter::SourceLabel { values }),
            F::Stage => self.list(|stages| Filter::Stage {
                stages,
                profile_key: context.profile_key.clone(),
            }),
            F::Title => self.title(TitleColumn::Title),
            F::OriginalTitle => self.title(TitleColumn::OriginalTitle),
            F::SortTitle => self.title(TitleColumn::SortTitle),
            F::Watched | F::New | F::Locked => None,
        }?;
        Some(if self.negated {
            Filter::not(positive)
        } else {
            positive
        })
    }
}

impl LibraryQueryDto {
    /// The preferred rating source of the query.
    fn rating_preference(&self) -> RatingPreference {
        self.rating_source
            .as_deref()
            .map(RatingPreference::new)
            .unwrap_or_default()
    }

    /// The query, with the lifecycle chips read as the pipeline counts read them (the same
    /// naming profile and rename mode).
    pub(crate) fn to_query_for(
        &self,
        context: &ChipContext,
    ) -> Result<ItemQuery, LibraryCommandError> {
        let preference = self.rating_preference();
        let mut query = ItemQuery::new(self.kind.into())
            .rating_preference(preference.clone())
            .profile_key(&context.profile_key);
        for key in &self.sort {
            query = query.sort_by(SortKey {
                field: key.field.into(),
                direction: if key.descending {
                    Direction::Desc
                } else {
                    Direction::Asc
                },
            });
        }
        let mut filters = Vec::new();
        if !self.search.trim().is_empty() {
            filters.push(Filter::Search {
                text: self.search.clone(),
            });
            // On the default title sort, the titles that hold the words come before the ones
            // found only by their plot, people or tags. Another sort the user chose is kept.
            let by_title = self.sort.first().is_none_or(|key| {
                matches!(key.field, LibrarySortFieldDto::Title) && !key.descending
            });
            if by_title {
                query = query.title_first(self.search.clone());
            }
        }
        filters.extend(
            self.filters
                .iter()
                .filter_map(|chip| chip.to_filter(&preference, context)),
        );
        for facet in &self.facets {
            filters.push(Filter::Facet(facet.to_condition()?));
        }
        if !filters.is_empty() {
            query = query.filter(Filter::and(filters));
        }
        Ok(query)
    }
}

/// The image of a card: a key for `cv://img` plus what is known for painting before it loads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PosterDto {
    /// Content key of the poster; build the URL with `imageUrl(key, "grid")`.
    pub key: String,
    /// ThumbHash of the poster as hex, once its thumbnail has been built.
    pub thumbhash: Option<String>,
    /// The poster's ambient colour as `#rrggbb`, once its thumbnail has been built.
    pub ambient: Option<String>,
}

/// Everything a poster card shows.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct LibraryCardDto {
    /// Item id as text, so it is safe in JavaScript and usable as a selection id.
    pub id: String,
    pub title: String,
    /// The title in its original language, empty when there is none. Shown instead of `title`
    /// when `providers.prefer-original-title` is on.
    pub original_title: String,
    /// Release year, `0` when unknown.
    pub year: u32,
    /// Found by a scan and not yet seen.
    pub is_new: bool,
    pub needs_review: bool,
    /// The stage, flags and explanation lines; `None` for a collection.
    pub lifecycle: Option<LifecycleDto>,
    /// No provider has identified this item.
    pub unmatched: bool,
    pub watched: bool,
    /// The name an unmatched item is shown by: its folder name, or its video's file name when it
    /// shares its folder with other movies.
    pub file_name: String,
    /// Approximate resolution class such as `2160p`; `None` when unknown.
    pub resolution: Option<String>,
    /// Short HDR label such as `DV` or `HDR10`; `None` for SDR or unknown.
    pub hdr: Option<String>,
    pub poster: Option<PosterDto>,
}

/// One row of the list and table views: the card plus every column the table can show.
///
/// Numbers that can exceed 32 bits (sizes, timestamps) are `f64`, which is exact up to 2^53, so
/// they stay plain numbers in TypeScript. Ratings are on the 0 to 10 scale.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct LibraryRowDto {
    pub card: LibraryCardDto,
    pub original_title: String,
    pub sort_title: String,
    /// The start of the plot, for the list view.
    pub plot_excerpt: String,
    /// Premiere or first-aired date as ISO text.
    pub release_date: Option<String>,
    pub runtime_minutes: u32,
    pub path: String,
    /// The label of the library folder the title lives on; empty when that folder was removed.
    pub source_label: String,
    /// File name of the main video file.
    pub filename: String,
    /// The movie set a movie belongs to.
    pub movie_set: Option<String>,
    pub certification: String,
    pub edition: String,
    pub media_source: String,
    pub note: String,
    pub locked: bool,
    pub video_in_3d: bool,
    /// Epoch milliseconds.
    pub date_added_ms: f64,
    pub top250: u32,
    /// The best rating (0 to 10) and where it came from.
    pub rating: Option<f64>,
    pub rating_source: Option<String>,
    /// True when `rating` comes from a source other than the preferred one, because the title has
    /// no rating from it.
    pub rating_is_fallback: bool,
    pub votes: u32,
    pub user_rating: Option<f64>,
    pub imdb_rating: Option<f64>,
    pub tmdb_rating: Option<f64>,
    pub trakt_rating: Option<f64>,
    /// Rotten Tomatoes, all critics (stored as a percentage, shown here out of 10).
    pub rotten_tomatoes_rating: Option<f64>,
    /// Rotten Tomatoes, audience score.
    pub rotten_tomatoes_audience_rating: Option<f64>,
    pub metacritic_rating: Option<f64>,
    pub letterboxd_rating: Option<f64>,
    pub myanimelist_rating: Option<f64>,
    pub rogerebert_rating: Option<f64>,
    /// Technical facts of the main video file; empty or zero when unknown.
    pub video_codec: String,
    pub container: String,
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: f64,
    /// The secondary aspect ratio found by the letterbox detector, `0` when none.
    pub aspect_ratio2: f64,
    pub video_bitrate_kbps: u32,
    pub hdr_formats: Vec<String>,
    /// Size of the main video file in bytes, and of every file of the item.
    pub file_size: f64,
    pub total_file_size: f64,
    /// Modification time of the main video file, epoch milliseconds; `None` without one.
    pub file_modified_ms: Option<f64>,
    pub audio_codec: String,
    pub audio_channels: u32,
    pub audio_streams: u32,
    pub subtitle_streams: u32,
    pub trailers: u32,
    pub has_poster: bool,
    pub has_fanart: bool,
    pub has_logo: bool,
    /// A Kodi-style NFO file sits next to the item.
    pub has_nfo: bool,
    /// Subtitle files next to the video; the embedded ones are `subtitle_streams`.
    pub subtitle_files: u32,
    /// Identified, with a plot, a year and a runtime.
    pub metadata_complete: bool,
    pub seasons: u32,
    pub episodes: u32,
    pub missing_episodes: u32,
    pub movies_in_set: u32,
    pub missing_movies: u32,
    /// `continuing`, `ended`, `canceled` or `unknown` for a show.
    pub aired_status: String,
}

/// One group of a listing grouped by library folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SourceGroupDto {
    /// The folder's label; empty for titles whose folder was removed.
    pub label: String,
    pub count: u32,
}

/// Rows `offset..` of a listing, with everything the table can show.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct LibraryRowsWindowDto {
    /// How many items match the query in all.
    pub total: u32,
    pub offset: u32,
    pub items: Vec<LibraryRowDto>,
}

/// The plot is cut to this many characters for the list view.
const PLOT_EXCERPT_CHARS: usize = 280;

fn excerpt(plot: &str) -> String {
    let plot = plot.trim();
    if plot.chars().count() <= PLOT_EXCERPT_CHARS {
        return plot.to_owned();
    }
    let cut: String = plot.chars().take(PLOT_EXCERPT_CHARS).collect();
    format!("{}...", cut.trim_end())
}

#[allow(clippy::cast_precision_loss)]
fn exact(n: u64) -> f64 {
    n as f64
}

#[allow(clippy::cast_precision_loss)]
fn millis(n: i64) -> f64 {
    n as f64
}

/// Rows `offset..` of a listing.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct LibraryWindowDto {
    /// How many items match the query in all.
    pub total: u32,
    pub offset: u32,
    pub items: Vec<LibraryCardDto>,
}

/// An actor or director of the library, for the command palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PersonHitDto {
    pub name: String,
    /// `actor` or `director`.
    pub role: String,
    /// Movies and shows that credit the person in that role.
    pub titles: u32,
}

/// The numbers the sidebar shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct LibraryCountsDto {
    pub movies: u32,
    pub tv_shows: u32,
    pub collections: u32,
    /// Movies and shows waiting in the Needs review inbox.
    pub needs_review: u32,
    /// Movies and shows found by a scan and not yet seen (`new_movies + new_shows`).
    pub new_items: u32,
    /// Movies flagged NEW: what the Movies "New" filter lists.
    pub new_movies: u32,
    /// Shows with NEW episodes: what the TV "New episodes" filter lists.
    pub new_shows: u32,
    /// Movies and shows no provider has identified: the cards marked unmatched.
    pub unmatched: u32,
}

/// How much the movie or the TV library holds under the one size rule: each file once, online
/// titles only.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Type)]
pub struct LibrarySizeTotalDto {
    /// Bytes of the distinct files of online titles: what Home shows as the library size.
    pub counted_bytes: f64,
    /// Bytes of offline titles' files that no online title also holds, for "and N offline".
    pub offline_bytes: f64,
}

/// Why a library command did not answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct LibraryCommandError {
    /// Stable code: `storage`, `bad_cursor` or `bad_filter`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<DbError> for LibraryCommandError {
    fn from(error: DbError) -> Self {
        match error {
            DbError::InvalidCursor => Self {
                code: "bad_cursor".to_owned(),
                message: "The list changed; reload it to continue.".to_owned(),
            },
            DbError::InvalidFilter(reason) => Self {
                code: "bad_filter".to_owned(),
                message: format!("That filter cannot be applied: {reason}"),
            },
            other => Self {
                code: "storage".to_owned(),
                message: format!("The library could not be read: {other}"),
            },
        }
    }
}

/// A storage failure in the library command shape.
pub fn storage(message: String) -> LibraryCommandError {
    LibraryCommandError {
        code: "storage".to_owned(),
        message,
    }
}

fn small(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Reads the library for the browsing views.
pub struct LibraryReader {
    db: Arc<Database>,
    sources: Arc<LibrarySources>,
    cache: Option<Arc<ThumbCache>>,
    lifecycle: Option<Arc<LifecycleService>>,
}

impl LibraryReader {
    pub fn new(
        db: Arc<Database>,
        sources: Arc<LibrarySources>,
        cache: Option<Arc<ThumbCache>>,
    ) -> Self {
        Self {
            db,
            sources,
            cache,
            lifecycle: None,
        }
    }

    /// Also puts the stage, flags and explanation of every title on its row, and reads the
    /// Needs review number from the lifecycle counts.
    #[must_use]
    pub fn with_lifecycle(mut self, lifecycle: Option<Arc<LifecycleService>>) -> Self {
        self.lifecycle = lifecycle;
        self
    }

    /// A reader over the parts a surface holds: the shared library, the poster index, the
    /// thumbnail cache and, when the library opened, the lifecycle service.
    #[must_use]
    pub fn from_parts(
        db: Arc<Database>,
        sources: Arc<LibrarySources>,
        cache: Option<Arc<ThumbCache>>,
        lifecycle: Option<Arc<LifecycleService>>,
    ) -> Self {
        Self::new(db, sources, cache).with_lifecycle(lifecycle)
    }

    /// What the lifecycle chips read: the active naming profile (else "the newest"), as the
    /// pipeline counts read it.
    fn chip_context(&self) -> ChipContext {
        self.lifecycle
            .as_ref()
            .map(|lifecycle| ChipContext {
                profile_key: lifecycle.profile_key(),
            })
            .unwrap_or_default()
    }

    /// Rows `offset..offset + limit` and the total. Blocking.
    pub fn window(
        &self,
        query: &LibraryQueryDto,
        offset: u32,
        limit: u32,
    ) -> Result<LibraryWindowDto, LibraryCommandError> {
        let limit = limit.clamp(1, MAX_ROWS) as usize;
        let query = query.to_query_for(&self.chip_context())?;
        let total = self.db.read(|conn| count_items(conn, &query))?;
        let rows = self
            .db
            .read(|conn| list_items_at(conn, &query, offset as usize, limit))?;
        Ok(LibraryWindowDto {
            total: small(total),
            offset,
            items: self.cards(rows)?,
        })
    }

    /// Rows `offset..offset + limit` with every table column, and the total. Blocking.
    pub fn rows_window(
        &self,
        query: &LibraryQueryDto,
        offset: u32,
        limit: u32,
    ) -> Result<LibraryRowsWindowDto, LibraryCommandError> {
        let limit = limit.clamp(1, MAX_ROWS) as usize;
        let query = query.to_query_for(&self.chip_context())?;
        let total = self.db.read(|conn| count_items(conn, &query))?;
        let rows = self
            .db
            .read(|conn| list_items_at(conn, &query, offset as usize, limit))?;
        Ok(LibraryRowsWindowDto {
            total: small(total),
            offset,
            items: self.table_rows(rows, &query.rating)?,
        })
    }

    /// The values the list filter `facet` has in the library of `query`, with the number of items
    /// that have each one among those matching the query's other chips. Pass the query without the
    /// chip of `facet` itself, so its own values do not narrow its list. Blocking.
    pub fn facet_values(
        &self,
        query: &LibraryQueryDto,
        facet: &str,
    ) -> Result<Vec<FacetValueDto>, LibraryCommandError> {
        let id = parse_filter_id(facet)?;
        let query = query.to_query_for(&self.chip_context())?;
        let values = self
            .db
            .read(|conn| facet_values(conn, query.kind, id, &query.filter))?;
        Ok(labelled_values(id, values))
    }

    /// The titles of the query grouped by the label of their library folder, in label order: the
    /// group headers of the grid, list and table (sort by `Source` to put the rows in the same
    /// order). Blocking.
    pub fn source_groups(
        &self,
        query: &LibraryQueryDto,
    ) -> Result<Vec<SourceGroupDto>, LibraryCommandError> {
        let query = query.to_query_for(&self.chip_context())?;
        let groups = self
            .db
            .read(|conn| cv_db::repo::source_groups(conn, &query))?;
        Ok(groups
            .into_iter()
            .map(|group| SourceGroupDto {
                label: group.label,
                count: u32::try_from(group.count).unwrap_or(u32::MAX),
            })
            .collect())
    }

    /// The actors and directors whose name contains `term`, most credited first. Blocking.
    pub fn people(&self, term: &str, limit: u32) -> Result<Vec<PersonHitDto>, LibraryCommandError> {
        let found = self
            .db
            .read(|conn| search_people(conn, term, limit as usize))?;
        Ok(found
            .into_iter()
            .map(|person| PersonHitDto {
                name: person.name,
                role: match person.role {
                    PersonRole::Actor => "actor",
                    PersonRole::Director => "director",
                }
                .to_owned(),
                titles: person.titles,
            })
            .collect())
    }

    /// The size of the movie or the TV library. Blocking.
    pub fn size_total(&self, kind: ItemKind) -> Result<LibrarySizeTotalDto, LibraryCommandError> {
        if !matches!(kind, ItemKind::Movie | ItemKind::Show) {
            return Err(LibraryCommandError {
                code: "bad_filter".to_owned(),
                message: "Collections have no size of their own.".to_owned(),
            });
        }
        let total = self
            .db
            .read(|conn| cv_db::repo::size_totals::library_size_total(conn, kind))?;
        Ok(LibrarySizeTotalDto {
            counted_bytes: exact(total.counted_bytes),
            offline_bytes: exact(total.offline_bytes),
        })
    }

    /// The sidebar numbers. Blocking.
    pub fn counts(&self) -> Result<LibraryCountsDto, LibraryCommandError> {
        let of_kind = |kind: ItemKind| {
            self.db
                .read(|conn| count_items(conn, &ItemQuery::new(kind)))
                .map(small)
        };
        let review = |kind: ItemKind| {
            let query = ItemQuery::new(kind).filter(Filter::Flag {
                flag: ItemFlag::NeedsReview,
                value: true,
            });
            self.db.read(|conn| count_items(conn, &query)).map(small)
        };
        let new_movies = self
            .db
            .read(|conn| count_new(conn, Some(ItemKind::Movie)))
            .map(small)?;
        let new_shows = self
            .db
            .read(cv_db::repo::flags::count_shows_with_new_episodes)
            .map(small)?;
        let unmatched = self
            .db
            .read(cv_db::repo::graphics::count_unmatched)
            .map(small)?;
        // The number is the lifecycle's: uncertain matches, titles that could not be matched and
        // automatic-rename conflicts, the same figure Home and the Needs review page show.
        let needs_review = match &self.lifecycle {
            Some(lifecycle) => {
                let of = |kind| lifecycle.counts(kind).map(|c| c.needs_review);
                of(ItemKind::Movie)?.saturating_add(of(ItemKind::Show)?)
            }
            None => review(ItemKind::Movie)?.saturating_add(review(ItemKind::Show)?),
        };
        Ok(LibraryCountsDto {
            movies: of_kind(ItemKind::Movie)?,
            tv_shows: of_kind(ItemKind::Show)?,
            collections: of_kind(ItemKind::Set)?,
            needs_review,
            new_items: new_movies.saturating_add(new_shows),
            new_movies,
            new_shows,
            unmatched,
        })
    }

    /// Adds poster keys, video facts and the unmatched marker to a page of rows.
    fn cards(&self, rows: Vec<ItemSummary>) -> Result<Vec<LibraryCardDto>, LibraryCommandError> {
        let ids: Vec<ItemId> = rows.iter().map(|row| row.id).collect();
        let posters = self.sources.posters_for(&self.db, &ids)?;
        let mut lifecycles = lifecycles_of(self.lifecycle.as_ref(), &ids);
        let (unmatched, facts, names) = self.db.read(|conn| {
            Ok((
                unmatched_among(conn, &ids)?,
                video_facts_for(conn, &ids)?,
                card_file_names(conn, &ids)?,
            ))
        })?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let poster = posters.get(&row.id).map(|key| self.poster(key));
                let video = facts.get(&row.id);
                let lifecycle = lifecycles.remove(&row.id);
                // The REVIEW marker is what Needs review lists: the lifecycle's rule, which leaves
                // out a review the user set aside and adds an automatic-rename conflict.
                let needs_review = lifecycle.as_ref().map_or_else(
                    || row.review_state == "needs_review",
                    |lifecycle| lifecycle.in_review,
                );
                LibraryCardDto {
                    id: row.id.0.to_string(),
                    unmatched: unmatched.contains(&row.id) && row.kind != ItemKind::Set,
                    file_name: names
                        .get(&row.id)
                        .cloned()
                        .unwrap_or_else(|| row.title.clone()),
                    resolution: video.and_then(resolution_label),
                    hdr: video.and_then(hdr_label),
                    lifecycle,
                    needs_review,
                    is_new: row.is_new,
                    watched: row.watched,
                    year: u32::from(row.year),
                    original_title: row.original_title,
                    title: row.title,
                    poster,
                }
            })
            .collect())
    }

    /// Adds the table columns to a page of rows (and the card each row carries).
    fn table_rows(
        &self,
        rows: Vec<ItemSummary>,
        preference: &RatingPreference,
    ) -> Result<Vec<LibraryRowDto>, LibraryCommandError> {
        let ids: Vec<ItemId> = rows.iter().map(|row| row.id).collect();
        let facts = self
            .db
            .read(|conn| table_facts_for(conn, &ids, preference))?;
        let summaries = rows.clone();
        let cards = self.cards(rows)?;
        Ok(summaries
            .into_iter()
            .zip(cards)
            .map(|(row, card)| {
                let fact = facts.get(&row.id).cloned().unwrap_or_default();
                let video = fact.video.clone().unwrap_or_default();
                // The Audio column leads with the hero track, as the detail pages do.
                let hero = hero_audio(&fact.audio_tracks, &fact.original_language);
                LibraryRowDto {
                    card,
                    original_title: row.original_title,
                    sort_title: row.sort_title,
                    plot_excerpt: excerpt(&fact.plot),
                    release_date: row.release_date,
                    runtime_minutes: row.runtime_minutes,
                    filename: video.filename.clone(),
                    source_label: row.source_label,
                    path: row.path,
                    movie_set: fact.movie_set,
                    certification: certification_label(&fact.certification),
                    edition: fact.edition,
                    media_source: fact.media_source,
                    note: fact.note,
                    locked: row.locked,
                    video_in_3d: fact.video_in_3d,
                    date_added_ms: millis(row.date_added_ms),
                    top250: row.top250,
                    rating: fact.rating,
                    rating_source: fact.rating_source,
                    rating_is_fallback: fact.rating_is_fallback,
                    votes: fact.votes,
                    user_rating: fact.user_rating,
                    imdb_rating: fact.imdb_rating,
                    tmdb_rating: fact.tmdb_rating,
                    trakt_rating: fact.trakt_rating,
                    rotten_tomatoes_rating: fact.rotten_tomatoes_rating,
                    rotten_tomatoes_audience_rating: fact.rotten_tomatoes_audience_rating,
                    metacritic_rating: fact.metacritic_rating,
                    letterboxd_rating: fact.letterboxd_rating,
                    myanimelist_rating: fact.myanimelist_rating,
                    rogerebert_rating: fact.rogerebert_rating,
                    video_codec: video.codec,
                    container: video.container,
                    width: video.width,
                    height: video.height,
                    aspect_ratio: video.aspect_ratio,
                    aspect_ratio2: video.aspect_ratio2,
                    video_bitrate_kbps: video.bitrate_kbps,
                    hdr_formats: video.hdr_formats,
                    file_size: exact(video.size_bytes),
                    total_file_size: exact(fact.total_size_bytes),
                    file_modified_ms: fact.video.is_some().then(|| millis(video.modified_ms)),
                    audio_codec: hero.map_or(fact.audio_codec, |a| a.codec.clone()),
                    audio_channels: hero.map_or(fact.audio_channels, |a| a.channels),
                    audio_streams: fact.audio_streams,
                    subtitle_streams: fact.subtitle_streams,
                    trailers: fact.trailers,
                    has_poster: fact.has_poster,
                    has_fanart: fact.has_fanart,
                    has_logo: fact.has_logo,
                    has_nfo: fact.has_nfo,
                    subtitle_files: fact.subtitle_files,
                    metadata_complete: fact.metadata_complete,
                    seasons: fact.seasons,
                    episodes: fact.episodes,
                    missing_episodes: fact.missing_episodes,
                    movies_in_set: fact.movies_in_set,
                    missing_movies: fact.missing_movies,
                    aired_status: fact.aired_status,
                }
            })
            .collect())
    }

    fn poster(&self, key: &ThumbKey) -> PosterDto {
        poster_dto(self.cache.as_deref(), key)
    }
}

/// The image reference of a thumbnail key, with the ThumbHash and ambient colour once its
/// thumbnail has been built.
pub fn poster_dto(cache: Option<&ThumbCache>, key: &ThumbKey) -> PosterDto {
    let meta = cache.and_then(|cache| cache.meta(key));
    PosterDto {
        key: key.to_string(),
        thumbhash: meta.as_ref().map(|m| m.thumbhash.clone()),
        ambient: meta
            .and_then(|m| m.palette)
            .map(|palette| format!("#{}", palette.ambient.hex())),
    }
}

/// The resolution label of a card: the same buckets the detail page, the technical badges and
/// the resolution filter use (`cv_media::resolution`), so one file never reads "480p" on its
/// card and "144p" on its page. A cropped wide-screen film is read by its width (1920x800 is
/// 1080p).
fn resolution_label(video: &VideoFacts) -> Option<String> {
    cv_media::resolution::resolution_label(video.width, video.height).map(str::to_owned)
}

/// The most notable HDR format as a short label.
fn hdr_label(video: &VideoFacts) -> Option<String> {
    let has = |name: &str| {
        video
            .hdr_formats
            .iter()
            .any(|f| f.eq_ignore_ascii_case(name))
    };
    if has("Dolby Vision") {
        return Some("DV".to_owned());
    }
    ["HDR10+", "HDR10", "HLG"]
        .into_iter()
        .find(|name| has(name))
        .map(str::to_owned)
        .or_else(|| video.hdr_formats.first().cloned())
}

/// Runs blocking library work off the async workers and turns a join failure into a storage error.
pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, LibraryCommandError> + Send + 'static,
) -> Result<T, LibraryCommandError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| storage(error.to_string()))?
}

/// Rows `offset..offset + limit` of a listing, with the total.
pub async fn list_library_window(
    reader: LibraryReader,
    query: LibraryQueryDto,
    offset: u32,
    limit: u32,
) -> Result<LibraryWindowDto, LibraryCommandError> {
    blocking(move || reader.window(&query, offset, limit)).await
}

/// Rows `offset..offset + limit` of a listing with every table column, and the total.
pub async fn list_library_rows(
    reader: LibraryReader,
    query: LibraryQueryDto,
    offset: u32,
    limit: u32,
) -> Result<LibraryRowsWindowDto, LibraryCommandError> {
    blocking(move || reader.rows_window(&query, offset, limit)).await
}

/// The filters of the full catalogue that exist for a library, in picker order.
pub fn library_filter_catalogue(kind: LibraryKindDto) -> Vec<FilterDefDto> {
    filter_catalogue(kind)
}

/// The values of one list filter with their item counts, narrowed by the query's other chips.
pub async fn library_facet_values(
    reader: LibraryReader,
    query: LibraryQueryDto,
    facet: String,
) -> Result<Vec<FacetValueDto>, LibraryCommandError> {
    blocking(move || reader.facet_values(&query, &facet)).await
}

/// The matching titles grouped by library folder label, with their counts.
pub async fn library_source_groups(
    reader: LibraryReader,
    query: LibraryQueryDto,
) -> Result<Vec<SourceGroupDto>, LibraryCommandError> {
    blocking(move || reader.source_groups(&query)).await
}

/// The actors and directors whose name contains `term` (all of them for an empty term), most
/// credited first, at most `limit`. The command palette's `@` search.
pub async fn search_library_people(
    reader: LibraryReader,
    term: String,
    limit: u32,
) -> Result<Vec<PersonHitDto>, LibraryCommandError> {
    blocking(move || reader.people(&term, limit)).await
}

/// The size of the movie or the TV library: every file once, online titles only, plus what is
/// offline. Home shows these two numbers.
pub async fn library_size_total(
    reader: LibraryReader,
    kind: LibraryKindDto,
) -> Result<LibrarySizeTotalDto, LibraryCommandError> {
    let kind = ItemKind::from(kind);
    blocking(move || reader.size_total(kind)).await
}

/// The counts the sidebar shows.
pub async fn library_counts(
    reader: LibraryReader,
) -> Result<LibraryCountsDto, LibraryCommandError> {
    blocking(move || reader.counts()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_facets::{FacetModeDto, FacetOpDto};
    use cv_domain::{Item, MediaFile, MediaFileType, id_key};

    struct Fixture {
        dir: tempfile::TempDir,
        db: Arc<Database>,
        sources: Arc<LibrarySources>,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
        Fixture {
            dir,
            db,
            sources: Arc::new(LibrarySources::new()),
        }
    }

    impl Fixture {
        fn reader(&self) -> LibraryReader {
            LibraryReader::new(self.db.clone(), self.sources.clone(), None)
        }

        fn add(&self, item: Item) -> ItemId {
            self.db
                .write(move |conn| cv_db::repo::insert_item(conn, &item))
                .unwrap()
        }

        fn movie(&self, title: &str, year: u16) -> Item {
            let folder = self.dir.path().join(title);
            std::fs::create_dir_all(&folder).unwrap();
            let mut item = Item::movie(title);
            item.core.year = year;
            item.core.path = folder.to_string_lossy().into_owned();
            item.core.ids.set(id_key::TMDB, "1");
            item
        }
    }

    fn query() -> LibraryQueryDto {
        LibraryQueryDto {
            kind: LibraryKindDto::Movies,
            sort: Vec::new(),
            search: String::new(),
            filters: Vec::new(),
            facets: Vec::new(),
            rating_source: None,
        }
    }

    #[test]
    fn a_window_returns_cards_with_markers_posters_and_the_total() {
        let f = fixture();
        let mut heat = f.movie("Heat", 1995);
        heat.core.media_files = vec![
            MediaFile {
                kind: MediaFileType::Poster,
                directory: heat.core.path.clone(),
                filename: "poster.jpg".into(),
                ..MediaFile::default()
            },
            MediaFile {
                kind: MediaFileType::Video,
                directory: heat.core.path.clone(),
                filename: "heat.mkv".into(),
                height: 2160,
                hdr_formats: vec!["HDR10".into(), "Dolby Vision".into()],
                ..MediaFile::default()
            },
        ];
        std::fs::write(
            std::path::Path::new(&heat.core.path).join("poster.jpg"),
            b"poster",
        )
        .unwrap();
        let heat_id = f.add(heat);
        let mut loose = f.movie("heat.1995.1080p", 0);
        loose.core.ids = Default::default();
        let loose_id = f.add(loose);
        f.db.write(move |conn| {
            cv_db::repo::set_new(conn, &[loose_id], true)?;
            cv_db::repo::set_review_state(conn, loose_id, cv_domain::ReviewState::NeedsReview)
        })
        .unwrap();

        let window = f.reader().window(&query(), 0, 50).unwrap();

        assert_eq!(window.total, 2);
        let heat_card = window.items.iter().find(|c| c.title == "Heat").unwrap();
        assert_eq!(heat_card.id, heat_id.0.to_string());
        assert_eq!(heat_card.year, 1995);
        assert_eq!(heat_card.resolution.as_deref(), Some("2160p"));
        assert_eq!(heat_card.hdr.as_deref(), Some("DV"));
        assert_eq!(heat_card.file_name, "Heat");
        assert!(!heat_card.unmatched && !heat_card.needs_review);
        let key = ThumbKey::of_bytes(b"poster").to_string();
        assert_eq!(
            heat_card.poster.as_ref().map(|p| p.key.as_str()),
            Some(key.as_str())
        );

        let loose_card = window
            .items
            .iter()
            .find(|c| c.title == "heat.1995.1080p")
            .unwrap();
        assert!(loose_card.unmatched && loose_card.needs_review && loose_card.is_new);
        assert!(
            loose_card.poster.is_none(),
            "no poster file: the UI draws a placeholder"
        );
    }

    #[test]
    fn rows_carry_the_source_label_and_sort_filter_and_group_by_it() {
        let f = fixture();
        let nas =
            f.db.write(|conn| {
                let source = cv_db::repo::data_sources::insert_source(
                    conn,
                    cv_domain::SourceKind::Movie,
                    r"D:\Movies",
                )?;
                cv_db::repo::data_sources::set_source_label(conn, source.id, "NAS 4K")?;
                cv_db::repo::data_sources::insert_source(
                    conn,
                    cv_domain::SourceKind::Movie,
                    r"E:\Archive",
                )
            })
            .unwrap();
        assert_eq!(nas.label, "Archive");
        for (title, root) in [
            ("Heat", r"D:\Movies"),
            ("Alien", r"E:\Archive"),
            ("Dune", r"D:\Movies"),
        ] {
            let mut item = f.movie(title, 2000);
            item.core.data_source = root.into();
            f.add(item);
        }

        let mut by_source = query();
        by_source.sort = vec![
            LibrarySortKeyDto {
                field: LibrarySortFieldDto::Source,
                descending: false,
            },
            LibrarySortKeyDto {
                field: LibrarySortFieldDto::Title,
                descending: false,
            },
        ];
        let rows = f.reader().rows_window(&by_source, 0, 50).unwrap();
        let listed: Vec<(&str, &str)> = rows
            .items
            .iter()
            .map(|r| (r.card.title.as_str(), r.source_label.as_str()))
            .collect();
        assert_eq!(
            listed,
            [("Alien", "Archive"), ("Dune", "NAS 4K"), ("Heat", "NAS 4K")]
        );

        let mut filtered = query();
        filtered.filters = vec![LibraryChipDto {
            field: LibraryChipFieldDto::LibrarySource,
            negated: false,
            values: vec!["nas 4k".into()],
            min: None,
            max: None,
        }];
        assert_eq!(f.reader().window(&filtered, 0, 50).unwrap().total, 2);

        let groups = f.reader().source_groups(&query()).unwrap();
        let pairs: Vec<(&str, u32)> = groups.iter().map(|g| (g.label.as_str(), g.count)).collect();
        assert_eq!(pairs, [("Archive", 1), ("NAS 4K", 2)]);
    }

    #[test]
    fn the_stage_chip_lists_the_titles_of_that_stage() {
        let f = fixture();
        let mut dune = f.movie("Dune", 2021);
        dune.core.ids.set(cv_domain::id_key::TMDB, "438631");
        dune.core.last_scraper_id = "tmdb".into();
        f.add(dune);
        f.add(f.movie("Alien", 1979));
        let stage = |value: &str| {
            let mut q = query();
            q.filters = vec![LibraryChipDto {
                field: LibraryChipFieldDto::Stage,
                negated: false,
                values: vec![value.into()],
                min: None,
                max: None,
            }];
            f.reader().window(&q, 0, 10).unwrap()
        };
        let new = stage("new");
        assert_eq!(new.total, 1);
        assert_eq!(new.items[0].title, "Alien");
        let scraped = stage("scraped");
        assert_eq!(scraped.total, 1);
        assert_eq!(scraped.items[0].title, "Dune");
        assert_eq!(stage("renamed").total, 0);
    }

    #[test]
    fn a_rows_window_carries_the_table_columns_in_the_requested_order() {
        let f = fixture();
        let mut dune = f.movie("Dune", 2021);
        dune.core.plot = "Spice.".into();
        dune.core
            .ratings
            .insert(cv_domain::Rating::new("tmdb", 8.0, 12, 10));
        dune.core.media_files = vec![MediaFile {
            kind: MediaFileType::Video,
            directory: dune.core.path.clone(),
            filename: "dune.mkv".into(),
            size_bytes: 2_000_000_000,
            height: 2160,
            video_codec: "HEVC".into(),
            ..MediaFile::default()
        }];
        f.add(dune);
        f.add(f.movie("Alien", 1979));
        f.add(f.movie("Blade", 1979));

        // Year descending, then title: the table's multi-sort with its title tie-break.
        let mut by_year = query();
        by_year.sort = vec![
            LibrarySortKeyDto {
                field: LibrarySortFieldDto::Year,
                descending: true,
            },
            LibrarySortKeyDto {
                field: LibrarySortFieldDto::Title,
                descending: false,
            },
        ];
        let window = f.reader().rows_window(&by_year, 0, 10).unwrap();

        assert_eq!(window.total, 3);
        let titles: Vec<&str> = window.items.iter().map(|r| r.card.title.as_str()).collect();
        assert_eq!(titles, ["Dune", "Alien", "Blade"]);
        let dune = &window.items[0];
        assert_eq!(dune.plot_excerpt, "Spice.");
        assert_eq!(dune.rating, Some(8.0));
        assert_eq!(dune.rating_source.as_deref(), Some("tmdb"));
        assert_eq!((dune.height, dune.video_codec.as_str()), (2160, "HEVC"));
        assert_eq!(dune.filename, "dune.mkv");
        assert!((dune.file_size - 2e9).abs() < 1.0);
        assert_eq!(window.items[1].rating, None);

        // Sorting by the best rating puts unrated titles last when descending.
        let mut by_rating = query();
        by_rating.sort = vec![LibrarySortKeyDto {
            field: LibrarySortFieldDto::Rating,
            descending: true,
        }];
        let first = f.reader().rows_window(&by_rating, 0, 1).unwrap();
        assert_eq!(first.items[0].card.title, "Dune");
        assert!(excerpt(&"word ".repeat(100)).ends_with("..."));
    }

    /// The Audio column leads with the hero track: the English original, not the French dub that
    /// is marked default, and never a commentary.
    #[test]
    fn the_audio_column_shows_the_hero_track() {
        use cv_domain::{AudioStream, StreamFlag};
        let f = fixture();
        let track =
            |codec: &str, language: &str, channels: u32, flags: &[StreamFlag]| AudioStream {
                codec: codec.to_owned(),
                language: language.to_owned(),
                channels,
                flags: flags.iter().copied().collect(),
                ..AudioStream::default()
            };
        let mut item = f.movie("Heat", 1995);
        item.core.original_language = "en".to_owned();
        let mut file = MediaFile::new(MediaFileType::Video, item.core.path.clone(), "heat.mkv");
        file.audio_streams = vec![
            track("AC3", "fra", 6, &[StreamFlag::Default, StreamFlag::Dub]),
            track("DTS-HD MA", "eng", 8, &[]),
            track("AC3", "eng", 2, &[StreamFlag::Comment]),
        ];
        item.core.media_files.push(file);
        f.add(item);

        let rows = f.reader().rows_window(&query(), 0, 10).unwrap().items;

        assert_eq!(rows[0].audio_streams, 3);
        assert_eq!(rows[0].audio_codec, "DTS-HD MA");
        assert_eq!(rows[0].audio_channels, 8);
    }

    /// A library where the preferred source matters: A has IMDb and TMDB, B only TMDB, C only
    /// Letterboxd and Rotten Tomatoes.
    fn rated_library() -> Fixture {
        let f = fixture();
        let rate = |item: &mut Item, ratings: &[(&str, f32, u32)]| {
            for (source, value, max) in ratings {
                item.core
                    .ratings
                    .insert(cv_domain::Rating::new(*source, *value, 100, *max));
            }
        };
        let mut a = f.movie("A", 2001);
        rate(&mut a, &[("imdb", 9.0, 10), ("tmdb", 5.0, 10)]);
        let mut b = f.movie("B", 2002);
        rate(&mut b, &[("tmdb", 8.0, 10)]);
        let mut c = f.movie("C", 2003);
        rate(
            &mut c,
            &[("letterboxd", 4.4, 5), ("tomatometerallcritics", 95.0, 100)],
        );
        for item in [a, b, c] {
            f.add(item);
        }
        f
    }

    fn rows_by(f: &Fixture, query: &LibraryQueryDto) -> Vec<LibraryRowDto> {
        f.reader().rows_window(query, 0, 10).unwrap().items
    }

    #[test]
    fn the_rating_column_follows_the_preferred_source_and_marks_a_fallback() {
        let f = rated_library();
        let mut q = query();
        q.rating_source = Some("imdb".to_owned());
        let rows = rows_by(&f, &q);
        let by_title = |t: &str| rows.iter().find(|r| r.card.title == t).unwrap().clone();

        let a = by_title("A");
        assert_eq!(
            (a.rating, a.rating_source.as_deref()),
            (Some(9.0), Some("imdb"))
        );
        assert!(!a.rating_is_fallback);
        // B has no IMDb rating: TMDB stands in and is marked.
        let b = by_title("B");
        assert_eq!(
            (b.rating, b.rating_source.as_deref()),
            (Some(8.0), Some("tmdb"))
        );
        assert!(b.rating_is_fallback);
        // The same rows with TMDB preferred: nothing falls back for A and B.
        q.rating_source = Some("tmdb".to_owned());
        let rows = rows_by(&f, &q);
        assert!(
            rows.iter()
                .filter(|r| r.card.title != "C")
                .all(|r| !r.rating_is_fallback)
        );
        // No preference sent means the default (TMDB).
        q.rating_source = None;
        assert_eq!(
            rows_by(&f, &q)
                .iter()
                .find(|r| r.card.title == "A")
                .unwrap()
                .rating,
            Some(5.0)
        );
    }

    #[test]
    fn sorting_by_rating_uses_the_preferred_source() {
        let f = rated_library();
        let sorted = |preference: &str, field: LibrarySortFieldDto| {
            let mut q = query();
            q.rating_source = Some(preference.to_owned());
            q.sort = vec![LibrarySortKeyDto {
                field,
                descending: true,
            }];
            rows_by(&f, &q)
                .into_iter()
                .map(|r| r.card.title)
                .collect::<Vec<_>>()
        };
        // IMDb preferred: A 9, B 8 (TMDB), C 8.8 (Letterboxd on ten).
        assert_eq!(sorted("imdb", LibrarySortFieldDto::Rating), ["A", "C", "B"]);
        // TMDB preferred: B 8, A 5, C (fallback Letterboxd 8.8).
        assert_eq!(sorted("tmdb", LibrarySortFieldDto::Rating), ["C", "B", "A"]);
        // Per source: Letterboxd and Rotten Tomatoes only C has them; others sort after.
        assert_eq!(
            sorted("imdb", LibrarySortFieldDto::LetterboxdRating)[0],
            "C"
        );
        assert_eq!(
            sorted("imdb", LibrarySortFieldDto::RottenTomatoesRating)[0],
            "C"
        );
    }

    #[test]
    fn rating_chips_use_the_preferred_source_and_the_real_rotten_tomatoes_key() {
        use LibraryChipFieldDto as F;
        let f = rated_library();
        let titles = |preference: Option<&str>, chip: LibraryChipDto| {
            let mut q = LibraryQueryDto {
                filters: vec![chip],
                ..query()
            };
            q.rating_source = preference.map(str::to_owned);
            let mut titles: Vec<String> =
                rows_by(&f, &q).into_iter().map(|r| r.card.title).collect();
            titles.sort();
            titles
        };
        let at_least = |field, min| LibraryChipDto {
            min: Some(min),
            ..chip(field)
        };
        // "rating >= 8" with IMDb preferred: A (9 IMDb), B (8 from TMDB), C (Letterboxd 8.8).
        assert_eq!(
            titles(Some("imdb"), at_least(F::Rating, 8.0)),
            ["A", "B", "C"]
        );
        // With TMDB preferred A is 5, so it drops out.
        assert_eq!(titles(Some("tmdb"), at_least(F::Rating, 8.0)), ["B", "C"]);
        // The Rotten Tomatoes chip finds the rating stored under its canonical key.
        assert_eq!(titles(None, at_least(F::RottenTomatoesRating, 9.0)), ["C"]);
        // The Top 250 chip and the per-source facet agree on ranked titles.
        assert!(titles(None, at_least(F::Top250, 1.0)).is_empty());
    }

    #[test]
    fn per_source_facets_compare_in_the_units_of_the_source() {
        let f = rated_library();
        let facet = |id: &str, op: FacetOpDto, number: f64| LibraryFacetDto {
            id: id.to_owned(),
            negated: false,
            mode: FacetModeDto::Any,
            op,
            values: Vec::new(),
            number: Some(number),
            number2: None,
        };
        let titles = |facet: LibraryFacetDto| {
            let q = LibraryQueryDto {
                facets: vec![facet],
                ..query()
            };
            let mut titles: Vec<String> =
                rows_by(&f, &q).into_iter().map(|r| r.card.title).collect();
            titles.sort();
            titles
        };
        // "Letterboxd >= 4.0" and "RT critics >= 90%" read as they are written.
        assert_eq!(
            titles(facet("rating_letterboxd", FacetOpDto::Ge, 4.0)),
            ["C"]
        );
        assert_eq!(
            titles(facet("rating_rt_critics", FacetOpDto::Ge, 90.0)),
            ["C"]
        );
        assert!(titles(facet("rating_rt_critics", FacetOpDto::Ge, 96.0)).is_empty());
        assert_eq!(titles(facet("rating_imdb", FacetOpDto::Ge, 9.0)), ["A"]);
        assert!(parse_filter_id("rating_letterboxd").is_ok());
    }

    #[test]
    fn windows_walk_the_whole_order_and_past_the_end_is_empty() {
        let f = fixture();
        for n in 0..25 {
            f.add(f.movie(&format!("Film {n:02}"), 2000 + n));
        }
        let reader = f.reader();
        let q = LibraryQueryDto {
            sort: vec![LibrarySortKeyDto {
                field: LibrarySortFieldDto::Year,
                descending: true,
            }],
            ..query()
        };

        let mut by_window = Vec::new();
        for offset in [0, 10, 20] {
            by_window.extend(
                reader
                    .window(&q, offset, 10)
                    .unwrap()
                    .items
                    .into_iter()
                    .map(|c| c.id),
            );
        }

        assert_eq!(by_window.len(), 25);
        // Newest year first.
        assert_eq!(reader.window(&q, 0, 1).unwrap().items[0].year, 2024);
        // Past the end is empty, not an error.
        assert!(reader.window(&q, 500, 10).unwrap().items.is_empty());
    }

    #[test]
    fn search_narrows_the_total() {
        let f = fixture();
        f.add(f.movie("Heat", 1995));
        f.add(f.movie("Ronin", 1998));
        let reader = f.reader();
        let q = LibraryQueryDto {
            search: "ronin".to_owned(),
            ..query()
        };

        let window = reader.window(&q, 0, 10).unwrap();
        assert_eq!(window.total, 1);
        assert_eq!(window.items[0].title, "Ronin");
    }

    #[test]
    fn a_search_on_the_default_sort_lists_title_matches_before_plot_matches() {
        let f = fixture();
        // Found by the plot only, and first in the alphabet.
        let mut by_plot = f.movie("Aardvark Tales", 2001);
        by_plot.core.plot = "Dark nights on the savannah".into();
        f.add(by_plot);
        f.add(f.movie("Zodiac of Dark Waters", 2007));
        let reader = f.reader();
        let q = LibraryQueryDto {
            search: "dark".to_owned(),
            ..query()
        };

        let titles: Vec<String> = reader
            .window(&q, 0, 10)
            .unwrap()
            .items
            .into_iter()
            .map(|c| c.title)
            .collect();
        assert_eq!(titles, ["Zodiac of Dark Waters", "Aardvark Tales"]);

        // A sort the user picked is kept: by year, the plot match is first.
        let by_year = LibraryQueryDto {
            sort: vec![LibrarySortKeyDto {
                field: LibrarySortFieldDto::Year,
                descending: false,
            }],
            ..q
        };
        let first = reader.window(&by_year, 0, 1).unwrap().items.remove(0);
        assert_eq!(first.title, "Aardvark Tales");
    }

    fn chip(field: LibraryChipFieldDto) -> LibraryChipDto {
        LibraryChipDto {
            field,
            negated: false,
            values: Vec::new(),
            min: None,
            max: None,
        }
    }

    fn titles_of(f: &Fixture, filters: Vec<LibraryChipDto>) -> Vec<String> {
        let q = LibraryQueryDto { filters, ..query() };
        let mut titles: Vec<String> = f
            .reader()
            .window(&q, 0, 50)
            .unwrap()
            .items
            .into_iter()
            .map(|c| c.title)
            .collect();
        titles.sort();
        titles
    }

    /// The `review`, `not_matched` and `missing` chips are the lifecycle's flags, so a pipeline
    /// count opens exactly its titles: a review the user set aside is not "needs review" any more
    /// but stays "not matched" (its scrape found no confident match), and a title no scrape was
    /// tried for is neither (QA final-07).
    #[test]
    fn the_lifecycle_chips_list_the_titles_the_pipeline_counts_count() {
        use LibraryChipFieldDto as F;
        let f = fixture();
        let heat = f.add(f.movie("Heat", 1995));
        let mut uncertain = f.movie("Uncertain", 2000);
        uncertain.core.ids = Default::default();
        let uncertain = f.add(uncertain);
        let mut skipped = f.movie("Skipped", 2001);
        skipped.core.ids = Default::default();
        let skipped = f.add(skipped);
        let mut fresh = f.movie("Fresh", 2002);
        fresh.core.ids = Default::default();
        f.add(fresh);
        f.db.write(move |conn| {
            cv_db::repo::review::enqueue(
                conn,
                &cv_db::repo::review::NewReview {
                    item_id: uncertain,
                    reason: "2 strong candidates".into(),
                    best_score: Some(0.6),
                    candidates: serde_json::json!([{}, {}]),
                    score_breakdown: None,
                },
            )?;
            // The stored state still says "needs review", but no decision waits any more; the
            // scrape that queued it found no confident match.
            cv_db::repo::set_review_state(conn, skipped, cv_domain::ReviewState::NeedsReview)?;
            cv_db::repo::step_outcome::record_step_outcome(
                conn,
                skipped,
                cv_db::repo::step_outcome::STEP_SCRAPE,
                cv_db::repo::step_outcome::StepStatus::NotFound,
                "2 strong candidates",
                1,
            )?;
            cv_db::repo::set_availability(conn, heat, cv_domain::Availability::Missing, "gone")
        })
        .unwrap();

        assert_eq!(titles_of(&f, vec![chip(F::Review)]), ["Uncertain"]);
        assert_eq!(titles_of(&f, vec![chip(F::NotMatched)]), ["Skipped"]);
        assert_eq!(titles_of(&f, vec![chip(F::Missing)]), ["Heat"]);
        assert!(titles_of(&f, vec![chip(F::RenamePending)]).is_empty());
        let not_review = LibraryChipDto {
            negated: true,
            ..chip(F::Review)
        };
        assert_eq!(
            titles_of(&f, vec![not_review]),
            ["Fresh", "Heat", "Skipped"]
        );
    }

    #[test]
    fn title_chips_match_words_of_the_title_original_title_or_sort_title() {
        use LibraryChipFieldDto as F;
        let f = fixture();
        let mut heat = f.movie("Heat", 1995);
        heat.core.original_title = "Heat (Hitze)".into();
        f.add(heat);
        let mut amelie = f.movie("Amélie", 2001);
        amelie.core.original_title = "Le Fabuleux Destin d'Amélie Poulain".into();
        if let cv_domain::ItemDetail::Movie(m) = &mut amelie.detail {
            m.sort_title = "Fabuleux Destin".into();
        }
        f.add(amelie);
        f.add(f.movie("Heathers", 1989));
        let words = |field, values: &[&str]| LibraryChipDto {
            values: values.iter().map(|v| (*v).to_owned()).collect(),
            ..chip(field)
        };

        assert_eq!(
            titles_of(&f, vec![words(F::Title, &["heat"])]),
            ["Heat", "Heathers"]
        );
        assert_eq!(
            titles_of(&f, vec![words(F::OriginalTitle, &["fabuleux"])]),
            ["Amélie"]
        );
        assert_eq!(
            titles_of(&f, vec![words(F::SortTitle, &["destin"])]),
            ["Amélie"]
        );
        // A wildcard is a literal character, and a negated chip keeps the rest.
        assert!(titles_of(&f, vec![words(F::Title, &["%"])]).is_empty());
        let not_heat = LibraryChipDto {
            negated: true,
            ..words(F::Title, &["heat"])
        };
        assert_eq!(titles_of(&f, vec![not_heat]), ["Amélie"]);
    }

    #[test]
    fn chips_narrow_a_listing_by_range_flag_hdr_and_negation() {
        use LibraryChipFieldDto as F;
        let f = fixture();
        let video = |item: &Item, height: u32, hdr: &[&str]| MediaFile {
            kind: MediaFileType::Video,
            directory: item.core.path.clone(),
            filename: "film.mkv".into(),
            height,
            hdr_formats: hdr.iter().map(|s| (*s).to_owned()).collect(),
            ..MediaFile::default()
        };
        let mut dune = f.movie("Dune", 2021);
        dune.core.media_files = vec![video(&dune, 2160, &["Dolby Vision"])];
        let mut heat = f.movie("Heat", 1995);
        heat.core.media_files = vec![video(&heat, 1080, &[])];
        let mut tenet = f.movie("Tenet", 2020);
        tenet.core.media_files = vec![video(&tenet, 2160, &["HDR10"])];
        let mut arrival = f.movie("Arrival", 2016);
        arrival.core.media_files = vec![video(&arrival, 1080, &["Dolby Vision"])];
        if let cv_domain::ItemDetail::Movie(m) = &mut arrival.detail {
            m.watched = true;
        }
        for item in [dune, heat, tenet, arrival] {
            f.add(item);
        }

        // hdr:dv year:2010..2019 -watched: the scenario of the quick search, as chips.
        let hdr = LibraryChipDto {
            values: vec!["Dolby Vision".into()],
            ..chip(F::Hdr)
        };
        let year = LibraryChipDto {
            min: Some(2010.0),
            max: Some(2019.0),
            ..chip(F::Year)
        };
        let unwatched = LibraryChipDto {
            negated: true,
            ..chip(F::Watched)
        };
        assert_eq!(titles_of(&f, vec![hdr.clone()]), ["Arrival", "Dune"]);
        assert_eq!(titles_of(&f, vec![hdr.clone(), year.clone()]), ["Arrival"]);
        assert!(titles_of(&f, vec![hdr.clone(), year.clone(), unwatched.clone()]).is_empty());
        assert_eq!(
            titles_of(&f, vec![hdr.clone(), unwatched]),
            ["Dune"],
            "a negated flag keeps the unwatched"
        );

        // A negated list chip inverts it; open-ended ranges and several values work too.
        let not_hdr = LibraryChipDto {
            negated: true,
            ..hdr
        };
        assert_eq!(titles_of(&f, vec![not_hdr]), ["Heat", "Tenet"]);
        let since_2020 = LibraryChipDto {
            min: Some(2020.0),
            ..chip(F::Year)
        };
        assert_eq!(titles_of(&f, vec![since_2020]), ["Dune", "Tenet"]);
        let uhd_or_hd = LibraryChipDto {
            values: vec!["2160p".into(), "1080p".into()],
            ..chip(F::Resolution)
        };
        assert_eq!(titles_of(&f, vec![uhd_or_hd]).len(), 4);
        let uhd = LibraryChipDto {
            values: vec!["2160p".into()],
            ..chip(F::Resolution)
        };
        assert_eq!(titles_of(&f, vec![uhd]), ["Dune", "Tenet"]);
    }

    fn facet(id: &str) -> LibraryFacetDto {
        LibraryFacetDto {
            id: id.to_owned(),
            negated: false,
            mode: FacetModeDto::Any,
            op: FacetOpDto::Eq,
            values: Vec::new(),
            number: None,
            number2: None,
        }
    }

    #[test]
    fn catalogue_facets_narrow_a_listing_negate_and_list_their_values() {
        let f = fixture();
        let mut heat = f.movie("Heat", 1995);
        heat.core.media_files = vec![MediaFile {
            width: 3840,
            height: 2160,
            video_codec: "HEVC".into(),
            video_bitrate_kbps: 20_000,
            ..MediaFile::new(MediaFileType::Video, "/media/Heat", "heat.mkv")
        }];
        let mut ronin = f.movie("Ronin", 1998);
        ronin.core.media_files = vec![MediaFile {
            width: 1920,
            height: 1080,
            video_codec: "AVC".into(),
            video_bitrate_kbps: 8_000,
            ..MediaFile::new(MediaFileType::Video, "/media/Ronin", "ronin.mkv")
        }];
        f.add(heat);
        f.add(ronin);
        f.add(f.movie("Thief", 1981));
        let reader = f.reader();
        let titles = |q: &LibraryQueryDto| -> Vec<String> {
            reader
                .window(q, 0, 10)
                .unwrap()
                .items
                .into_iter()
                .map(|c| c.title)
                .collect()
        };
        let with = |facets: Vec<LibraryFacetDto>| LibraryQueryDto { facets, ..query() };

        let resolution = LibraryFacetDto {
            values: vec!["2160p".into()],
            ..facet("video_format")
        };
        assert_eq!(titles(&with(vec![resolution.clone()])), ["Heat"]);
        let not_uhd = LibraryFacetDto {
            negated: true,
            ..resolution
        };
        assert_eq!(titles(&with(vec![not_uhd])), ["Ronin", "Thief"]);

        // A number filter with an operator, next to an older chip and the quick search.
        let bitrate = LibraryFacetDto {
            op: FacetOpDto::Between,
            number: Some(5_000.0),
            number2: Some(10_000.0),
            ..facet("video_bitrate")
        };
        let q = LibraryQueryDto {
            search: "ro".to_owned(),
            filters: vec![LibraryChipDto {
                min: Some(1990.0),
                max: None,
                ..chip(LibraryChipFieldDto::Year)
            }],
            facets: vec![bitrate],
            ..query()
        };
        assert_eq!(titles(&q), ["Ronin"]);

        // An empty list restricts nothing, as an empty chip does.
        assert_eq!(titles(&with(vec![facet("video_format")])).len(), 3);

        // The values of a list filter, counted over what the other chips leave.
        let values = |q: &LibraryQueryDto| -> Vec<(String, u32)> {
            reader
                .facet_values(q, "video_codec")
                .unwrap()
                .into_iter()
                .map(|v| (v.value, v.count))
                .collect()
        };
        assert_eq!(
            values(&query()),
            [("AVC".to_owned(), 1), ("HEVC".to_owned(), 1)]
        );
        assert_eq!(
            values(&LibraryQueryDto {
                filters: vec![LibraryChipDto {
                    min: Some(1996.0),
                    max: None,
                    ..chip(LibraryChipFieldDto::Year)
                }],
                ..query()
            }),
            [("AVC".to_owned(), 1)]
        );
    }

    #[test]
    fn unknown_or_foreign_catalogue_filters_are_refused_with_bad_filter() {
        let f = fixture();
        f.add(f.movie("Heat", 1995));
        let reader = f.reader();
        let ask = |id: &str| {
            reader.window(
                &LibraryQueryDto {
                    facets: vec![facet(id)],
                    ..query()
                },
                0,
                10,
            )
        };

        assert_eq!(ask("no_such_filter").unwrap_err().code, "bad_filter");
        // `status` is a TV show filter.
        assert_eq!(ask("status").unwrap_err().code, "bad_filter");
        assert_eq!(
            reader.facet_values(&query(), "year").unwrap_err().code,
            "bad_filter"
        );
        assert!(ask("watched").is_ok());
    }

    #[test]
    fn empty_chips_restrict_nothing_and_search_combines_with_chips() {
        use LibraryChipFieldDto as F;
        let f = fixture();
        f.add(f.movie("Heat", 1995));
        f.add(f.movie("Ronin", 1998));

        // A range without bounds and a list without values are ignored.
        assert_eq!(
            titles_of(&f, vec![chip(F::Year), chip(F::Genre), chip(F::Hdr)]),
            ["Heat", "Ronin"]
        );

        let q = LibraryQueryDto {
            search: "ro".to_owned(),
            filters: vec![LibraryChipDto {
                min: Some(1990.0),
                max: Some(1996.0),
                ..chip(F::Year)
            }],
            ..query()
        };
        assert_eq!(f.reader().window(&q, 0, 10).unwrap().total, 0);
        let q = LibraryQueryDto {
            filters: vec![LibraryChipDto {
                min: Some(1990.0),
                max: Some(1999.0),
                ..chip(F::Year)
            }],
            ..q
        };
        assert_eq!(f.reader().window(&q, 0, 10).unwrap().total, 1);
    }

    #[test]
    fn people_are_searched_by_name_with_their_role_and_title_count() {
        let f = fixture();
        let mut heat = f.movie("Heat", 1995);
        heat.core.credits = vec![
            cv_domain::Credit::new(CreditKind::Actor, "Al Pacino", "Vincent Hanna"),
            cv_domain::Credit::new(CreditKind::Director, "Michael Mann", ""),
        ];
        f.add(heat);

        let found = f.reader().people("al", 10).unwrap();
        assert_eq!(
            found,
            [PersonHitDto {
                name: "Al Pacino".into(),
                role: "actor".into(),
                titles: 1,
            }]
        );
        assert_eq!(f.reader().people("mann", 10).unwrap()[0].role, "director");
    }

    #[test]
    fn the_size_total_counts_each_file_once_and_reports_the_offline_remainder() {
        let f = fixture();
        let mut shared = Item::movie("Heat");
        shared.core.media_files = vec![MediaFile {
            kind: MediaFileType::Video,
            directory: "/m/Heat".into(),
            filename: "heat.mkv".into(),
            size_bytes: 4_000,
            ..MediaFile::default()
        }];
        let mut twin = shared.clone();
        twin.core.title = "Heat again".into();
        f.add(shared);
        f.add(twin);
        let mut away = Item::movie("Ronin");
        away.core.media_files = vec![MediaFile {
            kind: MediaFileType::Video,
            directory: "/off/Ronin".into(),
            filename: "ronin.mkv".into(),
            size_bytes: 1_500,
            ..MediaFile::default()
        }];
        let away = f.add(away);
        f.db.write(move |conn| {
            cv_db::repo::set_availability(conn, away, cv_domain::Availability::Offline, "unplugged")
        })
        .unwrap();

        let total = f.reader().size_total(ItemKind::Movie).unwrap();
        assert_eq!(
            (total.counted_bytes, total.offline_bytes),
            (4_000.0, 1_500.0)
        );
        assert!(f.reader().size_total(ItemKind::Set).is_err());
    }

    #[test]
    fn counts_cover_each_library_the_inbox_and_new_items() {
        let f = fixture();
        let a = f.add(f.movie("Heat", 1995));
        f.add(f.movie("Ronin", 1998));
        f.add(Item::show("Severance"));
        f.db.write(move |conn| {
            cv_db::repo::set_review_state(conn, a, cv_domain::ReviewState::NeedsReview)
        })
        .unwrap();

        let counts = f.reader().counts().unwrap();

        assert_eq!(counts.movies, 2);
        assert_eq!(counts.tv_shows, 1);
        assert_eq!(counts.collections, 0);
        assert_eq!(counts.needs_review, 1);

        // NEW counts follow what the Movies "New" and TV "New episodes" filters list.
        let show = f.add(Item::show("Dark"));
        let episode = f.add(Item::episode(show, "S01E01"));
        f.db.write(move |conn| cv_db::repo::set_new(conn, &[a, episode], true))
            .unwrap();
        let counts = f.reader().counts().unwrap();
        assert_eq!((counts.new_movies, counts.new_shows), (1, 1));
        assert_eq!(counts.new_items, 2);
    }

    /// reg2-2: the Needs review empty state says how many titles are still unmatched.
    #[test]
    fn counts_the_movies_and_shows_no_provider_identified() {
        let f = fixture();
        // The fixture's movies carry a TMDB ID; a movie without one is unmatched.
        f.add(f.movie("Heat", 1995));
        f.add(Item::movie("Ronin"));
        let show = f.add(Item::show("Severance"));
        f.add(Item::episode(show, "S01E01"));

        // Ronin and Severance; an episode is not a title of its own.
        assert_eq!(f.reader().counts().unwrap().unmatched, 2);
    }

    #[test]
    fn labels_follow_the_media_buckets_and_prefer_dolby_vision() {
        let sized = |width, height| VideoFacts {
            width,
            height,
            hdr_formats: Vec::new(),
        };
        let facts = |height, hdr: &[&str]| VideoFacts {
            width: 0,
            height,
            hdr_formats: hdr.iter().map(|s| (*s).to_owned()).collect(),
        };
        assert_eq!(resolution_label(&facts(0, &[])), None);
        assert_eq!(
            resolution_label(&sized(1920, 800)).as_deref(),
            Some("1080p")
        );
        assert_eq!(resolution_label(&sized(1280, 720)).as_deref(), Some("720p"));
        assert_eq!(
            resolution_label(&sized(3840, 1600)).as_deref(),
            Some("2160p")
        );
        // QA final-15: the card says what the detail page says for the same small file.
        assert_eq!(resolution_label(&sized(320, 180)).as_deref(), Some("144p"));
        assert_eq!(
            resolution_label(&sized(320, 180)).as_deref(),
            cv_media::resolution::resolution_label(320, 180)
        );
        assert_eq!(hdr_label(&facts(1080, &[])), None);
        assert_eq!(
            hdr_label(&facts(1080, &["HLG", "HDR10"])).as_deref(),
            Some("HDR10")
        );
        assert_eq!(
            hdr_label(&facts(1080, &["Dolby Vision"])).as_deref(),
            Some("DV")
        );
    }
}

//! Typed commands for the show, season and episode pages.
//!
//! [`get_show_detail`] answers the show page and the season pages with one read: the facts and
//! ratings of the hero, the technical badges (any episode may stand for a badge, like the library
//! filters of a show), the seasons with their artwork and counts, every episode of every season,
//! the episodes the provider lists but the library lacks (marked unaired when they have not aired
//! yet), the episodes without valid numbering, the guests and the artwork slots. The page filters
//! missing, unaired and special episodes itself, so its toggles need no round trip.
//! [`get_episode_detail`] answers the episode page. [`set_episodes_watched`] marks episodes
//! watched or not (a season or a show is its episodes).
//!
//! Numbers follow the show's chosen ordering and fall back to the aired numbers, like the NFO
//! writers. Images travel as content keys for the `cv://img` protocol, never as bytes. Reads run
//! on a blocking thread. Nothing here deletes anything.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use crate::detail_ipc::live_availability;
use cv_app::hero_audio::hero_audio;
use cv_app::lifecycle::LifecycleService;
use cv_app::nfo::model::{main_video, shown_number};
use cv_app::rating_status::RatingStatusService;
use cv_app::tv_info::{
    EpisodeAir, air_info, duplicate_numbers, season_art_fallback, with_show_fallback,
};
use cv_artwork::library_source::LibrarySources;
use cv_artwork::thumb::ThumbCache;
use cv_db::Database;
use cv_db::repo::{children_of, get_availability, get_flags, get_item};
use cv_domain::{
    AiredStatus, Availability, CalendarDate, CreditKind, EpisodeFields, EpisodeGroup,
    EpisodeOrdering, Item, ItemDetail, ItemId, MediaFileType, ShowFields,
};
use cv_media::resolution::resolution_label;
use cv_template::tokens::{audio_channels_dot, genre_label};
use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::curation_ipc::CurationState;
use crate::detail_ipc::{
    DetailArtworkDto, DetailCreditDto, DetailFileDto, DetailIdDto, DetailRatingDto, DetailReader,
    DetailSubtitleDto, DetailVideoDto, HeroAudioDto, RatingChipDto, TechBadgeDto, detail_ratings,
    enum_name, exact, hero_audio_of, millis, names_of, now_millis, parse_id, subtitles_of,
    video_files,
};
use crate::img_ipc::ImageState;
use crate::library_facets::certification_label;
use crate::library_ipc::{LibraryCommandError, PosterDto, blocking, storage};
use crate::lifecycle_ipc::LifecycleDto;
use crate::nfo_ipc::NfoState;
use crate::settings_ipc::SettingsState;
use crate::sources_ipc::LibraryState;

/// The most files the Files section lists.
const MAX_FILES: usize = 500;
/// The most technical badges of one facet a show shows.
const BADGES_PER_FACET: usize = 3;

/// The artwork slots of a show, in the order the page shows them.
const SHOW_SLOTS: [(&str, MediaFileType); 8] = [
    ("poster", MediaFileType::Poster),
    ("fanart", MediaFileType::Fanart),
    ("clearlogo", MediaFileType::Clearlogo),
    ("thumb", MediaFileType::Thumb),
    ("keyart", MediaFileType::Keyart),
    ("banner", MediaFileType::Banner),
    ("clearart", MediaFileType::Clearart),
    ("disc", MediaFileType::Disc),
];

/// The artwork slots of a season.
const SEASON_SLOTS: [(&str, MediaFileType); 4] = [
    ("poster", MediaFileType::SeasonPoster),
    ("fanart", MediaFileType::SeasonFanart),
    ("banner", MediaFileType::SeasonBanner),
    ("thumb", MediaFileType::SeasonThumb),
];

/// A season or episode number in one ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TvEpisodeNumberDto {
    /// `aired`, `dvd`, `absolute`, `alternate` or `display`.
    pub ordering: String,
    /// The provider's name of an alternate ordering, empty for the standard ones.
    pub name: String,
    pub season: u32,
    pub episode: u32,
}

/// One season of a show.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct TvSeasonDto {
    /// The season's item id, `None` when the season only exists through its episodes.
    pub id: Option<String>,
    /// The season number in the show's ordering; `0` is the specials.
    pub number: u32,
    /// The provider's name for the season, empty when it has none.
    pub name: String,
    pub overview: String,
    pub note: String,
    pub poster: Option<PosterDto>,
    pub fanart: Option<PosterDto>,
    /// The poster and the fanart are the show's, because the season has none of its own and the
    /// fallback setting is on.
    pub poster_from_show: bool,
    /// `poster`, `fanart`, `banner` and `thumb`.
    pub artwork: Vec<DetailArtworkDto>,
    /// `YYYY-MM-DD`, the earliest air date of the season's episodes.
    pub first_aired: Option<String>,
    /// Episodes in the library.
    pub episodes: u32,
    pub watched: u32,
    pub new_episodes: u32,
    /// Episodes the provider lists and the library lacks, unaired ones left out.
    pub missing: u32,
    /// Episodes the provider lists that have not aired yet.
    pub unaired: u32,
}

/// An episode of the library, as a row of a season.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct TvEpisodeRowDto {
    pub id: String,
    pub season: u32,
    pub number: u32,
    pub title: String,
    pub plot: String,
    /// `YYYY-MM-DD`.
    pub first_aired: Option<String>,
    pub runtime_minutes: u32,
    pub watched: bool,
    pub is_new: bool,
    pub thumb: Option<PosterDto>,
    /// The main video file's name, empty without one.
    pub file_name: String,
    pub size_bytes: f64,
    /// `1080p`, `2160p`, ... empty when media info has not been read.
    pub resolution: String,
    /// One file holds several episodes.
    pub multi_episode: bool,
    /// Another episode has the same number: two files for one episode.
    pub duplicate: bool,
}

/// An episode the provider lists and the library lacks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TvMissingEpisodeDto {
    pub season: u32,
    pub number: u32,
    pub title: String,
    pub plot: String,
    /// `YYYY-MM-DD`.
    pub first_aired: Option<String>,
    /// The episode airs after today.
    pub unaired: bool,
}

/// An episode without valid numbering, waiting for a manual assignment.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct TvUncategorisedDto {
    pub id: String,
    pub title: String,
    pub file_name: String,
    pub folder: String,
    pub size_bytes: f64,
}

/// A guest star of the show and the number of episodes they appear in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TvGuestDto {
    pub name: String,
    /// The character, the first one found.
    pub role: String,
    pub episodes: u32,
}

/// Everything the show page and its season pages show.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ShowDetailDto {
    pub id: String,
    pub title: String,
    pub original_title: String,
    pub tagline: String,
    /// `0` when unknown.
    pub year: u32,
    /// The year the show ended, `0` while it runs or when unknown.
    pub year_end: u32,
    pub certification: String,
    pub runtime_minutes: u32,
    /// `YYYY-MM-DD`.
    pub first_aired: Option<String>,
    /// `continuing`, `ended`, `canceled` or `unknown`.
    pub status: String,
    /// The date of the latest episode that has aired (`YYYY-MM-DD`).
    pub last_aired: Option<String>,
    /// The date of the next episode that has not aired yet.
    pub next_aired: Option<String>,
    /// The date of the last episode of an ended or cancelled show; the NFO's `<enddate>`.
    pub end_date: Option<String>,
    pub genres: Vec<String>,
    pub studios: Vec<String>,
    pub countries: Vec<String>,
    pub plot: String,
    pub ids: Vec<DetailIdDto>,
    pub ratings: Vec<DetailRatingDto>,
    /// The ratings strip: every source with its value or the reason it has none.
    pub rating_chips: Vec<RatingChipDto>,
    pub user_rating: Option<f64>,
    pub top250: u32,
    /// Every episode in the library is watched.
    pub watched: bool,
    pub locked: bool,
    pub needs_review: bool,
    /// `online`, `offline`, `missing` or `invalid`: whether the show's folder is where the
    /// library expects it.
    pub availability: String,
    /// The stage, flags and explanation lines.
    pub lifecycle: Option<LifecycleDto>,
    pub is_new: bool,
    pub date_added_ms: f64,
    pub folder: String,
    pub data_source: String,
    /// The label of the library folder the title lives on ("NAS 4K"); empty when it was removed.
    pub source_label: String,
    pub note: String,
    pub tags: Vec<String>,
    /// The ordering the numbers follow: `aired`, `dvd`, `absolute` or the alternate's name.
    pub ordering: String,
    pub backdrop: Option<PosterDto>,
    pub logo: Option<PosterDto>,
    pub poster: Option<PosterDto>,
    pub tech: Vec<TechBadgeDto>,
    /// The show's original language (ISO 639-1), empty when unknown.
    pub original_language: String,
    pub credits: Vec<DetailCreditDto>,
    pub guests: Vec<TvGuestDto>,
    pub seasons: Vec<TvSeasonDto>,
    pub episodes: Vec<TvEpisodeRowDto>,
    pub missing: Vec<TvMissingEpisodeDto>,
    /// The provider's episode list has been fetched, so `missing` can be trusted: an empty one
    /// means the show is complete. Before that there is nothing to compare with.
    pub episode_list: bool,
    pub uncategorised: Vec<TvUncategorisedDto>,
    /// Files of the show itself: NFO, artwork, theme music.
    pub files: Vec<DetailFileDto>,
    pub artwork: Vec<DetailArtworkDto>,
    pub trailers: Vec<crate::detail_ipc::DetailTrailerDto>,
    /// Episodes of the library flagged as new.
    pub new_episodes: u32,
    /// How many episode numbers more than one file claims.
    pub duplicate_episodes: u32,
    pub watched_episodes: u32,
}

/// The neighbour of an episode in the show's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TvEpisodeLinkDto {
    pub id: String,
    pub season: u32,
    pub number: u32,
    pub title: String,
}

/// Everything the episode page shows.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct EpisodeDetailDto {
    pub id: String,
    pub show_id: String,
    pub show_title: String,
    /// The season's item id, when the season has one.
    pub season_id: Option<String>,
    pub season: u32,
    pub number: u32,
    pub title: String,
    pub original_title: String,
    pub plot: String,
    /// `YYYY-MM-DD`.
    pub first_aired: Option<String>,
    pub runtime_minutes: u32,
    pub ids: Vec<DetailIdDto>,
    pub ratings: Vec<DetailRatingDto>,
    pub user_rating: Option<f64>,
    pub watched: bool,
    pub play_count: u32,
    pub locked: bool,
    /// `online`, `offline`, `missing` or `invalid`: whether this episode's files are where the
    /// library expects them.
    pub availability: String,
    /// The lifecycle of the show the episode belongs to.
    pub lifecycle: Option<LifecycleDto>,
    pub is_new: bool,
    pub multi_episode: bool,
    pub duplicate: bool,
    /// The episode has no valid numbering.
    pub uncategorised: bool,
    pub date_added_ms: f64,
    pub folder: String,
    pub data_source: String,
    /// The label of the library folder the title lives on ("NAS 4K"); empty when it was removed.
    pub source_label: String,
    pub note: String,
    pub tags: Vec<String>,
    /// Every number of the episode, one per ordering.
    pub numbers: Vec<TvEpisodeNumberDto>,
    pub directors: Vec<String>,
    pub writers: Vec<String>,
    pub studios: Vec<String>,
    pub guests: Vec<DetailCreditDto>,
    pub credits: Vec<DetailCreditDto>,
    pub thumb: Option<PosterDto>,
    pub show_backdrop: Option<PosterDto>,
    pub tech: Vec<TechBadgeDto>,
    /// The show's original language (ISO 639-1), empty when unknown.
    pub original_language: String,
    /// The audio track the page and the Inspector lead with.
    pub hero_audio: Option<HeroAudioDto>,
    pub videos: Vec<DetailVideoDto>,
    pub files: Vec<DetailFileDto>,
    pub artwork: Vec<DetailArtworkDto>,
    pub subtitles: Vec<DetailSubtitleDto>,
    pub previous: Option<TvEpisodeLinkDto>,
    pub next: Option<TvEpisodeLinkDto>,
}

fn not_found(what: &str) -> LibraryCommandError {
    LibraryCommandError {
        code: "not_found".to_owned(),
        message: format!("That {what} is not in the library any more."),
    }
}

fn date_text(date: Option<CalendarDate>) -> Option<String> {
    date.map(|d| d.to_string())
}

/// Today's date in UTC.
pub(crate) fn today_utc() -> CalendarDate {
    date_of_millis(now_millis())
}

/// The UTC date of an instant in epoch milliseconds. Days since 1970-01-01 become a civil date
/// (Hinnant's algorithm).
pub(crate) fn date_of_millis(millis: i64) -> CalendarDate {
    let days = millis.div_euclid(86_400_000) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    CalendarDate::new(
        i32::try_from(year).unwrap_or(1970),
        u8::try_from(month).unwrap_or(1),
        u8::try_from(day).unwrap_or(1),
    )
    .unwrap_or_else(|| CalendarDate::new(1970, 1, 1).expect("a valid date"))
}

fn ordering_label(group: &EpisodeGroup) -> String {
    match group.ordering {
        EpisodeOrdering::Alternate | EpisodeOrdering::Display if !group.name.is_empty() => {
            group.name.clone()
        }
        other => enum_name(&other),
    }
}

/// Whether the episode has a season and episode number in the show's ordering or the aired one.
fn categorised(episode: &EpisodeFields, active: &EpisodeGroup) -> bool {
    episode.numbers.get(&EpisodeGroup::aired()).is_some()
        || episode.numbers.get(active).is_some()
        || episode
            .numbers
            .iter()
            .any(|n| n.group.ordering == active.ordering)
}

/// How many episode numbers among `rows` have a row that `keep` takes.
fn distinct_numbers(rows: &[&TvEpisodeRowDto], keep: impl Fn(&TvEpisodeRowDto) -> bool) -> usize {
    rows.iter()
        .filter(|r| keep(r))
        .map(|r| (r.season, r.number))
        .collect::<HashSet<_>>()
        .len()
}

fn episode_fields(item: &Item) -> Option<&EpisodeFields> {
    match &item.detail {
        ItemDetail::Episode(fields) => Some(fields),
        _ => None,
    }
}

fn item_id(item: &Item) -> ItemId {
    item.id.unwrap_or(ItemId(0))
}

/// The distinct values of a technical fact over the episodes, the most common first.
fn top_values(counts: BTreeMap<String, u32>) -> Vec<String> {
    let mut values: Vec<(String, u32)> = counts.into_iter().collect();
    values.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    values
        .into_iter()
        .take(BADGES_PER_FACET)
        .map(|(value, _)| value)
        .collect()
}

fn badge(label: String, facet: &str, value: &str, notable: bool) -> TechBadgeDto {
    TechBadgeDto {
        label,
        facet: facet.to_owned(),
        values: vec![value.to_owned()],
        number: None,
        notable,
    }
}

/// The technical badges of a show: the resolutions, HDR formats, video codecs and audio codecs
/// its episodes have, most common first. Every badge names the library filter that matches a
/// show when any of its episodes does.
pub(crate) fn tech_badges_of<'a>(
    episodes: impl IntoIterator<Item = &'a Item>,
    original_language: &str,
) -> Vec<TechBadgeDto> {
    let mut resolutions: BTreeMap<String, u32> = BTreeMap::new();
    let mut hdr: BTreeMap<String, u32> = BTreeMap::new();
    let mut codecs: BTreeMap<String, u32> = BTreeMap::new();
    let mut audio: BTreeMap<String, u32> = BTreeMap::new();
    for episode in episodes {
        let Some(video) = main_video(episode) else {
            continue;
        };
        if let Some(label) = resolution_label(video.width, video.height) {
            *resolutions.entry(label.to_owned()).or_default() += 1;
        }
        for format in &video.hdr_formats {
            *hdr.entry(format.clone()).or_default() += 1;
        }
        if !video.video_codec.is_empty() {
            *codecs.entry(video.video_codec.clone()).or_default() += 1;
        }
        let stream = hero_audio(&video.audio_streams, original_language);
        if let Some(stream) = stream.filter(|a| !a.codec.is_empty()) {
            let layout = audio_channels_dot(stream.channels);
            let label = if layout.is_empty() {
                stream.codec.clone()
            } else {
                format!("{} {layout}", stream.codec)
            };
            // The filter matches the codec; the channel layout is only part of the label.
            *audio
                .entry(format!("{}\u{1f}{label}", stream.codec))
                .or_default() += 1;
        }
    }
    let mut badges = Vec::new();
    for value in top_values(resolutions) {
        badges.push(badge(value.clone(), "video_format", &value, false));
    }
    for value in top_values(hdr) {
        let notable = value.eq_ignore_ascii_case("Dolby Vision");
        badges.push(badge(value.clone(), "hdr_format", &value, notable));
    }
    for value in top_values(codecs) {
        badges.push(badge(value.clone(), "video_codec", &value, false));
    }
    for packed in top_values(audio) {
        let (codec, label) = packed.split_once('\u{1f}').unwrap_or((&packed, &packed));
        let lower = codec.to_lowercase();
        let notable = lower.contains("atmos") || lower.contains("dts:x") || lower.contains("dts-x");
        badges.push(badge(label.to_owned(), "audio_codec", codec, notable));
    }
    badges
}

/// What one read of the database gives the show page.
struct ShowTreeData {
    show: Item,
    seasons: Vec<Item>,
    episodes: Vec<(Item, bool)>,
    /// The episodes whose files are not online. They are listed, but they do not make a duplicate.
    unavailable: HashSet<ItemId>,
    needs_review: bool,
    is_new: bool,
    availability: Availability,
}

/// Reads the show, season and episode pages.
pub struct TvReader {
    db: Arc<Database>,
    images: DetailReader,
    /// A season without artwork of its own shows the show's (`tv.season-art-fallback`).
    season_art_fallback: bool,
}

impl TvReader {
    pub fn new(
        db: Arc<Database>,
        sources: Arc<LibrarySources>,
        cache: Option<Arc<ThumbCache>>,
    ) -> Self {
        Self {
            images: DetailReader::new(Arc::clone(&db), sources, cache),
            db,
            season_art_fallback: true,
        }
    }

    /// Also puts the stage, flags and explanation of the title on its page.
    #[must_use]
    pub fn with_lifecycle(mut self, lifecycle: Option<Arc<LifecycleService>>) -> Self {
        self.images = self.images.with_lifecycle(lifecycle);
        self
    }

    /// Also puts the ratings strip (a value or the reason there is none) on the show page.
    #[must_use]
    pub fn with_rating_status(mut self, status: Option<Arc<RatingStatusService>>) -> Self {
        self.images = self.images.with_rating_status(status);
        self
    }

    /// Whether a season without artwork of its own shows the show's. On by default.
    #[must_use]
    pub fn with_season_art_fallback(mut self, on: bool) -> Self {
        self.season_art_fallback = on;
        self
    }

    fn load_show(&self, id: ItemId) -> Result<ShowTreeData, LibraryCommandError> {
        let loaded = self.db.read(|conn| {
            let Some(show) = get_item(conn, id)? else {
                return Ok(None);
            };
            if !matches!(show.detail, ItemDetail::Show(_)) {
                return Ok(None);
            }
            let flags = get_flags(conn, id)?;
            let mut seasons = Vec::new();
            let mut episodes = Vec::new();
            let mut unavailable = HashSet::new();
            for child in children_of(conn, id)? {
                let Some(item) = get_item(conn, child)? else {
                    continue;
                };
                match item.detail {
                    ItemDetail::Season(_) => seasons.push(item),
                    ItemDetail::Episode(_) => {
                        let is_new = get_flags(conn, child)?.is_new;
                        if get_availability(conn, child)?.0 != Availability::Online {
                            unavailable.insert(child);
                        }
                        episodes.push((item, is_new));
                    }
                    _ => {}
                }
            }
            let availability =
                live_availability(get_availability(conn, id)?.0, &show.core.data_source);
            if availability == Availability::Offline {
                // The library folder cannot be reached: no episode can be played either.
                unavailable.extend(episodes.iter().filter_map(|(episode, _)| episode.id));
            }
            Ok(Some(ShowTreeData {
                show,
                seasons,
                episodes,
                unavailable,
                needs_review: flags.review_state == cv_domain::ReviewState::NeedsReview,
                is_new: flags.is_new,
                availability,
            }))
        })?;
        loaded.ok_or_else(|| not_found("show"))
    }

    fn file_rows(item: &Item) -> Vec<DetailFileDto> {
        item.core
            .media_files
            .iter()
            .filter(|f| f.kind != MediaFileType::Video)
            .take(MAX_FILES)
            .map(|f| DetailFileDto {
                kind: enum_name(&f.kind),
                name: f.filename.clone(),
                size_bytes: exact(f.size_bytes),
            })
            .collect()
    }

    fn image_of_kind(&self, item: &Item, kind: MediaFileType) -> Option<PosterDto> {
        item.core
            .media_files
            .iter()
            .find(|f| f.kind == kind)
            .and_then(|f| self.images.image_of(f))
    }

    fn slots(&self, item: &Item, slots: &[(&str, MediaFileType)]) -> Vec<DetailArtworkDto> {
        slots
            .iter()
            .map(|(slot, kind)| {
                let file = item.core.media_files.iter().find(|f| f.kind == *kind);
                DetailArtworkDto {
                    slot: (*slot).to_owned(),
                    file_name: file.map(|f| f.filename.clone()),
                    image: file.and_then(|f| self.images.image_of(f)),
                    width: file.map_or(0, |f| f.width),
                    height: file.map_or(0, |f| f.height),
                    size_bytes: file.map_or(0.0, |f| exact(f.size_bytes)),
                    online: item
                        .core
                        .artwork_urls
                        .get(kind)
                        .is_some_and(|url| !url.trim().is_empty()),
                }
            })
            .collect()
    }

    /// Everything the show page and its season pages show. Blocking.
    pub fn show(&self, id: ItemId) -> Result<ShowDetailDto, LibraryCommandError> {
        let ShowTreeData {
            show,
            seasons: season_items,
            episodes: episode_items,
            unavailable,
            needs_review,
            is_new,
            availability,
        } = self.load_show(id)?;
        let ItemDetail::Show(fields) = &show.detail else {
            return Err(not_found("show"));
        };
        let active = &fields.episode_group;
        let today = today_utc();
        let core = &show.core;

        // Episodes by season, with the numbering problems found on the way.
        let mut rows: Vec<TvEpisodeRowDto> = Vec::new();
        let mut uncategorised: Vec<TvUncategorisedDto> = Vec::new();
        let mut air: Vec<EpisodeAir> = Vec::new();
        // A file that is gone does not make a duplicate, so only online episodes are counted.
        let counts = duplicate_numbers(episode_items.iter().filter_map(|(item, _)| {
            let episode = episode_fields(item)?;
            (categorised(episode, active) && !unavailable.contains(&item_id(item)))
                .then(|| shown_number(item, active))
        }));
        for (item, is_new) in &episode_items {
            let Some(episode) = episode_fields(item) else {
                continue;
            };
            let video = main_video(item);
            if !categorised(episode, active) {
                uncategorised.push(TvUncategorisedDto {
                    id: item_id(item).0.to_string(),
                    title: item.core.title.clone(),
                    file_name: video.map(|v| v.filename.clone()).unwrap_or_default(),
                    folder: video.map(|v| v.directory.clone()).unwrap_or_default(),
                    size_bytes: video.map_or(0.0, |v| exact(v.size_bytes)),
                });
                continue;
            }
            let (season, number) = shown_number(item, active);
            if let Some(date) = episode.first_aired {
                air.push(EpisodeAir { season, date });
            }
            rows.push(TvEpisodeRowDto {
                id: item_id(item).0.to_string(),
                season,
                number,
                title: item.core.title.clone(),
                plot: item.core.plot.clone(),
                first_aired: date_text(episode.first_aired),
                runtime_minutes: episode.runtime_minutes,
                watched: episode.watched,
                is_new: *is_new,
                thumb: self.image_of_kind(item, MediaFileType::Thumb),
                file_name: video.map(|v| v.filename.clone()).unwrap_or_default(),
                size_bytes: video.map_or(0.0, |v| exact(v.size_bytes)),
                resolution: video
                    .and_then(|v| resolution_label(v.width, v.height))
                    .unwrap_or_default()
                    .to_owned(),
                multi_episode: episode.multi_episode,
                duplicate: !unavailable.contains(&item_id(item))
                    && counts.contains_key(&(season, number)),
            });
        }
        rows.sort_by(|a, b| {
            (a.season, a.number)
                .cmp(&(b.season, b.number))
                .then_with(|| a.title.cmp(&b.title))
        });
        uncategorised.sort_by(|a, b| a.file_name.cmp(&b.file_name));

        let mut missing: Vec<TvMissingEpisodeDto> = fields
            .missing_episodes
            .iter()
            .map(|m| {
                let (season, number) = m
                    .numbers
                    .get(active)
                    .or_else(|| m.numbers.get(&EpisodeGroup::aired()))
                    .or_else(|| m.numbers.iter().next())
                    .map_or((0, 0), |n| (n.season, n.episode));
                // The provider knows these episodes, so their dates count for the run too.
                if let Some(date) = m.first_aired {
                    air.push(EpisodeAir { season, date });
                }
                TvMissingEpisodeDto {
                    season,
                    number,
                    title: m.title.clone(),
                    plot: m.plot.clone(),
                    first_aired: date_text(m.first_aired),
                    unaired: m.first_aired.is_some_and(|d| d > today),
                }
            })
            .collect();
        missing.sort_by_key(|a| (a.season, a.number));

        let show_poster = self.image_of_kind(&show, MediaFileType::Poster);
        let show_backdrop = self.image_of_kind(&show, MediaFileType::Fanart);
        let seasons = self.seasons(
            fields,
            &season_items,
            &rows,
            &missing,
            (&show_poster, &show_backdrop),
        );
        let run = air_info(fields.status, fields.first_aired, &air, today);

        // Guests of all episodes, once each.
        let mut guests: Vec<TvGuestDto> = Vec::new();
        for (item, _) in &episode_items {
            for credit in item
                .core
                .credits
                .iter()
                .filter(|c| c.kind == CreditKind::Guest)
            {
                let name = credit.person.name.trim();
                if name.is_empty() {
                    continue;
                }
                match guests.iter_mut().find(|g| g.name == name) {
                    Some(guest) => {
                        guest.episodes += 1;
                        if guest.role.is_empty() {
                            guest.role.clone_from(&credit.role);
                        }
                    }
                    None => guests.push(TvGuestDto {
                        name: name.to_owned(),
                        role: credit.role.clone(),
                        episodes: 1,
                    }),
                }
            }
        }
        guests.sort_by(|a, b| {
            b.episodes
                .cmp(&a.episodes)
                .then_with(|| a.name.cmp(&b.name))
        });

        let (ratings, user_rating) = detail_ratings(core);
        // One per number, as in the season counts: a second file of an episode is a duplicate,
        // and an uncategorised file is no numbered episode at all.
        let all_rows: Vec<&TvEpisodeRowDto> = rows.iter().collect();
        let watched_episodes =
            u32::try_from(distinct_numbers(&all_rows, |r| r.watched)).unwrap_or(u32::MAX);
        let new_episodes =
            u32::try_from(distinct_numbers(&all_rows, |r| r.is_new)).unwrap_or(u32::MAX);
        let all_watched = !episode_items.is_empty()
            && episode_items
                .iter()
                .all(|(item, _)| episode_fields(item).is_some_and(|e| e.watched));

        Ok(ShowDetailDto {
            id: id.0.to_string(),
            title: core.title.clone(),
            original_title: core.original_title.clone(),
            tagline: fields.tagline.clone(),
            year: u32::from(core.year),
            year_end: match u32::from(fields.year_end) {
                0 => u32::from(run.end_year),
                stored => stored,
            },
            certification: certification_label(&fields.certification.0),
            runtime_minutes: fields.runtime_minutes,
            first_aired: date_text(run.first_aired),
            status: status_name(fields),
            last_aired: date_text(run.last_aired),
            next_aired: date_text(run.next_aired),
            end_date: date_text(run.end_date),
            genres: fields.genres.iter().map(genre_label).collect(),
            studios: core.production_companies.clone(),
            countries: fields
                .country
                .split([',', '/'])
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_owned)
                .collect(),
            plot: core.plot.clone(),
            ids: core
                .ids
                .iter()
                .map(|(provider, value)| DetailIdDto {
                    provider: provider.to_owned(),
                    value: value.to_owned(),
                })
                .collect(),
            ratings,
            rating_chips: self.images.rating_chips_of(id, &show),
            user_rating,
            top250: fields.top250,
            watched: all_watched,
            locked: core.locked,
            needs_review,
            availability: availability.as_str().to_owned(),
            lifecycle: self.images.lifecycle_of(id),
            is_new,
            date_added_ms: millis(core.date_added_ms),
            folder: core.path.clone(),
            data_source: core.data_source.clone(),
            source_label: crate::sources_ipc::source_label_of(
                &self.db,
                cv_domain::SourceKind::Tv,
                &core.data_source,
            ),
            note: core.note.clone(),
            tags: core.tags.iter().map(str::to_owned).collect(),
            ordering: ordering_label(active),
            backdrop: show_backdrop,
            logo: self.image_of_kind(&show, MediaFileType::Clearlogo),
            poster: show_poster,
            tech: tech_badges_of(
                episode_items.iter().map(|(item, _)| item),
                &core.original_language,
            ),
            original_language: core.original_language.clone(),
            credits: cv_domain::merge_crew(&core.credits)
                .iter()
                .filter(|c| c.kind != CreditKind::Guest && !c.person.name.trim().is_empty())
                .map(|c| self.images.credit(&core.path, c))
                .collect(),
            guests,
            seasons,
            episodes: rows,
            missing,
            episode_list: fields.episode_list_fetched || !fields.missing_episodes.is_empty(),
            uncategorised,
            files: Self::file_rows(&show),
            artwork: self.slots(&show, &SHOW_SLOTS),
            trailers: fields
                .trailers
                .iter()
                .map(|t| crate::detail_ipc::DetailTrailerDto {
                    name: t.name.clone(),
                    provider: t.provider.clone(),
                    quality: t.quality.clone(),
                    language: t.language.clone(),
                    url: t.url.clone(),
                    local_file: None,
                })
                .chain(
                    show.core
                        .media_files
                        .iter()
                        .filter(|f| f.kind == MediaFileType::Trailer)
                        .map(|f| crate::detail_ipc::DetailTrailerDto {
                            name: crate::detail_ipc::file_stem(&f.filename),
                            provider: String::new(),
                            quality: String::new(),
                            language: String::new(),
                            url: String::new(),
                            local_file: Some(f.filename.clone()),
                        }),
                )
                .collect(),
            new_episodes,
            duplicate_episodes: u32::try_from(counts.len()).unwrap_or(u32::MAX),
            watched_episodes,
        })
    }

    /// The seasons: every season number that has episodes, a season record or a missing episode.
    fn seasons(
        &self,
        fields: &ShowFields,
        items: &[Item],
        rows: &[TvEpisodeRowDto],
        missing: &[TvMissingEpisodeDto],
        (show_poster, show_fanart): (&Option<PosterDto>, &Option<PosterDto>),
    ) -> Vec<TvSeasonDto> {
        let active = &fields.episode_group;
        let mut numbers: Vec<u32> = rows
            .iter()
            .map(|r| r.season)
            .chain(missing.iter().map(|m| m.season))
            .collect();
        let record_of = |number: u32| {
            items
                .iter()
                .find(|s| matches!(&s.detail, ItemDetail::Season(f) if f.season == number))
        };
        for item in items {
            if let ItemDetail::Season(f) = &item.detail {
                numbers.push(f.season);
            }
        }
        numbers.sort_unstable();
        numbers.dedup();
        numbers
            .into_iter()
            .map(|number| {
                let record = record_of(number);
                let text = fields
                    .season_texts
                    .iter()
                    .find(|t| t.season == number && &t.group == active)
                    .or_else(|| {
                        fields
                            .season_texts
                            .iter()
                            .find(|t| t.season == number && t.group == EpisodeGroup::aired())
                    });
                let owned: Vec<&TvEpisodeRowDto> =
                    rows.iter().filter(|r| r.season == number).collect();
                let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
                let artwork = record.map_or_else(Vec::new, |s| self.slots(s, &SEASON_SLOTS));
                let image = |slot: &str| {
                    artwork
                        .iter()
                        .find(|a| a.slot == slot)
                        .and_then(|a| a.image.clone())
                };
                let fallback = self.season_art_fallback;
                let poster = with_show_fallback(image("poster"), show_poster.clone(), fallback);
                let fanart = with_show_fallback(image("fanart"), show_fanart.clone(), fallback);
                TvSeasonDto {
                    id: record.map(|s| item_id(s).0.to_string()),
                    number,
                    name: text.map(|t| t.name.clone()).unwrap_or_default(),
                    overview: text.map(|t| t.overview.clone()).unwrap_or_default(),
                    note: record.map(|s| s.core.note.clone()).unwrap_or_default(),
                    poster: poster.as_ref().map(|p| p.image.clone()),
                    fanart: fanart.as_ref().map(|p| p.image.clone()),
                    poster_from_show: poster.as_ref().is_some_and(|p| p.from_show),
                    artwork,
                    first_aired: owned
                        .iter()
                        .filter_map(|r| r.first_aired.clone())
                        .chain(
                            missing
                                .iter()
                                .filter(|m| m.season == number)
                                .filter_map(|m| m.first_aired.clone()),
                        )
                        .min(),
                    // One per number: a second file of an episode is a duplicate, not another
                    // episode, so it adds nothing to the season's counts.
                    episodes: count(distinct_numbers(&owned, |_| true)),
                    watched: count(distinct_numbers(&owned, |r| r.watched)),
                    new_episodes: count(distinct_numbers(&owned, |r| r.is_new)),
                    missing: count(
                        missing
                            .iter()
                            .filter(|m| m.season == number && !m.unaired)
                            .count(),
                    ),
                    unaired: count(
                        missing
                            .iter()
                            .filter(|m| m.season == number && m.unaired)
                            .count(),
                    ),
                }
            })
            .collect()
    }

    /// Everything the episode page shows. Blocking.
    pub fn episode(&self, id: ItemId) -> Result<EpisodeDetailDto, LibraryCommandError> {
        struct Loaded {
            item: Item,
            show: Item,
            siblings: Vec<Item>,
            seasons: Vec<Item>,
            unavailable: HashSet<ItemId>,
            is_new: bool,
            availability: Availability,
        }
        let loaded = self.db.read(|conn| {
            let Some(item) = get_item(conn, id)? else {
                return Ok(None);
            };
            if !matches!(item.detail, ItemDetail::Episode(_)) {
                return Ok(None);
            }
            let Some(show_id) = item.parent_id else {
                return Ok(None);
            };
            let Some(show) = get_item(conn, show_id)? else {
                return Ok(None);
            };
            if !matches!(show.detail, ItemDetail::Show(_)) {
                return Ok(None);
            }
            let is_new = get_flags(conn, id)?.is_new;
            let mut siblings = Vec::new();
            let mut seasons = Vec::new();
            let mut unavailable = HashSet::new();
            if get_availability(conn, id)?.0 != Availability::Online {
                unavailable.insert(id);
            }
            for child in children_of(conn, show_id)? {
                if child == id {
                    continue;
                }
                match get_item(conn, child)? {
                    Some(sibling) if matches!(sibling.detail, ItemDetail::Episode(_)) => {
                        if get_availability(conn, child)?.0 != Availability::Online {
                            unavailable.insert(child);
                        }
                        siblings.push(sibling);
                    }
                    Some(season) if matches!(season.detail, ItemDetail::Season(_)) => {
                        seasons.push(season);
                    }
                    _ => {}
                }
            }
            let availability =
                live_availability(get_availability(conn, id)?.0, &item.core.data_source);
            if availability == Availability::Offline {
                unavailable.insert(id);
                unavailable.extend(siblings.iter().filter_map(|sibling| sibling.id));
            }
            Ok(Some(Loaded {
                item,
                show,
                siblings,
                seasons,
                unavailable,
                is_new,
                availability,
            }))
        })?;
        let Loaded {
            item,
            show,
            siblings,
            seasons,
            unavailable,
            is_new,
            availability,
        } = loaded.ok_or_else(|| not_found("episode"))?;
        let (ItemDetail::Episode(episode), ItemDetail::Show(show_fields)) =
            (&item.detail, &show.detail)
        else {
            return Err(not_found("episode"));
        };
        let active = &show_fields.episode_group;
        let core = &item.core;
        let ok = categorised(episode, active);
        let (season, number) = shown_number(&item, active);

        // The neighbours in the show's order, among the episodes that have a number.
        let mut ordered: Vec<(&Item, (u32, u32))> = siblings
            .iter()
            .filter(|s| episode_fields(s).is_some_and(|e| categorised(e, active)))
            .map(|s| (s, shown_number(s, active)))
            .collect();
        let duplicate = ok
            && !unavailable.contains(&id)
            && ordered
                .iter()
                .any(|(other, n)| *n == (season, number) && !unavailable.contains(&item_id(other)));
        if ok {
            ordered.push((&item, (season, number)));
        }
        ordered.sort_by_key(|(_, n)| *n);
        let position = ordered.iter().position(|(i, _)| item_id(i) == id);
        let link = |index: Option<usize>| {
            index
                .and_then(|i| ordered.get(i))
                .map(|(other, (s, n))| TvEpisodeLinkDto {
                    id: item_id(other).0.to_string(),
                    season: *s,
                    number: *n,
                    title: other.core.title.clone(),
                })
        };
        let previous = link(position.and_then(|p| p.checked_sub(1)));
        let next = link(position.map(|p| p + 1));

        let season_id = seasons
            .iter()
            .find(|s| matches!(&s.detail, ItemDetail::Season(f) if f.season == season))
            .map(|s| item_id(s).0.to_string());
        let (ratings, user_rating) = detail_ratings(core);
        let credits_of = |kind: Option<CreditKind>| -> Vec<DetailCreditDto> {
            cv_domain::merge_crew(&core.credits)
                .iter()
                .filter(|c| !c.person.name.trim().is_empty())
                .filter(|c| match kind {
                    Some(k) => c.kind == k,
                    None => c.kind != CreditKind::Guest,
                })
                .map(|c| self.images.credit(&show.core.path, c))
                .collect()
        };

        Ok(EpisodeDetailDto {
            id: id.0.to_string(),
            show_id: item_id(&show).0.to_string(),
            show_title: show.core.title.clone(),
            season_id,
            season,
            number,
            title: core.title.clone(),
            original_title: core.original_title.clone(),
            plot: core.plot.clone(),
            first_aired: date_text(episode.first_aired),
            runtime_minutes: episode.runtime_minutes,
            ids: core
                .ids
                .iter()
                .map(|(provider, value)| DetailIdDto {
                    provider: provider.to_owned(),
                    value: value.to_owned(),
                })
                .collect(),
            ratings,
            user_rating,
            watched: episode.watched,
            play_count: episode.play_count,
            locked: core.locked,
            availability: availability.as_str().to_owned(),
            lifecycle: self.images.lifecycle_of(id),
            is_new,
            multi_episode: episode.multi_episode,
            duplicate,
            uncategorised: !ok,
            date_added_ms: millis(core.date_added_ms),
            folder: main_video(&item)
                .map(|v| v.directory.clone())
                .unwrap_or_else(|| core.path.clone()),
            data_source: core.data_source.clone(),
            source_label: crate::sources_ipc::source_label_of(
                &self.db,
                cv_domain::SourceKind::Tv,
                &core.data_source,
            ),
            note: core.note.clone(),
            tags: core.tags.iter().map(str::to_owned).collect(),
            numbers: episode
                .numbers
                .iter()
                .map(|n| TvEpisodeNumberDto {
                    ordering: enum_name(&n.group.ordering),
                    name: n.group.name.clone(),
                    season: n.season,
                    episode: n.episode,
                })
                .collect(),
            directors: names_of(&item, CreditKind::Director),
            writers: names_of(&item, CreditKind::Writer),
            studios: core.production_companies.clone(),
            guests: credits_of(Some(CreditKind::Guest)),
            credits: credits_of(None),
            thumb: self.image_of_kind(&item, MediaFileType::Thumb),
            show_backdrop: self.image_of_kind(&show, MediaFileType::Fanart),
            tech: tech_badges_of([&item], &core.original_language),
            original_language: core.original_language.clone(),
            hero_audio: main_video(&item)
                .and_then(|video| hero_audio_of(video, &core.original_language)),
            videos: video_files(&item, &crate::detail_ipc::read_reasons(&self.db, &item)),
            files: Self::file_rows(&item),
            artwork: self.slots(&item, &[("thumb", MediaFileType::Thumb)]),
            subtitles: subtitles_of(&item),
            previous,
            next,
        })
    }
}

/// `continuing`, `ended`, `canceled` or `unknown`.
fn status_name(fields: &ShowFields) -> String {
    match fields.status {
        AiredStatus::Unknown => "unknown",
        AiredStatus::Continuing => "continuing",
        AiredStatus::Ended => "ended",
        AiredStatus::Canceled => "canceled",
    }
    .to_owned()
}

fn reader(
    library: &State<'_, LibraryState>,
    images: &State<'_, ImageState>,
) -> Result<TvReader, LibraryCommandError> {
    let db = library.database().map_err(storage)?;
    let cache = images.service().map(|service| service.cache().clone());
    Ok(TvReader::new(db, images.sources().clone(), cache)
        .with_lifecycle(library.lifecycle().cloned())
        .with_rating_status(library.rating_status().cloned()))
}

/// Everything the show page and its season pages show.
#[tauri::command]
#[specta::specta]
pub async fn get_show_detail(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    settings: State<'_, SettingsState>,
    id: String,
) -> Result<ShowDetailDto, LibraryCommandError> {
    let id = parse_id(&id)?;
    let fallback = season_art_fallback(&settings.service().snapshot().values);
    let reader = reader(&library, &images)?.with_season_art_fallback(fallback);
    blocking(move || reader.show(id)).await
}

/// Everything the episode page shows.
#[tauri::command]
#[specta::specta]
pub async fn get_episode_detail(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    id: String,
) -> Result<EpisodeDetailDto, LibraryCommandError> {
    let id = parse_id(&id)?;
    let reader = reader(&library, &images)?;
    blocking(move || reader.episode(id)).await
}

/// Marks episodes watched or not watched; a season or a show is its episodes. The report counts
/// the changes and holds the state before, for `restore_watched`. Goes through the watched
/// service, so the NFO files are rewritten.
#[tauri::command]
#[specta::specta]
pub async fn set_episodes_watched(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    curation: State<'_, CurationState>,
    settings: State<'_, SettingsState>,
    ids: Vec<String>,
    watched: bool,
) -> Result<crate::curation_ipc::WatchReportDto, LibraryCommandError> {
    let ids = ids
        .iter()
        .map(|id| parse_id(id))
        .collect::<Result<Vec<_>, _>>()?;
    crate::curation_ipc::mark_watched(&library, &nfo, &curation, &settings, ids, watched).await
}

#[cfg(test)]
mod tests;

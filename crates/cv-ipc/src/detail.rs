//! Typed reads for the movie detail page.
//!
//! [`get_movie_detail`] answers everything the page shows: the facts and ratings of the hero, the
//! technical badges (each one names the library filter it stands for), the cast, the files with
//! their streams, the artwork slots, the trailers and the subtitles. Images travel as content keys
//! for the `cv://img` protocol, never as bytes. The watched toggle stays with the surface that
//! holds the watched service; the NFO preview has its own command in `nfo_ipc` because it reads
//! the NFO settings.
//!
//! Reads run on a blocking thread. Nothing here deletes anything.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use cv_app::hero_audio::{hero_audio, hero_audio_index};
use cv_app::lifecycle::LifecycleService;
use cv_app::nfo::model::main_video;
use cv_app::rating_status::{RatingChip, RatingStatusService};
use cv_artwork::download::actors::portrait_file;
use cv_artwork::library_source::LibrarySources;
use cv_artwork::thumb::ThumbCache;
use cv_db::Database;
use cv_db::repo::{children_of, get_availability, get_flags, get_item};
use cv_domain::{
    Availability, CreditKind, Item, ItemDetail, ItemId, MediaFile, MediaFileType, MovieFields,
    StreamFlag, id_key, rating_key,
};
use cv_media::resolution::resolution_label;
use cv_template::tokens::{audio_channels_dot, edition_label, genre_label, media_source_label};
use serde::Serialize;
use specta::Type;

use crate::library::{LibraryCommandError, PosterDto, blocking, poster_dto};
use crate::library_facets::certification_label;
use crate::lifecycle::{LifecycleDto, lifecycle_of};
use crate::sources::source_label_of;

/// The most files the Files section lists.
const MAX_FILES: usize = 500;

/// The artwork slots of the page, in the order the page shows them.
const SLOTS: [(&str, MediaFileType); 8] = [
    ("poster", MediaFileType::Poster),
    ("fanart", MediaFileType::Fanart),
    ("clearlogo", MediaFileType::Clearlogo),
    ("thumb", MediaFileType::Thumb),
    ("keyart", MediaFileType::Keyart),
    ("banner", MediaFileType::Banner),
    ("clearart", MediaFileType::Clearart),
    ("disc", MediaFileType::Disc),
];

/// One rating of the movie, on its own scale.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct DetailRatingDto {
    /// The rating source key as stored: `imdb`, `tmdb`, `tomatometerallcritics`, `metacritic`,
    /// `trakt`, ...
    pub source: String,
    pub value: f64,
    /// The top of the scale: 10, 5 or 100.
    pub max: u32,
    pub votes: u32,
    /// When the rating was fetched, in epoch milliseconds; 0 when unknown.
    pub fetched_at_ms: f64,
    /// The title's page on the source's site, when the source named it; absent when not known.
    #[specta(optional)]
    pub url: Option<String>,
}

/// One chip of the ratings strip: a value, or the reason there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RatingChipDto {
    /// The rating key, as in [`DetailRatingDto::source`].
    pub key: String,
    /// `value`, `fetching`, `needs_key`, `connect_service`, `not_found`, `failed`,
    /// `not_fetched` or `not_selected`.
    pub state: String,
    /// One plain sentence saying why there is no value; empty for a value.
    pub reason: String,
    /// The provider to set up (`needs_key`) or connect (`connect_service`), or the rating source
    /// (`imdb-dataset`, `omdb`, `mdblist`, `scraper`) that failed (`failed`) or is not chosen
    /// (`not_selected`).
    pub provider: Option<String>,
}

impl From<RatingChip> for RatingChipDto {
    fn from(chip: RatingChip) -> Self {
        Self {
            key: chip.key,
            state: chip.state.as_str().to_owned(),
            reason: chip.reason,
            provider: chip.provider,
        }
    }
}

/// An external ID of the movie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DetailIdDto {
    pub provider: String,
    pub value: String,
}

/// The collection the movie belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DetailCollectionDto {
    pub id: String,
    pub name: String,
    /// Movies of the collection that are in the library.
    pub owned: u32,
    /// Movies of the collection in all, those not in the library included.
    pub total: u32,
}

/// A technical badge: what it shows and the library filter a click applies.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct TechBadgeDto {
    pub label: String,
    /// The id of the filter in the catalogue (`video_format`, `hdr_format`, ...).
    pub facet: String,
    /// The value to match, for a list filter.
    pub values: Vec<String>,
    /// The number to compare with (`>=`), for a number filter.
    pub number: Option<f64>,
    /// Dolby Vision, Atmos and DTS:X get an accent border.
    pub notable: bool,
}

/// A person on the movie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DetailCreditDto {
    pub name: String,
    /// The character for an actor, the job for crew.
    pub role: String,
    /// `actor`, `director`, `writer`, `producer`, `composer`, `editor`, `camera` or `other`.
    pub kind: String,
    /// The portrait downloaded into the item's `.actors` folder (`cv://img`), when there is one.
    /// Only actors and guests have portraits.
    pub portrait: Option<PosterDto>,
    /// The person's IMDb id (`nm0000093`), when one is stored.
    pub imdb_id: Option<String>,
    /// The person's TMDB id, when one is stored.
    pub tmdb_id: Option<String>,
}

/// The stored provider id of a credited person; an empty value counts as none.
fn person_id(credit: &cv_domain::Credit, key: &str) -> Option<String> {
    credit
        .person
        .ids
        .get(key)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// What a video says about its HDR beyond the format names: see [`cv_domain::HdrDetails`].
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct HdrDetailsDto {
    /// `5`, `7`, `8.1`, `8.2`, `8.4`; empty without a Dolby Vision layer.
    pub dolby_vision_profile: String,
    /// What a player without Dolby Vision plays: `HDR10`, `HLG`, `SDR`; empty when unknown.
    pub dolby_vision_compatibility: String,
    pub mastering_primaries: String,
    /// The mastering display's darkest level in cd/m2.
    pub mastering_min_nits: Option<f64>,
    pub mastering_max_nits: Option<f64>,
    pub max_cll_nits: Option<u32>,
    pub max_fall_nits: Option<u32>,
}

impl HdrDetailsDto {
    fn of(details: &cv_domain::HdrDetails) -> Option<Self> {
        (!details.is_empty()).then(|| Self {
            dolby_vision_profile: details.dolby_vision_profile.clone(),
            dolby_vision_compatibility: details.dolby_vision_compatibility.clone(),
            mastering_primaries: details.mastering_primaries.clone(),
            mastering_min_nits: details.mastering_min_nits,
            mastering_max_nits: details.mastering_max_nits,
            max_cll_nits: details.max_cll_nits,
            max_fall_nits: details.max_fall_nits,
        })
    }
}

/// One stream of a video file.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct DetailStreamDto {
    /// `video`, `audio` or `subtitle`.
    pub kind: String,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub bitrate_kbps: u32,
    pub bit_depth: u32,
    /// Samples per second of an audio stream, `0` when unknown.
    pub sample_rate_hz: u32,
    /// HDR formats of a video stream.
    pub hdr: Vec<String>,
    /// The Dolby Vision profile, mastering display and light levels of a video stream; `None`
    /// when the file says none of them (or media info was read before they were kept).
    pub hdr_details: Option<HdrDetailsDto>,
    pub channels: u32,
    /// ISO 639 code, empty when unknown.
    pub language: String,
    pub title: String,
    /// `default`, `forced`, `hearing_impaired`, `dub`, `original`, `comment`, ...
    pub flags: Vec<String>,
}

/// A video file with its streams.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct DetailVideoDto {
    pub name: String,
    pub folder: String,
    pub size_bytes: f64,
    pub container: String,
    pub duration_secs: u32,
    /// Whether media info has been read; without it the table has nothing to list.
    pub analysed: bool,
    /// Why the details are missing when they are not read: `timeout` (the read ran out of time;
    /// it is tried again), `open` (the file could not be opened), `unreadable` (MediaInfo found
    /// nothing it could read), `disc` (a disc folder, read through its main title); empty while
    /// the read is simply pending, and once it is read.
    pub read_problem: String,
    pub streams: Vec<DetailStreamDto>,
}

/// A file next to the video: NFO, artwork, trailer, subtitle and so on.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct DetailFileDto {
    /// The file type, such as `nfo`, `poster`, `trailer` or `subtitle`.
    pub kind: String,
    pub name: String,
    pub size_bytes: f64,
}

/// One artwork slot.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct DetailArtworkDto {
    /// `poster`, `fanart`, `clearlogo`, `thumb`, `keyart`, `banner`, `clearart` or `disc`.
    pub slot: String,
    /// The file in the slot, `None` when the slot is empty.
    pub file_name: Option<String>,
    pub image: Option<PosterDto>,
    pub width: u32,
    pub height: u32,
    pub size_bytes: f64,
    /// A provider link for the slot is known, so the artwork can be fetched.
    pub online: bool,
}

/// A trailer, remote or a file next to the movie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DetailTrailerDto {
    pub name: String,
    pub provider: String,
    pub quality: String,
    pub language: String,
    /// The remote address, empty for a local file.
    pub url: String,
    /// The file name of a trailer that is on disk.
    pub local_file: Option<String>,
}

/// A subtitle, embedded in the video or a file next to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DetailSubtitleDto {
    /// The file name of an external subtitle, `None` for an embedded one.
    pub file_name: Option<String>,
    pub language: String,
    pub codec: String,
    pub title: String,
    pub flags: Vec<String>,
    /// For an embedded stream: the video file it is in. `None` for a subtitle file.
    pub video: Option<String>,
    /// For an embedded stream: its index among that video's subtitle streams, from 0. This is
    /// what extracting a stream names. `None` for a subtitle file.
    pub position: Option<u32>,
    /// For an embedded stream: the codec id of the container (`S_TEXT/UTF8`), empty while media
    /// info has not read it. Empty for a subtitle file.
    pub codec_id: String,
}

/// Everything the movie detail page shows.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MovieDetailDto {
    pub id: String,
    pub title: String,
    pub original_title: String,
    pub tagline: String,
    /// `0` when unknown.
    pub year: u32,
    pub certification: String,
    pub runtime_minutes: u32,
    /// `YYYY-MM-DD`.
    pub release_date: Option<String>,
    pub genres: Vec<String>,
    pub directors: Vec<String>,
    pub writers: Vec<String>,
    pub studios: Vec<String>,
    pub countries: Vec<String>,
    pub languages: Vec<String>,
    pub edition: String,
    pub media_source: String,
    pub plot: String,
    pub collection: Option<DetailCollectionDto>,
    pub ids: Vec<DetailIdDto>,
    pub ratings: Vec<DetailRatingDto>,
    /// The ratings strip: every source with its value or the reason it has none. The Inspector
    /// shows the same chips.
    pub rating_chips: Vec<RatingChipDto>,
    /// The user's own rating on the 0-10 scale.
    pub user_rating: Option<f64>,
    pub top250: u32,
    pub watched: bool,
    pub play_count: u32,
    /// When the movie was last watched, in epoch milliseconds.
    pub last_watched_ms: Option<f64>,
    /// Whether the files are where the library expects them: `online`, `offline` (the library
    /// folder cannot be reached), `missing` (the folder or video is gone) or `invalid`.
    pub availability: String,
    pub locked: bool,
    pub needs_review: bool,
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
    pub backdrop: Option<PosterDto>,
    pub logo: Option<PosterDto>,
    pub poster: Option<PosterDto>,
    pub tech: Vec<TechBadgeDto>,
    /// The movie's original language (ISO 639-1), empty when unknown.
    pub original_language: String,
    /// The audio track the page and the Inspector lead with.
    pub hero_audio: Option<HeroAudioDto>,
    pub credits: Vec<DetailCreditDto>,
    pub videos: Vec<DetailVideoDto>,
    pub files: Vec<DetailFileDto>,
    pub artwork: Vec<DetailArtworkDto>,
    pub trailers: Vec<DetailTrailerDto>,
    pub subtitles: Vec<DetailSubtitleDto>,
}

#[allow(clippy::cast_precision_loss)]
/// The availability a detail page shows: the stored state, except that a title stored as online
/// whose library folder cannot be reached right now (a drive unplugged, the folder moved away
/// since the last check) is offline. The stored state only changes at the next check of the
/// files; the page must not offer Play meanwhile.
pub fn live_availability(stored: Availability, data_source: &str) -> Availability {
    let root = data_source.trim();
    if stored == Availability::Online && !root.is_empty() && !std::path::Path::new(root).is_dir() {
        Availability::Offline
    } else {
        stored
    }
}

pub fn exact(n: u64) -> f64 {
    n as f64
}

#[allow(clippy::cast_precision_loss)]
pub fn millis(n: i64) -> f64 {
    n as f64
}

fn not_found() -> LibraryCommandError {
    LibraryCommandError {
        code: "not_found".to_owned(),
        message: "That movie is not in the library any more.".to_owned(),
    }
}

fn bad_id(text: &str) -> LibraryCommandError {
    LibraryCommandError {
        code: "bad_id".to_owned(),
        message: format!("{text:?} is not a library item id."),
    }
}

pub fn parse_id(text: &str) -> Result<ItemId, LibraryCommandError> {
    text.trim()
        .parse::<i64>()
        .map(ItemId)
        .map_err(|_| bad_id(text))
}

/// The serde name of an enum value (`Poster` is `poster`).
pub fn enum_name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn flag_names(flags: &BTreeSet<StreamFlag>) -> Vec<String> {
    flags.iter().map(enum_name).collect()
}

pub fn names_of(item: &Item, kind: CreditKind) -> Vec<String> {
    cv_domain::merge_crew(&item.core.credits)
        .iter()
        .filter(|c| c.kind == kind)
        .map(|c| c.person.name.clone())
        .filter(|name| !name.trim().is_empty())
        .collect()
}

/// The ratings of an item on their own scales, best-known sources first, and the user's own
/// rating on the 0-10 scale.
pub fn detail_ratings(core: &cv_domain::ItemCore) -> (Vec<DetailRatingDto>, Option<f64>) {
    let mut ratings: Vec<DetailRatingDto> = core
        .ratings
        .iter()
        .filter(|r| r.id != rating_key::USER && r.id != rating_key::DEFAULT)
        .map(|r| DetailRatingDto {
            source: r.id.clone(),
            value: f64::from(r.value),
            max: r.max_value,
            votes: r.votes,
            fetched_at_ms: millis(r.fetched_at_ms),
            url: (!r.url.is_empty()).then(|| r.url.clone()),
        })
        .collect();
    let rank = |source: &str| match source {
        "imdb" => 0,
        "tomatometerallcritics" => 1,
        "tomatometerallaudience" => 2,
        "metacritic" => 3,
        "letterboxd" => 4,
        "tmdb" => 5,
        "trakt" => 6,
        _ => 7,
    };
    ratings.sort_by(|a, b| {
        rank(&a.source)
            .cmp(&rank(&b.source))
            .then_with(|| a.source.cmp(&b.source))
    });
    let user_rating = core
        .ratings
        .get(rating_key::USER)
        .map(|r| f64::from(r.on_ten_scale()));
    (ratings, user_rating)
}

/// Bitrate for the badge: `46 Mb/s` from 10 Mb/s up, `8.4 Mb/s` below, kb/s under 1 Mb/s.
fn bitrate_label(kbps: u32) -> String {
    if kbps >= 10_000 {
        format!("{} Mb/s", (f64::from(kbps) / 1000.0).round())
    } else if kbps >= 1000 {
        format!("{:.1} Mb/s", f64::from(kbps) / 1000.0)
    } else {
        format!("{kbps} kb/s")
    }
}

fn list_badge(label: String, facet: &str, value: &str, notable: bool) -> TechBadgeDto {
    TechBadgeDto {
        label,
        facet: facet.to_owned(),
        values: vec![value.to_owned()],
        number: None,
        notable,
    }
}

/// The audio track a title leads with: the one [`hero_audio`] picks from the main video's tracks.
/// The detail pages, the Inspector and the library table's Audio column all read this one choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct HeroAudioDto {
    /// The position among the file's audio tracks, from 0.
    pub index: u32,
    /// The codec with its channel layout, such as `TrueHD Atmos 7.1`.
    pub label: String,
    pub codec: String,
    pub channels: u32,
    /// ISO 639 code, empty when unknown.
    pub language: String,
    pub title: String,
    /// `default`, `original`, `dub`, `comment`, ...
    pub flags: Vec<String>,
}

/// The hero audio track of `file` for a title in `original_language`.
pub fn hero_audio_of(file: &MediaFile, original_language: &str) -> Option<HeroAudioDto> {
    let index = hero_audio_index(&file.audio_streams, original_language)?;
    let audio = &file.audio_streams[index];
    let layout = audio_channels_dot(audio.channels);
    Some(HeroAudioDto {
        index: u32::try_from(index).unwrap_or(u32::MAX),
        label: if layout.is_empty() {
            audio.codec.clone()
        } else {
            format!("{} {layout}", audio.codec)
        },
        codec: audio.codec.clone(),
        channels: audio.channels,
        language: audio.language.clone(),
        title: audio.title.clone(),
        flags: flag_names(&audio.flags),
    })
}

/// The technical badges of a movie, from its main video file and its source.
///
/// Every badge names the catalogue filter that shows the movies sharing the fact, with the value
/// the filter stores: the resolution class, the HDR format, the video codec (the bit depth is
/// only part of the label), the audio codec of the hero track (the original version, else the original language, else the
/// default), the aspect ratio with two
/// decimals and the media source key. The bitrate badge filters by "at least" the bitrate rounded
/// down to a whole Mb/s, because no two files share an exact bitrate.
pub fn tech_badges(item: &Item, movie: &MovieFields) -> Vec<TechBadgeDto> {
    let mut badges = Vec::new();
    let Some(video) = main_video(item) else {
        return badges;
    };
    if let Some(resolution) = resolution_label(video.width, video.height) {
        badges.push(list_badge(
            resolution.to_owned(),
            "video_format",
            resolution,
            false,
        ));
    }
    for format in &video.hdr_formats {
        let dolby_vision = format.eq_ignore_ascii_case("Dolby Vision");
        // Dolby Vision names its profile when the file says it ("Dolby Vision Profile 8.1"); the
        // filter the badge opens still matches the format.
        let label = match &video.hdr_details.dolby_vision_profile {
            profile if dolby_vision && !profile.is_empty() => {
                format!("Dolby Vision Profile {profile}")
            }
            _ => format.clone(),
        };
        badges.push(list_badge(label, "hdr_format", format, dolby_vision));
    }
    if !video.video_codec.is_empty() {
        let label = if video.bit_depth > 8 {
            format!("{} {}-bit", video.video_codec, video.bit_depth)
        } else {
            video.video_codec.clone()
        };
        badges.push(list_badge(label, "video_codec", &video.video_codec, false));
    }
    if video.video_bitrate_kbps > 0 {
        let floor = video.video_bitrate_kbps / 1000 * 1000;
        badges.push(TechBadgeDto {
            label: bitrate_label(video.video_bitrate_kbps),
            facet: "video_bitrate".to_owned(),
            values: Vec::new(),
            number: Some(f64::from(floor)),
            notable: false,
        });
    }
    let audio = hero_audio(&video.audio_streams, &item.core.original_language);
    if let Some(audio) = audio.filter(|a| !a.codec.is_empty()) {
        let layout = audio_channels_dot(audio.channels);
        let label = if layout.is_empty() {
            audio.codec.clone()
        } else {
            format!("{} {layout}", audio.codec)
        };
        let lower = audio.codec.to_lowercase();
        let notable = lower.contains("atmos") || lower.contains("dts:x") || lower.contains("dts-x");
        badges.push(list_badge(label, "audio_codec", &audio.codec, notable));
    }
    if video.aspect_ratio > 0.0 {
        badges.push(list_badge(
            format!("{:.2}:1", video.aspect_ratio),
            "aspect_ratio",
            &format!("{:.2}", video.aspect_ratio),
            false,
        ));
    }
    let source = media_source_label(&movie.media_source);
    if !source.is_empty() {
        badges.push(list_badge(
            source,
            "media_source",
            movie.media_source.name(),
            false,
        ));
    }
    badges
}

fn stream_rows(file: &MediaFile) -> Vec<DetailStreamDto> {
    let mut rows = Vec::new();
    if !file.video_codec.is_empty() || file.width > 0 {
        rows.push(DetailStreamDto {
            kind: "video".to_owned(),
            codec: file.video_codec.clone(),
            width: file.width,
            height: file.height,
            frame_rate: file.frame_rate,
            bitrate_kbps: file.video_bitrate_kbps,
            bit_depth: file.bit_depth,
            sample_rate_hz: 0,
            hdr: file.hdr_formats.clone(),
            hdr_details: HdrDetailsDto::of(&file.hdr_details),
            channels: 0,
            language: String::new(),
            title: String::new(),
            flags: Vec::new(),
        });
    }
    for audio in &file.audio_streams {
        rows.push(DetailStreamDto {
            kind: "audio".to_owned(),
            codec: audio.codec.clone(),
            width: 0,
            height: 0,
            frame_rate: 0.0,
            bitrate_kbps: audio.bitrate_kbps,
            bit_depth: audio.bit_depth,
            sample_rate_hz: audio.sample_rate_hz,
            hdr: Vec::new(),
            hdr_details: None,
            channels: audio.channels,
            language: audio.language.clone(),
            title: audio.title.clone(),
            flags: flag_names(&audio.flags),
        });
    }
    for subtitle in &file.subtitles {
        rows.push(DetailStreamDto {
            kind: "subtitle".to_owned(),
            codec: subtitle.codec.clone(),
            width: 0,
            height: 0,
            frame_rate: 0.0,
            bitrate_kbps: 0,
            bit_depth: 0,
            sample_rate_hz: 0,
            hdr: Vec::new(),
            hdr_details: None,
            channels: 0,
            language: subtitle.language.clone(),
            title: subtitle.title.clone(),
            flags: flag_names(&subtitle.flags),
        });
    }
    rows
}

/// Why the details of `file` are missing (see [`DetailVideoDto::read_problem`]), from the reason
/// the library keeps for a failed read.
fn read_problem(file: &MediaFile, reason: Option<&str>) -> &'static str {
    if file.has_media_info() {
        return "";
    }
    match reason {
        Some(cv_db::repo::media_info::TIMED_OUT) => "timeout",
        Some("file cannot be opened") => "open",
        // Earlier versions read a disc's marker file as a file of its own.
        Some(cv_db::repo::media_info::NOT_MEDIA)
            if cv_media::disc::disc_folder_of(&file.path()).is_some() =>
        {
            "disc"
        }
        Some(_) => "unreadable",
        None if file.path().is_dir() || cv_media::disc::disc_folder_of(&file.path()).is_some() => {
            "disc"
        }
        None => "",
    }
}

/// The reasons the library keeps for the videos of `item` that could not be read, by path. A
/// library that cannot be read gives none: the page then says only that the details are missing.
pub fn read_reasons(
    db: &Database,
    item: &Item,
) -> std::collections::HashMap<std::path::PathBuf, String> {
    let unread: Vec<MediaFile> = item
        .core
        .media_files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video && !f.has_media_info())
        .cloned()
        .collect();
    if unread.is_empty() {
        return std::collections::HashMap::new();
    }
    db.read(move |conn| {
        let mut reasons = std::collections::HashMap::new();
        for file in &unread {
            if let Some(reason) = cv_db::repo::media_info::unreadable_reason(conn, file)? {
                reasons.insert(file.path(), reason);
            }
        }
        Ok(reasons)
    })
    .unwrap_or_default()
}

pub fn video_files(
    item: &Item,
    reasons: &std::collections::HashMap<std::path::PathBuf, String>,
) -> Vec<DetailVideoDto> {
    let all = &item.core.media_files;
    // A disc's marker file (`BDMV/index.bdmv`) next to its disc folder is not a video of its own.
    let mut files: Vec<&MediaFile> = all
        .iter()
        .filter(|f| f.kind == MediaFileType::Video && !f.is_covered_disc_marker(all))
        .collect();
    files.sort_by_key(|f| f.stacking);
    files
        .into_iter()
        .map(|f| DetailVideoDto {
            name: f.filename.clone(),
            folder: f.directory.clone(),
            size_bytes: exact(f.size_bytes),
            container: f.container_format.clone(),
            duration_secs: f.duration_secs,
            analysed: f.has_media_info(),
            read_problem: read_problem(f, reasons.get(&f.path()).map(String::as_str)).to_owned(),
            streams: stream_rows(f),
        })
        .collect()
}

pub fn file_stem(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
        _ => name.to_owned(),
    }
}

/// The trailers of a movie as the page lists them: the web trailers the scrape found (and the
/// NFO named), then the trailer files that sit next to the movie. A read-only list.
fn trailers_of(item: &Item) -> Vec<DetailTrailerDto> {
    let mut entries: Vec<DetailTrailerDto> = match &item.detail {
        ItemDetail::Movie(movie) => movie
            .trailers
            .iter()
            .map(|t| DetailTrailerDto {
                name: t.name.clone(),
                provider: t.provider.clone(),
                quality: t.quality.clone(),
                language: t.language.clone(),
                url: t.url.clone(),
                local_file: None,
            })
            .collect(),
        _ => Vec::new(),
    };
    entries.extend(
        item.core
            .media_files
            .iter()
            .filter(|f| f.kind == MediaFileType::Trailer)
            .map(|f| DetailTrailerDto {
                name: file_stem(&f.filename),
                provider: String::new(),
                quality: String::new(),
                language: String::new(),
                url: String::new(),
                local_file: Some(f.filename.clone()),
            }),
    );
    entries
}

pub fn subtitles_of(item: &Item) -> Vec<DetailSubtitleDto> {
    let mut subtitles = Vec::new();
    for file in item
        .core
        .media_files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
    {
        for (position, s) in file.subtitles.iter().enumerate() {
            subtitles.push(DetailSubtitleDto {
                file_name: None,
                language: s.language.clone(),
                codec: s.codec.clone(),
                title: s.title.clone(),
                flags: flag_names(&s.flags),
                video: Some(file.filename.clone()),
                position: u32::try_from(position).ok(),
                codec_id: s.codec_id.clone(),
            });
        }
    }
    // A subtitle file's language and marks are often only in its name (nothing reads them from
    // a text subtitle, and nothing is read without MediaInfo): the name fills the gaps.
    let video_stem = item
        .core
        .media_files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .min_by(|a, b| (a.stacking, &a.filename).cmp(&(b.stacking, &b.filename)))
        .map(|f| {
            f.filename
                .rsplit_once('.')
                .map_or(f.filename.as_str(), |(stem, _)| stem)
        })
        .unwrap_or_default();
    for file in item
        .core
        .media_files
        .iter()
        .filter(|f| f.kind == MediaFileType::Subtitle)
    {
        let stream = cv_media::external::subtitle_file_stream(file, video_stem);
        let codec = if stream.codec.is_empty() {
            file.filename
                .rsplit_once('.')
                .map(|(_, ext)| ext.to_uppercase())
                .unwrap_or_default()
        } else {
            stream.codec
        };
        subtitles.push(DetailSubtitleDto {
            file_name: Some(file.filename.clone()),
            language: stream.language,
            codec,
            title: stream.title,
            flags: flag_names(&stream.flags),
            video: None,
            position: None,
            codec_id: String::new(),
        });
    }
    subtitles
}

/// The movies of the collection the movie belongs to, as the page shows them.
struct SetFacts {
    id: ItemId,
    name: String,
    owned: u32,
    missing: u32,
}

/// What one read of the database gives the page.
struct Loaded {
    item: Item,
    set: Option<SetFacts>,
    needs_review: bool,
    is_new: bool,
    availability: Availability,
}

/// Reads the movie detail page.
pub struct DetailReader {
    db: Arc<Database>,
    sources: Arc<LibrarySources>,
    cache: Option<Arc<ThumbCache>>,
    lifecycle: Option<Arc<LifecycleService>>,
    rating_status: Option<Arc<RatingStatusService>>,
}

impl DetailReader {
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
            rating_status: None,
        }
    }

    /// Also puts the ratings strip (a value or the reason there is none) on the page.
    #[must_use]
    pub fn with_rating_status(mut self, status: Option<Arc<RatingStatusService>>) -> Self {
        self.rating_status = status;
        self
    }

    /// The chips of the ratings strip of a movie or a show; none without the service.
    pub fn rating_chips_of(&self, id: ItemId, item: &Item) -> Vec<RatingChipDto> {
        let Some(status) = &self.rating_status else {
            return Vec::new();
        };
        match status.chips(id, item.kind(), &item.core.ratings) {
            Ok(chips) => chips.into_iter().map(RatingChipDto::from).collect(),
            Err(error) => {
                tracing::warn!(%error, "the rating chips of a title could not be read");
                Vec::new()
            }
        }
    }

    /// Also puts the stage, flags and explanation of the title on its page.
    #[must_use]
    pub fn with_lifecycle(mut self, lifecycle: Option<Arc<LifecycleService>>) -> Self {
        self.lifecycle = lifecycle;
        self
    }

    /// The lifecycle of a title, when it has one.
    pub fn lifecycle_of(&self, id: ItemId) -> Option<LifecycleDto> {
        lifecycle_of(self.lifecycle.as_ref(), id)
    }

    fn load(&self, id: ItemId) -> Result<Loaded, LibraryCommandError> {
        let loaded = self.db.read(|conn| {
            let Some(item) = get_item(conn, id)? else {
                return Ok(None);
            };
            if !matches!(item.detail, ItemDetail::Movie(_)) {
                return Ok(None);
            }
            let flags = get_flags(conn, id)?;
            let set = match item.parent_id {
                Some(parent) => match get_item(conn, parent)? {
                    Some(Item {
                        detail: ItemDetail::Set(fields),
                        core,
                        ..
                    }) => Some(SetFacts {
                        id: parent,
                        name: core.title,
                        owned: u32::try_from(children_of(conn, parent)?.len()).unwrap_or(u32::MAX),
                        missing: u32::try_from(fields.missing_movies.len()).unwrap_or(u32::MAX),
                    }),
                    _ => None,
                },
                None => None,
            };
            let availability =
                live_availability(get_availability(conn, id)?.0, &item.core.data_source);
            Ok(Some(Loaded {
                item,
                set,
                needs_review: flags.review_state == cv_domain::ReviewState::NeedsReview,
                is_new: flags.is_new,
                availability,
            }))
        })?;
        loaded.ok_or_else(not_found)
    }

    /// The image behind a file: its key (the file is hashed once and remembered) and what is known
    /// for painting before the thumbnail arrives. `None` when the file cannot be read.
    pub fn image_of(&self, file: &MediaFile) -> Option<PosterDto> {
        match self.sources.key_for_file(&file.path()) {
            Ok(key) => Some(poster_dto(self.cache.as_deref(), &key)),
            Err(error) => {
                tracing::debug!(%error, path = %file.path().display(), "artwork not readable");
                None
            }
        }
    }

    /// A person of the credits, with the portrait the artwork download put in `folder/.actors`.
    pub fn credit(&self, folder: &str, credit: &cv_domain::Credit) -> DetailCreditDto {
        let portrait = matches!(credit.kind, CreditKind::Actor | CreditKind::Guest)
            .then(|| self.portrait_of(folder, &credit.person.name))
            .flatten();
        DetailCreditDto {
            name: credit.person.name.clone(),
            role: credit.role.clone(),
            kind: enum_name(&credit.kind),
            portrait,
            imdb_id: person_id(credit, id_key::IMDB),
            tmdb_id: person_id(credit, id_key::TMDB),
        }
    }

    /// The portrait of a person in the `.actors` folder of `folder`, keyed for `cv://img`.
    fn portrait_of(&self, folder: &str, name: &str) -> Option<PosterDto> {
        if folder.trim().is_empty() {
            return None;
        }
        let file = portrait_file(Path::new(folder), name)?;
        match self.sources.key_for_file(&file) {
            Ok(key) => Some(poster_dto(self.cache.as_deref(), &key)),
            Err(error) => {
                tracing::debug!(%error, path = %file.display(), "portrait not readable");
                None
            }
        }
    }

    pub fn artwork(&self, item: &Item) -> Vec<DetailArtworkDto> {
        SLOTS
            .iter()
            .map(|(slot, kind)| {
                let file = item.core.media_files.iter().find(|f| f.kind == *kind);
                DetailArtworkDto {
                    slot: (*slot).to_owned(),
                    file_name: file.map(|f| f.filename.clone()),
                    image: file.and_then(|f| self.image_of(f)),
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

    /// Everything the movie detail page shows. Blocking.
    pub fn movie(&self, id: ItemId) -> Result<MovieDetailDto, LibraryCommandError> {
        let Loaded {
            item,
            set,
            needs_review,
            is_new,
            availability,
        } = self.load(id)?;
        let ItemDetail::Movie(movie) = &item.detail else {
            return Err(not_found());
        };
        let core = &item.core;

        let (ratings, user_rating) = detail_ratings(core);

        let mut files: Vec<DetailFileDto> = core
            .media_files
            .iter()
            .filter(|f| f.kind != MediaFileType::Video)
            .take(MAX_FILES)
            .map(|f| DetailFileDto {
                kind: enum_name(&f.kind),
                name: f.filename.clone(),
                size_bytes: exact(f.size_bytes),
            })
            .collect();
        files.shrink_to_fit();
        let reasons = read_reasons(&self.db, &item);
        let videos = video_files(&item, &reasons);

        let backdrop = core
            .media_files
            .iter()
            .find(|f| f.kind == MediaFileType::Fanart)
            .and_then(|f| self.image_of(f));
        let logo = core
            .media_files
            .iter()
            .find(|f| f.kind == MediaFileType::Clearlogo)
            .and_then(|f| self.image_of(f));
        let poster = core
            .media_files
            .iter()
            .find(|f| f.kind == MediaFileType::Poster)
            .and_then(|f| self.image_of(f));

        Ok(MovieDetailDto {
            id: id.0.to_string(),
            title: core.title.clone(),
            original_title: core.original_title.clone(),
            tagline: movie.tagline.clone(),
            year: u32::from(core.year),
            certification: certification_label(&movie.certification.0),
            runtime_minutes: movie.runtime_minutes,
            release_date: movie.release_date.map(|d| d.to_string()),
            genres: movie.genres.iter().map(genre_label).collect(),
            directors: names_of(&item, CreditKind::Director),
            writers: names_of(&item, CreditKind::Writer),
            studios: core.production_companies.clone(),
            countries: movie.countries.clone(),
            languages: movie.spoken_languages.clone(),
            edition: edition_label(&movie.edition),
            media_source: media_source_label(&movie.media_source),
            plot: core.plot.clone(),
            collection: set.map(|s| DetailCollectionDto {
                id: s.id.0.to_string(),
                name: s.name,
                owned: s.owned,
                total: s.owned.saturating_add(s.missing),
            }),
            ids: core
                .ids
                .iter()
                .map(|(provider, value)| DetailIdDto {
                    provider: provider.to_owned(),
                    value: value.to_owned(),
                })
                .collect(),
            ratings,
            rating_chips: self.rating_chips_of(id, &item),
            user_rating,
            top250: movie.top250,
            watched: movie.watched,
            play_count: movie.play_count,
            last_watched_ms: movie.last_watched_ms.filter(|ms| *ms > 0).map(millis),
            availability: availability.as_str().to_owned(),
            locked: core.locked,
            needs_review,
            lifecycle: self.lifecycle_of(id),
            is_new,
            date_added_ms: millis(core.date_added_ms),
            folder: core.path.clone(),
            data_source: core.data_source.clone(),
            source_label: source_label_of(
                &self.db,
                cv_domain::SourceKind::Movie,
                &core.data_source,
            ),
            note: core.note.clone(),
            tags: core.tags.iter().map(str::to_owned).collect(),
            backdrop,
            logo,
            poster,
            tech: tech_badges(&item, movie),
            original_language: core.original_language.clone(),
            hero_audio: main_video(&item)
                .and_then(|video| hero_audio_of(video, &core.original_language)),
            credits: cv_domain::merge_crew(&core.credits)
                .iter()
                .filter(|c| !c.person.name.trim().is_empty())
                .map(|c| self.credit(&core.path, c))
                .collect(),
            videos,
            files,
            artwork: self.artwork(&item),
            trailers: trailers_of(&item),
            subtitles: subtitles_of(&item),
        })
    }
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Everything the movie detail page shows.
pub async fn get_movie_detail(
    reader: DetailReader,
    id: ItemId,
) -> Result<MovieDetailDto, LibraryCommandError> {
    blocking(move || reader.movie(id)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credit_ids_reach_the_dto() {
        let reader = Lab::new().reader();
        let mut credit = Credit::new(CreditKind::Actor, "Harrison Ford", "Deckard");
        credit.person.ids.set(id_key::IMDB, "nm0000148");
        credit.person.ids.set(id_key::TMDB, "3");
        let dto = reader.credit("", &credit);
        assert_eq!(dto.imdb_id.as_deref(), Some("nm0000148"));
        assert_eq!(dto.tmdb_id.as_deref(), Some("3"));
        credit.person.ids.set(id_key::TMDB, " ");
        let dto = reader.credit("", &credit);
        assert_eq!(dto.tmdb_id, None);
    }
    use cv_db::repo::update_item;
    use cv_domain::{
        AudioStream, Credit, CreditKind, Edition, Genre, MediaSource, Rating, SubtitleStream,
        Trailer,
    };

    struct Lab {
        dir: tempfile::TempDir,
        db: Arc<Database>,
        sources: Arc<LibrarySources>,
    }

    impl Lab {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
            Self {
                dir,
                db,
                sources: Arc::new(LibrarySources::new()),
            }
        }

        fn reader(&self) -> DetailReader {
            DetailReader::new(self.db.clone(), self.sources.clone(), None)
        }

        fn add(&self, item: Item) -> ItemId {
            self.db
                .write(move |conn| cv_db::repo::insert_item(conn, &item))
                .unwrap()
        }

        /// A 4K Dolby Vision movie with streams, artwork, a trailer file and subtitles on disk.
        fn dune(&self) -> ItemId {
            let folder = self.dir.path().join("Dune Part Two (2024)");
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("fanart.jpg"), b"fanart bytes").unwrap();
            std::fs::write(folder.join("poster.jpg"), b"poster bytes").unwrap();
            // The artwork download keeps portraits of the cast here, named after the person.
            std::fs::create_dir_all(folder.join(".actors")).unwrap();
            std::fs::write(
                folder.join(".actors").join("Zendaya.jpg"),
                b"portrait bytes",
            )
            .unwrap();
            let dir = folder.to_string_lossy().into_owned();
            let mut item = Item::movie("Dune: Part Two");
            item.core.year = 2024;
            item.core.path = dir.clone();
            item.core.plot = "Paul joins the Fremen.".into();
            item.core.date_added_ms = 1_700_000_000_000;
            item.core.ids.set("tmdb", "693134");
            item.core.ids.set("imdb", "tt15239678");
            item.core
                .production_companies
                .push("Legendary Pictures".into());
            item.core
                .ratings
                .insert(Rating::new("imdb", 8.5, 640_000, 10));
            item.core.ratings.insert(
                Rating::new("tomatometerallcritics", 92.0, 0, 100).fetched_at(1_700_000_000_000),
            );
            item.core.ratings.insert(Rating::new("user", 9.0, 0, 10));
            item.core.credits.extend([
                Credit::new(CreditKind::Actor, "Zendaya", "Chani"),
                Credit::new(CreditKind::Director, "Denis Villeneuve", ""),
                Credit::new(CreditKind::Writer, "Jon Spaihts", "Screenplay"),
            ]);
            let mut video = MediaFile::new(MediaFileType::Video, dir.clone(), "Dune.mkv");
            video.size_bytes = 58_000_000_000;
            video.video_codec = "HEVC".into();
            video.container_format = "Matroska".into();
            video.width = 3840;
            video.height = 1608;
            video.bit_depth = 10;
            video.frame_rate = 23.976;
            video.video_bitrate_kbps = 46_200;
            video.aspect_ratio = 2.39;
            video.hdr_formats = vec!["Dolby Vision".into(), "HDR10".into()];
            video.hdr_details = cv_domain::HdrDetails {
                dolby_vision_profile: "8.1".into(),
                dolby_vision_compatibility: "HDR10".into(),
                mastering_primaries: "Display P3".into(),
                mastering_min_nits: Some(0.005),
                mastering_max_nits: Some(1000.0),
                max_cll_nits: Some(1000),
                max_fall_nits: Some(400),
            };
            video.audio_streams = vec![
                AudioStream {
                    codec: "E-AC-3".into(),
                    language: "en".into(),
                    channels: 6,
                    ..AudioStream::default()
                },
                AudioStream {
                    codec: "TrueHD Atmos".into(),
                    language: "en".into(),
                    channels: 8,
                    flags: [StreamFlag::Default].into_iter().collect(),
                    ..AudioStream::default()
                },
            ];
            video.subtitles = vec![SubtitleStream {
                codec: "PGS".into(),
                codec_id: "S_HDMV/PGS".into(),
                language: "en".into(),
                flags: [StreamFlag::Forced].into_iter().collect(),
                ..SubtitleStream::default()
            }];
            item.core.media_files.push(video);
            for (kind, name) in [
                (MediaFileType::Fanart, "fanart.jpg"),
                (MediaFileType::Poster, "poster.jpg"),
                (MediaFileType::Nfo, "Dune.nfo"),
                (MediaFileType::Trailer, "Dune-trailer.mkv"),
            ] {
                let mut f = MediaFile::new(kind, dir.clone(), name);
                f.size_bytes = 1024;
                item.core.media_files.push(f);
            }
            let mut sub = MediaFile::new(MediaFileType::Subtitle, dir.clone(), "Dune.tr.srt");
            sub.subtitles = vec![SubtitleStream {
                codec: "SubRip".into(),
                language: "tr".into(),
                ..SubtitleStream::default()
            }];
            item.core.media_files.push(sub);
            if let ItemDetail::Movie(m) = &mut item.detail {
                m.runtime_minutes = 166;
                m.certification.0 = "US_PG13".into();
                m.genres = vec![Genre::ScienceFiction, Genre::Adventure];
                m.media_source = MediaSource::WebDl;
                m.edition = Edition::TheatricalEdition;
                m.trailers.push(Trailer {
                    name: "Official Trailer".into(),
                    provider: "tmdb".into(),
                    quality: "1080p".into(),
                    url: "https://example.test/t".into(),
                    ..Trailer::default()
                });
            }
            self.add(item)
        }
    }

    fn badge<'a>(badges: &'a [TechBadgeDto], label: &str) -> &'a TechBadgeDto {
        badges
            .iter()
            .find(|b| b.label == label)
            .unwrap_or_else(|| panic!("no badge {label}: {badges:?}"))
    }

    /// The movie, show and episode pages carry the lifecycle of the title, the per-source rating
    /// chips and the label of the library folder, read through the services the app boots.
    #[test]
    fn the_detail_pages_carry_the_lifecycle_the_rating_chips_and_the_source_label() {
        use cv_app::data_location::{DataLocation, LocationInputs};
        use cv_app::secrets::SecretStore;
        use cv_app::services::{AppServices, BootOptions};
        use cv_db::repo::data_sources::{insert_source, set_source_label};
        use cv_domain::SourceKind;

        let temp = tempfile::tempdir().unwrap();
        let inputs = LocationInputs {
            exe_dir: temp.path().join("app"),
            user_data_base: temp.path().join("userdata"),
            user_config_base: temp.path().join("userconfig"),
        };
        std::fs::create_dir_all(&inputs.exe_dir).unwrap();
        let services = AppServices::boot(BootOptions::new(
            DataLocation::resolve(&inputs),
            Arc::new(SecretStore::in_memory()),
        ))
        .expect("boot");
        let db = Arc::clone(services.db());
        let movies = temp.path().join("Movies 4K").to_string_lossy().into_owned();
        let shows = temp.path().join("Series").to_string_lossy().into_owned();
        let (movie_root, show_root) = (movies.clone(), shows.clone());
        db.write(move |tx| {
            let source = insert_source(tx, SourceKind::Movie, &movie_root)?;
            set_source_label(tx, source.id, "NAS 4K")?;
            insert_source(tx, SourceKind::Tv, &show_root)?;
            Ok(())
        })
        .unwrap();

        let mut movie = Item::movie("Heat");
        movie.core.year = 1995;
        movie.core.data_source.clone_from(&movies);
        movie.core.path = format!("{movies}/Heat (1995)");
        movie
            .core
            .ratings
            .insert(Rating::new("imdb", 8.3, 700_000, 10));
        let mut show = Item::show("Severance");
        show.core.data_source.clone_from(&shows);
        show.core.path = format!("{shows}/Severance");
        show.core
            .ratings
            .insert(Rating::new("imdb", 8.7, 300_000, 10));
        let (movie_id, show_id) = db
            .write(move |tx| {
                Ok((
                    cv_db::repo::insert_item(tx, &movie)?,
                    cv_db::repo::insert_item(tx, &show)?,
                ))
            })
            .unwrap();
        let mut episode = Item::episode(show_id, "Good News About Hell");
        episode.core.data_source.clone_from(&shows);
        episode.core.path = format!("{shows}/Severance/Season 1");
        let episode_id = db
            .write(move |tx| cv_db::repo::insert_item(tx, &episode))
            .unwrap();

        let lifecycle = Some(Arc::clone(services.lifecycle()));
        let status = Some(Arc::clone(services.rating_status()));
        let sources = Arc::new(LibrarySources::new());
        let movies_reader = DetailReader::new(Arc::clone(&db), Arc::clone(&sources), None)
            .with_lifecycle(lifecycle.clone())
            .with_rating_status(status.clone());
        let tv_reader = crate::tv::TvReader::new(Arc::clone(&db), sources, None)
            .with_lifecycle(lifecycle)
            .with_rating_status(status);

        let chip = |chips: &[RatingChipDto], key: &str| {
            chips
                .iter()
                .find(|c| c.key == key)
                .map(|c| c.state.clone())
                .unwrap_or_else(|| panic!("no {key} chip: {chips:?}"))
        };

        let detail = movies_reader.movie(movie_id).unwrap();
        assert!(
            detail.lifecycle.is_some(),
            "the movie page has no lifecycle"
        );
        assert_eq!(chip(&detail.rating_chips, "imdb"), "value");
        assert_eq!(detail.source_label, "NAS 4K");

        let show_detail = tv_reader.show(show_id).unwrap();
        assert!(
            show_detail.lifecycle.is_some(),
            "the show page has no lifecycle"
        );
        assert_eq!(chip(&show_detail.rating_chips, "imdb"), "value");
        assert_eq!(show_detail.source_label, "Series");

        let episode_detail = tv_reader.episode(episode_id).unwrap();
        assert!(
            episode_detail.lifecycle.is_some(),
            "the episode page has no lifecycle of its show"
        );
        assert_eq!(episode_detail.source_label, "Series");
    }

    #[test]
    fn the_page_gathers_the_facts_ratings_credits_and_collection() {
        let lab = Lab::new();
        let set = lab.add(Item::set("Dune Collection"));
        let id = lab.dune();
        let mut movie = lab
            .db
            .read(|conn| get_item(conn, id))
            .unwrap()
            .expect("movie");
        movie.parent_id = Some(set);
        lab.db.write(move |tx| update_item(tx, &movie)).unwrap();

        let detail = lab.reader().movie(id).unwrap();

        assert_eq!(detail.title, "Dune: Part Two");
        assert_eq!(detail.year, 2024);
        assert_eq!(detail.runtime_minutes, 166);
        assert_eq!(detail.genres, ["Science Fiction", "Adventure"]);
        // The stored token reads as the rating, not as `US_PG13`.
        assert_eq!(detail.certification, "PG-13");
        assert_eq!(detail.directors, ["Denis Villeneuve"]);
        assert_eq!(detail.writers, ["Jon Spaihts"]);
        assert_eq!(detail.studios, ["Legendary Pictures"]);
        assert_eq!(detail.media_source, "Web-DL");
        assert_eq!(detail.edition, "Theatrical Edition");
        assert_eq!(detail.user_rating, Some(9.0));
        let sources: Vec<&str> = detail.ratings.iter().map(|r| r.source.as_str()).collect();
        assert_eq!(
            sources,
            ["imdb", "tomatometerallcritics"],
            "user rating is separate"
        );
        assert_eq!(detail.ratings[0].votes, 640_000);
        // A rating without a date is stamped when it is stored; a given date is kept.
        assert!(detail.ratings[0].fetched_at_ms > 0.0);
        assert_eq!(detail.ratings[1].max, 100);
        assert_eq!(detail.ratings[1].fetched_at_ms, 1_700_000_000_000.0);
        let collection = detail.collection.expect("collection");
        assert_eq!(collection.name, "Dune Collection");
        assert_eq!((collection.owned, collection.total), (1, 1));
        assert!(
            detail
                .ids
                .iter()
                .any(|i| i.provider == "tmdb" && i.value == "693134")
        );
        assert_eq!(
            detail.credits.iter().filter(|c| c.kind == "actor").count(),
            1
        );
    }

    /// Keys of the providers in the list are set.
    struct Keys(Vec<&'static str>);

    impl cv_app::rating_status::RatingAccess for Keys {
        fn has_credentials(&self, provider: &str) -> bool {
            self.0.contains(&provider)
        }
    }

    fn chip_states(detail: &MovieDetailDto) -> Vec<(&str, &str)> {
        detail
            .rating_chips
            .iter()
            .map(|c| (c.key.as_str(), c.state.as_str()))
            .collect()
    }

    #[test]
    fn the_page_carries_the_ratings_strip_with_each_source_state() {
        let lab = Lab::new();
        let id = lab.dune();
        let settings = Arc::new(cv_app::app_settings::SettingsService::in_memory(""));
        let status = RatingStatusService::new(
            Arc::clone(&lab.db),
            Arc::clone(&settings),
            Arc::new(Keys(vec!["omdb", "tmdb"])),
            Arc::new(cv_app::lifecycle::Working::default()),
        );
        let reader = lab.reader().with_rating_status(Some(Arc::clone(&status)));

        // The default sources: OMDb has its key and is chosen, MDBList has no key, Trakt is not
        // connected.
        let detail = reader.movie(id).unwrap();
        let states = chip_states(&detail);
        assert!(states.contains(&("imdb", "value")), "{states:?}");
        assert!(
            states.contains(&("tomatometerallcritics", "value")),
            "{states:?}"
        );
        assert!(
            states.contains(&("metacritic", "not_fetched")),
            "{states:?}"
        );
        assert!(states.contains(&("letterboxd", "needs_key")), "{states:?}");
        assert!(states.contains(&("trakt", "connect_service")), "{states:?}");
        let letterboxd = detail
            .rating_chips
            .iter()
            .find(|c| c.key == "letterboxd")
            .unwrap();
        assert_eq!(letterboxd.provider.as_deref(), Some("mdblist"));
        assert_eq!(letterboxd.reason, "MDBList needs an API key.");

        // Only the dataset chosen: Metacritic waits for OMDb to be chosen, not for a key.
        settings
            .set("ratings.sources-movie", serde_json::json!("imdb-dataset"))
            .unwrap();
        let detail = reader.movie(id).unwrap();
        let metacritic = detail
            .rating_chips
            .iter()
            .find(|c| c.key == "metacritic")
            .unwrap();
        assert_eq!(metacritic.state, "not_selected");
        assert_eq!(metacritic.provider.as_deref(), Some("omdb"));
    }

    #[test]
    fn a_movie_says_whether_its_files_are_where_the_library_expects_them() {
        let lab = Lab::new();
        let id = lab.dune();
        assert_eq!(lab.reader().movie(id).unwrap().availability, "online");
        lab.db
            .write(move |tx| {
                cv_db::repo::set_availability(
                    tx,
                    id,
                    Availability::Missing,
                    "the folder was not found",
                )
            })
            .unwrap();
        assert_eq!(lab.reader().movie(id).unwrap().availability, "missing");
        lab.db
            .write(move |tx| {
                cv_db::repo::set_availability(tx, id, Availability::Offline, "drive unplugged")
            })
            .unwrap();
        assert_eq!(lab.reader().movie(id).unwrap().availability, "offline");
    }

    #[test]
    fn a_movie_whose_library_folder_is_away_is_offline_before_the_next_check() {
        let lab = Lab::new();
        let id = lab.dune();
        let away = lab.dir.path().join("moved away");
        let source = away.to_string_lossy().into_owned();
        lab.db
            .write(move |tx| {
                let mut movie = get_item(tx, id)?.unwrap();
                movie.core.data_source = source;
                cv_db::repo::update_item(tx, &movie)
            })
            .unwrap();
        // Stored as online: the files were not checked again since the folder went away.
        assert_eq!(lab.reader().movie(id).unwrap().availability, "offline");
        std::fs::create_dir_all(&away).unwrap();
        assert_eq!(lab.reader().movie(id).unwrap().availability, "online");
    }

    #[test]
    fn actors_with_a_downloaded_portrait_get_its_image_key_and_crew_do_not() {
        let lab = Lab::new();
        let id = lab.dune();
        let mut movie = lab.db.read(|conn| get_item(conn, id)).unwrap().unwrap();
        movie
            .core
            .credits
            .push(Credit::new(CreditKind::Actor, "Timothée Chalamet", "Paul"));
        lab.db.write(move |tx| update_item(tx, &movie)).unwrap();

        let detail = lab.reader().movie(id).unwrap();
        let by_name = |name: &str| {
            detail
                .credits
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("no credit {name}"))
        };

        let zendaya = by_name("Zendaya").portrait.as_ref().expect("a portrait");
        assert!(!zendaya.key.is_empty());
        // The key is the content key of the file, so the same file gives the same key.
        let expected = lab
            .sources
            .key_for_file(
                &lab.dir
                    .path()
                    .join("Dune Part Two (2024)/.actors/Zendaya.jpg"),
            )
            .unwrap();
        assert_eq!(zendaya.key, expected.to_string());
        // No file in `.actors` for this actor, and crew never have portraits.
        assert_eq!(by_name("Timothée Chalamet").portrait, None);
        assert_eq!(by_name("Denis Villeneuve").portrait, None);
    }

    #[test]
    fn the_hero_audio_is_the_original_version_not_the_default_dub() {
        use cv_domain::AudioStream;
        let lab = Lab::new();
        let track =
            |codec: &str, language: &str, channels: u32, flags: &[StreamFlag]| AudioStream {
                codec: codec.to_owned(),
                language: language.to_owned(),
                channels,
                flags: flags.iter().copied().collect(),
                ..AudioStream::default()
            };
        let streams = vec![
            track("AC3", "fra", 6, &[StreamFlag::Default, StreamFlag::Dub]),
            track("AC3", "eng", 2, &[StreamFlag::Comment]),
            track("TrueHD Atmos", "eng", 8, &[]),
        ];
        let movie = |title: &str, language: &str| {
            let folder = lab.dir.path().join(title);
            std::fs::create_dir_all(&folder).unwrap();
            let mut item = Item::movie(title);
            item.core.path = folder.to_string_lossy().into_owned();
            item.core.original_language = language.to_owned();
            let mut file =
                MediaFile::new(MediaFileType::Video, item.core.path.clone(), "movie.mkv");
            file.audio_streams = streams.clone();
            item.core.media_files.push(file);
            lab.add(item)
        };

        // English original: the Atmos track, found through `en` against `eng`; the commentary
        // in the same language is passed over.
        let english = lab.reader().movie(movie("English", "en")).unwrap();
        let hero = english.hero_audio.expect("a hero track");
        assert_eq!((hero.index, hero.label.as_str()), (2, "TrueHD Atmos 7.1"));
        assert_eq!(english.original_language, "en");
        assert!(english.tech.iter().any(|b| b.label == "TrueHD Atmos 7.1"));

        // A French original keeps the French track, though it is a dub of nothing here.
        let french = lab.reader().movie(movie("French", "fr")).unwrap();
        assert_eq!(
            french.hero_audio.map(|h| h.language),
            Some("fra".to_owned())
        );

        // Without a known original language the default track leads.
        let unknown = lab.reader().movie(movie("Unknown", "")).unwrap();
        assert_eq!(unknown.hero_audio.map(|h| h.index), Some(0));
    }

    #[test]
    fn badges_name_the_filter_and_value_a_click_applies() {
        let lab = Lab::new();
        let id = lab.dune();
        let detail = lab.reader().movie(id).unwrap();
        let tech = &detail.tech;

        let resolution = badge(tech, "2160p");
        assert_eq!(
            (resolution.facet.as_str(), resolution.values.as_slice()),
            ("video_format", ["2160p".to_owned()].as_slice())
        );
        let dv = badge(tech, "Dolby Vision Profile 8.1");
        assert_eq!(dv.facet, "hdr_format");
        assert_eq!(
            dv.values,
            ["Dolby Vision"],
            "the filter still matches the format"
        );
        assert!(dv.notable);
        assert!(!badge(tech, "HDR10").notable);
        // The bit depth is part of the label only: the filter matches the codec.
        let codec = badge(tech, "HEVC 10-bit");
        assert_eq!(codec.facet, "video_codec");
        assert_eq!(codec.values, ["HEVC"]);
        // The audio badge follows the default stream, not the first one.
        let audio = badge(tech, "TrueHD Atmos 7.1");
        assert_eq!(audio.facet, "audio_codec");
        assert_eq!(audio.values, ["TrueHD Atmos"]);
        assert!(audio.notable);
        let bitrate = badge(tech, "46 Mb/s");
        assert_eq!(bitrate.facet, "video_bitrate");
        assert_eq!(bitrate.number, Some(46_000.0));
        assert!(bitrate.values.is_empty());
        let aspect = badge(tech, "2.39:1");
        assert_eq!(
            (aspect.facet.as_str(), aspect.values[0].as_str()),
            ("aspect_ratio", "2.39")
        );
        let source = badge(tech, "Web-DL");
        assert_eq!(
            (source.facet.as_str(), source.values[0].as_str()),
            ("media_source", "WEB_DL")
        );
    }

    #[test]
    fn files_streams_artwork_trailers_and_subtitles_are_listed() {
        let lab = Lab::new();
        let id = lab.dune();
        let detail = lab.reader().movie(id).unwrap();

        assert_eq!(detail.videos.len(), 1);
        let video = &detail.videos[0];
        assert!(video.analysed);
        let kinds: Vec<&str> = video.streams.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(kinds, ["video", "audio", "audio", "subtitle"]);
        assert_eq!(video.streams[0].hdr, ["Dolby Vision", "HDR10"]);
        let details = video.streams[0].hdr_details.as_ref().unwrap();
        assert_eq!(details.dolby_vision_profile, "8.1");
        assert_eq!(details.max_cll_nits, Some(1000));
        assert_eq!(video.streams[3].flags, ["forced"]);

        assert!(detail.files.iter().all(|f| f.kind != "video"));
        assert!(detail.files.iter().any(|f| f.kind == "nfo"));

        let slots: Vec<(&str, bool)> = detail
            .artwork
            .iter()
            .map(|a| (a.slot.as_str(), a.file_name.is_some()))
            .collect();
        assert_eq!(slots[0], ("poster", true));
        assert_eq!(slots[1], ("fanart", true));
        assert_eq!(slots[5], ("banner", false));
        assert_eq!(detail.artwork.len(), 8);
        let backdrop = detail.backdrop.expect("backdrop key");
        assert_eq!(backdrop.key.len(), 32);
        assert!(detail.logo.is_none());

        assert_eq!(detail.trailers.len(), 2);
        assert_eq!(detail.trailers[0].url, "https://example.test/t");
        assert_eq!(
            detail.trailers[1].local_file.as_deref(),
            Some("Dune-trailer.mkv")
        );

        assert_eq!(detail.subtitles.len(), 2);
        assert_eq!(detail.subtitles[0].file_name, None);
        // An embedded stream says which video it is in, where it sits among that video's
        // subtitle streams and its codec id, so Extract can name it; a file says none of these.
        assert_eq!(detail.subtitles[0].position, Some(0));
        assert_eq!(detail.subtitles[0].codec_id, "S_HDMV/PGS");
        assert!(detail.subtitles[0].video.is_some());
        assert_eq!(detail.subtitles[1].video, None);
        assert_eq!(detail.subtitles[1].position, None);
        assert_eq!(
            detail.subtitles[1].file_name.as_deref(),
            Some("Dune.tr.srt")
        );
        assert_eq!(detail.subtitles[1].language, "tr");
    }

    /// reg2-6: without MediaInfo the subtitle files carry no stream; the page reads the language
    /// and the forced mark from their names instead of saying "Unknown language".
    #[test]
    fn subtitle_files_without_media_info_take_their_language_from_the_name() {
        let mut item = Item::movie("Heat");
        let dir = "D:/Movies/Heat (1995)";
        item.core
            .media_files
            .push(MediaFile::new(MediaFileType::Video, dir, "Heat (1995).mkv"));
        for name in [
            "Heat (1995).deu.forced.srt",
            "Heat (1995).eng.srt",
            "Heat (1995).fra.ass",
            "Heat (1995).jpn.srt",
        ] {
            item.core
                .media_files
                .push(MediaFile::new(MediaFileType::Subtitle, dir, name));
        }
        let rows: Vec<(String, Vec<String>, String)> = subtitles_of(&item)
            .into_iter()
            .map(|s| (s.language, s.flags, s.codec))
            .collect();
        assert_eq!(
            rows,
            [
                ("de".to_owned(), vec!["forced".to_owned()], "SRT".to_owned()),
                ("en".to_owned(), vec![], "SRT".to_owned()),
                ("fr".to_owned(), vec![], "ASS".to_owned()),
                ("ja".to_owned(), vec![], "SRT".to_owned()),
            ]
        );
    }

    #[test]
    fn a_movie_without_media_info_has_no_badges_and_unknown_ids_are_refused() {
        let lab = Lab::new();
        let bare = lab.add(Item::movie("Bare"));
        let set = lab.add(Item::set("A set"));
        let reader = lab.reader();

        let detail = reader.movie(bare).unwrap();
        assert!(detail.tech.is_empty());
        assert!(detail.videos.is_empty());
        assert!(detail.backdrop.is_none());

        for id in [ItemId(9999), set] {
            assert_eq!(reader.movie(id).unwrap_err().code, "not_found");
        }
        assert_eq!(parse_id("abc").unwrap_err().code, "bad_id");
        assert_eq!(parse_id(" 12 ").unwrap(), ItemId(12));
    }

    #[test]
    fn marking_watched_and_back_is_remembered() {
        let lab = Lab::new();
        let id = lab.dune();
        let reader = lab.reader();
        let nfo = Arc::new(cv_app::nfo::NfoService::new(
            lab.db.clone(),
            lab.dir.path().join("undo"),
        ));
        let watch = cv_app::curation::WatchService::new(lab.db.clone(), nfo);
        let settings = cv_app::nfo::NfoSettings::default();

        assert_eq!(
            watch.set_watched(&[id], true, &settings).unwrap().changed(),
            1
        );
        assert_eq!(
            watch.set_watched(&[id], true, &settings).unwrap().changed(),
            0,
            "already watched"
        );
        let watched = reader.movie(id).unwrap();
        assert!(watched.watched);
        assert_eq!(watched.play_count, 1);

        assert_eq!(
            watch
                .set_watched(&[id], false, &settings)
                .unwrap()
                .changed(),
            1
        );
        let unwatched = reader.movie(id).unwrap();
        assert!(!unwatched.watched);
        assert_eq!(unwatched.play_count, 0);
    }

    /// The chip a click on the badge adds to the library (what the UI builds from a badge).
    fn facet_of(badge: &TechBadgeDto) -> crate::library_facets::LibraryFacetDto {
        use crate::library_facets::{FacetModeDto, FacetOpDto, LibraryFacetDto};
        LibraryFacetDto {
            id: badge.facet.clone(),
            negated: false,
            mode: FacetModeDto::Any,
            op: FacetOpDto::Ge,
            values: badge.values.clone(),
            number: badge.number,
            number2: None,
        }
    }

    #[test]
    fn every_badge_filters_the_library_to_titles_that_share_the_fact() {
        use crate::library::{LibraryKindDto, LibraryQueryDto, LibraryReader};

        let lab = Lab::new();
        let dune = lab.dune();
        let mut plain = Item::movie("Plain");
        let mut video = MediaFile::new(MediaFileType::Video, "plain", "plain.mkv");
        video.video_codec = "H.264".into();
        video.width = 1280;
        video.height = 720;
        video.video_bitrate_kbps = 4_000;
        video.aspect_ratio = 1.78;
        video.container_format = "Matroska".into();
        video.audio_streams = vec![AudioStream {
            codec: "AAC".into(),
            channels: 2,
            ..AudioStream::default()
        }];
        plain.core.media_files.push(video);
        if let ItemDetail::Movie(m) = &mut plain.detail {
            m.media_source = MediaSource::Dvd;
        }
        let plain = lab.add(plain);

        let detail = lab.reader().movie(dune).unwrap();
        assert_eq!(detail.tech.len(), 8, "{:?}", detail.tech);
        let library = LibraryReader::new(lab.db.clone(), lab.sources.clone(), None);
        for badge in &detail.tech {
            let query = LibraryQueryDto {
                kind: LibraryKindDto::Movies,
                sort: Vec::new(),
                search: String::new(),
                filters: Vec::new(),
                facets: vec![facet_of(badge)],
                rating_source: None,
            };
            let ids: Vec<String> = library
                .window(&query, 0, 50)
                .unwrap()
                .items
                .into_iter()
                .map(|card| card.id)
                .collect();
            assert_eq!(
                ids,
                [dune.0.to_string()],
                "the {} badge ({}) shows the movies sharing it, not {}",
                badge.label,
                badge.facet,
                plain.0
            );
        }
    }

    #[test]
    fn bitrates_read_naturally() {
        assert_eq!(bitrate_label(46_200), "46 Mb/s");
        assert_eq!(bitrate_label(8_400), "8.4 Mb/s");
        assert_eq!(bitrate_label(640), "640 kb/s");
    }
}

//! The filter catalogue: every filter id with its label, group and kind, and which ids exist
//! for movies, shows and movie sets.

use cv_domain::ItemKind;
use serde::{Deserialize, Serialize};

use crate::error::{DbError, DbResult};

/// How a filter takes its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    /// On or off; negation turns it round.
    Flag,
    /// Values present in the library, combined with ANY or ALL.
    Multi,
    /// Case-insensitive "contains" terms, combined with ANY or ALL.
    Text,
    /// A number compared with `<`, `<=`, `=`, `>=`, `>` or between.
    Number,
    /// An ISO date compared the same way.
    Date,
}

/// The tab of the filter picker a filter belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterGroup {
    Details,
    Metadata,
    Video,
    Audio,
    Subtitles,
    Artwork,
    Files,
    Ids,
    /// Ratings per source and the Top 250 rank.
    Ratings,
}

/// What a catalogue entry is, as the filter picker lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FilterDef {
    pub id: FilterId,
    pub label: &'static str,
    pub group: FilterGroup,
    pub kind: FilterKind,
    /// The unit of a number filter (`min`, `kbps`, `MiB`, `px`), if it has one.
    pub unit: Option<&'static str>,
}

macro_rules! catalogue_ids {
    ($( $variant:ident => ($label:literal, $group:ident, $kind:ident, $unit:expr) ),+ $(,)?) => {
        /// A filter of the catalogue. The same id can exist for several libraries (a genre filter
        /// for movies and shows); [`catalogue`] lists the ids of one library.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum FilterId {
            $($variant),+
        }

        impl FilterId {
            /// Every id, in declaration order.
            pub const ALL: &'static [FilterId] = &[$(FilterId::$variant),+];

            /// The label, group and kind of the filter.
            pub fn def(self) -> FilterDef {
                match self {
                    $(FilterId::$variant => FilterDef {
                        id: self,
                        label: $label,
                        group: FilterGroup::$group,
                        kind: FilterKind::$kind,
                        unit: $unit,
                    }),+
                }
            }
        }
    };
}

catalogue_ids! {
    // ---- shared by movies and shows -----------------------------------------------------
    AllInOne => ("Any text", Details, Text, None),
    DataSource => ("Data source", Details, Multi, None),
    Watched => ("Watched", Details, Flag, None),
    Locked => ("Locked", Details, Flag, None),
    DateAdded => ("Date added", Details, Date, None),
    Year => ("Year", Details, Number, None),
    Decade => ("Decade", Details, Multi, None),
    Genre => ("Genre", Details, Multi, None),
    Certification => ("Certification", Details, Multi, None),
    Cast => ("Cast and crew", Details, Text, None),
    Country => ("Country", Details, Text, None),
    Edition => ("Edition", Details, Multi, None),
    Note => ("Note", Details, Text, None),
    MediaSource => ("Release", Details, Multi, None),
    Runtime => ("Runtime", Details, Number, Some("min")),
    ImdbId => ("IMDb ID", Ids, Text, None),
    TmdbId => ("TMDB ID", Ids, Text, None),
    VideoFormat => ("Resolution", Video, Multi, None),
    VideoCodec => ("Video codec", Video, Multi, None),
    VideoBitrate => ("Video bitrate", Video, Number, Some("kbps")),
    VideoBitdepth => ("Bit depth", Video, Number, Some("bit")),
    VideoContainer => ("Container", Video, Multi, None),
    AspectRatio => ("Aspect ratio", Video, Multi, None),
    FrameRate => ("Frame rate", Video, Multi, Some("fps")),
    HdrFormat => ("HDR format", Video, Multi, None),
    AudioCodec => ("Audio codec", Audio, Multi, None),
    AudioChannel => ("Audio channels", Audio, Multi, None),
    AudioStreamCount => ("Audio streams", Audio, Number, None),
    AudioLanguage => ("Audio language", Audio, Multi, None),
    AudioTitle => ("Audio title", Audio, Multi, None),
    PosterSize => ("Poster height", Artwork, Number, Some("px")),
    FanartSize => ("Fan art width", Artwork, Number, Some("px")),
    BannerSize => ("Banner width", Artwork, Number, Some("px")),
    ThumbSize => ("Thumb width", Artwork, Number, Some("px")),
    ClearLogoSize => ("Clear logo width", Artwork, Number, Some("px")),
    ClearArtSize => ("Clear art width", Artwork, Number, Some("px")),
    DiscArtSize => ("Disc art width", Artwork, Number, Some("px")),
    KeyArtSize => ("Key art height", Artwork, Number, Some("px")),
    MissingArtwork => ("Missing artwork", Artwork, Multi, None),
    MediaFiles => ("Has file type", Files, Multi, None),
    Filename => ("Any file name", Files, Text, None),
    VideoFilename => ("Video file name", Files, Text, None),
    Path => ("Path", Files, Text, None),
    VideoFilesize => ("Video file size", Files, Number, Some("MiB")),
    SubtitleCount => ("Subtitle count", Subtitles, Number, None),
    SubtitleLanguage => ("Subtitle language", Subtitles, Multi, None),
    SubtitleFormat => ("Subtitle format", Subtitles, Multi, None),
    SubtitleType => ("Subtitle type", Subtitles, Multi, None),
    SubtitleTitle => ("Subtitle title", Subtitles, Multi, None),
    MissingMetadata => ("Missing metadata", Metadata, Multi, None),
    MissingSubtitles => ("Missing subtitles", Subtitles, Flag, None),
    // ---- movies only --------------------------------------------------------------------
    New => ("New", Details, Flag, None),
    Duplicate => ("Duplicate", Details, Flag, None),
    InMovieSet => ("In a collection", Details, Flag, None),
    ReleaseDate => ("Release date", Details, Date, None),
    Language => ("Spoken language", Details, Text, None),
    ProductionCompany => ("Production company", Details, Text, None),
    TraktId => ("Trakt ID", Ids, Text, None),
    Video3d => ("3D format", Video, Multi, None),
    VideoCrc32 => ("Has CRC32", Video, Flag, None),
    VideoExtras => ("Has extras", Files, Flag, None),
    TrailerFormat => ("Trailer quality", Details, Multi, None),
    DifferentRuntime => ("Runtime differs from file", Metadata, Number, Some("min")),
    EditionLost => ("Edition only in the file name", Metadata, Flag, None),
    // ---- shows only ---------------------------------------------------------------------
    NewEpisodes => ("New episodes", Details, Flag, None),
    DuplicateEpisodes => ("Duplicate episodes", Details, Flag, None),
    Status => ("Status", Details, Multi, None),
    Empty => ("No episodes", Details, Flag, None),
    FirstAired => ("First aired", Details, Date, None),
    Studio => ("Studio", Details, Multi, None),
    SeasonNote => ("Season note", Details, Text, None),
    EpisodeNote => ("Episode note", Details, Text, None),
    TvdbId => ("TVDB ID", Ids, Text, None),
    EpisodeImdbId => ("Episode IMDb ID", Ids, Text, None),
    EpisodeTmdbId => ("Episode TMDB ID", Ids, Text, None),
    EpisodeTvdbId => ("Episode TVDB ID", Ids, Text, None),
    EpisodeCount => ("Episode count", Details, Number, None),
    SeasonCount => ("Season count", Details, Number, None),
    EpisodeCrc32 => ("Episode has CRC32", Video, Flag, None),
    EpisodePath => ("Episode path", Files, Text, None),
    UncategorizedEpisodes => ("Uncategorised episodes", Details, Flag, None),
    MissingEpisodes => ("Missing episodes", Details, Flag, None),
    // ---- movie sets only ----------------------------------------------------------------
    SetMissingMovies => ("Missing movies", Details, Flag, None),
    SetNewMovies => ("New movies", Details, Flag, None),
    SetMultipleMovies => ("More than one movie", Details, Flag, None),
    // ---- ratings, movies and shows (per source, in the source's own units) ---------------
    Top250Rank => ("IMDb Top 250 rank", Ratings, Number, None),
    RatingImdb => ("IMDb rating", Ratings, Number, None),
    RatingTmdb => ("TMDB rating", Ratings, Number, None),
    RatingTrakt => ("Trakt rating", Ratings, Number, None),
    RatingTvdb => ("TVDB rating", Ratings, Number, None),
    RatingMetacritic => ("Metacritic score", Ratings, Number, Some("/100")),
    RatingRtCritics => ("Rotten Tomatoes critics", Ratings, Number, Some("%")),
    RatingRtAudience => ("Rotten Tomatoes audience", Ratings, Number, Some("%")),
    RatingLetterboxd => ("Letterboxd rating", Ratings, Number, Some("/5")),
    RatingMyAnimeList => ("MyAnimeList rating", Ratings, Number, None),
    RatingRogerEbert => ("Roger Ebert rating", Ratings, Number, Some("/4")),
    RatingUser => ("My rating", Ratings, Number, None),
}

use FilterId as F;

/// The 64 movie filters.
const MOVIE: [FilterId; 63] = [
    F::New,
    F::DataSource,
    F::Watched,
    F::Locked,
    F::DateAdded,
    F::Duplicate,
    F::InMovieSet,
    F::AllInOne,
    F::Year,
    F::ReleaseDate,
    F::Decade,
    F::Genre,
    F::Certification,
    F::Cast,
    F::Country,
    F::Language,
    F::ProductionCompany,
    F::Edition,
    F::Note,
    F::ImdbId,
    F::TmdbId,
    F::TraktId,
    F::VideoFormat,
    F::VideoCodec,
    F::VideoBitrate,
    F::VideoBitdepth,
    F::VideoContainer,
    F::AspectRatio,
    F::FrameRate,
    F::Video3d,
    F::VideoCrc32,
    F::HdrFormat,
    F::AudioCodec,
    F::AudioChannel,
    F::AudioStreamCount,
    F::AudioLanguage,
    F::AudioTitle,
    F::PosterSize,
    F::FanartSize,
    F::BannerSize,
    F::ThumbSize,
    F::ClearLogoSize,
    F::ClearArtSize,
    F::DiscArtSize,
    F::KeyArtSize,
    F::MissingArtwork,
    F::MediaSource,
    F::MediaFiles,
    F::Filename,
    F::VideoFilename,
    F::Path,
    F::VideoFilesize,
    F::SubtitleCount,
    F::SubtitleLanguage,
    F::SubtitleFormat,
    F::SubtitleType,
    F::SubtitleTitle,
    F::VideoExtras,
    F::TrailerFormat,
    F::Runtime,
    F::DifferentRuntime,
    F::MissingMetadata,
    F::MissingSubtitles,
];

/// The 71 TV show filters.
const SHOW: [FilterId; 69] = [
    F::NewEpisodes,
    F::DuplicateEpisodes,
    F::Status,
    F::Empty,
    F::FirstAired,
    F::Studio,
    F::SeasonNote,
    F::EpisodeNote,
    F::TvdbId,
    F::EpisodeImdbId,
    F::EpisodeTmdbId,
    F::EpisodeTvdbId,
    F::EpisodeCount,
    F::SeasonCount,
    F::EpisodeCrc32,
    F::EpisodePath,
    F::SubtitleCount,
    F::UncategorizedEpisodes,
    F::MissingEpisodes,
    F::Edition,
    F::AudioStreamCount,
    F::AllInOne,
    F::AspectRatio,
    F::AudioChannel,
    F::AudioCodec,
    F::AudioLanguage,
    F::AudioTitle,
    F::BannerSize,
    F::ClearArtSize,
    F::ClearLogoSize,
    F::DiscArtSize,
    F::FanartSize,
    F::KeyArtSize,
    F::PosterSize,
    F::ThumbSize,
    F::Cast,
    F::Certification,
    F::Country,
    F::DataSource,
    F::DateAdded,
    F::Decade,
    F::Filename,
    F::FrameRate,
    F::Genre,
    F::HdrFormat,
    F::ImdbId,
    F::Locked,
    F::MediaFiles,
    F::MediaSource,
    F::MissingArtwork,
    F::MissingMetadata,
    F::MissingSubtitles,
    F::Note,
    F::Path,
    F::Runtime,
    F::SubtitleFormat,
    F::SubtitleLanguage,
    F::SubtitleTitle,
    F::SubtitleType,
    F::TmdbId,
    F::VideoBitdepth,
    F::VideoBitrate,
    F::VideoCodec,
    F::VideoContainer,
    F::VideoFilename,
    F::VideoFilesize,
    F::VideoFormat,
    F::Watched,
    F::Year,
];

/// The 4 movie-set filters.
const SET: [FilterId; 4] = [
    F::DataSource,
    F::SetMissingMovies,
    F::SetNewMovies,
    F::SetMultipleMovies,
];

/// The rating filters of movies and shows, after the 64 or 71 filters of the base catalogue.
/// Each compares in the units its source reports (Letterboxd out of 5, Rotten Tomatoes in
/// percent, ...), so "Letterboxd >= 4.0" and "RT critics >= 90" read as they are written.
pub(crate) const RATING_FILTERS: [FilterId; 12] = [
    F::Top250Rank,
    F::RatingImdb,
    F::RatingTmdb,
    F::RatingTrakt,
    F::RatingTvdb,
    F::RatingMetacritic,
    F::RatingRtCritics,
    F::RatingRtAudience,
    F::RatingLetterboxd,
    F::RatingMyAnimeList,
    F::RatingRogerEbert,
    F::RatingUser,
];

/// CineVault's own movie filters, after the rating filters: "Edition only in the file name"
/// finds movies whose original video name names an edition (`Mimic - Director's Cut (1997)`)
/// while the movie has none, the titles a rename may have stripped of their edition.
pub(crate) const MOVIE_EXTRA_FILTERS: [FilterId; 1] = [F::EditionLost];

/// The libraries that have a filter catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Scope {
    Movie,
    Show,
    Set,
}

impl Scope {
    pub(crate) fn of(kind: ItemKind) -> DbResult<Self> {
        match kind {
            ItemKind::Movie => Ok(Self::Movie),
            ItemKind::Show => Ok(Self::Show),
            ItemKind::Set => Ok(Self::Set),
            ItemKind::Season | ItemKind::Episode => Err(DbError::InvalidFilter(format!(
                "{kind:?} items have no filter catalogue"
            ))),
        }
    }

    fn ids(self) -> &'static [FilterId] {
        match self {
            Self::Movie => &MOVIE,
            Self::Show => &SHOW,
            Self::Set => &SET,
        }
    }

    /// The rating filters this library has after its base filters.
    fn rating_ids(self) -> &'static [FilterId] {
        match self {
            Self::Movie | Self::Show => &RATING_FILTERS,
            Self::Set => &[],
        }
    }

    /// CineVault's own filters of this library, after the rating filters.
    fn extra_ids(self) -> &'static [FilterId] {
        match self {
            Self::Movie => &MOVIE_EXTRA_FILTERS,
            Self::Show | Self::Set => &[],
        }
    }

    /// Whether the filter exists for this library.
    pub(crate) fn has(self, id: FilterId) -> bool {
        self.ids().contains(&id)
            || self.rating_ids().contains(&id)
            || self.extra_ids().contains(&id)
    }
}

/// The filters of the library of `kind` (movies, shows or sets), in catalogue order: the base
/// filters (64, 71 and 4), then the rating filters of movies and shows, then CineVault's own.
pub fn catalogue(kind: ItemKind) -> DbResult<Vec<FilterDef>> {
    let scope = Scope::of(kind)?;
    Ok(scope
        .ids()
        .iter()
        .chain(scope.rating_ids())
        .chain(scope.extra_ids())
        .map(|id| id.def())
        .collect())
}

//! The plain models and options a TV show, season and episode NFO is written from.
//!
//! Like the movie model, these are not database items: everything the writers need, already
//! resolved. A caller builds them from a show, its seasons and its episodes, applies locks and
//! carried-over values, and the writers read nothing else. Timestamps are text in the local time
//! zone (`yyyy-MM-dd HH:mm:ss`); dates are [`CalendarDate`]s.
//!
//! Values that are derived from the episodes (the watched state of the show, the first air date
//! of a season, the date of the last episode) are computed by the caller, because only the
//! caller knows which episodes count (missing episodes, specials).

use cv_domain::{
    AiredStatus, CalendarDate, Certification, Credit, Edition, EpisodeGroup, EpisodeNumber, Genre,
    MediaSource, ProviderIds, Ratings, SubtitleStream, Trailer,
};

use crate::movie::MovieNfoOptions;
use crate::streams::VideoInfo;
use crate::xml::Element;

/// The format TV show, season and episode NFOs are written in.
///
/// The setting names are `KODI` (the default), `XBMC`, `EMBY`, `JELLYFIN`, `PLEX` and
/// `MEDIAPORTAL`. Plex writes the Kodi format. MediaPortal reads the older XBMC layout for the
/// show, and the Kodi layout for episodes (the fixtures show it so). Seasons are written in the
/// same layout in every format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum TvConnector {
    #[default]
    Kodi,
    Xbmc,
    Emby,
    Jellyfin,
    MediaPortal,
}

/// The layouts the writers implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TvFlavor {
    Kodi,
    Xbmc,
    Emby,
    Jellyfin,
}

impl TvConnector {
    /// Parses the setting name, ignoring case. `PLEX` gives the Kodi format.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_uppercase().as_str() {
            "KODI" | "PLEX" => Self::Kodi,
            "XBMC" => Self::Xbmc,
            "EMBY" => Self::Emby,
            "JELLYFIN" => Self::Jellyfin,
            "MEDIAPORTAL" => Self::MediaPortal,
            _ => return None,
        })
    }

    /// The name in the header comment.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Kodi => "Kodi",
            Self::Xbmc => "XBMC",
            Self::Emby => "Emby",
            Self::Jellyfin => "Jellyfin",
            Self::MediaPortal => "MediaPortal",
        }
    }

    pub(crate) fn show_flavor(self) -> TvFlavor {
        match self {
            Self::Kodi => TvFlavor::Kodi,
            Self::Xbmc | Self::MediaPortal => TvFlavor::Xbmc,
            Self::Emby => TvFlavor::Emby,
            Self::Jellyfin => TvFlavor::Jellyfin,
        }
    }

    pub(crate) fn episode_flavor(self) -> TvFlavor {
        match self {
            Self::Kodi | Self::MediaPortal => TvFlavor::Kodi,
            Self::Xbmc => TvFlavor::Xbmc,
            Self::Emby => TvFlavor::Emby,
            Self::Jellyfin => TvFlavor::Jellyfin,
        }
    }
}

/// The keys of the old-style `<episodeguide>` URLs. They are not part of the writer: the caller
/// passes the keys of the app's own provider accounts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EpisodeguideKeys {
    pub tvdb: String,
    pub tmdb: String,
}

/// The NFO options of TV shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TvNfoOptions {
    /// The options TV shares with movies: date added, single studio, lock data, trailer, file
    /// info, artwork URLs, language, certification style, outline and rating sources. The
    /// movie-only option "include external audio streams" has no effect on TV.
    pub common: MovieNfoOptions,
    /// Write `<episodeguide>`.
    pub write_episodeguide: bool,
    /// The JSON style of `<episodeguide>` (`{"tvdb":"..."}`); off writes the old URL style.
    pub new_episodeguide_style: bool,
    /// Write `<enddate>` of an ended show (Emby and Jellyfin).
    pub write_date_ended: bool,
    /// Also list the guest actors of the episodes in the show NFO (Kodi, Emby, Jellyfin).
    pub write_all_actors: bool,
    pub episodeguide_keys: EpisodeguideKeys,
    /// The language code of the old-style TMDB URL.
    pub scraper_language: String,
}

impl Default for TvNfoOptions {
    fn default() -> Self {
        Self {
            common: MovieNfoOptions {
                rating_sources: vec!["tvdb".to_owned(), "imdb".to_owned()],
                ..MovieNfoOptions::default()
            },
            write_episodeguide: true,
            new_episodeguide_style: true,
            write_date_ended: true,
            write_all_actors: false,
            episodeguide_keys: EpisodeguideKeys::default(),
            scraper_language: "en".to_owned(),
        }
    }
}

/// Remote artwork URLs of a show. An empty URL is not written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShowArtwork {
    pub poster: String,
    pub banner: String,
    pub clearart: String,
    pub clearlogo: String,
    /// Written as `landscape`.
    pub thumb: String,
    pub keyart: String,
    pub squareart: String,
    pub logo: String,
    pub characterart: String,
    /// Written as `discart`.
    pub disc: String,
    /// The main fanart; the extra fanart follows it.
    pub fanart: String,
    pub extra_fanarts: Vec<String>,
}

/// Remote artwork URLs of a season.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeasonArtwork {
    pub poster: String,
    pub banner: String,
    pub thumb: String,
    pub fanart: String,
}

/// A season: what the show NFO lists about it and what the season NFO holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeasonNfo {
    /// The season number; `0` is the specials season.
    pub number: u32,
    /// The season's own name (`namedseason` in the show NFO); empty uses "Season N".
    pub title: String,
    pub plot: String,
    /// The user's own note.
    pub note: String,
    /// The earliest first-aired date of its episodes.
    pub first_aired: Option<CalendarDate>,
    pub artwork: SeasonArtwork,
    /// Elements of the old season NFO that CineVault does not manage.
    pub unknown_elements: Vec<Element>,
}

/// Values the previous show NFO held that a rewrite keeps, unless "write clean NFO" is on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShowCarried {
    /// Used only when the show has no outline and none is created from the plot.
    pub outline: String,
    /// The old `<episodeguide>`, used by the old style when no provider ID is known.
    pub episodeguide: Option<Element>,
    /// The play count of the old NFO, used for a watched show.
    pub play_count: u32,
}

/// The air date of one known episode (including missing ones), for `<enddate>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AirDate {
    pub season: u32,
    pub date: CalendarDate,
}

/// Everything a `tvshow.nfo` is written from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShowNfo {
    pub title: String,
    pub original_title: String,
    pub sort_title: String,
    pub english_title: String,
    /// The original language as a language code (`ko`), empty when unknown.
    pub original_language: String,
    /// The year the show ended, `0` when it runs on or is unknown.
    pub year_end: u32,
    /// First-aired year, `0` when unknown.
    pub year: u32,
    pub plot: String,
    pub outline: String,
    pub tagline: String,
    pub runtime_minutes: u32,
    /// Rank in the top 250, `0` when unranked.
    pub top250: u32,
    pub ids: ProviderIds,
    pub ratings: Ratings,
    pub certification: Certification,
    pub genres: Vec<Genre>,
    /// One entry per country (see [`split_show_list`]).
    pub countries: Vec<String>,
    /// One entry per studio (see [`split_show_list`]).
    pub studios: Vec<String>,
    pub tags: Vec<String>,
    /// Cast and crew in order. Actors are the cast; directors, writers and producers are the
    /// crew.
    pub credits: Vec<Credit>,
    /// Guest actors of the episodes, written after the cast with "write all actors".
    pub guest_actors: Vec<Credit>,
    pub trailers: Vec<Trailer>,
    pub artwork: ShowArtwork,
    pub first_aired: Option<CalendarDate>,
    pub status: AiredStatus,
    /// All its episodes are watched.
    pub watched: bool,
    /// When the show was added, local `yyyy-MM-dd HH:mm:ss`.
    pub date_added: Option<String>,
    /// The earliest creation time of the episodes' video files.
    pub file_created: Option<String>,
    /// The earliest modification time of the episodes' video files.
    pub file_modified: Option<String>,
    /// The user's own note.
    pub note: String,
    /// The provider that scraped the show last (`tvdb`, `tmdb`); picks the old-style
    /// `<episodeguide>` URL.
    pub last_scraper_id: String,
    /// The episode orderings of the show; only named ones are written.
    pub episode_groups: Vec<EpisodeGroup>,
    /// The ordering the show uses; marked `active` in the list.
    pub active_group: EpisodeGroup,
    /// The seasons that have episodes, in order.
    pub seasons: Vec<SeasonNfo>,
    /// The air dates of all known episodes, missing ones included.
    pub episode_air_dates: Vec<AirDate>,
    /// Names of locked fields, written to the lock element.
    pub locked_fields: Vec<String>,
    pub carried: ShowCarried,
    /// Elements of the old NFO that CineVault does not manage, written back unchanged.
    pub unknown_elements: Vec<Element>,
}

/// Values the previous episode NFO held that a rewrite keeps, unless "write clean NFO" is on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EpisodeCarried {
    pub epbookmark: String,
    pub code: String,
    /// Used only when the episode has no outline and none is created from the plot.
    pub outline: String,
    /// The old `<trailer>` text.
    pub trailer: String,
    /// The play count of the old NFO; the higher one wins.
    pub play_count: u32,
    /// The last played time of the old NFO; the later one wins.
    pub last_played: Option<String>,
}

/// Everything one `<episodedetails>` is written from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EpisodeNfo {
    pub title: String,
    pub original_title: String,
    pub english_title: String,
    /// The title of the show.
    pub show_title: String,
    /// The season and episode number in the ordering the show uses.
    pub season: u32,
    pub episode: u32,
    /// The display position (`displayseason`, `displayepisode`), when the episode has one.
    pub display: Option<(u32, u32)>,
    /// The numbers in every ordering; only entries whose ordering has a name are written.
    pub numbers: Vec<EpisodeNumber>,
    pub ids: ProviderIds,
    pub ratings: Ratings,
    pub plot: String,
    pub outline: String,
    /// The runtime of the episode when the video files give none.
    pub runtime_minutes: u32,
    /// The episode picture URL.
    pub thumb: String,
    pub certification: Certification,
    pub first_aired: Option<CalendarDate>,
    pub watched: bool,
    pub play_count: u32,
    /// Last played, local `yyyy-MM-dd HH:mm:ss`.
    pub last_played: Option<String>,
    /// One entry per studio (see [`split_episode_list`]).
    pub studios: Vec<String>,
    pub tags: Vec<String>,
    /// Guest actors, directors and writers. Guest credits get a `<type>GuestStar</type>`.
    pub credits: Vec<Credit>,
    pub date_added: Option<String>,
    /// The main video file; its duration is the duration of the whole file.
    pub video: VideoInfo,
    /// Subtitle streams from files next to the video.
    pub external_subtitles: Vec<SubtitleStream>,
    pub media_source: MediaSource,
    pub edition: Edition,
    pub original_filename: String,
    /// The user's own note.
    pub note: String,
    /// Checksum of the video file (8 uppercase hex digits), written when known.
    pub crc32: Option<String>,
    pub carried: EpisodeCarried,
    /// Elements of the old NFO that CineVault does not manage, written back unchanged.
    pub unknown_elements: Vec<Element>,
}

/// Splits the countries or studios of a show: at `,` and `/`, trimmed, without empty parts.
pub fn split_show_list(text: &str) -> Vec<String> {
    split_at(text, &[',', '/'])
}

/// Splits the studios of an episode: at `;`, `,`, `/` and `|`, trimmed, without empty parts.
pub fn split_episode_list(text: &str) -> Vec<String> {
    split_at(text, &[';', ',', '/', '|'])
}

fn split_at(text: &str, separators: &[char]) -> Vec<String> {
    text.split(separators)
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

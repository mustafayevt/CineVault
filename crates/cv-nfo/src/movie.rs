//! The plain model a movie NFO is written from, and the options that shape the output.
//!
//! The model is deliberately not the database item: it is everything the writer needs, already
//! resolved. Callers build it from an item (and the files next to it), apply locks and carried-over
//! values before writing, and the writer never reads anything else. Timestamps are text in the
//! local time zone (`yyyy-MM-dd HH:mm:ss`) so that the writer has no time-zone logic.

use std::collections::BTreeMap;

use cv_domain::{
    AudioStream, CalendarDate, Certification, Credit, Edition, Genre, MediaFileType, MediaSource,
    ProviderIds, Ratings, SubtitleStream, Trailer,
};

use crate::certification::CertificationStyle;
use crate::streams::VideoInfo;
use crate::xml::Element;

/// The collection a movie belongs to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MovieSetRef {
    pub title: String,
    pub overview: String,
    pub tmdb_id: Option<u64>,
}

/// Remote artwork URLs written as `<thumb>` and `<fanart>`. An empty URL is not written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MovieArtwork {
    pub poster: String,
    pub banner: String,
    pub clearart: String,
    pub clearlogo: String,
    pub disc: String,
    /// Written as `landscape`.
    pub thumb: String,
    pub keyart: String,
    pub squareart: String,
    pub logo: String,
    /// The main fanart; the extra fanart follows it.
    pub fanart: String,
    pub extra_fanarts: Vec<String>,
    /// Extra thumbnails, written in CineVault's own `<extrathumbs>` block.
    pub extra_thumbs: Vec<String>,
}

impl MovieArtwork {
    /// Builds the artwork block from an item's artwork URLs and extra fanart.
    pub fn from_urls(urls: &BTreeMap<MediaFileType, String>, extra_fanarts: &[String]) -> Self {
        let url = |kind| urls.get(&kind).cloned().unwrap_or_default();
        Self {
            poster: url(MediaFileType::Poster),
            banner: url(MediaFileType::Banner),
            clearart: url(MediaFileType::Clearart),
            clearlogo: url(MediaFileType::Clearlogo),
            disc: url(MediaFileType::Disc),
            thumb: url(MediaFileType::Thumb),
            keyart: url(MediaFileType::Keyart),
            squareart: url(MediaFileType::Squareart),
            logo: String::new(),
            fanart: url(MediaFileType::Fanart),
            extra_fanarts: extra_fanarts.to_vec(),
            extra_thumbs: Vec::new(),
        }
    }
}

/// Values the previous NFO held that a rewrite keeps (play position, status, code, outline, play
/// count and last played), unless "write clean NFO" is on. [`crate::preserve::apply_old_movie_nfo`]
/// fills them from the old file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CarriedValues {
    pub epbookmark: String,
    pub status: String,
    pub code: String,
    /// Used only when the movie has no outline and none is created from the plot.
    pub outline: String,
    /// The play count of the old NFO; the higher one wins, and a watched movie has at least 1.
    pub play_count: u32,
    /// The last played time of the old NFO (`yyyy-MM-dd HH:mm:ss`); the later one wins.
    pub last_played: Option<String>,
}

/// Everything a movie NFO is written from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MovieNfo {
    pub title: String,
    pub original_title: String,
    pub sort_title: String,
    pub english_title: String,
    /// The original language of the film as a language code (`ja`), empty when unknown.
    pub original_language: String,
    /// The video is 3D (from the file name or the media info).
    pub video_in_3d: bool,
    /// Release year, `0` when unknown.
    pub year: u32,
    pub plot: String,
    pub outline: String,
    pub tagline: String,
    pub runtime_minutes: u32,
    /// Rank in the top 250, `0` when unranked.
    pub top250: u32,
    /// Provider IDs; `imdb` and `tmdb` have their own elements.
    pub ids: ProviderIds,
    /// Ratings by source; the `user` rating is the user rating.
    pub ratings: Ratings,
    pub certification: Certification,
    pub genres: Vec<Genre>,
    /// One entry per country.
    pub countries: Vec<String>,
    /// One entry per studio.
    pub studios: Vec<String>,
    /// The countries as one text, as scraped or typed. The legacy MediaPortal NFO writes it
    /// unchanged; when it is empty the list is joined instead.
    pub country_text: String,
    /// The studios as one text; see [`MovieNfo::country_text`].
    pub studio_text: String,
    /// Language codes or names.
    pub spoken_languages: Vec<String>,
    pub tags: Vec<String>,
    /// Cast and crew in order; actors, directors, writers and producers get their own elements.
    pub credits: Vec<Credit>,
    pub trailers: Vec<Trailer>,
    /// Titles of linked TV shows.
    pub show_links: Vec<String>,
    pub movie_set: Option<MovieSetRef>,
    /// The other `<set>` entries of the old NFO. Only the Emby writer lists them, after the
    /// movie's own set; [`crate::preserve::apply_old_movie_nfo`] fills them from the old file.
    pub other_sets: Vec<MovieSetRef>,
    pub artwork: MovieArtwork,
    pub release_date: Option<CalendarDate>,
    /// When the movie was added, local `yyyy-MM-dd HH:mm:ss`.
    pub date_added: Option<String>,
    pub watched: bool,
    pub play_count: u32,
    /// Last played, local `yyyy-MM-dd HH:mm:ss`; written only for a watched movie.
    pub last_played: Option<String>,
    pub media_source: MediaSource,
    pub edition: Edition,
    pub original_filename: String,
    /// The user's own note.
    pub note: String,
    /// Checksum of the video file (8 uppercase hex digits), written when known.
    pub crc32: Option<String>,
    /// The main video file.
    pub video: VideoInfo,
    /// Audio streams from files next to the video, written when the option asks for them.
    pub external_audio: Vec<AudioStream>,
    /// Subtitle streams from files next to the video.
    pub external_subtitles: Vec<SubtitleStream>,
    /// A disc structure (`VIDEO_TS`, `BDMV`).
    pub is_disc: bool,
    /// Names of locked fields, written to the lock element. Empty when nothing is locked.
    pub locked_fields: Vec<String>,
    pub carried: CarriedValues,
    /// Elements of the old NFO that CineVault does not manage, written back unchanged.
    pub unknown_elements: Vec<Element>,
}

/// Which date `<dateadded>` holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum DateAddedField {
    /// When the movie was added to the library.
    #[default]
    DateAdded,
    /// When the video file was created.
    FileCreationDate,
    /// When the video file was last modified.
    FileLastModifiedDate,
    /// The release date (at midnight).
    ReleaseDate,
}

impl DateAddedField {
    /// Parses the setting name (`DATE_ADDED`, `FILE_CREATION_DATE`, `FILE_LAST_MODIFIED_DATE`,
    /// `RELEASE_DATE`), ignoring case.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_uppercase().as_str() {
            "DATE_ADDED" => Self::DateAdded,
            "FILE_CREATION_DATE" => Self::FileCreationDate,
            "FILE_LAST_MODIFIED_DATE" => Self::FileLastModifiedDate,
            "RELEASE_DATE" => Self::ReleaseDate,
            _ => return None,
        })
    }
}

/// The NFO options that change what is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieNfoOptions {
    /// Write `<dateadded>`.
    pub write_date_added: bool,
    pub date_added_field: DateAddedField,
    /// Write only the first studio.
    pub single_studio: bool,
    /// Write `<lockdata>true</lockdata>`.
    pub lockdata: bool,
    /// Write `<trailer>`.
    pub trailer: bool,
    /// Write `<fileinfo>`.
    pub fileinfo: bool,
    /// Write `<thumb>`, `<fanart>` and the actor and crew pictures.
    pub artwork_urls: bool,
    /// Language of genre and spoken-language names, for example `en`, `de`, `fr`.
    pub language: String,
    pub certification_style: CertificationStyle,
    /// Create `<outline>` from the plot when the movie has none.
    pub create_outline: bool,
    /// An created outline is the plot's first sentence (when it has at least 20 characters)
    /// instead of the whole plot.
    pub outline_first_sentence: bool,
    /// Also list audio streams of files next to the video.
    pub include_external_audio: bool,
    /// Rating sources in order of preference; the first one the movie has is the default rating.
    pub rating_sources: Vec<String>,
    /// "Write clean NFO": the old NFO is not read, so nothing is carried over from it and its
    /// unmanaged elements are dropped. See [`crate::preserve`].
    pub clean_nfo: bool,
    /// Write CineVault's additions to the standard Kodi layout (the full stream details, the
    /// original language, extra thumbnails, the 3D flag).
    /// They are extra elements media centres ignore; the compatibility fixtures are written
    /// without them.
    pub cv_extensions: bool,
    /// The generator name and version written in the header comment.
    pub generator: String,
    /// When the file is created, local `yyyy-MM-dd HH:mm:ss`; empty leaves it out of the comment.
    pub created_on: String,
}

impl Default for MovieNfoOptions {
    fn default() -> Self {
        Self {
            write_date_added: true,
            date_added_field: DateAddedField::DateAdded,
            single_studio: false,
            lockdata: false,
            trailer: true,
            fileinfo: true,
            artwork_urls: true,
            language: "en".to_owned(),
            certification_style: CertificationStyle::Short,
            create_outline: false,
            outline_first_sentence: true,
            include_external_audio: false,
            rating_sources: vec!["imdb".to_owned(), "themoviedb".to_owned()],
            clean_nfo: false,
            cv_extensions: true,
            generator: format!("CineVault {}", env!("CARGO_PKG_VERSION")),
            created_on: String::new(),
        }
    }
}

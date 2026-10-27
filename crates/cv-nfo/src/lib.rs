//! NFO readers and writers for Kodi, Jellyfin, Emby and MediaPortal.
//!
//! Written NFOs are byte-exact against the compatibility fixtures in `tests/compat`: UTF-8 XML,
//! two-space indentation, CRLF line ends, fixed element order.
//!
//! * [`xml`]: the small element tree and the serializer that fixes the layout.
//! * [`movie`]: the plain model a movie NFO is written from, and the NFO options.
//! * [`kodi_movie`]: the Kodi movie writer, with the Emby and Jellyfin variants.
//! * [`legacy_movie`]: the XBMC and MediaPortal movie writers.
//! * [`connector`]: the movie formats and the dispatch to their writers.
//! * [`movie_set`]: the Kodi and Emby movie-set writers and the NFO locations of a set.
//! * [`naming`]: NFO file names for movies.
//! * [`tv`], [`tv_show`], [`tv_season`], [`tv_episode`], [`tv_naming`]: the TV show, season and
//!   episode models, writers and NFO file names.
//! * [`write`]: write-only-if-changed file output.
//! * [`ownership`], [`journal`]: which NFO files CineVault owns (foreign files are never touched) and
//!   the undoable journal every NFO change goes through.
//! * [`preserve`], [`tree`]: what a rewrite keeps from the old NFO (unmanaged elements, carried-over
//!   values) and the forgiving XML reader it uses.
//! * [`read`], [`read_movie`], [`read_tv`]: the tolerant readers for movie, set, show, season and
//!   episode NFOs (and NextPVR recordings).
//! * [`certification`], [`locale`], [`streams`], [`format`]: value formats the writers share.

pub mod certification;
pub mod connector;
pub mod format;
pub mod journal;
pub mod kodi_movie;
pub mod legacy_movie;
pub mod locale;
pub mod movie;
pub mod movie_set;
pub mod naming;
pub mod ownership;
pub mod preserve;
pub mod read;
pub mod read_movie;
pub mod read_tv;
pub mod streams;
pub mod tree;
pub mod tv;
mod tv_common;
pub mod tv_episode;
pub mod tv_naming;
pub mod tv_season;
pub mod tv_show;
pub mod write;
pub mod xml;

pub use certification::{CertificationStyle, format_certification, parse_certification};
pub use connector::{MovieConnector, movie_document_for, render_movie_nfo_for};
pub use journal::{Journal, JournalEntry, UndoReport, UndoStep, WriteOutcome};
pub use kodi_movie::{movie_document, render_movie_nfo};
pub use legacy_movie::MediaPortalStyle;
pub use movie::{
    CarriedValues, DateAddedField, MovieArtwork, MovieNfo, MovieNfoOptions, MovieSetRef,
};
pub use movie_set::{
    DEFAULT_NAME_REPLACEMENT, MovieSetConnector, MovieSetNfo, MovieSetNfoNaming,
    movie_set_document, movie_set_file_name, movie_set_file_name_with, movie_set_folder_name,
    movie_set_folder_name_with, movie_set_nfo_paths, movie_set_nfo_paths_for, render_movie_set_nfo,
};
pub use naming::{MovieNameContext, MovieNfoNaming, movie_nfo_file_names};
pub use ownership::{
    NfoOwnership, classify_nfo_bytes, classify_nfo_file, classify_nfo_text, is_managed_nfo_text,
    stale_nfos, written_by_cinevault,
};
pub use preserve::{
    apply_old_episode_nfo, apply_old_movie_nfo, apply_old_season_nfo, apply_old_show_nfo,
    read_existing_nfo,
};
pub use read::{
    DetectedIds, decode_nfo_bytes, detect_collection_id, detect_movie_ids, detect_show_ids,
};
pub use read_movie::{read_movie_nfo, read_movie_set_nfo};
pub use read_tv::{EpisodeRead, SeasonRead, read_episode_nfo, read_season_nfo, read_show_nfo};
pub use streams::VideoInfo;
pub use tree::parse_roots;
pub use tv::{
    AirDate, EpisodeCarried, EpisodeNfo, EpisodeguideKeys, SeasonArtwork, SeasonNfo, ShowArtwork,
    ShowCarried, ShowNfo, TvConnector, TvNfoOptions, split_episode_list, split_show_list,
};
pub use tv_episode::{episode_document, render_episode_nfo};
pub use tv_naming::{
    EpisodeNfoNaming, SeasonNfoNaming, ShowNfoNaming, episode_nfo_file_name, season_nfo_path,
    season_nfo_paths, show_nfo_file_names,
};
pub use tv_season::{render_season_nfo, season_document};
pub use tv_show::{render_show_nfo, show_document};
pub use write::{content_differs, write_nfo_if_changed};
pub use xml::{Document, Element, Node};

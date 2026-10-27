//! Downloading artwork into an item's folder.
//!
//! Spec: `artwork` (Download missing artwork, Extra fanart and extra thumbs, Actor images, Local
//! artwork is authoritative, Remote artwork URLs retained).
//!
//! # Pieces
//!
//! | Module | What it does |
//! |---|---|
//! | [`config`] | the artwork settings (`artwork.*` keys) |
//! | [`named`] | one image to the files of a name list: season artwork and episode thumbnails |
//! | [`fetch`] | getting the bytes of an image URL through the shared HTTP client |
//! | [`picture`] | checking that the bytes are an image, and shrinking extra thumbs |
//! | [`files`] | atomic writes, the trash and the variants of a file name |
//! | [`actors`] | the `.actors` folder |
//! | [`engine`] | [`ArtworkDownloader`]: download missing, re-download, place a chosen image |
//! | [`set`] | the artwork of a collection (movie set), in the movie folders or the collection folder |
//! | [`record`] | writing the outcome into the library database |
//!
//! # Data safety
//!
//! * An image that is already there, under any name of its type or with any extension, is never
//!   overwritten unless the caller asks for replacement ([`DownloadRequest::replace`]).
//! * What a replacement displaces, and every leftover (old extras, portraits of actors who left
//!   the cast), is moved to the trash folder of the data source. Nothing is deleted.
//! * Every file is written next to its name and renamed into place, and bytes that are not a
//!   picture (an error page served with status 200) are never written.
//! * A multi-movie folder gets only filename-based artwork, no extras and no `.actors`.

use std::path::{Path, PathBuf};

use crate::naming::FolderKind;

pub mod actors;
pub mod config;
pub mod engine;
pub mod fetch;
pub mod files;
pub mod named;
pub mod picture;
pub mod record;
pub mod set;

pub use config::{ArtworkConfig, DownloadOptions};
pub use engine::{
    ArtworkDownloader, DownloadReport, DownloadRequest, Failure, WrittenActor, WrittenImage,
    candidate_kind, picker_slots,
};
pub use fetch::{FetchError, HttpImageFetcher, ImageFetcher};
pub use named::{NameList, NamedImage, download_named_image};
pub use picture::{ImageError, ImageInfo, Picture};
pub use set::{SetArtworkReport, SetArtworkRequest, SetMovieFolder, download_set_artwork};

/// What kind of item the artwork belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subject {
    Movie,
    Show,
}

/// Where an item's artwork files go.
#[derive(Debug, Clone)]
pub struct ArtworkTarget {
    pub subject: Subject,
    /// The folder the artwork files sit in: the video's folder for a movie (the item folder for
    /// a disc structure), the show folder for a show.
    pub dir: PathBuf,
    /// The item folder; `.actors` goes here.
    pub root: PathBuf,
    /// The video file name without extension and stacking marker; empty for a show.
    pub base: String,
    pub folder: FolderKind,
    /// The data source the item lives in; its trash folder takes what is replaced.
    pub data_source: Option<PathBuf>,
}

impl ArtworkTarget {
    /// Extra fanart and extra thumbs are written to this folder.
    pub fn writes_extras(&self) -> bool {
        self.folder != FolderKind::MultiMovie
    }

    /// Actor portraits are written to this folder.
    pub fn writes_actors(&self) -> bool {
        self.folder != FolderKind::MultiMovie
    }

    /// Whether the folder exists. Nothing is ever written into a folder that is not there, so a
    /// moved or offline item cannot sprout a stray folder.
    pub fn exists(&self) -> bool {
        self.dir.is_dir()
    }
}

/// The artwork types of an item whose image file exists on disk. The caller passes this as
/// [`DownloadRequest::present`] so that a poster found by the scanner under any name counts.
pub fn present_artwork(
    item: &cv_domain::Item,
) -> std::collections::BTreeSet<cv_domain::MediaFileType> {
    item.core
        .media_files
        .iter()
        .filter(|f| f.kind.is_graphic() && Path::new(&f.directory).join(&f.filename).is_file())
        .map(|f| f.kind)
        .collect()
}

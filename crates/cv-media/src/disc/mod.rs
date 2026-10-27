//! Disc and ISO analysis: the main title of a DVD, Blu-ray or HD-DVD, read without mounting.
//!
//! * [`DiscFs`] is a read-only view of a disc's files, over a folder on disk ([`FolderFs`]) or
//!   a disc image ([`IsoImage`], a Rust ISO 9660 and UDF reader).
//! * [`find_main_title`] picks the main title from the disc's own structure files: the longest
//!   program chain in the DVD `.IFO` files ([`dvd`]) or the longest Blu-ray playlist
//!   ([`bluray`]). It reads only those small files, so it is cheap and needs no MediaInfo.
//! * [`analyze_disc`] and [`analyze_path`] then feed the title's stream file to MediaInfo's buffer
//!   interface ([`crate::mediainfo::MediaInfoLibrary::read_buffered`]) for the streams, and put
//!   the title's real running time in the result, because one stream file is only a part of a
//!   title.
//!
//! HD-DVD has no structure parser: the largest `.EVO` file is analysed and its duration is what
//! MediaInfo makes of it. When a DVD or Blu-ray structure cannot be read, the largest stream file
//! stands in for the title the same way (`MainTitle::duration_ms` is `None` then).

pub mod bluray;
pub mod dvd;
mod files;
mod iso;

use std::fs::File;
use std::path::Path;

pub use files::{DiscEntry, DiscFs, FolderFs, FsFile, is_image_path, read_file};
pub use iso::IsoImage;

use crate::mediainfo::{
    DEFAULT_BUFFER_LIMIT, MediaInfoError, MediaInfoLibrary, MediaSnapshot, StreamKind,
};

/// Which kind of disc a title belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscKind {
    Dvd,
    BluRay,
    HdDvd,
}

/// The main title of a disc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MainTitle {
    pub kind: DiscKind,
    /// Where the title was found, for logs and the UI: `00800.mpls`, `VTS_01_0.IFO (title 1)`.
    pub source: String,
    /// The title's playing time. `None` when it could not be read from the disc structure.
    pub duration_ms: Option<u64>,
    pub chapters: u32,
    /// The file MediaInfo reads the streams from (a path inside the disc).
    pub stream_file: String,
    /// Every file that makes up the title, in play order.
    pub files: Vec<String>,
}

/// What the analysis of one disc found.
#[derive(Clone, Debug)]
pub struct DiscAnalysis {
    pub title: MainTitle,
    /// The streams of the title with the container duration replaced by the title's.
    pub snapshot: MediaSnapshot,
}

/// Why a disc could not be analysed. Every message is ready to show.
#[derive(Debug, thiserror::Error)]
pub enum DiscError {
    #[error("No DVD, Blu-ray or HD-DVD structure (VIDEO_TS, BDMV or HVDVD_TS) was found.")]
    NoDisc,
    #[error("The disc structure could not be read: {0}")]
    Structure(String),
    #[error("The disc could not be read: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    MediaInfo(#[from] MediaInfoError),
}

impl DiscError {
    pub(crate) fn structure(message: impl Into<String>) -> Self {
        Self::Structure(message.into())
    }
}

/// A disc folder's usual name, in the order they are tried when a hybrid has several.
const DISC_FOLDERS: [(&str, DiscKind); 3] = [
    ("BDMV", DiscKind::BluRay),
    ("VIDEO_TS", DiscKind::Dvd),
    ("HVDVD_TS", DiscKind::HdDvd),
];

/// The files that mark a disc structure, which the scan stores as the title's video.
const DISC_MARKERS: [&str; 3] = ["index.bdmv", "video_ts.ifo", "hv000i01.ifo"];

/// The disc folder (`.../BDMV`, `.../VIDEO_TS`, `.../HVDVD_TS`) that `path` stands for when it is
/// the marker file of a disc structure (`BDMV/index.bdmv`, `VIDEO_TS/VIDEO_TS.IFO`,
/// `HVDVD_TS/HV000I01.IFO`). The marker itself holds no streams: the disc is read through its
/// main title instead ([`analyze_path`] on the folder). `None` for any other path.
#[must_use]
pub fn disc_folder_of(path: &Path) -> Option<&Path> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if !DISC_MARKERS.contains(&name.as_str()) {
        return None;
    }
    let folder = path.parent()?;
    let folder_name = folder.file_name()?.to_string_lossy();
    DISC_FOLDERS
        .iter()
        .any(|(disc, _)| folder_name.eq_ignore_ascii_case(disc))
        .then_some(folder)
}

/// Finds the main title of the disc in `fs`, from its structure files alone.
///
/// # Errors
/// [`DiscError::NoDisc`] when the root holds no disc folder; [`DiscError::Structure`] when a
/// folder is there but neither its structure files nor any stream file can be used.
pub fn find_main_title(fs: &dyn DiscFs) -> Result<MainTitle, DiscError> {
    let root = fs.list("")?;
    let (folder, kind) = DISC_FOLDERS
        .iter()
        .find_map(|(name, kind)| {
            root.iter()
                .find(|entry| entry.is_dir && entry.name.eq_ignore_ascii_case(name))
                .map(|entry| (entry.name.clone(), *kind))
        })
        .ok_or(DiscError::NoDisc)?;
    let structured = match kind {
        DiscKind::BluRay => bluray::find_main_title(fs, &folder),
        DiscKind::Dvd => dvd::find_main_title(fs, &folder),
        DiscKind::HdDvd => Err(DiscError::structure("HD-DVD has no structure parser")),
    };
    match structured {
        Ok(title) => Ok(title),
        Err(DiscError::Io(e)) => Err(DiscError::Io(e)),
        Err(problem) => largest_stream(fs, &folder, kind).ok_or(problem),
    }
}

/// The biggest stream file of the disc, for when the structure files cannot be used.
fn largest_stream(fs: &dyn DiscFs, folder: &str, kind: DiscKind) -> Option<MainTitle> {
    let (directory, extensions): (String, &[&str]) = match kind {
        DiscKind::BluRay => (files::join(folder, "STREAM"), &[".m2ts"]),
        DiscKind::Dvd => (folder.to_owned(), &[".vob"]),
        DiscKind::HdDvd => (folder.to_owned(), &[".evo"]),
    };
    let entries = fs.list(&directory).ok()?;
    let biggest = entries
        .iter()
        .filter(|entry| {
            let name = entry.name.to_ascii_lowercase();
            !entry.is_dir
                && extensions.iter().any(|ext| name.ends_with(ext))
                // The menu video of a DVD is not the feature.
                && !name.starts_with("video_ts")
        })
        .max_by_key(|entry| entry.size)?;
    let path = files::join(&directory, &biggest.name);
    Some(MainTitle {
        kind,
        source: biggest.name.clone(),
        duration_ms: None,
        chapters: 0,
        stream_file: path.clone(),
        files: vec![path],
    })
}

/// Analyses the disc in `fs`: the main title, and its streams from MediaInfo.
///
/// # Errors
/// As [`find_main_title`]; [`DiscError::MediaInfo`] when the stream file cannot be read.
pub fn analyze_disc(
    library: &MediaInfoLibrary,
    fs: &dyn DiscFs,
) -> Result<DiscAnalysis, DiscError> {
    let title = find_main_title(fs)?;
    let mut source = FsFile::open(fs, &title.stream_file)?;
    let mut snapshot =
        library.read_buffered(&title.stream_file, &mut source, DEFAULT_BUFFER_LIMIT)?;
    if let Some(duration) = title.duration_ms {
        set_duration(&mut snapshot, duration);
    }
    Ok(DiscAnalysis { title, snapshot })
}

/// Analyses a disc folder (the disc folder itself, such as `.../BDMV`, or the folder that holds
/// it) or an `.iso` image.
///
/// # Errors
/// As [`analyze_disc`], and [`DiscError::Io`] when the path cannot be opened.
pub fn analyze_path(library: &MediaInfoLibrary, path: &Path) -> Result<DiscAnalysis, DiscError> {
    // The stream and playlist files below the folder are open while this runs: a rename of the
    // folder waits for it (see `cv_fs::reading`).
    let _reading = cv_fs::reading(path);
    let fs = open_path(path)?;
    analyze_disc(library, fs.as_ref())
}

/// Opens a disc folder or an image as a [`DiscFs`].
///
/// # Errors
/// [`DiscError::Io`] when the path cannot be read or the image is not a readable ISO 9660 or UDF
/// image.
pub fn open_path(path: &Path) -> Result<Box<dyn DiscFs + Send + Sync>, DiscError> {
    if path.is_dir() {
        let is_disc_folder = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .is_some_and(|name| {
                DISC_FOLDERS
                    .iter()
                    .any(|(folder, _)| name.eq_ignore_ascii_case(folder))
            });
        let root = match path.parent() {
            Some(parent) if is_disc_folder => parent,
            _ => path,
        };
        return Ok(Box::new(FolderFs::new(root)));
    }
    Ok(Box::new(IsoImage::open(File::open(path)?)?))
}

/// Replaces the container duration of `snapshot` with `milliseconds`.
fn set_duration(snapshot: &mut MediaSnapshot, milliseconds: u64) {
    if snapshot.general().is_none() {
        snapshot.push(StreamKind::General, crate::mediainfo::StreamInfo::new());
    }
    let Some(general) = snapshot.general_mut() else {
        return;
    };
    general.set("Duration", milliseconds.to_string());
    // The other renderings of the old duration would disagree with the new one.
    for suffix in ["/String", "/String1", "/String2", "/String4", "/String5"] {
        general.set(&format!("Duration{suffix}"), "");
    }
    let (hours, minutes, seconds, millis) = (
        milliseconds / 3_600_000,
        milliseconds / 60_000 % 60,
        milliseconds / 1000 % 60,
        milliseconds % 1000,
    );
    general.set(
        "Duration/String3",
        format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disc_marker_file_stands_for_its_disc_folder() {
        let bdmv = Path::new("D:/Media/Bridge of Spies (2015)/BDMV/index.bdmv");
        assert_eq!(
            disc_folder_of(bdmv),
            Some(Path::new("D:/Media/Bridge of Spies (2015)/BDMV"))
        );
        let dvd = Path::new("D:/Heat/video_ts/VIDEO_TS.IFO");
        assert_eq!(disc_folder_of(dvd), Some(Path::new("D:/Heat/video_ts")));
        let hd = Path::new("D:/Heat/HVDVD_TS/HV000I01.IFO");
        assert_eq!(disc_folder_of(hd), Some(Path::new("D:/Heat/HVDVD_TS")));
        // Not a marker, or a marker name outside a disc folder.
        assert_eq!(
            disc_folder_of(Path::new("D:/Heat/BDMV/STREAM/00001.m2ts")),
            None
        );
        assert_eq!(disc_folder_of(Path::new("D:/Heat/index.bdmv")), None);
        assert_eq!(disc_folder_of(Path::new("D:/Heat/Heat.mkv")), None);
    }
}

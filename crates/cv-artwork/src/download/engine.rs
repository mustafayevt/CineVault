//! The downloader: fills an item's folder with artwork.
//!
//! Three entry points share one write path:
//!
//! * [`ArtworkDownloader::download_missing`] fills only the artwork types that are empty, picking
//!   the best candidate of each with the best-image selection, then the extra fanart,
//!   extra thumbs and actor portraits the settings ask for.
//! * [`ArtworkDownloader::redownload`] fetches again, from the source URLs the library
//!   remembers, whatever is missing on disk.
//! * [`ArtworkDownloader::place`] stores one image the user chose (a candidate, a pasted URL or a
//!   dropped file as a `file:` URL), replacing the old one when asked.
//!
//! None of them touches the database; they return a [`DownloadReport`] that
//! [`crate::download::record::record`] writes into the library.

use std::collections::{BTreeSet, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_db::repo::artwork_refs::ArtworkRef;
use cv_domain::{Credit, MediaFileType};
use cv_providers::SourcedArtwork;
use cv_rename::naming::ShowArt;

use super::actors::{self, WantedActor};
use super::config::ArtworkConfig;
use super::fetch::ImageFetcher;
use super::files::{self, Trash, any_variant_exists, store, trash_other_variants};
use super::picture::{self, Picture};
use super::{ArtworkTarget, Subject};
use crate::naming::{ArtworkNames, MovieArt};
use crate::select::rank;
use crate::thumb::{ThumbCache, ThumbKey};
use crate::thumb_job::{BUILD_SIZES, THUMB_KINDS};

/// The most candidates tried for one image before giving up on it.
const MAX_TRIES: usize = 3;

/// The largest image that is accepted: a larger answer is not artwork.
pub const MAX_IMAGE_BYTES: usize = 40 * 1024 * 1024;

/// Portraits fetched at the same time.
const PORTRAIT_PARALLELISM: usize = 4;

// ---- what a run is about ---------------------------------------------------------------------

/// What one run is asked to do for one item.
pub struct DownloadRequest<'a> {
    pub target: &'a ArtworkTarget,
    pub config: &'a ArtworkConfig,
    /// What the artwork providers (and frame grabs) offer for the item. Not used by
    /// [`ArtworkDownloader::redownload`].
    pub candidates: &'a [SourcedArtwork],
    /// The source URLs the library remembers for the item.
    pub recorded: &'a [ArtworkRef],
    /// The artwork types the item has an image file for already, under any name
    /// ([`crate::download::present_artwork`]).
    pub present: &'a BTreeSet<MediaFileType>,
    /// The item's credits; the actors get portraits.
    pub cast: &'a [Credit],
    /// The user asked for replacement: existing files are replaced (and the old ones moved to the
    /// trash) instead of kept.
    pub replace: bool,
}

/// An image that was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenImage {
    /// The artwork type; `Extrafanart` and `Extrathumb` for extras.
    pub kind: MediaFileType,
    /// `0` for the single-image types, `1..` for extras.
    pub position: u32,
    pub url: String,
    pub width: u32,
    pub height: u32,
    /// The content key of the image (the thumbnail cache key).
    pub key: String,
    /// The files the image was written to (one per configured name).
    pub files: Vec<PathBuf>,
}

/// An actor portrait that was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenActor {
    pub name: String,
    pub file: PathBuf,
}

/// Something that could not be done, in words for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// What it was about: `poster`, `extra fanart 3`, `actor Tom Hanks`.
    pub what: String,
    pub reason: String,
}

impl Failure {
    pub(super) fn new(what: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            reason: reason.into(),
        }
    }
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DownloadReport {
    pub images: Vec<WrittenImage>,
    pub actors: Vec<WrittenActor>,
    /// The sets of extras that were replaced as a whole, so their remembered URLs are replaced as
    /// a whole too (even when the new set is shorter).
    pub replaced_extras: Vec<MediaFileType>,
    /// Files moved to the trash: replaced images, old extras, portraits of actors who left.
    pub trashed: Vec<PathBuf>,
    /// For a chosen image or set of extras: each `(original, trashed)` pair, so an Undo can move
    /// the displaced files back. Empty for the bulk runs.
    pub trash_moves: Vec<(PathBuf, PathBuf)>,
    /// Files that were left alone because an image was already there.
    pub kept: Vec<PathBuf>,
    pub failures: Vec<Failure>,
}

impl DownloadReport {
    /// Whether any file was written or moved.
    pub fn changed(&self) -> bool {
        !self.images.is_empty() || !self.actors.is_empty() || !self.trashed.is_empty()
    }
}

// ---- the artwork types -----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Art {
    Movie(MovieArt),
    Show(ShowArt),
}

const SHOW_ARTS: [ShowArt; 10] = [
    ShowArt::Poster,
    ShowArt::Fanart,
    ShowArt::Banner,
    ShowArt::Clearart,
    ShowArt::Clearlogo,
    ShowArt::Discart,
    ShowArt::Characterart,
    ShowArt::Keyart,
    ShowArt::Thumb,
    ShowArt::Squareart,
];

fn movie_kind(art: MovieArt) -> MediaFileType {
    match art {
        MovieArt::Poster => MediaFileType::Poster,
        MovieArt::Fanart => MediaFileType::Fanart,
        MovieArt::Banner => MediaFileType::Banner,
        MovieArt::Clearart => MediaFileType::Clearart,
        MovieArt::Thumb => MediaFileType::Thumb,
        MovieArt::Clearlogo => MediaFileType::Clearlogo,
        MovieArt::Discart => MediaFileType::Disc,
        MovieArt::Keyart => MediaFileType::Keyart,
        MovieArt::Squareart => MediaFileType::Squareart,
    }
}

fn show_kind(art: ShowArt) -> MediaFileType {
    match art {
        ShowArt::Poster => MediaFileType::Poster,
        ShowArt::Fanart => MediaFileType::Fanart,
        ShowArt::Banner => MediaFileType::Banner,
        ShowArt::Clearart => MediaFileType::Clearart,
        ShowArt::Clearlogo => MediaFileType::Clearlogo,
        ShowArt::Discart => MediaFileType::Disc,
        ShowArt::Characterart => MediaFileType::Characterart,
        ShowArt::Keyart => MediaFileType::Keyart,
        ShowArt::Thumb => MediaFileType::Thumb,
        ShowArt::Squareart => MediaFileType::Squareart,
    }
}

/// The name of a type in messages.
fn label(kind: MediaFileType) -> &'static str {
    match kind {
        MediaFileType::Poster => "poster",
        MediaFileType::Fanart => "fanart",
        MediaFileType::Banner => "banner",
        MediaFileType::Clearart => "clearart",
        MediaFileType::Thumb => "thumb",
        MediaFileType::Clearlogo => "clearlogo",
        MediaFileType::Disc => "disc art",
        MediaFileType::Keyart => "key art",
        MediaFileType::Squareart => "square art",
        MediaFileType::Characterart => "character art",
        MediaFileType::Extrafanart => "extra fanart",
        MediaFileType::Extrathumb => "extra thumb",
        _ => "image",
    }
}

impl Art {
    fn all(subject: Subject) -> Vec<Art> {
        match subject {
            Subject::Movie => MovieArt::ALL.into_iter().map(Art::Movie).collect(),
            Subject::Show => SHOW_ARTS.into_iter().map(Art::Show).collect(),
        }
    }

    fn of_kind(subject: Subject, kind: MediaFileType) -> Option<Art> {
        Self::all(subject).into_iter().find(|a| a.kind() == kind)
    }

    fn kind(self) -> MediaFileType {
        match self {
            Art::Movie(a) => movie_kind(a),
            Art::Show(a) => show_kind(a),
        }
    }

    /// The files the type is written to, relative to the artwork folder.
    fn targets(self, names: &ArtworkNames, target: &ArtworkTarget, ext: &str) -> Vec<String> {
        match self {
            Art::Movie(a) => names.movie.targets(a, &target.base, ext, target.folder),
            Art::Show(a) => names.tv.show_targets(a, ext),
        }
    }
}

/// The artwork types an item can have, in the order the artwork picker lists them: the single
/// images of its kind, then the extra fanart and extra thumbs when the folder takes extras.
#[must_use]
pub fn picker_slots(target: &ArtworkTarget) -> Vec<MediaFileType> {
    let mut kinds: Vec<MediaFileType> = Art::all(target.subject)
        .into_iter()
        .map(Art::kind)
        .collect();
    if target.writes_extras() {
        kinds.extend(Extra::ALL.into_iter().map(Extra::kind));
    }
    kinds
}

/// The type whose candidates a slot is chosen from: the extras come from the main image of their
/// kind, every other type from its own candidates.
#[must_use]
pub fn candidate_kind(slot: MediaFileType) -> MediaFileType {
    Extra::ALL
        .into_iter()
        .find(|extra| extra.kind() == slot)
        .map_or(slot, Extra::source)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Extra {
    Fanart,
    Thumbs,
}

impl Extra {
    const ALL: [Extra; 2] = [Extra::Fanart, Extra::Thumbs];

    /// The kind its files and remembered URLs are stored under.
    fn kind(self) -> MediaFileType {
        match self {
            Extra::Fanart => MediaFileType::Extrafanart,
            Extra::Thumbs => MediaFileType::Extrathumb,
        }
    }

    /// The kind of the main image, whose candidates the extras are chosen from.
    fn source(self) -> MediaFileType {
        match self {
            Extra::Fanart => MediaFileType::Fanart,
            Extra::Thumbs => MediaFileType::Thumb,
        }
    }
}

// ---- the disk half ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct SingleWrite {
    files: Vec<PathBuf>,
    trashed: Vec<PathBuf>,
    kept: Vec<PathBuf>,
}

#[derive(Debug, Default)]
struct ExtraWrite {
    /// The files of each picture, in the order of the pictures.
    files: Vec<Vec<PathBuf>>,
    trashed: Vec<PathBuf>,
}

/// Everything that touches the file system. Blocking; the downloader runs it on the blocking pool.
#[derive(Clone)]
struct Disk {
    target: ArtworkTarget,
    names: ArtworkNames,
    trash: Trash,
    thumbs: Option<Arc<ThumbCache>>,
}

impl Disk {
    fn new(
        target: &ArtworkTarget,
        config: &ArtworkConfig,
        thumbs: Option<Arc<ThumbCache>>,
    ) -> Self {
        Self {
            trash: Trash::new(target.data_source.as_deref(), &target.root),
            target: target.clone(),
            names: config.names.clone(),
            thumbs,
        }
    }

    /// Lets the thumbnail cache have the grid sizes of a poster-like image right away.
    fn cache_thumbs(&self, kind: MediaFileType, bytes: &[u8]) {
        if let Some(cache) = &self.thumbs
            && THUMB_KINDS.contains(&kind)
            && let Err(error) = cache.ensure_bytes(bytes, &BUILD_SIZES)
        {
            tracing::debug!(%error, "thumbnail of a new image not built");
        }
    }

    fn write_single(&self, art: Art, picture: &Picture, replace: bool) -> io::Result<SingleWrite> {
        let mut out = SingleWrite::default();
        for name in art.targets(&self.names, &self.target, picture.info.ext) {
            let path = self.target.dir.join(&name);
            if replace {
                store(&path, &picture.bytes, true, &self.trash, &mut out.trashed)?;
                trash_other_variants(&path, &self.trash, &mut out.trashed)?;
                out.files.push(path);
            } else if any_variant_exists(&path) {
                out.kept.push(path);
            } else {
                store(&path, &picture.bytes, false, &self.trash, &mut out.trashed)?;
                out.files.push(path);
            }
        }
        if !out.files.is_empty() {
            self.cache_thumbs(art.kind(), &picture.bytes);
        }
        Ok(out)
    }

    /// Moves each file of `current` that is not one of `written` to the trash.
    fn trash_superseded(
        &self,
        current: &[PathBuf],
        written: &[PathBuf],
    ) -> io::Result<Vec<PathBuf>> {
        let same = |a: &Path, b: &Path| {
            let normal = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
            normal(a) == normal(b)
        };
        let mut trashed = Vec::new();
        for path in current {
            if path.is_file() && !written.iter().any(|w| same(w, path)) {
                trashed.push(self.trash.put(path)?);
            }
        }
        Ok(trashed)
    }

    // -- extras --

    /// The file name of extra number `index`, or `None` when this item gets no such extras.
    fn extra_name(&self, extra: Extra, index: u32, ext: &str) -> Option<String> {
        if !self.target.writes_extras() {
            return None;
        }
        match (extra, self.target.subject) {
            (Extra::Fanart, Subject::Movie) => self
                .names
                .movie
                .extra_fanart_targets(&self.target.base, ext, self.target.folder, index)
                .pop(),
            (Extra::Fanart, Subject::Show) => Some(format!("extrafanart/fanart{index}.{ext}")),
            (Extra::Thumbs, Subject::Movie) => self
                .names
                .movie
                .extra_thumb_targets(self.target.folder, ext, index)
                .pop(),
            (Extra::Thumbs, Subject::Show) => None,
        }
    }

    /// The folder that holds the extras, when they have one of their own.
    fn extra_folder(&self, extra: Extra) -> Option<PathBuf> {
        if !self.target.writes_extras() {
            return None;
        }
        let name = match (extra, self.target.subject) {
            (Extra::Fanart, Subject::Movie) => {
                self.names.movie.extra_fanart_folder(self.target.folder)?
            }
            (Extra::Fanart, Subject::Show) => "extrafanart",
            (Extra::Thumbs, Subject::Movie) => crate::naming::EXTRA_THUMB_FOLDER,
            (Extra::Thumbs, Subject::Show) => return None,
        };
        Some(self.target.dir.join(name))
    }

    /// Every extra image that exists now.
    fn existing_extras(&self, extra: Extra) -> Vec<PathBuf> {
        if let Some(folder) = self.extra_folder(extra) {
            if actors::is_link(&folder) {
                return Vec::new();
            }
            let Ok(entries) = std::fs::read_dir(&folder) else {
                return Vec::new();
            };
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && files::is_image_name(p))
                .collect();
            found.sort();
            return found;
        }
        let mut found = Vec::new();
        for index in 1..=super::config::MAX_EXTRAS {
            let Some(name) = self.extra_name(extra, index, "jpg") else {
                break;
            };
            found.extend(files::variants(&self.target.dir.join(name)));
        }
        found
    }

    fn extra_exists(&self, extra: Extra, index: u32) -> bool {
        self.extra_name(extra, index, "jpg")
            .is_some_and(|name| any_variant_exists(&self.target.dir.join(name)))
    }

    /// Writes a whole new set of extras and moves the old ones that it does not overwrite to the
    /// trash. The old ones stay until the new ones are in place.
    fn write_extras(&self, extra: Extra, pictures: &[Picture]) -> io::Result<ExtraWrite> {
        let old = self.existing_extras(extra);
        let mut out = ExtraWrite::default();
        let mut written: HashSet<PathBuf> = HashSet::new();
        for (i, picture) in pictures.iter().enumerate() {
            let index = u32::try_from(i + 1).unwrap_or(u32::MAX);
            let Some(name) = self.extra_name(extra, index, picture.info.ext) else {
                continue;
            };
            let path = self.target.dir.join(name);
            store(&path, &picture.bytes, true, &self.trash, &mut out.trashed)?;
            trash_other_variants(&path, &self.trash, &mut out.trashed)?;
            written.insert(path.clone());
            out.files.push(vec![path]);
        }
        for leftover in old {
            if !written.contains(&leftover) && leftover.is_file() {
                out.trashed.push(self.trash.put(&leftover)?);
            }
        }
        Ok(out)
    }

    /// Writes extra number `index` only. `None` when it exists and `replace` is not set.
    fn write_extra_at(
        &self,
        extra: Extra,
        index: u32,
        picture: &Picture,
        replace: bool,
    ) -> io::Result<Option<(PathBuf, Vec<PathBuf>)>> {
        let Some(name) = self.extra_name(extra, index, picture.info.ext) else {
            return Ok(None);
        };
        let path = self.target.dir.join(name);
        if !replace && any_variant_exists(&path) {
            return Ok(None);
        }
        let mut trashed = Vec::new();
        store(&path, &picture.bytes, true, &self.trash, &mut trashed)?;
        trash_other_variants(&path, &self.trash, &mut trashed)?;
        Ok(Some((path, trashed)))
    }

    // -- actors --

    fn portrait_dir(&self) -> PathBuf {
        actors::actors_dir(&self.target.root)
    }

    fn has_portrait(&self, stem: &str) -> bool {
        any_variant_exists(&self.portrait_dir().join(format!("{stem}.jpg")))
    }

    /// Writes a portrait. `None` when the folder is a link, or the portrait exists and `replace`
    /// is not set.
    fn write_portrait(
        &self,
        stem: &str,
        picture: &Picture,
        replace: bool,
    ) -> io::Result<Option<(PathBuf, Vec<PathBuf>)>> {
        let dir = self.portrait_dir();
        if actors::is_link(&dir) {
            return Ok(None);
        }
        let path = dir.join(format!("{stem}.{}", picture.info.ext));
        if !replace && self.has_portrait(stem) {
            return Ok(None);
        }
        let mut trashed = Vec::new();
        store(&path, &picture.bytes, true, &self.trash, &mut trashed)?;
        trash_other_variants(&path, &self.trash, &mut trashed)?;
        Ok(Some((path, trashed)))
    }

    /// Moves the portraits of actors who are no longer in the cast to the trash.
    fn clean_portraits(&self, wanted: &[WantedActor]) -> io::Result<Vec<PathBuf>> {
        let mut trashed = Vec::new();
        for stale in actors::stale_portraits(&self.portrait_dir(), wanted) {
            trashed.push(self.trash.put(&stale)?);
        }
        Ok(trashed)
    }
}

async fn on_disk<T, F>(disk: &Disk, work: F) -> io::Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Disk) -> io::Result<T> + Send + 'static,
{
    let disk = disk.clone();
    tokio::task::spawn_blocking(move || work(&disk))
        .await
        .map_err(|e| io::Error::other(e.to_string()))?
}

// ---- fetching --------------------------------------------------------------------------------

/// Fetches and checks an image. The reason is in words for the user.
pub(super) async fn load_image(fetcher: &dyn ImageFetcher, url: &str) -> Result<Picture, String> {
    let bytes = fetcher.fetch(url).await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("the image is too large".to_owned());
    }
    let info = picture::inspect(&bytes).map_err(|e| match e {
        // A chosen file is no "answer": say what it is and what to use instead.
        picture::ImageError::NotAnImage if is_file_url(url) => {
            "the file is not a picture; use a JPEG, PNG or WebP file".to_owned()
        }
        picture::ImageError::NotAnImage => "the address does not give a picture".to_owned(),
        other => other.to_string(),
    })?;
    Ok(Picture { bytes, info })
}

fn is_file_url(url: &str) -> bool {
    url.get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
}

fn recorded_url(recorded: &[ArtworkRef], kind: MediaFileType, position: u32) -> Option<&str> {
    recorded
        .iter()
        .find(|r| r.kind == kind && r.position == position)
        .map(|r| r.url.as_str())
        .filter(|u| !u.trim().is_empty())
}

fn recorded_extras(recorded: &[ArtworkRef], kind: MediaFileType) -> Vec<&ArtworkRef> {
    let mut found: Vec<&ArtworkRef> = recorded
        .iter()
        .filter(|r| r.kind == kind && r.position >= 1 && !r.url.trim().is_empty())
        .collect();
    found.sort_by_key(|r| r.position);
    found
}

// ---- the downloader --------------------------------------------------------------------------

/// Downloads artwork for items.
#[derive(Clone)]
pub struct ArtworkDownloader {
    fetcher: Arc<dyn ImageFetcher>,
    thumbs: Option<Arc<ThumbCache>>,
}

impl ArtworkDownloader {
    pub fn new(fetcher: Arc<dyn ImageFetcher>) -> Self {
        Self {
            fetcher,
            thumbs: None,
        }
    }

    /// New posters get their grid thumbnails in `cache` as soon as they are written, from the
    /// bytes already in memory, so the grid shows them without reading the file again.
    #[must_use]
    pub fn with_thumbs(mut self, cache: Arc<ThumbCache>) -> Self {
        self.thumbs = Some(cache);
        self
    }

    fn disk(&self, target: &ArtworkTarget, config: &ArtworkConfig) -> Disk {
        Disk::new(target, config, self.thumbs.clone())
    }

    // ---- download missing ----------------------------------------------------------------

    /// Fills the empty artwork types of the item, then the extras and portraits the settings ask
    /// for. With [`DownloadRequest::replace`] every type is chosen again and the old files go to
    /// the trash.
    pub async fn download_missing(&self, request: &DownloadRequest<'_>) -> DownloadReport {
        let mut report = DownloadReport::default();
        if !request.target.exists() {
            return report;
        }
        let disk = self.disk(request.target, request.config);
        let chosen = self.fill_singles(request, &disk, &mut report).await;
        for extra in Extra::ALL {
            self.fill_extras(request, &disk, extra, &chosen, &mut report)
                .await;
        }
        self.sync_actors(request, &disk, &mut report).await;
        report
    }

    /// The URLs to try for a type, best first.
    fn single_urls(request: &DownloadRequest<'_>, kind: MediaFileType) -> Vec<String> {
        if request.config.download.auto_select {
            let mut seen = HashSet::new();
            rank(request.candidates, kind, &request.config.selection)
                .into_iter()
                .map(|c| c.candidate.url.clone())
                .filter(|u| seen.insert(u.clone()))
                .take(MAX_TRIES)
                .collect()
        } else {
            recorded_url(request.recorded, kind, 0)
                .map(str::to_owned)
                .into_iter()
                .collect()
        }
    }

    /// Returns the URL chosen for each type that was written.
    async fn fill_singles(
        &self,
        request: &DownloadRequest<'_>,
        disk: &Disk,
        report: &mut DownloadReport,
    ) -> Vec<(MediaFileType, String)> {
        let mut chosen = Vec::new();
        for art in Art::all(request.target.subject) {
            let kind = art.kind();
            let probe = art.targets(&request.config.names, request.target, "jpg");
            if probe.is_empty() {
                continue;
            }
            let there = request.present.contains(&kind)
                || probe
                    .iter()
                    .any(|name| any_variant_exists(&request.target.dir.join(name)));
            if there && !request.replace {
                continue;
            }
            let mut found = None;
            for url in Self::single_urls(request, kind) {
                match load_image(self.fetcher.as_ref(), &url).await {
                    Ok(picture) => {
                        found = Some((url, picture));
                        break;
                    }
                    Err(reason) => report.failures.push(Failure::new(label(kind), reason)),
                }
            }
            let Some((url, picture)) = found else {
                continue;
            };
            if self
                .store_single(disk, art, &url, picture, request.replace, report)
                .await
            {
                chosen.push((kind, url));
            }
        }
        chosen
    }

    /// Writes one single-image type. True when files were written.
    async fn store_single(
        &self,
        disk: &Disk,
        art: Art,
        url: &str,
        picture: Picture,
        replace: bool,
        report: &mut DownloadReport,
    ) -> bool {
        let info = picture.info;
        let key = ThumbKey::of_bytes(&picture.bytes).as_str().to_owned();
        let written = on_disk(disk, move |d| d.write_single(art, &picture, replace)).await;
        match written {
            Ok(write) => {
                report.trashed.extend(write.trashed);
                report.kept.extend(write.kept);
                if write.files.is_empty() {
                    return false;
                }
                report.images.push(WrittenImage {
                    kind: art.kind(),
                    position: 0,
                    url: url.to_owned(),
                    width: info.width,
                    height: info.height,
                    key,
                    files: write.files,
                });
                true
            }
            Err(error) => {
                report.failures.push(Failure::new(
                    label(art.kind()),
                    format!("it could not be saved: {error}"),
                ));
                false
            }
        }
    }

    async fn fill_extras(
        &self,
        request: &DownloadRequest<'_>,
        disk: &Disk,
        extra: Extra,
        chosen: &[(MediaFileType, String)],
        report: &mut DownloadReport,
    ) {
        let options = &request.config.download;
        let wanted = match extra {
            Extra::Fanart => options.extra_fanart,
            Extra::Thumbs => options.extra_thumbs,
        };
        let Some(count) = wanted else {
            return;
        };
        if disk.extra_name(extra, 1, "jpg").is_none() {
            return;
        }
        let count = count as usize;
        let existing = on_disk(disk, move |d| Ok(d.existing_extras(extra).len()))
            .await
            .unwrap_or(0);
        let remembered = recorded_extras(request.recorded, extra.kind()).len();
        // Complete: as many as asked for, or as many as the last run could find.
        let complete = existing >= count || (remembered > 0 && existing >= remembered);
        if complete && !request.replace {
            return;
        }

        let urls: Vec<String> = if options.auto_select {
            // The main image is not repeated as an extra.
            let main = chosen
                .iter()
                .find(|(kind, _)| *kind == extra.source())
                .map(|(_, url)| url.as_str())
                .or_else(|| recorded_url(request.recorded, extra.source(), 0));
            let mut seen: HashSet<&str> = main.into_iter().collect();
            rank(
                request.candidates,
                extra.source(),
                &request.config.selection,
            )
            .into_iter()
            .map(|c| c.candidate.url.as_str())
            .filter(|u| seen.insert(u))
            .map(str::to_owned)
            .collect()
        } else {
            recorded_extras(request.recorded, extra.kind())
                .into_iter()
                .map(|r| r.url.clone())
                .collect()
        };

        let mut pictures: Vec<(String, Picture)> = Vec::new();
        let mut failed = 0usize;
        for url in urls {
            if pictures.len() >= count || failed >= MAX_TRIES + count {
                break;
            }
            match self
                .load_extra(&url, extra, request.config.download.extra_thumb_width)
                .await
            {
                Ok(picture) => pictures.push((url, picture)),
                Err(reason) => {
                    failed += 1;
                    report.failures.push(Failure::new(
                        format!("{} {}", label(extra.kind()), pictures.len() + 1),
                        reason,
                    ));
                }
            }
        }
        self.store_extras(disk, extra, pictures, report).await;
    }

    /// Writes a whole new set of extras and records them in the report. The old extras that the
    /// new set does not overwrite go to the trash. Nothing is touched when `pictures` is empty.
    async fn store_extras(
        &self,
        disk: &Disk,
        extra: Extra,
        pictures: Vec<(String, Picture)>,
        report: &mut DownloadReport,
    ) {
        if pictures.is_empty() {
            return;
        }
        let infos: Vec<(String, picture::ImageInfo, String)> = pictures
            .iter()
            .map(|(url, p)| {
                (
                    url.clone(),
                    p.info,
                    ThumbKey::of_bytes(&p.bytes).as_str().to_owned(),
                )
            })
            .collect();
        let set: Vec<Picture> = pictures.into_iter().map(|(_, p)| p).collect();
        match on_disk(disk, move |d| d.write_extras(extra, &set)).await {
            Ok(write) => {
                report.trashed.extend(write.trashed);
                report.replaced_extras.push(extra.kind());
                for (i, ((url, info, key), files)) in infos.into_iter().zip(write.files).enumerate()
                {
                    report.images.push(WrittenImage {
                        kind: extra.kind(),
                        position: u32::try_from(i + 1).unwrap_or(u32::MAX),
                        url,
                        width: info.width,
                        height: info.height,
                        key,
                        files,
                    });
                }
            }
            Err(error) => report.failures.push(Failure::new(
                label(extra.kind()),
                format!("they could not be saved: {error}"),
            )),
        }
    }

    /// An extra image, scaled down when the settings say so (extra thumbs only).
    async fn load_extra(
        &self,
        url: &str,
        extra: Extra,
        thumb_width: Option<u32>,
    ) -> Result<Picture, String> {
        let picture = load_image(self.fetcher.as_ref(), url).await?;
        match (extra, thumb_width) {
            (Extra::Thumbs, Some(width)) if picture.info.width > width => {
                tokio::task::spawn_blocking(move || {
                    picture::shrink_to_width(&picture.bytes, width).map_err(|e| e.to_string())
                })
                .await
                .map_err(|e| e.to_string())?
            }
            _ => Ok(picture),
        }
    }

    // ---- actors ------------------------------------------------------------------------------

    async fn sync_actors(
        &self,
        request: &DownloadRequest<'_>,
        disk: &Disk,
        report: &mut DownloadReport,
    ) {
        if !request.config.download.actor_images || !request.target.writes_actors() {
            return;
        }
        let dir = actors::actors_dir(&request.target.root);
        if actors::is_link(&dir) {
            return;
        }
        let wanted = actors::wanted_actors(request.cast);

        // Portraits of actors who left the cast go first, so a name can be reused.
        let cleanup = wanted.clone();
        match on_disk(disk, move |d| d.clean_portraits(&cleanup)).await {
            Ok(trashed) => report.trashed.extend(trashed),
            Err(error) => report.failures.push(Failure::new(
                "actor images",
                format!("old portraits could not be removed: {error}"),
            )),
        }

        let mut to_fetch: Vec<&WantedActor> = Vec::new();
        for actor in wanted.iter().filter(|a| !a.url.is_empty()) {
            if request.replace || !any_variant_exists(&dir.join(format!("{}.jpg", actor.stem))) {
                to_fetch.push(actor);
            }
        }
        self.fetch_portraits(&to_fetch, request.replace, disk, report)
            .await;
    }

    /// Fetches portraits a few at a time and writes them.
    async fn fetch_portraits(
        &self,
        actors: &[&WantedActor],
        replace: bool,
        disk: &Disk,
        report: &mut DownloadReport,
    ) {
        for chunk in actors.chunks(PORTRAIT_PARALLELISM) {
            let mut tasks = tokio::task::JoinSet::new();
            for (index, actor) in chunk.iter().enumerate() {
                let fetcher = Arc::clone(&self.fetcher);
                let url = actor.url.clone();
                tasks.spawn(async move { (index, load_image(fetcher.as_ref(), &url).await) });
            }
            let mut loaded: Vec<(usize, Result<Picture, String>)> = Vec::new();
            while let Some(joined) = tasks.join_next().await {
                match joined {
                    Ok(answer) => loaded.push(answer),
                    Err(error) => tracing::warn!(%error, "a portrait download stopped"),
                }
            }
            loaded.sort_by_key(|(index, _)| *index);
            for (index, answer) in loaded {
                let actor = chunk[index];
                let what = format!("actor {}", actor.name);
                match answer {
                    Err(reason) => report.failures.push(Failure::new(what, reason)),
                    Ok(picture) => {
                        let stem = actor.stem.clone();
                        let written =
                            on_disk(disk, move |d| d.write_portrait(&stem, &picture, replace))
                                .await;
                        match written {
                            Ok(Some((file, trashed))) => {
                                report.trashed.extend(trashed);
                                report.actors.push(WrittenActor {
                                    name: actor.name.clone(),
                                    file,
                                });
                            }
                            Ok(None) => {}
                            Err(error) => report.failures.push(Failure::new(
                                what,
                                format!("it could not be saved: {error}"),
                            )),
                        }
                    }
                }
            }
        }
    }

    // ---- re-download -------------------------------------------------------------------------

    /// Fetches again, from the remembered source URLs, the images that are missing on disk: the
    /// single-image types, the extras and the actor portraits. An image that is there is left
    /// alone unless [`DownloadRequest::replace`] is set. Types the item has an image for under
    /// another name ([`DownloadRequest::present`]) are not added a second time.
    pub async fn redownload(&self, request: &DownloadRequest<'_>) -> DownloadReport {
        let mut report = DownloadReport::default();
        if !request.target.exists() {
            return report;
        }
        let disk = self.disk(request.target, request.config);

        for art in Art::all(request.target.subject) {
            let kind = art.kind();
            let Some(url) = recorded_url(request.recorded, kind, 0) else {
                continue;
            };
            let probe = art.targets(&request.config.names, request.target, "jpg");
            if probe.is_empty() {
                continue;
            }
            let there = request.present.contains(&kind)
                || probe
                    .iter()
                    .any(|name| any_variant_exists(&request.target.dir.join(name)));
            if there && !request.replace {
                continue;
            }
            match load_image(self.fetcher.as_ref(), url).await {
                Ok(picture) => {
                    self.store_single(&disk, art, url, picture, request.replace, &mut report)
                        .await;
                }
                Err(reason) => report.failures.push(Failure::new(label(kind), reason)),
            }
        }

        for extra in Extra::ALL {
            if disk.extra_name(extra, 1, "jpg").is_none() {
                continue;
            }
            for entry in recorded_extras(request.recorded, extra.kind()) {
                let index = entry.position;
                let replace = request.replace;
                let there = on_disk(&disk, move |d| Ok(d.extra_exists(extra, index)))
                    .await
                    .unwrap_or(false);
                if there && !replace {
                    continue;
                }
                let what = format!("{} {index}", label(extra.kind()));
                match self
                    .load_extra(&entry.url, extra, request.config.download.extra_thumb_width)
                    .await
                {
                    Err(reason) => report.failures.push(Failure::new(what, reason)),
                    Ok(picture) => {
                        let (info, key) = (
                            picture.info,
                            ThumbKey::of_bytes(&picture.bytes).as_str().to_owned(),
                        );
                        let written = on_disk(&disk, move |d| {
                            d.write_extra_at(extra, index, &picture, replace)
                        })
                        .await;
                        match written {
                            Ok(Some((file, trashed))) => {
                                report.trashed.extend(trashed);
                                report.images.push(WrittenImage {
                                    kind: extra.kind(),
                                    position: index,
                                    url: entry.url.clone(),
                                    width: info.width,
                                    height: info.height,
                                    key,
                                    files: vec![file],
                                });
                            }
                            Ok(None) => {}
                            Err(error) => report.failures.push(Failure::new(
                                what,
                                format!("it could not be saved: {error}"),
                            )),
                        }
                    }
                }
            }
        }

        // Portraits come from the credits, which keep the address of each one.
        if request.config.download.actor_images && request.target.writes_actors() {
            let dir = actors::actors_dir(&request.target.root);
            if !actors::is_link(&dir) {
                let wanted = actors::wanted_actors(request.cast);
                let missing: Vec<&WantedActor> = wanted
                    .iter()
                    .filter(|a| {
                        !a.url.is_empty()
                            && (request.replace
                                || !any_variant_exists(&dir.join(format!("{}.jpg", a.stem))))
                    })
                    .collect();
                self.fetch_portraits(&missing, request.replace, &disk, &mut report)
                    .await;
            }
        }
        report
    }

    // ---- a chosen image ------------------------------------------------------------------------

    /// Stores the image at `url` as the item's image of type `kind`: a candidate the user picked,
    /// a pasted address, or a dropped file as a `file:` URL. With `replace` the old image goes to
    /// the trash; without it an existing image is kept and nothing is written.
    pub async fn place(
        &self,
        target: &ArtworkTarget,
        config: &ArtworkConfig,
        kind: MediaFileType,
        url: &str,
        replace: bool,
    ) -> DownloadReport {
        self.place_over(target, config, kind, url, replace, &[])
            .await
    }

    /// [`Self::place`], where `current` lists the item's present files of type `kind`. With
    /// `replace`, those the new image does not overwrite go to the trash as well: an image under
    /// a name the settings no longer write (`poster.jpg` next to a new `<base>-poster.png`) is
    /// the old image, and keeping it would leave two images of one type.
    pub async fn place_over(
        &self,
        target: &ArtworkTarget,
        config: &ArtworkConfig,
        kind: MediaFileType,
        url: &str,
        replace: bool,
        current: &[PathBuf],
    ) -> DownloadReport {
        let mut report = DownloadReport::default();
        let Some(art) = Art::of_kind(target.subject, kind) else {
            report.failures.push(Failure::new(
                label(kind),
                "this kind of image cannot be set this way",
            ));
            return report;
        };
        if !target.exists() {
            report.failures.push(Failure::new(
                label(kind),
                "the folder of the item is missing",
            ));
            return report;
        }
        if art.targets(&config.names, target, "jpg").is_empty() {
            report.failures.push(Failure::new(
                label(kind),
                "no file name is set for it in the settings",
            ));
            return report;
        }
        match load_image(self.fetcher.as_ref(), url).await {
            Ok(picture) => {
                let disk = self.disk(target, config);
                let stored = self
                    .store_single(&disk, art, url, picture, replace, &mut report)
                    .await;
                if stored && replace && !current.is_empty() {
                    let written: Vec<PathBuf> = report
                        .images
                        .iter()
                        .flat_map(|i| i.files.iter().cloned())
                        .collect();
                    let current = current.to_vec();
                    match on_disk(&disk, move |d| d.trash_superseded(&current, &written)).await {
                        Ok(trashed) => report.trashed.extend(trashed),
                        Err(error) => report.failures.push(Failure::new(
                            label(kind),
                            format!("the old image could not be moved to the trash: {error}"),
                        )),
                    }
                }
                report.trash_moves = disk.trash.moves();
            }
            Err(reason) => report.failures.push(Failure::new(label(kind), reason)),
        }
        report
    }

    /// Replaces the extra fanart or the extra thumbs (`kind` is `Extrafanart` or `Extrathumb`)
    /// with the images at `urls`: the chosen candidates, pasted addresses and dropped files as
    /// `file:` URLs. The whole set is replaced, as with every run that writes extras; the old
    /// files that the new ones do not overwrite go to the trash. Extra thumbs are shrunk when the
    /// settings say so. The set stays as it was when no image could be loaded.
    pub async fn place_extras(
        &self,
        target: &ArtworkTarget,
        config: &ArtworkConfig,
        kind: MediaFileType,
        urls: &[String],
    ) -> DownloadReport {
        let mut report = DownloadReport::default();
        let extra = match kind {
            MediaFileType::Extrafanart => Extra::Fanart,
            MediaFileType::Extrathumb => Extra::Thumbs,
            other => {
                report.failures.push(Failure::new(
                    label(other),
                    "this kind of image cannot be set this way",
                ));
                return report;
            }
        };
        if !target.exists() {
            report.failures.push(Failure::new(
                label(kind),
                "the folder of the item is missing",
            ));
            return report;
        }
        let disk = self.disk(target, config);
        if disk.extra_name(extra, 1, "jpg").is_none() {
            report.failures.push(Failure::new(
                label(kind),
                "extras are not written for this folder",
            ));
            return report;
        }
        let mut seen = HashSet::new();
        let mut pictures: Vec<(String, Picture)> = Vec::new();
        for url in urls.iter().filter(|url| seen.insert(url.as_str())) {
            match self
                .load_extra(url, extra, config.download.extra_thumb_width)
                .await
            {
                Ok(picture) => pictures.push((url.clone(), picture)),
                Err(reason) => report.failures.push(Failure::new(
                    format!("{} {}", label(kind), pictures.len() + 1),
                    reason,
                )),
            }
        }
        self.store_extras(&disk, extra, pictures, &mut report).await;
        report.trash_moves = disk.trash.moves();
        report
    }
}

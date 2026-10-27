//! The media-info job: reads the technical details of the videos a scan found.
//!
//! A scan stores a new video without its stream details; this job fills them in, one title per
//! job item, on the disk lane:
//!
//! 1. **Sidecar first.** An up-to-date `<name>-mediainfo.xml` next to the video stands in for
//!    reading it (`cv_media::mediainfo::read_sidecar`), which spares a slow network share.
//! 2. **Then the video.** The MediaInfo library reads the file (a disc folder or an `.iso` is
//!    analysed through its structure). When the "store MediaInfo XML next to the media" setting is
//!    on, the result is written as a sidecar.
//! 3. **Aspect ratio (optional).** With the setting on, FFmpeg measures the visible picture. A
//!    missing FFmpeg and an ISO image are reported once as a notice, never silently skipped.
//! 4. **Runtime from media (optional).** With the setting on, the title's runtime becomes the
//!    summed length of its video parts, unless the title or its runtime field is locked.
//!
//! The read happens off the database. The result is written inside one transaction from a fresh
//! copy of the title, and only to files whose size and time still match the ones that were read,
//! so a video replaced or an edit made meanwhile is never overwritten.
//!
//! A disc structure stored by its marker file (`BDMV/index.bdmv`, `VIDEO_TS/VIDEO_TS.IFO`) is read
//! through the disc folder, so its main title gives the streams and the running time
//! (`cv_media::disc::disc_folder_of`); the marker is never read as a file of its own, and no
//! sidecar is written into the disc structure.
//!
//! A title that cannot be read fails alone, with a groupable reason, and is not retried in the
//! same session (a new request, which a scan, a start or Read now makes, forgets that). Each video
//! that failed is also marked *unreadable at its size and modification time* in the library
//! (`media_unreadable`), so the next launch does not try it again, each try with its own time
//! limit; a changed file is tried again. A read that ran out of time is marked too, for the
//! details page to say so, but stays pending: a busy or sleeping disk is not a broken file. **Reload media info**
//! ([`reload_titles`]) is the one thing that clears the mark: it also clears the stored details of
//! the title's videos and reads them again, without trusting a sidecar ([`MediaInfoWork::forced`]).
//! When MediaInfo is not installed the job is not started at all: the titles stay *pending* (their
//! video rows have no container format) and the caller offers the download.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use cv_db::Database;
use cv_db::repo::library_ops::tree_ids;
use cv_db::repo::media_info::{
    PendingMedia, TIMED_OUT, UnreadableMark, clear_unreadable, mark_unreadable,
};
use cv_db::repo::{field_locks, get_item, update_item};
use cv_domain::{ItemDetail, ItemId, MediaFile, MediaFileType, MetaField};
use cv_jobs::{
    ItemControl, ItemError, ItemWork, JobHandle, JobManager, JobSpec, LaneId, StageError,
};
use cv_media::aspect::{
    ArdMode, ArdSettings, AspectDetection, AspectError, AspectRequest, MultiFormat,
    detect_aspect_ratio,
};
use cv_media::disc::{DiscError, analyze_path, disc_folder_of, is_image_path};
use cv_media::mediainfo::{
    Analyzer, MediaInfoError, MediaInfoLibrary, MediaInfoPool, MediaSnapshot, PoolOptions,
    SidecarState, apply_snapshot, read_sidecar, write_sidecar,
};
use cv_media::runtime::effective_runtime_minutes;
use cv_media::tools::{Cancel, ToolsManager};
use serde_json::Value;

use crate::app_settings::SettingsService;
use crate::changes::{ChangePart, Changes};
use crate::notices::{AppNotice, NoticeHub, NoticeLevel};

/// Setting: write the MediaInfo XML sidecar next to each video that was read.
pub const STORE_XML_KEY: &str = "media.store-mediainfo-xml";
/// Setting: use the length of the video as the runtime.
pub const RUNTIME_FROM_MEDIA_KEY: &str = "media.runtime-from-media";
/// Setting: measure the visible aspect ratio with FFmpeg after media info is read.
pub const DETECT_ASPECT_KEY: &str = "media.detect-aspect-ratio";

/// Where the Settings page offers the tool download.
pub const TOOLS_PAGE: &str = "/settings/tools";

/// Setting: how thoroughly the picture is sampled (`fast`, `default` or `accurate`).
pub const ASPECT_MODE_KEY: &str = "media.aspect-mode";
/// Setting: round a measured ratio up to the next wider standard ratio.
pub const ASPECT_ROUND_UP_KEY: &str = "media.aspect-round-up";
/// Setting: which ratio of a multi-format film is recorded (`most-common`, `higher` or `wider`).
pub const ASPECT_MULTI_FORMAT_KEY: &str = "media.aspect-multi-format";

/// The aspect-ratio detection choices; a missing or unknown value takes the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AspectOptions {
    pub mode: ArdMode,
    pub round_up: bool,
    pub multi_format: MultiFormat,
}

impl AspectOptions {
    fn from_values(values: &BTreeMap<String, Value>) -> Self {
        let text = |key: &str| values.get(key).and_then(Value::as_str).map(str::trim);
        Self {
            mode: match text(ASPECT_MODE_KEY) {
                Some("fast") => ArdMode::Fast,
                Some("accurate") => ArdMode::Accurate,
                _ => ArdMode::Standard,
            },
            round_up: values
                .get(ASPECT_ROUND_UP_KEY)
                .and_then(Value::as_bool)
                .unwrap_or(false),
            multi_format: match text(ASPECT_MULTI_FORMAT_KEY) {
                Some("higher") => MultiFormat::Higher,
                Some("wider") => MultiFormat::Wider,
                _ => MultiFormat::MostCommon,
            },
        }
    }

    /// The detection settings these choices stand for.
    #[must_use]
    pub fn ard_settings(self) -> ArdSettings {
        ArdSettings {
            mode: self.mode,
            round_up: self.round_up,
            multi_format: self.multi_format,
            ..ArdSettings::default()
        }
    }
}

/// The media-info settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MediaInfoSettings {
    pub store_xml: bool,
    pub runtime_from_media: bool,
    pub detect_aspect: bool,
    pub aspect: AspectOptions,
}

impl MediaInfoSettings {
    /// Reads the settings; the switches are off and the aspect choices take their defaults
    /// when missing.
    #[must_use]
    pub fn read(settings: &SettingsService) -> Self {
        let values: BTreeMap<String, Value> = [
            STORE_XML_KEY,
            RUNTIME_FROM_MEDIA_KEY,
            DETECT_ASPECT_KEY,
            ASPECT_MODE_KEY,
            ASPECT_ROUND_UP_KEY,
            ASPECT_MULTI_FORMAT_KEY,
        ]
        .into_iter()
        .filter_map(|key| settings.get(key).map(|value| (key.to_owned(), value)))
        .collect();
        Self::from_values(&values)
    }

    /// The same from a plain map of values.
    #[must_use]
    pub fn from_values(values: &BTreeMap<String, Value>) -> Self {
        let on = |key: &str| values.get(key).and_then(Value::as_bool).unwrap_or(false);
        Self {
            store_xml: on(STORE_XML_KEY),
            runtime_from_media: on(RUNTIME_FROM_MEDIA_KEY),
            detect_aspect: on(DETECT_ASPECT_KEY),
            aspect: AspectOptions::from_values(values),
        }
    }
}

/// Reads one video (or disc) into a snapshot. [`LibraryReader`] is the real one; tests supply
/// fakes, so the job is exercised without the MediaInfo library.
pub trait MediaReader: Send + Sync + 'static {
    /// # Errors
    /// Whatever stops the file from being read.
    fn read(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError>;

    /// [`Self::read`] that stops waiting when `cancelled` says so
    /// ([`MediaInfoError::Cancelled`]). The default cannot be stopped and simply reads.
    ///
    /// # Errors
    /// As [`Self::read`].
    fn read_unless(
        &self,
        path: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<MediaSnapshot, MediaInfoError> {
        let _ = cancelled;
        self.read(path)
    }

    /// The reader's version, recorded in a sidecar.
    fn version(&self) -> String;
}

/// Opens a reader, or says why none is available (MediaInfo is not installed).
pub type ReaderSource = Arc<dyn Fn() -> Result<Arc<dyn MediaReader>, MediaInfoError> + Send + Sync>;

/// MediaInfo reading plain files, and disc folders and images through their structure.
struct DiscAware(MediaInfoLibrary);

impl Analyzer for DiscAware {
    fn analyze(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
        // A disc's marker file (`BDMV/index.bdmv`) is read through its disc folder.
        let path = disc_folder_of(path).unwrap_or(path);
        if path.is_dir() || is_image_path(path) {
            return analyze_path(&self.0, path)
                .map(|analysis| analysis.snapshot)
                .map_err(|error| match error {
                    DiscError::MediaInfo(inner) => inner,
                    other => MediaInfoError::Source {
                        name: path.display().to_string(),
                        message: other.to_string(),
                    },
                });
        }
        self.0.read(path)
    }
}

/// The real reader: the MediaInfo library behind a pool that limits parallel reads (one at a time
/// per drive) and gives each file a time limit that grows with its size.
pub struct LibraryReader {
    pool: MediaInfoPool<DiscAware>,
    version: String,
}

impl LibraryReader {
    /// Loads the library the tools manager resolves.
    ///
    /// # Errors
    /// [`MediaInfoError::Tool`] when MediaInfo is not installed; load errors otherwise.
    pub fn from_tools(tools: &ToolsManager) -> Result<Self, MediaInfoError> {
        let library = MediaInfoLibrary::from_tools(tools)?;
        let version = library.version().to_owned();
        Ok(Self {
            pool: MediaInfoPool::new(DiscAware(library), PoolOptions::default()),
            version,
        })
    }
}

impl MediaReader for LibraryReader {
    fn read(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
        self.pool.extract(path)
    }

    fn read_unless(
        &self,
        path: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<MediaSnapshot, MediaInfoError> {
        self.pool.extract_unless(path, cancelled)
    }

    fn version(&self) -> String {
        self.version.clone()
    }
}

/// The reader source for the real MediaInfo library.
#[must_use]
pub fn tools_reader_source(tools: Arc<ToolsManager>) -> ReaderSource {
    Arc::new(move || {
        LibraryReader::from_tools(&tools).map(|reader| Arc::new(reader) as Arc<dyn MediaReader>)
    })
}

/// One title whose videos are read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInfoTarget {
    pub item: i64,
    pub title: String,
}

impl From<PendingMedia> for MediaInfoTarget {
    fn from(pending: PendingMedia) -> Self {
        Self {
            item: pending.item_id.0,
            title: pending.title,
        }
    }
}

impl fmt::Display for MediaInfoTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.title)
    }
}

/// What a media-info job did so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MediaInfoStats {
    /// Titles whose details were stored.
    pub read: u64,
    /// Videos taken from an up-to-date sidecar instead of being read.
    pub from_sidecar: u64,
    /// Sidecars written.
    pub sidecars_written: u64,
    /// Titles whose aspect ratio was measured.
    pub aspect_detected: u64,
    /// ISO images the aspect detector skipped.
    pub iso_skipped: u64,
}

#[derive(Default)]
struct Counters {
    read: AtomicU64,
    from_sidecar: AtomicU64,
    sidecars_written: AtomicU64,
    aspect_detected: AtomicU64,
    iso_skipped: AtomicU64,
    /// The titles whose media info was stored, in the order they finished.
    stored: Mutex<Vec<ItemId>>,
}

/// State shared by the jobs of one session: titles that failed (not tried again until the next
/// launch) and whether the FFmpeg problem was already reported.
#[derive(Default)]
pub struct MediaInfoSession {
    failed: Mutex<HashSet<i64>>,
    ffmpeg_reported: AtomicBool,
}

impl MediaInfoSession {
    /// True when `item` failed earlier in this session.
    #[must_use]
    pub fn has_failed(&self, item: i64) -> bool {
        self.failed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&item)
    }

    /// Titles that failed in this session.
    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.failed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Lets every title that failed be tried again in this session: a scan, a start and Read now
    /// ask for another try.
    pub fn forget_all(&self) {
        self.failed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// Lets `item` be tried again in this session.
    pub fn forget(&self, item: i64) {
        self.failed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&item);
    }

    fn fail(&self, item: i64) {
        self.failed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(item);
    }
}

/// The worker of a media-info job.
#[derive(Clone)]
pub struct MediaInfoWork {
    db: Arc<Database>,
    reader: Arc<dyn MediaReader>,
    tools: Arc<ToolsManager>,
    settings: MediaInfoSettings,
    notices: Arc<NoticeHub>,
    session: Arc<MediaInfoSession>,
    counters: Arc<Counters>,
    /// Read the video itself, whatever a sidecar says (Reload media info).
    force: bool,
    /// Announces the titles whose media info was read, so open pages refresh.
    changes: Changes,
}

/// Why reading a title stopped early.
enum Stop {
    Failed(StageError),
    Cancelled,
}

impl From<cv_db::DbError> for Stop {
    fn from(error: cv_db::DbError) -> Self {
        Self::Failed(StageError::new(format!("database error: {error}")))
    }
}

/// The reason of a video that is not on disk.
const MISSING: &str = "file missing";

/// The reason of a disc folder whose main title gave no streams.
const DISC_UNREADABLE: &str = "disc main title unreadable";

/// Why one video gave no details.
enum ReadFailure {
    /// The short reason for the Activity drawer and the unreadable mark.
    Reason(&'static str),
    /// The job was cancelled while the read waited.
    Cancelled,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Forgets everything media info stored for a video, so it counts as not read again: the
/// container format is empty and the codec, picture, audio and subtitle details are gone. What
/// the scan knows about the file (its place, size, time, ISO and stacking) and its checksums stay.
pub fn clear_media_info(file: &mut MediaFile) {
    file.video_codec.clear();
    file.codec_id.clear();
    file.container_format.clear();
    file.exact_video_format.clear();
    file.video_3d = cv_domain::Video3dFormat::None;
    file.width = 0;
    file.height = 0;
    file.aspect_ratio = 0.0;
    file.aspect_ratio2 = 0.0;
    file.pixel_aspect_ratio = 0.0;
    file.video_bitrate_kbps = 0;
    file.overall_bitrate_kbps = 0;
    file.bit_depth = 0;
    file.frame_rate = 0.0;
    file.duration_secs = 0;
    file.title.clear();
    file.is_animated_graphic = false;
    file.hdr_formats.clear();
    file.hdr_details = cv_domain::HdrDetails::default();
    file.audio_streams.clear();
    file.subtitles.clear();
}

/// Prepares Reload media info for these titles (a show reloads its episodes): the stored media
/// info of every video is cleared, the "unreadable" marks of those videos are removed, and the
/// titles may be tried again in this session. Returns the titles that have a video, ready for a
/// job over [`MediaInfoWork::forced`].
///
/// # Errors
/// The library cannot be read or written.
pub fn reload_titles(
    db: &Database,
    session: &MediaInfoSession,
    ids: &[ItemId],
) -> Result<Vec<MediaInfoTarget>, cv_db::DbError> {
    let ids = ids.to_vec();
    let targets = db.write(move |tx| {
        let mut targets = Vec::new();
        let mut seen = HashSet::new();
        for root in ids {
            for id in tree_ids(tx, root)? {
                if !seen.insert(id.0) {
                    continue;
                }
                let Some(mut item) = get_item(tx, id)? else {
                    continue;
                };
                let mut keys = Vec::new();
                for file in item
                    .core
                    .media_files
                    .iter_mut()
                    .filter(|f| f.kind == MediaFileType::Video)
                {
                    clear_media_info(file);
                    keys.push((file.directory.clone(), file.filename.clone()));
                }
                if keys.is_empty() {
                    continue;
                }
                clear_unreadable(tx, keys)?;
                update_item(tx, &item)?;
                targets.push(MediaInfoTarget {
                    item: id.0,
                    title: item.core.title.clone(),
                });
            }
        }
        Ok(targets)
    })?;
    for target in &targets {
        session.forget(target.item);
    }
    Ok(targets)
}

impl MediaInfoWork {
    pub fn new(
        db: Arc<Database>,
        reader: Arc<dyn MediaReader>,
        tools: Arc<ToolsManager>,
        settings: MediaInfoSettings,
        notices: Arc<NoticeHub>,
        session: Arc<MediaInfoSession>,
    ) -> Self {
        Self {
            db,
            reader,
            tools,
            settings,
            notices,
            session,
            counters: Arc::default(),
            force: false,
            changes: Changes::default(),
        }
    }

    /// Announces the titles whose media info was read to the hub behind `changes`.
    #[must_use]
    pub fn with_changes(mut self, changes: Changes) -> Self {
        self.changes = changes;
        self
    }

    /// The same work, but a video is always read itself: an up-to-date sidecar is not trusted and
    /// is written again. This is what Reload media info runs.
    #[must_use]
    pub fn forced(mut self) -> Self {
        self.force = true;
        self
    }

    /// Counts of what finished so far.
    pub fn stats(&self) -> MediaInfoStats {
        let c = &self.counters;
        MediaInfoStats {
            read: c.read.load(Ordering::Relaxed),
            from_sidecar: c.from_sidecar.load(Ordering::Relaxed),
            sidecars_written: c.sidecars_written.load(Ordering::Relaxed),
            aspect_detected: c.aspect_detected.load(Ordering::Relaxed),
            iso_skipped: c.iso_skipped.load(Ordering::Relaxed),
        }
    }

    /// Reads the unread videos of one title and stores the details. Returns whether anything was
    /// stored; a title that is gone or already complete is not an error.
    fn read_title(&self, id: i64, cancelled: &dyn Fn() -> bool) -> Result<bool, Stop> {
        let Some(item) = self.db.read(|conn| get_item(conn, ItemId(id)))? else {
            return Ok(false);
        };
        let wanted: Vec<MediaFile> = item
            .core
            .media_files
            .iter()
            .filter(|f| {
                f.kind == MediaFileType::Video
                    && !f.has_media_info()
                    // The disc folder is read; its marker file is not a video of its own.
                    && !f.is_covered_disc_marker(&item.core.media_files)
            })
            .cloned()
            .collect();
        if wanted.is_empty() {
            return Ok(false);
        }

        let mut results: Vec<(MediaFile, MediaSnapshot)> = Vec::new();
        let mut first_failure: Option<&'static str> = None;
        let mut marks: Vec<UnreadableMark> = Vec::new();
        for file in &wanted {
            if cancelled() {
                return Err(Stop::Cancelled);
            }
            match self.read_file(file, cancelled) {
                Ok(snapshot) => results.push((file.clone(), snapshot)),
                Err(ReadFailure::Cancelled) => return Err(Stop::Cancelled),
                Err(ReadFailure::Reason(reason)) => {
                    first_failure.get_or_insert(reason);
                    // A file that is not there is for the rescan to deal with; a file that is
                    // there and cannot be read is remembered, so the next launch leaves it alone.
                    if reason != MISSING {
                        marks.push(UnreadableMark {
                            directory: file.directory.clone(),
                            filename: file.filename.clone(),
                            size_bytes: file.size_bytes,
                            modified_ms: file.modified_ms,
                            reason: reason.to_owned(),
                        });
                    }
                }
            }
        }
        if !marks.is_empty() {
            let now = now_ms();
            self.db.write(move |tx| {
                for mark in &marks {
                    mark_unreadable(tx, mark, now)?;
                }
                Ok(())
            })?;
        }
        if results.is_empty() {
            return Err(Stop::Failed(StageError::new(
                first_failure.unwrap_or("media info unreadable"),
            )));
        }

        let detection = self.detect_aspect(&item.core.media_files, &results);
        let measured = detection.is_some();
        let settings = self.settings;
        let stored = self.db.write(move |tx| {
            let Some(mut fresh) = get_item(tx, ItemId(id))? else {
                return Ok(false);
            };
            let mut changed = false;
            for (key, snapshot) in &results {
                let Some(file) = fresh.core.media_files.iter_mut().find(|f| {
                    f.kind == MediaFileType::Video
                        && f.directory == key.directory
                        && f.filename == key.filename
                }) else {
                    continue;
                };
                // The video was replaced while it was being read: leave it to the rescan.
                if file.size_bytes != key.size_bytes || file.modified_ms != key.modified_ms {
                    continue;
                }
                apply_snapshot(file, snapshot);
                if is_image_path(&file.path()) {
                    file.is_iso = true;
                }
                if let Some(detected) = &detection {
                    detected.apply_to(file);
                }
                changed = true;
            }
            if changed && settings.runtime_from_media {
                let runtime_locked =
                    fresh.core.locked || field_locks(tx, ItemId(id))?.contains(MetaField::Runtime);
                let files = &fresh.core.media_files;
                let all_read = files
                    .iter()
                    .filter(|f| f.kind == MediaFileType::Video && !f.is_covered_disc_marker(files))
                    .all(MediaFile::has_media_info);
                if all_read && !runtime_locked {
                    let files = fresh.core.media_files.clone();
                    match &mut fresh.detail {
                        ItemDetail::Movie(m) => {
                            m.runtime_minutes =
                                effective_runtime_minutes(true, &files, m.runtime_minutes);
                        }
                        ItemDetail::Episode(e) => {
                            e.runtime_minutes =
                                effective_runtime_minutes(true, &files, e.runtime_minutes);
                        }
                        _ => {}
                    }
                }
            }
            if changed {
                update_item(tx, &fresh)?;
            }
            Ok(changed)
        })?;
        if first_failure.is_some() {
            // Some part of this title could not be read: do not try it again before the next launch.
            self.session.fail(id);
        }
        if stored {
            self.changes
                .publish_one(ItemId(id), &[ChangePart::Files, ChangePart::Details]);
            self.counters
                .stored
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(ItemId(id));
            self.counters.read.fetch_add(1, Ordering::Relaxed);
            if measured {
                self.counters
                    .aspect_detected
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(stored)
    }

    /// One video: from its sidecar when that is up to date, else read, and then maybe saved as a
    /// sidecar. A disc's marker file is read through its disc folder. The error is the short
    /// reason for the Activity drawer.
    fn read_file(
        &self,
        file: &MediaFile,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<MediaSnapshot, ReadFailure> {
        let stored = file.path();
        if !stored.exists() {
            return Err(ReadFailure::Reason(MISSING));
        }
        let disc = disc_folder_of(&stored).map(Path::to_path_buf);
        let path = disc.clone().unwrap_or(stored);
        let plain = path.is_file() && !is_image_path(&path);
        if plain && !self.force {
            match read_sidecar(&path) {
                Ok(SidecarState::Fresh { snapshot, .. }) => {
                    self.counters.from_sidecar.fetch_add(1, Ordering::Relaxed);
                    return Ok(snapshot);
                }
                Ok(_) => {}
                Err(error) => tracing::debug!(%error, "sidecar not usable"),
            }
        }
        let snapshot = self.reader.read_unless(&path, cancelled).map_err(|error| {
            tracing::debug!(%error, path = %path.display(), "media info not read");
            ReadFailure::Reason(match error {
                MediaInfoError::Cancelled { .. } => return ReadFailure::Cancelled,
                MediaInfoError::Timeout { .. } => TIMED_OUT,
                MediaInfoError::Open { .. } => "file cannot be opened",
                _ if disc.is_some() => DISC_UNREADABLE,
                _ => "media info unreadable",
            })
        })?;
        if !snapshot.has_media_streams() {
            return Err(ReadFailure::Reason(if disc.is_some() {
                DISC_UNREADABLE
            } else {
                "not a recognised media file"
            }));
        }
        if plain {
            match write_sidecar(
                &path,
                &snapshot,
                &self.reader.version(),
                self.settings.store_xml,
            ) {
                Ok(cv_media::mediainfo::SidecarWrite::Written(_)) => {
                    self.counters
                        .sidecars_written
                        .fetch_add(1, Ordering::Relaxed);
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "could not write the media info sidecar"),
            }
        }
        Ok(snapshot)
    }

    /// Measures the picture of the title when the setting is on. Problems become notices.
    fn detect_aspect(
        &self,
        all_files: &[MediaFile],
        results: &[(MediaFile, MediaSnapshot)],
    ) -> Option<AspectDetection> {
        if !self.settings.detect_aspect {
            return None;
        }
        // The read files with their new details (the lengths), then the ones that already had
        // theirs, in the title's own order.
        let parts: Vec<MediaFile> = all_files
            .iter()
            .filter(|f| f.kind == MediaFileType::Video)
            .map(|file| {
                let mut part = file.clone();
                if let Some((_, snapshot)) = results.iter().find(|(key, _)| {
                    key.directory == file.directory && key.filename == file.filename
                }) {
                    apply_snapshot(&mut part, snapshot);
                }
                part
            })
            .collect();
        // A disc folder has no single file to sample.
        if parts
            .iter()
            .any(|p| p.path().is_dir() || disc_folder_of(&p.path()).is_some())
        {
            return None;
        }
        let refs: Vec<&MediaFile> = parts.iter().collect();
        let request = AspectRequest::from_media_files(&refs);
        // Disc images are refused (and counted) even after the FFmpeg problem was reported; every
        // other title would only repeat that problem.
        if self.session.ffmpeg_reported.load(Ordering::Relaxed)
            && !request.parts.iter().any(|part| part.disc_image)
        {
            return None;
        }
        match detect_aspect_ratio(
            &self.tools,
            &request,
            &self.settings.aspect.ard_settings(),
            &Cancel::new(),
        ) {
            Ok(detection) => Some(detection),
            Err(AspectError::IsoNotSupported(path)) => {
                tracing::info!(path = %path.display(), "aspect ratio skipped for an ISO image");
                self.counters.iso_skipped.fetch_add(1, Ordering::Relaxed);
                None
            }
            Err(error @ AspectError::Tool(_)) => {
                // Once per session: every other title would say the same.
                if !self.session.ffmpeg_reported.swap(true, Ordering::Relaxed) {
                    self.notices.publish(
                        AppNotice::new(NoticeLevel::Warning, "Aspect ratio detection needs FFmpeg")
                            .detail(format!(
                                "{error} Download FFmpeg or set its path under Settings, \
                                 External tools. Media info is still stored."
                            ))
                            .settings_link(TOOLS_PAGE)
                            .source("Aspect ratio"),
                    );
                }
                None
            }
            Err(error) => {
                tracing::debug!(%error, "aspect ratio not detected");
                None
            }
        }
    }
}

impl ItemWork for MediaInfoWork {
    type Item = MediaInfoTarget;

    async fn process(
        &self,
        item: &MediaInfoTarget,
        control: &ItemControl,
    ) -> Result<(), ItemError> {
        if control.is_cancelled() {
            return Err(ItemError::Cancelled);
        }
        let work = self.clone();
        let id = item.item;
        let control = control.clone();
        let outcome =
            tokio::task::spawn_blocking(move || work.read_title(id, &|| control.is_cancelled()))
                .await;
        match outcome {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(Stop::Cancelled)) => Err(ItemError::Cancelled),
            Ok(Err(Stop::Failed(failure))) => {
                self.session.fail(id);
                Err(failure.into())
            }
            Err(error) => {
                tracing::error!(%error, "media info worker stopped");
                self.session.fail(id);
                Err(StageError::new("media info worker stopped").into())
            }
        }
    }
}

/// A submitted media-info job and what it has done.
#[derive(Clone)]
pub struct MediaInfoJob {
    pub job: JobHandle,
    work: MediaInfoWork,
}

impl MediaInfoJob {
    pub fn stats(&self) -> MediaInfoStats {
        self.work.stats()
    }

    /// The titles whose media info the job stored so far.
    #[must_use]
    pub fn stored(&self) -> Vec<ItemId> {
        self.work
            .counters
            .stored
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Queues a media-info job over `targets` on the disk lane, `parallelism` titles at a time.
/// Returns `None` when there is nothing to read. Must be called inside a tokio runtime.
pub fn submit_media_info(
    manager: &JobManager,
    work: MediaInfoWork,
    targets: Vec<MediaInfoTarget>,
    parallelism: usize,
) -> Option<MediaInfoJob> {
    submit_named(manager, false, work, targets, parallelism)
}

/// As [`submit_media_info`]; `reload` names the job as a second, forced read.
pub fn submit_named(
    manager: &JobManager,
    reload: bool,
    work: MediaInfoWork,
    targets: Vec<MediaInfoTarget>,
    parallelism: usize,
) -> Option<MediaInfoJob> {
    if targets.is_empty() {
        return None;
    }
    let name = crate::wording::job::reading_media_info(targets.len(), reload);
    let spec = JobSpec::new(name, LaneId::Disk).concurrency(parallelism);
    let job = manager.submit(spec, work.clone(), targets);
    Some(MediaInfoJob { job, work })
}

/// Titles read at the same time when the caller does not choose: one per core.
#[must_use]
pub fn default_parallelism() -> usize {
    std::thread::available_parallelism().map_or(2, usize::from)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn values(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn a_fresh_install_samples_normally_and_does_not_round_up() {
        let settings = MediaInfoSettings::from_values(&BTreeMap::new());
        assert!(!settings.detect_aspect);
        assert_eq!(settings.aspect, AspectOptions::default());
        let ard = settings.aspect.ard_settings();
        assert_eq!(ard, ArdSettings::default());
    }

    #[test]
    fn the_aspect_choices_reach_the_detection_settings() {
        let settings = MediaInfoSettings::from_values(&values(&[
            (ASPECT_MODE_KEY, json!("accurate")),
            (ASPECT_ROUND_UP_KEY, json!(true)),
            (ASPECT_MULTI_FORMAT_KEY, json!("wider")),
        ]));
        let ard = settings.aspect.ard_settings();
        assert_eq!(ard.mode, ArdMode::Accurate);
        assert!(ard.round_up);
        assert_eq!(ard.multi_format, MultiFormat::Wider);
        // The percentages and the ratio list are not settings and keep their defaults.
        assert_eq!(ard.ratios, ArdSettings::default().ratios);

        let fast = MediaInfoSettings::from_values(&values(&[
            (ASPECT_MODE_KEY, json!("fast")),
            (ASPECT_MULTI_FORMAT_KEY, json!("higher")),
        ]));
        assert_eq!(fast.aspect.mode, ArdMode::Fast);
        assert_eq!(fast.aspect.multi_format, MultiFormat::Higher);
    }

    #[test]
    fn an_unknown_or_mistyped_aspect_value_takes_the_default() {
        let settings = MediaInfoSettings::from_values(&values(&[
            (ASPECT_MODE_KEY, json!("turbo")),
            (ASPECT_ROUND_UP_KEY, json!("yes")),
            (ASPECT_MULTI_FORMAT_KEY, json!(3)),
        ]));
        assert_eq!(settings.aspect, AspectOptions::default());
    }
}

//! The drives the library lives on (spec `drives`): per drive its name, kind, size and
//! free space, the library folders on it with their titles and bytes, the space that is not the
//! library's, the resolution mix, whether it answers and when it last did.
//!
//! # Which drive a folder is on
//!
//! A library folder belongs to the volume that holds it: a Windows drive letter (`D:`), a share
//! (`\\nas\media`), a macOS mount point (`/Volumes/Media`) or a Linux mount point
//! (`/media/alex/Films`, `/run/media/alex/Films`, `/mnt/nas`). Two folders on one volume are one
//! drive with two folders. The numbers come from the operating system ([`SystemVolumes`]); a share
//! or mapped drive the system list does not show is asked for its space directly.
//!
//! # Low on space
//!
//! A drive is low when less than 5 percent of it or less than 50 GB is free (spec `drives`
//! "Low space rule"): 41 GB of 931 GB is low, 60 GB of 931 GB is not, and 400 GB of a 10.9 TB
//! drive is low. An offline drive is not judged.
//!
//! # Offline
//!
//! A drive is online when at least one of its library folders answers the source health check
//! ([`crate::scanning::ScanService::source_health`], which also stamps every folder that answers
//! with the time, `data_source.last_seen_ms`). An offline drive keeps its folders and titles and
//! says when it was last seen.
//!
//! # Telling the interface
//!
//! [`DrivesService::notify_changed`] announces that the numbers moved (a scan, a move or a delete
//! ended, a drive came or went) to the listeners, at most once in ten seconds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cv_db::repo::data_sources::list_sources;
use cv_db::repo::drive_facts::{
    SourceQuality, SourceUsage, source_key, source_quality, source_usage, sources_last_seen,
};
use cv_db::{Database, DbError};
use cv_domain::{DataSource, SourceKind};
use sysinfo::{DiskKind, Disks};

use crate::scanning::{ScanService, SourceState};

/// A drive with less free space than this is low whatever its size.
pub const LOW_FREE_BYTES: u64 = 50_000_000_000;

/// "Not in library" is shown from this much.
pub const NOT_IN_LIBRARY_FROM: u64 = 1_000_000_000;

/// Announcements closer together than this are joined into one: a scan changes files all the
/// time, and reading the disks for every change would cost more than it tells.
pub const NOTIFY_WINDOW: Duration = Duration::from_secs(10);

/// What kind of device a drive is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveKind {
    Hdd,
    Ssd,
    Network,
    Removable,
}

/// One library folder on a drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFolder {
    pub source_id: i64,
    pub path: String,
    /// The folder's label ("Movies", "NAS 4K").
    pub label: String,
    pub kind: SourceKind,
    /// Movies, or shows for a TV folder.
    pub titles: u64,
    pub bytes: u64,
}

/// How many titles of a drive fall in each resolution class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QualityCounts {
    pub p2160: u64,
    pub p1080: u64,
    pub p720: u64,
    pub sd: u64,
}

impl QualityCounts {
    fn add(&mut self, other: &SourceQuality) {
        self.p2160 += other.p2160;
        self.p1080 += other.p1080;
        self.p720 += other.p720;
        self.sd += other.sd;
    }
}

/// One drive that holds a library folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    /// Stable: the drive letter (`D:`), the share or the mount point.
    pub id: String,
    /// `D: Media`: the letter or share, then the volume label.
    pub name: String,
    pub kind: DriveKind,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub folders: Vec<DriveFolder>,
    /// Used space that is not library titles; `None` under 1 GB, offline or unknown.
    pub not_in_library_bytes: Option<u64>,
    pub quality: QualityCounts,
    pub online: bool,
    /// When an offline drive was last reachable, epoch milliseconds.
    pub last_seen_ms: Option<i64>,
    pub low: bool,
    /// Bytes in the trash folders (`.deletedByCineVault`) of its library folders.
    pub trash_bytes: u64,
    /// Bytes CineVault sent to the Recycle Bin / Trash from this drive that are still there.
    pub bin_bytes: u64,
    /// The custom trash folder of Settings › Deleted files when it is on this drive, with its
    /// bytes.
    pub custom_trash: Option<(String, u64)>,
}

/// What a drive's Empty would erase (the Drives page's confirmation lists it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrashContents {
    /// Batches of the trash folders, biggest first.
    pub batches: Vec<crate::trash_folders::TrashBatch>,
    /// Paths CineVault sent to the Recycle Bin / Trash that are still there.
    pub bin: Vec<crate::trash_folders::BinEntry>,
}

impl TrashContents {
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.batches.iter().map(|b| b.bytes).sum::<u64>()
            + self.bin.iter().map(|b| b.item.size).sum::<u64>()
    }
}

/// The drives of the library and when the numbers were read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrivesOverview {
    pub drives: Vec<Drive>,
    pub updated_ms: i64,
}

/// Whether a drive is low on space: under 5 percent or under 50 GB free.
#[must_use]
pub fn is_low(total_bytes: u64, free_bytes: u64) -> bool {
    if total_bytes == 0 {
        return false;
    }
    // Integer arithmetic: free * 100 < total * 5, without overflow for any real disk.
    u128::from(free_bytes) * 100 < u128::from(total_bytes) * 5 || free_bytes < LOW_FREE_BYTES
}

// ---- volumes -----------------------------------------------------------------------------------

/// The volume a path is on, as the operating system reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// Groups folders: `D:`, `\\nas\media` or `/Volumes/Media`.
    pub key: String,
    /// The volume label, empty when it has none.
    pub label: String,
    pub kind: DriveKind,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// Where the sizes of volumes come from.
pub trait Volumes: Send + Sync {
    /// Reads the volumes again. Called once per overview.
    fn refresh(&self) {}

    /// The volume that holds `path`; `None` when it cannot be reached.
    fn locate(&self, path: &str) -> Option<Volume>;
}

/// The volume a folder is on, by its path alone: used for a folder whose drive is not there.
#[must_use]
pub fn volume_key(path: &str) -> String {
    let text = path.trim();
    let unc = text.strip_prefix(r"\\").or_else(|| text.strip_prefix("//"));
    if let Some(rest) = unc {
        let mut parts = rest.split(['\\', '/']).filter(|p| !p.is_empty());
        let server = parts.next().unwrap_or_default();
        return match parts.next() {
            Some(share) => format!(r"\\{server}\{share}"),
            None => format!(r"\\{server}"),
        };
    }
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return format!("{}:", char::from(bytes[0]).to_ascii_uppercase());
    }
    if let Some(rest) = text.strip_prefix("/Volumes/") {
        let name = rest.split('/').next().unwrap_or_default();
        return format!("/Volumes/{name}");
    }
    // A Linux mount point (`/media/<user>/<name>`, `/run/media/<user>/<name>`, `/mnt/<name>`) is
    // its own drive. Any other path is on the root drive.
    let mount = cv_fs::paths::linux_mount_root(text);
    if mount != "/" {
        return mount;
    }
    "/".to_owned()
}

/// Whether a key is a network share (`\\server\share`).
fn is_share(key: &str) -> bool {
    key.starts_with(r"\\")
}

/// The name of a Linux mount. The device is shown when it has a name of its own (a share such as
/// `server:/export`); a device path such as `/dev/sdb1` says nothing a person reads, so the last
/// part of the mount point is used instead (`/media/alex/Films` is "Films").
fn linux_label(device: &str, mount: &str) -> String {
    if device.is_empty() || device.starts_with("/dev/") {
        mount
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or_default()
            .to_owned()
    } else {
        device.to_owned()
    }
}

/// The comparison key of a drive id or volume key. Case is kept on Linux, where `Films` and
/// `films` are two mount points, and folded on Windows and macOS.
fn drive_key(text: &str) -> String {
    cv_fs::paths::key(Path::new(text))
}

/// The volumes of this computer from the operating system, with a direct question for a share or
/// mapped drive its list does not show.
pub struct SystemVolumes {
    disks: Mutex<Disks>,
}

impl SystemVolumes {
    #[must_use]
    pub fn new() -> Self {
        Self {
            disks: Mutex::new(Disks::new_with_refreshed_list()),
        }
    }
}

impl Default for SystemVolumes {
    fn default() -> Self {
        Self::new()
    }
}

fn network_file_system(name: &str) -> bool {
    let name = name.to_lowercase();
    ["nfs", "smb", "cifs", "afp", "webdav", "sshfs", "9p"]
        .iter()
        .any(|kind| name.contains(kind))
}

impl Volumes for SystemVolumes {
    fn refresh(&self) {
        self.disks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh(true);
    }

    fn locate(&self, path: &str) -> Option<Volume> {
        let key = volume_key(path);
        let target = Path::new(path);
        {
            let disks = self.disks.lock().unwrap_or_else(PoisonError::into_inner);
            let found = disks
                .list()
                .iter()
                .filter(|disk| cv_fs::paths::is_within(target, disk.mount_point()))
                .max_by_key(|disk| disk.mount_point().as_os_str().len());
            if let Some(disk) = found {
                let kind = if network_file_system(&disk.file_system().to_string_lossy()) {
                    DriveKind::Network
                } else if disk.is_removable() {
                    DriveKind::Removable
                } else if matches!(disk.kind(), DiskKind::SSD) {
                    DriveKind::Ssd
                } else {
                    DriveKind::Hdd
                };
                // Windows and macOS name a volume by its label; Linux names it by its device, so
                // the mount's own name stands in for the label.
                let label = if cfg!(any(windows, target_os = "macos")) {
                    disk.name().to_string_lossy().into_owned()
                } else {
                    linux_label(
                        &disk.name().to_string_lossy(),
                        &disk.mount_point().to_string_lossy(),
                    )
                };
                // The key of a mount point on macOS is the mount; on Windows the drive letter.
                let key = if cfg!(windows) || is_share(&key) {
                    key
                } else {
                    disk.mount_point().to_string_lossy().into_owned()
                };
                return Some(Volume {
                    key,
                    label,
                    kind,
                    total_bytes: disk.total_space(),
                    free_bytes: disk.available_space(),
                });
            }
        }
        // A share or a mapped drive: the system list does not show it. Ask for its space.
        let total = fs4::total_space(target).ok()?;
        let free = fs4::available_space(target).ok()?;
        Some(Volume {
            key,
            label: String::new(),
            kind: DriveKind::Network,
            total_bytes: total,
            free_bytes: free,
        })
    }
}

// ---- which folders answer -----------------------------------------------------------------------

/// Whether each library folder answers, by data source id.
pub trait SourceStates: Send + Sync {
    /// # Errors
    /// The library cannot be read.
    fn states(&self) -> Result<HashMap<i64, bool>, DbError>;
}

impl SourceStates for ScanService {
    fn states(&self) -> Result<HashMap<i64, bool>, DbError> {
        Ok(self
            .source_health()?
            .into_iter()
            .map(|health| (health.source.id, health.state == SourceState::Online))
            .collect())
    }
}

// ---- the service --------------------------------------------------------------------------------

type Listener = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Throttle {
    last: Option<Instant>,
    trailing: bool,
}

/// Reads the drives of the library.
pub struct DrivesService {
    db: Arc<Database>,
    volumes: Arc<dyn Volumes>,
    states: Arc<dyn SourceStates>,
    listeners: Mutex<Vec<Listener>>,
    throttle: Mutex<Throttle>,
    /// How long announcements are joined for.
    window: Duration,
    /// Which drives answered at the last overview, to notice a drive coming or going.
    seen: Mutex<Option<Vec<(String, bool)>>>,
    /// The bin CineVault sends to (to find what it sent there) and Settings › Deleted files.
    trash: Mutex<TrashSetup>,
    me: std::sync::Weak<DrivesService>,
}

type TrashSetup = (
    Arc<dyn crate::library_ops::SystemBin>,
    Arc<dyn crate::deleted_files::DeletedFilesSource>,
);

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl DrivesService {
    /// The service over the library, the operating system's volumes and the scan service's health
    /// check.
    #[must_use]
    pub fn new(db: Arc<Database>, scan: Arc<ScanService>) -> Arc<Self> {
        Self::with(db, Arc::new(SystemVolumes::new()), scan)
    }

    /// The service over any volumes and folder states, for tests.
    #[must_use]
    pub fn with(
        db: Arc<Database>,
        volumes: Arc<dyn Volumes>,
        states: Arc<dyn SourceStates>,
    ) -> Arc<Self> {
        Self::with_window(db, volumes, states, NOTIFY_WINDOW)
    }

    /// Like [`Self::with`], joining announcements for `window`.
    #[must_use]
    pub fn with_window(
        db: Arc<Database>,
        volumes: Arc<dyn Volumes>,
        states: Arc<dyn SourceStates>,
        window: Duration,
    ) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            db,
            volumes,
            states,
            listeners: Mutex::new(Vec::new()),
            throttle: Mutex::new(Throttle::default()),
            window,
            seen: Mutex::new(None),
            trash: Mutex::new((
                Arc::new(crate::library_ops::FolderOnly),
                crate::deleted_files::default_source(),
            )),
            me: me.clone(),
        })
    }

    /// Finds what CineVault sent to `bin` and reads Settings › Deleted files from `choice` (the
    /// app gives the real bin; without it only trash folders are listed).
    pub fn set_trash(
        &self,
        bin: Arc<dyn crate::library_ops::SystemBin>,
        choice: Arc<dyn crate::deleted_files::DeletedFilesSource>,
    ) {
        *locked(&self.trash) = (bin, choice);
    }

    /// Calls `listener` whenever [`Self::notify_changed`] announces new numbers.
    pub fn on_changed(&self, listener: Listener) {
        locked(&self.listeners).push(listener);
    }

    /// Announces that the numbers moved. The first announcement goes out at once; those that
    /// follow within a couple of seconds are joined into one that goes out when the window ends.
    pub fn notify_changed(&self) {
        let wait = {
            let mut throttle = locked(&self.throttle);
            match throttle.last {
                Some(last) if last.elapsed() < self.window => {
                    if throttle.trailing {
                        return;
                    }
                    throttle.trailing = true;
                    Some(self.window - last.elapsed())
                }
                _ => {
                    throttle.last = Some(Instant::now());
                    None
                }
            }
        };
        match wait {
            None => self.announce(),
            Some(wait) => {
                let me = self.me.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(wait);
                    if let Some(service) = me.upgrade() {
                        {
                            let mut throttle = locked(&service.throttle);
                            throttle.trailing = false;
                            throttle.last = Some(Instant::now());
                        }
                        service.announce();
                    }
                });
            }
        }
    }

    fn announce(&self) {
        let listeners = locked(&self.listeners).clone();
        for listener in listeners {
            listener();
        }
    }

    /// The drives that hold a library folder, their space and what is on them. Reads the disks:
    /// call it off the UI thread.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn overview(&self) -> Result<DrivesOverview, DbError> {
        self.volumes.refresh();
        let sources = self.db.read(|c| list_sources(c, None))?;
        let (usage, quality, seen_at) = self
            .db
            .read(|c| Ok((source_usage(c)?, source_quality(c)?, sources_last_seen(c)?)))?;
        let answering = self.states.states()?;
        let mut drives = group(
            &sources,
            &Facts {
                usage: &usage,
                quality: &quality,
                last_seen: &seen_at,
                answering: &answering,
            },
            self.volumes.as_ref(),
        );
        self.add_trash(&mut drives);
        let signature: Vec<(String, bool)> = drives
            .iter()
            .map(|drive| (drive.id.clone(), drive.online))
            .collect();
        let moved = {
            let mut seen = locked(&self.seen);
            let moved = seen.as_ref().is_some_and(|before| *before != signature);
            *seen = Some(signature);
            moved
        };
        if moved {
            // A drive came or went: the sidebar and the Drives page read it again.
            self.notify_changed();
        }
        Ok(DrivesOverview {
            drives,
            updated_ms: now_ms(),
        })
    }
}

// ---- trash ---------------------------------------------------------------------------------------

impl DrivesService {
    fn key_of(&self, path: &str) -> String {
        let volume = self
            .volumes
            .locate(path)
            .map_or_else(|| volume_key(path), |v| v.key);
        drive_key(&volume)
    }

    fn custom_folder(&self) -> Option<PathBuf> {
        let choice = locked(&self.trash).1.deleted_files();
        choice.custom_folder().map(Path::to_path_buf)
    }

    /// The trash folders and recorded bin items of every drive, or of the drive `id`.
    fn trash_of(&self, drives: &[Drive], id: Option<&str>) -> Result<TrashContents, DbError> {
        let wanted = |drive: &Drive| {
            drive.online && id.is_none_or(|id| drive_key(&drive.id) == drive_key(id))
        };
        let mut dirs: Vec<PathBuf> = Vec::new();
        for drive in drives.iter().filter(|d| wanted(d)) {
            let roots: Vec<PathBuf> = drive
                .folders
                .iter()
                .map(|f| PathBuf::from(&f.path))
                .collect();
            dirs.extend(crate::trash_folders::trash_dirs(&roots));
        }
        if let Some(custom) = self.custom_folder() {
            let key = self.key_of(&custom.to_string_lossy());
            // Only CineVault's own sub-folder of the custom folder, never its other entries.
            let own = cv_fs::volume::custom_trash_root(&custom);
            if drives.iter().any(|d| wanted(d) && drive_key(&d.id) == key)
                && !dirs.iter().any(|d| cv_fs::volume::inside(d, &own))
            {
                dirs.extend(crate::trash_folders::trash_dirs(&[custom]));
            }
        }
        let batches = dirs
            .iter()
            .flat_map(|dir| crate::trash_folders::batches(dir))
            .collect();
        let bin = locked(&self.trash).0.clone();
        let keys: Vec<String> = drives
            .iter()
            .filter(|d| wanted(d))
            .map(|d| drive_key(&d.id))
            .collect();
        let recorded: Vec<crate::trash_folders::BinEntry> =
            crate::trash_folders::recorded_bin_items(&self.db)?
                .into_iter()
                .filter(|e| keys.contains(&self.key_of(&e.item.original)))
                .collect();
        let bin = crate::trash_folders::present_bin_items(bin.as_ref(), recorded);
        Ok(TrashContents { batches, bin })
    }

    /// Fills in the trash numbers of each drive. A failure leaves them at zero.
    fn add_trash(&self, drives: &mut [Drive]) {
        let contents = match self.trash_of(drives, None) {
            Ok(contents) => contents,
            Err(error) => {
                tracing::warn!(%error, "the trash of the drives could not be read");
                return;
            }
        };
        let custom = self.custom_folder();
        // The custom folder is named on its drive even while it is empty.
        if let Some(custom) = &custom {
            let key = self.key_of(&custom.to_string_lossy());
            if let Some(drive) = drives
                .iter_mut()
                .find(|d| d.online && drive_key(&d.id) == key)
            {
                drive.custom_trash = Some((custom.to_string_lossy().into_owned(), 0));
            }
        }
        for batch in &contents.batches {
            let key = self.key_of(&batch.trash.to_string_lossy());
            let is_custom = custom.as_ref().is_some_and(|c| {
                cv_fs::volume::inside(&batch.trash, &cv_fs::volume::custom_trash_root(c))
            });
            if let Some(drive) = drives.iter_mut().find(|d| drive_key(&d.id) == key) {
                if is_custom {
                    if let Some((_, bytes)) = drive.custom_trash.as_mut() {
                        *bytes += batch.bytes;
                    }
                } else {
                    drive.trash_bytes += batch.bytes;
                }
            }
        }
        for entry in &contents.bin {
            let key = self.key_of(&entry.item.original);
            if let Some(drive) = drives.iter_mut().find(|d| drive_key(&d.id) == key) {
                drive.bin_bytes += entry.item.size;
            }
        }
    }

    fn drives_now(&self) -> Result<Vec<Drive>, DbError> {
        self.volumes.refresh();
        let sources = self.db.read(|c| list_sources(c, None))?;
        let (usage, quality, seen_at) = self
            .db
            .read(|c| Ok((source_usage(c)?, source_quality(c)?, sources_last_seen(c)?)))?;
        let answering = self.states.states()?;
        Ok(group(
            &sources,
            &Facts {
                usage: &usage,
                quality: &quality,
                last_seen: &seen_at,
                answering: &answering,
            },
            self.volumes.as_ref(),
        ))
    }

    /// What Empty would erase on the drive `id`, or on every drive.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn trash_contents(&self, id: Option<&str>) -> Result<TrashContents, DbError> {
        let drives = self.drives_now()?;
        self.trash_of(&drives, id)
    }

    /// Erases the trash folders and the recorded bin items of the drive `id` (every drive when
    /// `None`), after the typed confirmation. History follows (see [`crate::trash_folders`]).
    ///
    /// # Errors
    /// The confirmation is not exactly `DELETE` (nothing is touched), or the library cannot be
    /// read.
    pub fn empty_trash(
        &self,
        id: Option<&str>,
        confirm: &str,
    ) -> Result<crate::trash_folders::EmptyReport, EmptyError> {
        if confirm != crate::trash_folders::EMPTY_CONFIRMATION {
            return Err(EmptyError::Confirmation);
        }
        let contents = self.trash_contents(id)?;
        let custom = self.custom_folder();
        let mut report = crate::trash_folders::EmptyReport::default();
        let mut emptied = Vec::new();
        for batch in &contents.batches {
            match crate::trash_folders::erase_batch(&batch.trash, &batch.name, custom.as_deref()) {
                Ok(bytes) => {
                    report.bytes += bytes;
                    report.batches += 1;
                    emptied.push(batch.path());
                }
                Err(error) => {
                    // Partly erased: History follows, so no Undo brings back half a title.
                    if crate::trash_folders::partly_erased(batch) {
                        emptied.push(batch.path());
                    }
                    report.problems.push((
                        batch.path().to_string_lossy().into_owned(),
                        error.to_string(),
                    ));
                }
            }
        }
        let mut purged = Vec::new();
        if !contents.bin.is_empty() {
            let bin = locked(&self.trash).0.clone();
            let items: Vec<crate::library_ops::BinnedItem> =
                contents.bin.iter().map(|e| e.item.clone()).collect();
            let failed = bin.purge(&items).err();
            // What left the bin counts as erased even when the purge stopped part way, so
            // History switches off the Undo of exactly those entries.
            for (item, here) in items.iter().zip(bin.present(&items)) {
                if here {
                    if failed.is_none() {
                        report.problems.push((
                            item.original.clone(),
                            "It is still in the Recycle Bin".to_owned(),
                        ));
                    }
                } else {
                    report.bytes += item.size;
                    report.bin_items += 1;
                    purged.push(item.bin_id.clone());
                }
            }
            if let Some(error) = failed {
                report.problems.push((
                    String::new(),
                    format!("The Recycle Bin could not be emptied: {error}"),
                ));
            }
        }
        report.entries = crate::trash_folders::follow_in_history(&self.db, &emptied, &purged)?;
        if report.bytes > 0 || report.batches > 0 || report.bin_items > 0 {
            self.notify_changed();
        }
        Ok(report)
    }

    /// Settings › Deleted files "Empty trash folders automatically": erases what has been in
    /// CineVault's own trash folders longer than the chosen days. Nothing when it is off; never
    /// the system bin.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn auto_empty(&self) -> Result<Option<crate::trash_folders::EmptyReport>, DbError> {
        let choice = locked(&self.trash).1.deleted_files();
        let Some(days) = choice.auto_empty_days else {
            return Ok(None);
        };
        let sources = self.db.read(|c| list_sources(c, None))?;
        let roots: Vec<PathBuf> = sources.iter().map(|s| PathBuf::from(&s.path)).collect();
        let custom = choice.custom_folder().map(Path::to_path_buf);
        // The custom folder's own `.deletedByCineVault`, never the chosen folder's other entries.
        let mut with_custom = roots;
        with_custom.extend(custom.iter().cloned());
        let dirs = crate::trash_folders::trash_dirs(&with_custom);
        let report = crate::trash_folders::empty_older_than(
            &self.db,
            &dirs,
            custom.as_deref(),
            days,
            SystemTime::now(),
        );
        if report.batches > 0 {
            self.notify_changed();
        }
        Ok(Some(report))
    }
}

/// Why an Empty did not run.
#[derive(Debug, thiserror::Error)]
pub enum EmptyError {
    #[error("Type DELETE to empty the trash. Nothing was erased.")]
    Confirmation,
    #[error("the library could not be read: {0}")]
    Db(#[from] DbError),
}

/// What the library knows about the folders.
struct Facts<'a> {
    usage: &'a HashMap<String, SourceUsage>,
    quality: &'a HashMap<String, SourceQuality>,
    last_seen: &'a HashMap<i64, i64>,
    answering: &'a HashMap<i64, bool>,
}

/// Groups the library folders into drives, in the order their first folder is listed.
fn group(sources: &[DataSource], facts: &Facts<'_>, volumes: &dyn Volumes) -> Vec<Drive> {
    let mut drives: Vec<Drive> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for source in sources {
        let volume = volumes.locate(&source.path);
        let key = volume
            .as_ref()
            .map_or_else(|| volume_key(&source.path), |v| v.key.clone());
        let at = *by_key.entry(drive_key(&key)).or_insert_with(|| {
            drives.push(new_drive(&key, volume.as_ref()));
            drives.len() - 1
        });
        let drive = &mut drives[at];
        // The usage and quality tables are keyed like this in cv-db: folded to lower case where
        // file names ignore case, as stored on Linux.
        let root = source_key(&source.path);
        let usage = facts.usage.get(&root).copied().unwrap_or_default();
        drive.folders.push(DriveFolder {
            source_id: source.id,
            path: source.path.clone(),
            label: source.label.clone(),
            kind: source.kind,
            titles: usage.titles,
            bytes: usage.bytes,
        });
        if let Some(quality) = facts.quality.get(&root) {
            drive.quality.add(quality);
        }
        // A drive answers when one of its folders does.
        if facts.answering.get(&source.id).copied().unwrap_or(false) && volume.is_some() {
            drive.online = true;
        }
        if let Some(seen) = facts.last_seen.get(&source.id) {
            drive.last_seen_ms = Some(drive.last_seen_ms.map_or(*seen, |before| before.max(*seen)));
        }
    }
    for drive in &mut drives {
        if drive.online {
            // Used space that no library title accounts for.
            let library: u64 = drive.folders.iter().map(|f| f.bytes).sum();
            let used = drive.total_bytes.saturating_sub(drive.free_bytes);
            drive.not_in_library_bytes =
                Some(used.saturating_sub(library)).filter(|bytes| *bytes >= NOT_IN_LIBRARY_FROM);
            drive.low = is_low(drive.total_bytes, drive.free_bytes);
            // Only an offline drive says when it was last seen.
            drive.last_seen_ms = None;
        }
    }
    drives
}

fn new_drive(key: &str, volume: Option<&Volume>) -> Drive {
    let (kind, total_bytes, free_bytes, label) = match volume {
        Some(v) => (v.kind, v.total_bytes, v.free_bytes, v.label.trim()),
        None => (
            if is_share(key) {
                DriveKind::Network
            } else {
                DriveKind::Hdd
            },
            0,
            0,
            "",
        ),
    };
    let name = if label.is_empty() || label == key {
        key.to_owned()
    } else if key.starts_with('/') {
        // A mount point is shown by its volume name.
        label.to_owned()
    } else {
        format!("{key} {label}")
    };
    Drive {
        id: key.to_owned(),
        name,
        kind,
        total_bytes,
        free_bytes,
        folders: Vec::new(),
        not_in_library_bytes: None,
        quality: QualityCounts::default(),
        online: false,
        last_seen_ms: None,
        low: false,
        trash_bytes: 0,
        bin_bytes: 0,
        custom_trash: None,
    }
}

#[cfg(test)]
mod tests;

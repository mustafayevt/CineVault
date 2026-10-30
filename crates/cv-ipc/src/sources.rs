//! The library folder types the read pages share: which kind a folder is and the label it shows.
//!
//! The rules live in `cv_db::repo::data_sources`; this module keeps the DTO and the one lookup the
//! title pages use, so the movie and the show page read a folder's label the same way.

use std::sync::Arc;

use cv_app::changes::ChangeHub;
use cv_app::data_location::DataLocation;
use cv_app::data_sources::{DataSourceError, DataSourceService};
use cv_app::lifecycle::LifecycleService;
use cv_app::organize::OrganizeStateService;
use cv_app::rating_status::RatingStatusService;
use cv_app::scanning::{ScanService, SourceHealth, SourceState};
use cv_app::services::AppServices;
use cv_db::Database;
use cv_domain::{DataSource, SourceKind};
use cv_scan::sources::SourceGuard;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Which library a folder feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SourceKindDto {
    Movie,
    Tv,
}

impl From<SourceKindDto> for SourceKind {
    fn from(kind: SourceKindDto) -> Self {
        match kind {
            SourceKindDto::Movie => Self::Movie,
            SourceKindDto::Tv => Self::Tv,
        }
    }
}

impl From<SourceKind> for SourceKindDto {
    fn from(kind: SourceKind) -> Self {
        match kind {
            SourceKind::Movie => Self::Movie,
            SourceKind::Tv => Self::Tv,
        }
    }
}

/// The label of the library folder at `root` ("NAS 4K"), or empty when the folder is gone.
pub fn source_label_of(db: &Database, kind: SourceKind, root: &str) -> String {
    if root.is_empty() {
        return String::new();
    }
    db.read(|conn| cv_db::repo::data_sources::label_of_root(conn, kind, root))
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// One library folder with its live state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SourceHealthDto {
    pub id: u32,
    pub kind: SourceKindDto,
    pub path: String,
    /// False while the drive is disconnected or the share is down; its titles stay in the library.
    pub online: bool,
    /// Movies, or shows for a TV folder.
    pub titles: u32,
    /// Titles whose folder or video is gone while the library folder itself is reachable.
    pub missing: u32,
    /// Titles still waiting for their media info.
    pub pending_media_info: u32,
}

fn small(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl From<SourceHealth> for SourceHealthDto {
    fn from(health: SourceHealth) -> Self {
        Self {
            id: u32::try_from(health.source.id).unwrap_or(u32::MAX),
            kind: health.source.kind.into(),
            path: health.source.path,
            online: health.state == SourceState::Online,
            titles: small(health.counts.titles),
            missing: small(health.counts.missing),
            pending_media_info: small(health.counts.pending_media_info),
        }
    }
}

/// Every library folder with whether it is reachable and what it holds, for the sidebar.
pub async fn source_health(service: Arc<ScanService>) -> Result<Vec<SourceHealthDto>, String> {
    cv_jobs::client::spawn_blocking(move || service.source_health())
        .await
        .map_err(|error| error.to_string())?
        .map(|rows| rows.into_iter().map(Into::into).collect())
        .map_err(|error| error.to_string())
}

/// The one library the commands work on. When the services could not start, the window still
/// opens and every command answers with a `storage` error that says why.
pub struct LibraryState {
    service: Result<DataSourceService, String>,
    location: Option<DataLocation>,
    /// Told when the list of folders changes, so the folder watcher follows it.
    scan: Option<Arc<ScanService>>,
    /// Told which titles a rename or an undo touched, so their name check is recomputed.
    organize_state: Option<Arc<OrganizeStateService>>,
    /// The stage and flags of titles, and the pipeline counts.
    lifecycle: Option<Arc<LifecycleService>>,
    /// The chips of the ratings strip.
    rating_status: Option<Arc<RatingStatusService>>,
    /// Told what changed for which titles, so open pages refresh.
    changes: Option<Arc<ChangeHub>>,
    /// Settings › Deleted files: where deleted and replaced files go.
    settings: Option<Arc<cv_app::app_settings::SettingsService>>,
}

impl LibraryState {
    /// Library folders over the services' shared database. The app's own data folder is never a
    /// library folder.
    pub fn from_services(services: &Arc<AppServices>) -> Self {
        let guard = SourceGuard::for_current_os(&[services
            .location()
            .root()
            .to_string_lossy()
            .into_owned()]);
        Self::with_guard(services, guard)
    }

    /// Library folders checked by `guard`. A test needs one that lets the temp folder through.
    pub fn with_guard(services: &Arc<AppServices>, guard: SourceGuard) -> Self {
        let location = services.location().clone();
        Self {
            service: Ok(DataSourceService::new(Arc::clone(services.db()), guard)),
            location: Some(location),
            scan: Some(Arc::clone(services.scan())),
            organize_state: Some(Arc::clone(services.organize_state())),
            lifecycle: Some(Arc::clone(services.lifecycle())),
            rating_status: Some(Arc::clone(services.rating_status())),
            changes: Some(Arc::clone(services.changes())),
            settings: Some(Arc::clone(services.settings())),
        }
    }

    /// State for when the library could not be opened.
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            service: Err(message.into()),
            location: None,
            scan: None,
            organize_state: None,
            lifecycle: None,
            rating_status: None,
            changes: None,
            settings: None,
        }
    }

    /// State around an already built service (tests).
    #[cfg(test)]
    pub fn with_service(service: DataSourceService) -> Self {
        Self {
            service: Ok(service),
            location: None,
            scan: None,
            organize_state: None,
            lifecycle: None,
            rating_status: None,
            changes: None,
            settings: None,
        }
    }

    /// The change hub, when the library is open.
    pub fn changes(&self) -> Option<&Arc<ChangeHub>> {
        self.changes.as_ref()
    }

    pub fn lifecycle(&self) -> Option<&Arc<LifecycleService>> {
        self.lifecycle.as_ref()
    }

    pub fn rating_status(&self) -> Option<&Arc<RatingStatusService>> {
        self.rating_status.as_ref()
    }

    pub fn organize_state(&self) -> Option<&Arc<OrganizeStateService>> {
        self.organize_state.as_ref()
    }

    /// Settings › Deleted files, read when a file is deleted (the default when there are no
    /// settings).
    pub fn deleted_files(&self) -> Arc<dyn cv_app::deleted_files::DeletedFilesSource> {
        match &self.settings {
            Some(settings) => {
                Arc::clone(settings) as Arc<dyn cv_app::deleted_files::DeletedFilesSource>
            }
            None => cv_app::deleted_files::default_source(),
        }
    }

    /// The bin of Settings › Deleted files over the system Recycle Bin / Trash.
    pub fn bin(&self) -> Arc<dyn cv_app::library_ops::SystemBin> {
        cv_app::library_ops::PolicyBin::os(self.deleted_files())
    }

    /// The data location, when the library is open.
    pub fn location(&self) -> Option<&DataLocation> {
        self.location.as_ref()
    }

    fn service(&self) -> Result<&DataSourceService, SourceCommandError> {
        self.service.as_ref().map_err(|message| SourceCommandError {
            code: "storage".to_owned(),
            message: message.clone(),
        })
    }

    /// The library database. Shared with the browsing commands.
    pub fn database(&self) -> Result<Arc<Database>, String> {
        self.service()
            .map(|service| service.database().clone())
            .map_err(|error| error.message)
    }

    /// Restarts the folder watcher for the current list of folders. The returned work is
    /// blocking (it probes the folders), so run it off the async workers.
    pub fn watch_again(&self) -> impl FnOnce() + Send + 'static {
        let scan = self.scan.clone();
        move || {
            if let Some(scan) = scan {
                scan.refresh_watcher();
            }
        }
    }

    /// The rules a folder must pass before its library is known (system folders, the app's
    /// data folder), as a check that can run off the async workers. When the library could not
    /// be opened, the operating system's folders are still refused.
    pub fn folder_check(
        &self,
    ) -> impl Fn(&str) -> Result<String, SourceCommandError> + Send + 'static {
        let service = self.service.as_ref().ok().cloned();
        move |path: &str| match &service {
            Some(service) => Ok(service.check_folder(path)?),
            None => SourceGuard::for_current_os(&[])
                .check_with_links(path, &[], None)
                .map_err(|error| DataSourceError::from(error).into()),
        }
    }

    pub fn list(&self) -> Result<Vec<DataSourceDto>, SourceCommandError> {
        Ok(self
            .service()?
            .list()?
            .into_iter()
            .map(DataSourceDto::from)
            .collect())
    }

    pub fn add(
        &self,
        kind: SourceKindDto,
        path: &str,
    ) -> Result<DataSourceDto, SourceCommandError> {
        Ok(self.service()?.add(kind.into(), path)?.into())
    }

    pub fn remove(&self, id: u32) -> Result<RemovedSourceDto, SourceCommandError> {
        let removed = self.service()?.remove(i64::from(id))?;
        Ok(RemovedSourceDto {
            source: removed.source.into(),
            kept_items: small(removed.kept_items),
        })
    }

    pub fn reorder(
        &self,
        kind: SourceKindDto,
        ids: &[u32],
    ) -> Result<Vec<DataSourceDto>, SourceCommandError> {
        let ids: Vec<i64> = ids.iter().copied().map(i64::from).collect();
        Ok(self
            .service()?
            .reorder(kind.into(), &ids)?
            .into_iter()
            .map(DataSourceDto::from)
            .collect())
    }

    pub fn set_label(&self, id: u32, label: &str) -> Result<DataSourceDto, SourceCommandError> {
        let service = self.service()?;
        let renamed = service.set_label(i64::from(id), label)?;
        // Every title of the folder shows the new label, so open pages refresh them.
        if let Some(changes) = &self.changes {
            let root = renamed.path.clone();
            let titles = service
                .database()
                .read(move |conn| {
                    let mut stmt = conn.prepare(
                        "SELECT id FROM item WHERE data_source = ?1 COLLATE NOCASE                          AND kind IN ('movie', 'show')",
                    )?;
                    let ids = stmt
                        .query_map([root], |row| row.get::<_, i64>(0))?
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(ids)
                })
                .unwrap_or_default();
            let titles: Vec<cv_domain::ItemId> =
                titles.into_iter().map(cv_domain::ItemId).collect();
            changes.publish(&titles, &[cv_app::changes::ChangePart::Details]);
        }
        // A warning about the folder names it by its new label too.
        if let Some(scan) = &self.scan {
            scan.source_relabelled(&renamed);
        }
        Ok(renamed.into())
    }

    pub fn change_kind(
        &self,
        id: u32,
        kind: SourceKindDto,
    ) -> Result<DataSourceDto, SourceCommandError> {
        Ok(self
            .service()?
            .change_kind(i64::from(id), kind.into())?
            .into())
    }

    pub fn remap(&self, id: u32, path: &str) -> Result<RemapSourceDto, SourceCommandError> {
        let report = self.service()?.remap(i64::from(id), path)?;
        Ok(RemapSourceDto {
            source: report.source.into(),
            old_path: report.old_path,
            items: small(report.items),
        })
    }
}

/// One library folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DataSourceDto {
    pub id: u32,
    pub kind: SourceKindDto,
    /// Absolute path in the syntax of this operating system.
    pub path: String,
    /// The name the user sees ("NAS 4K"); unique within its library type.
    pub label: String,
    /// Position inside its kind, starting at 0.
    pub position: u32,
}

impl From<DataSource> for DataSourceDto {
    fn from(source: DataSource) -> Self {
        Self {
            id: u32::try_from(source.id).unwrap_or(u32::MAX),
            kind: source.kind.into(),
            path: source.path,
            label: source.label,
            position: u32::try_from(source.position).unwrap_or(u32::MAX),
        }
    }
}

/// What removing a folder did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RemovedSourceDto {
    pub source: DataSourceDto,
    /// Titles that stay in the library, offline, until the folder is added again.
    pub kept_items: u32,
}

/// What remapping a folder did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RemapSourceDto {
    pub source: DataSourceDto,
    pub old_path: String,
    /// Titles that now point at the new location; the next scan verifies them.
    pub items: u32,
}

/// Why a command did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SourceCommandError {
    /// Stable code: `empty`, `not_absolute`, `invalid_characters`, `system_folder`,
    /// `already_added`, `inside_existing`, `contains_existing`, `unchanged`, `unknown_source`,
    /// `not_reachable`, `bad_order`, `label_taken`, `label_empty` or `storage`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<DataSourceError> for SourceCommandError {
    fn from(error: DataSourceError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

/// Runs the watcher restart off the async workers; a failure only means it stays as it was.
async fn watch_again(work: impl FnOnce() + Send + 'static) {
    let _ = cv_jobs::client::spawn_blocking(work).await;
}

/// Lists every library folder: movies first, then TV, each in the user's order.
pub fn list_data_sources(state: &LibraryState) -> Result<Vec<DataSourceDto>, SourceCommandError> {
    state.list()
}

/// Adds a folder to the movie or TV library.
pub async fn add_data_source(
    state: &LibraryState,
    kind: SourceKindDto,
    path: String,
) -> Result<DataSourceDto, SourceCommandError> {
    let added = state.add(kind, &path)?;
    watch_again(state.watch_again()).await;
    Ok(added)
}

/// Removes a folder from the library list. Its titles stay in the library.
pub async fn remove_data_source(
    state: &LibraryState,
    id: u32,
) -> Result<RemovedSourceDto, SourceCommandError> {
    let removed = state.remove(id)?;
    watch_again(state.watch_again()).await;
    Ok(removed)
}

/// Sets the order of one library's folders; `ids` lists every folder of `kind` once.
pub fn reorder_data_sources(
    state: &LibraryState,
    kind: SourceKindDto,
    ids: Vec<u32>,
) -> Result<Vec<DataSourceDto>, SourceCommandError> {
    state.reorder(kind, &ids)
}

/// Renames a folder's label ("NAS 4K"). It touches no file and scans nothing; a label another
/// folder of the same library already uses is refused.
pub fn rename_data_source_label(
    state: &LibraryState,
    id: u32,
    label: String,
) -> Result<DataSourceDto, SourceCommandError> {
    state.set_label(id, &label)
}

/// Moves a folder to the other library type (Movies or TV shows). No title is deleted or
/// changed; the caller scans the folder again so its titles are read as the new type.
pub async fn change_data_source_kind(
    state: &LibraryState,
    id: u32,
    kind: SourceKindDto,
) -> Result<DataSourceDto, SourceCommandError> {
    let changed = state.change_kind(id, kind)?;
    watch_again(state.watch_again()).await;
    Ok(changed)
}

/// Moves a folder to a new path and re-points its titles, without scraping again.
pub async fn remap_data_source(
    state: &LibraryState,
    id: u32,
    path: String,
) -> Result<RemapSourceDto, SourceCommandError> {
    let remapped = state.remap(id, &path)?;
    watch_again(state.watch_again()).await;
    Ok(remapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_scan::sources::{PathStyle, SystemFolders};

    fn state() -> (tempfile::TempDir, LibraryState) {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = Database::open(dir.path().join("library.db")).expect("library");
        let mut system = SystemFolders::new(PathStyle::current());
        system.protect_tree(
            &dir.path().join("system").to_string_lossy(),
            "a system folder",
        );
        let guard = SourceGuard::new(PathStyle::current(), system);
        let service = DataSourceService::new(Arc::new(db), guard);
        (dir, LibraryState::with_service(service))
    }

    fn folder(dir: &tempfile::TempDir, parts: &[&str]) -> String {
        let path = parts
            .iter()
            .fold(dir.path().to_path_buf(), |p, part| p.join(part));
        std::fs::create_dir_all(&path).expect("create folder");
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn folders_can_be_added_listed_reordered_remapped_and_removed() {
        let (dir, state) = state();
        let a = state
            .add(SourceKindDto::Movie, &folder(&dir, &["A"]))
            .expect("add a");
        let b = state
            .add(SourceKindDto::Movie, &folder(&dir, &["B"]))
            .expect("add b");
        assert_eq!((a.position, b.position), (0, 1));

        let order = state
            .reorder(SourceKindDto::Movie, &[b.id, a.id])
            .expect("reorder");
        assert_eq!(order.iter().map(|s| s.id).collect::<Vec<_>>(), [b.id, a.id]);

        let new_path = folder(&dir, &["C"]);
        let remapped = state.remap(a.id, &new_path).expect("remap");
        assert_eq!(remapped.source.path, new_path);
        assert_eq!(remapped.items, 0);

        let removed = state.remove(b.id).expect("remove");
        assert_eq!(removed.kept_items, 0);
        let left = state.list().expect("list");
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].position, 0);
    }

    #[test]
    fn rejections_carry_a_code_and_a_sentence() {
        let (dir, state) = state();
        state
            .add(SourceKindDto::Movie, &folder(&dir, &["Library", "Movies"]))
            .expect("add");

        let nested = state
            .add(
                SourceKindDto::Movie,
                &folder(&dir, &["Library", "Movies", "Action"]),
            )
            .expect_err("nested");
        assert_eq!(nested.code, "inside_existing");
        assert!(
            nested.message.contains("already covered"),
            "{}",
            nested.message
        );

        let system = state
            .add(SourceKindDto::Movie, &folder(&dir, &["system", "x"]))
            .expect_err("system");
        assert_eq!(system.code, "system_folder");

        let unknown = state.remove(404).expect_err("unknown");
        assert_eq!(unknown.code, "unknown_source");
    }

    /// Every file and folder below `dir` with its size and modification time.
    fn snapshot(dir: &std::path::Path) -> Vec<(String, u64, std::time::SystemTime)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(path) = stack.pop() {
            for entry in std::fs::read_dir(&path).expect("read dir").flatten() {
                let meta = entry.metadata().expect("metadata");
                if meta.is_dir() {
                    stack.push(entry.path());
                }
                // A folder's own time is updated lazily by some file systems; a file's is not.
                let modified = if meta.is_dir() {
                    std::time::UNIX_EPOCH
                } else {
                    meta.modified().expect("modified")
                };
                out.push((
                    entry.path().to_string_lossy().into_owned(),
                    meta.len(),
                    modified,
                ));
            }
        }
        out.sort();
        out
    }

    #[test]
    fn a_new_folder_is_labelled_with_its_short_name() {
        let (dir, state) = state();
        let added = state
            .add(
                SourceKindDto::Movie,
                &folder(&dir, &["Library", "Movies 4K"]),
            )
            .expect("add");
        assert_eq!(added.label, "Movies 4K");
        assert_eq!(state.list().expect("list")[0].label, "Movies 4K");
    }

    #[test]
    fn a_label_can_be_renamed_and_a_duplicate_is_refused_with_a_sentence() {
        let (dir, state) = state();
        let a = state
            .add(SourceKindDto::Movie, &folder(&dir, &["A"]))
            .expect("add a");
        let b = state
            .add(SourceKindDto::Movie, &folder(&dir, &["B"]))
            .expect("add b");
        let tv = state
            .add(SourceKindDto::Tv, &folder(&dir, &["C"]))
            .expect("add tv");

        let renamed = state.set_label(a.id, "NAS 4K").expect("rename");
        assert_eq!(renamed.label, "NAS 4K");
        assert_eq!(state.list().expect("list")[0].label, "NAS 4K");

        let taken = state.set_label(b.id, "nas 4k").expect_err("duplicate");
        assert_eq!(taken.code, "label_taken");
        assert_eq!(
            taken.message,
            "Another movie source already uses this label"
        );
        // The other library type may use it.
        assert_eq!(
            state.set_label(tv.id, "NAS 4K").expect("tv").label,
            "NAS 4K"
        );

        assert_eq!(
            state.set_label(b.id, "  ").expect_err("blank").code,
            "label_empty"
        );
        assert_eq!(
            state.set_label(404, "x").expect_err("unknown").code,
            "unknown_source"
        );
    }

    #[test]
    fn relabelling_a_folder_touches_no_file() {
        let (dir, state) = state();
        let path = folder(&dir, &["Library", "Heat (1995)"]);
        std::fs::write(
            std::path::Path::new(&path).join("Heat (1995).mkv"),
            b"video bytes",
        )
        .expect("write");
        let root = folder(&dir, &["Library"]);
        let source = state.add(SourceKindDto::Movie, &root).expect("add");
        let before = snapshot(std::path::Path::new(&root));

        state.set_label(source.id, "Archive").expect("rename");

        assert_eq!(
            snapshot(std::path::Path::new(&root)),
            before,
            "no file or folder changed"
        );
    }

    #[test]
    fn a_folder_changes_library_type_without_losing_its_place_or_a_title() {
        let (dir, state) = state();
        let path = folder(&dir, &["Library"]);
        let source = state.add(SourceKindDto::Movie, &path).expect("add");
        let other = state
            .add(SourceKindDto::Tv, &folder(&dir, &["Shows"]))
            .expect("add tv");

        let changed = state
            .change_kind(source.id, SourceKindDto::Tv)
            .expect("change");
        assert_eq!((changed.kind, changed.position), (SourceKindDto::Tv, 1));
        let listed = state.list().expect("list");
        assert_eq!(
            listed.iter().map(|s| s.id).collect::<Vec<_>>(),
            [other.id, source.id]
        );
        assert_eq!(
            state
                .change_kind(404, SourceKindDto::Tv)
                .expect_err("unknown")
                .code,
            "unknown_source"
        );
    }

    #[test]
    fn a_folder_that_clashes_in_the_other_library_is_refused_and_stays() {
        let (dir, state) = state();
        let parent = state
            .add(SourceKindDto::Tv, &folder(&dir, &["Media"]))
            .expect("tv");
        let inside = state
            .add(SourceKindDto::Movie, &folder(&dir, &["Media", "Films"]))
            .expect("movies");
        let refused = state
            .change_kind(inside.id, SourceKindDto::Tv)
            .expect_err("inside a tv folder");
        assert_eq!(refused.code, "inside_existing");
        let listed = state.list().expect("list");
        assert_eq!(listed.len(), 2);
        assert!(
            listed
                .iter()
                .any(|s| s.id == inside.id && s.kind == SourceKindDto::Movie)
        );
        assert!(listed.iter().any(|s| s.id == parent.id));
    }
}

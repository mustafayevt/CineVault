//! The Tauri surface of the scan pipeline commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::scan`]. Each function here wraps
//! the shared implementation for the desktop app.

use tauri::State;

pub use cv_ipc::scan::*;
pub use cv_ipc::sources::SourceHealthDto;

/// Rescans every library folder: new titles appear, changed and removed files are noticed, and
/// the thumbnails and media info of what was found follow in the background. Titles whose files
/// are gone become Missing; nothing is deleted.
#[tauri::command]
#[specta::specta]
pub async fn rescan_library(
    state: State<'_, ScanState>,
) -> Result<RescanStartedDto, ScanCommandError> {
    cv_ipc::scan::rescan_library(&state).await
}

/// Lists the video files inside the library folders that belong to no title.
#[tauri::command]
#[specta::specta]
pub async fn find_missing_files(
    state: State<'_, ScanState>,
) -> Result<Vec<MissingInSourceDto>, ScanCommandError> {
    cv_ipc::scan::find_missing_files(&state).await
}

/// Every library folder with whether it is reachable and what it holds, for the sidebar.
#[tauri::command]
#[specta::specta]
pub async fn list_source_health(
    state: State<'_, ScanState>,
) -> Result<Vec<SourceHealthDto>, ScanCommandError> {
    cv_ipc::scan::list_source_health(&state).await
}

/// Reads the media info of the titles that are still waiting for it, for example after MediaInfo
/// was installed. `missing` means it is still not installed.
#[tauri::command]
#[specta::specta]
pub async fn read_pending_media_info(
    state: State<'_, ScanState>,
) -> Result<MediaInfoStatusDto, ScanCommandError> {
    cv_ipc::scan::read_pending_media_info(&state).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use cv_app::data_location::{DataLocation, LocationInputs};
    use cv_app::media_info::MediaReader;
    use cv_app::secrets::SecretStore;
    use cv_app::services::{AppServices, BootOptions};
    use cv_jobs::{JobId, JobState};
    use cv_media::mediainfo::{MediaInfoError, MediaSnapshot, StreamInfo, StreamKind};
    use cv_scan::sources::{PathStyle, SourceGuard, SystemFolders};
    use tauri::Manager;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    use super::*;
    use crate::boot::Boot;
    use crate::img_ipc::ImageState;
    use crate::library_ipc::{LibraryCountsDto, library_counts};
    use crate::sources_ipc::{LibraryState, SourceKindDto, add_data_source, list_data_sources};

    fn put(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// The app as `main` assembles it: the services, and the state objects the commands read.
    fn app(root: &Path) -> (tauri::App<MockRuntime>, Arc<AppServices>) {
        let inputs = LocationInputs {
            exe_dir: root.join("app"),
            user_data_base: root.join("userdata"),
            user_config_base: root.join("userconfig"),
        };
        std::fs::create_dir_all(&inputs.exe_dir).unwrap();
        let options = BootOptions::new(
            DataLocation::resolve(&inputs),
            Arc::new(SecretStore::in_memory()),
        )
        // The titles the scans find must not be matched on the providers' websites.
        .without_providers();
        let services = AppServices::boot(options).expect("boot");
        // Watching and the startup scan are not under test here.
        services.scan().set_background_scans(false);
        let boot = Boot::for_services(Arc::clone(&services));
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        // The real guard refuses the whole application-data tree, which holds the temp folder.
        let guard = SourceGuard::new(
            PathStyle::current(),
            SystemFolders::new(PathStyle::current()),
        );
        app.manage(LibraryState::with_guard(&boot, guard));
        app.manage(ImageState::from_boot(&boot));
        app.manage(ScanState::from_services(boot.services()));
        app.manage(boot);
        (app, services)
    }

    async fn counts_of(app: &tauri::App<MockRuntime>) -> LibraryCountsDto {
        library_counts(app.state::<LibraryState>(), app.state::<ImageState>())
            .await
            .expect("counts")
    }

    async fn wait_for_job(services: &AppServices, id: u32) {
        let job = services.jobs().get(JobId(u64::from(id))).expect("the job");
        let report = job.wait().await;
        assert_eq!(report.snapshot.state, JobState::Completed);
    }

    async fn add_folder(app: &tauri::App<MockRuntime>, kind: SourceKindDto, path: &Path) {
        add_data_source(
            app.state::<LibraryState>(),
            kind,
            path.to_string_lossy().into_owned(),
        )
        .await
        .expect("add folder");
    }

    struct OneHour(Arc<AtomicUsize>);

    impl MediaReader for OneHour {
        fn read(&self, _path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let mut snapshot = MediaSnapshot::new();
            let mut general = StreamInfo::new();
            general.push("Format", "Matroska");
            general.push("Duration", "3600000");
            snapshot.push(StreamKind::General, general);
            let mut video = StreamInfo::new();
            video.push("Format", "AVC");
            video.push("Width", "1280");
            video.push("Height", "720");
            snapshot.push(StreamKind::Video, video);
            Ok(snapshot)
        }

        fn version(&self) -> String {
            "test".to_owned()
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn rescan_through_the_commands_puts_the_items_in_the_library_counts() {
        let temp = tempfile::tempdir().unwrap();
        let (app, services) = app(temp.path());
        // The media live outside the app's data folder, which is never a library folder.
        let media = tempfile::tempdir().unwrap();
        let movies = media.path().join("media").join("movies");
        let shows = media.path().join("media").join("shows");
        put(&movies.join("Alien (1979)/Alien (1979).mkv"), "a");
        put(&movies.join("Heat (1995)/Heat (1995).mkv"), "h");
        put(&shows.join("Andor (2022)/Season 01/Andor S01E01.mkv"), "e");

        // Nothing is scanned yet.
        assert_eq!(counts_of(&app).await.movies, 0);

        // Add the folders and rescan, the way the UI does.
        add_folder(&app, SourceKindDto::Movie, &movies).await;
        add_folder(&app, SourceKindDto::Tv, &shows).await;
        let started = rescan_library(app.state::<ScanState>())
            .await
            .expect("rescan");
        assert_eq!(started.sources, 2);
        wait_for_job(&services, started.job_id).await;

        let counts = counts_of(&app).await;
        assert_eq!(counts.movies, 2);
        assert_eq!(counts.tv_shows, 1);
        assert_eq!(counts.new_items, 3, "found by the scan and not yet seen");

        // The sidebar's folder list: both online, with their titles.
        let health = list_source_health(app.state::<ScanState>())
            .await
            .expect("health");
        assert_eq!(health.len(), 2);
        assert!(health.iter().all(|h| h.online));
        assert_eq!(health.iter().map(|h| h.titles).collect::<Vec<_>>(), [2, 1]);
        let listed = list_data_sources(app.state::<LibraryState>()).unwrap();
        assert_eq!(listed.len(), 2);

        // Rescanning again changes nothing: still the same counts.
        let again = rescan_library(app.state::<ScanState>()).await.unwrap();
        wait_for_job(&services, again.job_id).await;
        assert_eq!(counts_of(&app).await.movies, 2);
        services.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn without_mediainfo_the_rescan_still_completes_and_offers_the_download() {
        let temp = tempfile::tempdir().unwrap();
        let (app, services) = app(temp.path());
        let media = tempfile::tempdir().unwrap();
        let movies = media.path().join("movies");
        put(&movies.join("Alien (1979)/Alien (1979).mkv"), "a");
        add_folder(&app, SourceKindDto::Movie, &movies).await;

        // No MediaInfo in this temporary tools folder.
        let started = rescan_library(app.state::<ScanState>()).await.unwrap();
        assert_eq!(started.media_info, MediaInfoStatusDto::Missing);
        wait_for_job(&services, started.job_id).await;
        assert_eq!(counts_of(&app).await.movies, 1);
        let health = list_source_health(app.state::<ScanState>()).await.unwrap();
        assert_eq!(health[0].pending_media_info, 1, "shown as pending");

        // After the download (here a fake reader), the pending titles are read.
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&reads);
        services.scan().set_media_reader(Arc::new(move || {
            Ok(Arc::new(OneHour(Arc::clone(&counter))) as Arc<dyn MediaReader>)
        }));
        let status = read_pending_media_info(app.state::<ScanState>())
            .await
            .unwrap();
        assert_eq!(status, MediaInfoStatusDto::Ready);
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            services.scan().media_info_idle().await;
            let health = list_source_health(app.state::<ScanState>()).await.unwrap();
            if health[0].pending_media_info == 0 {
                break;
            }
            assert!(Instant::now() < deadline, "media info was not read");
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        services.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn find_missing_lists_a_video_no_title_owns() {
        let temp = tempfile::tempdir().unwrap();
        let (app, services) = app(temp.path());
        let media = tempfile::tempdir().unwrap();
        let movies = media.path().join("movies");
        put(&movies.join("Alien (1979)/Alien (1979).mkv"), "a");
        add_folder(&app, SourceKindDto::Movie, &movies).await;
        let started = rescan_library(app.state::<ScanState>()).await.unwrap();
        wait_for_job(&services, started.job_id).await;

        put(&movies.join("Hidden/.cinevaultignore"), "");
        put(&movies.join("Hidden/Lost.mkv"), "x");
        let found = find_missing_files(app.state::<ScanState>()).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].videos.len(), 1);
        assert_eq!(found[0].videos[0].relative, "Hidden/Lost.mkv");
        assert_eq!(found[0].videos[0].reason, MissingReasonDto::SkippedLocation);
        services.shutdown();
    }
}

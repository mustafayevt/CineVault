//! The scan pipeline wired into the app services: Rescan, scan on startup, the
//! folder watcher, Find missing, thumbnails and media info after a scan, runtime from media,
//! sidecars, and the health of each data source. Media info is read through a fake reader, so
//! no MediaInfo library is needed.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::media_info::{MediaReader, ReaderSource};
use cv_app::notices::AppNotice;
use cv_app::scanning::{MediaInfoStatus, SourceState};
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::data_sources::{insert_source, list_sources};
use cv_db::repo::media_info::pending_media_info;
use cv_db::repo::query::count_items;
use cv_db::repo::{ItemQuery, get_item};
use cv_domain::{ItemDetail, ItemId, ItemKind, MediaFileType, SourceKind};
use cv_jobs::JobState;
use cv_media::mediainfo::{MediaInfoError, MediaSnapshot, StreamInfo, StreamKind};
use tempfile::TempDir;

/// The smallest valid PNG, standing in for a poster.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

fn boot(root: &Path) -> Arc<AppServices> {
    let options = BootOptions::new(location(root), Arc::new(SecretStore::in_memory()))
        // The titles the scans find must not be matched on the providers' websites.
        .without_providers();
    AppServices::boot(options).expect("boot")
}

fn put(path: &Path, content: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn add_source(services: &AppServices, kind: SourceKind, path: &Path) -> i64 {
    let text = path.to_string_lossy().into_owned();
    services
        .db()
        .write(move |tx| insert_source(tx, kind, &text))
        .unwrap()
        .id
}

async fn until<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn movies(services: &AppServices) -> u64 {
    services
        .db()
        .read(|conn| count_items(conn, &ItemQuery::new(ItemKind::Movie)))
        .unwrap()
}

fn pending(services: &AppServices) -> usize {
    services
        .db()
        .read(|conn| pending_media_info(conn, usize::MAX))
        .unwrap()
        .len()
}

fn notices(services: &AppServices) -> Arc<std::sync::Mutex<Vec<AppNotice>>> {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    services.notices().subscribe(move |notice| {
        sink.lock().unwrap().push(notice.clone());
        true
    });
    seen
}

/// Reads every file as a one-hour HEVC video with one audio track, and counts its reads.
struct FakeReader {
    reads: Arc<AtomicUsize>,
}

impl MediaReader for FakeReader {
    fn read(&self, _path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let mut snapshot = MediaSnapshot::new();
        let mut general = StreamInfo::new();
        general.push("Format", "Matroska");
        general.push("Duration", "3600000");
        snapshot.push(StreamKind::General, general);
        let mut video = StreamInfo::new();
        video.push("Format", "HEVC");
        video.push("Width", "1920");
        video.push("Height", "1080");
        snapshot.push(StreamKind::Video, video);
        let mut audio = StreamInfo::new();
        audio.push("Format", "AC-3");
        audio.push("Channel(s)", "6");
        snapshot.push(StreamKind::Audio, audio);
        Ok(snapshot)
    }

    fn version(&self) -> String {
        "test".to_owned()
    }
}

fn fake_source(reads: &Arc<AtomicUsize>) -> ReaderSource {
    let reads = Arc::clone(reads);
    Arc::new(move || {
        Ok(Arc::new(FakeReader {
            reads: Arc::clone(&reads),
        }) as Arc<dyn MediaReader>)
    })
}

fn turn_off_startup_features(services: &AppServices) {
    services.scan().set_background_scans(false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rescan_fills_the_library_then_thumbnails_and_media_info_follow() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "alien");
    put(&media.join("Alien (1979)/poster.png"), PNG);
    put(&media.join("Heat (1995)/Heat (1995).mkv"), "heat");
    add_source(&services, SourceKind::Movie, &media);

    let reads = Arc::new(AtomicUsize::new(0));
    services.scan().set_media_reader(fake_source(&reads));
    services
        .settings()
        .set("media.runtime-from-media", true.into())
        .unwrap();
    services
        .settings()
        .set("media.store-mediainfo-xml", true.into())
        .unwrap();

    let run = services.scan().rescan_all().expect("rescan");
    assert_eq!(run.sources, 1);
    let report = run.handle.job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(movies(&services), 2);

    // Media info is read after the scan, for both titles.
    services.scan().media_info_idle().await;
    until("media info for both titles", || {
        (pending(&services) == 0).then_some(())
    })
    .await;
    assert_eq!(reads.load(Ordering::SeqCst), 2);

    let id = services
        .db()
        .read(|conn| cv_db::repo::list_items(conn, &ItemQuery::new(ItemKind::Movie), None, 10))
        .unwrap()
        .items[0]
        .id;
    let movie = services
        .db()
        .read(|conn| get_item(conn, ItemId(id.0)))
        .unwrap()
        .unwrap();
    let video = movie
        .core
        .media_files
        .iter()
        .find(|f| f.kind == MediaFileType::Video)
        .unwrap();
    assert!(video.has_media_info());
    assert_eq!(video.video_codec, "HEVC");
    assert_eq!(video.height, 1080);
    assert_eq!(video.audio_streams.len(), 1);
    // Runtime from media: one hour.
    let ItemDetail::Movie(fields) = &movie.detail else {
        panic!("a movie");
    };
    assert_eq!(fields.runtime_minutes, 60);
    // The sidecar was written next to the video.
    let sidecars = std::fs::read_dir(media.join("Alien (1979)"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with("mediainfo.xml"))
        .count();
    assert_eq!(sidecars, 1);

    // The image cache job built the poster's thumbnails.
    until("the image cache job", || {
        services
            .jobs()
            .list()
            .iter()
            .find(|j| j.name.starts_with("Preparing"))
            .filter(|j| j.state.is_terminal())
            .map(|_| ())
    })
    .await;

    // The first rescan after the sidecars were written files them with their titles; after that
    // an unchanged library reads nothing again.
    let first = services.scan().rescan_all().unwrap();
    first.handle.job.wait().await;
    let again = services.scan().rescan_all().unwrap();
    again.handle.job.wait().await;
    services.scan().media_info_idle().await;
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    for report in again.handle.reports() {
        assert!(report.is_unchanged(), "{report:?}");
    }
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_up_to_date_sidecar_is_used_instead_of_reading_the_video() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "alien");
    add_source(&services, SourceKind::Movie, &media);
    let reads = Arc::new(AtomicUsize::new(0));
    services.scan().set_media_reader(fake_source(&reads));
    services
        .settings()
        .set("media.store-mediainfo-xml", true.into())
        .unwrap();

    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    services.scan().media_info_idle().await;
    assert_eq!(reads.load(Ordering::SeqCst), 1);

    // A second library on another machine: same files and sidecar, but no stored details.
    let other = TempDir::new().unwrap();
    let second = boot(other.path());
    add_source(&second, SourceKind::Movie, &media);
    let second_reads = Arc::new(AtomicUsize::new(0));
    second.scan().set_media_reader(fake_source(&second_reads));
    second.scan().rescan_all().unwrap().handle.job.wait().await;
    second.scan().media_info_idle().await;
    assert_eq!(
        second_reads.load(Ordering::SeqCst),
        0,
        "the sidecar was used"
    );
    assert_eq!(pending(&second), 0);
    services.shutdown();
    second.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_mediainfo_the_scan_completes_and_media_info_stays_pending() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let seen = notices(&services);
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "alien");
    add_source(&services, SourceKind::Movie, &media);

    // The real reader source: nothing is installed in this temporary tools folder.
    assert_eq!(
        services.scan().media_info_status(),
        MediaInfoStatus::Missing
    );
    let run = services.scan().rescan_all().unwrap();
    let report = run.handle.job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(movies(&services), 1);

    until("the pending notice", || {
        seen.lock()
            .unwrap()
            .iter()
            .find(|n| n.source.as_deref() == Some("Media info"))
            .cloned()
    })
    .await;
    assert_eq!(pending(&services), 1, "the title waits for its media info");
    let notice = seen.lock().unwrap()[0].clone();
    // The notice links to the tools page and names it in words, never as a route.
    assert_eq!(notice.settings_link.as_deref(), Some("/settings/tools"));
    let detail = notice.detail.unwrap();
    assert!(detail.contains("Settings, External tools"), "{detail}");
    assert!(!detail.contains("/settings"), "{detail}");

    // Installing the tool later (here: a fake reader) reads what was waiting.
    let reads = Arc::new(AtomicUsize::new(0));
    services.scan().set_media_reader(fake_source(&reads));
    assert_eq!(services.scan().request_media_info(), MediaInfoStatus::Ready);
    services.scan().media_info_idle().await;
    assert_eq!(pending(&services), 0);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    services.shutdown();
}

/// Media info read later (Home › Read now) is written to the NFO of a title CineVault
/// scraped, so the NFO's <fileinfo> matches the file; an NFO that only came with the folder is
/// left as it was.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn media_info_read_later_is_written_to_the_nfo() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    turn_off_startup_features(&services);
    let media = temp.path().join("movies");
    let nfo = media.join("Heat (1995)/Heat (1995).nfo");
    let theirs = media.join("Alien (1979)/Alien (1979).nfo");
    let their_text = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<movie><title>Alien</title>\
                      <year>1979</year><uniqueid type=\"tmdb\" default=\"true\">348</uniqueid>\
                      </movie>\n";
    put(&media.join("Heat (1995)/Heat (1995).mkv"), "heat");
    put(
        &nfo,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<movie><title>Heat</title><year>1995</year>\
         <uniqueid type=\"tmdb\" default=\"true\">949</uniqueid></movie>\n",
    );
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "alien");
    put(&theirs, their_text);
    add_source(&services, SourceKind::Movie, &media);
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    services.scan().media_info_idle().await;
    assert!(
        !std::fs::read_to_string(&nfo)
            .unwrap()
            .contains("<fileinfo>")
    );
    // The NFO next to the video is read after the scan: the title is scraped from it.
    until("the title read from its NFO", || {
        let ids: Vec<ItemId> = services
            .db()
            .read(|conn| cv_db::repo::list_items(conn, &ItemQuery::new(ItemKind::Movie), None, 10))
            .unwrap()
            .items
            .iter()
            .map(|row| ItemId(row.id.0))
            .collect();
        let scraped = services
            .db()
            .read(move |conn| cv_db::repo::lifecycle::scraped_items(conn, &ids))
            .unwrap();
        (scraped.len() == 2).then_some(())
    })
    .await;
    // Heat was scraped by CineVault, so its NFO is CineVault's own.
    services
        .db()
        .write(|tx| {
            tx.execute(
                "UPDATE item SET last_scraper_id = 'tmdb' WHERE kind = 'movie' AND title = 'Heat'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    let reads = Arc::new(AtomicUsize::new(0));
    services.scan().set_media_reader(fake_source(&reads));
    assert_eq!(services.scan().request_media_info(), MediaInfoStatus::Ready);
    services.scan().media_info_idle().await;
    let written = until("the NFO with the media info", || {
        let text = std::fs::read_to_string(&nfo).ok()?;
        text.contains("<fileinfo>").then_some(text)
    })
    .await;
    assert!(written.to_lowercase().contains("hevc"), "{written}");
    assert_eq!(std::fs::read_to_string(&theirs).unwrap(), their_text);
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unreadable_video_fails_alone_and_is_marked_so_it_is_not_retried() {
    struct Picky;
    impl MediaReader for Picky {
        fn read(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
            if path.to_string_lossy().contains("Broken") {
                return Err(MediaInfoError::Open {
                    path: path.to_path_buf(),
                });
            }
            FakeReader {
                reads: Arc::default(),
            }
            .read(path)
        }
        fn version(&self) -> String {
            "test".to_owned()
        }
    }
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "a");
    put(&media.join("Broken (2001)/Broken (2001).mkv"), "b");
    add_source(&services, SourceKind::Movie, &media);
    services
        .scan()
        .set_media_reader(Arc::new(|| Ok(Arc::new(Picky) as Arc<dyn MediaReader>)));

    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    services.scan().media_info_idle().await;
    // The good title was stored; the broken one failed once and is marked unreadable at its size
    // and time, so it no longer counts as pending (a later launch leaves it alone too).
    assert_eq!(pending(&services), 0);
    let marks = services
        .db()
        .read(cv_db::repo::media_info::list_unreadable)
        .unwrap();
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert_eq!(marks[0].filename, "Broken (2001).mkv");
    let job = services
        .jobs()
        .list()
        .into_iter()
        .find(|j| j.name.starts_with("Reading media info"))
        .expect("a media info job");
    assert_eq!((job.total, job.failed), (2, 1));
    // A second request does not try the broken file again.
    services.scan().request_media_info();
    services.scan().media_info_idle().await;
    assert_eq!(
        services
            .jobs()
            .list()
            .iter()
            .filter(|j| j.name.starts_with("Reading media info"))
            .count(),
        1
    );
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_read_that_timed_out_is_tried_again_on_the_next_request() {
    /// Times out on the first read of each file, then reads it.
    struct SlowFirst(std::sync::Mutex<Vec<std::path::PathBuf>>);
    impl MediaReader for SlowFirst {
        fn read(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
            let mut seen = self.0.lock().unwrap();
            if !seen.iter().any(|p| p == path) {
                seen.push(path.to_path_buf());
                return Err(MediaInfoError::Timeout {
                    path: path.to_path_buf(),
                    seconds: 120,
                });
            }
            FakeReader {
                reads: Arc::default(),
            }
            .read(path)
        }
        fn version(&self) -> String {
            "test".to_owned()
        }
    }
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    turn_off_startup_features(&services);
    let media = temp.path().join("movies");
    put(&media.join("Excalibur (1981)/Excalibur (1981).mkv"), "x");
    add_source(&services, SourceKind::Movie, &media);
    let reader: Arc<dyn MediaReader> = Arc::new(SlowFirst(std::sync::Mutex::default()));
    services
        .scan()
        .set_media_reader(Arc::new(move || Ok(Arc::clone(&reader))));
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    services.scan().media_info_idle().await;
    // The read ran out of time: the reason is kept for the details page, and the title still
    // waits for its details.
    let marks = services
        .db()
        .read(cv_db::repo::media_info::list_unreadable)
        .unwrap();
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert_eq!(marks[0].reason, cv_db::repo::media_info::TIMED_OUT);
    assert_eq!(pending(&services), 1);

    // The next scan, start or Read now tries it again.
    assert_eq!(services.scan().request_media_info(), MediaInfoStatus::Ready);
    services.scan().media_info_idle().await;
    assert_eq!(pending(&services), 0, "read on the second try");
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_blu_ray_folder_is_read_once_through_its_disc_folder_not_its_index_file() {
    struct Recording(std::sync::Mutex<Vec<std::path::PathBuf>>);
    impl MediaReader for Recording {
        fn read(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
            self.0.lock().unwrap().push(path.to_path_buf());
            if path.is_file() {
                // What MediaInfo makes of `index.bdmv` read as a file: nothing.
                let mut snapshot = MediaSnapshot::new();
                snapshot.push(StreamKind::General, StreamInfo::new());
                return Ok(snapshot);
            }
            FakeReader {
                reads: Arc::default(),
            }
            .read(path)
        }
        fn version(&self) -> String {
            "test".to_owned()
        }
    }
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    turn_off_startup_features(&services);
    let media = temp.path().join("movies");
    let disc = media.join("Bridge of Spies (2015)");
    put(&disc.join("BDMV/index.bdmv"), "INDX0200");
    put(&disc.join("BDMV/MovieObject.bdmv"), "MOBJ0200");
    put(&disc.join("BDMV/STREAM/00001.m2ts"), "film");
    add_source(&services, SourceKind::Movie, &media);
    let recording = Arc::new(Recording(std::sync::Mutex::default()));
    let reader: Arc<dyn MediaReader> = recording.clone();
    services
        .scan()
        .set_media_reader(Arc::new(move || Ok(Arc::clone(&reader))));
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    services.scan().media_info_idle().await;
    let read = recording.0.lock().unwrap().clone();
    // The scan stores the disc folder and its `index.bdmv` marker as videos: the folder is read
    // through the disc's main title, once; the marker is not a video of its own.
    assert_eq!(read.len(), 1, "{read:?}");
    assert!(
        read[0].ends_with("BDMV"),
        "the disc folder is read: {read:?}"
    );
    assert_eq!(pending(&services), 0);
    let marks = services
        .db()
        .read(cv_db::repo::media_info::list_unreadable)
        .unwrap();
    assert!(marks.is_empty(), "{marks:?}");
    // No sidecar is written into the disc structure.
    assert!(!disc.join("BDMV/index-mediainfo.xml").exists());
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_scan_on_startup_always_runs_and_finds_titles_without_contacting_a_provider() {
    let temp = TempDir::new().unwrap();
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "a");

    // Only a test can switch the background scans off.
    let off = boot(&temp.path().join("off"));
    add_source(&off, SourceKind::Movie, &media);
    turn_off_startup_features(&off);
    off.start();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(movies(&off), 0, "no scan on startup");
    assert!(!off.scan().is_watching());
    off.shutdown();

    // With default settings the app scans on start and watches, and the title it finds stays
    // New: a scan contacts no provider and renames nothing.
    let on = boot(&temp.path().join("on"));
    add_source(&on, SourceKind::Movie, &media);
    on.start();
    until("the startup scan", || (movies(&on) == 1).then_some(())).await;
    assert!(on.scan().is_watching());
    tokio::time::sleep(Duration::from_millis(300)).await;
    let id = on
        .db()
        .read(|c| {
            cv_db::repo::list_items(c, &cv_db::repo::ItemQuery::new(ItemKind::Movie), None, 10)
        })
        .unwrap()
        .items[0]
        .id;
    let item = on
        .db()
        .read(|c| cv_db::repo::get_item(c, id))
        .unwrap()
        .unwrap();
    assert_eq!(item.core.last_scraper_id, "", "nothing was scraped");
    assert!(
        media.join("Alien (1979)/Alien (1979).mkv").is_file(),
        "nothing was renamed"
    );
    on.shutdown();

    // With no data source there is nothing to scan and no job is queued.
    let empty = boot(&temp.path().join("empty"));
    empty.start();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(empty.jobs().list().is_empty());
    empty.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_watcher_rescans_a_source_after_a_new_folder_settles() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "a");
    add_source(&services, SourceKind::Movie, &media);
    // Default settings: the app scans on start and watches the folders.
    services.start();
    assert!(services.scan().is_watching());
    until("the startup scan", || {
        (movies(&services) == 1).then_some(())
    })
    .await;

    // Copy a new movie in; once the folder has been quiet for the debounce it appears.
    put(&media.join("Heat (1995)/Heat (1995).mkv"), "h");
    until("the watcher rescan", || {
        (movies(&services) == 2).then_some(())
    })
    .await;

    // Only a test can turn watching off.
    services.scan().set_background_scans(false);
    services.scan().refresh_watcher();
    assert!(!services.scan().is_watching());
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn find_missing_lists_videos_that_belong_to_no_title() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "a");
    add_source(&services, SourceKind::Movie, &media);
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;

    // A video in a folder the scanner skips is not part of any title.
    put(&media.join("Extras/.cinevaultignore"), "");
    put(&media.join("Extras/Behind the scenes.mkv"), "x");
    let reports = services.scan().find_missing().unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].videos.len(), 1);
    assert_eq!(
        reports[0].videos[0].relative,
        "Extras/Behind the scenes.mkv"
    );
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_health_follows_the_drive_and_counts_titles() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let media = temp.path().join("movies");
    let shows = temp.path().join("shows");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "a");
    put(&media.join("Heat (1995)/Heat (1995).mkv"), "h");
    put(&shows.join("Andor (2022)/Season 01/Andor S01E01.mkv"), "e");
    add_source(&services, SourceKind::Movie, &media);
    add_source(&services, SourceKind::Tv, &shows);
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;

    let health = services.scan().source_health().unwrap();
    assert_eq!(health.len(), 2);
    assert_eq!(health[0].state, SourceState::Online);
    assert_eq!(health[0].counts.titles, 2);
    assert_eq!(health[1].state, SourceState::Online);
    assert_eq!(health[1].counts.titles, 1);

    // The drive goes away: the source shows offline and keeps its titles.
    std::fs::rename(&media, temp.path().join("unplugged")).unwrap();
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    let health = services.scan().source_health().unwrap();
    assert_eq!(health[0].state, SourceState::Offline);
    assert_eq!(health[0].counts.titles, 2);
    assert_eq!(health[1].state, SourceState::Online);
    assert_eq!(movies(&services), 2, "nothing is deleted");
    assert_eq!(
        services
            .db()
            .read(|conn| list_sources(conn, None))
            .unwrap()
            .len(),
        2
    );
    services.shutdown();
}

/// A folder added empty on purpose is not a "disconnected drive", and the warning of a
/// folder that could not be read is taken back once a scan reads it again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_skipped_folder_warns_once_and_its_warning_goes_when_it_reads_again() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    turn_off_startup_features(&services);
    let seen = notices(&services);
    let media = temp.path().join("movies");
    let archive = temp.path().join("archive");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "a");
    std::fs::create_dir_all(&archive).unwrap();
    add_source(&services, SourceKind::Movie, &media);
    add_source(&services, SourceKind::Movie, &archive);
    let rescan = || async {
        services
            .scan()
            .rescan_all()
            .unwrap()
            .handle
            .job
            .wait()
            .await;
        services.scan().media_info_idle().await;
    };
    rescan().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .all(|n| !n.title.contains("empty")),
        "an empty folder without titles is just empty: {:?}",
        seen.lock().unwrap()
    );

    std::fs::rename(&media, temp.path().join("unplugged")).unwrap();
    rescan().await;
    let warning = until("the warning", || {
        seen.lock()
            .unwrap()
            .iter()
            .find(|n| n.title.ends_with("could not be read"))
            .cloned()
    })
    .await;
    let topic = warning.topic.clone().expect("the warning has a topic");

    std::fs::rename(temp.path().join("unplugged"), &media).unwrap();
    rescan().await;
    until("the warning taken back", || {
        seen.lock()
            .unwrap()
            .iter()
            .any(|n| n.resolves && n.topic.as_deref() == Some(topic.as_str()))
            .then_some(())
    })
    .await;
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn aspect_detection_reports_a_missing_ffmpeg_once_and_still_stores_media_info() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let seen = notices(&services);
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "a");
    put(&media.join("Heat (1995)/Heat (1995).mkv"), "h");
    put(&media.join("Dune (2021)/Dune (2021).iso"), "i");
    add_source(&services, SourceKind::Movie, &media);
    let reads = Arc::new(AtomicUsize::new(0));
    services.scan().set_media_reader(fake_source(&reads));
    services
        .settings()
        .set("media.detect-aspect-ratio", true.into())
        .unwrap();

    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    services.scan().media_info_idle().await;
    assert_eq!(pending(&services), 0, "media info is stored without FFmpeg");

    let notices = seen.lock().unwrap().clone();
    let ffmpeg: Vec<_> = notices
        .iter()
        .filter(|n| n.title == "Aspect ratio detection needs FFmpeg")
        .collect();
    assert_eq!(ffmpeg.len(), 1, "{notices:?}");
    assert_eq!(ffmpeg[0].settings_link.as_deref(), Some("/settings/tools"));
    let detail = ffmpeg[0].detail.as_deref().unwrap();
    assert!(detail.contains("Settings, External tools"), "{detail}");
    assert!(!detail.contains("/settings"), "{detail}");
    // The ISO image is refused before FFmpeg is looked for, and said so.
    assert!(
        notices
            .iter()
            .any(|n| n.title == "Aspect ratio detection skipped some ISO images"),
        "{notices:?}"
    );
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_locked_runtime_is_not_replaced_by_the_media_length() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "a");
    put(&media.join("Heat (1995)/Heat (1995).mkv"), "h");
    add_source(&services, SourceKind::Movie, &media);
    services
        .settings()
        .set("media.runtime-from-media", true.into())
        .unwrap();
    // Scan first without a reader, lock Heat's runtime, then read.
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    services.scan().media_info_idle().await;
    let heat = services
        .db()
        .read(|conn| cv_db::repo::list_items(conn, &ItemQuery::new(ItemKind::Movie), None, 10))
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.title.starts_with("Heat"))
        .unwrap()
        .id;
    services
        .db()
        .write(move |tx| {
            let mut fields = cv_domain::FieldSet::new();
            fields.insert(cv_domain::MetaField::Runtime);
            cv_db::repo::set_field_locks(tx, heat, &fields)?;
            let mut item = get_item(tx, heat)?.unwrap();
            if let ItemDetail::Movie(m) = &mut item.detail {
                m.runtime_minutes = 99;
            }
            cv_db::repo::update_item(tx, &item)
        })
        .unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    services.scan().set_media_reader(fake_source(&reads));
    services.scan().request_media_info();
    services.scan().media_info_idle().await;

    let runtime = |title: &str| {
        let items = services
            .db()
            .read(|conn| cv_db::repo::list_items(conn, &ItemQuery::new(ItemKind::Movie), None, 10))
            .unwrap()
            .items;
        let id = items
            .iter()
            .find(|i| i.title.starts_with(title))
            .unwrap()
            .id;
        let item = services.db().read(|c| get_item(c, id)).unwrap().unwrap();
        match item.detail {
            ItemDetail::Movie(m) => m.runtime_minutes,
            _ => unreachable!(),
        }
    };
    assert_eq!(runtime("Heat"), 99, "locked");
    assert_eq!(runtime("Alien"), 60, "from the media");
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_update_clears_the_new_flags_first_only_when_the_setting_is_on() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "alien");
    add_source(&services, SourceKind::Movie, &media);
    turn_off_startup_features(&services);
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    let new_count = |services: &AppServices| {
        services
            .db()
            .read(|conn| cv_db::repo::count_new(conn, None))
            .unwrap()
    };
    assert_eq!(new_count(&services), 1, "a scan flags what it finds");

    // Off (the default): a later update leaves the flag alone.
    put(&media.join("Heat (1995)/Heat (1995).mkv"), "heat");
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    assert_eq!(new_count(&services), 2);

    // On: the flags are cleared first, so only what this update finds is flagged.
    services
        .settings()
        .set("library.reset-new-on-update", true.into())
        .unwrap();
    put(&media.join("Ronin (1998)/Ronin (1998).mkv"), "ronin");
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    assert_eq!(new_count(&services), 1);
    services.shutdown();
}

//! "Scan this folder" and "Scan again" through the app services, and a scan's progress in
//! titles.

use std::path::Path;
use std::sync::Arc;

use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::scanning::ScanError;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::data_sources::{insert_source, remove_source};
use cv_db::repo::{ItemQuery, list_items};
use cv_domain::{ItemId, ItemKind, SourceKind};
use cv_jobs::{JobState, UnitPhase};
use tempfile::TempDir;

fn boot(root: &Path) -> Arc<AppServices> {
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
    .without_providers();
    let services = AppServices::boot(options).expect("boot");
    services.scan().set_background_scans(false);
    services
}

fn put(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "video").unwrap();
}

fn add_source(services: &AppServices, kind: SourceKind, path: &Path) -> i64 {
    let text = path.to_string_lossy().into_owned();
    services
        .db()
        .write(move |tx| insert_source(tx, kind, &text))
        .unwrap()
        .id
}

fn movie_id(services: &AppServices, title: &str) -> ItemId {
    let list = services
        .db()
        .read(|conn| list_items(conn, &ItemQuery::new(ItemKind::Movie), None, 100))
        .unwrap();
    let id = list
        .items
        .iter()
        .find(|item| item.title == title)
        .unwrap_or_else(|| panic!("{title} is in the library"))
        .id;
    ItemId(id.0)
}

fn availability(services: &AppServices, id: ItemId) -> String {
    services
        .db()
        .read(|conn| {
            Ok(
                conn.query_row("SELECT availability FROM item WHERE id = ?1", [id.0], |r| {
                    r.get::<_, String>(0)
                })?,
            )
        })
        .unwrap()
}

/// Two movie folders: Alien and Heat in "movies", Ronin in "archive".
async fn library(temp: &TempDir) -> (Arc<AppServices>, i64, i64) {
    let services = boot(temp.path());
    let movies = temp.path().join("movies");
    let archive = temp.path().join("archive");
    put(&movies.join("Alien (1979)/Alien (1979).mkv"));
    put(&movies.join("Heat (1995)/Heat (1995).mkv"));
    put(&archive.join("Ronin (1998)/Ronin (1998).mkv"));
    let first = add_source(&services, SourceKind::Movie, &movies);
    let second = add_source(&services, SourceKind::Movie, &archive);
    let run = services.scan().rescan_all().expect("scan");
    let report = run.handle.job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);
    // The progress is in titles: three of them, all read.
    let units = report.snapshot.units.expect("the scan counts titles");
    assert_eq!(units.phase, UnitPhase::Working);
    assert_eq!((units.done, units.total), (3, 3));
    (services, first, second)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scan_this_folder_reads_one_library_folder_under_its_name() {
    let temp = TempDir::new().unwrap();
    let (services, _, archive) = library(&temp).await;
    // A folder of the other library folder goes; scanning "archive" must not notice it.
    std::fs::remove_dir_all(temp.path().join("movies/Heat (1995)")).unwrap();
    put(&temp
        .path()
        .join("archive/Brand New (2024)/Brand New (2024).mkv"));
    let run = services
        .scan()
        .scan_folder(archive)
        .expect("scan")
        .expect("the folder exists");
    let report = run.handle.job.wait().await;
    assert_eq!(report.snapshot.name, "Scanning archive");
    let units = report.snapshot.units.expect("titles");
    assert_eq!((units.done, units.total), (2, 2));
    assert_eq!(
        availability(&services, movie_id(&services, "Heat")),
        "online"
    );
    movie_id(&services, "Brand New");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scan_again_reads_only_the_titles_folder_and_notices_it_is_gone() {
    let temp = TempDir::new().unwrap();
    let (services, _, _) = library(&temp).await;
    let alien = movie_id(&services, "Alien");
    let heat = movie_id(&services, "Heat");
    std::fs::remove_dir_all(temp.path().join("movies/Heat (1995)")).unwrap();

    let run = services
        .scan()
        .rescan_title(alien)
        .expect("scan")
        .expect("the title exists");
    let report = run.handle.job.wait().await;
    assert_eq!(report.snapshot.name, "Scanning Alien (1979)");
    let units = report.snapshot.units.expect("titles");
    assert_eq!((units.done, units.total), (1, 1));
    assert_eq!(availability(&services, heat), "online", "another title");

    let run = services.scan().rescan_title(heat).unwrap().unwrap();
    run.handle.job.wait().await;
    assert_eq!(availability(&services, heat), "missing");
    put(&temp.path().join("movies/Heat (1995)/Heat (1995).mkv"));
    let run = services.scan().rescan_title(heat).unwrap().unwrap();
    run.handle.job.wait().await;
    assert_eq!(availability(&services, heat), "online");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_title_whose_library_folder_was_removed_cannot_be_scanned_alone() {
    let temp = TempDir::new().unwrap();
    let (services, _, archive) = library(&temp).await;
    let ronin = movie_id(&services, "Ronin");
    services
        .db()
        .write(move |tx| remove_source(tx, archive).map(drop))
        .unwrap();
    match services.scan().rescan_title(ronin) {
        Err(ScanError::NoFolder(title)) => assert_eq!(title, "Ronin (1998)"),
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("a title without a library folder was scanned"),
    }
    assert!(
        services
            .scan()
            .rescan_title(ItemId(999_999))
            .unwrap()
            .is_none()
    );
}

//! Change data source and remove from library against the real scan pipeline
//!: what a rescan does afterwards.
//!
//! * a removed title comes back with the next scan, unless its folder carries `.cinevaultignore`;
//! * a title moved to another data source is not duplicated by scanning both sources.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::library_ops::{
    BinnedItem, ChangeSourceService, IgnoreOutcome, MoveResult, RemovalService, SendOutcome,
    SystemBin, SystemTrash,
};
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::query::count_items;
use cv_db::repo::{ItemQuery, get_item, list_items};
use cv_domain::{ItemId, ItemKind, SourceKind};
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

fn put(path: &Path, content: &str) {
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

async fn rescan(services: &AppServices) {
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
}

fn movies(services: &AppServices) -> Vec<(ItemId, String, String)> {
    let page = services
        .db()
        .read(|conn| list_items(conn, &ItemQuery::new(ItemKind::Movie), None, 100))
        .unwrap();
    page.items
        .iter()
        .map(|item| {
            let full = services
                .db()
                .read(|conn| get_item(conn, ItemId(item.id.0)))
                .unwrap()
                .unwrap();
            (
                ItemId(item.id.0),
                full.core.title.clone(),
                full.core.path.clone(),
            )
        })
        .collect()
}

fn count(services: &AppServices) -> u64 {
    services
        .db()
        .read(|conn| count_items(conn, &ItemQuery::new(ItemKind::Movie)))
        .unwrap()
}

/// The system bin is never reached by these tests.
struct NoTrash;

impl SystemTrash for NoTrash {
    fn supports(&self, _path: &Path) -> bool {
        false
    }
    fn send(&self, _paths: &[PathBuf]) -> Result<(), String> {
        Err("not available in tests".to_owned())
    }
}

impl SystemBin for NoTrash {
    fn send_tracked(&self, _paths: &[PathBuf]) -> SendOutcome {
        SendOutcome {
            sent: Vec::new(),
            error: Some("not available in tests".to_owned()),
        }
    }
    fn present(&self, items: &[BinnedItem]) -> Vec<bool> {
        vec![false; items.len()]
    }
    fn restore(&self, _items: &[BinnedItem]) -> Result<(), Vec<(String, String)>> {
        Err(Vec::new())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_removed_title_comes_back_with_the_next_scan_unless_its_folder_is_ignored() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "alien");
    put(&media.join("Heat (1995)/Heat (1995).mkv"), "heat");
    add_source(&services, SourceKind::Movie, &media);
    rescan(&services).await;
    assert_eq!(count(&services), 2);

    let removal = RemovalService::new(Arc::clone(services.db())).with_bin(Arc::new(NoTrash));
    // Alien: removed, files kept, no marker.
    let alien = movies(&services)
        .into_iter()
        .find(|m| m.1 == "Alien")
        .unwrap()
        .0;
    removal.remove_from_library(&[alien], false).unwrap();
    assert_eq!(count(&services), 1);
    assert!(media.join("Alien (1979)/Alien (1979).mkv").is_file());
    rescan(&services).await;
    assert_eq!(count(&services), 2, "the next scan adds it again");

    // Heat: removed with "ignore this folder".
    let heat = movies(&services)
        .into_iter()
        .find(|m| m.1 == "Heat")
        .unwrap()
        .0;
    let report = removal.remove_from_library(&[heat], true).unwrap();
    assert_eq!(report.removed[0].ignore, IgnoreOutcome::Written);
    assert!(media.join("Heat (1995)/.cinevaultignore").is_file());
    rescan(&services).await;
    let titles: Vec<String> = movies(&services).into_iter().map(|m| m.1).collect();
    assert_eq!(titles, ["Alien"], "the ignored folder stays out");

    // Deleting the marker brings the folder back at the next scan.
    std::fs::remove_file(media.join("Heat (1995)/.cinevaultignore")).unwrap();
    rescan(&services).await;
    assert_eq!(count(&services), 2);
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_moved_title_is_not_duplicated_by_scanning_both_sources() {
    let temp = TempDir::new().unwrap();
    let services = boot(temp.path());
    let old = temp.path().join("movies");
    let new = temp.path().join("archive");
    std::fs::create_dir_all(&new).unwrap();
    put(&old.join("Heat (1995)/Heat (1995).mkv"), "heat");
    put(&old.join("Heat (1995)/poster.jpg"), "poster");
    put(&old.join("Ronin (1998)/Ronin (1998).mkv"), "ronin");
    add_source(&services, SourceKind::Movie, &old);
    let target = add_source(&services, SourceKind::Movie, &new);
    rescan(&services).await;
    assert_eq!(count(&services), 2);

    let heat = movies(&services)
        .into_iter()
        .find(|m| m.1 == "Heat")
        .unwrap()
        .0;
    let service = ChangeSourceService::new(Arc::clone(services.db()));
    let applied = service.apply(&[heat], target, &mut |_| {}).unwrap();
    assert_eq!(applied.items[0].result, MoveResult::Moved, "{applied:?}");
    assert!(new.join("Heat (1995)/Heat (1995).mkv").is_file());

    rescan(&services).await;
    let all = movies(&services);
    assert_eq!(all.len(), 2, "{all:?}");
    let heat_after = all.iter().find(|m| m.0 == heat).expect("the same item");
    assert!(
        Path::new(&heat_after.2).starts_with(&new),
        "now in the archive: {heat_after:?}"
    );
    services.shutdown();
}

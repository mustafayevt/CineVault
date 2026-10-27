//! The organize-state cache. A job on the CPU lane runs the renamer planner as a dry
//! run per title under the active profile and stores tidy, pending, conflict or locked. It is
//! recomputed when the profile changes, after a rename and after a scrape.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::organize::{ApplyRequest, PreviewRequest, Scope, current_profile};
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::organize_state::{
    OrganizeStateKind, organize_state, organize_state_counts, stale_items,
};
use cv_db::repo::{insert_item, set_item_locked};
use cv_domain::{Item, ItemId, ItemKind, MediaFile, MediaFileType, SourceKind};
use serde_json::json;
use tempfile::TempDir;

struct Rig {
    _temp: TempDir,
    root: PathBuf,
    services: Arc<AppServices>,
}

fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

impl Rig {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("movies");
        std::fs::create_dir_all(&root).unwrap();
        let options = BootOptions::new(location(temp.path()), Arc::new(SecretStore::in_memory()))
            .without_providers();
        let services = AppServices::boot(options).expect("boot");
        let source = root.to_string_lossy().into_owned();
        services
            .db()
            .write(move |tx| insert_source(tx, SourceKind::Movie, &source))
            .unwrap();
        Self {
            _temp: temp,
            root,
            services,
        }
    }

    /// A movie in `folder` with a video named `base`, on disk and in the library.
    fn movie(&self, folder: &str, base: &str, title: &str, year: u16, tmdb: &str) -> ItemId {
        let dir = self.root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{base}.mkv")), b"video").unwrap();
        let mut item = Item::movie(title);
        item.core.year = year;
        item.core.ids.set("tmdb", tmdb);
        item.core.last_scraper_id = "tmdb".to_owned();
        item.core.data_source = self.root.to_string_lossy().into_owned();
        item.core.path = dir.to_string_lossy().into_owned();
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            item.core.path.clone(),
            format!("{base}.mkv"),
        ));
        self.services
            .db()
            .write(move |tx| insert_item(tx, &item))
            .unwrap()
    }

    fn state(&self, id: ItemId) -> Option<(OrganizeStateKind, String)> {
        let key = self.services.organize_state().current_key();
        self.services
            .db()
            .read(move |c| organize_state(c, id))
            .unwrap()
            .filter(|row| row.profile_key == key)
            .map(|row| (row.state, row.reason))
    }

    /// Queues the missing states and waits for the job.
    async fn refresh(&self) {
        if let Some(job) = self.services.organize_state().refresh_missing() {
            job.wait().await;
        }
    }
}

async fn until<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tidy_title_is_tidy_a_misnamed_one_pending_and_a_locked_one_locked() {
    let rig = Rig::new();
    let tidy = rig.movie("Alien (1979)", "Alien (1979)", "Alien", 1979, "348");
    let pending = rig.movie("heat.1995.1080p", "heat", "Heat", 1995, "949");
    let locked = rig.movie("blade", "blade", "Blade Runner", 1982, "78");
    let id = locked;
    rig.services
        .db()
        .write(move |tx| set_item_locked(tx, &[id], true))
        .unwrap();

    assert_eq!(rig.state(tidy), None, "nothing is computed before the job");
    rig.refresh().await;

    assert_eq!(rig.state(tidy).unwrap().0, OrganizeStateKind::Tidy);
    assert_eq!(rig.state(pending).unwrap().0, OrganizeStateKind::Pending);
    // A locked title is locked even though its names differ from the profile.
    assert_eq!(rig.state(locked).unwrap().0, OrganizeStateKind::Locked);
    let key = rig.services.organize_state().current_key();
    let counts = rig
        .services
        .db()
        .read(move |c| organize_state_counts(c, ItemKind::Movie, &key))
        .unwrap();
    assert_eq!(
        (counts.tidy, counts.pending, counts.locked, counts.checking),
        (1, 1, 1, 0)
    );
    // Nothing else is left to compute.
    assert!(rig.services.organize_state().refresh_missing().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_changed_template_marks_a_tidy_title_pending() {
    let rig = Rig::new();
    let alien = rig.movie("Alien (1979)", "Alien (1979)", "Alien", 1979, "348");
    rig.refresh().await;
    assert_eq!(rig.state(alien).unwrap().0, OrganizeStateKind::Tidy);

    // The media-server preset puts the TMDB id in the folder name.
    rig.services
        .settings()
        .set("renamer.active-profile", json!("media-server-safe"))
        .unwrap();
    // The settings listener noticed the new profile and queued the job; the old row is stale.
    assert_eq!(rig.state(alien), None);
    let pending = until("the new profile's state", || rig.state(alien)).await;
    assert_eq!(pending.0, OrganizeStateKind::Pending);

    // Back to the clean profile: tidy again.
    rig.services
        .settings()
        .set("renamer.active-profile", json!("clean"))
        .unwrap();
    let again = until("the clean profile's state", || rig.state(alien)).await;
    assert_eq!(again.0, OrganizeStateKind::Tidy);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_titles_with_one_destination_conflict_and_say_which() {
    let rig = Rig::new();
    let first = rig.movie("heat-a", "heat", "Heat", 1995, "949");
    let second = rig.movie("heat-b", "heat", "Heat", 1995, "949");
    rig.refresh().await;
    for id in [first, second] {
        let (state, reason) = rig.state(id).unwrap();
        assert_eq!(state, OrganizeStateKind::Conflict);
        assert!(reason.contains("Heat (1995)"), "{reason}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_makes_the_title_tidy_without_a_full_recompute() {
    let rig = Rig::new();
    let heat = rig.movie("heat.1995.1080p", "heat", "Heat", 1995, "949");
    rig.refresh().await;
    assert_eq!(rig.state(heat).unwrap().0, OrganizeStateKind::Pending);

    let values = rig.services.settings().snapshot().values;
    let preview = PreviewRequest {
        scope: Scope::Items(vec![heat]),
        profile: current_profile(&values),
        choices: Default::default(),
    };
    let organize = cv_app::organize::OrganizeService::new(Arc::clone(rig.services.db()))
        .with_changed_hook(rig.services.organize_state().hook());
    let applied = tokio::task::spawn_blocking(move || {
        organize.apply(&ApplyRequest {
            preview,
            include: vec![heat.0.to_string()],
        })
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(applied.items.len(), 1);

    // The apply forgot the old answer and queued the recomputation.
    let tidy = until("the state after the rename", || rig.state(heat)).await;
    assert_eq!(tidy.0, OrganizeStateKind::Tidy);
    assert!(rig.root.join("Heat (1995)").is_dir());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn titles_that_changed_are_forgotten_and_recomputed_together() {
    let rig = Rig::new();
    let a = rig.movie("Alien (1979)", "Alien (1979)", "Alien", 1979, "348");
    let b = rig.movie("heat.1995.1080p", "heat", "Heat", 1995, "949");
    rig.refresh().await;

    // A scrape (or any change) of two titles: their state is cleared at once.
    rig.services.organize_state().note_changed(&[a, b]);
    assert_eq!(rig.state(a), None);
    assert_eq!(rig.state(b), None);
    let key = rig.services.organize_state().current_key();
    let stale = rig
        .services
        .db()
        .read(move |c| stale_items(c, &key))
        .unwrap();
    assert_eq!(stale.len(), 2);
    until("both states", || rig.state(a).and(rig.state(b))).await;
}

/// The sidebar's Organize badge (the pipeline's Rename pending) and the
/// Organize page's tab (the plan's changes plus, while renaming is manual, its conflicts) count
/// the same titles, read from one library. A title that is not scraped is in neither.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_organize_tab_and_the_rename_pending_count_agree_on_one_library() {
    let rig = Rig::new();
    rig.movie("heat.1995.1080p", "heat", "Heat", 1995, "949");
    rig.movie("drive.2011", "drive", "Drive", 2011, "64690");
    rig.movie("Alien (1979)", "Alien (1979)", "Alien", 1979, "348");
    rig.movie("dune-a", "dune", "Dune", 2021, "438631");
    rig.movie("dune-b", "dune", "Dune", 2021, "438631");
    let locked = rig.movie("blade", "blade", "Blade Runner", 1982, "78");
    rig.services
        .db()
        .write(move |tx| set_item_locked(tx, &[locked], true))
        .unwrap();
    let raw = rig.movie("arrival.2016", "arrival", "Arrival", 2016, "329865");
    rig.services
        .db()
        .write(move |tx| {
            tx.execute("DELETE FROM ext_id WHERE item_id = ?1", [raw.0])?;
            tx.execute(
                "UPDATE item SET last_scraper_id = '' WHERE id = ?1",
                [raw.0],
            )?;
            Ok(())
        })
        .unwrap();
    rig.refresh().await;

    let values = rig.services.settings().snapshot().values;
    let request = PreviewRequest {
        scope: Scope::Everything,
        profile: current_profile(&values),
        choices: Default::default(),
    };
    let organize = cv_app::organize::OrganizeService::new(Arc::clone(rig.services.db()));
    let preview = tokio::task::spawn_blocking(move || organize.preview(&request))
        .await
        .unwrap()
        .unwrap();
    let tab = preview
        .rows
        .iter()
        .filter(|row| {
            matches!(
                row.status,
                cv_app::organize::RowStatus::Change | cv_app::organize::RowStatus::Conflict
            )
        })
        .count();
    let counts = rig.services.lifecycle().counts(ItemKind::Movie).unwrap();
    assert_eq!(
        tab, 4,
        "Heat and Drive change, the two Dunes conflict: {preview:?}"
    );
    assert_eq!(u32::try_from(tab).unwrap(), counts.rename_pending);
    assert!(
        preview.rows.iter().any(|row| row.id == raw.0.to_string()
            && row.status == cv_app::organize::RowStatus::Unscraped)
    );
}

//! The first run from a folder that already holds NFO files and artwork: the library
//! is instant (real titles, IDs and posters before any online lookup), matching then runs in the
//! background and fills the Needs review queue, and nothing on disk is renamed.
//!
//! Everything happens in temporary directories with a scripted provider and no network.

mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::local_library::LOCAL_IMPORT_JOB;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::graphics::graphic_files_of;
use cv_db::repo::query::list_items;
use cv_db::repo::rename_journal::batch_summaries;
use cv_db::repo::review;
use cv_db::repo::{ItemQuery, get_item};
use cv_domain::{ItemId, ItemKind, MediaFileType, SourceKind};
use cv_providers::{CredentialResolver, MemoryUserCredentials, MetadataProvider, ProviderRegistry};
use support::{Entry, Scripted, arc};
use tempfile::TempDir;

/// The smallest valid PNG, standing in for a poster.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// How long the scripted provider takes to answer, so the test can look at the library while
/// matching is still waiting for it.
const ONLINE_DELAY: Duration = Duration::from_millis(2500);

const SOLARIS: &str = "solaris.1972.720p.bluray-grp";
const DUNE: &str = "dune.part.two.2024.2160p-grp";
const ALIEN: &str = "Alien (1979)";
const HEAT: &str = "Heat (1995)";

fn kodi_nfo(title: &str, year: u16, tmdb: &str, plot: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<movie>\n  \
         <title>{title}</title>\n  <year>{year}</year>\n  <plot>{plot}</plot>\n  \
         <uniqueid type=\"tmdb\" default=\"true\">{tmdb}</uniqueid>\n</movie>\n"
    )
}

fn put(path: &Path, content: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every file below `root` (relative, with `/`) with its bytes, and every folder.
fn snapshot(root: &Path) -> (BTreeMap<String, Vec<u8>>, Vec<String>) {
    fn walk(
        dir: &Path,
        root: &Path,
        files: &mut BTreeMap<String, Vec<u8>>,
        dirs: &mut Vec<String>,
    ) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(relative(&path, root));
                walk(&path, root, files, dirs);
            } else {
                files.insert(relative(&path, root), std::fs::read(&path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    let mut dirs = Vec::new();
    walk(root, root, &mut files, &mut dirs);
    dirs.sort();
    (files, dirs)
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

fn catalogue() -> Arc<Scripted> {
    arc(Scripted::new(
        "scripted",
        vec![
            Entry::new("593", "Solaris", 1972).runtime(167),
            Entry::new("693134", "Dune: Part Two", 2024),
            Entry::new("348", "Alien", 1979),
            // Two films with one title: Heat cannot be told apart without help.
            Entry::new("949", "Heat", 1995),
            Entry::new("950", "Heat", 1995),
        ],
    )
    .latency(ONLINE_DELAY))
}

fn boot(root: &Path, provider: &Arc<Scripted>) -> Arc<AppServices> {
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(Arc::clone(provider) as Arc<dyn MetadataProvider>);
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let options = BootOptions::new(location(root), Arc::new(SecretStore::in_memory()))
        .with_providers(ProviderCredentialsService::with_resolver(
            resolver, registry,
        ));
    AppServices::boot(options).expect("boot")
}

/// The movie in `folder`, if the library has it.
fn movie_in(services: &AppServices, folder: &str) -> Option<ItemId> {
    let page = services
        .db()
        .read(|c| list_items(c, &ItemQuery::new(ItemKind::Movie), None, 100))
        .unwrap();
    page.items
        .into_iter()
        .find(|s| s.path.replace('\\', "/").ends_with(folder))
        .map(|s| s.id)
}

/// The title of the movie in `folder` and whether a provider has scraped it.
fn state_of(services: &AppServices, folder: &str) -> (String, bool) {
    let id = movie_in(services, folder).unwrap_or_else(|| panic!("{folder} is in the library"));
    let item = services.db().read(|c| get_item(c, id)).unwrap().unwrap();
    (
        item.core.title.clone(),
        !item.core.last_scraper_id.is_empty(),
    )
}

async fn until<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_library_with_nfo_files_is_instant_and_nothing_is_scraped_or_renamed() {
    let temp = TempDir::new().unwrap();
    let media = temp.path().join("movies");
    // Release-style folders whose NFO files carry the real title, with their posters.
    put(&media.join(format!("{SOLARIS}/{SOLARIS}.mkv")), "solaris");
    put(
        &media.join(format!("{SOLARIS}/{SOLARIS}.nfo")),
        kodi_nfo(
            "Solaris",
            1972,
            "593",
            "A psychologist visits a space station.",
        ),
    );
    put(&media.join(format!("{SOLARIS}/poster.png")), PNG);
    put(&media.join(format!("{DUNE}/{DUNE}.mkv")), "dune");
    put(
        &media.join(format!("{DUNE}/{DUNE}.nfo")),
        kodi_nfo(
            "Dune: Part Two",
            2024,
            "693134",
            "Paul Atreides unites with the Fremen.",
        ),
    );
    put(&media.join(format!("{DUNE}/{DUNE}-poster.png")), PNG);
    // No NFO at all: only the names say what it is.
    put(&media.join(format!("{ALIEN}/{ALIEN}.mkv")), "alien");
    // A release note that is not an NFO of ours must not retitle anything.
    put(&media.join(format!("{HEAT}/{HEAT}.mkv")), "heat");
    put(
        &media.join(format!("{HEAT}/info.nfo")),
        "Released by GRP. Greetings to everyone.",
    );

    let (files_before, folders_before) = snapshot(&media);

    let provider = catalogue();
    let services = boot(temp.path(), &provider);
    services.scan().set_background_scans(false);
    let text = media.to_string_lossy().into_owned();
    services
        .db()
        .write(move |tx| insert_source(tx, SourceKind::Movie, &text))
        .unwrap();

    let run = services.scan().rescan_all().expect("rescan");
    run.handle.job.wait().await;

    // The local import is a job right after the scan; wait for it, not for matching.
    until("the local NFO import", || {
        services
            .jobs()
            .list()
            .iter()
            .find(|j| j.name == LOCAL_IMPORT_JOB)
            .filter(|j| j.state.is_terminal())
            .map(|_| ())
    })
    .await;

    // Instant library: real titles from the NFO files and the local posters.
    assert_eq!(state_of(&services, SOLARIS), ("Solaris".to_owned(), false));
    assert_eq!(
        state_of(&services, DUNE),
        ("Dune: Part Two".to_owned(), false)
    );
    assert_eq!(state_of(&services, ALIEN), ("Alien".to_owned(), false));
    let heat = state_of(&services, HEAT);
    assert_eq!(
        heat,
        ("Heat".to_owned(), false),
        "a foreign note retitles nothing"
    );

    let solaris = movie_in(&services, SOLARIS).unwrap();
    let dune = movie_in(&services, DUNE).unwrap();
    let item = services
        .db()
        .read(|c| get_item(c, solaris))
        .unwrap()
        .unwrap();
    assert_eq!(item.core.ids.get("tmdb"), Some("593"));
    assert_eq!(item.core.plot, "A psychologist visits a space station.");
    let posters = services
        .db()
        .read(|c| graphic_files_of(c, &[solaris, dune], &[MediaFileType::Poster]))
        .unwrap();
    assert_eq!(posters.len(), 2, "both local posters are in the library");

    // Nothing runs by itself after the scan: no provider is asked, no scrape job starts and
    // nothing waits in Needs review. A title stays New until the user scrapes it.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        services
            .jobs()
            .list()
            .iter()
            .all(|j| !j.name.starts_with("Scraping")),
        "no scrape started"
    );
    assert_eq!(provider.searches.load(Ordering::Relaxed), 0);
    assert_eq!(provider.lookups.load(Ordering::Relaxed), 0);
    assert_eq!(provider.details.load(Ordering::Relaxed), 0);
    assert_eq!(services.db().read(review::count_pending).unwrap(), 0);

    // The first run renames nothing: every file is where it was, no folder moved, and no rename
    // batch was ever journaled. A scrape writes the title's NFO by the NFO settings (the
    // follow-up of any scraper), so an NFO may be rewritten; everything else keeps its bytes.
    services.scrape_follow_up().idle().await;
    let (files_after, folders_after) = snapshot(&media);
    let trash = format!("{}/", cv_rename::exec::TRASH_DIR);
    for (path, bytes) in &files_before {
        if path.ends_with(".nfo") {
            assert!(files_after.contains_key(path), "{path} vanished");
        } else {
            assert_eq!(
                files_after.get(path),
                Some(bytes),
                "{path} changed or vanished"
            );
        }
    }
    let folders_after: Vec<String> = folders_after
        .into_iter()
        .filter(|f| !f.starts_with(&trash) && f != cv_rename::exec::TRASH_DIR)
        .collect();
    assert_eq!(folders_after, folders_before);
    // A hand-written NFO the scrape replaced is never lost: its original bytes are kept in the
    // trash folder of the source, below the title's own folder name.
    for folder in [SOLARIS, DUNE] {
        let name = format!("{folder}/{folder}.nfo");
        let original = &files_before[&name];
        if files_after[&name] != *original {
            assert!(
                files_after
                    .iter()
                    .any(|(path, bytes)| path.starts_with(&trash)
                        && path.ends_with(&name)
                        && bytes == original),
                "the original of {name} is kept in the trash folder"
            );
        }
    }
    assert!(
        services.db().read(batch_summaries).unwrap().is_empty(),
        "no rename was journaled"
    );
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn skipping_the_tmdb_key_still_builds_the_library_from_local_files() {
    let temp = TempDir::new().unwrap();
    let media = temp.path().join("movies");
    put(&media.join(format!("{SOLARIS}/{SOLARIS}.mkv")), "solaris");
    put(
        &media.join(format!("{SOLARIS}/{SOLARIS}.nfo")),
        kodi_nfo("Solaris", 1972, "593", "A psychologist visits a station."),
    );
    put(&media.join(format!("{SOLARIS}/poster.png")), PNG);
    let (files_before, folders_before) = snapshot(&media);

    // No provider is ready, as after "Skip for now".
    let options = BootOptions::new(location(temp.path()), Arc::new(SecretStore::in_memory()))
        .without_providers();
    let services = AppServices::boot(options).expect("boot");
    services.scan().set_background_scans(false);
    let text = media.to_string_lossy().into_owned();
    services
        .db()
        .write(move |tx| insert_source(tx, SourceKind::Movie, &text))
        .unwrap();
    services
        .scan()
        .rescan_all()
        .expect("rescan")
        .handle
        .job
        .wait()
        .await;
    until("the local NFO import", || {
        services
            .jobs()
            .list()
            .iter()
            .find(|j| j.name == LOCAL_IMPORT_JOB)
            .filter(|j| j.state.is_terminal())
            .map(|_| ())
    })
    .await;

    assert_eq!(state_of(&services, SOLARIS), ("Solaris".to_owned(), false));
    let id = movie_in(&services, SOLARIS).unwrap();
    let posters = services
        .db()
        .read(|c| graphic_files_of(c, &[id], &[MediaFileType::Poster]))
        .unwrap();
    assert_eq!(posters.len(), 1);
    assert!(
        !services
            .jobs()
            .list()
            .iter()
            .any(|j| j.name.starts_with("Match")),
        "nothing is matched without a provider"
    );
    // The import keeps no undo record: it only reads.
    assert!(
        !services.location().nfo_undo_dir().exists()
            || std::fs::read_dir(services.location().nfo_undo_dir())
                .unwrap()
                .next()
                .is_none()
    );
    let (files_after, folders_after) = snapshot(&media);
    assert_eq!(files_after, files_before);
    assert_eq!(folders_after, folders_before);
    services.shutdown();
}

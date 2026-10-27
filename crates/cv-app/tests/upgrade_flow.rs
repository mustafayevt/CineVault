//! An update from 0.2.5 end to end, on a temporary data folder that looks
//! like a 0.2.5 install: the app services start, the old library and settings are set aside
//! whole, defaults load, the kept provider keys resolve and the credentials of removed features
//! are gone, the first scan stages five titles from their local files without any network call
//! or change to the media folders, and then a scrape, a rename, a move to another data source
//! and the undo of both behave as before (journal, History, bin and recovery).
//!
//! Everything is in temporary folders with files of a few bytes; the credential store is in
//! memory and the bin is a folder, so neither the user's keyring, data folder nor Recycle Bin
//! is touched.

#[path = "support/flow_kit.rs"]
mod kit;
mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use cv_app::fresh_start::RETIRED_ACCOUNTS;
use cv_app::history::{HistoryFilter, HistoryKind, HistoryService, UndoState};
use cv_app::library_ops::{ChangeSourceService, RecoveryService, SystemBin};
use cv_app::lifecycle::Stage;
use cv_app::local_library::LOCAL_IMPORT_JOB;
use cv_app::organize::{ApplyRequest, OrganizeService, PreviewRequest, Scope, current_profile};
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::data_sources::insert_source;
use cv_domain::SourceKind;
use cv_providers::{CredentialState, MetadataProvider, ProviderRegistry};
use kit::{FakeBin, FakeWeb, cinevault_nfo, put, snapshot};
use sha2::{Digest, Sha256};
use support::{Entry, Scripted, arc};
use tempfile::TempDir;

/// The settings of a 0.2.5 install: envelope version 1, a few changed values.
const OLD_SETTINGS: &str = r#"{
  "version": 1,
  "settings": {
    "values": {
      "onboarding.completed": true,
      "ui.theme": "light",
      "renamer.mode": "automatic",
      "renamer.movie-folder": "{title} ({year})"
    }
  }
}
"#;

/// A library file of schema version 17 (header only; it is never opened as a database).
fn old_library_bytes() -> Vec<u8> {
    let mut bytes = b"SQLite format 3\0".to_vec();
    bytes.resize(100, 0);
    bytes[60..64].copy_from_slice(&17u32.to_be_bytes());
    bytes.extend_from_slice(b"pages of the old library");
    bytes
}

const OLD_WAL: &[u8] = b"write-ahead log of the old library";
const OLD_SHM: &[u8] = b"shared memory index of the old library";

fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// The five titles of the first-scan cases, each a few bytes.
fn five_titles(media: &Path, web: &FakeWeb) {
    // An NFO with a TMDB ID in a folder that matches the naming style: Renamed.
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "alien");
    put(
        &media.join("Alien (1979)/Alien (1979).nfo"),
        cinevault_nfo("Alien", 1979, Some("348"), web),
    );
    put(
        &media.join("Alien (1979)/Alien (1979)-poster.png"),
        kit::PNG,
    );
    // An NFO with an ID in a folder that does not match: Scraped.
    put(&media.join("heat.1995.1080p/heat.1995.1080p.mkv"), "heat");
    put(
        &media.join("heat.1995.1080p/heat.1995.1080p.nfo"),
        cinevault_nfo("Heat", 1995, Some("949"), web),
    );
    // An NFO with title and year but no ID: New, with its title.
    put(&media.join("solyaris/solyaris.mkv"), "solaris");
    put(
        &media.join("solyaris/solyaris.nfo"),
        cinevault_nfo("Solaris", 1972, None, web),
    );
    // A malformed NFO: New.
    put(&media.join("Broken (2001)/Broken (2001).mkv"), "broken");
    put(
        &media.join("Broken (2001)/Broken (2001).nfo"),
        "<movie><year>20",
    );
    // No NFO: New.
    put(&media.join("Plain (2010)/Plain (2010).mkv"), "plain");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_update_from_0_2_5_starts_fresh_scans_locally_and_keeps_every_file_operation_working() {
    let temp = TempDir::new().unwrap();
    let web = FakeWeb::start();
    let loc = kit::location(temp.path());
    loc.ensure_dirs().unwrap();
    let root = loc.root().to_path_buf();
    assert!(
        root.starts_with(temp.path()),
        "the data folder is temporary"
    );

    // ---- the 0.2.5 install -------------------------------------------------------------
    std::fs::write(loc.library_db(), old_library_bytes()).unwrap();
    let db_name = loc.library_db();
    let wal = Path::new(&format!("{}-wal", db_name.display())).to_path_buf();
    let shm = Path::new(&format!("{}-shm", db_name.display())).to_path_buf();
    std::fs::write(&wal, OLD_WAL).unwrap();
    std::fs::write(&shm, OLD_SHM).unwrap();
    std::fs::write(loc.settings_file(), OLD_SETTINGS).unwrap();

    // A fake keyring: the kept providers' keys, a Trakt rating key that stays, and the accounts
    // of removed features (tokens, subtitle services, the HTTP API key).
    let secrets = Arc::new(SecretStore::in_memory());
    secrets.set("tmdb.api_key", "cvtest-tmdb-key-1b7c").unwrap();
    secrets.set("omdb.api_key", "cvtest-omdb-key-5e2a").unwrap();
    secrets
        .set("trakt.client_id", "cvtest-trakt-client-9f30")
        .unwrap();
    for account in RETIRED_ACCOUNTS {
        secrets.set(account, "cvtest-retired-value").unwrap();
    }

    let media = temp.path().join("movies");
    five_titles(&media, &web);
    let archive = temp.path().join("archive");
    std::fs::create_dir_all(&archive).unwrap();
    // A folder the app has nothing to do with: it must come out untouched.
    put(&temp.path().join("bystander/notes.txt"), "not a library");
    let bystander_before = snapshot(&temp.path().join("bystander"));
    let media_before = snapshot(&media);

    // ---- start the services ------------------------------------------------------------
    // Fake TMDB: it answers anything and counts what it is asked. The first scan may not ask.
    let tmdb = arc(Scripted::new(
        "tmdb",
        vec![
            Entry::new("348", "Alien", 1979),
            Entry::new("949", "Heat", 1995),
            Entry::new("593", "Solaris", 1972),
        ],
    ));
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(Arc::clone(&tmdb) as Arc<dyn MetadataProvider>);
    let providers = ProviderCredentialsService::new(Arc::clone(&secrets), registry);
    let services = AppServices::boot(
        BootOptions::new(loc.clone(), Arc::clone(&secrets)).with_providers(providers),
    )
    .expect("boot");

    // The old files were set aside whole: the same bytes under the new names, none deleted.
    assert_eq!(
        std::fs::read(root.join("library-old.db")).unwrap(),
        old_library_bytes()
    );
    assert_eq!(
        hash(&std::fs::read(root.join("library-old.db-wal")).unwrap()),
        hash(OLD_WAL)
    );
    assert_eq!(
        hash(&std::fs::read(root.join("library-old.db-shm")).unwrap()),
        hash(OLD_SHM)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("settings-old.json")).unwrap(),
        OLD_SETTINGS
    );
    // The new library has WAL files of its own at most; the old ones are not beside it.
    for (path, old) in [(&wal, OLD_WAL), (&shm, OLD_SHM)] {
        if path.exists() {
            assert_ne!(
                std::fs::read(path).unwrap(),
                old,
                "a stale file stayed behind"
            );
        }
    }
    // A new library of this schema, default settings: the first-run setup opens.
    assert_eq!(
        services.db().schema_version().unwrap(),
        cv_db::SCHEMA_VERSION
    );
    assert!(
        services.settings().values().is_empty(),
        "defaults are loaded"
    );
    assert!(services.settings().get("onboarding.completed").is_none());
    assert!(services.settings().snapshot().notice.is_none());
    // The kept keys resolve; every removed account is gone.
    let active = |provider: &str| {
        services
            .providers()
            .list()
            .unwrap()
            .into_iter()
            .any(|s| s.provider == provider && s.active && s.state == CredentialState::Active)
    };
    assert!(active("tmdb") && active("omdb"), "the kept keys resolve");
    assert!(secrets.get("trakt.client_id").unwrap().is_some());
    for account in RETIRED_ACCOUNTS {
        assert!(
            secrets.get(account).unwrap().is_none(),
            "{account} was not deleted"
        );
    }

    // ---- the first scan ----------------------------------------------------------------
    // Nothing may reach the web after this point except what the test asks for below.
    let source = media.to_string_lossy().into_owned();
    services
        .db()
        .write(move |tx| insert_source(tx, SourceKind::Movie, &source))
        .unwrap();
    let archive_source = archive.to_string_lossy().into_owned();
    let archive_id = services
        .db()
        .write(move |tx| insert_source(tx, SourceKind::Movie, &archive_source))
        .unwrap()
        .id;
    services.start();
    kit::until("the five titles", || {
        (kit::movies(&services).len() == 5).then_some(())
    })
    .await;
    kit::until("the NFO import", || {
        services
            .jobs()
            .list()
            .iter()
            .find(|j| j.name == LOCAL_IMPORT_JOB)
            .filter(|j| j.state.is_terminal())
            .map(|_| ())
    })
    .await;
    let items = kit::movies(&services);
    let stage_of = |folder: &str| {
        let id = kit::by_folder(&items, folder).id.unwrap();
        services.lifecycle().read(&[id]).unwrap()[&id].stage
    };
    kit::until("the name check of every title", || {
        (stage_of("Alien (1979)") == Stage::Renamed).then_some(())
    })
    .await;
    assert_eq!(stage_of("Alien (1979)"), Stage::Renamed);
    assert_eq!(stage_of("heat.1995.1080p"), Stage::Scraped);
    assert_eq!(stage_of("solyaris"), Stage::New);
    assert_eq!(stage_of("Broken (2001)"), Stage::New);
    assert_eq!(stage_of("Plain (2010)"), Stage::New);
    assert_eq!(kit::by_folder(&items, "solyaris").core.title, "Solaris");
    assert_eq!(
        kit::by_folder(&items, "Alien (1979)").core.ids.get("tmdb"),
        Some("348")
    );
    // No network and no change to a media file: not asked, not written, not touched.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(Scripted::count(&tmdb.searches), 0);
    assert_eq!(Scripted::count(&tmdb.lookups), 0);
    assert_eq!(Scripted::count(&tmdb.details), 0);
    assert_eq!(web.hits(), 0, "a remote URL was fetched");
    assert_eq!(snapshot(&media), media_before);
    // The old files are still exactly what they were after the whole first scan.
    assert_eq!(
        std::fs::read(root.join("library-old.db")).unwrap(),
        old_library_bytes()
    );

    // ---- Scrape one New title with the fake TMDB ---------------------------------------
    // The network stays shut: no rating source may be asked after the scrape.
    services
        .settings()
        .set("ratings.fetch-after-scrape-movie", serde_json::json!(false))
        .unwrap();
    let bin = FakeBin::new(temp.path());
    services
        .matching()
        .set_bin(Arc::clone(&bin) as Arc<dyn SystemBin>);
    let solaris = kit::by_folder(&items, "solyaris").id.unwrap();
    let alien = kit::by_folder(&items, "Alien (1979)").id.unwrap();
    let done = services
        .matching()
        .accept_confident(&[solaris])
        .await
        .expect("accept");
    assert_eq!(done.accepted.len(), 1, "{done:?}");
    assert!(done.history_entry.is_some());
    services.scrape_follow_up().idle().await;
    kit::until("the steps after the scrape", || {
        (!services.lifecycle().working().is_working(solaris)).then_some(())
    })
    .await;
    assert!(
        Scripted::count(&tmdb.searches) >= 1,
        "the fake TMDB was asked"
    );
    let scraped = kit::item(&services, solaris);
    assert_eq!(scraped.core.ids.get("tmdb"), Some("593"));
    assert_eq!(scraped.core.last_scraper_id, "tmdb");
    // The scrape wrote the NFO next to the video, the video itself is as it was.
    let solyaris = media.join("solyaris");
    let nfo = std::fs::read_to_string(solyaris.join("solyaris.nfo")).unwrap();
    assert!(nfo.contains("593"), "the NFO carries the new ID: {nfo}");
    assert_eq!(
        std::fs::read(solyaris.join("solyaris.mkv")).unwrap(),
        b"solaris"
    );
    assert_eq!(web.hits(), 0, "the scrape fetched a remote URL");
    // The files of every other title are exactly as the first scan left them.
    let after_scrape = snapshot(&media);
    for (path, stamp) in &media_before {
        if !path.starts_with(&solyaris) {
            assert_eq!(after_scrape.get(path), Some(stamp), "{}", path.display());
        }
    }

    // ---- Rename it, then undo the rename ------------------------------------------------
    let hook = services.organize_state().hook();
    let organize = OrganizeService::new(Arc::clone(services.db()))
        .with_changed_hook(Arc::clone(&hook))
        .with_bin(Arc::clone(&bin) as Arc<dyn SystemBin>);
    assert!(
        organize.journal().unwrap().is_empty(),
        "the scrape journaled no rename, whatever the old settings said"
    );
    let before_rename = kit::contents(&media);
    let request = ApplyRequest {
        preview: PreviewRequest {
            scope: Scope::Everything,
            profile: current_profile(&services.settings().values()),
            choices: Default::default(),
        },
        include: vec![solaris.0.to_string()],
    };
    let applied = organize.apply(&request).unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");
    let rename_batch = applied.batch_id.clone().unwrap();
    assert!(!media.join("solyaris").exists());
    assert!(media.join("Solaris (1972)/Solaris (1972).mkv").is_file());
    assert!(media.join("Solaris (1972)/Solaris (1972).nfo").is_file());
    assert!(
        kit::item(&services, solaris)
            .core
            .path
            .ends_with("Solaris (1972)")
    );
    kit::until("the Renamed stage", || {
        (services.lifecycle().read(&[solaris]).unwrap()[&solaris].stage == Stage::Renamed)
            .then_some(())
    })
    .await;
    // One journal batch and one History entry; the other titles did not move.
    assert_eq!(organize.journal().unwrap().len(), 1);
    let history = HistoryService::new(Arc::clone(services.db()))
        .with_bin(Arc::clone(&bin) as Arc<dyn SystemBin>)
        .with_organize(
            OrganizeService::new(Arc::clone(services.db()))
                .with_changed_hook(Arc::clone(&hook))
                .with_bin(Arc::clone(&bin) as Arc<dyn SystemBin>),
        )
        .with_changed_hook(Arc::clone(&hook));
    let renames = history.list(HistoryFilter::Renames, 0, 10).unwrap();
    assert_eq!(renames.total, 1);
    assert_eq!(renames.entries[0].kind, HistoryKind::Rename);
    assert!(media.join("heat.1995.1080p/heat.1995.1080p.mkv").is_file());

    let undone = organize.undo(&rename_batch).unwrap();
    assert_eq!(undone.restored, 1, "{undone:?}");
    assert_eq!(kit::contents(&media), before_rename, "every file is back");
    assert!(
        kit::item(&services, solaris)
            .core
            .path
            .ends_with("solyaris")
    );
    kit::until("the Scraped stage again", || {
        (services.lifecycle().read(&[solaris]).unwrap()[&solaris].stage == Stage::Scraped)
            .then_some(())
    })
    .await;

    // ---- Move a title to another data source, then undo --------------------------------
    // Across drives: copied and verified, the original goes to the bin, Undo brings it back.
    let mover = ChangeSourceService::new(Arc::clone(services.db()))
        .with_bin(Arc::clone(&bin) as Arc<dyn SystemBin>)
        .with_volume_probe(|_, _| false)
        .with_changed_hook(Arc::clone(&hook));
    let before_move = kit::contents(&media);
    let moved = mover.apply(&[alien], archive_id, &mut |_| {}).unwrap();
    assert_eq!(moved.moved(), 1, "{moved:?}");
    let move_batch = moved.batch_id.clone().unwrap();
    assert!(!media.join("Alien (1979)").exists(), "the original left");
    assert_eq!(
        kit::contents(&archive.join("Alien (1979)")),
        before_move
            .iter()
            .filter_map(|(rel, h)| rel
                .strip_prefix("Alien (1979)/")
                .map(|rest| (rest.to_owned(), *h)))
            .collect(),
        "the copy is whole"
    );
    assert_eq!(
        bin.names(),
        [
            "Alien (1979)-poster.png",
            "Alien (1979).mkv",
            "Alien (1979).nfo"
        ],
        "the original files are in the bin"
    );
    assert!(
        kit::item(&services, alien)
            .core
            .path
            .starts_with(archive.to_string_lossy().as_ref())
    );
    let moves = history.list(HistoryFilter::Moves, 0, 10).unwrap();
    assert_eq!(moves.total, 1);
    let move_entry = moves.entries[0].id;
    assert_eq!(
        history.entry_for_batch(&move_batch).unwrap(),
        Some(move_entry)
    );

    let report = history.undo(move_entry).unwrap();
    assert_eq!(report.undone, [alien], "{report:?}");
    assert!(report.left.is_empty());
    assert_eq!(
        kit::contents(&media),
        before_move,
        "the title is back whole"
    );
    assert!(
        kit::item(&services, alien)
            .core
            .path
            .starts_with(media.to_string_lossy().as_ref())
    );
    assert_eq!(
        history.entry(move_entry).unwrap().undo_state,
        UndoState::Undone
    );
    assert!(
        snapshot(&archive).is_empty(),
        "no file is left in the other data source"
    );

    // ---- recovery has nothing to settle; nothing outside the library was touched ---------
    assert!(
        RecoveryService::new(Arc::clone(services.db()))
            .find()
            .unwrap()
            .is_empty()
    );
    assert_eq!(snapshot(&temp.path().join("bystander")), bystander_before);
    assert_eq!(web.hits(), 0, "no remote URL was fetched at any point");
    // The old library and settings are still the ones the update set aside.
    assert_eq!(
        std::fs::read_to_string(root.join("settings-old.json")).unwrap(),
        OLD_SETTINGS
    );
    assert_eq!(
        hash(&std::fs::read(root.join("library-old.db-wal")).unwrap()),
        hash(OLD_WAL)
    );
    services.shutdown();
}

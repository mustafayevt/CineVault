//! The redesigned file operations end to end on a temporary library:
//! scan, Scrape (accept confident), Rename, Move to another data source, Delete, Restore, the
//! History Undo of each kind (move, rename, match) and a permanent delete with its typed
//! confirmation. The disk and the library are checked after every step.
//!
//! Everything is in temporary folders with files of a few bytes. The bin is a folder; the real
//! Recycle Bin / Trash is used only with the existing opt-in (`CINEVAULT_LIVE_BIN=1`), in which
//! case the checks of the bin's folder are skipped. The credential store is in memory.

#[path = "support/flow_kit.rs"]
mod kit;
mod support;

use std::sync::Arc;

use cv_app::history::{HistoryFilter, HistoryKind, HistoryService, UndoState};
use cv_app::library_ops::{
    ChangeSourceService, DeleteMethod, OsTrash, RecoveryService, RemovalService, SystemBin,
};
use cv_app::lifecycle::Stage;
use cv_app::organize::{ApplyRequest, OrganizeService, PreviewRequest, Scope, current_profile};
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::library_ops::is_hidden;
use cv_db::repo::query::count_items;
use cv_db::repo::{ItemQuery, item_exists};
use cv_domain::{ItemId, ItemKind, SourceKind};
use cv_providers::{MetadataProvider, ProviderRegistry};
use kit::{FakeBin, FakeWeb, put, snapshot};
use support::{Entry, Scripted, arc};
use tempfile::TempDir;

fn listed(services: &AppServices) -> u64 {
    services
        .db()
        .read(|c| count_items(c, &ItemQuery::new(ItemKind::Movie)))
        .unwrap()
}

fn hidden(services: &AppServices, id: ItemId) -> bool {
    services.db().read(move |c| is_hidden(c, id)).unwrap()
}

fn stage(services: &AppServices, id: ItemId) -> Stage {
    services.lifecycle().read(&[id]).unwrap()[&id].stage
}

async fn stage_becomes(services: &AppServices, id: ItemId, wanted: Stage) {
    kit::until(&format!("stage {wanted:?}"), || {
        (stage(services, id) == wanted).then_some(())
    })
    .await;
}

/// The path of the title in the library, as a text.
fn path_of(services: &AppServices, id: ItemId) -> String {
    kit::item(services, id).core.path
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_whole_life_of_a_title_scan_scrape_rename_move_delete_restore_undo_and_erase() {
    let temp = TempDir::new().unwrap();
    let web = FakeWeb::start();
    let loc = kit::location(temp.path());
    let movies_dir = temp.path().join("movies");
    let archive_dir = temp.path().join("archive");
    std::fs::create_dir_all(&archive_dir).unwrap();
    // Two titles; Ronin is never touched and must come out exactly as it went in.
    put(
        &movies_dir.join("heat.1995.1080p/heat.1995.1080p.mkv"),
        "heat",
    );
    put(&movies_dir.join("Ronin (1998)/Ronin (1998).mkv"), "ronin");
    put(&temp.path().join("bystander/notes.txt"), "not a library");
    let bystander = snapshot(&temp.path().join("bystander"));
    let ronin_before = snapshot(&movies_dir.join("Ronin (1998)"));

    // The bin: a folder, or the real one when opted in.
    let live_bin = std::env::var("CINEVAULT_LIVE_BIN").as_deref() == Ok("1");
    let fake = FakeBin::new(temp.path());
    let bin: Arc<dyn SystemBin> = if live_bin {
        Arc::new(OsTrash)
    } else {
        Arc::clone(&fake) as Arc<dyn SystemBin>
    };

    // The fake TMDB; the key is in an in-memory store.
    let tmdb = arc(Scripted::new(
        "tmdb",
        vec![
            Entry::new("949", "Heat", 1995),
            Entry::new("2", "Ronin", 1998),
        ],
    ));
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(Arc::clone(&tmdb) as Arc<dyn MetadataProvider>);
    let secrets = Arc::new(SecretStore::in_memory());
    secrets.set("tmdb.api_key", "cvtest-tmdb-key").unwrap();
    let providers = ProviderCredentialsService::new(Arc::clone(&secrets), registry);
    let mut options = BootOptions::new(loc.clone(), secrets).with_providers(providers);
    // Changes are announced at once, so a test can read them right after the write.
    options.change_window = std::time::Duration::ZERO;
    let services = AppServices::boot(options).expect("boot");
    services.scan().set_background_scans(false);
    services.matching().set_bin(Arc::clone(&bin));
    services
        .settings()
        .set("ratings.fetch-after-scrape-movie", serde_json::json!(false))
        .unwrap();
    // The services the app builds for these operations, over the same library and hook.
    let hook = services.organize_state().hook();
    let organize = || {
        OrganizeService::new(Arc::clone(services.db()))
            .with_changed_hook(Arc::clone(&hook))
            .with_bin(Arc::clone(&bin))
    };
    let history = HistoryService::new(Arc::clone(services.db()))
        .with_bin(Arc::clone(&bin))
        .with_organize(organize())
        .with_changed_hook(Arc::clone(&hook));
    let removal = RemovalService::new(Arc::clone(services.db())).with_bin(Arc::clone(&bin));
    let mover = ChangeSourceService::new(Arc::clone(services.db()))
        .with_bin(Arc::clone(&bin))
        .with_volume_probe(|_, _| false)
        .with_changed_hook(Arc::clone(&hook));

    // ---- scan --------------------------------------------------------------------------
    let source = movies_dir.to_string_lossy().into_owned();
    services
        .db()
        .write(move |tx| insert_source(tx, SourceKind::Movie, &source))
        .unwrap();
    let archive_text = archive_dir.to_string_lossy().into_owned();
    let archive_id = services
        .db()
        .write(move |tx| insert_source(tx, SourceKind::Movie, &archive_text))
        .unwrap()
        .id;
    services
        .scan()
        .rescan_all()
        .unwrap()
        .handle
        .job
        .wait()
        .await;
    assert_eq!(listed(&services), 2);
    let all = kit::movies(&services);
    let heat = kit::by_folder(&all, "heat.1995.1080p").id.unwrap();
    let ronin = kit::by_folder(&all, "Ronin (1998)").id.unwrap();
    assert_eq!(stage(&services, heat), Stage::New);
    assert_eq!(stage(&services, ronin), Stage::New);
    assert_eq!(kit::item(&services, heat).core.title.to_lowercase(), "heat");
    // A scan reads only: no file appeared in the media folders.
    assert_eq!(
        kit::tree(&movies_dir),
        [
            "Ronin (1998)/",
            "Ronin (1998)/Ronin (1998).mkv",
            "heat.1995.1080p/",
            "heat.1995.1080p/heat.1995.1080p.mkv"
        ]
    );
    assert!(kit::snapshot(&archive_dir).is_empty());

    // ---- Scrape: accept confident -------------------------------------------------------
    let accepted = services.matching().accept_confident(&[heat]).await.unwrap();
    assert_eq!(accepted.accepted.len(), 1, "{accepted:?}");
    let match_entry = accepted.history_entry.expect("a History entry");
    services.scrape_follow_up().idle().await;
    kit::until("the steps after the scrape", || {
        (!services.lifecycle().working().is_working(heat)).then_some(())
    })
    .await;
    let scraped = kit::item(&services, heat);
    assert_eq!(scraped.core.ids.get("tmdb"), Some("949"));
    assert_eq!(scraped.core.last_scraper_id, "tmdb");
    assert_eq!(stage(&services, heat), Stage::Scraped);
    let heat_dir = movies_dir.join("heat.1995.1080p");
    assert!(
        heat_dir.join("heat.1995.1080p.nfo").is_file(),
        "the scrape wrote the NFO: {:?}",
        kit::tree(&heat_dir)
    );
    assert_eq!(
        std::fs::read(heat_dir.join("heat.1995.1080p.mkv")).unwrap(),
        b"heat"
    );
    assert_eq!(stage(&services, ronin), Stage::New, "only Heat was scraped");
    let entry = history.entry(match_entry).unwrap();
    assert_eq!(entry.kind, HistoryKind::Match);
    assert_eq!(entry.undo_state, UndoState::Available);
    let scraped_files = kit::contents(&heat_dir);

    // ---- Rename -------------------------------------------------------------------------
    let applied = organize()
        .apply(&ApplyRequest {
            preview: PreviewRequest {
                scope: Scope::Everything,
                profile: current_profile(&services.settings().values()),
                choices: Default::default(),
            },
            include: vec![heat.0.to_string()],
        })
        .unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");
    let renamed_dir = movies_dir.join("Heat (1995)");
    assert!(!heat_dir.exists());
    assert!(renamed_dir.join("Heat (1995).mkv").is_file());
    assert!(renamed_dir.join("Heat (1995).nfo").is_file());
    assert!(path_of(&services, heat).ends_with("Heat (1995)"));
    stage_becomes(&services, heat, Stage::Renamed).await;
    let renamed_files = kit::contents(&renamed_dir);
    let rename_entry = history.list(HistoryFilter::Renames, 0, 10).unwrap().entries[0].id;
    assert_eq!(
        history.entry(rename_entry).unwrap().undo_state,
        UndoState::Available
    );
    assert_eq!(snapshot(&movies_dir.join("Ronin (1998)")), ronin_before);

    // ---- Move to another data source (another drive: copied, verified, original binned) ---
    let moved = mover.apply(&[heat], archive_id, &mut |_| {}).unwrap();
    assert_eq!(moved.moved(), 1, "{moved:?}");
    let archived_dir = archive_dir.join("Heat (1995)");
    assert!(!renamed_dir.exists(), "the original left the source");
    assert_eq!(
        kit::contents(&archived_dir),
        renamed_files,
        "the copy is whole"
    );
    assert!(path_of(&services, heat).ends_with("Heat (1995)"));
    assert!(path_of(&services, heat).starts_with(archive_dir.to_string_lossy().as_ref()));
    if !live_bin {
        assert_eq!(
            fake.names(),
            ["Heat (1995).mkv", "Heat (1995).nfo"],
            "the originals are in the bin"
        );
    }
    let move_entry = history.list(HistoryFilter::Moves, 0, 10).unwrap().entries[0].id;
    stage_becomes(&services, heat, Stage::Renamed).await;

    // ---- Delete (to the bin) ------------------------------------------------------------
    let deleted = removal.delete_with_files(&[heat]).unwrap();
    assert_eq!(deleted.deleted.len(), 1, "{deleted:?}");
    assert!(deleted.problems.is_empty(), "{:?}", deleted.problems);
    assert_ne!(deleted.deleted[0].method, DeleteMethod::NothingOnDisk);
    let delete_entry = deleted.history_id.expect("a History entry");
    assert!(!archived_dir.exists(), "the files left the folder");
    assert!(hidden(&services, heat));
    assert_eq!(listed(&services), 1, "a deleted title is in no list");
    assert!(
        services.db().read(move |c| item_exists(c, heat)).unwrap(),
        "its row is kept, hidden"
    );
    if !live_bin {
        assert_eq!(
            fake.names(),
            ["Heat (1995)", "Heat (1995).mkv", "Heat (1995).nfo"],
            "the folder joined the files of the move in the bin"
        );
    }
    assert_eq!(
        history.entry(delete_entry).unwrap().kind,
        HistoryKind::Delete
    );
    assert_eq!(snapshot(&movies_dir.join("Ronin (1998)")), ronin_before);

    // ---- Restore (the Undo of the delete) -------------------------------------------------
    let restored = history.undo(delete_entry).unwrap();
    assert_eq!(restored.undone, [heat], "{restored:?}");
    assert!(restored.left.is_empty());
    assert!(!hidden(&services, heat));
    assert_eq!(listed(&services), 2);
    assert_eq!(
        kit::contents(&archived_dir),
        renamed_files,
        "back with its files"
    );
    assert_eq!(
        history.entry(delete_entry).unwrap().undo_state,
        UndoState::Undone
    );
    stage_becomes(&services, heat, Stage::Renamed).await;

    // ---- History Undo of each kind, newest first ------------------------------------------
    // Move: the title goes back to its source, the library follows.
    let report = history.undo(move_entry).unwrap();
    assert_eq!(report.undone, [heat], "{report:?}");
    assert_eq!(kit::contents(&renamed_dir), renamed_files);
    assert!(path_of(&services, heat).starts_with(movies_dir.to_string_lossy().as_ref()));
    assert!(
        kit::snapshot(&archive_dir).is_empty(),
        "no file is left in the other data source"
    );
    assert_eq!(
        history.entry(move_entry).unwrap().undo_state,
        UndoState::Undone
    );

    // Rename: the folder and the files have their old names, the NFO is the scrape's.
    let report = history.undo(rename_entry).unwrap();
    assert_eq!(report.undone, [heat], "{report:?}");
    assert!(!renamed_dir.exists());
    assert_eq!(kit::contents(&heat_dir), scraped_files);
    assert!(path_of(&services, heat).ends_with("heat.1995.1080p"));
    stage_becomes(&services, heat, Stage::Scraped).await;

    // Match: the title is New again, as before the scrape.
    let report = services
        .matching()
        .undo_entry(match_entry, None)
        .await
        .unwrap();
    assert_eq!(report.undone, [heat], "{report:?}");
    let back = kit::item(&services, heat);
    assert_eq!(back.core.ids.get("tmdb"), None);
    assert_eq!(back.core.last_scraper_id, "");
    assert_eq!(
        std::fs::read(heat_dir.join("heat.1995.1080p.mkv")).unwrap(),
        b"heat"
    );
    kit::until("the New stage", || {
        (stage(&services, heat) == Stage::New).then_some(())
    })
    .await;
    // The title was never scraped before: the scrape's NFO went to the bin with the Undo, and
    // no NFO is written for a New title, so the folder is as the scan found it.
    assert_eq!(kit::tree(&heat_dir), ["heat.1995.1080p.mkv"]);
    assert!(!services.scrape_follow_up().is_writing(heat));
    for id in [match_entry, rename_entry, move_entry, delete_entry] {
        assert_eq!(history.entry(id).unwrap().undo_state, UndoState::Undone);
    }
    // Nothing is left half done.
    assert!(
        RecoveryService::new(Arc::clone(services.db()))
            .find()
            .unwrap()
            .is_empty()
    );

    // ---- Permanent delete, with the typed confirmation ------------------------------------
    let sends_before = fake.sends();
    let before_erase = kit::tree(&heat_dir);
    assert_eq!(before_erase, ["heat.1995.1080p.mkv"]);
    for typed in ["", "delete", "DELETE ", "yes"] {
        let refused = removal.erase(&[heat], typed).unwrap_err();
        assert_eq!(refused.code(), "confirmation");
    }
    assert_eq!(kit::tree(&heat_dir), before_erase, "nothing was deleted");
    assert!(services.db().read(move |c| item_exists(c, heat)).unwrap());

    let erased = removal.erase(&[heat], "DELETE").unwrap();
    assert_eq!(erased.erased.len(), 1, "{erased:?}");
    assert!(erased.stopped.is_empty() && erased.problems.is_empty());
    assert!(
        !heat_dir.exists(),
        "the folder is gone for good: {:?}",
        kit::tree(&heat_dir)
    );
    assert!(
        !services.db().read(move |c| item_exists(c, heat)).unwrap(),
        "an erased title leaves the library"
    );
    assert_eq!(listed(&services), 1);
    assert_eq!(fake.sends(), sends_before, "nothing went to the bin");
    let entry = history.entry(erased.history_id.unwrap()).unwrap();
    assert_eq!(entry.kind, HistoryKind::DeletePermanent);
    assert_eq!(entry.undo_state, UndoState::None);
    assert!(history.undo(entry.id).is_err(), "it cannot be undone");

    // ---- the rest of the world is as it was ---------------------------------------------
    assert_eq!(snapshot(&movies_dir.join("Ronin (1998)")), ronin_before);
    assert_eq!(snapshot(&temp.path().join("bystander")), bystander);
    assert_eq!(web.hits(), 0);
    assert!(movies_dir.is_dir());
    services.shutdown();
}

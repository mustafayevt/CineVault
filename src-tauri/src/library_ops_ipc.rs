//! The Tauri surface of the library operations (moves, removal, delete, review).
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::library_ops`].

use tauri::State;
use tauri::ipc::Channel;

use crate::sources_ipc::LibraryState;

pub use cv_ipc::library_ops::*;

/// Says what moving the titles to the data source would do. Changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn plan_change_source(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
    target: u32,
) -> Result<MovePlanDto, LibraryOpsCommandError> {
    cv_ipc::library_ops::plan_change_source(&library, &ops, ids, target).await
}

/// Says exactly what moving one title to the data source would copy or rename: every file and
/// folder with its destination below `row.to`, the total size, whether it is a rename or a
/// copy, the free-space check and the destination conflicts. Changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn preview_change_source_title(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    id: String,
    target: u32,
) -> Result<MoveTitlePreviewDto, LibraryOpsCommandError> {
    cv_ipc::library_ops::preview_change_source_title(&library, &ops, id, target).await
}

/// Moves the titles to the data source as one undoable batch. Progress goes to `on_progress`.
#[tauri::command]
#[specta::specta]
pub async fn apply_change_source(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
    target: u32,
    on_progress: Channel<MoveProgressDto>,
) -> Result<MoveAppliedDto, LibraryOpsCommandError> {
    cv_ipc::library_ops::apply_change_source(
        &library,
        &ops,
        ids,
        target,
        crate::ipc_sink::sink(on_progress),
    )
    .await
}

/// Pauses, resumes or cancels the move that is running. Does nothing when none is.
#[tauri::command]
#[specta::specta]
pub fn control_move(ops: State<'_, LibraryOpsState>, action: MoveControlDto) {
    cv_ipc::library_ops::control_move(&ops, action)
}

/// Lists the moves and renames that did not finish, newest first.
#[tauri::command]
#[specta::specta]
pub async fn list_unfinished_batches(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
) -> Result<Vec<UnfinishedBatchDto>, LibraryOpsCommandError> {
    cv_ipc::library_ops::list_unfinished_batches(&library, &ops).await
}

/// Carries on an unfinished move or rename: the files already copied are checked again against
/// their originals, then the rest is copied and the titles are put in place. Progress goes to
/// `on_progress`; `control_move` pauses and cancels it.
#[tauri::command]
#[specta::specta]
pub async fn resume_unfinished_batch(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    batch_id: String,
    on_progress: Channel<MoveProgressDto>,
) -> Result<ResumeOutcomeDto, LibraryOpsCommandError> {
    cv_ipc::library_ops::resume_unfinished_batch(
        &library,
        &ops,
        batch_id,
        crate::ipc_sink::sink(on_progress),
    )
    .await
}

/// Takes an unfinished move or rename back, removing only what the batch itself made (the
/// `.cvpart` files, the copies and the staging folder); the original files were never touched.
#[tauri::command]
#[specta::specta]
pub async fn roll_back_unfinished_batch(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    batch_id: String,
) -> Result<RollBackOutcomeDto, LibraryOpsCommandError> {
    cv_ipc::library_ops::roll_back_unfinished_batch(&library, &ops, batch_id).await
}

/// Says what removing or deleting the titles would touch. Changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn plan_removal(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
) -> Result<RemovalPlanDto, LibraryOpsCommandError> {
    cv_ipc::library_ops::plan_removal(&library, &ops, ids).await
}

/// Takes the titles out of the library and leaves their files alone. With `ignore_folders`, each
/// title that has its folder to itself also gets a `.cinevaultignore` marker.
#[tauri::command]
#[specta::specta]
pub async fn remove_from_library(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
    ignore_folders: bool,
) -> Result<RemoveReportDto, LibraryOpsCommandError> {
    cv_ipc::library_ops::remove_from_library(&library, &ops, ids, ignore_folders).await
}

/// Sends the titles' files to the system trash and removes the titles from the library. Files
/// the system trash cannot take go to the data source's trash folder; nothing is deleted for good.
#[tauri::command]
#[specta::specta]
pub async fn delete_with_files(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
) -> Result<DeleteReportDto, LibraryOpsCommandError> {
    cv_ipc::library_ops::delete_with_files(&library, &ops, ids).await
}

/// The titles that are missing on disk, offline, duplicates of another row or set aside by a
/// rescan, each with its reason, size and last-seen date. Reads only: nothing is removed.
#[tauri::command]
#[specta::specta]
pub async fn list_library_review(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
) -> Result<ReviewListDto, LibraryOpsCommandError> {
    cv_ipc::library_ops::list_library_review(&library, &ops).await
}

/// Takes the chosen review rows out of the library. Their files stay on disk. Only rows that the
/// review lists can be removed.
#[tauri::command]
#[specta::specta]
pub async fn remove_review_rows(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
) -> Result<ReviewRemovalDto, LibraryOpsCommandError> {
    cv_ipc::library_ops::remove_review_rows(&library, &ops, ids).await
}

/// Puts back the rows of a removal (the Undo of `remove_review_rows`). The titles get new ids.
#[tauri::command]
#[specta::specta]
pub async fn undo_review_removal(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    token: String,
) -> Result<Vec<RestoredTitleDto>, LibraryOpsCommandError> {
    cv_ipc::library_ops::undo_review_removal(&library, &ops, token).await
}

/// Leaves the chosen rows in the library and out of the review until they have a different
/// problem. Returns how many were kept.
#[tauri::command]
#[specta::specta]
pub async fn keep_review_rows(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
) -> Result<u32, LibraryOpsCommandError> {
    cv_ipc::library_ops::keep_review_rows(&library, &ops, ids).await
}

/// Takes back "keep": the rows are listed again while they still have a problem.
#[tauri::command]
#[specta::specta]
pub async fn unkeep_review_rows(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
) -> Result<(), LibraryOpsCommandError> {
    cv_ipc::library_ops::unkeep_review_rows(&library, &ops, ids).await
}

/// Points a missing or offline movie or show at the folder its files moved to. No file is
/// touched; `from` lets the page offer to point it back.
#[tauri::command]
#[specta::specta]
pub async fn relocate_review_row(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    id: String,
    folder: String,
) -> Result<RelocatedDto, LibraryOpsCommandError> {
    cv_ipc::library_ops::relocate_review_row(&library, &ops, id, folder).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use cv_app::library_ops::*;
    use cv_artwork::library_source::LibrarySources;
    use cv_domain::ItemId;

    #[test]
    fn ids_are_numbers_and_anything_else_is_refused() {
        assert_eq!(
            ids_of(&["12".to_owned(), " 7 ".to_owned()]).unwrap(),
            vec![ItemId(12), ItemId(7)]
        );
        assert_eq!(ids_of(&["x".to_owned()]).unwrap_err().code, "bad_id");
    }

    #[test]
    fn a_status_becomes_a_code_and_a_reason() {
        assert_eq!(status_parts(&MoveStatus::Ready), ("ready", String::new()));
        assert_eq!(
            status_parts(&MoveStatus::Unavailable("gone".into())),
            ("unavailable", "gone".to_owned())
        );
    }

    #[test]
    fn a_move_row_carries_the_space_check_and_the_conflicts() {
        let row = MoveRow {
            id: ItemId(4),
            kind: TitleKind::Movie,
            title: "Heat".into(),
            year: 1995,
            source: "D:/Movies".into(),
            from: "D:/Movies/Heat (1995)".into(),
            to: "Z:/NAS 4K/Heat (1995)".into(),
            status: MoveStatus::InsufficientSpace {
                needed: 62,
                free: 40,
            },
            files: 3,
            bytes: 60,
            across_volumes: true,
            entries: vec![
                MoveEntry {
                    path: "Heat.mkv".into(),
                    kind: EntryKind::File,
                    size: 60,
                },
                MoveEntry {
                    path: "link".into(),
                    kind: EntryKind::Link {
                        target: "/elsewhere".into(),
                    },
                    size: 0,
                },
            ],
            needed_bytes: 62,
            free_bytes: Some(40),
            conflicts: vec!["Z:/NAS 4K/Heat (1995)".into()],
        };
        let entries: Vec<MoveEntryDto> = row
            .entries
            .iter()
            .cloned()
            .map(MoveEntryDto::from)
            .collect();
        let dto = MoveRowDto::from(row);
        assert_eq!(dto.status, "insufficient_space");
        assert_eq!((dto.needed_bytes, dto.free_bytes), (62.0, Some(40.0)));
        assert_eq!(dto.conflicts, ["Z:/NAS 4K/Heat (1995)"]);
        assert_eq!(entries[0].kind, "file");
        assert_eq!(
            (entries[1].kind.as_str(), entries[1].link_target.as_str()),
            ("link", "/elsewhere")
        );
    }

    #[test]
    fn an_unfinished_batch_names_its_titles_progress_and_reason() {
        let dto = UnfinishedBatchDto::from(UnfinishedBatch {
            batch_id: "move-1700000000000-1".into(),
            kind: BatchKind::Move,
            created_ms: 1_700_000_000_000,
            titles: vec!["Heat".into()],
            items: 1,
            files_done: 3,
            files_total: 9,
            reason: "Destination is full.".into(),
            in_new_place: false,
        });
        assert_eq!(dto.kind, "move");
        assert_eq!((dto.files_done, dto.files_total, dto.items), (3, 9, 1));
        assert_eq!(dto.reason, "Destination is full.");
        let stopped = MoveResult::Stopped("Destination is full".into());
        let applied = MoveApplied {
            batch_id: Some("move-1".into()),
            items: vec![cv_app::library_ops::MoveOutcome {
                id: ItemId(1),
                title: "Heat".into(),
                result: stopped,
                notes: Vec::new(),
            }],
        };
        let dto = MoveAppliedDto::from(applied);
        assert_eq!(dto.items[0].result, "stopped");
        assert_eq!(dto.items[0].reason, "Destination is full");
    }

    /// The missing/duplicate review through its commands, the way the page calls them: list,
    /// keep and unkeep, remove with Undo, and relocate.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_review_commands_list_keep_remove_undo_and_relocate() {
        use std::path::Path;

        use cv_app::data_location::{DataLocation, LocationInputs};
        use cv_app::secrets::SecretStore;
        use cv_app::services::{AppServices, BootOptions};
        use cv_db::repo::data_sources::insert_source;
        use cv_db::repo::{StdFileProbe, get_item, insert_item};
        use cv_domain::{Item, MediaFile, MediaFileType, SourceKind};
        use cv_scan::sources::{PathStyle, SourceGuard, SystemFolders};
        use tauri::Manager;
        use tauri::test::{mock_builder, mock_context, noop_assets};

        let temp = tempfile::tempdir().unwrap();
        let inputs = LocationInputs {
            exe_dir: temp.path().join("app"),
            user_data_base: temp.path().join("userdata"),
            user_config_base: temp.path().join("userconfig"),
        };
        std::fs::create_dir_all(&inputs.exe_dir).unwrap();
        let services = AppServices::boot(BootOptions::new(
            DataLocation::resolve(&inputs),
            Arc::new(SecretStore::in_memory()),
        ))
        .expect("boot");
        let db = Arc::clone(services.db());
        let movies = temp.path().join("media").join("Movies");
        let archive = temp.path().join("media").join("Archive");
        let (movies_root, archive_root) = (
            movies.to_string_lossy().into_owned(),
            archive.to_string_lossy().into_owned(),
        );
        {
            let (a, b) = (movies_root.clone(), archive_root.clone());
            db.write(move |tx| {
                insert_source(tx, SourceKind::Movie, &a)?;
                insert_source(tx, SourceKind::Movie, &b)?;
                Ok(())
            })
            .unwrap();
        }
        let add_movie = |root: &Path, name: &str| -> ItemId {
            let folder = root.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(format!("{name}.mkv")), b"video").unwrap();
            let mut item = Item::movie(name.split(" (").next().unwrap());
            item.core.year = 1995;
            item.core.data_source = root.to_string_lossy().into_owned();
            item.core.path = folder.to_string_lossy().into_owned();
            let mut video = MediaFile::new(
                MediaFileType::Video,
                folder.to_string_lossy(),
                format!("{name}.mkv"),
            );
            video.size_bytes = 5;
            item.core.media_files.push(video);
            db.write(move |tx| insert_item(tx, &item)).unwrap()
        };
        let gone = add_movie(&movies, "Gone (1995)");
        let moved = add_movie(&movies, "Moved (1995)");
        add_movie(&movies, "Fine (1995)");
        std::fs::remove_dir_all(movies.join("Gone (1995)")).unwrap();
        std::fs::create_dir_all(&archive).unwrap();
        std::fs::rename(movies.join("Moved (1995)"), archive.join("Moved (1995)")).unwrap();
        db.refresh_availability(&StdFileProbe, cv_db::repo::PathStyle::current())
            .unwrap();

        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let guard = SourceGuard::new(
            PathStyle::current(),
            SystemFolders::new(PathStyle::current()),
        );
        app.manage(LibraryState::with_guard(&services, guard));
        app.manage(LibraryOpsState::new(Arc::new(LibrarySources::new())));
        let library = || app.state::<LibraryState>();
        let ops = || app.state::<LibraryOpsState>();
        let listed = |list: &ReviewListDto| -> Vec<(String, String)> {
            let mut rows: Vec<(String, String)> = list
                .rows
                .iter()
                .map(|row| (row.title.clone(), row.reason.clone()))
                .collect();
            rows.sort();
            rows
        };

        let list = list_library_review(library(), ops()).await.expect("list");
        assert_eq!(
            listed(&list),
            [
                ("Gone".to_owned(), "missing".to_owned()),
                ("Moved".to_owned(), "missing".to_owned())
            ]
        );
        assert_eq!(list.kept, 0);

        // Keep leaves the row in the library and out of the list; unkeep lists it again.
        let kept = keep_review_rows(library(), ops(), vec![gone.0.to_string()])
            .await
            .expect("keep");
        assert_eq!(kept, 1);
        let list = list_library_review(library(), ops()).await.expect("list");
        assert_eq!(list.kept, 1);
        assert!(list.rows.iter().all(|row| row.id != gone.0.to_string()));
        unkeep_review_rows(library(), ops(), vec![gone.0.to_string()])
            .await
            .expect("unkeep");
        assert_eq!(
            list_library_review(library(), ops())
                .await
                .expect("list")
                .rows
                .len(),
            2
        );

        // Remove takes the row out; its Undo puts it back under a new id.
        let removed = remove_review_rows(library(), ops(), vec![gone.0.to_string()])
            .await
            .expect("remove");
        assert_eq!(removed.removed.len(), 1);
        assert!(db.read(move |c| get_item(c, gone)).unwrap().is_none());
        let token = removed.undo_token.expect("an undo token");
        let restored = undo_review_removal(library(), ops(), token)
            .await
            .expect("undo");
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].was, gone.0.to_string());
        assert_eq!(restored[0].title, "Gone");
        // An id that is not a number is refused before anything runs.
        let refused = keep_review_rows(library(), ops(), vec!["x".to_owned()])
            .await
            .unwrap_err();
        assert_eq!(refused.code, "bad_id");

        // Relocate points the moved title at its new folder; no file is touched.
        let to = archive.join("Moved (1995)").to_string_lossy().into_owned();
        let relocated = relocate_review_row(library(), ops(), moved.0.to_string(), to.clone())
            .await
            .expect("relocate");
        assert_eq!(relocated.availability, "online");
        assert_eq!(relocated.to, to);
        assert!(
            archive
                .join("Moved (1995)")
                .join("Moved (1995).mkv")
                .is_file()
        );
        let list = list_library_review(library(), ops()).await.expect("list");
        assert_eq!(listed(&list), [("Gone".to_owned(), "missing".to_owned())]);
    }

    #[test]
    fn a_review_row_keeps_its_reason_size_and_original() {
        use cv_app::library_ops::{DuplicateOf, ReviewReason};
        let row = ReviewRow {
            id: ItemId(9),
            kind: TitleKind::Show,
            title: "Breaking Bad".into(),
            year: 2008,
            source: "D:/TV".into(),
            folder: "D:/TV/Breaking Bad".into(),
            reason: ReviewReason::Duplicate,
            detail: "Duplicate of Breaking Bad (2008)".into(),
            duplicate_of: Some(DuplicateOf {
                id: ItemId(3),
                title: "Breaking Bad".into(),
                year: 2008,
            }),
            files: 62,
            bytes: 4_000,
            last_seen_ms: 1_700_000_000_000,
            can_relocate: false,
        };
        let dto = ReviewRowDto::from(row);
        assert_eq!((dto.id.as_str(), dto.kind.as_str()), ("9", "show"));
        assert_eq!(dto.reason, "duplicate");
        assert_eq!(dto.duplicate_of.map(|of| of.id), Some("3".to_owned()));
        assert_eq!((dto.files, dto.bytes), (62, 4_000.0));
        assert!(!dto.can_relocate);
    }

    /// The missing and duplicate review through its commands: list, keep and take it
    /// back, remove with Undo, and relocate. Nothing is ever removed without a command.
    mod commands {
        use std::path::Path;
        use std::sync::Arc;

        use cv_app::data_location::{DataLocation, LocationInputs};
        use cv_app::secrets::SecretStore;
        use cv_app::services::{AppServices, BootOptions};
        use cv_domain::{Availability, Item, ItemId, SourceKind};
        use cv_scan::sources::{PathStyle, SourceGuard, SystemFolders};
        use tauri::Manager;
        use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

        use super::super::*;
        use crate::boot::Boot;
        use cv_artwork::library_source::LibrarySources;

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
            .without_providers();
            let services = AppServices::boot(options).expect("boot");
            let boot = Boot::for_services(Arc::clone(&services));
            let app = mock_builder()
                .build(mock_context(noop_assets()))
                .expect("mock app builds");
            let guard = SourceGuard::new(
                PathStyle::current(),
                SystemFolders::new(PathStyle::current()),
            );
            app.manage(LibraryState::with_guard(&services, guard));
            app.manage(LibraryOpsState::new(Arc::new(LibrarySources::new())));
            app.manage(boot);
            (app, services)
        }

        /// A movie whose folder is gone from the movie library folder `library`.
        fn missing_movie(services: &AppServices, library: &Path, title: &str) -> ItemId {
            let library_path = library.to_string_lossy().into_owned();
            let mut item = Item::movie(title);
            item.core.year = 1995;
            item.core.path = library.join(title).to_string_lossy().into_owned();
            item.core.data_source.clone_from(&library_path);
            services
                .db()
                .write(move |conn| {
                    let id = cv_db::repo::insert_item(conn, &item)?;
                    cv_db::repo::set_availability(conn, id, Availability::Missing, "gone")?;
                    Ok(id)
                })
                .unwrap()
        }

        async fn rows(app: &tauri::App<MockRuntime>) -> ReviewListDto {
            list_library_review(app.state(), app.state())
                .await
                .expect("list")
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn keep_remove_undo_and_relocate_go_through_the_commands() {
            let root = tempfile::tempdir().unwrap();
            let library = root.path().join("Movies");
            std::fs::create_dir_all(&library).unwrap();
            let (app, services) = app(root.path());
            let library_text = library.to_string_lossy().into_owned();
            services
                .db()
                .write(move |conn| {
                    cv_db::repo::data_sources::insert_source(conn, SourceKind::Movie, &library_text)
                })
                .unwrap();
            let heat = missing_movie(&services, &library, "Heat (1995)");
            let ronin = missing_movie(&services, &library, "Ronin (1995)");

            let listed = rows(&app).await;
            assert_eq!(listed.rows.len(), 2);
            assert!(listed.rows.iter().all(|row| row.reason == "missing"));

            // Keep leaves the row in the library and out of the list; unkeep lists it again.
            let kept = keep_review_rows(app.state(), app.state(), vec![heat.0.to_string()])
                .await
                .expect("keep");
            assert_eq!(kept, 1);
            let listed = rows(&app).await;
            assert_eq!((listed.rows.len(), listed.kept), (1, 1));
            let still = services
                .db()
                .read(move |conn| cv_db::repo::get_item(conn, heat))
                .unwrap();
            assert!(still.is_some(), "a kept row stays in the library");
            unkeep_review_rows(app.state(), app.state(), vec![heat.0.to_string()])
                .await
                .expect("unkeep");
            assert_eq!(rows(&app).await.rows.len(), 2);

            // Remove takes the row out (its files, had it any, stay); Undo puts it back.
            let removed = remove_review_rows(app.state(), app.state(), vec![ronin.0.to_string()])
                .await
                .expect("remove");
            assert_eq!(removed.removed.len(), 1);
            let token = removed.undo_token.expect("an undo token");
            assert_eq!(rows(&app).await.rows.len(), 1);
            let gone = services
                .db()
                .read(move |conn| cv_db::repo::get_item(conn, ronin))
                .unwrap();
            assert!(gone.is_none());
            let restored = undo_review_removal(app.state(), app.state(), token)
                .await
                .expect("undo");
            assert_eq!(restored.len(), 1);
            assert_eq!(restored[0].title, "Ronin (1995)");
            assert_eq!(rows(&app).await.rows.len(), 2);

            // Relocate points the title at the folder its files moved to: it leaves the list.
            let moved = library.join("Heat 1995 moved");
            std::fs::create_dir_all(&moved).unwrap();
            std::fs::write(moved.join("heat.mkv"), b"video").unwrap();
            let relocated = relocate_review_row(
                app.state(),
                app.state(),
                heat.0.to_string(),
                moved.to_string_lossy().into_owned(),
            )
            .await
            .expect("relocate");
            assert_eq!(relocated.id, heat.0.to_string());
            assert_eq!(relocated.to, moved.to_string_lossy());
            let listed = rows(&app).await;
            assert!(listed.rows.iter().all(|row| row.id != heat.0.to_string()));
        }
    }
}

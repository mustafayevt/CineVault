//! A rename stopped between two steps of a title (data-safety review): found at the
//! next start, resumed to completion, or rolled back title by title like a move.

use std::panic::AssertUnwindSafe;

use cv_rename::conflict::BatchItem;
use cv_rename::exec::{
    DbJournal, ExecOptions, Executor, Fs, JournalStore, Meta, StdFs, new_batch_id,
};
use cv_rename::{Fate, FolderChange, Owner, PlannedFile};

use super::*;
use crate::library_ops::{BatchKind, RecoveryService};

/// The disk, which dies (the process is killed) when the NFO step starts after `after` renames
/// were made: between two journaled steps, when every row is settled.
struct DiesBetweenSteps {
    renames: AtomicUsize,
    after: usize,
}

impl Fs for DiesBetweenSteps {
    fn meta(&self, path: &Path) -> std::io::Result<Option<Meta>> {
        let nfo = path.extension().is_some_and(|e| e == "nfo");
        if nfo && self.renames.load(Ordering::SeqCst) == self.after {
            std::panic::resume_unwind(Box::new("killed"));
        }
        StdFs.meta(path)
    }
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        StdFs.create_dir_all(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFs.rename(from, to)?;
        self.renames.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn copy_file(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFs.copy_file(from, to)
    }
    fn same_content(&self, a: &Path, b: &Path) -> std::io::Result<bool> {
        StdFs.same_content(a, b)
    }
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFs.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFs.remove_dir(path)
    }
    fn list_dir(&self, path: &Path) -> std::io::Result<Vec<String>> {
        StdFs.list_dir(path)
    }
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The plan Organize makes for `<name>` → `<name> [x]`: the folder, the video and the NFO
/// (the poster keeps its name).
fn tagged(lab: &Lab, id: ItemId, name: &str) -> BatchItem {
    let new = format!("{name} [x]");
    let file = |kind: MediaFileType, from: String, to: String| PlannedFile {
        from,
        kind,
        owner: Owner::Movie,
        fate: Fate::Carried { to: vec![to] },
    };
    BatchItem {
        id: id.0.to_string(),
        root: lab.movies.path.clone(),
        old_folder: name.to_owned(),
        new_folder: new.clone(),
        folder_change: FolderChange::Rename,
        base_name: Some(new.clone()),
        version_label: None,
        shares_folder: false,
        files: vec![
            file(
                MediaFileType::Video,
                format!("{name}/{name}.mkv"),
                format!("{new}/{new}.mkv"),
            ),
            file(
                MediaFileType::Nfo,
                format!("{name}/{name}.nfo"),
                format!("{new}/{new}.nfo"),
            ),
            file(
                MediaFileType::Poster,
                format!("{name}/poster.jpg"),
                format!("{new}/poster.jpg"),
            ),
        ],
    }
}

/// Renames Alpha and Beta in one batch; the process is killed inside Beta, after its folder and
/// its video were renamed and before its NFO step was written to the journal. Then the app
/// starts again.
fn killed_inside_the_second_title(lab: &Lab) -> (String, ItemId, ItemId) {
    let alpha = lab.movie(&lab.movies, "Alpha (1995)");
    let beta = lab.movie(&lab.movies, "Beta (1995)");
    let items = [
        tagged(lab, alpha, "Alpha (1995)"),
        tagged(lab, beta, "Beta (1995)"),
    ];
    let batch = new_batch_id();
    let disk = DiesBetweenSteps {
        renames: AtomicUsize::new(0),
        // Alpha: folder, video, NFO. Beta: folder, video, then killed.
        after: 5,
    };
    let killed = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let mut journal = DbJournal::new(Arc::clone(&lab.db));
        let options =
            ExecOptions::new(batch.clone()).with_protected_dirs([PathBuf::from(&lab.movies.path)]);
        Executor::new(&disk, &mut journal, options)
            .unwrap()
            .run(&items)
    }));
    assert!(killed.is_err(), "the run was killed");
    let root = Path::new(&lab.movies.path);
    assert_eq!(
        files_in(&root.join("Beta (1995) [x]")),
        ["Beta (1995) [x].mkv", "Beta (1995).nfo", "poster.jpg"],
        "half renamed"
    );
    // Every row is settled: the stop fell between two steps.
    assert!(
        DbJournal::new(Arc::clone(&lab.db))
            .pending()
            .unwrap()
            .is_empty()
    );

    crate::organize::recover_interrupted_batches(&lab.db).unwrap();
    (batch, alpha, beta)
}

#[test]
fn a_title_left_half_renamed_between_two_steps_is_found_at_the_next_start() {
    let lab = Lab::new();
    let (batch, _, _) = killed_inside_the_second_title(&lab);
    let found = RecoveryService::new(Arc::clone(&lab.db)).find().unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].batch_id, batch);
    assert_eq!(found[0].kind, BatchKind::Rename);
    assert_eq!(found[0].items, 1);
    assert_eq!(found[0].titles, ["Beta"]);
}

#[test]
fn resume_finishes_the_half_renamed_title_and_one_undo_takes_the_batch_back() {
    let lab = Lab::new();
    let (batch, alpha, beta) = killed_inside_the_second_title(&lab);
    let service = RecoveryService::new(Arc::clone(&lab.db));

    let outcome = service.resume(&batch, None, &mut no_progress()).unwrap();
    assert_eq!(outcome.completed, 1, "{outcome:?}");
    assert_eq!(outcome.stopped, None, "{outcome:?}");
    let root = Path::new(&lab.movies.path);
    assert_eq!(
        files_in(&root.join("Beta (1995) [x]")),
        ["Beta (1995) [x].mkv", "Beta (1995) [x].nfo", "poster.jpg"]
    );
    assert!(!root.join("Beta (1995)").exists());
    assert!(lab.item(beta).core.path.ends_with("Beta (1995) [x]"));
    assert!(service.find().unwrap().is_empty());

    let undone = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();
    assert_eq!(undone.restored, 2, "{undone:?}");
    for name in ["Alpha (1995)", "Beta (1995)"] {
        assert_eq!(
            files_in(&root.join(name)),
            [
                format!("{name}.mkv"),
                format!("{name}.nfo"),
                "poster.jpg".to_owned()
            ]
        );
    }
    assert!(lab.item(alpha).core.path.ends_with("Alpha (1995)"));
    assert!(lab.item(beta).core.path.ends_with("Beta (1995)"));
}

#[test]
fn roll_back_takes_back_only_the_unfinished_title_like_a_move_does() {
    let lab = Lab::new();
    let (batch, alpha, beta) = killed_inside_the_second_title(&lab);
    let service = RecoveryService::new(Arc::clone(&lab.db));

    let outcome = service.roll_back(&batch).unwrap();
    assert!(outcome.problems.is_empty(), "{outcome:?}");
    let root = Path::new(&lab.movies.path);
    // Beta is whole under its old names again ...
    assert_eq!(
        files_in(&root.join("Beta (1995)")),
        ["Beta (1995).mkv", "Beta (1995).nfo", "poster.jpg"]
    );
    assert!(!root.join("Beta (1995) [x]").exists());
    assert!(lab.item(beta).core.path.ends_with("Beta (1995)"));
    // ... and Alpha, which had finished, keeps its new names.
    assert_eq!(
        files_in(&root.join("Alpha (1995) [x]")),
        ["Alpha (1995) [x].mkv", "Alpha (1995) [x].nfo", "poster.jpg"]
    );
    assert!(lab.item(alpha).core.path.ends_with("Alpha (1995) [x]"));
    assert!(service.find().unwrap().is_empty());
    // History still undoes what is left of the batch.
    let undone = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();
    assert_eq!(undone.restored, 1, "{undone:?}");
    assert!(root.join("Alpha (1995)/Alpha (1995).mkv").exists());
}

/// Safety review finding 5: the batch Resume finished gets its History entry, with both titles,
/// and History takes the whole batch back.
#[test]
fn resume_writes_the_history_entry_and_history_undoes_the_batch() {
    let lab = Lab::new();
    let (batch, alpha, beta) = killed_inside_the_second_title(&lab);
    let history = crate::history::HistoryService::new(Arc::clone(&lab.db));
    assert_eq!(history.entry_for_batch(&batch).unwrap(), None);

    RecoveryService::new(Arc::clone(&lab.db))
        .resume(&batch, None, &mut no_progress())
        .unwrap();

    let id = history.entry_for_batch(&batch).unwrap().expect("written");
    let entry = history.entry(id).unwrap();
    assert_eq!(entry.kind, crate::history::HistoryKind::Rename);
    assert_eq!(entry.summary, "Renamed 2 titles");
    let titles: Vec<(String, String)> = history
        .titles(id)
        .unwrap()
        .into_iter()
        .map(|t| (t.before, t.after))
        .collect();
    assert!(
        titles.contains(&("Beta (1995)".to_owned(), "Beta (1995) [x]".to_owned())),
        "{titles:?}"
    );

    let report = history.undo(id).unwrap();
    assert_eq!(report.undone.len(), 2, "{report:?}");
    assert!(lab.item(alpha).core.path.ends_with("Alpha (1995)"));
    assert!(lab.item(beta).core.path.ends_with("Beta (1995)"));
}

/// A rename that finished but whose entry was never written (the app stopped in between) gets
/// it at the next start; one already recorded is left alone.
#[test]
fn a_finished_rename_without_an_entry_gets_one_at_start_up() {
    let lab = Lab::new();
    let alpha = lab.movie(&lab.movies, "Alpha (1995)");
    let batch = new_batch_id();
    {
        let mut journal = DbJournal::new(Arc::clone(&lab.db));
        let options =
            ExecOptions::new(batch.clone()).with_protected_dirs([PathBuf::from(&lab.movies.path)]);
        Executor::new(&StdFs, &mut journal, options)
            .unwrap()
            .run(&[tagged(&lab, alpha, "Alpha (1995)")])
            .unwrap();
    }
    let recovery = RecoveryService::new(Arc::clone(&lab.db));
    assert_eq!(recovery.record_unrecorded().unwrap(), 1);
    assert_eq!(recovery.record_unrecorded().unwrap(), 0);
    let history = crate::history::HistoryService::new(Arc::clone(&lab.db));
    let id = history.entry_for_batch(&batch).unwrap().expect("written");
    assert_eq!(history.entry(id).unwrap().summary, "Renamed Alpha (1995)");
}

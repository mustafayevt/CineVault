//! The one-time repair of "Date added".
//!
//! A title is dated when its main video file was created ([`file_added_ms`]); builds before that
//! dated every title with the time a scan first saw it. A library added in one scan therefore has
//! many titles sharing one exact value, and sorting by Date added told nothing. [`repair`] finds
//! those titles (at least [`SHARED_MIN_ROWS`] movies and shows with the identical value), dates
//! each from its file (a show from its oldest episode file), and records that it is done under
//! [`REPAIRED_KEY`] so it never runs again. Titles whose dates differ are never touched, and a
//! title whose files cannot be read now (an unplugged drive) keeps its date and is tried at the
//! next scan, because the repair is then not recorded as done.

use std::path::Path;
use std::time::UNIX_EPOCH;

use cv_db::repo::date_added::{SharedDateTitle, apply_date_added, titles_with_shared_date};
use cv_db::{Database, DbError};
use cv_domain::ItemId;
use serde_json::Value;

use crate::app_settings::SettingsService;

/// The setting that records the repair as done.
pub const REPAIRED_KEY: &str = "library.date-added-repaired";

/// How many titles must share one date before it counts as the stamp of one scan.
pub const SHARED_MIN_ROWS: usize = 10;

/// What a repair did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Repaired {
    /// Titles that were dated from their files.
    pub changed: usize,
    /// Titles with a shared date whose files could not be read now.
    pub unreadable: usize,
}

fn millis(time: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    let since = time.ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok().filter(|ms| *ms > 0)
}

/// When a file joined the library: its creation time, else its last change; never later than
/// `now_ms`. `None` when the file cannot be read.
#[must_use]
pub fn file_added_ms(path: &Path, now_ms: i64) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    millis(meta.created())
        .or_else(|| millis(meta.modified()))
        .map(|ms| ms.min(now_ms))
}

fn new_date(title: &SharedDateTitle, now_ms: i64) -> Option<i64> {
    title
        .videos
        .iter()
        .filter_map(|(directory, name)| file_added_ms(&Path::new(directory).join(name), now_ms))
        .min()
}

/// Dates the titles that share one scan stamp from their files, once. Does nothing when the
/// repair is recorded as done or the settings could not be loaded (the record could not be kept).
///
/// # Errors
/// The library cannot be read or written.
pub fn repair(db: &Database, settings: &SettingsService, now_ms: i64) -> Result<Repaired, DbError> {
    if settings.load_failed() || settings.get(REPAIRED_KEY) == Some(Value::Bool(true)) {
        return Ok(Repaired::default());
    }
    let shared = db.read(|conn| titles_with_shared_date(conn, SHARED_MIN_ROWS))?;
    let mut changes: Vec<(ItemId, i64, i64)> = Vec::new();
    let mut unreadable = 0;
    for title in &shared {
        match new_date(title, now_ms) {
            Some(date) => {
                if date != title.date_added_ms {
                    changes.push((title.item, title.date_added_ms, date));
                }
            }
            None => unreadable += 1,
        }
    }
    let changed = if changes.is_empty() {
        0
    } else {
        db.write(move |tx| apply_date_added(tx, &changes))?
    };
    if unreadable == 0
        && let Err(error) = settings.set(REPAIRED_KEY, Value::Bool(true))
    {
        tracing::warn!(%error, "the date added repair could not be recorded");
    }
    if changed > 0 || unreadable > 0 {
        tracing::info!(changed, unreadable, "repaired the date added of titles");
    }
    Ok(Repaired {
        changed,
        unreadable,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use cv_db::repo::{get_item, insert_item};
    use cv_domain::{Item, MediaFile, MediaFileType};

    use super::*;

    const STAMP: i64 = 1_700_000_000_000;
    const LATER: i64 = 4_000_000_000_000;

    fn db(dir: &Path) -> Arc<Database> {
        Arc::new(Database::open(dir.join("library.db")).expect("open"))
    }

    fn settings() -> SettingsService {
        SettingsService::in_memory("test")
    }

    fn video(dir: &Path, name: &str) -> MediaFile {
        let folder = dir.join("lib").join(name);
        fs::create_dir_all(&folder).unwrap();
        let file = format!("{name}.mkv");
        fs::write(folder.join(&file), "x").unwrap();
        MediaFile::new(
            MediaFileType::Video,
            folder.to_string_lossy().into_owned(),
            file,
        )
    }

    fn movie(db: &Database, dir: &Path, name: &str, added: i64) -> (ItemId, std::path::PathBuf) {
        let mut item = Item::movie(name);
        item.core.date_added_ms = added;
        let file = video(dir, name);
        let path = Path::new(&file.directory).join(&file.filename);
        item.core.media_files.push(file);
        let id = db.write(move |tx| insert_item(tx, &item)).unwrap();
        (id, path)
    }

    fn added(db: &Database, id: ItemId) -> i64 {
        db.read(|c| Ok(get_item(c, id)?.unwrap().core.date_added_ms))
            .unwrap()
    }

    #[test]
    fn dates_the_titles_that_share_one_stamp_from_their_files_and_only_those() {
        let dir = tempfile::tempdir().unwrap();
        let db = db(dir.path());
        let shared: Vec<_> = (0..SHARED_MIN_ROWS)
            .map(|n| movie(&db, dir.path(), &format!("Shared {n}"), STAMP))
            .collect();
        // Titles with their own dates are not touched, whatever their files say.
        let distinct: Vec<_> = (0..3)
            .map(|n| movie(&db, dir.path(), &format!("Own {n}"), STAMP + 1 + n))
            .collect();
        let settings = settings();

        let outcome = repair(&db, &settings, LATER).unwrap();
        assert_eq!(outcome.changed, SHARED_MIN_ROWS);
        for (id, path) in &shared {
            assert_eq!(Some(added(&db, *id)), file_added_ms(path, LATER));
            assert_ne!(added(&db, *id), STAMP);
        }
        for (n, (id, _)) in distinct.iter().enumerate() {
            assert_eq!(added(&db, *id), STAMP + 1 + i64::try_from(n).unwrap());
        }
        assert_eq!(settings.get(REPAIRED_KEY), Some(Value::Bool(true)));
    }

    #[test]
    fn runs_once_and_leaves_a_later_shared_value_alone() {
        let dir = tempfile::tempdir().unwrap();
        let db = db(dir.path());
        let settings = settings();
        for n in 0..SHARED_MIN_ROWS {
            movie(&db, dir.path(), &format!("A {n}"), STAMP);
        }
        assert_eq!(
            repair(&db, &settings, LATER).unwrap().changed,
            SHARED_MIN_ROWS
        );
        // Ten more titles with one value, after the repair was recorded: left as they are.
        let later: Vec<_> = (0..SHARED_MIN_ROWS)
            .map(|n| movie(&db, dir.path(), &format!("B {n}"), STAMP + 5))
            .collect();
        assert_eq!(repair(&db, &settings, LATER).unwrap(), Repaired::default());
        assert!(later.iter().all(|(id, _)| added(&db, *id) == STAMP + 5));
    }

    #[test]
    fn a_few_titles_with_the_same_date_are_not_a_scan_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let db = db(dir.path());
        let settings = settings();
        let few: Vec<_> = (0..SHARED_MIN_ROWS - 1)
            .map(|n| movie(&db, dir.path(), &format!("Few {n}"), STAMP))
            .collect();
        assert_eq!(repair(&db, &settings, LATER).unwrap().changed, 0);
        assert!(few.iter().all(|(id, _)| added(&db, *id) == STAMP));
    }

    #[test]
    fn a_title_whose_files_cannot_be_read_keeps_its_date_and_the_repair_is_tried_again() {
        let dir = tempfile::tempdir().unwrap();
        let db = db(dir.path());
        let settings = settings();
        let all: Vec<_> = (0..SHARED_MIN_ROWS)
            .map(|n| movie(&db, dir.path(), &format!("T {n}"), STAMP))
            .collect();
        // One drive is away: its file is not there.
        fs::remove_file(&all[0].1).unwrap();

        let outcome = repair(&db, &settings, LATER).unwrap();
        assert_eq!(outcome.unreadable, 1);
        assert_eq!(outcome.changed, SHARED_MIN_ROWS - 1);
        assert_eq!(added(&db, all[0].0), STAMP);
        assert_eq!(settings.get(REPAIRED_KEY), None, "not recorded as done");
    }

    #[test]
    fn a_show_takes_the_oldest_of_its_episode_files() {
        let dir = tempfile::tempdir().unwrap();
        let db = db(dir.path());
        let settings = settings();
        for n in 0..SHARED_MIN_ROWS - 1 {
            movie(&db, dir.path(), &format!("M {n}"), STAMP);
        }
        let mut show = Item::show("Severance");
        show.core.date_added_ms = STAMP;
        let show_id = db.write(move |tx| insert_item(tx, &show)).unwrap();
        let mut paths = Vec::new();
        for n in 0..2 {
            let mut episode = Item::episode(show_id, format!("Episode {n}"));
            episode.core.date_added_ms = STAMP;
            let file = video(dir.path(), &format!("episode {n}"));
            paths.push(Path::new(&file.directory).join(&file.filename));
            episode.core.media_files.push(file);
            db.write(move |tx| insert_item(tx, &episode)).unwrap();
        }
        repair(&db, &settings, LATER).unwrap();
        let oldest = paths
            .iter()
            .filter_map(|p| file_added_ms(p, LATER))
            .min()
            .unwrap();
        assert_eq!(added(&db, show_id), oldest);
    }

    #[test]
    fn a_date_is_never_in_the_future() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mkv");
        fs::write(&path, "x").unwrap();
        assert_eq!(file_added_ms(&path, 5_000), Some(5_000));
        assert_eq!(file_added_ms(&dir.path().join("nope.mkv"), LATER), None);
    }
}

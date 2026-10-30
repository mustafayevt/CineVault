//! Tests of change data source, remove from library and delete with files over a
//! real library file and real (temporary) folders. The system trash is a folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cv_db::Database;
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::fingerprint::{Fingerprint, count_fingerprints, upsert_fingerprints};
use cv_db::repo::{get_item, insert_item, item_exists};
use cv_domain::{DataSource, Item, ItemId, MediaFile, MediaFileType, SourceKind};

use super::*;
use crate::organize::OrganizeService;

mod edges;
mod faults;
mod guards;
mod moves;
mod named;
mod recovery;
mod rename_stops;
mod safety;

// ---- helpers -------------------------------------------------------------------------------

/// A library with a movie source `movies`, a second movie source `archive` and a TV source `tv`.
struct Lab {
    dir: tempfile::TempDir,
    db: Arc<Database>,
    movies: DataSource,
    archive: DataSource,
    tv: DataSource,
}

/// A data source id no other test of this process uses.
pub(crate) fn next_source_id() -> i64 {
    static NEXT: AtomicI64 = AtomicI64::new(1_000_000);
    NEXT.fetch_add(1, Ordering::SeqCst)
}

impl Lab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
        let make = |name: &str, kind: SourceKind| {
            let path = dir.path().join(name);
            fs::create_dir_all(&path).unwrap();
            let text = path.to_string_lossy().into_owned();
            let mut source = db.write(move |tx| insert_source(tx, kind, &text)).unwrap();
            // The source lock is process-wide by source id (one library per process in the
            // app): ids of their own keep one test's moves from holding up another's.
            let (old, new) = (source.id, next_source_id());
            db.write(move |tx| {
                tx.execute("UPDATE data_source SET id = ?1 WHERE id = ?2", [new, old])?;
                Ok(())
            })
            .unwrap();
            source.id = new;
            source
        };
        let movies = make("movies", SourceKind::Movie);
        let archive = make("archive", SourceKind::Movie);
        let tv = make("tv", SourceKind::Tv);
        Self {
            dir,
            db,
            movies,
            archive,
            tv,
        }
    }

    fn change(&self) -> ChangeSourceService {
        ChangeSourceService::new(Arc::clone(&self.db))
    }

    fn removal(&self, trash: &Arc<FakeTrash>) -> RemovalService {
        RemovalService::new(Arc::clone(&self.db)).with_bin(Arc::clone(trash) as Arc<dyn SystemBin>)
    }

    fn put(root: &Path, rel: &str, content: &str) -> PathBuf {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    /// A movie folder `<source>/<name>` with a video, an NFO and a poster, on disk and in the
    /// library.
    fn movie(&self, source: &DataSource, name: &str) -> ItemId {
        let root = Path::new(&source.path);
        let folder = root.join(name);
        let mut item = Item::movie(name.split(" (").next().unwrap());
        item.core.year = 1995;
        item.core.data_source.clone_from(&source.path);
        item.core.path = folder.to_string_lossy().into_owned();
        for (kind, file) in [
            (MediaFileType::Video, format!("{name}.mkv")),
            (MediaFileType::Nfo, format!("{name}.nfo")),
            (MediaFileType::Poster, "poster.jpg".to_owned()),
        ] {
            Self::put(&folder, &file, &format!("{kind:?}"));
            let mut row = MediaFile::new(kind, folder.to_string_lossy(), file);
            row.size_bytes = 10;
            item.core.media_files.push(row);
        }
        self.insert(item)
    }

    fn insert(&self, item: Item) -> ItemId {
        self.db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    fn item(&self, id: ItemId) -> Item {
        self.db.read(|c| get_item(c, id)).unwrap().unwrap()
    }

    fn exists(&self, id: ItemId) -> bool {
        self.db.read(move |c| item_exists(c, id)).unwrap()
    }

    /// The row is there but hidden by a delete to the bin.
    fn hidden(&self, id: ItemId) -> bool {
        self.exists(id)
            && self
                .db
                .read(move |c| cv_db::repo::library_ops::is_hidden(c, id))
                .unwrap()
    }

    fn history(&self, bin: &Arc<FakeTrash>) -> crate::history::HistoryService {
        crate::history::HistoryService::new(Arc::clone(&self.db))
            .with_bin(Arc::clone(bin) as Arc<dyn SystemBin>)
    }

    /// A show with one season and two episodes, in `<tv>/<name>/Season 1`.
    fn show(&self, name: &str) -> (ItemId, ItemId, Vec<ItemId>) {
        let root = Path::new(&self.tv.path);
        let folder = root.join(name);
        let mut show = Item::show(name);
        show.core.data_source.clone_from(&self.tv.path);
        show.core.path = folder.to_string_lossy().into_owned();
        Self::put(&folder, "tvshow.nfo", "<tvshow/>");
        show.core.media_files.push(MediaFile::new(
            MediaFileType::Nfo,
            folder.to_string_lossy(),
            "tvshow.nfo",
        ));
        let show_id = self.insert(show);
        let season = self.insert(Item::season(show_id, 1));
        let season_folder = folder.join("Season 1");
        let mut episodes = Vec::new();
        for n in 1..=2 {
            let file = format!("{name} S01E0{n}.mkv");
            Self::put(&season_folder, &file, &format!("episode {n}"));
            let mut episode = Item::episode(show_id, format!("Episode {n}"));
            episode.core.data_source.clone_from(&self.tv.path);
            episode.core.path = season_folder.to_string_lossy().into_owned();
            let mut row =
                MediaFile::new(MediaFileType::Video, season_folder.to_string_lossy(), file);
            row.size_bytes = 100;
            episode.core.media_files.push(row);
            episodes.push(self.insert(episode));
        }
        (show_id, season, episodes)
    }

    fn fingerprint(&self, source: &DataSource, path: &Path) {
        let print = Fingerprint {
            path: path.to_string_lossy().into_owned(),
            size_bytes: 1,
            modified_ms: 1,
            file_id: String::new(),
        };
        let root = source.path.clone();
        self.db
            .write(move |tx| upsert_fingerprints(tx, &root, &[print], 1))
            .unwrap();
    }

    fn fingerprints(&self) -> u64 {
        self.db.read(count_fingerprints).unwrap()
    }
}

/// The bin of the tests: a folder (see `trash_bin::fake`).
type FakeTrash = super::trash_bin::fake::FakeBin;

fn no_progress() -> impl FnMut(&MoveProgress) {
    |_| {}
}

// ---- change data source --------------------------------------------------------------------

#[test]
fn the_preview_lists_where_each_folder_would_go() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let plan = lab.change().plan(&[heat], lab.archive.id).unwrap();
    assert_eq!(plan.ready(), 1);
    let row = &plan.rows[0];
    assert_eq!(row.status, MoveStatus::Ready);
    // The files are counted on disk: "Video", "Nfo" and "Poster" are 5, 3 and 6 bytes.
    assert_eq!(row.files, 3);
    assert_eq!(row.bytes, 14);
    assert_eq!(
        PathBuf::from(&row.to),
        Path::new(&lab.archive.path).join("Heat (1995)")
    );
    assert!(!row.across_volumes);
    assert!(Path::new(&row.from).is_dir(), "the preview changes nothing");
}

#[test]
fn a_movie_is_moved_with_its_files_and_the_library_follows() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let old_poster = Path::new(&lab.movies.path).join("Heat (1995)/poster.jpg");
    lab.fingerprint(&lab.movies, &old_poster);
    assert_eq!(lab.fingerprints(), 1);

    let mut seen = Vec::new();
    let applied = lab
        .change()
        .apply(&[heat], lab.archive.id, &mut |p| seen.push(p.clone()))
        .unwrap();

    assert_eq!(applied.moved(), 1, "{applied:?}");
    assert!(applied.batch_id.is_some());
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].done, seen[0].total), (1, 1));
    let new_folder = Path::new(&lab.archive.path).join("Heat (1995)");
    assert!(new_folder.join("Heat (1995).mkv").is_file());
    assert!(new_folder.join("poster.jpg").is_file());
    assert!(!Path::new(&lab.movies.path).join("Heat (1995)").exists());

    let item = lab.item(heat);
    assert_eq!(item.core.data_source, lab.archive.path);
    assert_eq!(PathBuf::from(&item.core.path), new_folder);
    assert!(
        item.core
            .media_files
            .iter()
            .all(|file| Path::new(&file.directory) == new_folder),
        "{:?}",
        item.core.media_files
    );
    assert_eq!(lab.fingerprints(), 0, "the scanner forgets the old files");
}

/// Puts `movie` in a new collection whose poster is copied into the movie's folder, as the Kodi
/// formats do. Returns the collection and the poster path.
fn in_collection(lab: &Lab, movie: ItemId) -> (ItemId, PathBuf) {
    let mut item = lab.item(movie);
    let poster = Lab::put(Path::new(&item.core.path), "movieset-poster.jpg", "set");
    let mut set = Item::set("Heat Collection");
    set.core.media_files.push(MediaFile::new(
        MediaFileType::Poster,
        item.core.path.clone(),
        "movieset-poster.jpg",
    ));
    let set = lab.insert(set);
    item.parent_id = Some(set);
    lab.db
        .write(move |tx| cv_db::repo::update_item(tx, &item))
        .unwrap();
    (set, poster)
}

#[test]
fn a_movie_in_a_collection_moves_with_the_collection_artwork_in_its_folder() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let (set, _) = in_collection(&lab, heat);

    let plan = lab.change().plan(&[heat], lab.archive.id).unwrap();
    assert_eq!(plan.rows[0].status, MoveStatus::Ready, "{plan:?}");

    let applied = lab
        .change()
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 1, "{applied:?}");
    let new_folder = Path::new(&lab.archive.path).join("Heat (1995)");
    assert!(new_folder.join("movieset-poster.jpg").is_file());
    let set = lab.item(set);
    assert_eq!(
        PathBuf::from(&set.core.media_files[0].directory),
        new_folder,
        "the collection's record of its artwork follows the folder"
    );
}

#[test]
fn undo_puts_the_files_and_the_library_back() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let before = lab.item(heat);
    let applied = lab
        .change()
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 1);

    let outcome = OrganizeService::new(Arc::clone(&lab.db))
        .undo(applied.batch_id.as_deref().unwrap())
        .unwrap();
    assert_eq!(outcome.restored, 1, "{outcome:?}");
    assert!(outcome.not_restorable.is_empty() && outcome.failed.is_empty());

    assert!(
        Path::new(&lab.movies.path)
            .join("Heat (1995)/Heat (1995).mkv")
            .is_file()
    );
    assert!(!Path::new(&lab.archive.path).join("Heat (1995)").exists());
    let after = lab.item(heat);
    assert_eq!(after.core.path, before.core.path);
    assert_eq!(after.core.data_source, before.core.data_source);
    assert_eq!(after.core.media_files.len(), before.core.media_files.len());
    assert!(
        after
            .core
            .media_files
            .iter()
            .all(|file| file.directory == before.core.path)
    );
}

#[test]
fn a_show_moves_with_its_seasons_and_episodes() {
    let lab = Lab::new();
    let other_tv = {
        let path = lab.dir.path().join("tv2");
        fs::create_dir_all(&path).unwrap();
        let text = path.to_string_lossy().into_owned();
        lab.db
            .write(move |tx| insert_source(tx, SourceKind::Tv, &text))
            .unwrap()
    };
    let (show, season, episodes) = lab.show("Dark");

    // An episode stands for its show; the selection is moved once.
    let plan = lab
        .change()
        .plan(&[episodes[0], season, show], other_tv.id)
        .unwrap();
    assert_eq!(plan.rows.len(), 1, "{plan:?}");
    assert_eq!(plan.rows[0].kind, TitleKind::Show);
    assert_eq!(plan.rows[0].files, 3);

    let applied = lab
        .change()
        .apply(&[show], other_tv.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 1, "{applied:?}");
    let moved = Path::new(&other_tv.path).join("Dark");
    assert!(moved.join("Season 1/Dark S01E02.mkv").is_file());
    for id in [show, season, episodes[0], episodes[1]] {
        let item = lab.item(id);
        if !item.core.path.is_empty() {
            assert!(
                Path::new(&item.core.path).starts_with(&moved),
                "{:?}",
                item.core.path
            );
            assert_eq!(item.core.data_source, other_tv.path);
        }
    }
    let episode = lab.item(episodes[1]);
    assert_eq!(
        PathBuf::from(&episode.core.media_files[0].directory),
        moved.join("Season 1")
    );
}

#[test]
fn titles_that_cannot_be_moved_say_why_and_stay_put() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let ronin = lab.movie(&lab.movies, "Ronin (1998)");
    let archived = lab.movie(&lab.archive, "Alien (1979)");
    let (show, ..) = lab.show("Dark");
    let collection = lab.insert(Item::set("Heat Collection"));

    // Another title keeps a file in Ronin's folder.
    let mut squatter = Item::movie("Squatter");
    squatter.core.data_source.clone_from(&lab.movies.path);
    squatter.core.path = Path::new(&lab.movies.path)
        .join("Ronin (1998)")
        .to_string_lossy()
        .into_owned();
    squatter.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        squatter.core.path.clone(),
        "squatter.mkv",
    ));
    lab.insert(squatter);

    // A destination that holds files already.
    let blocker = Path::new(&lab.archive.path).join("Heat (1995)");
    Lab::put(&blocker, "other.mkv", "x");

    let plan = lab
        .change()
        .plan(&[heat, ronin, archived, show, collection], lab.archive.id)
        .unwrap();
    let status = |id: ItemId| {
        plan.rows
            .iter()
            .find(|r| r.id == id)
            .unwrap()
            .status
            .clone()
    };
    assert_eq!(status(heat), MoveStatus::DestinationTaken);
    assert_eq!(status(ronin), MoveStatus::SharedFolder);
    assert_eq!(status(archived), MoveStatus::SameSource);
    assert_eq!(status(show), MoveStatus::WrongKind);
    assert!(matches!(status(collection), MoveStatus::NotMovable(_)));
    assert_eq!(plan.ready(), 0);

    let applied = lab
        .change()
        .apply(
            &[heat, ronin, archived, show, collection],
            lab.archive.id,
            &mut no_progress(),
        )
        .unwrap();
    assert_eq!(applied.moved(), 0);
    assert!(applied.batch_id.is_none(), "nothing was journaled");
    assert!(Path::new(&lab.movies.path).join("Heat (1995)").is_dir());
    assert!(blocker.join("other.mkv").is_file(), "nothing was replaced");
}

/// Puts the extras, subtitles, a hidden file and a theme next to a movie's own files.
fn fill_movie_folder(lab: &Lab, folder: &str) {
    let dir = Path::new(&lab.movies.path).join(folder);
    for (rel, content) in [
        ("Heat (1995).en.srt", "subtitle"),
        ("Heat (1995).fr.forced.srt", "sub"),
        ("Heat (1995)-trailer.mkv", "trailer"),
        ("theme.mp3", "theme"),
        (".hidden", "h"),
        ("extras/making-of.mkv", "making of"),
        ("extras/deleted/scene.mkv", "scene"),
    ] {
        Lab::put(&dir, rel, content);
    }
    fs::create_dir_all(dir.join("empty-folder")).unwrap();
}

#[test]
fn a_movie_folder_with_extras_subtitles_and_a_theme_lists_every_file() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    fill_movie_folder(&lab, "Heat (1995)");

    let plan = lab.change().plan(&[heat], lab.archive.id).unwrap();

    let row = &plan.rows[0];
    assert_eq!(row.status, MoveStatus::Ready);
    let sep = std::path::MAIN_SEPARATOR;
    let mut listed: Vec<String> = row
        .entries
        .iter()
        .map(|e| format!("{}{}", e.path, if e.is_file() { "" } else { "/" }))
        .collect();
    listed.sort();
    let mut expected = vec![
        ".hidden".to_owned(),
        "Heat (1995)-trailer.mkv".to_owned(),
        "Heat (1995).en.srt".to_owned(),
        "Heat (1995).fr.forced.srt".to_owned(),
        "Heat (1995).mkv".to_owned(),
        "Heat (1995).nfo".to_owned(),
        "empty-folder/".to_owned(),
        "extras/".to_owned(),
        format!("extras{sep}deleted/"),
        format!("extras{sep}deleted{sep}scene.mkv"),
        format!("extras{sep}making-of.mkv"),
        "poster.jpg".to_owned(),
        "theme.mp3".to_owned(),
    ];
    expected.sort();
    assert_eq!(listed, expected);
    assert_eq!(row.files, 10, "ten files; the three folders are listed too");
    assert_eq!(
        row.bytes,
        row.entries.iter().map(|e| e.size).sum::<u64>(),
        "the size is the sum of the entries"
    );
    assert!(
        Path::new(&row.from).join(".hidden").is_file(),
        "nothing moved"
    );
}

#[test]
fn a_show_lists_every_season_folder() {
    let lab = Lab::new();
    let (show, ..) = lab.show("Dark");
    let season_two = Path::new(&lab.tv.path).join("Dark/Season 2");
    Lab::put(&season_two, "Dark S02E01.mkv", "episode");
    Lab::put(&season_two, "Dark S02E01.en.srt", "sub");
    let second_tv = {
        let path = lab.dir.path().join("tv2");
        fs::create_dir_all(&path).unwrap();
        let text = path.to_string_lossy().into_owned();
        lab.db
            .write(move |tx| insert_source(tx, SourceKind::Tv, &text))
            .unwrap()
    };

    let plan = lab.change().plan(&[show], second_tv.id).unwrap();

    let row = &plan.rows[0];
    assert_eq!(row.status, MoveStatus::Ready, "{row:?}");
    let sep = std::path::MAIN_SEPARATOR;
    let paths: Vec<&str> = row.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.contains(&"tvshow.nfo"));
    assert!(paths.contains(&"Season 1"));
    assert!(paths.contains(&"Season 2"));
    assert!(paths.contains(&format!("Season 1{sep}Dark S01E01.mkv").as_str()));
    assert!(paths.contains(&format!("Season 2{sep}Dark S02E01.en.srt").as_str()));
    assert_eq!(row.files, 5);
}

fn margin_for_tests(total: u64) -> u64 {
    (total / 100).max(SPACE_MARGIN_FLOOR)
}

#[test]
fn a_copy_that_needs_more_room_than_the_destination_has_is_blocked() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let ronin = lab.movie(&lab.movies, "Ronin (1998)");
    // Pretend the archive is another volume with 100 GB free.
    let change = lab
        .change()
        .with_volume_probe(|_, _| false)
        .with_free_space_probe(|_| Some(100 * 1024 * 1024 * 1024));
    let plan = change.plan(&[heat, ronin], lab.archive.id).unwrap();
    for row in &plan.rows {
        assert!(row.across_volumes);
        assert_eq!(row.status, MoveStatus::Ready, "small folders fit");
        assert_eq!(row.free_bytes, Some(100 * 1024 * 1024 * 1024));
        assert_eq!(row.needed_bytes, row.bytes + margin_for_tests(row.bytes));
    }

    // A video of 300 MB with 100 MB free. Kept small: NTFS has no sparse `set_len`, so the
    // file really takes its size on disk (a 62 GB version filled the test machine).
    let big_bytes = 300 * 1024 * 1024_u64;
    let free_bytes = 100 * 1024 * 1024_u64;
    let big = Path::new(&lab.movies.path).join("Heat (1995)/Heat (1995).mkv");
    let file = fs::File::options().write(true).open(&big).unwrap();
    if file.set_len(big_bytes).is_ok() {
        let change = lab
            .change()
            .with_volume_probe(|_, _| false)
            .with_free_space_probe(move |_| Some(free_bytes));
        let plan = change.plan(&[heat], lab.archive.id).unwrap();
        let row = &plan.rows[0];
        let MoveStatus::InsufficientSpace { needed, free } = row.status else {
            panic!("not blocked: {:?}", row.status);
        };
        assert!(needed > big_bytes);
        assert_eq!(free, free_bytes);
        assert_eq!(plan.ready(), 0);
        let applied = change
            .apply(&[heat], lab.archive.id, &mut no_progress())
            .unwrap();
        assert_eq!(applied.moved(), 0, "apply is blocked too");
        assert!(big.is_file());
    }
}

#[test]
fn two_titles_that_fit_alone_but_not_together_share_the_free_space() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let ronin = lab.movie(&lab.movies, "Ronin (1998)");
    // Each folder is 14 bytes and the margin is 100 MB: room for one title with its margin only.
    let room = SPACE_MARGIN_FLOOR + 20;
    let change = lab
        .change()
        .with_volume_probe(|_, _| false)
        .with_free_space_probe(move |_| Some(room));
    let plan = change.plan(&[heat, ronin], lab.archive.id).unwrap();
    assert_eq!(plan.rows[0].status, MoveStatus::Ready);
    assert!(
        matches!(plan.rows[1].status, MoveStatus::InsufficientSpace { .. }),
        "{:?}",
        plan.rows[1].status
    );
}

#[test]
fn an_existing_destination_is_a_conflict_that_lists_the_taken_paths() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let archived = Path::new(&lab.archive.path).join("Heat (1995)");
    // Even an empty folder of that name is in the way: nothing is merged.
    fs::create_dir_all(&archived).unwrap();

    let plan = lab.change().plan(&[heat], lab.archive.id).unwrap();
    let row = &plan.rows[0];
    assert_eq!(row.status, MoveStatus::DestinationTaken);
    assert_eq!(row.conflicts, [archived.to_string_lossy().into_owned()]);
    assert_eq!(plan.ready(), 0);

    Lab::put(&archived, "poster.jpg", "someone else's poster");
    let plan = lab.change().plan(&[heat], lab.archive.id).unwrap();
    let conflicts = &plan.rows[0].conflicts;
    assert_eq!(conflicts.len(), 2, "{conflicts:?}");
    assert!(conflicts[1].ends_with("poster.jpg"));
}

#[test]
fn a_shared_folder_and_a_source_root_are_refused_with_no_entries_read() {
    let lab = Lab::new();
    let ronin = lab.movie(&lab.movies, "Ronin (1998)");
    let mut squatter = Item::movie("Squatter");
    squatter.core.data_source.clone_from(&lab.movies.path);
    squatter.core.path = Path::new(&lab.movies.path)
        .join("Ronin (1998)")
        .to_string_lossy()
        .into_owned();
    squatter.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        squatter.core.path.clone(),
        "squatter.mkv",
    ));
    lab.insert(squatter);
    let mut loose = Item::movie("Loose");
    loose.core.data_source.clone_from(&lab.movies.path);
    loose.core.path.clone_from(&lab.movies.path);
    let loose = lab.insert(loose);

    let plan = lab.change().plan(&[ronin, loose], lab.archive.id).unwrap();
    assert_eq!(plan.rows[0].status, MoveStatus::SharedFolder);
    assert_eq!(plan.rows[1].status, MoveStatus::InSourceRoot);
    assert!(plan.rows.iter().all(|row| row.entries.is_empty()));
}

#[cfg(unix)]
#[test]
fn a_link_in_the_folder_is_listed_and_blocks_a_copy_to_another_volume() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let outside = lab.dir.path().join("elsewhere");
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(
        &outside,
        Path::new(&lab.movies.path).join("Heat (1995)/link"),
    )
    .unwrap();

    let same = lab.change().plan(&[heat], lab.archive.id).unwrap();
    assert_eq!(
        same.rows[0].status,
        MoveStatus::Ready,
        "a rename carries it"
    );
    assert!(
        same.rows[0]
            .entries
            .iter()
            .any(|e| matches!(e.kind, EntryKind::Link { .. }))
    );

    let across = lab
        .change()
        .with_volume_probe(|_, _| false)
        .plan(&[heat], lab.archive.id)
        .unwrap();
    assert!(matches!(across.rows[0].status, MoveStatus::NotMovable(_)));
}

#[test]
fn a_movie_in_the_root_of_its_source_and_an_unreachable_folder_are_not_moved() {
    let lab = Lab::new();
    let mut loose = Item::movie("Loose");
    loose.core.data_source.clone_from(&lab.movies.path);
    loose.core.path.clone_from(&lab.movies.path);
    let loose = lab.insert(loose);
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    fs::remove_dir_all(Path::new(&lab.movies.path).join("Heat (1995)")).unwrap();

    let plan = lab.change().plan(&[loose, heat], lab.archive.id).unwrap();
    assert_eq!(plan.rows[0].status, MoveStatus::InSourceRoot);
    assert!(matches!(plan.rows[1].status, MoveStatus::Unavailable(_)));
}

#[test]
fn an_unknown_data_source_is_refused() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let error = lab.change().plan(&[heat], 9999).unwrap_err();
    assert_eq!(error.code(), "unknown_source");
}

// ---- remove from library -------------------------------------------------------------------

#[test]
fn removing_a_title_keeps_its_files_and_forgets_the_scan() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let ronin = lab.movie(&lab.movies, "Ronin (1998)");
    lab.fingerprint(
        &lab.movies,
        &Path::new(&lab.movies.path).join("Heat (1995)/poster.jpg"),
    );
    lab.fingerprint(
        &lab.movies,
        &Path::new(&lab.movies.path).join("Ronin (1998)/poster.jpg"),
    );

    let report = lab
        .removal(&trash)
        .remove_from_library(&[heat], false)
        .unwrap();
    assert_eq!(report.removed.len(), 1);
    assert_eq!(report.removed[0].ignore, IgnoreOutcome::NotRequested);
    assert!(!lab.exists(heat));
    assert!(lab.exists(ronin));
    let folder = Path::new(&lab.movies.path).join("Heat (1995)");
    assert!(folder.join("Heat (1995).mkv").is_file(), "the files stay");
    assert!(!folder.join(MARKER_IGNORE).exists());
    assert_eq!(
        lab.fingerprints(),
        1,
        "only the removed title's files are forgotten"
    );
    assert_eq!(trash.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn removing_with_ignore_writes_the_marker_into_the_folder() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let report = lab
        .removal(&trash)
        .remove_from_library(&[heat], true)
        .unwrap();
    assert_eq!(report.removed[0].ignore, IgnoreOutcome::Written);
    let marker = Path::new(&lab.movies.path)
        .join("Heat (1995)")
        .join(MARKER_IGNORE);
    assert!(marker.is_file());
    // The scanner's own rule sees the marker.
    let rules = cv_scan::skip::SkipRules::with_defaults(cv_scan::skip::LibraryKind::Movies);
    assert!(rules.dir_has_marker(marker.parent().unwrap()));
}

#[test]
fn the_ignore_marker_is_only_written_for_a_folder_the_title_owns() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let first = lab.movie(&lab.movies, "Mixed");
    let mut second = lab.item(first);
    second.id = None;
    second.core.title = "Second".to_owned();
    second.core.media_files = vec![MediaFile::new(
        MediaFileType::Video,
        second.core.path.clone(),
        "second.mkv",
    )];
    let second = lab.insert(second);
    let mut loose = Item::movie("Loose");
    loose.core.data_source.clone_from(&lab.movies.path);
    loose.core.path.clone_from(&lab.movies.path);
    let loose = lab.insert(loose);

    let report = lab
        .removal(&trash)
        .remove_from_library(&[first, second, loose], true)
        .unwrap();
    assert_eq!(report.removed.len(), 3, "{report:?}");
    for removed in &report.removed {
        assert!(
            matches!(removed.ignore, IgnoreOutcome::Skipped(_)),
            "{removed:?}"
        );
    }
    assert!(
        !Path::new(&lab.movies.path)
            .join("Mixed")
            .join(MARKER_IGNORE)
            .exists()
    );
    assert!(!Path::new(&lab.movies.path).join(MARKER_IGNORE).exists());
}

#[test]
fn removing_a_show_removes_its_seasons_and_episodes_once() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let (show, season, episodes) = lab.show("Dark");
    let plan = lab
        .removal(&trash)
        .plan(&[episodes[0], show, season])
        .unwrap();
    assert_eq!(plan.rows.len(), 1);
    assert_eq!(plan.rows[0].children, 3);
    assert_eq!(plan.rows[0].files, 3);

    lab.removal(&trash)
        .remove_from_library(&[episodes[0], show, season], false)
        .unwrap();
    for id in [show, season, episodes[0], episodes[1]] {
        assert!(!lab.exists(id));
    }
    assert!(
        Path::new(&lab.tv.path)
            .join("Dark/Season 1/Dark S01E01.mkv")
            .is_file()
    );
}

#[test]
fn a_single_episode_can_be_removed_without_its_show() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let (show, _, episodes) = lab.show("Dark");
    let report = lab
        .removal(&trash)
        .remove_from_library(&[episodes[0]], true)
        .unwrap();
    assert_eq!(report.removed.len(), 1);
    assert!(matches!(
        report.removed[0].ignore,
        IgnoreOutcome::Skipped(_)
    ));
    assert!(lab.exists(show) && lab.exists(episodes[1]));
    assert!(!lab.exists(episodes[0]));
}

#[test]
fn a_collection_is_not_removed_here() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let set = lab.insert(Item::set("Heat Collection"));
    let report = lab
        .removal(&trash)
        .remove_from_library(&[set], false)
        .unwrap();
    assert!(report.removed.is_empty());
    assert_eq!(report.problems.len(), 1);
    assert!(lab.exists(set));
}

// ---- delete with files ---------------------------------------------------------------------

#[test]
fn deleting_sends_the_folder_to_the_system_trash_and_hides_the_title() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let ronin = lab.movie(&lab.movies, "Ronin (1998)");

    let report = lab.removal(&trash).delete_with_files(&[heat]).unwrap();
    assert_eq!(report.deleted.len(), 1, "{report:?}");
    assert_eq!(report.deleted[0].method, DeleteMethod::SystemTrash);
    assert_eq!(report.deleted[0].paths, 1, "the folder goes as one");
    assert!(report.problems.is_empty() && report.batch_id.is_none());
    assert!(report.history_id.is_some());
    assert_eq!(trash.names(), ["Heat (1995)"]);
    assert!(!Path::new(&lab.movies.path).join("Heat (1995)").exists());
    assert!(
        lab.hidden(heat),
        "the row stays, hidden, so Undo brings it back"
    );
    assert!(lab.exists(ronin) && !lab.hidden(ronin));
    assert!(
        Path::new(&lab.movies.path)
            .join("Ronin (1998)/Ronin (1998).mkv")
            .is_file()
    );
}

#[test]
fn deleting_a_movie_in_a_collection_trashes_its_whole_folder() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let (set, poster) = in_collection(&lab, heat);

    let report = lab.removal(&trash).delete_with_files(&[heat]).unwrap();
    assert_eq!(report.deleted.len(), 1, "{report:?}");
    assert_eq!(report.deleted[0].paths, 1, "the folder goes as one");
    assert!(!poster.exists());
    assert!(lab.exists(set) && !lab.hidden(set), "the collection stays");
    assert!(
        !lab.item(set).core.media_files.is_empty(),
        "the collection keeps its rows; Undo brings the copy back with the folder"
    );
}

#[test]
fn a_shared_folder_loses_only_the_files_of_the_deleted_title() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let folder = Path::new(&lab.movies.path).join("Mixed");
    let make = |title: &str, file: &str| {
        Lab::put(&folder, file, title);
        let mut item = Item::movie(title);
        item.core.data_source.clone_from(&lab.movies.path);
        item.core.path = folder.to_string_lossy().into_owned();
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            folder.to_string_lossy(),
            file,
        ));
        lab.insert(item)
    };
    let alien = make("Alien", "alien.mkv");
    let heat = make("Heat", "heat.mkv");

    let report = lab.removal(&trash).delete_with_files(&[alien]).unwrap();
    assert_eq!(report.deleted[0].paths, 1);
    assert!(!folder.join("alien.mkv").exists());
    assert!(
        folder.join("heat.mkv").is_file(),
        "the other title keeps its file"
    );
    assert!(lab.hidden(alien) && !lab.hidden(heat));
}

#[test]
fn deleting_an_episode_trashes_its_file_only() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let (show, _, episodes) = lab.show("Dark");
    lab.removal(&trash)
        .delete_with_files(&[episodes[0]])
        .unwrap();
    let season = Path::new(&lab.tv.path).join("Dark/Season 1");
    assert!(!season.join("Dark S01E01.mkv").exists());
    assert!(season.join("Dark S01E02.mkv").is_file());
    assert!(!lab.hidden(show) && lab.hidden(episodes[0]));
}

#[test]
fn deleting_a_show_trashes_its_folder_and_every_row_below() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let (show, season, episodes) = lab.show("Dark");
    let report = lab.removal(&trash).delete_with_files(&[show]).unwrap();
    assert_eq!(report.deleted.len(), 1);
    assert_eq!(trash.names(), ["Dark"]);
    for id in [show, season, episodes[0], episodes[1]] {
        assert!(lab.hidden(id));
    }
}

#[test]
fn when_the_system_trash_refuses_the_files_go_to_the_data_source_trash_and_can_be_undone() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    trash.fail.store(true, Ordering::SeqCst);
    let heat = lab.movie(&lab.movies, "Heat (1995)");

    let report = lab.removal(&trash).delete_with_files(&[heat]).unwrap();
    assert_eq!(
        report.deleted[0].method,
        DeleteMethod::DataSourceTrash,
        "{report:?}"
    );
    assert!(report.deleted[0].notes[0].contains("bin is full"));
    let trash_dir = Path::new(&lab.movies.path).join(".deletedByCineVault");
    let batch = fs::read_dir(&trash_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let in_trash = batch.join("Heat (1995)/Heat (1995).mkv");
    assert!(in_trash.is_file(), "{in_trash:?}");
    assert!(lab.hidden(heat), "the title left the library");

    let outcome = lab
        .history(&trash)
        .undo(report.history_id.unwrap())
        .unwrap();
    assert_eq!(outcome.undone, [heat], "{outcome:?}");
    assert!(!lab.hidden(heat));
    assert!(
        Path::new(&lab.movies.path)
            .join("Heat (1995)/Heat (1995).mkv")
            .is_file(),
        "undo brings the files back"
    );
}

#[test]
fn a_location_the_system_trash_cannot_take_uses_the_data_source_trash_without_asking() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    *trash.refuse.lock().unwrap() = Some(PathBuf::from(&lab.movies.path));
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let report = lab.removal(&trash).delete_with_files(&[heat]).unwrap();
    assert_eq!(report.deleted[0].method, DeleteMethod::DataSourceTrash);
    assert_eq!(
        trash.calls.load(Ordering::SeqCst),
        0,
        "the system trash was not asked"
    );
    assert!(trash.names().is_empty());
    assert!(
        Path::new(&lab.movies.path)
            .join(".deletedByCineVault")
            .is_dir()
    );
}

#[test]
fn a_title_on_an_unreachable_data_source_is_kept() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let mut gone = lab.item(heat);
    gone.core.data_source = lab
        .dir
        .path()
        .join("unplugged")
        .to_string_lossy()
        .into_owned();
    lab.db
        .write(move |tx| cv_db::repo::update_item(tx, &gone))
        .unwrap();

    let report = lab.removal(&trash).delete_with_files(&[heat]).unwrap();
    assert!(report.deleted.is_empty());
    assert_eq!(report.problems.len(), 1);
    assert!(report.problems[0].reason.contains("offline"));
    assert!(!lab.hidden(heat), "the title stays");
    assert!(report.history_id.is_none(), "nothing to record");
    assert!(Path::new(&lab.movies.path).join("Heat (1995)").is_dir());
}

#[test]
fn a_file_outside_the_data_source_is_never_trashed() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let outside = Lab::put(lab.dir.path(), "elsewhere/precious.mkv", "keep");
    let mut item = Item::movie("Stray");
    item.core.data_source.clone_from(&lab.movies.path);
    item.core.path = outside.parent().unwrap().to_string_lossy().into_owned();
    item.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        item.core.path.clone(),
        "precious.mkv",
    ));
    let stray = lab.insert(item);

    let report = lab.removal(&trash).delete_with_files(&[stray]).unwrap();
    assert!(report.deleted.is_empty(), "{report:?}");
    assert!(outside.is_file());
    assert!(lab.exists(stray) && !lab.hidden(stray));
}

#[test]
fn a_title_whose_files_are_gone_is_removed_without_touching_the_trash() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    fs::remove_dir_all(Path::new(&lab.movies.path).join("Heat (1995)")).unwrap();
    let report = lab.removal(&trash).delete_with_files(&[heat]).unwrap();
    assert_eq!(report.deleted[0].method, DeleteMethod::NothingOnDisk);
    assert_eq!(trash.calls.load(Ordering::SeqCst), 0);
    assert!(lab.hidden(heat));
}

// ---- the missing/duplicate review -----------------------------------------------

use cv_db::repo::{StdFileProbe, set_availability};
use cv_domain::Availability;

impl Lab {
    fn review(&self) -> ReviewService {
        ReviewService::new(Arc::clone(&self.db))
    }

    /// Re-checks every title against the disk.
    fn refresh(&self) {
        self.db
            .refresh_availability(&StdFileProbe, cv_db::repo::PathStyle::current())
            .unwrap();
    }

    fn availability(&self, id: ItemId) -> Availability {
        self.db
            .read(|c| cv_db::repo::get_availability(c, id))
            .unwrap()
            .0
    }

    /// A second show row on the folder of the show `of`, with the same episodes.
    fn duplicate_show(&self, of: ItemId) -> ItemId {
        let mut copy = self.item(of);
        copy.id = None;
        let copy_id = self.insert(copy);
        for kid in self.db.read(|c| cv_db::repo::children_of(c, of)).unwrap() {
            let mut kid = self.item(kid);
            kid.id = None;
            kid.parent_id = Some(copy_id);
            self.insert(kid);
        }
        copy_id
    }
}

fn reasons_of(list: &ReviewList) -> Vec<(ItemId, ReviewReason)> {
    list.rows.iter().map(|row| (row.id, row.reason)).collect()
}

#[test]
fn the_review_lists_missing_offline_and_duplicate_rows_with_reason_size_and_last_seen() {
    let lab = Lab::new();
    let fine = lab.movie(&lab.movies, "Fine (1995)");
    let gone = lab.movie(&lab.movies, "Gone (1995)");
    let away = lab.movie(&lab.archive, "Away (1995)");
    fs::remove_dir_all(Path::new(&lab.movies.path).join("Gone (1995)")).unwrap();
    lab.refresh();
    lab.db
        .write(move |tx| set_availability(tx, away, Availability::Offline, "drive unplugged"))
        .unwrap();
    // A second row on the same video file as `fine`.
    let mut twin = lab.item(fine);
    twin.id = None;
    twin.core.title = "Fine Again".into();
    let twin = lab.insert(twin);

    let list = lab.review().list().unwrap();

    assert_eq!(
        reasons_of(&list),
        vec![
            (gone, ReviewReason::Missing),
            (away, ReviewReason::Offline),
            (twin, ReviewReason::Duplicate),
        ]
    );
    let missing = &list.rows[0];
    assert_eq!((missing.files, missing.bytes), (3, 30));
    assert!(missing.last_seen_ms > 0 && missing.can_relocate);
    assert!(list.rows[1].detail.contains("Drive unplugged"));
    let duplicate = &list.rows[2];
    assert_eq!(duplicate.duplicate_of.as_ref().map(|o| o.id), Some(fine));
    assert_eq!(duplicate.detail, "Duplicate of Fine (1995)");
    assert!(!duplicate.can_relocate);
    // Listing is a read: every row is still there.
    assert!(lab.exists(fine) && lab.exists(gone) && lab.exists(away) && lab.exists(twin));
}

#[test]
fn a_new_row_for_the_same_title_lists_the_missing_one_as_a_possible_duplicate() {
    let lab = Lab::new();
    let fine = lab.movie(&lab.movies, "Fine (1995)");
    let gone = lab.movie(&lab.movies, "Gone (1995)");
    fs::remove_dir_all(Path::new(&lab.movies.path).join("Gone (1995)")).unwrap();
    lab.refresh();
    // The same film found again under another folder: a second row with the same title and year.
    let mut again = lab.item(fine);
    again.id = None;
    again.core.title = "Gone".into();
    let again = lab.insert(again);

    let list = lab.review().list().unwrap();

    let row = list.rows.iter().find(|r| r.id == gone).expect("listed");
    assert_eq!(row.reason, ReviewReason::Duplicate);
    assert_eq!(row.duplicate_of.as_ref().map(|o| o.id), Some(again));
    assert!(row.detail.starts_with("Possible duplicate of Gone (1995)"));
    assert!(
        row.can_relocate,
        "the missing row can still be pointed at its files"
    );
    assert!(lab.exists(gone) && lab.exists(again));
}

#[test]
fn a_duplicate_show_row_is_listed_with_the_show_it_duplicates() {
    let lab = Lab::new();
    let (show, _, _) = lab.show("Breaking Bad");
    lab.db
        .write(move |tx| {
            let mut item = cv_db::repo::get_item(tx, show)?.unwrap();
            item.core.year = 2008;
            cv_db::repo::update_item(tx, &item)
        })
        .unwrap();
    let copy = lab.duplicate_show(show);

    let list = lab.review().list().unwrap();

    assert_eq!(reasons_of(&list), vec![(copy, ReviewReason::Duplicate)]);
    assert_eq!(list.rows[0].detail, "Duplicate of Breaking Bad (2008)");
}

#[test]
fn a_new_row_for_the_same_show_lists_the_missing_one_as_a_possible_duplicate() {
    let lab = Lab::new();
    let (gone, _, _) = lab.show("Dark Old");
    let (again, _, _) = lab.show("Dark New");
    for (id, title) in [(gone, "Dark"), (again, "Dark")] {
        lab.db
            .write(move |tx| {
                let mut item = cv_db::repo::get_item(tx, id)?.unwrap();
                item.core.title = title.into();
                item.core.year = 2017;
                cv_db::repo::update_item(tx, &item)
            })
            .unwrap();
    }
    fs::remove_dir_all(Path::new(&lab.tv.path).join("Dark Old")).unwrap();
    lab.refresh();

    let list = lab.review().list().unwrap();

    let row = list.rows.iter().find(|r| r.id == gone).expect("listed");
    assert_eq!(row.reason, ReviewReason::Duplicate);
    assert_eq!(row.duplicate_of.as_ref().map(|o| o.id), Some(again));
    assert!(row.detail.starts_with("Possible duplicate of Dark (2017)"));
    assert!(lab.exists(gone) && lab.exists(again), "nothing is deleted");
}

#[test]
fn a_restart_and_a_rescan_keep_every_row_of_the_review() {
    let lab = Lab::new();
    let ids: Vec<ItemId> = (0..12)
        .map(|n| lab.movie(&lab.movies, &format!("Lost {n:02} (1995)")))
        .collect();
    for n in 0..12 {
        fs::remove_dir_all(Path::new(&lab.movies.path).join(format!("Lost {n:02} (1995)")))
            .unwrap();
    }
    // A source folder with nothing in it is not trusted to mean "everything is gone".
    Lab::put(
        Path::new(&lab.movies.path),
        "Other (2001)/Other (2001).mkv",
        "x",
    );
    lab.refresh();
    assert_eq!(lab.review().list().unwrap().rows.len(), 12);

    // Restart: a new connection to the same file, then a rescan of the source.
    let path = lab.dir.path().join("library.db");
    let reopened = Arc::new(Database::open(&path).unwrap());
    cv_scan::rescan::rescan_source(
        &reopened,
        &lab.movies,
        &cv_scan::rescan::RescanOptions::default(),
    )
    .unwrap();

    let list = ReviewService::new(Arc::clone(&reopened)).list().unwrap();
    let listed: Vec<ItemId> = list.rows.iter().map(|r| r.id).collect();
    for id in &ids {
        assert!(listed.contains(id), "{id:?} is still in the review");
        assert!(reopened.read(|c| item_exists(c, *id)).unwrap());
    }
}

#[test]
fn a_show_a_rescan_set_aside_is_listed_as_superseded() {
    let lab = Lab::new();
    let root = Path::new(&lab.tv.path);
    Lab::put(root, "Chernobyl/Season 1/Chernobyl S01E01.mkv", "e1");
    let options = cv_scan::rescan::RescanOptions::default();
    cv_scan::rescan::rescan_source(&lab.db, &lab.tv, &options).unwrap();
    // An older scan read the season folder as a show of its own.
    let folder = root.join("Chernobyl/Season 1");
    let mut stray = Item::show("Season 1");
    stray.core.data_source.clone_from(&lab.tv.path);
    stray.core.path = folder.to_string_lossy().into_owned();
    let stray = lab.insert(stray);

    cv_scan::rescan::rescan_source(&lab.db, &lab.tv, &options).unwrap();

    let list = lab.review().list().unwrap();
    assert_eq!(reasons_of(&list), vec![(stray, ReviewReason::Superseded)]);
    assert!(list.rows[0].detail.contains("part of the show"));
    assert!(
        lab.exists(stray),
        "the rescan sets it aside, it does not delete it"
    );
}

#[test]
fn keep_hides_a_row_until_its_problem_changes() {
    let lab = Lab::new();
    let gone = lab.movie(&lab.movies, "Gone (1995)");
    fs::remove_dir_all(Path::new(&lab.movies.path).join("Gone (1995)")).unwrap();
    lab.refresh();
    let review = lab.review();

    assert_eq!(review.keep(&[gone]).unwrap(), 1);
    let list = review.list().unwrap();
    assert_eq!((list.rows.len(), list.kept), (0, 1));

    // Another problem lists the row again; taking the keep back does too.
    lab.db
        .write(move |tx| set_availability(tx, gone, Availability::Offline, "drive unplugged"))
        .unwrap();
    assert_eq!(
        reasons_of(&review.list().unwrap()),
        vec![(gone, ReviewReason::Offline)]
    );
    lab.db
        .write(move |tx| set_availability(tx, gone, Availability::Missing, "folder gone"))
        .unwrap();
    assert!(review.list().unwrap().rows.is_empty());
    review.unkeep(&[gone]).unwrap();
    assert_eq!(review.list().unwrap().rows.len(), 1);
    assert!(lab.exists(gone));
}

#[test]
fn removing_a_review_row_keeps_the_files_and_the_other_row_and_can_be_undone() {
    let lab = Lab::new();
    let (show, _season, episodes) = lab.show("Dark");
    let copy = lab.duplicate_show(show);
    let video = Path::new(&lab.tv.path).join("Dark/Season 1/Dark S01E01.mkv");
    lab.fingerprint(&lab.tv, &video);
    let review = lab.review();
    assert_eq!(
        reasons_of(&review.list().unwrap()),
        vec![(copy, ReviewReason::Duplicate)]
    );

    let removal = review.remove(&[copy, show]).unwrap();

    // Only the listed row goes: the show that is not in the review is refused.
    assert_eq!(removal.report.removed.len(), 1);
    assert_eq!(removal.report.problems.len(), 1);
    assert!(!lab.exists(copy) && lab.exists(show));
    assert!(
        episodes.iter().all(|e| lab.exists(*e)),
        "the other row keeps its episodes"
    );
    assert!(video.is_file(), "no file is touched");
    assert_eq!(
        lab.fingerprints(),
        1,
        "the kept row's file keeps its fingerprint"
    );
    assert!(review.list().unwrap().rows.is_empty());

    let token = removal.undo_token.unwrap();
    let restored = review.undo_removal(&token).unwrap();
    assert_eq!(restored.len(), 1);
    let back = restored[0].id;
    assert_eq!(lab.item(back).core.title, "Dark");
    let kids = lab.db.read(|c| cv_db::repo::children_of(c, back)).unwrap();
    assert_eq!(kids.len(), 3, "the season and both episodes come back");
    assert_eq!(
        reasons_of(&review.list().unwrap()),
        vec![(back, ReviewReason::Duplicate)]
    );
    // A token works once.
    assert!(review.undo_removal(&token).unwrap().is_empty());
}

#[test]
fn relocating_a_missing_movie_points_it_at_the_folder_its_files_moved_to() {
    let lab = Lab::new();
    let id = lab.movie(&lab.movies, "Moved (1995)");
    let old = Path::new(&lab.movies.path).join("Moved (1995)");
    let new = Path::new(&lab.archive.path).join("Moved (1995)");
    fs::rename(&old, &new).unwrap();
    lab.refresh();
    assert_eq!(lab.availability(id), Availability::Missing);
    let review = lab.review();

    let outside = lab.dir.path().join("elsewhere");
    fs::create_dir_all(&outside).unwrap();
    let refused = review.relocate(id, &outside.to_string_lossy()).unwrap_err();
    assert_eq!(refused.code(), "data_source");
    let nowhere = review
        .relocate(id, &new.join("nope").to_string_lossy())
        .unwrap_err();
    assert_eq!(nowhere.code(), "exists");

    let done = review.relocate(id, &new.to_string_lossy()).unwrap();

    assert_eq!(done.availability, Availability::Online);
    let item = lab.item(id);
    assert_eq!(item.core.path, new.to_string_lossy());
    assert_eq!(item.core.data_source, lab.archive.path);
    assert!(
        item.core
            .media_files
            .iter()
            .all(|f| f.directory == new.to_string_lossy())
    );
    assert!(review.list().unwrap().rows.is_empty());
    // An online title is not relocated.
    assert_eq!(
        review
            .relocate(id, &new.to_string_lossy())
            .unwrap_err()
            .code(),
        "unavailable"
    );
}

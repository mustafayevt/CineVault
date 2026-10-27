//! The library's answer to "which file is behind this image key", for the `cv://img` protocol.
//!
//! The grid shows a poster per library item. The poster is an image file recorded for the item
//! (`media_file` rows of the poster kinds, see [`poster_files_for`]). The thumbnail cache is keyed
//! by the *content* of the image ([`ThumbKey::of_bytes`]), so showing a poster takes two steps:
//!
//! 1. [`LibrarySources::posters_for`] reads the poster rows of a page of items, hashes each file
//!    once and remembers `key -> file`. The key goes to the UI inside the card.
//! 2. The UI puts the key in a `cv://img/<key>/grid` URL. If the thumbnail is cached the protocol
//!    serves it and never asks for the file; otherwise [`ImageService`](crate::img::ImageService)
//!    asks [`SourceResolver::source_of`] here and generates it lazily.
//!
//! # Cost
//!
//! Hashing reads the whole poster. The answer is remembered per file together with its size and
//! modification time, so a poster is read again only when the file changed. The memory held is one
//! key and one path per poster that was shown.
//!
//! A page of the grid does not touch the disk for posters it already knows: the library records
//! each poster's size and modification time (the scan and the artwork writer set them), and when
//! they match what was remembered with the key, the key is used as it is. Only a poster that is
//! new, or whose recorded size or time changed, is looked at on disk. A title whose folder is
//! offline is never looked at; it shows the key remembered for it, if any. A collection that
//! borrows a `movieset-poster.*` from a movie folder is looked up at most once every
//! [`SET_LOOKUP_TTL`].
//!
//! # Renames
//!
//! The thumbnails are keyed by content, so a renamed poster still has its cached thumbnails. What
//! a rename changes is the path behind the key. Organize tells [`LibrarySources`] about every move
//! it commits ([`PathObserver`]) and [`LibrarySources::repoint`] carries the index and the
//! remembered hashes over to the new path: the next page of the grid asks for the poster at its
//! new place, finds its remembered key (no file is read again) and the protocol serves the cached
//! thumbnail, or generates a missing size from the new path. No rescan is involved.
//!
//! # Offline folders
//!
//! The thumbnails live in the app's own cache, so a poster whose source folder is offline can
//! still be shown, as long as its key is known. When a poster file cannot be read,
//! [`LibrarySources::key_for_file`] answers with the key remembered for that path. The
//! path-to-key map is saved next to the thumbnail cache ([`LibrarySources::with_store`]), so it
//! survives a restart that happens while the folder is away.
//!
//! # Safety
//!
//! The protocol only ever receives a parsed [`ThumbKey`]; the path comes from this index, never from
//! the request. A file whose content changed since it was indexed no longer hashes to its key, so the
//! protocol answers 404 and the entry is forgotten (see [`SourceResolver::forget`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use cv_db::repo::graphics::{PosterFile, poster_files_with_state, set_member_folders};
use cv_db::repo::rename_journal::PathObserver;
use cv_db::{Database, DbError};
use cv_domain::ItemId;
use serde::{Deserialize, Serialize};

use crate::img::{SourceMap, SourceResolver};
use crate::thumb::{ThumbError, ThumbKey};

/// The names a collection poster has next to a movie (`cv_artwork::naming::set`, movie-folder
/// style), in the order they are looked for.
const SET_POSTER_NAMES: [&str; 4] = [
    "movieset-poster.jpg",
    "movieset-poster.png",
    "movieset-poster.webp",
    "movieset-poster.jpeg",
];

/// How long the answer to "which `movieset-poster.*` does this collection borrow" is reused before
/// the movie folders are looked at again.
pub const SET_LOOKUP_TTL: Duration = Duration::from_secs(10 * 60);

/// A remembered key that has not been used for this many days is dropped when the map is
/// loaded: its title was removed, or its poster moved.
pub const FORGET_UNUSED_AFTER_DAYS: u32 = 180;

/// The map is written at most this often while pages are shown.
pub const SAVE_INTERVAL: Duration = Duration::from_secs(30);

/// What was learned about one file the last time it was hashed.
#[derive(Clone)]
struct Hashed {
    size: u64,
    modified_ms: u128,
    key: ThumbKey,
    /// The last day (since 1970) the key was used.
    seen_day: u32,
}

/// Today, in days since 1970.
fn today() -> u32 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| {
            u32::try_from(d.as_secs() / 86_400).unwrap_or(u32::MAX)
        })
}

/// Maps image keys to the poster files of the library, and knows each poster's key.
#[derive(Default)]
pub struct LibrarySources {
    index: RwLock<HashMap<ThumbKey, PathBuf>>,
    hashed: Mutex<HashMap<PathBuf, Hashed>>,
    /// Files registered by hand (the gallery's image probe in debug builds).
    manual: SourceMap,
    /// Where the path-to-key map is saved, when it is.
    store: Option<PathBuf>,
    /// Whether the map learned something the store does not have yet.
    dirty: AtomicBool,
    /// The borrowed collection posters found (or not) recently, by collection.
    set_lookups: Mutex<HashMap<ItemId, (Instant, Option<PathBuf>)>>,
    /// When the map was last written.
    last_save: Mutex<Option<Instant>>,
}

/// One saved entry of the path-to-key map.
#[derive(Serialize, Deserialize)]
struct StoredKey {
    path: String,
    key: String,
    size: u64,
    modified_ms: u64,
    /// The last day (since 1970) the key was used; 0 in maps written before it was kept.
    #[serde(default)]
    seen_day: u32,
}

impl LibrarySources {
    pub fn new() -> Self {
        Self::default()
    }

    /// An index that saves its path-to-key map in `store` and starts from what is saved there. A
    /// missing or unreadable file means an empty map: the keys are learned again from the files.
    pub fn with_store(store: PathBuf) -> Self {
        let sources = Self {
            store: Some(store.clone()),
            ..Self::default()
        };
        let saved: Vec<StoredKey> = std::fs::read(&store)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let today = today();
        if let Ok(mut hashed) = sources.hashed.lock() {
            for entry in saved {
                let seen_day = if entry.seen_day == 0 {
                    today
                } else {
                    entry.seen_day
                };
                if today.saturating_sub(seen_day) > FORGET_UNUSED_AFTER_DAYS {
                    sources.dirty.store(true, Ordering::Release);
                    continue;
                }
                if let Ok(key) = ThumbKey::parse(&entry.key) {
                    hashed.insert(
                        PathBuf::from(entry.path),
                        Hashed {
                            size: entry.size,
                            modified_ms: u128::from(entry.modified_ms),
                            key,
                            seen_day,
                        },
                    );
                }
            }
        }
        sources
    }

    /// Writes the map when it changed and the last write is at least [`SAVE_INTERVAL`] ago.
    fn save_now_and_then(&self) {
        if !self.dirty.load(Ordering::Acquire) {
            return;
        }
        let due = self.last_save.lock().map_or(true, |last| {
            last.is_none_or(|at| at.elapsed() >= SAVE_INTERVAL)
        });
        if due {
            self.save();
        }
    }

    /// Notes that the key of `path` was used today.
    fn mark_seen(&self, path: &Path) {
        let today = today();
        if let Ok(mut hashed) = self.hashed.lock()
            && let Some(known) = hashed.get_mut(path)
            && known.seen_day != today
        {
            known.seen_day = today;
            self.dirty.store(true, Ordering::Release);
        }
    }

    /// Saves the path-to-key map when it changed since it was last saved. Failures are logged:
    /// the map is only a convenience for offline folders.
    pub fn save(&self) {
        let Some(store) = &self.store else { return };
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return;
        }
        if let Ok(mut last) = self.last_save.lock() {
            *last = Some(Instant::now());
        }
        let entries: Vec<StoredKey> = match self.hashed.lock() {
            Ok(hashed) => hashed
                .iter()
                .map(|(path, known)| StoredKey {
                    path: path.to_string_lossy().into_owned(),
                    key: known.key.as_str().to_owned(),
                    size: known.size,
                    modified_ms: u64::try_from(known.modified_ms).unwrap_or(0),
                    seen_day: known.seen_day,
                })
                .collect(),
            Err(_) => return,
        };
        let written = serde_json::to_vec(&entries)
            .map_err(std::io::Error::other)
            .and_then(|bytes| cv_fs::write_atomic(store, &bytes));
        if let Err(error) = written {
            self.dirty.store(true, Ordering::Release);
            tracing::warn!(%error, "cannot save the poster key map");
        }
    }

    /// The hand-registered files; consulted after the library index.
    pub fn manual(&self) -> &SourceMap {
        &self.manual
    }

    /// The content key of the image file at `path`, hashing it unless an up-to-date answer is
    /// remembered, and indexes `key -> path`. Blocking file I/O.
    pub fn key_for_file(&self, path: &Path) -> Result<ThumbKey, ThumbError> {
        let io = |source| ThumbError::Io {
            path: path.to_path_buf(),
            source,
        };
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(source) => {
                // The file cannot be reached (its folder is offline): the thumbnail cache is in
                // the app's data, so the key remembered for this path still shows the poster.
                let remembered = self
                    .hashed
                    .lock()
                    .ok()
                    .and_then(|hashed| hashed.get(path).map(|known| known.key.clone()));
                return match remembered {
                    Some(key) => {
                        self.index_key(&key, path);
                        self.mark_seen(path);
                        Ok(key)
                    }
                    None => Err(io(source)),
                };
            }
        };
        let size = meta.len();
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis());

        let known = self
            .hashed
            .lock()
            .ok()
            .and_then(|hashed| hashed.get(path).cloned())
            .filter(|known| known.size == size && known.modified_ms == modified_ms);
        if let Some(known) = known {
            self.index_key(&known.key, path);
            self.mark_seen(path);
            return Ok(known.key);
        }

        let bytes = std::fs::read(path).map_err(io)?;
        let key = ThumbKey::of_bytes(&bytes);
        if let Ok(mut hashed) = self.hashed.lock() {
            hashed.insert(
                path.to_path_buf(),
                Hashed {
                    size,
                    modified_ms,
                    key: key.clone(),
                    seen_day: today(),
                },
            );
            self.dirty.store(true, Ordering::Release);
        }
        self.index_key(&key, path);
        Ok(key)
    }

    /// The key remembered for `path`, if any, without touching the disk.
    fn remembered(&self, path: &Path) -> Option<Hashed> {
        self.hashed
            .lock()
            .ok()
            .and_then(|hashed| hashed.get(path).cloned())
    }

    /// The key of a grid poster, from memory when the library's record of the file matches what
    /// was remembered, and from the disk otherwise. An unavailable title is never looked at on
    /// disk.
    fn key_for_poster(&self, path: &Path, poster: &PosterFile) -> Result<ThumbKey, ThumbError> {
        let remembered = self.remembered(path);
        if let Some(known) = &remembered {
            let recorded = poster.size_bytes > 0
                && known.size == poster.size_bytes
                && u128::try_from(poster.modified_ms).is_ok_and(|ms| ms == known.modified_ms);
            if recorded || poster.unavailable {
                self.index_key(&known.key, path);
                self.mark_seen(path);
                return Ok(known.key.clone());
            }
        }
        if poster.unavailable {
            return Err(ThumbError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "the title's folder is offline",
                ),
            });
        }
        self.key_for_file(path)
    }

    /// The `movieset-poster.*` a collection borrows from one of its movie folders, looked up on
    /// disk at most once every [`SET_LOOKUP_TTL`].
    fn set_poster(&self, set: ItemId, dirs: &[String]) -> Option<PathBuf> {
        if let Some((at, found)) = self
            .set_lookups
            .lock()
            .ok()
            .and_then(|lookups| lookups.get(&set).cloned())
            && at.elapsed() < SET_LOOKUP_TTL
        {
            return found;
        }
        let found = dirs.iter().find_map(|dir| {
            SET_POSTER_NAMES
                .iter()
                .map(|name| Path::new(dir).join(name))
                .find(|path| path.is_file())
        });
        if let Ok(mut lookups) = self.set_lookups.lock() {
            lookups.insert(set, (Instant::now(), found.clone()));
        }
        found
    }

    /// Forgets the borrowed collection posters found so far, so the next page looks again (a
    /// collection poster was written, or folders moved).
    pub fn forget_set_lookups(&self) {
        if let Ok(mut lookups) = self.set_lookups.lock() {
            lookups.clear();
        }
    }

    fn index_key(&self, key: &ThumbKey, path: &Path) {
        if let Ok(mut index) = self.index.write() {
            index.insert(key.clone(), path.to_path_buf());
        }
    }

    /// Follows a move of `from` to `to` (a folder moves everything below it): every indexed and
    /// remembered path that is `from`, or lies below it when `is_dir`, now points at the new place.
    /// Keys, sizes and modification times are kept, so nothing is hashed or decoded again.
    /// Matching ignores ASCII case and the kind of separator, like the library's own repointing.
    /// Returns how many paths were moved.
    pub fn repoint(&self, from: &str, to: &str, is_dir: bool) -> usize {
        self.forget_set_lookups();
        let mut moved = 0;
        if let Ok(mut index) = self.index.write() {
            for path in index.values_mut() {
                if let Some(new) = rebase(path, from, to, is_dir) {
                    *path = new;
                    moved += 1;
                }
            }
        }
        if let Ok(mut hashed) = self.hashed.lock() {
            let renamed: Vec<(PathBuf, PathBuf)> = hashed
                .keys()
                .filter_map(|path| rebase(path, from, to, is_dir).map(|new| (path.clone(), new)))
                .collect();
            for (old, new) in renamed {
                if let Some(entry) = hashed.remove(&old) {
                    hashed.insert(new, entry);
                    self.dirty.store(true, Ordering::Release);
                }
            }
        }
        moved
    }

    /// The poster key of each of `items` that has a readable poster file. Items without a poster
    /// row, or whose file cannot be read, are absent: the grid draws a generated placeholder. A
    /// collection without a poster row borrows the `movieset-poster.*` next to one of its movies.
    /// Blocking: reads the database and hashes files.
    pub fn posters_for(
        &self,
        db: &Database,
        items: &[ItemId],
    ) -> Result<HashMap<ItemId, ThumbKey>, DbError> {
        let files = db.read(|conn| poster_files_with_state(conn, items))?;
        let mut keys = HashMap::with_capacity(files.len());
        for (item, poster) in files {
            let path = PathBuf::from(&poster.file.directory).join(&poster.file.filename);
            match self.key_for_poster(&path, &poster) {
                Ok(key) => {
                    keys.insert(item, key);
                }
                Err(error) => {
                    tracing::debug!(%error, path = %path.display(), "poster not readable");
                }
            }
        }
        // A collection without a poster of its own shows the `movieset-poster.*` next to one of
        // its movies (the Kodi movie-folder style), the earliest release first.
        let missing: Vec<ItemId> = items
            .iter()
            .copied()
            .filter(|item| !keys.contains_key(item))
            .collect();
        if !missing.is_empty() {
            let folders = db.read(|conn| set_member_folders(conn, &missing))?;
            for (set, dirs) in folders {
                if let Some(path) = self.set_poster(set, &dirs) {
                    match self.key_for_file(&path) {
                        Ok(key) => {
                            keys.insert(set, key);
                        }
                        Err(error) => {
                            tracing::debug!(%error, path = %path.display(), "set poster not readable");
                        }
                    }
                }
            }
        }
        self.save_now_and_then();
        Ok(keys)
    }

    /// How many keys are indexed.
    pub fn len(&self) -> usize {
        self.index.read().map_or(0, |index| index.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl PathObserver for LibrarySources {
    fn path_moved(&self, from: &str, to: &str, is_dir: bool) {
        self.repoint(from, to, is_dir);
    }
}

/// `path` after the move of `from` to `to`, or `None` when the move does not touch it.
fn rebase(path: &Path, from: &str, to: &str, is_dir: bool) -> Option<PathBuf> {
    let path_text = path.to_string_lossy();
    let from = from.trim_end_matches(['\\', '/']);
    let to = to.trim_end_matches(['\\', '/']);
    let mut path_chars = path_text.chars();
    let mut from_chars = from.chars();
    let mut consumed = 0;
    loop {
        match from_chars.next() {
            None => break,
            Some(f) => {
                let p = path_chars.next()?;
                if !same_char(p, f) {
                    return None;
                }
                consumed += p.len_utf8();
            }
        }
    }
    let rest = &path_text[consumed..];
    if rest.is_empty() {
        // The moved file or folder itself.
        return Some(PathBuf::from(to));
    }
    if is_dir && rest.starts_with(['\\', '/']) {
        return Some(PathBuf::from(format!("{to}{rest}")));
    }
    None
}

fn same_char(a: char, b: char) -> bool {
    let is_sep = |c: char| c == '\\' || c == '/';
    (is_sep(a) && is_sep(b)) || a.eq_ignore_ascii_case(&b)
}

impl SourceResolver for LibrarySources {
    fn source_of(&self, key: &ThumbKey) -> Option<PathBuf> {
        if let Some(path) = self.index.read().ok().and_then(|i| i.get(key).cloned()) {
            return Some(path);
        }
        self.manual.source_of(key)
    }

    fn forget(&self, key: &ThumbKey) {
        if let Ok(mut index) = self.index.write() {
            index.remove(key);
        }
        self.manual.forget(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_domain::{Item, MediaFile, MediaFileType};

    fn open() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        (dir, db)
    }

    fn movie_with_poster(dir: &Path, title: &str, bytes: &[u8]) -> Item {
        let folder = dir.join(title);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("poster.jpg"), bytes).unwrap();
        let mut item = Item::movie(title);
        item.core.path = folder.to_string_lossy().into_owned();
        item.core.media_files.push(MediaFile {
            kind: MediaFileType::Poster,
            directory: folder.to_string_lossy().into_owned(),
            filename: "poster.jpg".into(),
            ..MediaFile::default()
        });
        item
    }

    fn insert(db: &Database, item: Item) -> ItemId {
        db.write(move |conn| cv_db::repo::insert_item(conn, &item))
            .unwrap()
    }

    #[test]
    fn a_poster_row_resolves_to_its_content_key_and_back_to_the_file() {
        let (dir, db) = open();
        let id = insert(&db, movie_with_poster(dir.path(), "Heat", b"poster bytes"));
        let sources = LibrarySources::new();

        let keys = sources.posters_for(&db, &[id]).unwrap();

        let key = &keys[&id];
        assert_eq!(*key, ThumbKey::of_bytes(b"poster bytes"));
        let path = sources.source_of(key).unwrap();
        assert!(path.ends_with("poster.jpg"));
    }

    #[test]
    fn items_without_a_readable_poster_are_left_out() {
        let (dir, db) = open();
        let with = insert(&db, movie_with_poster(dir.path(), "Heat", b"a"));
        let missing = {
            let item = movie_with_poster(dir.path(), "Ronin", b"b");
            std::fs::remove_file(dir.path().join("Ronin").join("poster.jpg")).unwrap();
            insert(&db, item)
        };
        let bare = insert(&db, Item::movie("Thief"));
        let sources = LibrarySources::new();

        let keys = sources.posters_for(&db, &[with, missing, bare]).unwrap();

        assert_eq!(keys.len(), 1);
        assert!(keys.contains_key(&with));
    }

    #[test]
    fn a_collection_without_a_poster_shows_the_movieset_poster_next_to_its_movies() {
        // Every movie folder held a movieset-poster.jpg, the cards drew placeholders.
        let (dir, db) = open();
        let set = insert(&db, Item::set("Heat Collection"));
        let bare = insert(&db, Item::set("Empty Collection"));
        let mut heat = movie_with_poster(dir.path(), "Heat", b"movie poster");
        heat.parent_id = Some(set);
        heat.core.media_files.push(MediaFile {
            kind: MediaFileType::Video,
            directory: dir.path().join("Heat").to_string_lossy().into_owned(),
            filename: "Heat.mkv".into(),
            ..MediaFile::default()
        });
        std::fs::write(
            dir.path().join("Heat").join("movieset-poster.jpg"),
            b"set poster",
        )
        .unwrap();
        let heat = insert(&db, heat);
        let sources = LibrarySources::new();

        let keys = sources.posters_for(&db, &[set, bare, heat]).unwrap();

        assert_eq!(keys[&set], ThumbKey::of_bytes(b"set poster"));
        assert_eq!(keys[&heat], ThumbKey::of_bytes(b"movie poster"));
        assert!(!keys.contains_key(&bare));
    }

    /// A movie whose poster row carries the file's real size and modification time, as the scan
    /// and the artwork writer record them.
    fn movie_with_recorded_poster(dir: &Path, title: &str, bytes: &[u8]) -> Item {
        let mut item = movie_with_poster(dir, title, bytes);
        let poster = &mut item.core.media_files[0];
        let meta = std::fs::metadata(poster.path()).unwrap();
        poster.size_bytes = meta.len();
        poster.modified_ms = meta
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        item
    }

    #[test]
    fn a_known_poster_whose_record_did_not_change_is_not_looked_at_on_disk() {
        let (dir, db) = open();
        let id = insert(
            &db,
            movie_with_recorded_poster(dir.path(), "Heat", b"poster bytes"),
        );
        let sources = LibrarySources::new();
        let first = sources.posters_for(&db, &[id]).unwrap()[&id].clone();

        // The file changes behind the library's back: as long as the record says the poster is
        // the one that was hashed, the page does not read the disk and keeps the key.
        let path = dir.path().join("Heat").join("poster.jpg");
        std::fs::write(&path, b"changed on disk, not yet rescanned").unwrap();
        assert_eq!(sources.posters_for(&db, &[id]).unwrap()[&id], first);

        // The scan (or the artwork writer) records the new file: it is hashed again.
        let mut item = movie_with_recorded_poster(dir.path(), "Heat", b"the new poster");
        item.id = Some(id);
        db.write(move |conn| cv_db::repo::update_item(conn, &item))
            .unwrap();
        assert_eq!(
            sources.posters_for(&db, &[id]).unwrap()[&id],
            ThumbKey::of_bytes(b"the new poster")
        );
    }

    #[test]
    fn an_offline_title_is_never_looked_at_and_shows_its_remembered_poster() {
        let (dir, db) = open();
        let known = insert(&db, movie_with_poster(dir.path(), "Heat", b"heat poster"));
        let unknown = insert(&db, movie_with_poster(dir.path(), "Ronin", b"ronin poster"));
        let sources = LibrarySources::new();
        let key = sources.posters_for(&db, &[known]).unwrap()[&known].clone();

        db.write(|conn| {
            conn.execute("UPDATE item SET availability = 'offline'", [])?;
            Ok(())
        })
        .unwrap();
        // Readable files, but the titles are offline: no disk access, only what is remembered.
        let keys = sources.posters_for(&db, &[known, unknown]).unwrap();
        assert_eq!(keys[&known], key);
        assert!(!keys.contains_key(&unknown));
    }

    #[test]
    fn a_borrowed_collection_poster_is_looked_up_once_until_forgotten() {
        let (dir, db) = open();
        let set = insert(&db, Item::set("Heat Collection"));
        let mut heat = movie_with_poster(dir.path(), "Heat", b"movie poster");
        heat.parent_id = Some(set);
        heat.core.media_files.push(MediaFile {
            kind: MediaFileType::Video,
            directory: dir.path().join("Heat").to_string_lossy().into_owned(),
            filename: "Heat.mkv".into(),
            ..MediaFile::default()
        });
        insert(&db, heat);
        let sources = LibrarySources::new();
        assert!(sources.posters_for(&db, &[set]).unwrap().is_empty());

        std::fs::write(
            dir.path().join("Heat").join("movieset-poster.jpg"),
            b"set poster",
        )
        .unwrap();
        assert!(
            sources.posters_for(&db, &[set]).unwrap().is_empty(),
            "the folders are not looked at again right away"
        );
        sources.forget_set_lookups();
        assert_eq!(
            sources.posters_for(&db, &[set]).unwrap()[&set],
            ThumbKey::of_bytes(b"set poster")
        );
    }

    #[test]
    fn an_unchanged_file_keeps_its_key_and_a_changed_one_is_hashed_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("poster.jpg");
        std::fs::write(&path, b"first").unwrap();
        let sources = LibrarySources::new();
        let first = sources.key_for_file(&path).unwrap();

        assert_eq!(sources.key_for_file(&path).unwrap(), first);
        assert_eq!(sources.len(), 1);

        std::fs::write(&path, b"second, longer").unwrap();
        let second = sources.key_for_file(&path).unwrap();
        assert_ne!(first, second);
        assert_eq!(second, ThumbKey::of_bytes(b"second, longer"));
    }

    #[test]
    fn an_unreachable_file_keeps_the_key_remembered_for_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("poster.jpg");
        std::fs::write(&path, b"pixels").unwrap();
        let sources = LibrarySources::new();
        let key = sources.key_for_file(&path).unwrap();

        // The folder goes offline.
        std::fs::remove_file(&path).unwrap();

        assert_eq!(sources.key_for_file(&path).unwrap(), key);
        // A file never seen is still an error.
        assert!(sources.key_for_file(&dir.path().join("other.jpg")).is_err());
    }

    #[test]
    fn the_remembered_keys_survive_a_restart_while_the_folder_is_offline() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("cache").join("poster-keys.json");
        std::fs::create_dir_all(store.parent().unwrap()).unwrap();
        let path = dir.path().join("poster.jpg");
        std::fs::write(&path, b"pixels").unwrap();
        let first = LibrarySources::with_store(store.clone());
        let key = first.key_for_file(&path).unwrap();
        first.save();
        assert!(store.is_file());

        std::fs::remove_file(&path).unwrap();
        let second = LibrarySources::with_store(store);

        assert_eq!(second.key_for_file(&path).unwrap(), key);
    }

    #[test]
    fn keys_unused_for_half_a_year_are_dropped_when_the_map_is_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("poster-keys.json");
        let key = ThumbKey::of_bytes(b"x");
        let entries = serde_json::json!([
            { "path": "/old/poster.jpg", "key": key.as_str(), "size": 1, "modified_ms": 1,
              "seen_day": today() - FORGET_UNUSED_AFTER_DAYS - 1 },
            { "path": "/recent/poster.jpg", "key": key.as_str(), "size": 1, "modified_ms": 1,
              "seen_day": today() - 3 },
            { "path": "/before-days-were-kept/poster.jpg", "key": key.as_str(), "size": 1,
              "modified_ms": 1 }
        ]);
        std::fs::write(&store, serde_json::to_vec(&entries).unwrap()).unwrap();
        let sources = LibrarySources::with_store(store.clone());
        assert!(sources.remembered(Path::new("/old/poster.jpg")).is_none());
        assert!(
            sources
                .remembered(Path::new("/recent/poster.jpg"))
                .is_some()
        );
        assert!(
            sources
                .remembered(Path::new("/before-days-were-kept/poster.jpg"))
                .is_some()
        );
        sources.save();
        let saved: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(&store).unwrap()).unwrap();
        assert_eq!(saved.len(), 2);
    }

    #[test]
    fn pages_in_quick_succession_write_the_map_once() {
        let (dir, db) = open();
        let store = dir.path().join("poster-keys.json");
        let a = insert(&db, movie_with_poster(dir.path(), "Heat", b"heat"));
        let b = insert(&db, movie_with_poster(dir.path(), "Ronin", b"ronin"));
        let sources = LibrarySources::with_store(store.clone());
        sources.posters_for(&db, &[a]).unwrap();
        let first = std::fs::read(&store).unwrap();
        // A new poster right after: remembered, but not written until the interval is over.
        sources.posters_for(&db, &[b]).unwrap();
        assert_eq!(std::fs::read(&store).unwrap(), first);
        sources.save();
        assert_ne!(std::fs::read(&store).unwrap(), first);
    }

    #[test]
    fn a_damaged_store_is_an_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("poster-keys.json");
        std::fs::write(&store, b"{ not json").unwrap();
        let sources = LibrarySources::with_store(store);
        assert!(sources.key_for_file(&dir.path().join("nope.jpg")).is_err());
    }

    #[test]
    fn forgetting_a_key_removes_it_and_unknown_keys_do_not_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("poster.jpg");
        std::fs::write(&path, b"x").unwrap();
        let sources = LibrarySources::new();
        let key = sources.key_for_file(&path).unwrap();

        sources.forget(&key);

        assert!(sources.source_of(&key).is_none());
        assert!(
            sources
                .source_of(&ThumbKey::of_bytes(b"never seen"))
                .is_none()
        );
    }

    #[test]
    fn a_renamed_file_keeps_its_key_and_is_found_at_the_new_path_without_hashing() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("poster.jpg");
        let new = dir.path().join("Heat (1995)-poster.jpg");
        std::fs::write(&old, b"pixels").unwrap();
        let sources = LibrarySources::new();
        let key = sources.key_for_file(&old).unwrap();

        std::fs::rename(&old, &new).unwrap();
        let moved = sources.repoint(&old.to_string_lossy(), &new.to_string_lossy(), false);

        assert_eq!(moved, 1);
        assert_eq!(sources.source_of(&key).as_deref(), Some(new.as_path()));
        // The remembered hash followed the file: poison the content, keep size and mtime, and the
        // answer is still the old key, which proves the file was not read again.
        let meta = std::fs::metadata(&new).unwrap();
        std::fs::write(&new, b"PIXELS").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&new)
            .unwrap()
            .set_modified(meta.modified().unwrap())
            .unwrap();
        assert_eq!(sources.key_for_file(&new).unwrap(), key);
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn a_renamed_folder_moves_every_poster_below_it_but_not_a_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let heat = dir.path().join("Heat");
        let heat2 = dir.path().join("Heat 2");
        std::fs::create_dir_all(heat.join("extrafanart")).unwrap();
        std::fs::create_dir_all(&heat2).unwrap();
        std::fs::write(heat.join("poster.jpg"), b"a").unwrap();
        std::fs::write(heat.join("extrafanart").join("fanart1.jpg"), b"b").unwrap();
        std::fs::write(heat2.join("poster.jpg"), b"c").unwrap();
        let sources = LibrarySources::new();
        let a = sources.key_for_file(&heat.join("poster.jpg")).unwrap();
        let b = sources
            .key_for_file(&heat.join("extrafanart").join("fanart1.jpg"))
            .unwrap();
        let c = sources.key_for_file(&heat2.join("poster.jpg")).unwrap();
        let renamed = dir.path().join("Heat (1995)");

        let moved = sources.repoint(
            &heat.to_string_lossy().to_ascii_uppercase(),
            &renamed.to_string_lossy(),
            true,
        );

        assert_eq!(moved, 2);
        assert_eq!(sources.source_of(&a).unwrap(), renamed.join("poster.jpg"));
        assert_eq!(
            sources.source_of(&b).unwrap(),
            renamed.join("extrafanart").join("fanart1.jpg")
        );
        assert_eq!(sources.source_of(&c).unwrap(), heat2.join("poster.jpg"));
    }

    #[test]
    fn a_file_move_does_not_touch_a_folder_of_the_same_prefix() {
        assert!(
            rebase(
                Path::new("D:\\M\\Heat\\a.jpg"),
                "D:\\M\\Heat",
                "D:\\M\\X",
                false
            )
            .is_none()
        );
        assert_eq!(
            rebase(
                Path::new("D:\\M\\Heat\\a.jpg"),
                "d:/m/heat",
                "D:\\M\\X",
                true
            )
            .unwrap(),
            PathBuf::from("D:\\M\\X\\a.jpg")
        );
    }

    #[test]
    fn hand_registered_files_resolve_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe.png");
        std::fs::write(&path, b"probe").unwrap();
        let sources = LibrarySources::new();
        let key = sources.manual().register_file(&path).unwrap();
        assert_eq!(sources.source_of(&key).as_deref(), Some(path.as_path()));
    }
}

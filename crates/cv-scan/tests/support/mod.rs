//! Shared helpers for the rescan tests: a library database with one data source, file-tree
//! builders and a content hash of the tables a rescan must not touch.

#![allow(dead_code)]

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use cv_db::repo::data_sources::insert_source;
use cv_db::repo::{get_item, update_item};
use cv_db::{Database, DbOptions};
use cv_domain::{DataSource, Item, ItemId, ItemKind, SourceKind};
use rusqlite::types::ValueRef;

/// A temporary library: a database and one data source folder.
pub struct Lib {
    pub dir: tempfile::TempDir,
    pub db: Arc<Database>,
    pub root: PathBuf,
    pub source: DataSource,
}

impl Lib {
    pub fn movies() -> Self {
        Self::with(SourceKind::Movie, 4)
    }

    pub fn tv() -> Self {
        Self::with(SourceKind::Tv, 4)
    }

    /// One reader connection, so `PRAGMA data_version` always asks the same connection.
    pub fn single_reader(kind: SourceKind) -> Self {
        Self::with(kind, 1)
    }

    fn with(kind: SourceKind, readers: usize) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("library");
        fs::create_dir_all(&root).expect("library folder");
        let db = Arc::new(
            Database::open_with(dir.path().join("library.db"), &DbOptions { readers })
                .expect("open database"),
        );
        let path = root.to_string_lossy().into_owned();
        let source = db
            .write(move |tx| insert_source(tx, kind, &path))
            .expect("add data source");
        Self {
            dir,
            db,
            root,
            source,
        }
    }

    /// Creates a file with `content` under the library folder.
    pub fn put(&self, rel: &str, content: &str) -> PathBuf {
        put(&self.root.join(rel), content)
    }

    pub fn remove(&self, rel: &str) {
        let path = self.root.join(rel);
        if path.is_dir() {
            fs::remove_dir_all(path).expect("remove folder");
        } else {
            fs::remove_file(path).expect("remove file");
        }
    }

    pub fn options(&self) -> cv_scan::rescan::RescanOptions {
        cv_scan::rescan::RescanOptions {
            now_ms: Some(1_700_000_000_000),
            ..Default::default()
        }
    }

    pub fn rescan(&self) -> cv_scan::rescan::RescanReport {
        cv_scan::rescan::rescan_source(&self.db, &self.source, &self.options()).expect("rescan")
    }

    /// Every item of a kind, in id order.
    pub fn items(&self, kind: ItemKind) -> Vec<Item> {
        let ids = self.item_ids(kind);
        self.db
            .read(|conn| {
                ids.iter()
                    .map(|id| Ok(get_item(conn, *id)?.expect("item exists")))
                    .collect()
            })
            .expect("read items")
    }

    pub fn item(&self, id: ItemId) -> Item {
        self.db
            .read(|conn| Ok(get_item(conn, id)?.expect("item exists")))
            .expect("read item")
    }

    pub fn item_ids(&self, kind: ItemKind) -> Vec<ItemId> {
        let kind = serde_json::to_value(kind).unwrap();
        let kind = kind.as_str().unwrap().to_owned();
        self.db
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT id FROM item WHERE kind = ?1 ORDER BY id")?;
                let ids = stmt
                    .query_map([kind], |r| Ok(ItemId(r.get(0)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(ids)
            })
            .expect("list ids")
    }

    pub fn item_count(&self) -> i64 {
        self.count("item")
    }

    pub fn count(&self, table: &str) -> i64 {
        self.db
            .read(|conn| {
                Ok(conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
            })
            .expect("count")
    }

    pub fn availability(&self, id: ItemId) -> String {
        self.db
            .read(|conn| {
                Ok(
                    conn.query_row("SELECT availability FROM item WHERE id = ?1", [id.0], |r| {
                        r.get::<_, String>(0)
                    })?,
                )
            })
            .expect("availability")
    }

    pub fn is_new(&self, id: ItemId) -> bool {
        self.db
            .read(|conn| Ok(cv_db::repo::get_flags(conn, id)?.is_new))
            .expect("flags")
    }

    /// Pretends the media-info step ran: every video of every movie and episode gets a container.
    pub fn read_media_info(&self) {
        let ids: Vec<ItemId> = self
            .item_ids(ItemKind::Movie)
            .into_iter()
            .chain(self.item_ids(ItemKind::Episode))
            .collect();
        self.db
            .write(move |tx| {
                for id in ids {
                    let mut item = get_item(tx, id)?.expect("item exists");
                    for file in &mut item.core.media_files {
                        if file.kind == cv_domain::MediaFileType::Video {
                            file.container_format = "Matroska".into();
                            file.width = 1920;
                            file.height = 1080;
                        }
                    }
                    update_item(tx, &item)?;
                }
                Ok(())
            })
            .expect("write media info");
    }

    /// A hash of the tables a rescan owns, to prove a rescan changed nothing.
    pub fn content_hash(&self) -> u64 {
        content_hash(&self.db)
    }
}

/// Creates a file (and its folders) with `content`.
pub fn put(path: &Path, content: &str) -> PathBuf {
    fs::create_dir_all(path.parent().expect("parent")).expect("create folder");
    fs::write(path, content).expect("create file");
    path.to_path_buf()
}

/// Sets a file's modification time to `secs` seconds after the epoch.
pub fn set_mtime(path: &Path, secs: u64) {
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for time change");
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
        .expect("set modification time");
}

const HASHED_TABLES: &[(&str, &str)] = &[
    ("item", "id"),
    ("media_file", "id"),
    ("stream", "id"),
    ("ext_id", "item_id, provider"),
    ("episode_number", "item_id, ordering, group_name"),
    ("fingerprint", "path"),
    ("data_source", "id"),
    ("job", "id"),
];

/// Hashes every row of the library tables, in a stable order.
pub fn content_hash(db: &Database) -> u64 {
    db.read(|conn| {
        let mut hasher = DefaultHasher::new();
        for (table, order) in HASHED_TABLES {
            table.hash(&mut hasher);
            let mut stmt = conn.prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))?;
            let columns = stmt.column_count();
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                for i in 0..columns {
                    match row.get_ref(i)? {
                        ValueRef::Null => 0u8.hash(&mut hasher),
                        ValueRef::Integer(v) => v.hash(&mut hasher),
                        ValueRef::Real(v) => v.to_bits().hash(&mut hasher),
                        ValueRef::Text(t) => t.hash(&mut hasher),
                        ValueRef::Blob(b) => b.hash(&mut hasher),
                    }
                }
            }
        }
        Ok(hasher.finish())
    })
    .expect("hash tables")
}

/// SQLite's `data_version` of this reader: it changes when any other connection commits.
pub fn data_version(db: &Database) -> i64 {
    db.read(|conn| Ok(conn.pragma_query_value(None, "data_version", |r| r.get(0))?))
        .expect("data_version")
}

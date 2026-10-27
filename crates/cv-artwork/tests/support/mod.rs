//! Shared helpers for the thumbnail-job tests: a temporary library with a rescan job manager,
//! synthetic poster files and a cache.

#![allow(dead_code)]

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_artwork::thumb::ThumbCache;
use cv_db::repo::data_sources::insert_source;
use cv_db::{Database, DbOptions};
use cv_domain::{DataSource, ItemId, SourceKind};
use cv_jobs::{JobManager, JobStore, LaneConfig, Lanes, ManagerConfig};
use cv_scan::rescan::RescanOptions;
use cv_scan::rescan::job::{RescanHandle, rescan_targets, submit_rescan};
use image::{ImageFormat, Rgb, RgbImage};

/// A temporary library with one data source, a job manager with a job store, and a thumbnail
/// cache.
pub struct Lib {
    pub dir: tempfile::TempDir,
    pub db: Arc<Database>,
    pub root: PathBuf,
    pub source: DataSource,
    pub manager: JobManager,
    pub cache: Arc<ThumbCache>,
}

impl Lib {
    pub fn new(kind: SourceKind) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("library");
        fs::create_dir_all(&root).expect("library folder");
        let db = Arc::new(
            Database::open_with(dir.path().join("library.db"), &DbOptions { readers: 4 })
                .expect("open database"),
        );
        let path = root.to_string_lossy().into_owned();
        let source = db
            .write(move |tx| insert_source(tx, kind, &path))
            .expect("add data source");
        let manager = JobManager::with_store(
            Lanes::new(LaneConfig::default()),
            ManagerConfig::default(),
            JobStore::new(Arc::clone(&db)),
        );
        let cache = Arc::new(
            ThumbCache::open(&dir.path().join("cache"), None).expect("open thumbnail cache"),
        );
        Self {
            dir,
            db,
            root,
            source,
            manager,
            cache,
        }
    }

    pub fn movies() -> Self {
        Self::new(SourceKind::Movie)
    }

    pub fn tv() -> Self {
        Self::new(SourceKind::Tv)
    }

    /// Writes `bytes` to `rel` under the library folder.
    pub fn put_bytes(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        put_bytes(&self.root.join(rel), bytes)
    }

    /// Queues a rescan of every source.
    pub fn rescan(&self) -> RescanHandle {
        let options = RescanOptions {
            now_ms: Some(1_700_000_000_000),
            ..Default::default()
        };
        let targets = rescan_targets(&self.db).expect("rescan targets");
        submit_rescan(&self.manager, Arc::clone(&self.db), options, targets, 2)
            .expect("submit rescan")
    }

    /// Every movie, show, season and episode id, in id order.
    pub fn item_ids(&self) -> Vec<ItemId> {
        self.db
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT id FROM item ORDER BY id")?;
                let ids = stmt
                    .query_map([], |r| Ok(ItemId(r.get(0)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(ids)
            })
            .expect("list ids")
    }
}

pub fn put_bytes(path: &Path, bytes: &[u8]) -> PathBuf {
    fs::create_dir_all(path.parent().expect("parent")).expect("create folder");
    fs::write(path, bytes).expect("create file");
    path.to_path_buf()
}

/// A smooth, patterned JPEG of `width` x `height`. `seed` shifts the pattern.
pub fn poster_jpeg(width: u32, height: u32, seed: u32) -> Vec<u8> {
    let image = RgbImage::from_fn(width, height, |x, y| {
        let fx = f64::from(x) / f64::from(width);
        let fy = f64::from(y) / f64::from(height);
        let wave = ((fx * 18.0 + f64::from(seed)).sin() * (fy * 12.0).cos() + 1.0) * 60.0;
        Rgb([
            (fx * 150.0 + wave) as u8,
            (fy * 180.0 + wave * 0.5) as u8,
            (110.0 + wave + f64::from(seed % 40)) as u8,
        ])
    });
    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
        .expect("encode jpeg");
    out
}

/// A copy of `base` with a different content hash: bytes after the end-of-image marker are
/// ignored by decoders, so the picture is the same but the cache key is new.
pub fn variant(base: &[u8], n: usize) -> Vec<u8> {
    let mut bytes = base.to_vec();
    bytes.extend_from_slice(format!("\nvariant-{n}").as_bytes());
    bytes
}

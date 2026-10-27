//! Content-keyed thumbnail cache.
//!
//! Every artwork image is resized once into lossy WebP files at three sizes (grid, list and
//! hero) and stored under the cache folder of the data location (never next to the media).
//! Next to the thumbnails the cache keeps a tiny ThumbHash placeholder and a colour palette
//! (dominant, ambient, vibrant and muted swatches), so a tile can paint a colour placeholder
//! in its first frame while the real thumbnail loads.
//!
//! # Keys
//!
//! The key is the SHA-256 of the image *bytes* (first 128 bits, 32 hex characters), not its
//! path. Renaming or moving a movie folder therefore finds the same entry again and nothing
//! is decoded or encoded: [`ThumbCache::ensure`] only reads and hashes the file. Two copies of
//! the same image share an entry.
//!
//! # Sizes
//!
//! Sizes are bounding boxes; the image keeps its aspect ratio and is never upscaled. They
//! cover the largest poster column of the design system (260 px minimum column width at
//! 1.5x to 2x display scale), a list row thumbnail, and a full-width hero backdrop:
//!
//! | Size | Bounding box | WebP quality |
//! |------|--------------|--------------|
//! | [`ThumbSize::Grid`] | 360 x 540 | 80 |
//! | [`ThumbSize::List`] | 120 x 180 | 80 |
//! | [`ThumbSize::Hero`] | 1600 x 900 | 82 |
//!
//! Sizes are generated lazily: callers ask for the sizes they need and only missing ones are
//! produced.
//!
//! # Layout
//!
//! ```text
//! <cache>/thumbs/<key[0..2]>/<key>/grid.webp
//!                                  list.webp
//!                                  hero.webp
//!                                  meta.json     (written last; its mtime is the last-use time)
//! ```
//!
//! # Size cap and cleanup
//!
//! [`ThumbCache::enforce_cap`] evicts whole entries, least recently used first, until the
//! cache is back under 90% of the cap. It runs automatically every
//! [`AUTO_PRUNE_EVERY`] generations. [`ThumbCache::clear`] is the "clear image cache" action
//! for Settings. Both only ever delete cache entries (folders whose names match the layout
//! above); they never touch media files, library rows or foreign files in the folder.

pub mod palette;

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use image::imageops::FilterType;
use image::{DynamicImage, ImageReader, RgbaImage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use palette::{Palette, Rgb, extract_palette};

/// Default cache cap: 1 GiB (5,000 posters at the three sizes take roughly 150 MB).
pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// After this many generated images the cap is enforced automatically.
pub const AUTO_PRUNE_EVERY: u64 = 200;

/// Stored in place of the cap when the cache has none.
const NO_CAP: u64 = u64::MAX;

/// Name of the folder inside the data location's cache folder that holds the thumbnails.
pub const THUMBS_DIR: &str = "thumbs";

/// Longest side of the image the ThumbHash and palette are computed from (ThumbHash needs <= 100).
const PLACEHOLDER_SIDE: u32 = 100;

/// `last use` is only refreshed when older than this, to avoid a write per cache hit.
const TOUCH_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// Temp files older than this are leftovers of an interrupted write.
const STALE_TEMP: Duration = Duration::from_secs(60 * 60);

const META_FILE: &str = "meta.json";

/// File stem of the original image kept in an entry beside its thumbnails.
const ORIGINAL_STEM: &str = "original";

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// How often [`write_atomic`] repeats a rename that failed (a file another program briefly holds).
const RENAME_RETRIES: u32 = 6;

/// Thumbnail errors.
#[derive(Debug, thiserror::Error)]
pub enum ThumbError {
    #[error("i/o error on {}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("cannot decode image: {0}")]
    Decode(String),
    #[error("cannot encode thumbnail: {0}")]
    Encode(String),
    #[error("invalid thumbnail key {0:?}")]
    InvalidKey(String),
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> ThumbError + '_ {
    move |source| ThumbError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// A thumbnail size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThumbSize {
    Grid,
    List,
    Hero,
}

impl ThumbSize {
    pub const ALL: [ThumbSize; 3] = [ThumbSize::Grid, ThumbSize::List, ThumbSize::Hero];

    /// Bounding box `(width, height)` in pixels.
    pub fn bounds(self) -> (u32, u32) {
        match self {
            ThumbSize::Grid => (360, 540),
            ThumbSize::List => (120, 180),
            ThumbSize::Hero => (1600, 900),
        }
    }

    /// WebP quality, 0 to 100.
    pub fn quality(self) -> f32 {
        match self {
            ThumbSize::Grid | ThumbSize::List => 80.0,
            ThumbSize::Hero => 82.0,
        }
    }

    /// Lower-case name, as used in `cv://img/<key>/<size>` and in file names.
    pub fn as_str(self) -> &'static str {
        match self {
            ThumbSize::Grid => "grid",
            ThumbSize::List => "list",
            ThumbSize::Hero => "hero",
        }
    }

    /// Inverse of [`ThumbSize::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }

    fn file_name(self) -> String {
        format!("{}.webp", self.as_str())
    }

    /// Dimensions an image of `width` x `height` gets at this size (aspect kept, no upscaling).
    pub fn fit(self, width: u32, height: u32) -> (u32, u32) {
        let (bw, bh) = self.bounds();
        if width <= bw && height <= bh {
            return (width.max(1), height.max(1));
        }
        let scale = (f64::from(bw) / f64::from(width)).min(f64::from(bh) / f64::from(height));
        (
            ((f64::from(width) * scale).round() as u32).clamp(1, bw),
            ((f64::from(height) * scale).round() as u32).clamp(1, bh),
        )
    }
}

/// Identity of an image by content: 32 lower-case hex characters.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThumbKey(String);

impl ThumbKey {
    /// Key of the given image bytes.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        Self(hex(&digest[..16]))
    }

    /// Parse a key (for example from a `cv://img` URL). Rejects anything but 32 lower-case hex digits.
    pub fn parse(text: &str) -> Result<Self, ThumbError> {
        if text.len() == 32 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            Ok(Self(text.to_owned()))
        } else {
            Err(ThumbError::InvalidKey(text.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ThumbKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

/// Per-image data stored beside the thumbnails.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbMeta {
    pub key: ThumbKey,
    /// Pixel size of the original image.
    pub source_width: u32,
    pub source_height: u32,
    /// ThumbHash of the image, as hex. Decode it with [`ThumbMeta::thumbhash_bytes`].
    pub thumbhash: String,
    /// `None` only when the image is fully transparent.
    pub palette: Option<Palette>,
    /// Seconds since the Unix epoch when the entry was first generated.
    pub created_unix: u64,
}

impl ThumbMeta {
    /// The raw ThumbHash bytes (the form the ThumbHash decoders expect).
    pub fn thumbhash_bytes(&self) -> Vec<u8> {
        unhex(&self.thumbhash).unwrap_or_default()
    }
}

/// Result of [`ThumbCache::ensure`].
#[derive(Clone, Debug)]
pub struct Ensured {
    pub key: ThumbKey,
    pub meta: ThumbMeta,
    /// Sizes that were generated by this call. Empty when everything came from the cache.
    pub generated: Vec<ThumbSize>,
}

impl Ensured {
    /// True when the call produced nothing, because every requested size was already cached.
    pub fn reused(&self) -> bool {
        self.generated.is_empty()
    }
}

/// Disk usage of the cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheUsage {
    pub entries: u64,
    pub bytes: u64,
}

/// What a cleanup removed and what is left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub removed_entries: u64,
    pub freed_bytes: u64,
    pub remaining_entries: u64,
    pub remaining_bytes: u64,
}

struct EntryInfo {
    key: ThumbKey,
    bytes: u64,
    last_used: SystemTime,
}

/// The thumbnail cache. Cheap to share behind an `Arc`; all methods take `&self` and are
/// safe to call from several threads (concurrent generation of one key is harmless: files
/// are written to a temp name and renamed into place).
#[derive(Debug)]
pub struct ThumbCache {
    root: PathBuf,
    /// The size cap in bytes; [`NO_CAP`] for none. Changeable while the cache is in use.
    max_bytes: AtomicU64,
    generations: AtomicU64,
    since_prune: AtomicU64,
}

impl ThumbCache {
    /// Open (creating if needed) the cache under `cache_dir` (the data location's cache folder),
    /// capped at `max_bytes` (`None` for no cap).
    pub fn open(cache_dir: &Path, max_bytes: Option<u64>) -> Result<Self, ThumbError> {
        let root = cache_dir.join(THUMBS_DIR);
        fs::create_dir_all(&root).map_err(io_err(&root))?;
        Ok(Self {
            root,
            max_bytes: AtomicU64::new(max_bytes.unwrap_or(NO_CAP)),
            generations: AtomicU64::new(0),
            since_prune: AtomicU64::new(0),
        })
    }

    /// The folder holding the shard folders.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The size cap, if there is one.
    pub fn max_bytes(&self) -> Option<u64> {
        match self.max_bytes.load(Ordering::Relaxed) {
            NO_CAP => None,
            cap => Some(cap),
        }
    }

    /// Changes the size cap (`None` for no cap). It takes effect at once for later cleanups; call
    /// [`Self::enforce_cap`] to bring a cache that is already over the new cap down now.
    pub fn set_max_bytes(&self, max_bytes: Option<u64>) {
        self.max_bytes
            .store(max_bytes.unwrap_or(NO_CAP), Ordering::Relaxed);
    }

    /// How many images this cache object has decoded and generated thumbnails for.
    /// A cache hit does not count.
    pub fn generations(&self) -> u64 {
        self.generations.load(Ordering::Relaxed)
    }

    fn entry_dir(&self, key: &ThumbKey) -> PathBuf {
        self.root.join(&key.as_str()[..2]).join(key.as_str())
    }

    /// Path of a cached thumbnail (it may not exist; see [`ThumbCache::has`]).
    pub fn path(&self, key: &ThumbKey, size: ThumbSize) -> PathBuf {
        self.entry_dir(key).join(size.file_name())
    }

    /// Whether the thumbnail of this size exists.
    pub fn has(&self, key: &ThumbKey, size: ThumbSize) -> bool {
        self.path(key, size).is_file()
    }

    /// The stored placeholder/palette data, or `None` if the entry is missing or unreadable.
    pub fn meta(&self, key: &ThumbKey) -> Option<ThumbMeta> {
        let bytes = fs::read(self.entry_dir(key).join(META_FILE)).ok()?;
        let meta: ThumbMeta = serde_json::from_slice(&bytes).ok()?;
        (meta.key == *key).then_some(meta)
    }

    /// Keeps the original file of an entry next to its thumbnails (`original.<ext>`), so a big
    /// view can show it at full size. It counts toward the cache size, goes when the entry is
    /// evicted or the cache is cleared, and never lands in a media folder. `ext` is `jpg`, `png`
    /// or `webp`; the entry's own folder is made when it is not there.
    pub fn store_original(
        &self,
        key: &ThumbKey,
        bytes: &[u8],
        ext: &str,
    ) -> Result<PathBuf, ThumbError> {
        if !matches!(ext, "jpg" | "png" | "webp") {
            return Err(ThumbError::Decode(format!("unknown picture type {ext:?}")));
        }
        let dir = self.entry_dir(key);
        fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        let path = dir.join(format!("{ORIGINAL_STEM}.{ext}"));
        write_atomic(&path, bytes)?;
        self.after_generation();
        Ok(path)
    }

    /// The stored original of an entry (see [`Self::store_original`]), if it has one.
    pub fn original_path(&self, key: &ThumbKey) -> Option<PathBuf> {
        ["jpg", "png", "webp"]
            .iter()
            .map(|ext| self.entry_dir(key).join(format!("{ORIGINAL_STEM}.{ext}")))
            .find(|path| path.is_file())
    }

    /// The WebP bytes of a cached thumbnail, or `None` if it is not cached.
    pub fn read(&self, key: &ThumbKey, size: ThumbSize) -> Result<Option<Vec<u8>>, ThumbError> {
        let path = self.path(key, size);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_err(&path)(e)),
        }
    }

    /// Make sure the thumbnails of `sizes` exist for the image file at `source`.
    ///
    /// The file is read and hashed; if the entry already has the meta data and all requested
    /// sizes, nothing is decoded. Otherwise the image is decoded once and the missing parts
    /// are generated.
    pub fn ensure(&self, source: &Path, sizes: &[ThumbSize]) -> Result<Ensured, ThumbError> {
        let size = fs::metadata(source).map_err(io_err(source))?.len();
        if size > MAX_SOURCE_BYTES {
            return Err(ThumbError::Decode(format!(
                "the image file is larger than {} MB",
                MAX_SOURCE_BYTES / (1024 * 1024)
            )));
        }
        let bytes = fs::read(source).map_err(io_err(source))?;
        self.ensure_bytes(&bytes, sizes)
    }

    /// Like [`ThumbCache::ensure`] for image bytes already in memory (for example a download).
    pub fn ensure_bytes(&self, bytes: &[u8], sizes: &[ThumbSize]) -> Result<Ensured, ThumbError> {
        let key = ThumbKey::of_bytes(bytes);
        let wanted: BTreeSet<ThumbSize> = sizes.iter().copied().collect();
        let existing = self.meta(&key);
        let missing: Vec<ThumbSize> = wanted
            .iter()
            .copied()
            .filter(|s| !self.has(&key, *s))
            .collect();

        if let (Some(meta), true) = (&existing, missing.is_empty()) {
            self.touch(&key);
            return Ok(Ensured {
                key,
                meta: meta.clone(),
                generated: Vec::new(),
            });
        }

        let generated = self.generate(&key, bytes, &missing, existing)?;
        Ok(generated)
    }

    fn generate(
        &self,
        key: &ThumbKey,
        bytes: &[u8],
        missing: &[ThumbSize],
        existing: Option<ThumbMeta>,
    ) -> Result<Ensured, ThumbError> {
        let decoded = decode(bytes)?;
        let (source_width, source_height) = (decoded.width(), decoded.height());
        let dir = self.entry_dir(key);
        fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        self.generations.fetch_add(1, Ordering::Relaxed);

        // Placeholder and palette come from the full image, so they do not depend on which sizes
        // happened to be requested first.
        let new_meta = existing
            .is_none()
            .then(|| build_meta(key, source_width, source_height, &decoded));

        // Largest first, so each smaller size is resized from the previous (already reduced) one.
        let mut order: Vec<ThumbSize> = missing.to_vec();
        order.sort_by_key(|s| std::cmp::Reverse(s.bounds().0 * s.bounds().1));
        let mut current = decoded;
        for size in &order {
            let (w, h) = size.fit(current.width(), current.height());
            if (w, h) != (current.width(), current.height()) {
                current = image::imageops::resize(&current, w, h, FilterType::Lanczos3);
            }
            let webp = encode_webp(&current, size.quality())?;
            write_atomic(&dir.join(size.file_name()), &webp)?;
        }

        let meta = match (existing, new_meta) {
            (Some(meta), _) => meta,
            (None, Some(meta)) => {
                let json = serde_json::to_vec_pretty(&meta)
                    .map_err(|e| ThumbError::Encode(e.to_string()))?;
                write_atomic(&dir.join(META_FILE), &json)?;
                meta
            }
            (None, None) => unreachable!("new meta is built whenever none exists"),
        };

        self.after_generation();
        Ok(Ensured {
            key: key.clone(),
            meta,
            generated: order,
        })
    }

    fn after_generation(&self) {
        if self.max_bytes().is_none() {
            return;
        }
        if self.since_prune.fetch_add(1, Ordering::Relaxed) + 1 >= AUTO_PRUNE_EVERY {
            self.since_prune.store(0, Ordering::Relaxed);
            if let Err(e) = self.enforce_cap() {
                tracing::warn!(error = %e, "thumbnail cache cleanup failed");
            }
        }
    }

    /// Refresh the last-use time of an entry (at most every ten minutes), so the size cap evicts
    /// the entries that were shown longest ago. Serving a thumbnail calls this.
    pub fn touch(&self, key: &ThumbKey) {
        let path = self.entry_dir(key).join(META_FILE);
        let Ok(modified) = fs::metadata(&path).and_then(|m| m.modified()) else {
            return;
        };
        if SystemTime::now()
            .duration_since(modified)
            .is_ok_and(|age| age < TOUCH_INTERVAL)
        {
            return;
        }
        if let Ok(file) = fs::OpenOptions::new().write(true).open(&path) {
            let _ = file.set_modified(SystemTime::now());
        }
    }

    /// Remove one entry. Returns whether it existed.
    pub fn remove(&self, key: &ThumbKey) -> Result<bool, ThumbError> {
        let dir = self.entry_dir(key);
        match fs::remove_dir_all(&dir) {
            Ok(()) => {
                let _ = fs::remove_dir(dir.parent().unwrap_or(&self.root));
                Ok(true)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io_err(&dir)(e)),
        }
    }

    fn entries(&self) -> Result<Vec<EntryInfo>, ThumbError> {
        let mut out = Vec::new();
        let shards = match fs::read_dir(&self.root) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(io_err(&self.root)(e)),
        };
        for shard in shards.flatten() {
            let name = shard.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.len() != 2 || !name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                continue;
            }
            if !shard.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Ok(children) = fs::read_dir(shard.path()) else {
                continue;
            };
            for child in children.flatten() {
                let child_name = child.file_name();
                let Some(child_name) = child_name.to_str() else {
                    continue;
                };
                let Ok(key) = ThumbKey::parse(child_name) else {
                    continue;
                };
                if !child_name.starts_with(name) || !child.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                out.push(entry_info(key, child.path()));
            }
        }
        Ok(out)
    }

    /// Entry count and total size of the cache.
    pub fn usage(&self) -> Result<CacheUsage, ThumbError> {
        let entries = self.entries()?;
        Ok(CacheUsage {
            entries: entries.len() as u64,
            bytes: entries.iter().map(|e| e.bytes).sum(),
        })
    }

    /// Evict least-recently-used entries until the cache is at most `limit` bytes.
    pub fn prune_to(&self, limit: u64) -> Result<PruneReport, ThumbError> {
        let mut entries = self.entries()?;
        let mut total: u64 = entries.iter().map(|e| e.bytes).sum();
        let mut report = PruneReport::default();
        entries.sort_by(|a, b| {
            a.last_used
                .cmp(&b.last_used)
                .then_with(|| a.key.cmp(&b.key))
        });
        let mut remaining = entries.len() as u64;
        for entry in entries {
            if total <= limit {
                break;
            }
            if self.remove(&entry.key)? {
                total = total.saturating_sub(entry.bytes);
                report.removed_entries += 1;
                report.freed_bytes += entry.bytes;
            }
            remaining -= 1;
        }
        report.remaining_entries = remaining;
        report.remaining_bytes = total;
        Ok(report)
    }

    /// Bring the cache back under its cap, if it has one and is over it. Evicts down to 90%
    /// of the cap so the next generations do not immediately trigger another cleanup.
    pub fn enforce_cap(&self) -> Result<PruneReport, ThumbError> {
        let Some(cap) = self.max_bytes() else {
            let usage = self.usage()?;
            return Ok(PruneReport {
                remaining_entries: usage.entries,
                remaining_bytes: usage.bytes,
                ..PruneReport::default()
            });
        };
        let usage = self.usage()?;
        if usage.bytes <= cap {
            return Ok(PruneReport {
                remaining_entries: usage.entries,
                remaining_bytes: usage.bytes,
                ..PruneReport::default()
            });
        }
        self.prune_to(cap - cap / 10)
    }

    /// Delete every cache entry ("Clear image cache"). Files in the folder that are not cache
    /// entries are left alone.
    pub fn clear(&self) -> Result<PruneReport, ThumbError> {
        self.prune_to(0)
    }
}

fn entry_info(key: ThumbKey, dir: PathBuf) -> EntryInfo {
    let mut bytes = 0;
    let mut newest = SystemTime::UNIX_EPOCH;
    let mut meta_time = None;
    if let Ok(files) = fs::read_dir(&dir) {
        for file in files.flatten() {
            let Ok(md) = file.metadata() else { continue };
            if !md.is_file() {
                continue;
            }
            let modified = md.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let is_temp = file.file_name().to_string_lossy().starts_with(".tmp-");
            if is_temp
                && SystemTime::now()
                    .duration_since(modified)
                    .is_ok_and(|a| a > STALE_TEMP)
            {
                let _ = fs::remove_file(file.path());
                continue;
            }
            bytes += md.len();
            newest = newest.max(modified);
            if file.file_name() == META_FILE {
                meta_time = Some(modified);
            }
        }
    }
    EntryInfo {
        key,
        bytes,
        last_used: meta_time.unwrap_or(newest),
    }
}

/// The largest image file a thumbnail is made from. Artwork downloads stop at 40 MB; a larger
/// file next to a video is not artwork.
pub const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;

/// The limits every artwork decode runs under: at most 16384 pixels a side and 256 MB of
/// decoder memory, so a crafted image (a "PNG bomb") fails instead of exhausting the memory.
/// 16384 × 16384 RGBA is 1 GiB, so the allocation limit is what bounds a large picture.
pub fn decode_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    limits
}

/// Whether a `width × height` picture fits the decode limits once expanded to RGBA, which the
/// thumbnail and resize steps do outside the decoder's own accounting.
pub fn fits_decode_limits(width: u32, height: u32) -> bool {
    let limits = decode_limits();
    let pixels = u64::from(width) * u64::from(height);
    limits.max_image_width.is_none_or(|max| width <= max)
        && limits.max_image_height.is_none_or(|max| height <= max)
        && limits
            .max_alloc
            .is_none_or(|max| pixels.saturating_mul(4) <= max)
}

fn decode(bytes: &[u8]) -> Result<RgbaImage, ThumbError> {
    let reader = || {
        ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| ThumbError::Decode(e.to_string()))
    };
    let (width, height) = reader()?
        .into_dimensions()
        .map_err(|e| ThumbError::Decode(e.to_string()))?;
    if !fits_decode_limits(width, height) {
        return Err(ThumbError::Decode(format!(
            "the image is too large to show ({width} × {height})"
        )));
    }
    let mut reader = reader()?;
    reader.limits(decode_limits());
    let image: DynamicImage = reader
        .decode()
        .map_err(|e| ThumbError::Decode(e.to_string()))?;
    if image.width() == 0 || image.height() == 0 {
        return Err(ThumbError::Decode("image has no pixels".into()));
    }
    Ok(image.into_rgba8())
}

fn encode_webp(image: &RgbaImage, quality: f32) -> Result<Vec<u8>, ThumbError> {
    let (w, h) = (image.width(), image.height());
    let opaque = image.pixels().all(|p| p.0[3] == 255);
    let memory = if opaque {
        let rgb: Vec<u8> = image
            .pixels()
            .flat_map(|p| [p.0[0], p.0[1], p.0[2]])
            .collect();
        webp::Encoder::from_rgb(&rgb, w, h).encode(quality)
    } else {
        webp::Encoder::from_rgba(image.as_raw(), w, h).encode(quality)
    };
    if memory.is_empty() {
        return Err(ThumbError::Encode("WebP encoder returned no data".into()));
    }
    Ok(memory.to_vec())
}

fn build_meta(
    key: &ThumbKey,
    source_width: u32,
    source_height: u32,
    full: &RgbaImage,
) -> ThumbMeta {
    let longest = full.width().max(full.height());
    let small = if longest > PLACEHOLDER_SIDE {
        let scale = f64::from(PLACEHOLDER_SIDE) / f64::from(longest);
        let w = ((f64::from(full.width()) * scale).round() as u32).max(1);
        let h = ((f64::from(full.height()) * scale).round() as u32).max(1);
        image::imageops::resize(full, w, h, FilterType::Triangle)
    } else {
        full.clone()
    };
    let hash = thumbhash::rgba_to_thumb_hash(
        small.width() as usize,
        small.height() as usize,
        small.as_raw(),
    );
    ThumbMeta {
        key: key.clone(),
        source_width,
        source_height,
        thumbhash: hex(&hash),
        palette: extract_palette(small.as_raw()),
        created_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    }
}

/// Write `data` to `path` through a temp file in the same folder, so readers never see a partial file.
fn write_atomic(path: &Path, data: &[u8]) -> Result<(), ThumbError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let temp = dir.join(format!(
        ".tmp-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temp, data).map_err(io_err(&temp))?;
    // On Windows a virus scanner or the search indexer can hold a new file open for a moment, and
    // the rename then fails with a sharing violation or "access denied". The same rename a few
    // milliseconds later works, so try a few times before giving up.
    let mut attempt = 0_u32;
    loop {
        match fs::rename(&temp, path) {
            Ok(()) => return Ok(()),
            Err(_) if attempt < RENAME_RETRIES => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(15 * u64::from(attempt)));
            }
            Err(e) => {
                let _ = fs::remove_file(&temp);
                return Err(io_err(path)(e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
    use image::{ExtendedColorType, ImageEncoder, Rgba};

    /// A poster-like test image: dark navy with a bright orange block.
    fn poster(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            if x > width / 4 && x < width * 3 / 4 && y > height / 4 && y < height / 2 {
                Rgba([240, 140, 20, 255])
            } else {
                Rgba([15, 25, 70, 255])
            }
        })
    }

    fn jpeg_bytes(img: &RgbaImage) -> Vec<u8> {
        let rgb: Vec<u8> = img
            .pixels()
            .flat_map(|p| [p.0[0], p.0[1], p.0[2]])
            .collect();
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, 90)
            .write_image(&rgb, img.width(), img.height(), ExtendedColorType::Rgb8)
            .unwrap();
        out
    }

    fn png_bytes(img: &RgbaImage) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn cache(dir: &Path, cap: Option<u64>) -> ThumbCache {
        ThumbCache::open(dir, cap).unwrap()
    }

    fn webp_dims(bytes: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory_with_format(bytes, image::ImageFormat::WebP).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn sizes_fit_bounds_keep_aspect_and_never_upscale() {
        assert_eq!(ThumbSize::Grid.fit(2000, 3000), (360, 540));
        assert_eq!(ThumbSize::List.fit(2000, 3000), (120, 180));
        assert_eq!(ThumbSize::Hero.fit(3840, 2160), (1600, 900));
        assert_eq!(ThumbSize::Grid.fit(1000, 500), (360, 180));
        assert_eq!(ThumbSize::Grid.fit(100, 150), (100, 150));
        assert_eq!(ThumbSize::Grid.fit(1, 5000), (1, 540));
        assert_eq!(ThumbSize::parse("hero"), Some(ThumbSize::Hero));
        assert_eq!(ThumbSize::parse("huge"), None);
    }

    #[test]
    fn generates_webp_at_all_three_sizes() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let src = tmp.path().join("poster.jpg");
        fs::write(&src, jpeg_bytes(&poster(1000, 1500))).unwrap();

        let out = cache.ensure(&src, &ThumbSize::ALL).unwrap();
        assert_eq!(out.generated.len(), 3);
        assert!(!out.reused());
        // Hero is bounded by height 900: 1000x1500 -> 600x900; grid 360x540; list 120x180.
        assert_eq!(
            webp_dims(&cache.read(&out.key, ThumbSize::Hero).unwrap().unwrap()),
            (600, 900)
        );
        assert_eq!(
            webp_dims(&cache.read(&out.key, ThumbSize::Grid).unwrap().unwrap()),
            (360, 540)
        );
        assert_eq!(
            webp_dims(&cache.read(&out.key, ThumbSize::List).unwrap().unwrap()),
            (120, 180)
        );
        let bytes = cache.read(&out.key, ThumbSize::Grid).unwrap().unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let out = cache
            .ensure_bytes(&png_bytes(&poster(80, 120)), &ThumbSize::ALL)
            .unwrap();
        for size in ThumbSize::ALL {
            assert_eq!(
                webp_dims(&cache.read(&out.key, size).unwrap().unwrap()),
                (80, 120)
            );
        }
    }

    #[test]
    fn meta_has_thumbhash_and_palette() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let out = cache
            .ensure_bytes(&png_bytes(&poster(400, 600)), &[ThumbSize::Grid])
            .unwrap();
        let meta = cache.meta(&out.key).unwrap();
        assert_eq!(meta, out.meta);
        assert_eq!((meta.source_width, meta.source_height), (400, 600));
        let hash = meta.thumbhash_bytes();
        assert!(
            !hash.is_empty() && hash.len() < 40,
            "thumbhash is tiny: {} bytes",
            hash.len()
        );
        let (w, h, rgba) = thumbhash::thumb_hash_to_rgba(&hash).unwrap();
        assert_eq!(rgba.len(), w * h * 4);
        let palette = meta.palette.unwrap();
        assert_eq!(palette.dominant, Rgb([15, 25, 70]));
        assert_eq!(palette.vibrant, Some(Rgb([240, 140, 20])));
        assert_eq!(palette.ambient, Rgb([240, 140, 20]));
    }

    #[test]
    fn renamed_file_reuses_cache_entries_without_regeneration() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(&tmp.path().join("cache"), None);
        let old_dir = tmp.path().join("media").join("Heat (1995)");
        fs::create_dir_all(&old_dir).unwrap();
        let old = old_dir.join("Heat (1995)-poster.jpg");
        fs::write(&old, jpeg_bytes(&poster(800, 1200))).unwrap();

        let first = cache.ensure(&old, &ThumbSize::ALL).unwrap();
        assert_eq!(first.generated.len(), 3);
        assert_eq!(cache.generations(), 1);
        let before: Vec<(Vec<u8>, SystemTime)> = ThumbSize::ALL
            .iter()
            .map(|s| {
                let p = cache.path(&first.key, *s);
                (
                    fs::read(&p).unwrap(),
                    fs::metadata(&p).unwrap().modified().unwrap(),
                )
            })
            .collect();

        // Rename the folder and the file, as the Organize step does.
        let new_dir = tmp.path().join("media").join("Heat 1995 [1080p]");
        fs::rename(&old_dir, &new_dir).unwrap();
        let new = new_dir.join("Heat 1995 [1080p]-poster.jpg");
        fs::rename(new_dir.join("Heat (1995)-poster.jpg"), &new).unwrap();

        let second = cache.ensure(&new, &ThumbSize::ALL).unwrap();
        assert_eq!(second.key, first.key, "key follows content, not path");
        assert!(second.reused(), "nothing regenerated after the rename");
        assert_eq!(cache.generations(), 1, "no decode happened");
        assert_eq!(second.meta, first.meta);
        for (size, (bytes, modified)) in ThumbSize::ALL.iter().zip(before) {
            let p = cache.path(&second.key, *size);
            assert_eq!(fs::read(&p).unwrap(), bytes);
            assert_eq!(fs::metadata(&p).unwrap().modified().unwrap(), modified);
        }
        assert_eq!(cache.usage().unwrap().entries, 1);

        // A copy elsewhere with identical bytes shares the entry too.
        let copy = tmp.path().join("elsewhere.jpg");
        fs::copy(&new, &copy).unwrap();
        assert!(cache.ensure(&copy, &ThumbSize::ALL).unwrap().reused());
        assert_eq!(cache.generations(), 1);
    }

    #[test]
    fn changed_content_gets_a_new_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let a = cache
            .ensure_bytes(&png_bytes(&poster(200, 300)), &[ThumbSize::List])
            .unwrap();
        let mut other = poster(200, 300);
        other.put_pixel(0, 0, Rgba([1, 2, 3, 255]));
        let b = cache
            .ensure_bytes(&png_bytes(&other), &[ThumbSize::List])
            .unwrap();
        assert_ne!(a.key, b.key);
        assert_eq!(cache.generations(), 2);
        assert_eq!(cache.usage().unwrap().entries, 2);
    }

    #[test]
    fn sizes_are_generated_lazily_and_only_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let bytes = png_bytes(&poster(900, 1350));
        let first = cache.ensure_bytes(&bytes, &[ThumbSize::Grid]).unwrap();
        assert_eq!(first.generated, vec![ThumbSize::Grid]);
        assert!(!cache.has(&first.key, ThumbSize::Hero));

        let second = cache
            .ensure_bytes(&bytes, &[ThumbSize::Grid, ThumbSize::Hero])
            .unwrap();
        assert_eq!(second.generated, vec![ThumbSize::Hero]);
        assert_eq!(second.meta, first.meta, "meta is not recomputed");
        assert!(
            cache
                .ensure_bytes(&bytes, &ThumbSize::ALL)
                .unwrap()
                .generated
                == vec![ThumbSize::List]
        );
        assert!(
            cache
                .ensure_bytes(&bytes, &ThumbSize::ALL)
                .unwrap()
                .reused()
        );
    }

    #[test]
    fn transparency_is_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let logo = RgbaImage::from_fn(300, 100, |x, _| {
            if x < 150 {
                Rgba([250, 250, 250, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let out = cache
            .ensure_bytes(&png_bytes(&logo), &[ThumbSize::Grid])
            .unwrap();
        let webp = cache.read(&out.key, ThumbSize::Grid).unwrap().unwrap();
        let decoded = image::load_from_memory_with_format(&webp, image::ImageFormat::WebP)
            .unwrap()
            .into_rgba8();
        assert!(decoded.get_pixel(10, 50).0[3] > 200);
        assert!(decoded.get_pixel(290, 50).0[3] < 50);
    }

    #[test]
    fn undecodable_source_is_an_error_and_leaves_no_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let err = cache
            .ensure_bytes(b"this is not an image", &ThumbSize::ALL)
            .unwrap_err();
        assert!(matches!(err, ThumbError::Decode(_)), "{err}");
        assert_eq!(cache.usage().unwrap(), CacheUsage::default());
        let missing = cache
            .ensure(&tmp.path().join("nope.jpg"), &ThumbSize::ALL)
            .unwrap_err();
        assert!(matches!(missing, ThumbError::Io { .. }));
    }

    #[test]
    fn keys_reject_path_tricks() {
        assert!(ThumbKey::parse("../../etc/passwd").is_err());
        assert!(ThumbKey::parse("ABCDEF0123456789ABCDEF0123456789").is_err());
        assert!(ThumbKey::parse("abcdef0123456789abcdef012345678").is_err());
        assert!(ThumbKey::parse("abcdef0123456789abcdef0123456789").is_ok());
    }

    fn set_last_used(cache: &ThumbCache, key: &ThumbKey, age_secs: u64) {
        let file = fs::OpenOptions::new()
            .write(true)
            .open(cache.entry_dir(key).join(META_FILE))
            .unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(age_secs))
            .unwrap();
    }

    fn distinct(n: u8) -> Vec<u8> {
        let img = RgbaImage::from_fn(300, 450, |x, y| {
            Rgba([(x % 251) as u8, (y % 241) as u8, n, 255])
        });
        png_bytes(&img)
    }

    #[test]
    fn prune_evicts_least_recently_used_first() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let keys: Vec<ThumbKey> = (0..4)
            .map(|n| {
                cache
                    .ensure_bytes(&distinct(n), &ThumbSize::ALL)
                    .unwrap()
                    .key
            })
            .collect();
        // keys[1] is the oldest, then keys[3], keys[0], keys[2] the newest.
        for (key, age) in [
            (&keys[1], 4000),
            (&keys[3], 3000),
            (&keys[0], 2000),
            (&keys[2], 1000),
        ] {
            set_last_used(&cache, key, age);
        }
        let usage = cache.usage().unwrap();
        assert_eq!(usage.entries, 4);
        let per_entry = usage.bytes / 4;

        let report = cache.prune_to(usage.bytes - per_entry).unwrap();
        assert!(report.removed_entries >= 1);
        assert!(report.remaining_bytes <= usage.bytes - per_entry);
        assert_eq!(report.remaining_entries + report.removed_entries, 4);
        assert!(cache.meta(&keys[1]).is_none(), "oldest entry evicted");
        assert!(cache.meta(&keys[2]).is_some(), "newest entry kept");
        assert_eq!(cache.usage().unwrap().bytes, report.remaining_bytes);
    }

    #[test]
    fn the_cap_can_change_while_the_cache_is_open_and_clear_empties_it() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        for n in 0..4 {
            cache.ensure_bytes(&distinct(n), &ThumbSize::ALL).unwrap();
        }
        let usage = cache.usage().unwrap();
        assert_eq!(cache.max_bytes(), None);
        assert_eq!(cache.enforce_cap().unwrap().removed_entries, 0);

        cache.set_max_bytes(Some(usage.bytes / 2));
        assert_eq!(cache.max_bytes(), Some(usage.bytes / 2));
        let report = cache.enforce_cap().unwrap();
        assert!(report.removed_entries >= 1);
        assert!(cache.usage().unwrap().bytes <= usage.bytes / 2);

        let cleared = cache.clear().unwrap();
        assert!(cleared.removed_entries >= 1);
        assert_eq!(cache.usage().unwrap(), CacheUsage::default());
    }

    #[test]
    fn hits_refresh_last_use() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let old = distinct(1);
        let a = cache.ensure_bytes(&old, &[ThumbSize::List]).unwrap().key;
        let b = cache
            .ensure_bytes(&distinct(2), &[ThumbSize::List])
            .unwrap()
            .key;
        set_last_used(&cache, &a, 7200);
        set_last_used(&cache, &b, 3600);
        // Using `a` again makes it the most recent.
        assert!(
            cache
                .ensure_bytes(&old, &[ThumbSize::List])
                .unwrap()
                .reused()
        );
        let total = cache.usage().unwrap().bytes;
        cache.prune_to(total - 1).unwrap();
        assert!(cache.meta(&a).is_some());
        assert!(cache.meta(&b).is_none());
    }

    #[test]
    fn cap_is_enforced_with_headroom() {
        let tmp = tempfile::tempdir().unwrap();
        let unlimited = cache(tmp.path(), None);
        for n in 0..6 {
            unlimited
                .ensure_bytes(&distinct(n), &ThumbSize::ALL)
                .unwrap();
        }
        let usage = unlimited.usage().unwrap();
        assert_eq!(usage.entries, 6);

        let capped = ThumbCache::open(tmp.path(), Some(usage.bytes / 2)).unwrap();
        let report = capped.enforce_cap().unwrap();
        assert!(report.removed_entries > 0);
        let after = capped.usage().unwrap();
        assert!(
            after.bytes <= usage.bytes / 2 - usage.bytes / 20,
            "{after:?}"
        );
        // A cache that is under its cap is left alone.
        assert_eq!(capped.enforce_cap().unwrap().removed_entries, 0);
        // And no cap means no eviction.
        assert_eq!(unlimited.enforce_cap().unwrap().removed_entries, 0);
    }

    #[test]
    fn clear_removes_only_cache_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        for n in 0..3 {
            cache.ensure_bytes(&distinct(n), &ThumbSize::ALL).unwrap();
        }
        // Things that are not cache entries must survive.
        let foreign_file = cache.root().join("readme.txt");
        fs::write(&foreign_file, "keep").unwrap();
        let foreign_dir = cache.root().join("zz");
        fs::create_dir_all(&foreign_dir).unwrap();
        fs::write(foreign_dir.join("x"), "keep").unwrap();
        let media = tmp.path().join("movie.mkv");
        fs::write(&media, "video").unwrap();

        let report = cache.clear().unwrap();
        assert_eq!(report.removed_entries, 3);
        assert_eq!(report.remaining_entries, 0);
        assert_eq!(cache.usage().unwrap(), CacheUsage::default());
        assert!(foreign_file.is_file() && foreign_dir.join("x").is_file() && media.is_file());
    }

    #[test]
    fn stale_temp_files_are_cleaned_and_fresh_ones_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let key = cache
            .ensure_bytes(&distinct(9), &[ThumbSize::List])
            .unwrap()
            .key;
        let dir = cache.entry_dir(&key);
        let stale = dir.join(".tmp-1-1");
        fs::write(&stale, "partial").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(2 * 3600))
            .unwrap();
        cache.usage().unwrap();
        assert!(!stale.exists());
    }

    #[test]
    fn corrupt_meta_is_rebuilt() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let bytes = distinct(5);
        let first = cache.ensure_bytes(&bytes, &[ThumbSize::List]).unwrap();
        fs::write(cache.entry_dir(&first.key).join(META_FILE), "{ not json").unwrap();
        assert!(cache.meta(&first.key).is_none());
        let again = cache.ensure_bytes(&bytes, &[ThumbSize::List]).unwrap();
        assert_eq!(again.meta.thumbhash, first.meta.thumbhash);
        assert_eq!(again.meta.palette, first.meta.palette);
        assert!(cache.meta(&first.key).is_some());
    }

    #[test]
    fn remove_deletes_one_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = cache(tmp.path(), None);
        let key = cache
            .ensure_bytes(&distinct(3), &[ThumbSize::List])
            .unwrap()
            .key;
        assert!(cache.remove(&key).unwrap());
        assert!(!cache.remove(&key).unwrap());
        assert_eq!(cache.usage().unwrap().entries, 0);
    }

    /// A PNG whose header claims `width × height` pixels, with no pixel data behind it.
    fn png_header_only(width: u32, height: u32) -> Vec<u8> {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = 0xFFFF_FFFF_u32;
            for byte in bytes {
                crc ^= u32::from(*byte);
                for _ in 0..8 {
                    crc = if crc & 1 == 1 {
                        (crc >> 1) ^ 0xEDB8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }
        let mut ihdr = b"IHDR".to_vec();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        out.extend_from_slice(&13_u32.to_be_bytes());
        out.extend_from_slice(&ihdr);
        out.extend_from_slice(&crc32(&ihdr).to_be_bytes());
        out
    }

    #[test]
    fn a_picture_past_the_decode_limits_is_refused_before_decoding() {
        assert!(fits_decode_limits(7680, 4320));
        assert!(!fits_decode_limits(20_000, 100));
        assert!(!fits_decode_limits(16_000, 16_000));
        // A real picture wider than the limit is refused by the check, not by the decoder.
        let wide = png_bytes(&poster(20_000, 1));
        assert!(
            matches!(decode(&wide), Err(ThumbError::Decode(message)) if message.contains("too large"))
        );
        // A header that promises 60000 × 60000 pixels never gets as far as a decode.
        let bomb = png_header_only(60_000, 60_000);
        assert!(matches!(decode(&bomb), Err(ThumbError::Decode(_))));
    }

    #[test]
    fn a_source_file_past_the_size_cap_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ThumbCache::open(&dir.path().join("cache"), None).unwrap();
        let path = dir.path().join("huge.png");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_SOURCE_BYTES + 1).unwrap();
        drop(file);
        assert!(matches!(
            cache.ensure(&path, &[ThumbSize::Grid]),
            Err(ThumbError::Decode(_))
        ));
    }

    #[test]
    fn an_original_is_kept_in_its_entry_and_leaves_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(&dir.path().join("cache"), None);
        let bytes = png_bytes(&poster(300, 450));
        let key = cache.ensure_bytes(&bytes, &[ThumbSize::Grid]).unwrap().key;
        assert_eq!(cache.original_path(&key), None);

        let path = cache.store_original(&key, &bytes, "png").unwrap();

        assert_eq!(cache.original_path(&key), Some(path.clone()));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(path.starts_with(cache.root()), "inside the cache folder");
        assert!(cache.usage().unwrap().bytes >= bytes.len() as u64);
        assert!(matches!(
            cache.store_original(&key, &bytes, "exe"),
            Err(ThumbError::Decode(_))
        ));
        cache.remove(&key).unwrap();
        assert_eq!(cache.original_path(&key), None);
    }
}

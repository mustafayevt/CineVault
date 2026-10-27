//! The `cv://img/<key>/<size>` image protocol, without any windowing framework.
//!
//! The desktop shell registers a custom URI scheme named `cv` and passes each request here as a
//! method, a path and an `If-None-Match` header. The answer is a status, headers and a body, so
//! the whole protocol is testable without a window. Images never travel over IPC: the UI puts the
//! URL in `<img src>` and the web view fetches, caches and decodes it natively.
//!
//! # URL
//!
//! `cv://img/<key>/<size>`, where `<key>` is a [`ThumbKey`] (32 lower-case hex characters) and
//! `<size>` is `grid`, `list`, `hero` or `full`. WebView2 and Android serve custom schemes as
//! `http://cv.localhost/...`, other platforms as `cv://localhost/...`, and the shell helper puts the
//! whole `img/<key>/<size>` in one percent-encoded path segment; [`parse_path`] accepts both forms.
//!
//! # Answers
//!
//! | Case | Status | Notes |
//! |------|--------|-------|
//! | cached or lazily generated | 200 | `image/webp`, long-lived `Cache-Control`, strong `ETag` |
//! | `full` with a readable original | 200 | the file as it is (`image/jpeg`, `image/png` or `image/webp`) |
//! | another size stands in (see below) | 200 | `image/webp`, `no-store`, no `ETag` |
//! | `If-None-Match` matches a cached thumbnail | 304 | no body |
//! | malformed path, bad key, bad size, unknown key, missing source | 404 | `no-store`, empty body |
//! | any method but `GET` and `HEAD` | 405 | `Allow: GET, HEAD` |
//! | the source cannot be decoded or the cache cannot be written | 500 | `no-store`, no details |
//!
//! Thumbnails are immutable for their key (the key is the hash of the image bytes), so the
//! `Cache-Control` is `public, max-age=31536000, immutable` and the web view never revalidates.
//! Errors are `no-store` because an unknown key may become known a moment later.
//!
//! # Lazy generation
//!
//! A thumbnail that is not in the [`ThumbCache`] is generated on the first request: the key is
//! looked up in a [`SourceResolver`] to find the original image file, which is decoded once and
//! thumbnailed ([`ThumbCache::ensure`]). If the file's content no longer hashes to the requested
//! key (the image was replaced) the answer is 404 and the stale mapping is forgotten. Callers do the
//! work on a blocking thread; [`ImageService::handle`] does file I/O and decoding.
//!
//! # Stand-ins
//!
//! A size that cannot be made (an artwork candidate is cached from its download at the grid size,
//! with no file behind it yet) is answered with the largest size the cache has, and `full` without
//! a readable original with the hero size. Such an answer is `no-store`: kept under the asked URL,
//! it would go on showing the small picture once the chosen image is written and its real size
//! can be made.
//!
//! # The original (`full`)
//!
//! `full` answers with the image file itself, byte for byte, for the large displays: a backdrop
//! that fills a 2560-pixel window at twice the pixel density needs more than any thumbnail
//! holds, and the original costs no decode or encode. The file is read and hashed on each
//! request (the web view keeps the answer, which never changes for the key); when it no longer
//! matches its key, or its folder is offline, or the picture is too large to show, the hero
//! thumbnail answers instead (or the largest cached one).
//!
//! # Safety
//!
//! Only a [`ThumbKey`] and a [`ThumbSize`] ever reach the file system, and both parse strictly
//! (hex digits and three fixed words), so a request cannot name a path: `..`, separators, drive
//! letters and encoded variants all fail to parse and answer 404.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::thumb::{ThumbCache, ThumbError, ThumbKey, ThumbSize};

/// The URI scheme the shell registers.
pub const SCHEME: &str = "cv";

/// First path segment of every image URL.
pub const PATH_PREFIX: &str = "img";

/// `Cache-Control` of a thumbnail: one year, never revalidated.
pub const CACHE_CONTROL_IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// Finds the original image file behind a key, for lazy generation.
pub trait SourceResolver: Send + Sync {
    /// The image file whose content hashes to `key`, if known.
    fn source_of(&self, key: &ThumbKey) -> Option<PathBuf>;

    /// Forget a mapping whose file no longer matches its key.
    fn forget(&self, _key: &ThumbKey) {}
}

/// An in-memory [`SourceResolver`]. Callers that learn an image's key and path (a scan, a download,
/// a database row) register them here; the library database will implement the trait itself later.
#[derive(Debug, Default)]
pub struct SourceMap {
    map: RwLock<HashMap<ThumbKey, PathBuf>>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record where the image with this key lives.
    pub fn register(&self, key: ThumbKey, path: PathBuf) {
        if let Ok(mut map) = self.map.write() {
            map.insert(key, path);
        }
    }

    /// Hash the file at `path`, record it, and return its key. Reads the whole file.
    pub fn register_file(&self, path: &Path) -> Result<ThumbKey, ThumbError> {
        let bytes = std::fs::read(path).map_err(|source| ThumbError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let key = ThumbKey::of_bytes(&bytes);
        self.register(key.clone(), path.to_path_buf());
        Ok(key)
    }

    pub fn len(&self) -> usize {
        self.map.read().map_or(0, |m| m.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SourceResolver for SourceMap {
    fn source_of(&self, key: &ThumbKey) -> Option<PathBuf> {
        self.map.read().ok()?.get(key).cloned()
    }

    fn forget(&self, key: &ThumbKey) {
        if let Ok(mut map) = self.map.write() {
            map.remove(key);
        }
    }
}

/// A request to the protocol, reduced to what the handler uses.
#[derive(Clone, Copy, Debug)]
pub struct ImageRequest<'a> {
    /// HTTP method, for example `GET`.
    pub method: &'a str,
    /// The URI path as received (percent-encoded, with or without the leading slash).
    pub path: &'a str,
    /// The `If-None-Match` header value, if any.
    pub if_none_match: Option<&'a str>,
}

/// The answer: status, headers and body, ready to convert to an HTTP response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageResponse {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl ImageResponse {
    /// The value of a header (names are matched case-insensitively).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn error(status: u16) -> Self {
        let mut headers = common_headers();
        headers.push(("cache-control", "no-store".to_owned()));
        if status == 405 {
            headers.push(("allow", "GET, HEAD".to_owned()));
        }
        Self {
            status,
            headers,
            body: Vec::new(),
        }
    }
}

fn common_headers() -> Vec<(&'static str, String)> {
    vec![
        ("x-content-type-options", "nosniff".to_owned()),
        // The page origin differs from the protocol's, and the UI may read pixels (placeholders).
        ("access-control-allow-origin", "*".to_owned()),
        ("cross-origin-resource-policy", "cross-origin".to_owned()),
    ]
}

/// The strong ETag of a thumbnail: its key and size, which fully determine the content.
pub fn etag(key: &ThumbKey, size: ThumbSize) -> String {
    format!("\"{}-{}\"", key, size.as_str())
}

/// The size word of the original image in a URL: `cv://img/<key>/full`.
pub const FULL: &str = "full";

/// The ETag of an original image.
fn full_etag(key: &ThumbKey) -> String {
    format!("\"{key}-{FULL}\"")
}

/// What a request asks for: a thumbnail size, or the original file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wanted {
    Thumb(ThumbSize),
    Full,
}

/// Whether an `If-None-Match` header value matches `etag` (weak comparison; `*` matches any).
pub fn if_none_match_hits(header: &str, etag: &str) -> bool {
    header.split(',').map(str::trim).any(|candidate| {
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
    })
}

/// Parse a request path into the key and size it names. Accepts `/img/<key>/<size>`,
/// `img/<key>/<size>` and the single percent-encoded segment `img%2F<key>%2F<size>`; a query
/// string or fragment is ignored. Anything else is `None`.
pub fn parse_path(path: &str) -> Option<(ThumbKey, ThumbSize)> {
    match parse_request_path(path)? {
        (key, Wanted::Thumb(size)) => Some((key, size)),
        (_, Wanted::Full) => None,
    }
}

/// [`parse_path`], which also accepts the size word [`FULL`] for the original image.
pub fn parse_request_path(path: &str) -> Option<(ThumbKey, Wanted)> {
    let path = path.split(['?', '#']).next().unwrap_or("");
    let decoded = percent_decode(path.strip_prefix('/').unwrap_or(path))?;
    let mut parts = decoded.split('/');
    let (prefix, key, size) = (parts.next()?, parts.next()?, parts.next()?);
    if prefix != PATH_PREFIX || parts.next().is_some() {
        return None;
    }
    let wanted = if size == FULL {
        Wanted::Full
    } else {
        Wanted::Thumb(ThumbSize::parse(size)?)
    };
    Some((ThumbKey::parse(key).ok()?, wanted))
}

/// Decode `%XX` escapes; `None` for a malformed escape or a result that is not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = hex_value(*bytes.get(i + 1)?)?;
            let lo = hex_value(*bytes.get(i + 2)?)?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// At most `max` thumbnails are made at once. Making one decodes the whole original (a 4K
/// fanart is about 30 MB of pixels), and the web view asks for every card of a fast scroll at
/// the same time; the rest wait their turn instead of decoding side by side.
#[derive(Debug)]
pub struct DecodeGate {
    busy: std::sync::Mutex<usize>,
    freed: std::sync::Condvar,
    max: usize,
}

/// A turn at making thumbnails; the next waiting request goes when it is dropped.
pub struct DecodeTurn<'a> {
    gate: &'a DecodeGate,
}

impl DecodeGate {
    pub fn new(max: usize) -> Self {
        Self {
            busy: std::sync::Mutex::new(0),
            freed: std::sync::Condvar::new(),
            max: max.max(1),
        }
    }

    /// One turn per core, at most eight.
    pub fn per_core() -> Self {
        Self::new(std::thread::available_parallelism().map_or(4, |n| n.get().min(8)))
    }

    /// Waits for a turn.
    pub fn enter(&self) -> DecodeTurn<'_> {
        let mut busy = self
            .busy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *busy >= self.max {
            busy = self
                .freed
                .wait(busy)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *busy += 1;
        DecodeTurn { gate: self }
    }
}

impl Drop for DecodeTurn<'_> {
    fn drop(&mut self) {
        let mut busy = self
            .gate
            .busy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *busy = busy.saturating_sub(1);
        self.gate.freed.notify_one();
    }
}

/// Serves thumbnails from the cache and generates missing ones on first request.
#[derive(Clone)]
pub struct ImageService {
    cache: Arc<ThumbCache>,
    sources: Arc<dyn SourceResolver>,
    decodes: Arc<DecodeGate>,
}

impl std::fmt::Debug for ImageService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageService")
            .field("cache", &self.cache.root())
            .finish_non_exhaustive()
    }
}

impl ImageService {
    pub fn new(cache: Arc<ThumbCache>, sources: Arc<dyn SourceResolver>) -> Self {
        Self {
            cache,
            sources,
            decodes: Arc::new(DecodeGate::per_core()),
        }
    }

    pub fn cache(&self) -> &Arc<ThumbCache> {
        &self.cache
    }

    /// Answer one request. Blocking: may read, decode and encode an image, so call it from a
    /// blocking thread.
    pub fn handle(&self, request: &ImageRequest<'_>) -> ImageResponse {
        let head = request.method.eq_ignore_ascii_case("HEAD");
        if !head && !request.method.eq_ignore_ascii_case("GET") {
            return ImageResponse::error(405);
        }
        let Some((key, wanted)) = parse_request_path(request.path) else {
            return ImageResponse::error(404);
        };
        let (wanted, full) = match wanted {
            Wanted::Thumb(size) => (size, false),
            Wanted::Full => match self.original(&key, request, head) {
                Some(answer) => return answer,
                // No readable original: the largest thumbnail there is, or can be made.
                None => (ThumbSize::Hero, true),
            },
        };
        // A size that cannot be made (an artwork candidate is cached from its download, with no
        // file to make another size from) is answered with a size the cache has, rather than an
        // error and a blurred placeholder.
        let size = if self.cache.has(&key, wanted) || self.sources.source_of(&key).is_some() {
            wanted
        } else {
            ThumbSize::ALL
                .into_iter()
                .rev()
                .find(|other| self.cache.has(&key, *other))
                .unwrap_or(wanted)
        };
        let tag = etag(&key, size);
        // An answer that stands in for what was asked must not be kept under the asked URL:
        // the web view would show that small picture for the key long after the real size can
        // be made (a chosen candidate is cached at the grid size before its file is written).
        let stand_in = full || size != wanted;

        // A cached thumbnail never changes for its key, so a matching validator needs no body.
        if stand_in {
            if !self.cache.has(&key, size)
                && let Err(status) = self.generate(&key, size)
            {
                return ImageResponse::error(status);
            }
        } else if self.cache.has(&key, size) {
            if request
                .if_none_match
                .is_some_and(|h| if_none_match_hits(h, &tag))
            {
                self.cache.touch(&key);
                return not_modified(&tag);
            }
        } else if let Err(status) = self.generate(&key, size) {
            return ImageResponse::error(status);
        }

        match self.cache.read(&key, size) {
            Ok(Some(bytes)) if stand_in => {
                self.cache.touch(&key);
                stand_in_ok(bytes, head)
            }
            Ok(Some(bytes)) => {
                self.cache.touch(&key);
                ok(&tag, bytes, head)
            }
            // Evicted between generation and read: the next request regenerates.
            Ok(None) => ImageResponse::error(404),
            Err(error) => {
                tracing::warn!(%error, "cannot read a cached thumbnail");
                ImageResponse::error(500)
            }
        }
    }

    /// The answer for the original image of `key`, or `None` when its file is unknown, gone,
    /// changed, unreadable or too large to show.
    fn original(
        &self,
        key: &ThumbKey,
        request: &ImageRequest<'_>,
        head: bool,
    ) -> Option<ImageResponse> {
        let source = self.sources.source_of(key)?;
        let tag = full_etag(key);
        let size = std::fs::metadata(&source).ok()?.len();
        if size > crate::thumb::MAX_SOURCE_BYTES {
            return None;
        }
        // The content never changes for its key, so a validator the web view still holds is
        // answered without reading the file (the key was checked when that copy was served).
        if request
            .if_none_match
            .is_some_and(|h| if_none_match_hits(h, &tag))
        {
            return Some(not_modified(&tag));
        }
        let bytes = std::fs::read(&source).ok()?;
        if ThumbKey::of_bytes(&bytes) != *key {
            // The file was replaced since it was registered; its old content is unreachable.
            self.sources.forget(key);
            return None;
        }
        let info = crate::download::picture::inspect(&bytes).ok()?;
        if !crate::thumb::fits_decode_limits(info.width, info.height) {
            return None;
        }
        let content_type = match info.ext {
            "png" => "image/png",
            "webp" => "image/webp",
            _ => "image/jpeg",
        };
        Some(ok_typed(&tag, content_type, bytes, head))
    }

    /// Generate the missing thumbnail from the registered source. `Err` carries the status.
    fn generate(&self, key: &ThumbKey, size: ThumbSize) -> Result<(), u16> {
        let source = self.sources.source_of(key).ok_or(404_u16)?;
        let _turn = self.decodes.enter();
        // Another request for the same image may have made it while this one waited.
        if self.cache.has(key, size) {
            return Ok(());
        }
        match self.cache.ensure(&source, &[size]) {
            Ok(ensured) if ensured.key == *key => Ok(()),
            Ok(_) => {
                // The file changed since it was registered; its old content is unreachable.
                self.sources.forget(key);
                Err(404)
            }
            Err(ThumbError::Io { source: e, .. }) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(404)
            }
            Err(error) => {
                tracing::warn!(%error, "cannot generate a thumbnail");
                Err(500)
            }
        }
    }
}

fn ok(tag: &str, bytes: Vec<u8>, head: bool) -> ImageResponse {
    ok_typed(tag, "image/webp", bytes, head)
}

fn ok_typed(tag: &str, content_type: &str, bytes: Vec<u8>, head: bool) -> ImageResponse {
    let mut headers = common_headers();
    headers.push(("content-type", content_type.to_owned()));
    headers.push(("content-length", bytes.len().to_string()));
    headers.push(("cache-control", CACHE_CONTROL_IMMUTABLE.to_owned()));
    headers.push(("etag", tag.to_owned()));
    ImageResponse {
        status: 200,
        headers,
        body: if head { Vec::new() } else { bytes },
    }
}

/// A thumbnail that stands in for another size or the original: not stored and not validated.
fn stand_in_ok(bytes: Vec<u8>, head: bool) -> ImageResponse {
    let mut headers = common_headers();
    headers.push(("content-type", "image/webp".to_owned()));
    headers.push(("content-length", bytes.len().to_string()));
    headers.push(("cache-control", "no-store".to_owned()));
    ImageResponse {
        status: 200,
        headers,
        body: if head { Vec::new() } else { bytes },
    }
}

fn not_modified(tag: &str) -> ImageResponse {
    let mut headers = common_headers();
    headers.push(("cache-control", CACHE_CONTROL_IMMUTABLE.to_owned()));
    headers.push(("etag", tag.to_owned()));
    ImageResponse {
        status: 304,
        headers,
        body: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::io::Cursor;

    fn png(width: u32, height: u32, tint: u8) -> Vec<u8> {
        let img = RgbaImage::from_fn(width, height, |x, y| {
            Rgba([tint, (x % 255) as u8, (y % 255) as u8, 255])
        });
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        dir: PathBuf,
        service: ImageService,
        sources: Arc<SourceMap>,
        cache: Arc<ThumbCache>,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let cache = Arc::new(ThumbCache::open(&dir.join("cache"), None).unwrap());
        let sources = Arc::new(SourceMap::new());
        let service = ImageService::new(cache.clone(), sources.clone());
        Fixture {
            _tmp: tmp,
            dir,
            service,
            sources,
            cache,
        }
    }

    fn get(path: &str) -> ImageRequest<'_> {
        ImageRequest {
            method: "GET",
            path,
            if_none_match: None,
        }
    }

    fn poster(f: &Fixture, name: &str, tint: u8) -> (ThumbKey, PathBuf) {
        let path = f.dir.join(name);
        std::fs::write(&path, png(800, 1200, tint)).unwrap();
        let key = f.sources.register_file(&path).unwrap();
        (key, path)
    }

    #[test]
    fn first_request_generates_lazily_and_answers_200_with_cache_headers() {
        let f = fixture();
        let (key, _) = poster(&f, "poster.png", 10);
        assert!(!f.cache.has(&key, ThumbSize::Grid));
        assert_eq!(f.cache.generations(), 0);

        let url = format!("/img/{key}/grid");
        let response = f.service.handle(&get(&url));

        assert_eq!(response.status, 200);
        assert_eq!(response.header("content-type"), Some("image/webp"));
        assert_eq!(
            response.header("cache-control"),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(
            response.header("etag").map(str::to_owned),
            Some(format!("\"{key}-grid\""))
        );
        assert_eq!(
            response.header("content-length"),
            Some(response.body.len().to_string().as_str())
        );
        assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(&response.body[..4], b"RIFF");
        assert_eq!(&response.body[8..12], b"WEBP");
        assert_eq!(f.cache.generations(), 1);
        assert!(f.cache.has(&key, ThumbSize::Grid));
        // Only the requested size was produced.
        assert!(!f.cache.has(&key, ThumbSize::Hero));
    }

    #[test]
    fn second_request_is_served_from_the_cache_without_decoding() {
        let f = fixture();
        let (key, path) = poster(&f, "poster.png", 20);
        let url = format!("/img/{key}/list");
        let first = f.service.handle(&get(&url));
        // The original is no longer needed once the thumbnail exists.
        std::fs::remove_file(&path).unwrap();
        let second = f.service.handle(&get(&url));
        assert_eq!(second.status, 200);
        assert_eq!(second.body, first.body);
        assert_eq!(f.cache.generations(), 1);
    }

    #[test]
    fn each_size_is_generated_on_its_own_first_request() {
        let f = fixture();
        let (key, _) = poster(&f, "poster.png", 30);
        for size in ["grid", "list", "hero"] {
            let url = format!("/img/{key}/{size}");
            assert_eq!(f.service.handle(&get(&url)).status, 200);
        }
        assert_eq!(f.cache.generations(), 3);
    }

    #[test]
    fn matching_if_none_match_answers_304_without_a_body() {
        let f = fixture();
        let (key, _) = poster(&f, "poster.png", 40);
        let url = format!("/img/{key}/grid");
        let first = f.service.handle(&get(&url));
        let tag = first.header("etag").unwrap().to_owned();

        let matching = [
            tag.clone(),
            format!("W/{tag}"),
            format!("\"other\", {tag}"),
            "*".to_owned(),
        ];
        for header in matching {
            let response = f.service.handle(&ImageRequest {
                method: "GET",
                path: &url,
                if_none_match: Some(&header),
            });
            assert_eq!(response.status, 304, "header {header}");
            assert!(response.body.is_empty());
            assert_eq!(response.header("etag"), Some(tag.as_str()));
            assert_eq!(
                response.header("cache-control"),
                Some(CACHE_CONTROL_IMMUTABLE)
            );
        }

        let stale = f.service.handle(&ImageRequest {
            method: "GET",
            path: &url,
            if_none_match: Some("\"something-else\""),
        });
        assert_eq!(stale.status, 200);
    }

    #[test]
    fn head_has_the_headers_but_no_body() {
        let f = fixture();
        let (key, _) = poster(&f, "poster.png", 50);
        let url = format!("/img/{key}/grid");
        let response = f.service.handle(&ImageRequest {
            method: "HEAD",
            path: &url,
            if_none_match: None,
        });
        assert_eq!(response.status, 200);
        assert!(response.body.is_empty());
        let length: usize = response.header("content-length").unwrap().parse().unwrap();
        assert!(length > 0);
    }

    #[test]
    fn other_methods_answer_405() {
        let f = fixture();
        let (key, _) = poster(&f, "poster.png", 60);
        let url = format!("/img/{key}/grid");
        let response = f.service.handle(&ImageRequest {
            method: "POST",
            path: &url,
            if_none_match: None,
        });
        assert_eq!(response.status, 405);
        assert_eq!(response.header("allow"), Some("GET, HEAD"));
        assert_eq!(f.cache.generations(), 0);
    }

    #[test]
    fn a_size_that_cannot_be_made_is_answered_with_a_cached_one() {
        let f = fixture();
        // An artwork candidate: cached from its downloaded bytes, at the grid size only.
        let bytes = png(800, 1200, 40);
        let key = f
            .cache
            .ensure_bytes(&bytes, &[ThumbSize::Grid])
            .unwrap()
            .key;
        let grid = f.service.handle(&get(&format!("/img/{key}/grid")));
        let hero = f.service.handle(&get(&format!("/img/{key}/hero")));
        assert_eq!(hero.status, 200);
        assert_eq!(hero.body, grid.body);
        // A reported blurred backdrop: this stand-in was kept for a year under the hero URL,
        // so the chosen image showed at the grid size after its file was written.
        assert_eq!(hero.header("cache-control"), Some("no-store"));
        assert_eq!(hero.header("etag"), None);
        assert_eq!(grid.header("cache-control"), Some(CACHE_CONTROL_IMMUTABLE));

        // Once the file is written and known, the hero size is made from it and kept.
        let path = f.dir.join("chosen.png");
        std::fs::write(&path, &bytes).unwrap();
        f.sources.register(key.clone(), path);
        let made = f.service.handle(&get(&format!("/img/{key}/hero")));
        assert_eq!(made.status, 200);
        assert_eq!(made.header("cache-control"), Some(CACHE_CONTROL_IMMUTABLE));
        assert!(f.cache.has(&key, ThumbSize::Hero));
    }

    #[test]
    fn unknown_key_answers_404_without_caching_the_miss() {
        let f = fixture();
        let key = ThumbKey::of_bytes(b"never registered");
        let response = f.service.handle(&get(&format!("/img/{key}/grid")));
        assert_eq!(response.status, 404);
        assert!(response.body.is_empty());
        assert_eq!(response.header("cache-control"), Some("no-store"));
        assert_eq!(f.cache.generations(), 0);
    }

    #[test]
    fn malformed_paths_answer_404() {
        let f = fixture();
        let key = ThumbKey::of_bytes(b"x");
        let upper = key.as_str().to_uppercase();
        let bad = [
            String::new(),
            "/".to_owned(),
            "/img".to_owned(),
            "/img/".to_owned(),
            format!("/img/{key}"),
            format!("/img/{key}/"),
            format!("/img/{key}/huge"),
            format!("/img/{key}/grid/extra"),
            format!("/other/{key}/grid"),
            format!("/img/{upper}/grid"),
            "/img/abc/grid".to_owned(),
            format!("/img/{key}/grid.webp"),
            format!("/img/{key}/%zz"),
        ];
        for path in &bad {
            assert_eq!(f.service.handle(&get(path)).status, 404, "path {path:?}");
        }
    }

    #[test]
    fn path_traversal_cannot_reach_the_file_system() {
        let f = fixture();
        let secret = f.dir.join("secret.png");
        std::fs::write(&secret, png(10, 10, 1)).unwrap();
        let key = ThumbKey::of_bytes(b"x");
        let attempts = [
            "/img/../secret.png/grid".to_owned(),
            "/img/%2e%2e/secret.png/grid".to_owned(),
            "/img/..%2f..%2fsecret/grid".to_owned(),
            format!("/img/{key}/..%2f..%2fsecret"),
            format!("/img/{key}/..%5c..%5csecret"),
            format!("/img/{key}%00/grid"),
            "/img/C:%5Cwindows%5Cwin.ini/grid".to_owned(),
            format!("/img/{key}/grid/../../../secret"),
            "/../../secret.png".to_owned(),
        ];
        for path in &attempts {
            assert_eq!(f.service.handle(&get(path)).status, 404, "path {path:?}");
        }
        assert_eq!(f.cache.generations(), 0);
        assert!(f.sources.is_empty());
    }

    #[test]
    fn parse_path_accepts_the_platform_forms() {
        let key = ThumbKey::of_bytes(b"form");
        let want = Some((key.clone(), ThumbSize::Hero));
        assert_eq!(parse_path(&format!("/img/{key}/hero")), want);
        assert_eq!(parse_path(&format!("img/{key}/hero")), want);
        // `convertFileSrc` encodes the whole path as one segment.
        assert_eq!(parse_path(&format!("/img%2F{key}%2Fhero")), want);
        assert_eq!(parse_path(&format!("/img/{key}/hero?v=1#x")), want);
        assert_eq!(parse_path(&format!("/img/{key}/hero/")), None);
        assert_eq!(parse_path(&format!("/img/{key}/full")), None);
        assert_eq!(
            parse_request_path(&format!("/img%2F{key}%2Ffull")),
            Some((key.clone(), Wanted::Full))
        );
    }

    #[test]
    fn full_answers_with_the_original_file_byte_for_byte() {
        let f = fixture();
        let (key, path) = poster(&f, "fanart.png", 100);
        let original = std::fs::read(&path).unwrap();
        let url = format!("/img/{key}/full");

        let response = f.service.handle(&get(&url));
        assert_eq!(response.status, 200);
        assert!(
            response.body == original,
            "the file itself, not a thumbnail"
        );
        assert_eq!(response.header("content-type"), Some("image/png"));
        assert_eq!(
            response.header("cache-control"),
            Some(CACHE_CONTROL_IMMUTABLE)
        );
        let tag = format!("\"{key}-full\"");
        assert_eq!(response.header("etag"), Some(tag.as_str()));
        assert_eq!(f.cache.generations(), 0, "nothing is decoded or encoded");

        let again = f.service.handle(&ImageRequest {
            method: "GET",
            path: &url,
            if_none_match: Some(&tag),
        });
        assert_eq!(again.status, 304);
        let head = f.service.handle(&ImageRequest {
            method: "HEAD",
            path: &url,
            if_none_match: None,
        });
        assert_eq!(head.status, 200);
        assert!(head.body.is_empty());
    }

    #[test]
    fn full_falls_back_to_a_thumbnail_without_a_readable_original() {
        let f = fixture();
        // A candidate in the picker: cached from its download, with no file behind it.
        let bytes = png(800, 1200, 110);
        let key = f
            .cache
            .ensure_bytes(&bytes, &[ThumbSize::Grid, ThumbSize::Hero])
            .unwrap()
            .key;
        let hero = f.service.handle(&get(&format!("/img/{key}/hero")));
        let full = f.service.handle(&get(&format!("/img/{key}/full")));
        assert_eq!(full.status, 200);
        assert_eq!(full.body, hero.body);
        assert_eq!(full.header("content-type"), Some("image/webp"));
        assert_eq!(full.header("cache-control"), Some("no-store"));

        // A replaced original is not served under the old key, and nothing else can be made
        // for it: 404, and the stale mapping is forgotten.
        let (old, path) = poster(&f, "replaced.png", 120);
        std::fs::write(&path, png(800, 1200, 121)).unwrap();
        assert_eq!(
            f.service.handle(&get(&format!("/img/{old}/full"))).status,
            404
        );
        assert!(f.sources.source_of(&old).is_none());

        // An offline original whose hero thumbnail is cached still shows that.
        let (offline, path) = poster(&f, "offline.png", 130);
        assert_eq!(
            f.service
                .handle(&get(&format!("/img/{offline}/hero")))
                .status,
            200
        );
        std::fs::remove_file(&path).unwrap();
        let response = f.service.handle(&get(&format!("/img/{offline}/full")));
        assert_eq!(response.status, 200);
        assert_eq!(response.header("content-type"), Some("image/webp"));
    }

    #[test]
    fn a_replaced_source_answers_404_and_is_forgotten() {
        let f = fixture();
        let (key, path) = poster(&f, "poster.png", 70);
        std::fs::write(&path, png(800, 1200, 200)).unwrap();
        let response = f.service.handle(&get(&format!("/img/{key}/grid")));
        assert_eq!(response.status, 404);
        assert!(f.sources.source_of(&key).is_none());
    }

    #[test]
    fn a_missing_source_answers_404_and_a_broken_one_500() {
        let f = fixture();
        let (gone, path) = poster(&f, "gone.png", 80);
        std::fs::remove_file(&path).unwrap();
        let url = format!("/img/{gone}/grid");
        assert_eq!(f.service.handle(&get(&url)).status, 404);

        let junk = f.dir.join("junk.png");
        std::fs::write(&junk, b"not an image").unwrap();
        let key = f.sources.register_file(&junk).unwrap();
        let response = f.service.handle(&get(&format!("/img/{key}/grid")));
        assert_eq!(response.status, 500);
        assert!(response.body.is_empty());
        assert_eq!(response.header("cache-control"), Some("no-store"));
    }

    #[test]
    fn renaming_the_original_keeps_serving_the_same_url() {
        let f = fixture();
        let (key, path) = poster(&f, "before.png", 90);
        let url = format!("/img/{key}/grid");
        let first = f.service.handle(&get(&url));
        let renamed = f.dir.join("after.png");
        std::fs::rename(&path, &renamed).unwrap();
        f.sources.register(key.clone(), renamed);
        let second = f.service.handle(&get(&url));
        assert_eq!(second.body, first.body);
        assert_eq!(f.cache.generations(), 1);
    }

    #[test]
    fn concurrent_requests_all_succeed() {
        let f = fixture();
        let (key, _) = poster(&f, "poster.png", 100);
        let url = format!("/img/{key}/grid");
        let handles: Vec<_> = (0..6)
            .map(|_| {
                let service = f.service.clone();
                let url = url.clone();
                std::thread::spawn(move || service.handle(&get(&url)).status)
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), 200);
        }
    }

    #[test]
    fn concurrent_requests_for_one_image_do_not_all_decode_it() {
        let f = fixture();
        let (key, _) = poster(&f, "poster.png", 120);
        let url = format!("/img/{key}/grid");
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let service = f.service.clone();
                let url = url.clone();
                std::thread::spawn(move || service.handle(&get(&url)).status)
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), 200);
        }
        // A request that waited for its turn finds the thumbnail made and does not decode again.
        let made = usize::try_from(f.cache.generations()).unwrap();
        assert!(made >= 1 && made <= f.service.decodes.max, "{made}");
    }

    #[test]
    fn no_more_than_the_turns_decode_at_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let gate = DecodeGate::new(2);
        let now = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..12 {
                scope.spawn(|| {
                    let _turn = gate.enter();
                    let running = now.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(running, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(15));
                    now.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        assert_eq!(peak.load(Ordering::SeqCst), 2);
    }
}

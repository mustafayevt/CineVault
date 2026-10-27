//! Torrentio (the Stremio addon): releases by IMDb ID, one request per search.
//!
//! `GET {base}/stream/movie/{imdb}.json` and `{base}/stream/series/{imdb}:{season}:{episode}.json`.
//! Each stream carries `infoHash`, `fileIdx`, `behaviorHints.filename` and a `title` of
//! two to four lines:
//!
//! ```text
//! Release.Name.1080p.BluRay.x264-GRP        <- the torrent's name
//! Show.S01E02.Title.mkv                      <- the file inside it (series only)
//! 👤 54 💾 4.35 GB ⚙️ YTS                    <- seeders, size, the tracker it came from
//! 🇬🇧 / 🇮🇹                                  <- languages (not always)
//! ```
//!
//! A season search asks for episode 1 and keeps the packs (the relevance check hides single
//! episodes of that season). A whole-show search is skipped: the addon has no such request.
//! Answers are kept in memory for 30 minutes per address.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use cv_providers::http::HttpClient;
use regex::Regex;
use serde::Deserialize;

use crate::magnet::normalise_info_hash;
use crate::model::{
    RawRelease, Scope, SkipReason, SourceError, SourceId, SourceKind, Support, TorrentQuery,
};
use crate::source::{SearchContext, SourceHttp, TorrentSource};

/// The id of this source.
pub const ID: &str = "torrentio";
/// How long an answer is reused.
pub const CACHE_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Deserialize)]
struct Answer {
    #[serde(default)]
    streams: Vec<Stream>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stream {
    title: Option<String>,
    info_hash: Option<String>,
    behavior_hints: Option<Hints>,
}

#[derive(Debug, Deserialize)]
struct Hints {
    filename: Option<String>,
}

struct Patterns {
    seeders: Regex,
    size: Regex,
}

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| Patterns {
    seeders: Regex::new(r"\u{1F464}\s*(\d+)").expect("seeders pattern"),
    size: Regex::new(r"\u{1F4BE}\s*([\d.]+)\s*([KMGT]i?B)").expect("size pattern"),
});

fn size_bytes(amount: &str, unit: &str) -> Option<u64> {
    let amount: f64 = amount.parse().ok()?;
    let power = match unit.chars().next()? {
        'K' => 1,
        'M' => 2,
        'G' => 3,
        'T' => 4,
        _ => return None,
    };
    let bytes = amount * 1024f64.powi(power);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (bytes.is_finite() && bytes >= 0.0).then_some(bytes as u64)
}

/// Reads one stream into a release; `None` for a stream that cannot be used (no title, no valid
/// info-hash).
fn read_stream(stream: Stream) -> Option<RawRelease> {
    let title = stream.title?;
    let info_hash = normalise_info_hash(stream.info_hash.as_deref()?)?;
    let mut lines = title.lines().map(str::trim).filter(|line| !line.is_empty());
    let name = lines.next()?.to_owned();

    let mut files: Vec<String> = Vec::new();
    let mut seeders = None;
    let mut size = None;
    for line in lines {
        if let Some(found) = PATTERNS.seeders.captures(line) {
            seeders = found[1].parse().ok();
            if let Some(found) = PATTERNS.size.captures(line) {
                size = size_bytes(&found[1], &found[2]);
            }
        } else if !line
            .chars()
            .any(|c| ('\u{1F1E6}'..='\u{1F1FF}').contains(&c))
        {
            // A line that is neither the statistics nor the language flags is the file name.
            files.push(line.to_owned());
        }
    }
    if let Some(hints) = stream.behavior_hints
        && let Some(filename) = hints.filename
        && !filename.trim().is_empty()
        && !files.contains(&filename)
    {
        files.push(filename);
    }
    Some(RawRelease {
        source: Some(SourceId(ID.to_owned())),
        name,
        info_hash: Some(info_hash),
        seeders,
        size,
        files,
        ..RawRelease::default()
    })
}

/// Parses an answer; streams that cannot be read are dropped.
fn parse_answer(answer: Answer) -> Vec<RawRelease> {
    answer.streams.into_iter().filter_map(read_stream).collect()
}

/// The Torrentio source.
pub struct TorrentioSource {
    id: SourceId,
    http: SourceHttp,
    base_url: String,
    ttl: Duration,
    cache: Mutex<HashMap<String, (Instant, Vec<RawRelease>)>>,
}

impl TorrentioSource {
    /// A source that asks `base_url` (no trailing slash) through `client`.
    #[must_use]
    pub fn new(client: HttpClient, base_url: &str) -> Self {
        Self {
            id: SourceId(ID.to_owned()),
            http: SourceHttp::new(client, ID),
            base_url: base_url.trim_end_matches('/').to_owned(),
            ttl: CACHE_TTL,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Changes how long answers are reused (tests use a very short time).
    #[must_use]
    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// The address to ask for this query, or why the source does not apply.
    fn address(&self, query: &TorrentQuery) -> Result<String, SkipReason> {
        let imdb = query.ids.imdb.as_deref().ok_or(SkipReason::NoImdbId)?;
        match query.scope {
            Scope::Movie => Ok(format!("{}/stream/movie/{imdb}.json", self.base_url)),
            Scope::Show => Err(SkipReason::ShowScope),
            Scope::Season(season) => Ok(format!(
                "{}/stream/series/{imdb}:{season}:1.json",
                self.base_url
            )),
            Scope::Episode(season, episode) => Ok(format!(
                "{}/stream/series/{imdb}:{season}:{episode}.json",
                self.base_url
            )),
        }
    }

    fn cached(&self, address: &str) -> Option<Vec<RawRelease>> {
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .get(address)
            .filter(|(at, _)| at.elapsed() < self.ttl)
            .map(|(_, rows)| rows.clone())
    }
}

#[async_trait]
impl TorrentSource for TorrentioSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn name(&self) -> &str {
        "Torrentio"
    }

    fn kind(&self) -> SourceKind {
        SourceKind::BuiltIn
    }

    fn supports(&self, query: &TorrentQuery) -> Support {
        match self.address(query) {
            Ok(_) => Support::Yes,
            Err(reason) => Support::Skip(reason),
        }
    }

    async fn search(
        &self,
        query: &TorrentQuery,
        _context: &SearchContext,
    ) -> Result<Vec<RawRelease>, SourceError> {
        let address = self
            .address(query)
            .map_err(|_| SourceError::BadAnswer("the source does not apply".to_owned()))?;
        if let Some(rows) = self.cached(&address) {
            return Ok(rows);
        }
        let answer: Answer = self.http.get_json(&address, &[]).await?;
        let rows = parse_answer(answer);
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(address, (Instant::now(), rows.clone()));
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(title: &str, hash: &str) -> Stream {
        Stream {
            title: Some(title.to_owned()),
            info_hash: Some(hash.to_owned()),
            behavior_hints: None,
        }
    }

    const HASH: &str = "65fc916711647cfe3a5ab0e3718f776414a5914f";

    #[test]
    fn a_movie_stream_is_read() {
        let row = read_stream(stream(
            "Night.of.the.Living.Dead.1968.2160p.BluRay\n\u{1F464} 54 \u{1F4BE} 4.35 GB \u{2699}\u{FE0F} YTS",
            HASH,
        ))
        .expect("a release");
        assert_eq!(row.name, "Night.of.the.Living.Dead.1968.2160p.BluRay");
        assert_eq!(row.seeders, Some(54));
        assert_eq!(row.size, Some((4.35 * 1024f64.powi(3)) as u64));
        assert_eq!(row.info_hash.as_deref(), Some(HASH));
        assert!(row.files.is_empty());
        assert_eq!(row.source, Some(SourceId("torrentio".into())));
    }

    #[test]
    fn a_series_stream_keeps_the_file_name_and_skips_language_flags() {
        let row = read_stream(stream(
            "Show.S01.1080p\nShow.S01E02.Title.mkv\n\u{1F464} 6 \u{1F4BE} 800 MB \u{2699}\u{FE0F} 1337x\n\u{1F1EC}\u{1F1E7} / \u{1F1EE}\u{1F1F9}",
            HASH,
        ))
        .expect("a release");
        assert_eq!(row.name, "Show.S01.1080p");
        assert_eq!(row.files, vec!["Show.S01E02.Title.mkv"]);
        assert_eq!(row.size, Some(800 * 1024 * 1024));
    }

    #[test]
    fn the_hint_file_name_is_added_once() {
        let mut s = stream("Name\nFile.mkv\n\u{1F464} 1", HASH);
        s.behavior_hints = Some(Hints {
            filename: Some("File.mkv".into()),
        });
        let row = read_stream(s).expect("a release");
        assert_eq!(row.files, vec!["File.mkv"]);
    }

    #[test]
    fn a_title_without_statistics_still_gives_a_release() {
        let row = read_stream(stream("Just.A.Name", HASH)).expect("a release");
        assert_eq!(row.seeders, None);
        assert_eq!(row.size, None);
    }

    #[test]
    fn unusable_streams_are_dropped() {
        assert!(
            read_stream(Stream {
                title: None,
                info_hash: Some(HASH.into()),
                behavior_hints: None
            })
            .is_none()
        );
        assert!(read_stream(stream("Name", "not-a-hash")).is_none());
        assert!(
            read_stream(Stream {
                title: Some("Name".into()),
                info_hash: None,
                behavior_hints: None
            })
            .is_none()
        );
        assert!(read_stream(stream("   \n  ", HASH)).is_none());
    }

    #[test]
    fn sizes_use_binary_units() {
        assert_eq!(size_bytes("1", "KB"), Some(1024));
        assert_eq!(size_bytes("1.5", "MB"), Some(1_572_864));
        assert_eq!(size_bytes("1", "TiB"), Some(1024u64.pow(4)));
        assert_eq!(size_bytes("x", "GB"), None);
        assert_eq!(size_bytes("1", "PB"), None);
    }
}

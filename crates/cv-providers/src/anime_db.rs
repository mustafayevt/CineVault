//! The anime offline ID cross-reference: an on-demand download of the anime-offline-database
//! into the app's cache folder, converted to [`AnimeCrossReference`] rows.
//!
//! The database is a community-maintained list of every anime with the address of its page at
//! each anime site. Its releases are one JSON line per anime (`anime-offline-database.jsonl`,
//! about 60 MB); the first line is metadata. Each address becomes one ID (`myanimelist`,
//! `anilist`, `kitsu`, `anisearch`, `animeplanet`, `livechart`, `notify`, `animecountdown`,
//! `animenewsnetwork`, `simkl` and AniDB's `anidb`), and the IDs of one anime form one row. The
//! rows let an anime found by any of those IDs get the others without a request (see
//! [`crate::complete_ids`]).
//!
//! **Nothing ships with the app and nothing is committed.** [`AnimeOfflineDb::load`] downloads the
//! file the first time it is needed, converts it to a compact `crosswalk.json` (only anime with
//! at least two IDs) and keeps it with a `meta.json` in the folder it was given (the app's cache
//! folder). After that:
//!
//! * while the copy is younger than the **refresh interval** (30 days by default) it is read
//!   from disk and no request is made;
//! * when it is older, one conditional request asks the server whether it changed, sending the
//!   stored **ETag**; a `304` or an unchanged **SHA-256** of the download keeps the converted
//!   file and only renews its check time;
//! * when the server cannot be reached the old copy is used, and the caller is told.
//!
//! Files are written to a temporary name and renamed, so an interrupted download leaves the
//! previous copy intact.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cv_domain::ProviderIds;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::completion::AnimeCrossReference;
use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest};

/// The lane the download goes through.
pub const PROVIDER_ID: &str = "animeofflinedb";

/// The latest release of the database, as one JSON line per anime.
pub const DEFAULT_URL: &str = "https://github.com/manami-project/anime-offline-database/releases/latest/download/anime-offline-database.jsonl";

/// How long a downloaded copy is used before the server is asked again: 30 days.
pub const DEFAULT_REFRESH: Duration = Duration::from_secs(30 * 24 * 60 * 60);

const CROSSWALK_FILE: &str = "crosswalk.json";
const META_FILE: &str = "meta.json";

/// Why the cross-reference could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnimeDbError {
    /// There is no usable local copy and the download failed.
    #[error("the anime ID database could not be downloaded: {0}")]
    Download(ProviderError),
    /// The downloaded file is not the anime-offline-database.
    #[error("the downloaded anime ID database is not in the expected format: {0}")]
    Format(String),
    /// The cache folder could not be read or written.
    #[error("the anime ID database cache could not be used: {0}")]
    Cache(String),
}

/// What [`AnimeOfflineDb::load`] did to get the cross-reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    /// The local copy was young enough; no request was made.
    Fresh,
    /// The server was asked and the local copy is still the current one.
    Unchanged,
    /// A new release was downloaded and converted.
    Updated,
    /// The server could not be reached or answered with an error; the older local copy was used.
    KeptAfterError(ProviderError),
}

/// The loaded cross-reference and how it was obtained.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub crosswalk: AnimeCrossReference,
    pub outcome: LoadOutcome,
}

/// What is known about the local copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    /// Where the copy was downloaded from.
    pub url: String,
    /// The `ETag` the server sent with it, for the next conditional request.
    pub etag: Option<String>,
    /// SHA-256 of the downloaded file, in hex.
    pub sha256: String,
    /// When the server was last asked, in milliseconds since the Unix epoch.
    pub checked_at_ms: u64,
    /// When the copy was downloaded, in milliseconds since the Unix epoch.
    pub downloaded_at_ms: u64,
    /// The number of rows in `crosswalk.json`.
    pub rows: usize,
}

/// The anime offline database in a cache folder. Cloning is cheap.
#[derive(Clone)]
pub struct AnimeOfflineDb {
    http: HttpClient,
    dir: PathBuf,
    url: String,
    refresh: Duration,
}

impl AnimeOfflineDb {
    /// The database in `dir`, downloaded from [`DEFAULT_URL`] by a client with the long timeout
    /// a 60 MB file needs. `dir` is created on the first download.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self, ProviderError> {
        Ok(Self::with_http(download_client()?, dir, DEFAULT_URL))
    }

    /// The database in `dir`, downloaded from `url` with `http` (tests use a local server).
    pub fn with_http(http: HttpClient, dir: impl Into<PathBuf>, url: &str) -> Self {
        Self {
            http,
            dir: dir.into(),
            url: url.to_owned(),
            refresh: DEFAULT_REFRESH,
        }
    }

    /// Sets how long a local copy is used before the server is asked again.
    #[must_use]
    pub fn refresh_interval(mut self, interval: Duration) -> Self {
        self.refresh = interval;
        self
    }

    /// The folder the copy lives in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn crosswalk_path(&self) -> PathBuf {
        self.dir.join(CROSSWALK_FILE)
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join(META_FILE)
    }

    /// What is known about the local copy, if there is one.
    pub fn meta(&self) -> Option<Meta> {
        let text = std::fs::read_to_string(self.meta_path()).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Reads the local copy without any request. `None` when there is none (or it is damaged).
    pub fn load_cached(&self) -> Option<AnimeCrossReference> {
        let text = std::fs::read_to_string(self.crosswalk_path()).ok()?;
        match AnimeCrossReference::from_json(&text) {
            Ok(crosswalk) if !crosswalk.is_empty() => Some(crosswalk),
            _ => None,
        }
    }

    /// Whether the local copy is old enough that the server should be asked.
    fn due(&self, meta: &Meta) -> bool {
        let now = now_ms();
        now.saturating_sub(meta.checked_at_ms)
            >= u64::try_from(self.refresh.as_millis()).unwrap_or(u64::MAX)
    }

    /// The cross-reference, downloading it when there is no local copy or the copy is older than
    /// the refresh interval.
    pub async fn load(&self) -> Result<Loaded, AnimeDbError> {
        if let (Some(meta), Some(crosswalk)) = (self.meta(), self.load_cached())
            && !self.due(&meta)
        {
            return Ok(Loaded {
                crosswalk,
                outcome: LoadOutcome::Fresh,
            });
        }
        self.refresh_now().await
    }

    /// Asks the server whether there is a newer release and updates the local copy, whatever its
    /// age.
    pub async fn refresh_now(&self) -> Result<Loaded, AnimeDbError> {
        let cached = self.load_cached();
        // The ETag is only worth sending when the converted copy it belongs to is there.
        let meta = self
            .meta()
            .filter(|m| cached.is_some() && m.url == self.url);

        match self.download(meta.as_ref()).await {
            Ok(Download::NotModified) => {
                let (Some(meta), Some(crosswalk)) = (meta, cached) else {
                    return Err(AnimeDbError::Format(
                        "the server answered 304 without a local copy".to_owned(),
                    ));
                };
                self.write_meta(&Meta {
                    checked_at_ms: now_ms(),
                    ..meta
                })?;
                Ok(Loaded {
                    crosswalk,
                    outcome: LoadOutcome::Unchanged,
                })
            }
            Ok(Download::Body { bytes, etag }) => {
                let sha256 = hex_sha256(&bytes);
                if let (Some(meta), Some(crosswalk)) = (meta.clone(), cached.clone())
                    && meta.sha256 == sha256
                {
                    // The same file under a new ETag (or none): keep the converted copy.
                    self.write_meta(&Meta {
                        etag,
                        checked_at_ms: now_ms(),
                        ..meta
                    })?;
                    return Ok(Loaded {
                        crosswalk,
                        outcome: LoadOutcome::Unchanged,
                    });
                }
                let rows =
                    tokio::task::spawn_blocking(move || convert(&String::from_utf8_lossy(&bytes)))
                        .await
                        .map_err(|e| AnimeDbError::Cache(e.to_string()))??;
                let crosswalk = self.store(&rows, &sha256, etag)?;
                Ok(Loaded {
                    crosswalk,
                    outcome: LoadOutcome::Updated,
                })
            }
            Err(error) => match cached {
                Some(crosswalk) => {
                    tracing::warn!(%error, "anime ID database not refreshed; using the older copy");
                    Ok(Loaded {
                        crosswalk,
                        outcome: LoadOutcome::KeptAfterError(error),
                    })
                }
                None => Err(AnimeDbError::Download(error)),
            },
        }
    }

    async fn download(&self, meta: Option<&Meta>) -> Result<Download, ProviderError> {
        let mut request = HttpRequest::get(PROVIDER_ID, &self.url).uncached();
        if let Some(etag) = meta.and_then(|m| m.etag.as_deref()) {
            request = request.header("If-None-Match", etag);
        }
        let response = self.http.send(request).await?;
        match response.status() {
            304 => Ok(Download::NotModified),
            s if (200..300).contains(&s) => Ok(Download::Body {
                bytes: response.body().to_vec(),
                etag: response.etag().map(str::to_owned),
            }),
            s => Err(ProviderError::Request {
                provider: "anime ID database".to_owned(),
                message: format!("HTTP {s}"),
            }),
        }
    }

    /// Writes the converted rows and their metadata, and returns the cross-reference.
    fn store(
        &self,
        rows: &[ProviderIds],
        sha256: &str,
        etag: Option<String>,
    ) -> Result<AnimeCrossReference, AnimeDbError> {
        std::fs::create_dir_all(&self.dir).map_err(cache_error)?;
        let json = serde_json::to_vec(rows).map_err(|e| AnimeDbError::Cache(e.to_string()))?;
        write_atomic(&self.crosswalk_path(), &json)?;
        let now = now_ms();
        self.write_meta(&Meta {
            url: self.url.clone(),
            etag,
            sha256: sha256.to_owned(),
            checked_at_ms: now,
            downloaded_at_ms: now,
            rows: rows.len(),
        })?;
        Ok(AnimeCrossReference::from_rows(rows.iter().cloned()))
    }

    fn write_meta(&self, meta: &Meta) -> Result<(), AnimeDbError> {
        std::fs::create_dir_all(&self.dir).map_err(cache_error)?;
        let json =
            serde_json::to_vec_pretty(meta).map_err(|e| AnimeDbError::Cache(e.to_string()))?;
        write_atomic(&self.meta_path(), &json)
    }
}

enum Download {
    NotModified,
    Body {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
}

/// An HTTP client for the big download: no disk cache and a ten minute timeout.
pub fn download_client() -> Result<HttpClient, ProviderError> {
    HttpClient::builder()
        .timeout(Duration::from_secs(600))
        // The offline database is far larger than an API answer.
        .max_body_bytes(512 * 1024 * 1024)
        .provider_limits(
            PROVIDER_ID,
            crate::http::ProviderLimits::new(1, 1, Duration::from_secs(10)),
        )
        .build()
}

fn cache_error(error: std::io::Error) -> AnimeDbError {
    AnimeDbError::Cache(error.to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Writes `bytes` to a temporary file next to `path` and renames it over `path`.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AnimeDbError> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, bytes).map_err(cache_error)?;
    std::fs::rename(&temporary, path).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        cache_error(e)
    })
}

/// One line of the database. Everything but the page addresses is ignored.
#[derive(Deserialize)]
struct Line {
    #[serde(default)]
    sources: Vec<String>,
}

/// The whole-document form of the database: `{"data": [ ... ]}`.
#[derive(Deserialize)]
struct Document {
    data: Vec<Line>,
}

/// The ID an anime page address stands for, as `(provider key, ID)`.
pub fn source_id(address: &str) -> Option<(&'static str, String)> {
    let rest = address.trim().split_once("://")?.1;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let path = path.trim_matches('/');

    // `anime/{id}` on most sites; the ID may be followed by a slug (`4563-death-note`).
    let after_anime = || {
        let id = path.strip_prefix("anime/")?;
        let id = id.split('/').next()?;
        Some(id.to_owned())
    };
    let numeric = |id: String| {
        let digits: String = id.chars().take_while(char::is_ascii_digit).collect();
        (!digits.is_empty()).then_some(digits)
    };
    match host {
        "anidb.net" => {
            // `anime/4563` and the short form `a4563`.
            let id = after_anime().or_else(|| path.strip_prefix('a').map(str::to_owned))?;
            numeric(id).map(|id| ("anidb", id))
        }
        "myanimelist.net" => numeric(after_anime()?).map(|id| ("myanimelist", id)),
        "anilist.co" => numeric(after_anime()?).map(|id| ("anilist", id)),
        "kitsu.app" | "kitsu.io" => after_anime()
            .filter(|id| !id.is_empty())
            .map(|id| ("kitsu", id)),
        "anime-planet.com" => after_anime()
            .filter(|id| !id.is_empty())
            .map(|id| ("animeplanet", id)),
        "anisearch.com" => numeric(after_anime()?).map(|id| ("anisearch", id)),
        "notify.moe" => after_anime()
            .filter(|id| !id.is_empty())
            .map(|id| ("notify", id)),
        "livechart.me" => numeric(after_anime()?).map(|id| ("livechart", id)),
        "simkl.com" => numeric(after_anime()?).map(|id| ("simkl", id)),
        "animecountdown.com" => numeric(path.to_owned()).map(|id| ("animecountdown", id)),
        "animenewsnetwork.com" => {
            let id = query.split('&').find_map(|pair| pair.strip_prefix("id="))?;
            numeric(id.to_owned()).map(|id| ("animenewsnetwork", id))
        }
        _ => None,
    }
}

/// Converts the database text to rows of IDs. Anime with fewer than two IDs link nothing and are
/// left out. A text that holds no anime at all is not the database.
pub fn convert(text: &str) -> Result<Vec<ProviderIds>, AnimeDbError> {
    let mut lines: Vec<Line> = Vec::new();
    let mut anime_lines = 0usize;
    for raw in text.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        // The metadata line has no `sources`; a line that is not JSON ends the per-line form.
        match serde_json::from_str::<Line>(raw) {
            Ok(line) if !line.sources.is_empty() => {
                anime_lines += 1;
                lines.push(line);
            }
            Ok(_) | Err(_) => {}
        }
    }
    if anime_lines == 0
        && let Ok(document) = serde_json::from_str::<Document>(text)
    {
        lines = document.data;
        anime_lines = lines.len();
    }
    if anime_lines == 0 {
        return Err(AnimeDbError::Format(
            "no anime with page addresses were found".to_owned(),
        ));
    }
    let rows: Vec<ProviderIds> = lines
        .iter()
        .filter_map(|line| {
            let mut ids = ProviderIds::new();
            for address in &line.sources {
                if let Some((key, id)) = source_id(address)
                    && !ids.contains(key)
                {
                    ids.set(key, id);
                }
            }
            (ids.len() > 1).then_some(ids)
        })
        .collect();
    if rows.is_empty() {
        return Err(AnimeDbError::Format(
            "no anime has two known page addresses".to_owned(),
        ));
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_addresses_become_ids() {
        let id = |address: &str| source_id(address);
        assert_eq!(
            id("https://anidb.net/anime/4563"),
            Some(("anidb", "4563".to_owned()))
        );
        assert_eq!(
            id("https://anidb.net/a4563"),
            Some(("anidb", "4563".to_owned()))
        );
        assert_eq!(
            id("https://myanimelist.net/anime/1535"),
            Some(("myanimelist", "1535".to_owned()))
        );
        assert_eq!(
            id("https://anilist.co/anime/1535"),
            Some(("anilist", "1535".to_owned()))
        );
        assert_eq!(
            id("https://kitsu.app/anime/1376"),
            Some(("kitsu", "1376".to_owned()))
        );
        assert_eq!(
            id("https://kitsu.io/anime/death-note"),
            Some(("kitsu", "death-note".to_owned()))
        );
        assert_eq!(
            id("https://anime-planet.com/anime/death-note"),
            Some(("animeplanet", "death-note".to_owned()))
        );
        assert_eq!(
            id("https://anisearch.com/anime/4563,death-note"),
            Some(("anisearch", "4563".to_owned()))
        );
        assert_eq!(
            id("https://notify.moe/anime/0-A-5Fimg"),
            Some(("notify", "0-A-5Fimg".to_owned()))
        );
        assert_eq!(
            id("https://livechart.me/anime/3437"),
            Some(("livechart", "3437".to_owned()))
        );
        assert_eq!(
            id("https://animecountdown.com/48858"),
            Some(("animecountdown", "48858".to_owned()))
        );
        assert_eq!(
            id("https://www.animenewsnetwork.com/encyclopedia/anime.php?id=2808"),
            Some(("animenewsnetwork", "2808".to_owned()))
        );
        assert_eq!(
            id("https://simkl.com/anime/48858"),
            Some(("simkl", "48858".to_owned()))
        );
    }

    #[test]
    fn unknown_sites_and_malformed_addresses_are_ignored() {
        for address in [
            "https://example.test/anime/1",
            "https://myanimelist.net/manga/1",
            "https://myanimelist.net/anime/",
            "https://myanimelist.net/anime/abc",
            "myanimelist.net/anime/1",
            "",
        ] {
            assert_eq!(source_id(address), None, "{address}");
        }
    }

    #[test]
    fn each_anime_becomes_a_row_and_single_address_anime_are_dropped() {
        let text = concat!(
            r#"{"$schema":"x","lastUpdate":"2026-07-04"}"#,
            "\n",
            r#"{"sources":["https://anidb.net/anime/1","https://myanimelist.net/anime/10","https://example.test/a"],"title":"A"}"#,
            "\n",
            r#"{"sources":["https://myanimelist.net/anime/20"],"title":"Only one"}"#,
            "\n",
            "\n",
            r#"{"sources":["https://anilist.co/anime/30","https://anilist.co/anime/31","https://kitsu.app/anime/5"],"title":"Twice"}"#,
            "\n",
        );
        let rows = convert(text).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("anidb"), Some("1"));
        assert_eq!(rows[0].get("myanimelist"), Some("10"));
        assert_eq!(rows[0].len(), 2);
        // The first address of a site wins.
        assert_eq!(rows[1].get("anilist"), Some("30"));
        assert_eq!(rows[1].get("kitsu"), Some("5"));
    }

    #[test]
    fn the_whole_document_form_is_read_too() {
        let text = r#"{"license":{},"data":[{"sources":["https://anidb.net/anime/1","https://myanimelist.net/anime/10"]}]}"#;
        assert_eq!(convert(text).unwrap().len(), 1);
    }

    #[test]
    fn a_file_that_is_not_the_database_is_rejected() {
        for text in [
            "",
            "<html>Not found</html>",
            r#"{"hello":"world"}"#,
            r#"{"sources":["https://example.test/a"]}"#,
        ] {
            assert!(
                matches!(convert(text), Err(AnimeDbError::Format(_))),
                "{text}"
            );
        }
    }
}

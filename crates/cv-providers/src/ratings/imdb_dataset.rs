//! The free IMDb ratings dataset (`title.ratings.tsv.gz`): an on-demand download into the app's
//! cache folder, kept as a compact file that is searched on disk.
//!
//! **Nothing ships with the app and nothing is committed.** The first lookup downloads the file
//! (about 7 MB compressed, one line per title with its average rating and vote count), converts
//! it to `ratings.bin` (10 bytes per title, sorted by IMDb number, searched by bisection without
//! loading it into memory) and writes a `meta.json` beside it. After that:
//!
//! * while the copy is younger than the **refresh interval** (one day by default) no request is
//!   made;
//! * when it is older, one conditional request asks the server whether it changed, sending the
//!   stored **ETag**; a `304` or an unchanged **SHA-256** keeps the converted file and only
//!   renews its check time;
//! * when the server cannot be reached the old copy is used, and the failure is remembered for
//!   a few minutes so a batch of lookups does not retry the download for every title.
//!
//! The dataset is also a [`RatingProvider`], so the rating fetch treats it like any other source.
//! It needs no credentials. Files are written to a temporary name and renamed, so an interrupted
//! download leaves the previous copy intact.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use cv_domain::{ProviderIds, Rating, Ratings};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest, ProviderLimits};
use crate::model::{MediaKind, RequestOptions};
use crate::shared::imdb_id_of;
use crate::traits::{Provider, RatingProvider};

/// The provider key and HTTP lane of the download.
pub const PROVIDER_ID: &str = "imdbdataset";

/// The dataset file IMDb publishes for non-commercial use.
pub const DEFAULT_URL: &str = "https://datasets.imdbws.com/title.ratings.tsv.gz";

/// How long a downloaded copy is used before the server is asked again: one day.
pub const DEFAULT_REFRESH: Duration = Duration::from_secs(24 * 60 * 60);

/// How long a failed refresh is not tried again.
const FAILURE_BACKOFF: Duration = Duration::from_secs(5 * 60);

const DISPLAY_NAME: &str = "IMDb ratings dataset";
const DATA_FILE: &str = "ratings.bin";
const META_FILE: &str = "meta.json";
const MAGIC: &[u8; 8] = b"CVIRATE1";
const RECORD: usize = 10;

/// What [`ImdbRatingsDataset::ensure`] did to have a copy ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatasetOutcome {
    /// The local copy was young enough; no request was made.
    Fresh,
    /// The server was asked and the local copy is still the current one.
    Unchanged,
    /// A new file was downloaded and converted.
    Updated,
    /// The server could not be reached; the older local copy is used.
    KeptAfterError(ProviderError),
}

/// What is known about the local copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetMeta {
    pub url: String,
    /// The `ETag` the server sent, for the next conditional request.
    pub etag: Option<String>,
    /// SHA-256 of the downloaded file, in hex.
    pub sha256: String,
    /// When the server was last asked, in milliseconds since the Unix epoch.
    pub checked_at_ms: u64,
    /// When the copy was downloaded, in milliseconds since the Unix epoch.
    pub downloaded_at_ms: u64,
    /// The number of titles in `ratings.bin`.
    pub rows: usize,
}

struct Inner {
    http: HttpClient,
    dir: PathBuf,
    url: String,
    refresh: Duration,
    /// Serialises refreshes so two lookups do not download the file twice.
    gate: tokio::sync::Mutex<()>,
    failure: Mutex<Option<(Instant, ProviderError)>>,
}

/// The IMDb ratings dataset in a cache folder. Cloning is cheap.
#[derive(Clone)]
pub struct ImdbRatingsDataset {
    inner: Arc<Inner>,
}

impl ImdbRatingsDataset {
    /// The dataset in `dir`, downloaded from [`DEFAULT_URL`]. `dir` is created on the first
    /// download.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self, ProviderError> {
        Ok(Self::with_http(download_client()?, dir, DEFAULT_URL))
    }

    /// The dataset in `dir`, downloaded from `url` with `http` (tests use a local server).
    pub fn with_http(http: HttpClient, dir: impl Into<PathBuf>, url: &str) -> Self {
        Self::build(http, dir.into(), url, DEFAULT_REFRESH)
    }

    fn build(http: HttpClient, dir: PathBuf, url: &str, refresh: Duration) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                dir,
                url: url.to_owned(),
                refresh,
                gate: tokio::sync::Mutex::new(()),
                failure: Mutex::new(None),
            }),
        }
    }

    /// Sets how long a local copy is used before the server is asked again.
    #[must_use]
    pub fn refresh_interval(self, interval: Duration) -> Self {
        Self::build(
            self.inner.http.clone(),
            self.inner.dir.clone(),
            &self.inner.url,
            interval,
        )
    }

    /// Forgets the last failed download, so the next [`Self::ensure`] tries again at once instead
    /// of waiting out the pause after a failure. For an explicit Retry.
    pub fn clear_failure(&self) {
        *self.inner.failure.lock().unwrap() = None;
    }

    /// The folder the copy lives in.
    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    fn data_path(&self) -> PathBuf {
        self.inner.dir.join(DATA_FILE)
    }

    fn meta_path(&self) -> PathBuf {
        self.inner.dir.join(META_FILE)
    }

    /// What is known about the local copy, if there is one.
    pub fn meta(&self) -> Option<DatasetMeta> {
        let text = std::fs::read_to_string(self.meta_path()).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Whether a usable converted copy is on disk.
    pub fn has_copy(&self) -> bool {
        File::open(self.data_path()).is_ok_and(|mut f| {
            let mut magic = [0u8; 8];
            f.read_exact(&mut magic).is_ok() && &magic == MAGIC
        })
    }

    fn due(&self, meta: &DatasetMeta) -> bool {
        now_ms().saturating_sub(meta.checked_at_ms)
            >= u64::try_from(self.inner.refresh.as_millis()).unwrap_or(u64::MAX)
    }

    /// Makes sure a local copy exists and is not older than the refresh interval, downloading
    /// or revalidating it when needed.
    pub async fn ensure(&self) -> Result<DatasetOutcome, ProviderError> {
        let _gate = self.inner.gate.lock().await;
        let meta = self
            .meta()
            .filter(|m| m.url == self.inner.url && self.has_copy());
        if let Some(meta) = &meta
            && !self.due(meta)
        {
            return Ok(DatasetOutcome::Fresh);
        }
        let recent_failure = self.inner.failure.lock().unwrap().clone();
        if let Some((at, error)) = recent_failure
            && at.elapsed() < FAILURE_BACKOFF
        {
            return if meta.is_some() {
                Ok(DatasetOutcome::KeptAfterError(error))
            } else {
                Err(error)
            };
        }
        match self.refresh(meta.as_ref()).await {
            Ok(outcome) => {
                *self.inner.failure.lock().unwrap() = None;
                Ok(outcome)
            }
            Err(error) => {
                *self.inner.failure.lock().unwrap() = Some((Instant::now(), error.clone()));
                if meta.is_some() {
                    tracing::warn!(%error, "IMDb ratings dataset not refreshed; using the older copy");
                    Ok(DatasetOutcome::KeptAfterError(error))
                } else {
                    Err(error)
                }
            }
        }
    }

    /// Asks the server for a newer file, whatever the age of the copy.
    async fn refresh(&self, meta: Option<&DatasetMeta>) -> Result<DatasetOutcome, ProviderError> {
        let mut request = HttpRequest::get(PROVIDER_ID, &self.inner.url).uncached();
        if let Some(etag) = meta.and_then(|m| m.etag.as_deref()) {
            request = request.header("If-None-Match", etag);
        }
        let response = self.inner.http.send(request).await?;
        let status = response.status();
        if status == 304 {
            let Some(meta) = meta else {
                return Err(format_error("the server answered 304 without a local copy"));
            };
            self.write_meta(&DatasetMeta {
                checked_at_ms: now_ms(),
                ..meta.clone()
            })?;
            return Ok(DatasetOutcome::Unchanged);
        }
        if !(200..300).contains(&status) {
            return Err(ProviderError::Request {
                provider: DISPLAY_NAME.to_owned(),
                message: format!("HTTP {status}"),
            });
        }
        let etag = response.etag().map(str::to_owned);
        let bytes = response.body().to_vec();
        let sha256 = hex_sha256(&bytes);
        if let Some(meta) = meta
            && meta.sha256 == sha256
        {
            self.write_meta(&DatasetMeta {
                etag,
                checked_at_ms: now_ms(),
                ..meta.clone()
            })?;
            return Ok(DatasetOutcome::Unchanged);
        }
        let records = tokio::task::spawn_blocking(move || convert(&bytes))
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))??;
        let rows = records.len() / RECORD;
        std::fs::create_dir_all(&self.inner.dir).map_err(cache_error)?;
        write_atomic(&self.data_path(), &[MAGIC.as_slice(), &records].concat())?;
        let now = now_ms();
        self.write_meta(&DatasetMeta {
            url: self.inner.url.clone(),
            etag,
            sha256,
            checked_at_ms: now,
            downloaded_at_ms: now,
            rows,
        })?;
        Ok(DatasetOutcome::Updated)
    }

    fn write_meta(&self, meta: &DatasetMeta) -> Result<(), ProviderError> {
        std::fs::create_dir_all(&self.inner.dir).map_err(cache_error)?;
        let json =
            serde_json::to_vec_pretty(meta).map_err(|e| ProviderError::Other(e.to_string()))?;
        write_atomic(&self.meta_path(), &json)
    }

    /// The IMDb rating of `imdb_id` in the local copy, without any request. `None` when the
    /// title is not in the dataset or there is no copy.
    pub fn lookup(&self, imdb_id: &str) -> Option<Rating> {
        let lower = imdb_id.trim().to_ascii_lowercase();
        let number: u32 = lower.strip_prefix("tt")?.parse().ok()?;
        let mut file = File::open(self.data_path()).ok()?;
        let length = file.metadata().ok()?.len();
        let header = MAGIC.len() as u64;
        if length < header {
            return None;
        }
        let width = RECORD as u64;
        let (mut low, mut high) = (0u64, (length - header) / width);
        let mut record = [0u8; RECORD];
        while low < high {
            let middle = low + (high - low) / 2;
            file.seek(SeekFrom::Start(header + middle * width)).ok()?;
            file.read_exact(&mut record).ok()?;
            let id = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
            match id.cmp(&number) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => {
                    let tenths = u16::from_le_bytes([record[4], record[5]]);
                    let votes = u32::from_le_bytes([record[6], record[7], record[8], record[9]]);
                    return Some(Rating::new(
                        cv_domain::rating_key::IMDB,
                        f32::from(tenths) / 10.0,
                        votes,
                        10,
                    ));
                }
            }
        }
        None
    }
}

/// An HTTP client for the download: no disk cache and a generous timeout.
pub fn download_client() -> Result<HttpClient, ProviderError> {
    HttpClient::builder()
        .timeout(Duration::from_secs(300))
        // The ratings file is far larger than an API answer.
        .max_body_bytes(256 * 1024 * 1024)
        .provider_limits(
            PROVIDER_ID,
            ProviderLimits::new(1, 1, Duration::from_secs(10)).named(DISPLAY_NAME),
        )
        .build()
}

fn format_error(message: &str) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message: message.to_owned(),
    }
}

fn cache_error(error: std::io::Error) -> ProviderError {
    ProviderError::Other(format!(
        "the {DISPLAY_NAME} cache could not be used: {error}"
    ))
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

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ProviderError> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, bytes).map_err(cache_error)?;
    std::fs::rename(&temporary, path).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        cache_error(e)
    })
}

/// Converts the downloaded file (gzip or plain text) to the sorted record bytes.
fn convert(bytes: &[u8]) -> Result<Vec<u8>, ProviderError> {
    let mut text = String::new();
    if bytes.starts_with(&[0x1f, 0x8b]) {
        GzDecoder::new(bytes)
            .read_to_string(&mut text)
            .map_err(|e| format_error(&format!("the file is not a valid gzip file: {e}")))?;
    } else {
        text = String::from_utf8_lossy(bytes).into_owned();
    }
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").trim().split('\t').collect();
    if header.len() < 3 || header[0] != "tconst" || header[1] != "averageRating" {
        return Err(format_error(
            "the file is not the IMDb ratings dataset (unexpected header)",
        ));
    }
    let mut rows: Vec<(u32, u16, u32)> = lines.filter_map(parse_line).collect();
    if rows.is_empty() {
        return Err(format_error("the IMDb ratings dataset holds no ratings"));
    }
    rows.sort_unstable_by_key(|r| r.0);
    rows.dedup_by_key(|r| r.0);
    let mut out = Vec::with_capacity(rows.len() * RECORD);
    for (id, tenths, votes) in rows {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&tenths.to_le_bytes());
        out.extend_from_slice(&votes.to_le_bytes());
    }
    Ok(out)
}

fn parse_line(line: &str) -> Option<(u32, u16, u32)> {
    let mut parts = line.split('\t');
    let id: u32 = parts.next()?.strip_prefix("tt")?.parse().ok()?;
    let rating: f32 = parts.next()?.trim().parse().ok()?;
    let votes: u32 = parts.next()?.trim().parse().ok()?;
    if !(0.0..=10.0).contains(&rating) {
        return None;
    }
    Some((id, (rating * 10.0).round() as u16, votes))
}

#[async_trait]
impl Provider for ImdbRatingsDataset {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn name(&self) -> &'static str {
        DISPLAY_NAME
    }
}

#[async_trait]
impl RatingProvider for ImdbRatingsDataset {
    async fn ratings(
        &self,
        _kind: MediaKind,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        let id = imdb_id_of(ids).ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?;
        self.ensure().await?;
        let rating = self
            .lookup(&id)
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?;
        let mut out = Ratings::new();
        out.insert(rating);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_become_sorted_records() {
        let text = "tconst\taverageRating\tnumVotes\ntt0000002\t5.6\t300\ntt0000001\t5.7\t2100\nbad\tx\ty\ntt0000003\t11.0\t1\n";
        let bytes = convert(text.as_bytes()).unwrap();
        assert_eq!(bytes.len(), 2 * RECORD);
        assert_eq!(&bytes[0..4], &1u32.to_le_bytes());
        assert_eq!(&bytes[4..6], &57u16.to_le_bytes());
        assert_eq!(&bytes[10..14], &2u32.to_le_bytes());
    }

    #[test]
    fn a_foreign_file_is_refused() {
        assert!(convert(b"<html>nope</html>").is_err());
        assert!(convert(b"tconst\taverageRating\tnumVotes\n").is_err());
    }
}

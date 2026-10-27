//! The on-disk response cache with forced TTLs.
//!
//! The lifetime of an entry depends on the kind of request (15 minutes for API responses, one
//! day for scraped HTML by default), never on the server's own `Cache-Control`. Entries are keyed
//! by a hash of the request identity, which leaves out credentials, so changing a key does not
//! orphan the cache and a credential never reaches the disk.
//!
//! File layout under the root: `<provider>/<first two hash chars>/<hash>.bin`, holding
//! `CVH1`, a little-endian `u32` metadata length, the metadata as JSON, then the body. Files are
//! written to a temporary name and renamed, so a crash leaves no half-written entry; a file that
//! cannot be read back is treated as a miss and removed.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const MAGIC: &[u8; 4] = b"CVH1";

/// What kind of response a request expects, which decides how long it stays cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheKind {
    /// A JSON/XML API response.
    Api,
    /// A scraped HTML page.
    Html,
    /// Never cached (for example a POST, or a response that must always be fresh).
    None,
}

/// How long each kind of response is kept. Both values are configurable in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachePolicy {
    pub api_ttl: Duration,
    pub html_ttl: Duration,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            api_ttl: Duration::from_secs(15 * 60),
            html_ttl: Duration::from_secs(24 * 60 * 60),
        }
    }
}

impl CachePolicy {
    /// The lifetime for `kind`, or `None` when that kind is not cached.
    #[must_use]
    pub fn ttl(&self, kind: CacheKind) -> Option<Duration> {
        match kind {
            CacheKind::Api => Some(self.api_ttl),
            CacheKind::Html => Some(self.html_ttl),
            CacheKind::None => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Meta {
    stored_at_ms: u64,
    status: u16,
    content_type: Option<String>,
    kind: CacheKind,
    /// The request URL with credentials already masked. Diagnostic only.
    url: String,
}

/// A response read back from disk.
#[derive(Debug, Clone)]
pub(crate) struct CachedResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// How much room the cached responses take.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiskUsage {
    pub entries: u64,
    pub bytes: u64,
}

/// What to store for a response.
pub(crate) struct NewEntry<'a> {
    pub kind: CacheKind,
    pub status: u16,
    pub content_type: Option<&'a str>,
    pub redacted_url: &'a str,
    pub body: &'a [u8],
}

/// The cache directory for one app run.
#[derive(Debug, Clone)]
pub struct DiskCache {
    root: PathBuf,
}

fn now_ms(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Provider ids are folder names; keep them to a safe alphabet.
fn folder_name(provider: &str) -> String {
    let cleaned: String = provider
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "_".to_owned()
    } else {
        cleaned
    }
}

impl DiskCache {
    /// A cache stored under `root` (created on first write).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root folder.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn entry_path(&self, provider: &str, key: &str) -> PathBuf {
        let shard = key.get(..2).unwrap_or("00");
        self.root
            .join(folder_name(provider))
            .join(shard)
            .join(format!("{key}.bin"))
    }

    /// The entry for `key` if it exists and is younger than the policy's TTL for its kind.
    /// Expired and unreadable entries are removed.
    pub(crate) async fn get(
        &self,
        provider: &str,
        key: &str,
        policy: &CachePolicy,
        now: SystemTime,
    ) -> Option<CachedResponse> {
        let path = self.entry_path(provider, key);
        let bytes = tokio::fs::read(&path).await.ok()?;
        let Some((meta, body)) = decode(&bytes) else {
            let _ = tokio::fs::remove_file(&path).await;
            return None;
        };
        let fresh = policy
            .ttl(meta.kind)
            .is_some_and(|ttl| is_fresh(meta.stored_at_ms, ttl, now));
        if !fresh {
            let _ = tokio::fs::remove_file(&path).await;
            return None;
        }
        Some(CachedResponse {
            status: meta.status,
            content_type: meta.content_type,
            body: body.to_vec(),
        })
    }

    /// Stores an entry. Failures are logged by the caller; a cache that cannot be written must
    /// never fail the request.
    pub(crate) async fn put(
        &self,
        provider: &str,
        key: &str,
        entry: &NewEntry<'_>,
        now: SystemTime,
    ) -> io::Result<()> {
        let path = self.entry_path(provider, key);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let meta = Meta {
            stored_at_ms: now_ms(now),
            status: entry.status,
            content_type: entry.content_type.map(str::to_owned),
            kind: entry.kind,
            url: entry.redacted_url.to_owned(),
        };
        let meta_json = serde_json::to_vec(&meta).map_err(io::Error::other)?;
        let meta_len = u32::try_from(meta_json.len()).map_err(io::Error::other)?;
        let mut data = Vec::with_capacity(8 + meta_json.len() + entry.body.len());
        data.extend_from_slice(MAGIC);
        data.extend_from_slice(&meta_len.to_le_bytes());
        data.extend_from_slice(&meta_json);
        data.extend_from_slice(entry.body);

        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        tokio::fs::write(&tmp, &data).await?;
        if let Err(err) = tokio::fs::rename(&tmp, &path).await {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(err);
        }
        Ok(())
    }

    /// Removes the entry for `key`, if there is one.
    pub(crate) async fn remove(&self, provider: &str, key: &str) -> io::Result<()> {
        match tokio::fs::remove_file(self.entry_path(provider, key)).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Removes every expired or unreadable entry; returns how many files were removed.
    pub async fn purge_expired(&self, policy: &CachePolicy, now: SystemTime) -> usize {
        let mut removed = 0;
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let Ok(file_type) = entry.file_type().await else {
                    continue;
                };
                if file_type.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "bin") {
                    let keep = match tokio::fs::read(&path).await {
                        Ok(bytes) => decode(&bytes).is_some_and(|(meta, _)| {
                            policy
                                .ttl(meta.kind)
                                .is_some_and(|ttl| is_fresh(meta.stored_at_ms, ttl, now))
                        }),
                        Err(_) => true,
                    };
                    if !keep && tokio::fs::remove_file(&path).await.is_ok() {
                        removed += 1;
                    }
                }
            }
        }
        removed
    }

    /// How many cached responses there are and how many bytes they take (Settings shows it).
    pub async fn usage(&self) -> DiskUsage {
        let mut usage = DiskUsage::default();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let Ok(file_type) = entry.file_type().await else {
                    continue;
                };
                if file_type.is_dir() {
                    stack.push(entry.path());
                } else if entry.path().extension().is_some_and(|e| e == "bin") {
                    usage.entries += 1;
                    usage.bytes += entry.metadata().await.map_or(0, |m| m.len());
                }
            }
        }
        usage
    }

    /// Deletes the cached responses of one provider, or of all when `provider` is `None`.
    pub async fn clear(&self, provider: Option<&str>) -> io::Result<()> {
        let target = match provider {
            Some(id) => self.root.join(folder_name(id)),
            None => self.root.clone(),
        };
        match tokio::fs::remove_dir_all(&target).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }
}

fn is_fresh(stored_at_ms: u64, ttl: Duration, now: SystemTime) -> bool {
    let age_ms = now_ms(now).saturating_sub(stored_at_ms);
    u128::from(age_ms) < ttl.as_millis()
}

fn decode(bytes: &[u8]) -> Option<(Meta, &[u8])> {
    let rest = bytes.strip_prefix(MAGIC)?;
    let (len_bytes, rest) = rest.split_first_chunk::<4>()?;
    let meta_len = usize::try_from(u32::from_le_bytes(*len_bytes)).ok()?;
    if rest.len() < meta_len {
        return None;
    }
    let (meta_json, body) = rest.split_at(meta_len);
    let meta: Meta = serde_json::from_slice(meta_json).ok()?;
    Some((meta, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(body: &[u8], kind: CacheKind) -> NewEntry<'_> {
        NewEntry {
            kind,
            status: 200,
            content_type: Some("application/json"),
            redacted_url: "https://example.test/x?api_key=[REDACTED]",
            body,
        }
    }

    #[tokio::test]
    async fn round_trips_and_expires_by_kind() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let policy = CachePolicy {
            api_ttl: Duration::from_secs(60),
            html_ttl: Duration::from_secs(3600),
        };
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

        cache
            .put("tmdb", "aabb", &entry(b"{}", CacheKind::Api), t0)
            .await
            .unwrap();
        cache
            .put("imdb", "ccdd", &entry(b"<html>", CacheKind::Html), t0)
            .await
            .unwrap();

        let hit = cache
            .get("tmdb", "aabb", &policy, t0 + Duration::from_secs(59))
            .await
            .unwrap();
        assert_eq!(hit.body, b"{}");
        assert_eq!(hit.status, 200);
        assert_eq!(hit.content_type.as_deref(), Some("application/json"));

        let later = t0 + Duration::from_secs(61);
        assert!(cache.get("tmdb", "aabb", &policy, later).await.is_none());
        assert!(
            cache.get("imdb", "ccdd", &policy, later).await.is_some(),
            "html entries live longer than api entries"
        );
        assert!(
            cache
                .get("imdb", "ccdd", &policy, t0 + Duration::from_secs(3601))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn ttl_follows_the_current_policy() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        cache
            .put("tmdb", "aabb", &entry(b"x", CacheKind::Api), t0)
            .await
            .unwrap();
        let later = t0 + Duration::from_secs(120);
        let short = CachePolicy {
            api_ttl: Duration::from_secs(60),
            ..CachePolicy::default()
        };
        let long = CachePolicy {
            api_ttl: Duration::from_secs(600),
            ..CachePolicy::default()
        };
        assert!(cache.get("tmdb", "aabb", &long, later).await.is_some());
        assert!(cache.get("tmdb", "aabb", &short, later).await.is_none());
    }

    #[tokio::test]
    async fn corrupt_entries_are_a_miss_and_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let policy = CachePolicy::default();
        let now = SystemTime::now();
        cache
            .put("tmdb", "aabb", &entry(b"hello", CacheKind::Api), now)
            .await
            .unwrap();
        let path = cache.entry_path("tmdb", "aabb");
        std::fs::write(&path, b"CVH1garbage").unwrap();
        assert!(cache.get("tmdb", "aabb", &policy, now).await.is_none());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn purge_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let policy = CachePolicy {
            api_ttl: Duration::from_secs(60),
            html_ttl: Duration::from_secs(3600),
        };
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        cache
            .put("tmdb", "aabb", &entry(b"1", CacheKind::Api), t0)
            .await
            .unwrap();
        cache
            .put("tmdb", "eeff", &entry(b"2", CacheKind::Html), t0)
            .await
            .unwrap();
        cache
            .put("fanart", "1122", &entry(b"3", CacheKind::Api), t0)
            .await
            .unwrap();

        let removed = cache
            .purge_expired(&policy, t0 + Duration::from_secs(120))
            .await;
        assert_eq!(removed, 2);
        assert!(
            cache
                .get("tmdb", "eeff", &policy, t0 + Duration::from_secs(120))
                .await
                .is_some()
        );

        cache.clear(Some("tmdb")).await.unwrap();
        assert!(cache.get("tmdb", "eeff", &policy, t0).await.is_none());
        cache.clear(None).await.unwrap();
        cache.clear(None).await.unwrap();
    }

    #[tokio::test]
    async fn usage_counts_entries_and_clear_empties_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().join("http"));
        assert_eq!(cache.usage().await, DiskUsage::default(), "no folder yet");
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        cache
            .put("tmdb", "aabb", &entry(b"12345", CacheKind::Api), t0)
            .await
            .unwrap();
        cache
            .put("fanart", "1122", &entry(b"678", CacheKind::Html), t0)
            .await
            .unwrap();
        let usage = cache.usage().await;
        assert_eq!(usage.entries, 2);
        assert!(usage.bytes >= 8);

        cache.clear(None).await.unwrap();
        assert_eq!(cache.usage().await, DiskUsage::default());
    }

    #[test]
    fn provider_ids_become_safe_folder_names() {
        assert_eq!(folder_name("TMDB"), "tmdb");
        assert_eq!(folder_name("../evil"), "___evil");
        assert_eq!(folder_name(""), "_");
    }
}

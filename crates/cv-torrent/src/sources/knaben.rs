//! Knaben: text search over many public trackers.
//!
//! `GET {base}/v2/search?q={text}&sf=title&o=seeders&d=desc&s=300`. The v2 answer (recorded in
//! `tests/fixtures/knaben/search.json`) is `{"hits": [...]}` with, per hit, `title`, `hash`,
//! `magnetUrl`, `bytes`, `seeders`, `peers`, `date` (RFC 3339), `tracker` and `category`.
//! The adult, unsafe and dead filters stay at the site's defaults (off). The v1 field names
//! (`title`, `hash`, `magnetUrl`, `bytes`, `seeders`, `peers`, `date`) are the same as v2's;
//! `liveSeeders` and `livePeers` are read only when the plain counts are missing.
//!
//! The newest releases for the feed come from the v1 interface, which can list without search
//! words: `POST {base}/v1` with `order_by: date`, the movie (`3000000`) and TV (`2000000`)
//! categories, `hide_xxx` and `hide_unsafe` on, and `from`/`size` paging (100 a page).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cv_providers::http::HttpClient;
use serde::Deserialize;

use super::{flexible_u32, flexible_u64, run_text_queries};
use crate::latest::{FeedKind, FeedSupport, LatestAnswer};
use crate::magnet::{normalise_info_hash, validate_magnet};
use crate::model::{RawRelease, SourceError, SourceId, SourceKind, Support, TorrentQuery};
use crate::source::{SearchContext, SourceHttp, TorrentSource};

/// The id of this source.
pub const ID: &str = "knaben";
/// Hits asked for per query.
pub const HITS_PER_QUERY: &str = "300";
/// Releases asked for per page of the newest releases.
pub const LATEST_PAGE_SIZE: u32 = 100;
/// Knaben's movie category.
pub const MOVIE_CATEGORY: u64 = 3_000_000;
/// Knaben's TV category.
pub const TV_CATEGORY: u64 = 2_000_000;

/// The body of a request for one page of the newest releases of `kind`.
#[must_use]
pub fn latest_body(kind: FeedKind, page: u32) -> serde_json::Value {
    let mut categories = Vec::new();
    if kind.wants_movies() {
        categories.push(MOVIE_CATEGORY);
    }
    if kind.wants_tv() {
        categories.push(TV_CATEGORY);
    }
    serde_json::json!({
        "order_by": "date",
        "order_direction": "desc",
        "categories": categories,
        "from": u64::from(page) * u64::from(LATEST_PAGE_SIZE),
        "size": LATEST_PAGE_SIZE,
        "hide_unsafe": true,
        "hide_xxx": true,
    })
}

#[derive(Debug, Deserialize)]
struct Answer {
    #[serde(default)]
    hits: Vec<Hit>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hit {
    title: Option<String>,
    hash: Option<String>,
    magnet_url: Option<String>,
    bytes: Option<serde_json::Value>,
    seeders: Option<serde_json::Value>,
    peers: Option<serde_json::Value>,
    live_seeders: Option<serde_json::Value>,
    live_peers: Option<serde_json::Value>,
    date: Option<String>,
    category: Option<String>,
    details: Option<String>,
}

fn read_hit(hit: Hit) -> Option<RawRelease> {
    let name = hit.title.as_deref()?.trim().to_owned();
    if name.is_empty() {
        return None;
    }
    let magnet = hit.magnet_url.filter(|m| validate_magnet(m).is_ok());
    let info_hash = hit
        .hash
        .as_deref()
        .and_then(normalise_info_hash)
        .or_else(|| {
            magnet
                .as_deref()
                .and_then(|m| validate_magnet(m).ok())
                .map(|m| m.info_hash)
        })?;
    Some(RawRelease {
        source: Some(SourceId(ID.to_owned())),
        name,
        info_hash: Some(info_hash),
        magnet,
        size: flexible_u64(hit.bytes.as_ref()).filter(|size| *size > 0),
        seeders: flexible_u32(hit.seeders.as_ref())
            .or_else(|| flexible_u32(hit.live_seeders.as_ref())),
        leechers: flexible_u32(hit.peers.as_ref())
            .or_else(|| flexible_u32(hit.live_peers.as_ref())),
        published: hit
            .date
            .as_deref()
            .and_then(|d| DateTime::parse_from_rfc3339(d).ok())
            .map(|d| d.with_timezone(&Utc)),
        category: hit.category,
        details_url: hit.details.as_deref().and_then(crate::model::web_page),
        ..RawRelease::default()
    })
}

fn read_answer(answer: Answer) -> Vec<RawRelease> {
    answer.hits.into_iter().filter_map(read_hit).collect()
}

/// The Knaben source.
pub struct KnabenSource {
    id: SourceId,
    http: SourceHttp,
    base_url: String,
}

impl KnabenSource {
    /// A source that asks `base_url` (no trailing slash) through `client`.
    #[must_use]
    pub fn new(client: HttpClient, base_url: &str) -> Self {
        Self {
            id: SourceId(ID.to_owned()),
            http: SourceHttp::new(client, ID),
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }
}

#[async_trait]
impl TorrentSource for KnabenSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn name(&self) -> &str {
        "Knaben"
    }

    fn kind(&self) -> SourceKind {
        SourceKind::BuiltIn
    }

    fn supports(&self, _query: &TorrentQuery) -> Support {
        Support::Yes
    }

    async fn search(
        &self,
        query: &TorrentQuery,
        context: &SearchContext,
    ) -> Result<Vec<RawRelease>, SourceError> {
        let url = format!("{}/v2/search", self.base_url);
        run_text_queries(query, context, |text| {
            let url = url.clone();
            async move {
                let answer: Answer = self
                    .http
                    .get_json(
                        &url,
                        &[
                            ("q", &text),
                            ("sf", "title"),
                            ("o", "seeders"),
                            ("d", "desc"),
                            ("s", HITS_PER_QUERY),
                        ],
                    )
                    .await?;
                Ok(read_answer(answer))
            }
        })
        .await
    }

    fn latest_support(&self, _kind: FeedKind) -> FeedSupport {
        FeedSupport::Yes
    }

    async fn latest(
        &self,
        kind: FeedKind,
        page: u32,
        _context: &SearchContext,
    ) -> Result<LatestAnswer, SourceError> {
        let url = format!("{}/v1", self.base_url);
        let response = self
            .http
            .post_json_raw(&url, &latest_body(kind, page))
            .await?;
        let answer: Answer = crate::source::parse_json(&response)?;
        let got = answer.hits.len();
        let rows = read_answer(answer);
        let more = got >= LATEST_PAGE_SIZE as usize;
        Ok(LatestAnswer::Page { rows, more })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(json: &str) -> Vec<RawRelease> {
        read_answer(serde_json::from_str(json).expect("test JSON"))
    }

    #[test]
    fn a_hit_is_read() {
        let found = rows(
            r#"{"hits":[{"title":"Night of the Living Dead (1968) 2160p","hash":"65FC916711647CFE3A5AB0E3718F776414A5914F","magnetUrl":"magnet:?xt=urn:btih:65FC916711647CFE3A5AB0E3718F776414A5914F&dn=x","bytes":4670776934,"seeders":51,"peers":30,"date":"2022-10-16T12:17:39+00:00","category":"Movies / UHD","tracker":"YTS"}]}"#,
        );
        assert_eq!(found.len(), 1);
        let row = &found[0];
        assert_eq!(
            row.info_hash.as_deref(),
            Some("65fc916711647cfe3a5ab0e3718f776414a5914f")
        );
        assert_eq!((row.seeders, row.leechers), (Some(51), Some(30)));
        assert_eq!(row.size, Some(4_670_776_934));
        assert!(row.magnet.is_some());
        assert_eq!(row.published.map(|d| d.timestamp()), Some(1_665_922_659));
        assert_eq!(row.category.as_deref(), Some("Movies / UHD"));
    }

    #[test]
    fn the_hash_can_come_from_the_magnet_and_counts_fall_back_to_live_ones() {
        let found = rows(
            r#"{"hits":[{"title":"X","hash":null,"magnetUrl":"magnet:?xt=urn:btih:65fc916711647cfe3a5ab0e3718f776414a5914f","liveSeeders":7,"livePeers":2}]}"#,
        );
        assert_eq!(
            found[0].info_hash.as_deref(),
            Some("65fc916711647cfe3a5ab0e3718f776414a5914f")
        );
        assert_eq!((found[0].seeders, found[0].leechers), (Some(7), Some(2)));
    }

    #[test]
    fn unusable_hits_are_dropped() {
        assert!(rows(r#"{"hits":[{"title":"","hash":"65FC916711647CFE3A5AB0E3718F776414A5914F"},{"title":"No hash"},{"title":"Bad","hash":"zz","magnetUrl":"javascript:1"}]}"#).is_empty());
        assert!(rows("{}").is_empty());
    }
}

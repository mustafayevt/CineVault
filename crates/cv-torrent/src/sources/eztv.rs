//! EZTV: TV releases by IMDb ID.
//!
//! `GET {base}/api/get-torrents?imdb_id={digits}&limit=100&page={n}`. Up to three pages are
//! read while `torrents_count` says there is more. Season and episode filtering is left to the
//! relevance check, which also shows season packs. TV only.

use async_trait::async_trait;
use chrono::DateTime;
use cv_providers::http::HttpClient;
use regex::Regex;
use serde::Deserialize;
use std::sync::LazyLock;

use super::{flexible_u16, flexible_u32, flexible_u64};
use crate::magnet::{normalise_info_hash, validate_magnet};
use crate::model::{
    MediaKind, RawRelease, SkipReason, SourceError, SourceId, SourceKind, Support, TorrentQuery,
};
use crate::source::{SearchContext, SourceHttp, TorrentSource};

/// The id of this source.
pub const ID: &str = "eztv";
/// The most pages read for one search.
pub const MAX_PAGES: u32 = 3;
/// Releases asked for per page.
pub const PAGE_SIZE: u32 = 100;

#[derive(Debug, Deserialize)]
struct Answer {
    status: Option<String>,
    status_message: Option<String>,
    torrents_count: Option<serde_json::Value>,
    #[serde(default)]
    torrents: Vec<Item>,
}

#[derive(Debug, Deserialize)]
struct Item {
    id: Option<serde_json::Value>,
    hash: Option<String>,
    filename: Option<String>,
    title: Option<String>,
    magnet_url: Option<String>,
    imdb_id: Option<String>,
    season: Option<serde_json::Value>,
    episode: Option<serde_json::Value>,
    seeds: Option<serde_json::Value>,
    peers: Option<serde_json::Value>,
    date_released_unix: Option<i64>,
    size_bytes: Option<serde_json::Value>,
}

static SITE_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*\[eztv[^\]]*\]").expect("site tag pattern"));
static EXTENSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(mkv|mp4|avi|m4v|mov|wmv|ts)$").expect("extension pattern")
});

/// The release name: the file name without its extension and the site's own tag, else the title.
fn name_of(item: &Item) -> Option<String> {
    let from_file = item.filename.as_deref().map(|f| {
        let without_tag = SITE_TAG.replace_all(f.trim(), "");
        EXTENSION.replace(&without_tag, "").trim().to_owned()
    });
    from_file.filter(|name| !name.is_empty()).or_else(|| {
        item.title
            .as_deref()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
    })
}

fn read_item(item: Item) -> Option<RawRelease> {
    let name = name_of(&item)?;
    let magnet = item
        .magnet_url
        .clone()
        .filter(|m| validate_magnet(m).is_ok());
    let info_hash = item
        .hash
        .as_deref()
        .and_then(normalise_info_hash)
        .or_else(|| {
            magnet
                .as_deref()
                .and_then(|m| validate_magnet(m).ok())
                .map(|m| m.info_hash)
        })?;
    // The site's page for the episode is `{site}/ep/{id}/`; the site address is added by the search.
    let page = flexible_u64(item.id.as_ref()).map(|id| format!("/ep/{id}/"));
    Some(RawRelease {
        details_url: page,
        source: Some(SourceId(ID.to_owned())),
        name,
        info_hash: Some(info_hash),
        magnet,
        size: flexible_u64(item.size_bytes.as_ref()),
        seeders: flexible_u32(item.seeds.as_ref()),
        leechers: flexible_u32(item.peers.as_ref()),
        published: item
            .date_released_unix
            .and_then(|secs| DateTime::from_timestamp(secs, 0)),
        imdb: item
            .imdb_id
            .map(|id| format!("tt{}", id.trim_start_matches("tt"))),
        season: flexible_u16(item.season.as_ref()).filter(|s| *s > 0),
        episode: flexible_u16(item.episode.as_ref()).filter(|e| *e > 0),
        ..RawRelease::default()
    })
}

fn check_status(answer: &Answer) -> Result<(), SourceError> {
    if answer.status.as_deref().is_some_and(|s| s != "ok") {
        return Err(SourceError::BadAnswer(
            answer
                .status_message
                .clone()
                .unwrap_or_else(|| "the site reported an error".to_owned()),
        ));
    }
    Ok(())
}

/// The EZTV source.
pub struct EztvSource {
    id: SourceId,
    http: SourceHttp,
    base_url: String,
}

impl EztvSource {
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
impl TorrentSource for EztvSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn name(&self) -> &str {
        "EZTV"
    }

    fn kind(&self) -> SourceKind {
        SourceKind::BuiltIn
    }

    fn supports(&self, query: &TorrentQuery) -> Support {
        if query.kind != MediaKind::Show {
            Support::Skip(SkipReason::TvOnly)
        } else if query.ids.imdb.is_none() {
            Support::Skip(SkipReason::NoImdbId)
        } else {
            Support::Yes
        }
    }

    async fn search(
        &self,
        query: &TorrentQuery,
        context: &SearchContext,
    ) -> Result<Vec<RawRelease>, SourceError> {
        let Some(imdb) = query.ids.imdb.as_deref() else {
            return Err(SourceError::BadAnswer("no IMDb ID".to_owned()));
        };
        let digits = imdb.trim_start_matches("tt");
        let url = format!("{}/api/get-torrents", self.base_url);
        let limit = PAGE_SIZE.to_string();
        let mut out = Vec::new();
        for page in 1..=MAX_PAGES {
            if context.is_cancelled() {
                break;
            }
            let page_number = page.to_string();
            let answer: Answer = self
                .http
                .get_json(
                    &url,
                    &[
                        ("imdb_id", digits),
                        ("limit", &limit),
                        ("page", &page_number),
                    ],
                )
                .await?;
            check_status(&answer)?;
            let total = flexible_u64(answer.torrents_count.as_ref()).unwrap_or(0);
            let got = answer.torrents.len();
            let rows: Vec<RawRelease> = answer
                .torrents
                .into_iter()
                .filter_map(read_item)
                .map(|mut row| {
                    row.details_url = row
                        .details_url
                        .map(|path| format!("{}{path}", self.base_url));
                    row
                })
                .collect();
            context.partial(rows.clone());
            out.extend(rows);
            if got == 0 || u64::from(page) * u64::from(PAGE_SIZE) >= total {
                break;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: &str) -> Item {
        serde_json::from_str(json).expect("test JSON")
    }

    #[test]
    fn an_item_is_read_with_string_numbers_and_the_site_tag_removed() {
        let row = read_item(item(
            r#"{"hash":"B8538EB0D0B929B4F64962EF79BBBD70BC7950D1","filename":"Show.S09E05.1080p.WEB.x265-GRP[eztv.re].mkv","title":"Show S09E05 1080p EZTV","imdb_id":"0098904","season":"9","episode":"5","seeds":3,"peers":"4","date_released_unix":1640120283,"size_bytes":"2782354719"}"#,
        ))
        .expect("a release");
        assert_eq!(row.name, "Show.S09E05.1080p.WEB.x265-GRP");
        assert_eq!(
            row.info_hash.as_deref(),
            Some("b8538eb0d0b929b4f64962ef79bbbd70bc7950d1")
        );
        assert_eq!((row.season, row.episode), (Some(9), Some(5)));
        assert_eq!(row.size, Some(2_782_354_719));
        assert_eq!(row.seeders, Some(3));
        assert_eq!(row.leechers, Some(4));
        assert_eq!(row.imdb.as_deref(), Some("tt0098904"));
    }

    #[test]
    fn the_title_is_the_fallback_name_and_the_magnet_the_fallback_hash() {
        let row = read_item(item(
            r#"{"title":"Show S01E01 720p","magnet_url":"magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=x","season":"0","episode":"0"}"#,
        ))
        .expect("a release");
        assert_eq!(row.name, "Show S01E01 720p");
        assert_eq!(
            row.info_hash.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert!(row.magnet.is_some());
        assert_eq!(
            (row.season, row.episode),
            (None, None),
            "zero means unknown"
        );
    }

    #[test]
    fn items_without_a_hash_or_a_name_are_dropped() {
        assert!(read_item(item(r#"{"title":"Name"}"#)).is_none());
        assert!(
            read_item(item(
                r#"{"hash":"0123456789abcdef0123456789abcdef01234567"}"#
            ))
            .is_none()
        );
        assert!(read_item(item(r#"{"hash":"bad","title":"Name"}"#)).is_none());
    }

    #[test]
    fn a_bad_magnet_is_ignored() {
        let row = read_item(item(
            r#"{"hash":"0123456789abcdef0123456789abcdef01234567","title":"N","magnet_url":"javascript:alert(1)"}"#,
        ))
        .expect("a release");
        assert_eq!(row.magnet, None);
    }
}

//! The Pirate Bay's JSON interface (apibay): text search.
//!
//! `GET {base}/q.php?q={text}&cat={ids}` with the categories 201, 207, 211 for movies and
//! 205, 208, 212 for TV. The answer is a JSON array of rows with every number as text and the
//! info-hash in upper case. "Nothing found" is one row with `id` `"0"`. No paging; about a
//! hundred rows per query.

use async_trait::async_trait;
use chrono::DateTime;
use cv_providers::http::HttpClient;
use serde::Deserialize;

use super::{flexible_u32, flexible_u64, run_text_queries};
use crate::magnet::normalise_info_hash;
use crate::model::{
    MediaKind, RawRelease, SourceError, SourceId, SourceKind, Support, TorrentQuery,
};
use crate::source::{SearchContext, SourceHttp, TorrentSource};

/// The id of this source.
pub const ID: &str = "apibay";
/// Movie categories: movies, HD movies, 4K movies.
pub const MOVIE_CATEGORIES: &str = "201,207,211";
/// TV categories: TV, HD TV, 4K TV.
pub const TV_CATEGORIES: &str = "205,208,212";

#[derive(Debug, Deserialize)]
struct Row {
    id: Option<serde_json::Value>,
    name: Option<String>,
    info_hash: Option<String>,
    seeders: Option<serde_json::Value>,
    leechers: Option<serde_json::Value>,
    size: Option<serde_json::Value>,
    added: Option<serde_json::Value>,
    category: Option<serde_json::Value>,
    imdb: Option<String>,
}

fn is_sentinel(row: &Row) -> bool {
    let id = match &row.id {
        Some(serde_json::Value::String(text)) => text.trim().to_owned(),
        Some(serde_json::Value::Number(number)) => number.to_string(),
        _ => String::new(),
    };
    id == "0"
}

fn read_row(row: Row) -> Option<RawRelease> {
    if is_sentinel(&row) {
        return None;
    }
    let name = row.name.as_deref()?.trim().to_owned();
    if name.is_empty() {
        return None;
    }
    let info_hash = normalise_info_hash(row.info_hash.as_deref()?)?;
    let id = match &row.id {
        Some(serde_json::Value::String(text)) => Some(text.trim().to_owned()),
        Some(serde_json::Value::Number(number)) => Some(number.to_string()),
        _ => None,
    };
    Some(RawRelease {
        details_url: id
            .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
            .map(|id| format!("https://thepiratebay.org/description.php?id={id}")),
        source: Some(SourceId(ID.to_owned())),
        name,
        info_hash: Some(info_hash),
        seeders: flexible_u32(row.seeders.as_ref()),
        leechers: flexible_u32(row.leechers.as_ref()),
        size: flexible_u64(row.size.as_ref()).filter(|size| *size > 0),
        published: flexible_u64(row.added.as_ref())
            .filter(|secs| *secs > 0)
            .and_then(|secs| i64::try_from(secs).ok())
            .and_then(|secs| DateTime::from_timestamp(secs, 0)),
        imdb: row.imdb.filter(|id| id.starts_with("tt")),
        category: match row.category {
            Some(serde_json::Value::String(text)) => Some(text),
            Some(serde_json::Value::Number(number)) => Some(number.to_string()),
            _ => None,
        },
        ..RawRelease::default()
    })
}

/// Rows of one answer; the "No results returned" row and unreadable rows are left out.
fn read_answer(rows: Vec<Row>) -> Vec<RawRelease> {
    rows.into_iter().filter_map(read_row).collect()
}

/// The Pirate Bay source.
pub struct ApibaySource {
    id: SourceId,
    http: SourceHttp,
    base_url: String,
}

impl ApibaySource {
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
impl TorrentSource for ApibaySource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn name(&self) -> &str {
        "The Pirate Bay"
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
        let url = format!("{}/q.php", self.base_url);
        let categories = match query.kind {
            MediaKind::Movie => MOVIE_CATEGORIES,
            MediaKind::Show => TV_CATEGORIES,
        };
        run_text_queries(query, context, |text| {
            let url = url.clone();
            async move {
                let rows: Vec<Row> = self
                    .http
                    .get_json(&url, &[("q", &text), ("cat", categories)])
                    .await?;
                Ok(read_answer(rows))
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(json: &str) -> Vec<RawRelease> {
        read_answer(serde_json::from_str(json).expect("test JSON"))
    }

    #[test]
    fn a_row_is_read_with_text_numbers() {
        let found = rows(
            r#"[{"id":"25550850","name":"Night.of.the.Living.Dead.1968.1080p","info_hash":"EA7DCDFD19AE0BE46E3BF52EF310225F7AA97855","leechers":"15","seeders":"104","size":"10575236092","num_files":"5","username":"x","added":"1539729341","status":"vip","category":"207","imdb":"tt0063350"}]"#,
        );
        assert_eq!(found.len(), 1);
        let row = &found[0];
        assert_eq!(
            row.info_hash.as_deref(),
            Some("ea7dcdfd19ae0be46e3bf52ef310225f7aa97855")
        );
        assert_eq!((row.seeders, row.leechers), (Some(104), Some(15)));
        assert_eq!(row.size, Some(10_575_236_092));
        assert_eq!(row.published.map(|d| d.timestamp()), Some(1_539_729_341));
        assert_eq!(row.imdb.as_deref(), Some("tt0063350"));
        assert_eq!(row.category.as_deref(), Some("207"));
    }

    #[test]
    fn the_no_results_row_means_nothing_found() {
        assert!(rows(
            r#"[{"id":"0","name":"No results returned","info_hash":"0000000000000000000000000000000000000000","leechers":"0","seeders":"0","num_files":"0","size":"0","username":"","added":"0","status":"member","category":"0","imdb":""}]"#
        )
        .is_empty());
        assert!(rows(r#"[{"id":0,"name":"No results returned","info_hash":"0000000000000000000000000000000000000000"}]"#).is_empty());
        assert!(rows("[]").is_empty());
    }

    #[test]
    fn unreadable_rows_are_dropped_and_empty_values_are_unknown() {
        let found = rows(
            r#"[{"id":"1","name":"","info_hash":"EA7DCDFD19AE0BE46E3BF52EF310225F7AA97855"},
                {"id":"2","name":"No hash"},
                {"id":"3","name":"Bad hash","info_hash":"zz"},
                {"id":"4","name":"Fine","info_hash":"EA7DCDFD19AE0BE46E3BF52EF310225F7AA97855","size":"0","added":"0","imdb":"","seeders":"x"}]"#,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Fine");
        assert_eq!(found[0].size, None);
        assert_eq!(found[0].published, None);
        assert_eq!(found[0].imdb, None);
        assert_eq!(found[0].seeders, None);
    }
}

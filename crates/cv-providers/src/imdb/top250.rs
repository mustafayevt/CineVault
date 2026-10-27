//! The IMDb Top 250 charts (movies and TV shows).
//!
//! The chart pages (`/chart/top/` and `/chart/toptv/`) embed their list in the same
//! `__NEXT_DATA__` block as every other page: `props.pageProps.pageData.chartTitles.edges`, each
//! entry with its position (`currentRank`) and the title (`node.id`, `node.titleText.text`).
//! Like the title pages this is best-effort: a chart that cannot be read fails with
//! [`ProviderError::FormatChanged`] and stores nothing.
//!
//! A chart is only accepted when it looks like one: every entry has a rank and an IMDb ID, ranks
//! are distinct and start at 1, and there are at least [`MIN_ENTRIES`] of them. A half-read list
//! must never reach the library, where it would clear the ranks of titles that are still on the
//! real chart.

use std::collections::BTreeSet;

use async_trait::async_trait;
use serde_json::Value;

use super::ImdbProvider;
use super::convert::is_title_id;
use super::next_data::{self, ParseError, list, number, text};
use crate::error::ProviderError;
use crate::model::RequestOptions;

/// Fewer entries than this is not a chart (the real ones have 250), so it is a format change.
pub const MIN_ENTRIES: usize = 100;

/// Which chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Top250List {
    /// The Top 250 movies.
    Movies,
    /// The Top 250 TV shows.
    Shows,
}

impl Top250List {
    fn path(self) -> &'static str {
        match self {
            Self::Movies => "/chart/top/",
            Self::Shows => "/chart/toptv/",
        }
    }
}

/// One position of a chart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Top250Entry {
    /// The position, 1 for the best.
    pub rank: u32,
    /// The IMDb title ID (`tt0111161`).
    pub imdb_id: String,
    /// The title as the chart shows it; for messages only.
    pub title: String,
}

/// Where a Top 250 list comes from. [`ImdbProvider`] is the real source; tests use a fake.
#[async_trait]
pub trait Top250Source: Send + Sync {
    /// The current chart, best first.
    async fn top250(
        &self,
        list: Top250List,
        options: &RequestOptions,
    ) -> Result<Vec<Top250Entry>, ProviderError>;
}

#[async_trait]
impl Top250Source for ImdbProvider {
    async fn top250(
        &self,
        list: Top250List,
        options: &RequestOptions,
    ) -> Result<Vec<Top250Entry>, ProviderError> {
        let request = self.request(list.path(), options);
        self.fetch(request, parse_chart).await
    }
}

/// Reads a chart page.
pub(super) fn parse_chart(html: &str) -> Result<Vec<Top250Entry>, ParseError> {
    let data = next_data::extract(html)?;
    let props = next_data::page_props(&data)?;
    let edges = list(props, "/pageData/chartTitles/edges");
    if edges.len() < MIN_ENTRIES {
        return Err(ParseError::Missing("chart entries"));
    }
    let mut entries = edges
        .iter()
        .map(entry)
        .collect::<Option<Vec<_>>>()
        .ok_or(ParseError::Missing("chart entry"))?;
    entries.sort_by_key(|e| e.rank);
    let distinct = entries
        .iter()
        .map(|e| e.rank)
        .collect::<BTreeSet<_>>()
        .len();
    if distinct != entries.len() || entries.first().map(|e| e.rank) != Some(1) {
        return Err(ParseError::Missing("chart ranks"));
    }
    Ok(entries)
}

fn entry(edge: &Value) -> Option<Top250Entry> {
    let rank = u32::try_from(number(edge, "/currentRank")?).ok()?;
    let imdb_id = text(edge, "/node/id")?.to_ascii_lowercase();
    if rank == 0 || !is_title_id(&imdb_id) {
        return None;
    }
    Some(Top250Entry {
        rank,
        imdb_id,
        title: text(edge, "/node/titleText/text")
            .unwrap_or_default()
            .to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart(count: usize, tweak: impl Fn(usize, &mut Value)) -> String {
        let edges: Vec<Value> = (1..=count)
            .map(|rank| {
                let mut edge = serde_json::json!({
                    "currentRank": rank,
                    "node": {"id": format!("tt{:07}", 100_000 + rank), "titleText": {"text": format!("Film {rank}")}}
                });
                tweak(rank, &mut edge);
                edge
            })
            .collect();
        let data = serde_json::json!({"props": {"pageProps": {"pageData": {"chartTitles": {"edges": edges}}}}});
        format!(
            r#"<html><script id="__NEXT_DATA__" type="application/json">{data}</script></html>"#
        )
    }

    #[test]
    fn a_chart_is_read_best_first() {
        let entries = parse_chart(&chart(250, |_, _| {})).unwrap();
        assert_eq!(entries.len(), 250);
        assert_eq!(entries[0].rank, 1);
        assert_eq!(entries[0].imdb_id, "tt0100001");
        assert_eq!(entries[249].rank, 250);
        assert_eq!(entries[1].title, "Film 2");
    }

    #[test]
    fn entries_in_any_order_are_sorted_by_rank() {
        let html = chart(120, |rank, edge| {
            edge["currentRank"] = serde_json::json!(121 - rank);
        });
        let entries = parse_chart(&html).unwrap();
        assert_eq!(entries[0].rank, 1);
        assert_eq!(entries[0].imdb_id, "tt0100120");
    }

    #[test]
    fn a_short_list_or_a_bad_entry_is_a_format_change() {
        assert!(parse_chart(&chart(50, |_, _| {})).is_err(), "too few");
        let bad_id = chart(120, |rank, edge| {
            if rank == 7 {
                edge["node"]["id"] = serde_json::json!("nm0000093");
            }
        });
        assert!(parse_chart(&bad_id).is_err());
        let no_rank = chart(120, |rank, edge| {
            if rank == 7 {
                edge["currentRank"] = Value::Null;
            }
        });
        assert!(parse_chart(&no_rank).is_err());
        let duplicate = chart(120, |rank, edge| {
            if rank == 7 {
                edge["currentRank"] = serde_json::json!(6);
            }
        });
        assert!(parse_chart(&duplicate).is_err());
        let not_from_one = chart(120, |rank, edge| {
            edge["currentRank"] = serde_json::json!(rank + 1);
        });
        assert!(parse_chart(&not_from_one).is_err());
    }

    #[test]
    fn a_page_without_data_is_reported() {
        assert_eq!(
            parse_chart("<html>robot check</html>"),
            Err(ParseError::NoNextData)
        );
    }
}

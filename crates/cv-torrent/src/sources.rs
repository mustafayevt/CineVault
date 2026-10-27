//! One module per torrent source, and the number readers they share.

pub mod apibay;
pub mod eztv;
pub mod indexer;
pub mod knaben;
pub mod nyaa;
pub mod torrentio;
pub mod torznab;
pub mod yts;

use std::future::Future;

use serde_json::Value;

use crate::model::{RawRelease, SourceError, TorrentQuery};
use crate::query::text_queries;
use crate::source::SearchContext;

/// Runs the text queries of a title one after another through `fetch`, passing each
/// batch on as partial rows, and returns everything found.
///
/// A query that fails ends the run: the error is returned when nothing was found yet, otherwise
/// the rows found so far are returned (a source that answered once is shown as answered).
pub(crate) async fn run_text_queries<F, Fut>(
    query: &TorrentQuery,
    context: &SearchContext,
    fetch: F,
) -> Result<Vec<RawRelease>, SourceError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<Vec<RawRelease>, SourceError>>,
{
    run_text_queries_from(text_queries(query), context, fetch).await
}

/// [`run_text_queries`] for a source that builds its own list of texts.
pub(crate) async fn run_text_queries_from<F, Fut>(
    texts: Vec<String>,
    context: &SearchContext,
    fetch: F,
) -> Result<Vec<RawRelease>, SourceError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<Vec<RawRelease>, SourceError>>,
{
    let mut out: Vec<RawRelease> = Vec::new();
    for text in texts {
        if context.is_cancelled() {
            break;
        }
        match fetch(text).await {
            Ok(rows) => {
                context.partial(rows.clone());
                out.extend(rows);
            }
            Err(error) if out.is_empty() => return Err(error),
            Err(_) => break,
        }
    }
    Ok(out)
}

/// A whole number that a site may send as a JSON number or as a string.
pub(crate) fn flexible_u64(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

pub(crate) fn flexible_u32(value: Option<&Value>) -> Option<u32> {
    flexible_u64(value).and_then(|n| u32::try_from(n).ok())
}

pub(crate) fn flexible_u16(value: Option<&Value>) -> Option<u16> {
    flexible_u64(value).and_then(|n| u16::try_from(n).ok())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn numbers_may_arrive_as_text() {
        assert_eq!(flexible_u64(Some(&json!(5))), Some(5));
        assert_eq!(flexible_u64(Some(&json!(" 7 "))), Some(7));
        assert_eq!(flexible_u64(Some(&json!("x"))), None);
        assert_eq!(flexible_u64(Some(&json!(-1))), None);
        assert_eq!(flexible_u64(Some(&json!(1.5))), None);
        assert_eq!(flexible_u64(None), None);
        assert_eq!(flexible_u32(Some(&json!("99999999999"))), None);
        assert_eq!(flexible_u16(Some(&json!("70000"))), None);
    }
}

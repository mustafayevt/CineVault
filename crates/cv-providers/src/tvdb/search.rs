//! Search and ID lookup: the ID-first cascade.

use cv_domain::{ProviderIds, id_key};

use super::api::{RemoteMatch, SearchItem};
use super::client::Params;
use super::convert::{Context, hit_from_extended, hit_from_search};
use super::language::language_chain;
use super::{DISPLAY_NAME, TvdbProvider};
use crate::error::ProviderError;
use crate::model::{MediaKind, RequestOptions, SearchHit, SearchQuery};

fn not_found() -> ProviderError {
    ProviderError::NotFound(DISPLAY_NAME.to_owned())
}

/// Splits a trailing `(yyyy)` off a title: `Doctor Who (2005)` is `Doctor Who` and 2005.
/// Titles without one come back unchanged.
pub(super) fn split_parenthesised_year(title: &str) -> (&str, Option<u16>) {
    let trimmed = title.trim();
    let Some(open) = trimmed.rfind('(') else {
        return (trimmed, None);
    };
    let Some(inner) = trimmed[open + 1..].strip_suffix(')') else {
        return (trimmed, None);
    };
    let inner = inner.trim();
    let head = trimmed[..open].trim_end();
    if inner.len() == 4
        && inner.chars().all(|c| c.is_ascii_digit())
        && !head.is_empty()
        && let Ok(year) = inner.parse::<u16>()
    {
        return (head, Some(year));
    }
    (trimmed, None)
}

/// Whether `value` can stand in a URL path as a remote ID (letters and digits only).
fn is_path_safe(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric())
}

impl TvdbProvider {
    /// Searches for a title. Known IDs are tried first and settle the search when one is found:
    ///
    /// 1. A TVDB ID reads the record directly.
    /// 2. An IMDb ID goes through `search/remoteid`.
    ///
    /// A hit found by ID is marked `exact_id`. Otherwise the title is searched as text; a
    /// trailing `(year)` is taken off the title (and used as the year when none was given), and
    /// a search that finds nothing is repeated without the year.
    pub async fn search_with_ids(
        &self,
        query: &SearchQuery,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        if let Some(hit) = self.lookup_by_ids(query.kind, ids, options).await? {
            return Ok(vec![hit]);
        }
        let (title, parenthesised) = split_parenthesised_year(&query.title);
        if title.is_empty() {
            return Ok(Vec::new());
        }
        let year = query.year.filter(|y| *y > 0).or(parenthesised);
        let mut hits = self.text_search(query.kind, title, year, options).await?;
        if hits.is_empty() && year.is_some() {
            hits = self.text_search(query.kind, title, None, options).await?;
        }
        Ok(hits)
    }

    /// The title for the first ID the provider can resolve: TVDB, then IMDb.
    pub(super) async fn lookup_by_ids(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        let chain = language_chain(&options.language, &options.fallback_language);
        let ctx = Context {
            options,
            chain: &chain,
        };
        if let Some(id) = ids.get_u64(id_key::TVDB)
            && let Some(ext) = self.extended_optional(kind, id, options).await?
        {
            return Ok(Some(hit_from_extended(&ctx, &ext, kind)));
        }
        if let Some(imdb) = ids.get(id_key::IMDB)
            && let Some(id) = self.remote_id(kind, imdb, options).await?
            && let Some(ext) = self.extended_optional(kind, id, options).await?
        {
            return Ok(Some(hit_from_extended(&ctx, &ext, kind)));
        }
        Ok(None)
    }

    /// The TVDB ID for the ids: the given one, or the one `search/remoteid` resolves from IMDb.
    pub(super) async fn resolve_id(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<u64, ProviderError> {
        if let Some(id) = ids.get_u64(id_key::TVDB) {
            return Ok(id);
        }
        if let Some(imdb) = ids.get(id_key::IMDB)
            && let Some(id) = self.remote_id(kind, imdb, options).await?
        {
            return Ok(id);
        }
        Err(not_found())
    }

    /// `search/remoteid/{id}`: the series or movie TheTVDB links to an outside ID.
    async fn remote_id(
        &self,
        kind: MediaKind,
        remote: &str,
        options: &RequestOptions,
    ) -> Result<Option<u64>, ProviderError> {
        let remote = remote.trim();
        if !is_path_safe(remote) {
            return Ok(None);
        }
        let matches: Vec<RemoteMatch> = self
            .get_optional(&format!("search/remoteid/{remote}"), Vec::new(), options)
            .await?
            .unwrap_or_default();
        Ok(matches.iter().find_map(|m| {
            let record = match kind {
                MediaKind::Movie => m.movie.as_ref(),
                MediaKind::Show => m.series.as_ref(),
            };
            record.and_then(|r| r.id.as_u64())
        }))
    }

    /// `search?query=...&type=series|movie[&year=...]`.
    async fn text_search(
        &self,
        kind: MediaKind,
        title: &str,
        year: Option<u16>,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        let mut params: Params = vec![
            ("query", title.to_owned()),
            (
                "type",
                match kind {
                    MediaKind::Movie => "movie",
                    MediaKind::Show => "series",
                }
                .to_owned(),
            ),
        ];
        if let Some(year) = year {
            params.push(("year", year.to_string()));
        }
        let items: Vec<SearchItem> = self
            .get_optional("search", params, options)
            .await?
            .unwrap_or_default();
        let chain = language_chain(&options.language, &options.fallback_language);
        let mut hits: Vec<SearchHit> = Vec::new();
        for item in &items {
            let Some(hit) = hit_from_search(item, &chain) else {
                continue;
            };
            if !hits
                .iter()
                .any(|h| h.ids.get(id_key::TVDB) == hit.ids.get(id_key::TVDB))
            {
                hits.push(hit);
            }
        }
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parenthesised_year_is_split_off() {
        assert_eq!(
            split_parenthesised_year("Doctor Who (2005)"),
            ("Doctor Who", Some(2005))
        );
        assert_eq!(
            split_parenthesised_year("  Heat ( 1995 ) "),
            ("Heat", Some(1995))
        );
    }

    #[test]
    fn other_parentheses_are_left_alone() {
        assert_eq!(split_parenthesised_year("Heat"), ("Heat", None));
        assert_eq!(split_parenthesised_year("(2005)"), ("(2005)", None));
        assert_eq!(
            split_parenthesised_year("Planet (Earth) Two"),
            ("Planet (Earth) Two", None)
        );
        assert_eq!(split_parenthesised_year("Show (US)"), ("Show (US)", None));
        assert_eq!(
            split_parenthesised_year("Show (12345)"),
            ("Show (12345)", None)
        );
    }

    #[test]
    fn remote_ids_must_be_path_safe() {
        assert!(is_path_safe("tt0903747"));
        assert!(!is_path_safe("tt09/../x"));
        assert!(!is_path_safe(""));
    }
}

//! Search and ID lookup: the ID-first cascade.

use cv_domain::{ProviderIds, id_key};

use super::api::{FindResponse, MovieDetails, Page, SearchResult, Text, TvDetails};
use super::client::{Params, language_params};
use super::convert::{hit_year, image_url};
use super::{DISPLAY_NAME, PROVIDER_ID, TmdbProvider};
use crate::error::ProviderError;
use crate::model::{MediaKind, RequestOptions, SearchHit, SearchQuery};

/// Text search stops after this many pages (100 results).
const MAX_PAGES: u32 = 5;

/// Poster size used for search results.
const HIT_POSTER_SIZE: &str = "w342";

fn not_found() -> ProviderError {
    ProviderError::NotFound(DISPLAY_NAME.to_owned())
}

/// The query without a trailing four-digit year (`Heat 1995` is `Heat`), if it has one.
pub(super) fn strip_trailing_year(query: &str) -> Option<&str> {
    let trimmed = query.trim_end();
    let start = trimmed.char_indices().rev().nth(3)?.0;
    let (head, tail) = trimmed.split_at(start);
    if tail.chars().count() != 4 || !tail.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let head_trimmed = head.trim_end();
    // The year must be a separate word: whitespace between it and the rest of the title.
    (head_trimmed.len() < head.len() && !head_trimmed.is_empty()).then_some(head_trimmed)
}

impl TmdbProvider {
    /// Searches for a title. Known IDs are tried first and settle the search when one is found.
    ///
    /// 1. A TMDB ID reads the title directly.
    /// 2. An IMDb ID (or, for shows, a TVDB ID) goes through `find`.
    ///
    /// A hit found by ID is marked `exact_id`: it is the requested title, so matching scores it
    /// 1.0. Otherwise the title is searched as text, up to five pages:
    ///
    /// 3. A query of only digits is also tried as a TMDB ID; that hit leads the list but is not
    ///    `exact_id`, because a number can be part of a title.
    /// 4. The text search, with the year when there is one.
    /// 5. If that finds nothing and the title ends in a year (`Heat 1995`), the search is
    ///    repeated without it.
    pub async fn search_with_ids(
        &self,
        query: &SearchQuery,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        if let Some(hit) = self.lookup_by_ids(query.kind, ids, options).await? {
            return Ok(vec![hit]);
        }

        let title = query.title.trim();
        let mut hits = Vec::new();
        if !title.is_empty()
            && title.chars().all(|c| c.is_ascii_digit())
            && let Ok(id) = title.parse::<u64>()
            && let Some(mut hit) = self.hit_by_tmdb_id(query.kind, id, options).await?
        {
            hit.exact_id = false;
            hits.push(hit);
        }
        if title.is_empty() {
            return Ok(hits);
        }

        let mut found = self
            .text_search(query.kind, title, query.year, options)
            .await?;
        if found.is_empty()
            && hits.is_empty()
            && let Some(stripped) = strip_trailing_year(title)
        {
            found = self
                .text_search(query.kind, stripped, query.year, options)
                .await?;
        }
        for hit in found {
            if !hits
                .iter()
                .any(|h: &SearchHit| h.ids.get(id_key::TMDB) == hit.ids.get(id_key::TMDB))
            {
                hits.push(hit);
            }
        }
        Ok(hits)
    }

    /// The title for the first ID the provider can resolve: TMDB, then IMDb, then (shows) TVDB.
    pub(super) async fn lookup_by_ids(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        if let Some(id) = ids.get_u64(id_key::TMDB)
            && let Some(hit) = self.hit_by_tmdb_id(kind, id, options).await?
        {
            return Ok(Some(hit));
        }
        if let Some(imdb) = ids.get(id_key::IMDB)
            && let Some(hit) = self.find(kind, "imdb_id", imdb, options).await?
        {
            return Ok(Some(hit));
        }
        if kind == MediaKind::Show
            && let Some(tvdb) = ids.get(id_key::TVDB)
            && let Some(hit) = self.find(kind, "tvdb_id", tvdb, options).await?
        {
            return Ok(Some(hit));
        }
        Ok(None)
    }

    /// The TMDB ID for the ids: the given one, or the one `find` resolves from IMDb or TVDB.
    pub(super) async fn resolve_id(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<u64, ProviderError> {
        if let Some(id) = ids.get_u64(id_key::TMDB) {
            return Ok(id);
        }
        // `find` does not need the details, just the ID.
        self.lookup_by_ids(kind, ids, options)
            .await?
            .and_then(|hit| hit.ids.get_u64(id_key::TMDB))
            .ok_or_else(not_found)
    }

    async fn hit_by_tmdb_id(
        &self,
        kind: MediaKind,
        id: u64,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        let image_base = self.image_base(options).await;
        let mut params = language_params(options);
        params.push(("append_to_response", "external_ids".to_owned()));
        match kind {
            MediaKind::Movie => {
                let Some(movie) = self
                    .get_optional::<MovieDetails>(&format!("movie/{id}"), params, options)
                    .await?
                else {
                    return Ok(None);
                };
                let mut ids = ProviderIds::new();
                ids.set(id_key::TMDB, movie.id.to_string());
                ids.set(id_key::IMDB, movie.imdb_id.text());
                ids.set(id_key::IMDB, movie.external_ids.imdb_id.text());
                ids.set(id_key::TVDB, movie.external_ids.tvdb_id.to_text());
                Ok(Some(SearchHit {
                    provider: PROVIDER_ID.to_owned(),
                    ids,
                    title: movie.title.text().trim().to_owned(),
                    original_title: movie.original_title.text().trim().to_owned(),
                    year: super::convert::parse_date(movie.release_date.text())
                        .and_then(|d| u16::try_from(d.year()).ok())
                        .unwrap_or(0),
                    overview: movie.overview.text().trim().to_owned(),
                    poster_url: image_url(&image_base, HIT_POSTER_SIZE, movie.poster_path.text()),
                    exact_id: true,
                    popularity: 0.0,
                    votes: 0,
                }))
            }
            MediaKind::Show => {
                let Some(show) = self
                    .get_optional::<TvDetails>(&format!("tv/{id}"), params, options)
                    .await?
                else {
                    return Ok(None);
                };
                let mut ids = ProviderIds::new();
                ids.set(id_key::TMDB, show.id.to_string());
                ids.set(id_key::IMDB, show.external_ids.imdb_id.text());
                ids.set(id_key::TVDB, show.external_ids.tvdb_id.to_text());
                Ok(Some(SearchHit {
                    provider: PROVIDER_ID.to_owned(),
                    ids,
                    title: show.name.text().trim().to_owned(),
                    original_title: show.original_name.text().trim().to_owned(),
                    year: super::convert::parse_date(show.first_air_date.text())
                        .and_then(|d| u16::try_from(d.year()).ok())
                        .unwrap_or(0),
                    overview: show.overview.text().trim().to_owned(),
                    poster_url: image_url(&image_base, HIT_POSTER_SIZE, show.poster_path.text()),
                    exact_id: true,
                    popularity: 0.0,
                    votes: 0,
                }))
            }
        }
    }

    /// `find/{value}?external_source=...`
    async fn find(
        &self,
        kind: MediaKind,
        source: &'static str,
        value: &str,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        let mut params = language_params(options);
        params.push(("external_source", source.to_owned()));
        let response: FindResponse = match self
            .get_optional(&format!("find/{}", value.trim()), params, options)
            .await?
        {
            Some(response) => response,
            None => return Ok(None),
        };
        let results = match kind {
            MediaKind::Movie => response.movie_results,
            MediaKind::Show => response.tv_results,
        };
        let Some(result) = results.into_iter().find(|r| r.id != 0) else {
            return Ok(None);
        };
        let image_base = self.image_base(options).await;
        let mut hit = self.hit_from_result(&image_base, &result);
        hit.ids.set(
            match source {
                "imdb_id" => id_key::IMDB,
                _ => id_key::TVDB,
            },
            value.trim(),
        );
        hit.exact_id = true;
        Ok(Some(hit))
    }

    pub(super) fn hit_from_result(&self, image_base: &str, result: &SearchResult) -> SearchHit {
        let mut ids = ProviderIds::new();
        ids.set(id_key::TMDB, result.id.to_string());
        let title = if result.title.text().trim().is_empty() {
            result.name.text()
        } else {
            result.title.text()
        };
        let original = if result.original_title.text().trim().is_empty() {
            result.original_name.text()
        } else {
            result.original_title.text()
        };
        SearchHit {
            provider: PROVIDER_ID.to_owned(),
            ids,
            title: title.trim().to_owned(),
            original_title: original.trim().to_owned(),
            year: hit_year(result),
            overview: result.overview.text().trim().to_owned(),
            poster_url: image_url(image_base, HIT_POSTER_SIZE, result.poster_path.text()),
            exact_id: false,
            popularity: result.popularity.unwrap_or(0.0),
            votes: result.vote_count.unwrap_or(0),
        }
    }

    /// `search/movie` or `search/tv`, page by page, until the pages run out.
    async fn text_search(
        &self,
        kind: MediaKind,
        title: &str,
        year: Option<u16>,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        let (path, year_param) = match kind {
            MediaKind::Movie => ("search/movie", "year"),
            MediaKind::Show => ("search/tv", "first_air_date_year"),
        };
        let image_base = self.image_base(options).await;
        let mut hits = Vec::new();
        for page in 1..=MAX_PAGES {
            let mut params: Params = language_params(options);
            params.push(("query", title.to_owned()));
            params.push(("page", page.to_string()));
            params.push(("include_adult", "false".to_owned()));
            if let Some(year) = year.filter(|y| *y > 0) {
                params.push((year_param, year.to_string()));
            }
            let response: Page<SearchResult> = self.get(path, params, options).await?;
            hits.extend(
                response
                    .results
                    .iter()
                    .filter(|r| r.id != 0)
                    .map(|r| self.hit_from_result(&image_base, r))
                    .filter(|h| !h.title.is_empty()),
            );
            if response.results.is_empty() || page >= response.total_pages {
                break;
            }
        }
        Ok(hits)
    }

    /// `search/collection`.
    pub(super) async fn collection_search(
        &self,
        title: &str,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        let image_base = self.image_base(options).await;
        let mut hits = Vec::new();
        for page in 1..=MAX_PAGES {
            let mut params: Params = language_params(options);
            params.push(("query", title.trim().to_owned()));
            params.push(("page", page.to_string()));
            let response: Page<SearchResult> =
                self.get("search/collection", params, options).await?;
            for result in response.results.iter().filter(|r| r.id != 0) {
                let mut hit = self.hit_from_result(&image_base, result);
                // A collection's ID lives under its own key.
                hit.ids = ProviderIds::new();
                hit.ids.set(id_key::TMDB_SET, result.id.to_string());
                if !hit.title.is_empty() {
                    hits.push(hit);
                }
            }
            if response.results.is_empty() || page >= response.total_pages {
                break;
            }
        }
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::strip_trailing_year;

    #[test]
    fn a_trailing_year_is_stripped() {
        assert_eq!(strip_trailing_year("Heat 1995"), Some("Heat"));
        assert_eq!(strip_trailing_year("The Matrix  1999 "), Some("The Matrix"));
    }

    #[test]
    fn titles_that_are_only_a_number_or_have_none_are_left_alone() {
        assert_eq!(strip_trailing_year("1917"), None);
        assert_eq!(strip_trailing_year("Heat"), None);
        assert_eq!(strip_trailing_year("Apollo13"), None);
        assert_eq!(strip_trailing_year("Heat 95"), None);
        assert_eq!(strip_trailing_year(""), None);
    }
}

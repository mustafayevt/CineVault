//! The IMDb provider: titles read from the website's own pages.
//!
//! IMDb has no free public API, so this provider reads the `__NEXT_DATA__` JSON that its
//! Next.js pages embed (see [`next_data`]). That makes it **best-effort**: it is labelled so in
//! [`Capabilities::best_effort`], and it can break whenever the site changes.
//!
//! * [`MetadataProvider::search`] reads the title results of the `find` page.
//! * [`MetadataProvider::lookup`] and [`MetadataProvider::details`] read a title page by IMDb ID
//!   (`tt` and digits); this provider has no other ID to resolve from.
//! * Titles, original titles, years, release dates, plots, taglines, genres, runtimes,
//!   certifications, countries, languages, production companies, the IMDb rating, the poster and
//!   the cast and crew come from the title page. Episode lists and ratings-only fetches belong
//!   to the rating tasks.
//! * [`ArtworkProvider::artwork`] offers the title's main poster (scored 10, so it sorts first),
//!   the portrait images of its poster index as posters and the landscape images of its
//!   still-frame index as fanart (`title/{id}/mediaindex/?contentTypes=poster` and
//!   `...=still_frame`). If one index cannot be read the other still answers; if none can, the
//!   call fails.
//!
//! **Parse failures.** A page that cannot be read (no `__NEXT_DATA__`, invalid JSON, or a
//! required value missing) fails the call with [`ProviderError::FormatChanged`]. The result is
//! built only after every required value was found, so a failed scrape returns no item and
//! callers keep every field they had. The unreadable page is also dropped from the HTML cache,
//! and a cached page that no longer parses is re-fetched once, so a bad answer never sticks.
//! A human-check page (HTTP 202 or 403) is reported as a failed request, not as a format change.
//!
//! **Top 250.** [`Top250Source`] reads the movie and TV charts (`/chart/top/`, `/chart/toptv/`) so
//! the library can store each title's chart position.
//!
//! Requests go through the shared [`HttpClient`] under the lane `imdb`, cached as HTML.

mod convert;
mod images;
mod next_data;
mod top250;

use std::time::Duration;

use async_trait::async_trait;
use cv_domain::{Item, ProviderIds, id_key};

use crate::error::{HUMAN_CHECK, ProviderError};
use crate::http::{HttpClient, HttpRequest, ProviderLimits};
use crate::model::{
    ArtworkCandidate, Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery,
};
use crate::traits::{ArtworkProvider, MetadataProvider, Provider};

pub use convert::RATING_ID;
pub use next_data::ParseError;
pub use top250::{MIN_ENTRIES as TOP250_MIN_ENTRIES, Top250Entry, Top250List, Top250Source};

use convert::{PageKind, TitlePage, is_title_id, parse_find};

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = "imdb";
/// The website the pages are read from.
pub const DEFAULT_BASE_URL: &str = "https://www.imdb.com";

const DISPLAY_NAME: &str = "IMDb";

/// The browser identity pages are requested with; the site refuses clients that do not look like
/// one.
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// How fast pages are fetched: two at a time and no more than two per second.
pub fn default_limits() -> ProviderLimits {
    ProviderLimits::new(2, 2, Duration::from_secs(1)).named(DISPLAY_NAME)
}

/// The IMDb provider. Cloning is cheap.
#[derive(Clone)]
pub struct ImdbProvider {
    http: HttpClient,
    base_url: String,
}

impl ImdbProvider {
    /// A provider for the real website. It sets the polite [`default_limits`] on the `imdb` lane.
    pub fn new(http: HttpClient) -> Self {
        http.set_provider_limits(PROVIDER_ID, default_limits());
        Self::with_base_url(http, DEFAULT_BASE_URL)
    }

    /// A provider for another server, such as a local one in tests. Lane limits are left as
    /// configured.
    pub fn with_base_url(http: HttpClient, base_url: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    fn request(&self, path: &str, options: &RequestOptions) -> HttpRequest {
        HttpRequest::get(PROVIDER_ID, &format!("{}{path}", self.base_url))
            .html()
            .header("User-Agent", BROWSER_USER_AGENT)
            .header("Accept", "text/html,application/xhtml+xml")
            .header("Accept-Language", &accept_language(options))
            .bypass_cache(options.bypass_cache)
    }

    /// Fetches a page and reads it with `parse`.
    ///
    /// * A page cached earlier that no longer parses is fetched again once.
    /// * A response that does not parse (or is a human-check page) is removed from the cache
    ///   before the error is returned.
    async fn fetch<T>(
        &self,
        mut request: HttpRequest,
        parse: impl Fn(&str) -> Result<T, ParseError>,
    ) -> Result<T, ProviderError> {
        // A site that asked for a human check a moment ago is left alone for a while.
        if self.http.human_check_pause(PROVIDER_ID) {
            return Err(request_error(format!(
                "the site {HUMAN_CHECK}; it is left alone for a few minutes"
            )));
        }
        loop {
            let response = self.http.send(request.clone()).await?;
            if matches!(response.status(), 202 | 403) {
                self.http.note_human_check(PROVIDER_ID);
            }
            let outcome = match response.status() {
                404 => Err(ProviderError::NotFound(DISPLAY_NAME.to_owned())),
                202 | 403 => Err(request_error(format!(
                    "the site {HUMAN_CHECK} (HTTP {}); try again later",
                    response.status()
                ))),
                _ if !response.is_success() => {
                    Err(request_error(format!("HTTP {}", response.status())))
                }
                _ => parse(&response.text()).map_err(|reason| {
                    tracing::warn!(%reason, "IMDb page could not be read");
                    ProviderError::FormatChanged(DISPLAY_NAME.to_owned())
                }),
            };
            match outcome {
                Ok(value) => return Ok(value),
                Err(error) => {
                    if response.is_success() {
                        self.http.forget(&request).await;
                    }
                    let retry_fresh =
                        response.from_cache() && matches!(error, ProviderError::FormatChanged(_));
                    if retry_fresh {
                        request = request.bypass_cache(true);
                        continue;
                    }
                    return Err(error);
                }
            }
        }
    }

    async fn title_page(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<TitlePage, ProviderError> {
        let id = title_id(ids).ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?;
        let request = self.request(&format!("/title/{id}/"), options);
        self.fetch(request, TitlePage::parse).await
    }
}

fn request_error(message: String) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message,
    }
}

/// The IMDb title ID in `ids`, normalised to lower case.
fn title_id(ids: &ProviderIds) -> Option<String> {
    let id = ids.get(id_key::IMDB)?.trim().to_ascii_lowercase();
    is_title_id(&id).then_some(id)
}

/// `en-US,en;q=0.9` for language `en` and country `US`.
fn accept_language(options: &RequestOptions) -> String {
    let language = options.language.trim().to_ascii_lowercase();
    let language = language.split(['-', '_']).next().unwrap_or("en");
    let language = if language.is_empty() { "en" } else { language };
    let country = options.country.trim().to_ascii_uppercase();
    if country.len() == 2 {
        format!("{language}-{country},{language};q=0.9")
    } else {
        format!("{language};q=0.9")
    }
}

#[async_trait]
impl Provider for ImdbProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn name(&self) -> &'static str {
        DISPLAY_NAME
    }
}

#[async_trait]
impl ArtworkProvider for ImdbProvider {
    fn offers(&self, _kind: MediaKind, slot: cv_domain::MediaFileType) -> bool {
        matches!(
            slot,
            cv_domain::MediaFileType::Poster | cv_domain::MediaFileType::Fanart
        )
    }

    async fn artwork(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let page = self.title_page(ids, options).await?;
        if page.kind == PageKind::Other || !page.kind.fits(kind) {
            return Err(ProviderError::NotFound(DISPLAY_NAME.to_owned()));
        }
        let path = format!("/title/{}/mediaindex/", page.id);
        let index = |content_type: &str| {
            self.request(&path, options)
                .query("contentTypes", content_type)
        };
        let posters = self.fetch(index("poster"), images::parse_index).await;
        let stills = self.fetch(index("still_frame"), images::parse_index).await;
        if let (Err(first), Err(_)) = (&posters, &stills) {
            // Nothing readable, not even the main poster's list: report the first failure.
            return Err(first.clone());
        }
        let posters = posters.unwrap_or_else(|error| {
            tracing::warn!(%error, "IMDb poster index skipped");
            Vec::new()
        });
        let stills = stills.unwrap_or_else(|error| {
            tracing::warn!(%error, "IMDb still-frame index skipped");
            Vec::new()
        });
        let main = page.primary_image();
        let mut out = images::posters(main.as_ref(), &posters, options);
        out.extend(images::fanart(&stills, options));
        Ok(out)
    }
}

#[async_trait]
impl MetadataProvider for ImdbProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: true,
            orderings: Vec::new(),
            lookup_ids: vec![id_key::IMDB],
            best_effort: true,
        }
    }

    async fn search(
        &self,
        query: &SearchQuery,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        let title = query.title.trim();
        if title.is_empty() {
            return Ok(Vec::new());
        }
        let request = self
            .request("/find/", options)
            .query("q", title)
            .query("s", "tt");
        let kind = query.kind;
        self.fetch(request, move |html| parse_find(html, kind))
            .await
    }

    async fn lookup(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        if title_id(ids).is_none() {
            return Ok(None);
        }
        match self.title_page(ids, options).await {
            Ok(page) if page.kind.fits(kind) => Ok(Some(page.build_hit())),
            Ok(_) => Ok(None),
            Err(ProviderError::NotFound(_)) => Ok(None),
            Err(other) => Err(other),
        }
    }

    async fn details(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let page = self.title_page(ids, options).await?;
        if page.kind == PageKind::Other || !page.kind.fits(kind) {
            return Err(ProviderError::NotFound(DISPLAY_NAME.to_owned()));
        }
        Ok(page.build_item(kind, options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_language_carries_language_and_country() {
        let mut options = RequestOptions::default();
        assert_eq!(accept_language(&options), "en-US,en;q=0.9");
        options.language = "tr_TR".into();
        options.country = "tr".into();
        assert_eq!(accept_language(&options), "tr-TR,tr;q=0.9");
        options.country = String::new();
        assert_eq!(accept_language(&options), "tr;q=0.9");
    }

    #[test]
    fn only_well_formed_title_ids_are_used() {
        let mut ids = ProviderIds::new();
        ids.set(id_key::IMDB, " TT0113277 ");
        assert_eq!(title_id(&ids).as_deref(), Some("tt0113277"));
        ids.set(id_key::IMDB, "nm0000093");
        assert_eq!(title_id(&ids), None);
        assert_eq!(title_id(&ProviderIds::new()), None);
    }
}

//! The OFDb provider: German film data read from the website.
//!
//! OFDb has no public API, so this provider reads its pages (see [`parse`]). That makes it
//! **best-effort**: it is labelled so in [`Capabilities::best_effort`], and a page it cannot read
//! fails with a "page format changed" error that returns nothing (see [`crate::webpage`]).
//!
//! * [`MetadataProvider::search`] posts the title to the search page and reads the result table.
//!   Series rows are left out: OFDb is a movie provider here. The search page takes a title, a
//!   person or an IMDb ID, which is how an IMDb ID is resolved.
//! * [`MetadataProvider::lookup`] and [`MetadataProvider::details`] read the film page by OFDb ID
//!   (`ofdb`), or by IMDb ID through the search page (the film page must then name the same IMDb
//!   ID). The plot on the film page is cut off with an ellipsis; the full text comes from the
//!   plot page.
//! * Titles, original titles, years, plots, genres, countries, directors, actors, the OFDb
//!   rating (ten points) and the IMDb ID come from the film page, which also names the poster
//!   and the backdrop ([`ArtworkProvider`]).
//!
//! Search answers are not cached (the search is a POST); film and plot pages are cached for a
//! day like every scraped page. Requests go through the shared [`HttpClient`] under the lane
//! `ofdb`.

mod parse;

use std::time::Duration;

use async_trait::async_trait;
use cv_domain::{Credit, CreditKind, Item, ItemDetail, MediaFileType, ProviderIds, Rating, id_key};

use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest, ProviderLimits};
use crate::model::{
    ArtworkCandidate, Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery,
};
use crate::shared::{imdb_id_of, set_id};
use crate::traits::{ArtworkProvider, MetadataProvider, Provider};
use crate::webpage::fetch_page;

use parse::{Film, SearchRow, parse_film, parse_plot_page, parse_search};

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = "ofdb";
/// The provider's ID key in [`ProviderIds`].
pub const ID_KEY: &str = "ofdb";
/// The website the pages are read from.
pub const DEFAULT_BASE_URL: &str = "https://www.ofdb.de";
/// The rating source key OFDb ratings are stored under.
pub const RATING_ID: &str = "ofdb";

const DISPLAY_NAME: &str = "OFDb";

/// The browser identity pages are requested with.
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// How fast pages are fetched: two at a time and one per second.
pub fn default_limits() -> ProviderLimits {
    ProviderLimits::new(2, 1, Duration::from_secs(1)).named(DISPLAY_NAME)
}

/// The OFDb provider. Cloning is cheap.
#[derive(Clone)]
pub struct OfdbProvider {
    http: HttpClient,
    base_url: String,
}

impl OfdbProvider {
    /// A provider for the real website. It sets the polite [`default_limits`] on the `ofdb` lane.
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

    fn get(&self, path: &str, options: &RequestOptions) -> HttpRequest {
        HttpRequest::get(PROVIDER_ID, &format!("{}{path}", self.base_url))
            .html()
            .header("User-Agent", BROWSER_USER_AGENT)
            .header("Accept", "text/html,application/xhtml+xml")
            .header("Accept-Language", "de-DE,de;q=0.9")
            .bypass_cache(options.bypass_cache)
    }

    /// The result rows for a search term (a title, or an IMDb ID).
    async fn rows(&self, term: &str) -> Result<Vec<SearchRow>, ProviderError> {
        let mut form = reqwest::Url::parse("http://form.invalid/").expect("a constant URL");
        form.query_pairs_mut().append_pair("QSinput", term);
        let body = form.query().unwrap_or_default().to_owned();
        let request = HttpRequest::post(
            PROVIDER_ID,
            &format!("{}/suchergebnis/", self.base_url),
            body.into_bytes(),
        )
        .header("User-Agent", BROWSER_USER_AGENT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept-Language", "de-DE,de;q=0.9");
        let base = self.base_url.clone();
        fetch_page(&self.http, DISPLAY_NAME, request, move |html| {
            parse_search(html, &base)
        })
        .await
    }

    /// The film page for an OFDb ID.
    async fn film(&self, id: u64, options: &RequestOptions) -> Result<Film, ProviderError> {
        let request = self.get(&format!("/film/{id}/"), options);
        let base = self.base_url.clone();
        fetch_page(&self.http, DISPLAY_NAME, request, move |html| {
            parse_film(html, &base)
        })
        .await
    }

    /// The full plot, when the film page's text is cut off and the plot page is readable. A plot
    /// page that cannot be read leaves the short text: the film is still good.
    async fn full_plot(&self, film: &Film, options: &RequestOptions) -> Option<String> {
        let link = film.plot_link.as_deref()?;
        let cut = film.plot.ends_with('…') || film.plot.ends_with("...");
        if !cut {
            return None;
        }
        // The link may be absolute; only its path is used, so the page is always read from this
        // provider's own site.
        let path = match link.split_once("://") {
            Some((_, rest)) => &rest[rest.find('/')?..],
            None => link,
        };
        let path = format!("/{}", path.trim_start_matches("./").trim_start_matches('/'));
        if !path.starts_with("/film/") || !path.contains("/plot/") {
            return None;
        }
        let request = self.get(&path, options);
        match fetch_page(&self.http, DISPLAY_NAME, request, parse_plot_page).await {
            Ok(plot) => Some(plot),
            Err(error) => {
                tracing::debug!(%error, "OFDb plot page unavailable; keeping the short plot");
                None
            }
        }
    }

    /// The OFDb ID for `ids`: the one it has, else the film the IMDb ID finds, which must name
    /// the same IMDb ID.
    async fn resolve(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<(u64, Film)>, ProviderError> {
        if let Some(id) = ids.get_u64(ID_KEY) {
            return Ok(Some((id, self.film(id, options).await?)));
        }
        let Some(imdb) = imdb_id_of(ids) else {
            return Ok(None);
        };
        let Some(row) = self.rows(&imdb).await?.into_iter().find(|r| !r.is_series) else {
            return Ok(None);
        };
        let film = self.film(row.id, options).await?;
        Ok((film.imdb.as_deref() == Some(imdb.as_str())).then_some((row.id, film)))
    }
}

fn ids_of(id: u64, imdb: Option<&str>) -> ProviderIds {
    let mut ids = ProviderIds::new();
    set_id(&mut ids, ID_KEY, id.to_string());
    if let Some(imdb) = imdb {
        set_id(&mut ids, id_key::IMDB, imdb);
    }
    ids
}

fn build_hit(id: u64, film: &Film) -> SearchHit {
    SearchHit {
        provider: PROVIDER_ID.to_owned(),
        ids: ids_of(id, film.imdb.as_deref()),
        title: film.title.clone(),
        original_title: if film.original_title.is_empty() {
            film.title.clone()
        } else {
            film.original_title.clone()
        },
        year: film.year,
        overview: film.plot.clone(),
        poster_url: film.poster.clone(),
        exact_id: true,
        popularity: 0.0,
        votes: 0,
    }
}

fn build_item(id: u64, film: &Film, plot: Option<String>) -> Item {
    let mut item = Item::movie(&film.title);
    let core = &mut item.core;
    core.ids = ids_of(id, film.imdb.as_deref());
    core.original_title = if film.original_title.is_empty() {
        film.title.clone()
    } else {
        film.original_title.clone()
    };
    core.year = film.year;
    core.plot = plot.unwrap_or_else(|| film.plot.clone());
    if let Some((value, votes)) = film.rating {
        core.ratings
            .insert(Rating::new(RATING_ID, value, votes, 10));
    }
    for name in &film.directors {
        core.credits
            .push(Credit::new(CreditKind::Director, name, ""));
    }
    for name in &film.actors {
        core.credits.push(Credit::new(CreditKind::Actor, name, ""));
    }
    if let Some(poster) = &film.poster {
        core.artwork_urls
            .insert(MediaFileType::Poster, poster.clone());
    }
    if let Some(backdrop) = &film.backdrop {
        core.artwork_urls
            .insert(MediaFileType::Fanart, backdrop.clone());
    }
    if let ItemDetail::Movie(movie) = &mut item.detail {
        movie.genres = crate::german::genres(&film.genres);
        movie.countries = crate::german::countries(&film.countries);
    }
    item
}

/// The pixel width an OFDb image path names (`film.370px`, `backdrop.870px`), `0` otherwise.
fn width_of(url: &str) -> u32 {
    url.split('/')
        .find_map(|part| {
            let digits = part.split("px").next()?;
            let width = digits.rsplit('.').next()?;
            part.contains("px")
                .then(|| width.parse::<u32>().ok())
                .flatten()
        })
        .unwrap_or(0)
}

#[async_trait]
impl Provider for OfdbProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn name(&self) -> &'static str {
        DISPLAY_NAME
    }
}

#[async_trait]
impl MetadataProvider for OfdbProvider {
    fn languages(&self) -> &'static [&'static str] {
        // German plots and genre names, whatever the request asks for.
        &["de"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: false,
            orderings: Vec::new(),
            lookup_ids: vec![ID_KEY, id_key::IMDB],
            best_effort: true,
        }
    }

    async fn search(
        &self,
        query: &SearchQuery,
        _options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        let title = query.title.trim();
        if query.kind != MediaKind::Movie || title.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .rows(title)
            .await?
            .into_iter()
            .filter(|row| !row.is_series)
            .map(|row| SearchHit {
                provider: PROVIDER_ID.to_owned(),
                ids: ids_of(row.id, None),
                original_title: row.original_title.unwrap_or_else(|| row.title.clone()),
                title: row.title,
                year: row.year,
                overview: String::new(),
                poster_url: row.poster,
                exact_id: false,
                popularity: 0.0,
                votes: 0,
            })
            .collect())
    }

    async fn lookup(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        if kind != MediaKind::Movie {
            return Ok(None);
        }
        match self.resolve(ids, options).await {
            Ok(found) => Ok(found.map(|(id, film)| build_hit(id, &film))),
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
        if kind != MediaKind::Movie {
            return Err(ProviderError::Unsupported {
                provider: DISPLAY_NAME.to_owned(),
                operation: "shows",
            });
        }
        let (id, film) = self
            .resolve(ids, options)
            .await?
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?;
        let plot = self.full_plot(&film, options).await;
        Ok(build_item(id, &film, plot))
    }
}

#[async_trait]
impl ArtworkProvider for OfdbProvider {
    fn offers(&self, kind: MediaKind, slot: cv_domain::MediaFileType) -> bool {
        kind == MediaKind::Movie
            && matches!(
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
        if kind != MediaKind::Movie {
            return Ok(Vec::new());
        }
        let Some((_, film)) = self.resolve(ids, options).await? else {
            return Ok(Vec::new());
        };
        let candidate = |kind: MediaFileType, url: &String| ArtworkCandidate {
            kind,
            url: url.clone(),
            language: Some("de".to_owned()),
            width: width_of(url),
            height: 0,
            score: 0.0,
            popularity: crate::model::Popularity::Unknown,
        };
        let mut out = Vec::new();
        if let Some(poster) = &film.poster {
            out.push(candidate(MediaFileType::Poster, poster));
        }
        if let Some(backdrop) = &film.backdrop {
            let mut fanart = candidate(MediaFileType::Fanart, backdrop);
            fanart.language = None;
            out.push(fanart);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_polite() {
        let limits = default_limits();
        assert_eq!((limits.requests, limits.max_concurrent), (1, 2));
    }

    #[test]
    fn image_paths_name_their_width() {
        assert_eq!(
            width_of("https://www.ofdb.de/images/film.370px/2/2706.jpg?v=1"),
            370
        );
        assert_eq!(
            width_of("https://www.ofdb.de/images/backdrop.870px/2/2706.jpg"),
            870
        );
        assert_eq!(width_of("https://www.ofdb.de/img/logo.png"), 0);
    }

    #[test]
    fn movies_only_and_best_effort() {
        let provider = OfdbProvider::with_base_url(
            HttpClient::builder().build().unwrap(),
            "http://127.0.0.1:1",
        );
        let capabilities = provider.capabilities();
        assert!(capabilities.movies && !capabilities.shows && capabilities.best_effort);
    }
}

//! The fernsehserien.de provider: German TV shows and TV films read from the website.
//!
//! The site has no API, so this provider reads its pages (see [`parse`]). That makes it
//! **best-effort**: it is labelled so in [`Capabilities::best_effort`], and a page it cannot read
//! fails with a "page format changed" error that returns nothing (see [`crate::webpage`]). The
//! site also sits behind a bot check; a challenge answer (HTTP 202 or 403) is reported as a
//! failed request, never as a format change.
//!
//! * [`MetadataProvider::search`] reads `suche/{query}`. When the site jumps straight to the one
//!   matching show, that page is the single hit.
//! * [`MetadataProvider::lookup`] and [`MetadataProvider::details`] read the show page
//!   `/{slug}` by the provider's ID (`fernsehserien`, the slug), and the `cast-crew` page when the
//!   show page names no cast.
//! * [`MetadataProvider::episodes`] reads the episode guide `/{slug}/episodenguide`, then the
//!   `episodenguide/staffel-{n}/{id}` page of every season the guide links to but does not list.
//!
//! Pages are cached for a day. Requests go through the shared [`HttpClient`] under the lane
//! `fernsehserien`.

mod parse;

use std::time::Duration;

use async_trait::async_trait;
use cv_domain::{
    AiredStatus, Credit, CreditKind, EpisodeFields, EpisodeGroup, EpisodeNumber, EpisodeOrdering,
    Item, ItemDetail, MediaFileType, ProviderIds, Rating,
};

use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest, ProviderLimits};
use crate::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use crate::shared::{set_id, year_of};
use crate::traits::{MetadataProvider, Provider};
use crate::webpage::fetch_page;

use parse::{
    EpisodeRow, Page, PageKind, SearchRow, parse_episodes, parse_page, parse_people, parse_search,
};

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = "fernsehserien";
/// The provider's ID key in [`ProviderIds`]: the show's path on the site, such as `tatort`.
pub const ID_KEY: &str = "fernsehserien";
/// The website the pages are read from.
pub const DEFAULT_BASE_URL: &str = "https://www.fernsehserien.de";
/// The rating source key fernsehserien.de ratings are stored under.
pub const RATING_ID: &str = "fernsehserien";

const DISPLAY_NAME: &str = "fernsehserien.de";

/// The browser identity pages are requested with.
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// How fast pages are fetched: two at a time and one per second.
pub fn default_limits() -> ProviderLimits {
    ProviderLimits::new(2, 1, Duration::from_secs(1)).named(DISPLAY_NAME)
}

/// The fernsehserien.de provider. Cloning is cheap.
#[derive(Clone)]
pub struct FernsehserienProvider {
    http: HttpClient,
    base_url: String,
}

impl FernsehserienProvider {
    /// A provider for the real website. It sets the polite [`default_limits`] on its lane.
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
            .header("Accept-Language", "de-DE,de;q=0.9")
            .bypass_cache(options.bypass_cache)
    }

    /// The path `suche/{query}` with the query escaped as one path segment.
    fn search_path(&self, query: &str) -> Result<String, ProviderError> {
        let mut url = reqwest::Url::parse(&format!("{}/", self.base_url))
            .map_err(|_| request_error("invalid base address"))?;
        url.path_segments_mut()
            .map_err(|()| request_error("invalid base address"))?
            .pop_if_empty()
            .extend(["suche", query]);
        Ok(url.path().to_owned())
    }

    /// The show or film page for a slug.
    async fn page(&self, slug: &str, options: &RequestOptions) -> Result<Page, ProviderError> {
        let request = self.request(&format!("/{slug}"), options);
        fetch_page(&self.http, DISPLAY_NAME, request, parse_page).await
    }

    /// The slug in `ids`, when it is one.
    fn slug_of(ids: &ProviderIds) -> Option<String> {
        ids.get(ID_KEY)
            .map(str::trim)
            .filter(|s| parse::is_slug(s))
            .map(str::to_owned)
    }
}

fn request_error(message: &str) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message: message.to_owned(),
    }
}

fn not_found() -> ProviderError {
    ProviderError::NotFound(DISPLAY_NAME.to_owned())
}

fn kind_fits(page: PageKind, wanted: MediaKind) -> bool {
    matches!(
        (page, wanted),
        (PageKind::Series, MediaKind::Show) | (PageKind::Movie, MediaKind::Movie)
    )
}

fn ids_of(slug: &str, imdb: Option<&str>) -> ProviderIds {
    let mut ids = ProviderIds::new();
    set_id(&mut ids, ID_KEY, slug);
    if let Some(imdb) = imdb {
        set_id(&mut ids, cv_domain::id_key::IMDB, imdb);
    }
    ids
}

fn build_hit(slug: &str, page: &Page) -> SearchHit {
    SearchHit {
        provider: PROVIDER_ID.to_owned(),
        ids: ids_of(slug, page.imdb.as_deref()),
        title: page.title.clone(),
        original_title: page.title.clone(),
        year: page.year,
        overview: page.plot.clone(),
        poster_url: page.image.clone(),
        exact_id: true,
        popularity: 0.0,
        votes: 0,
    }
}

fn credits(actors: &[String], directors: &[String]) -> Vec<Credit> {
    let mut out: Vec<Credit> = Vec::new();
    for (kind, names) in [
        (CreditKind::Director, directors),
        (CreditKind::Actor, actors),
    ] {
        for name in names {
            if !out.iter().any(|c| c.kind == kind && c.person.name == *name) {
                out.push(Credit::new(kind, name, ""));
            }
        }
    }
    out
}

fn build_item(slug: &str, page: &Page, actors: &[String], directors: &[String]) -> Item {
    let mut item = match page.kind {
        PageKind::Series => Item::show(&page.title),
        PageKind::Movie => Item::movie(&page.title),
    };
    let core = &mut item.core;
    core.ids = ids_of(slug, page.imdb.as_deref());
    core.original_title = page.title.clone();
    core.year = if page.year != 0 {
        page.year
    } else {
        year_of(page.first_aired)
    };
    core.plot = page.plot.clone();
    core.credits = credits(actors, directors);
    if let Some((value, votes, best)) = page.rating {
        core.ratings
            .insert(Rating::new(RATING_ID, value, votes, best));
    }
    if let Some(image) = &page.image {
        core.artwork_urls
            .insert(MediaFileType::Poster, image.clone());
    }
    let genres = crate::german::genres(&page.genres);
    let countries = crate::german::countries(&page.countries);
    match &mut item.detail {
        ItemDetail::Show(show) => {
            show.first_aired = page.first_aired;
            show.year_end = page.year_end;
            show.status = if page.year_end != 0 {
                AiredStatus::Ended
            } else if page.year != 0 || page.first_aired.is_some() {
                AiredStatus::Continuing
            } else {
                AiredStatus::Unknown
            };
            show.genres = genres;
            show.country = countries.into_iter().next().unwrap_or_default();
            show.episode_group = EpisodeGroup::aired();
            show.episode_groups = vec![EpisodeGroup::aired()];
        }
        ItemDetail::Movie(movie) => {
            movie.genres = genres;
            movie.countries = countries;
            movie.release_date = page.first_aired;
        }
        _ => {}
    }
    item
}

fn episode_item(row: &EpisodeRow) -> Item {
    let mut item = Item::new(ItemDetail::Episode(EpisodeFields::default()));
    item.core.title = row.title.clone();
    item.core.plot = row.plot.clone();
    item.core.year = year_of(row.aired);
    if let ItemDetail::Episode(fields) = &mut item.detail {
        fields.first_aired = row.aired;
        fields.numbers.set(EpisodeNumber::new(
            EpisodeGroup::aired(),
            row.season,
            row.number,
        ));
    }
    item
}

#[async_trait]
impl Provider for FernsehserienProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn name(&self) -> &'static str {
        DISPLAY_NAME
    }
}

#[async_trait]
impl MetadataProvider for FernsehserienProvider {
    fn languages(&self) -> &'static [&'static str] {
        &["de"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: true,
            orderings: vec![EpisodeOrdering::Aired],
            lookup_ids: vec![ID_KEY],
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
        let request = self.request(&self.search_path(title)?, options);
        let rows: Vec<SearchRow> =
            fetch_page(&self.http, DISPLAY_NAME, request, parse_search).await?;
        Ok(rows
            .into_iter()
            .map(|row| SearchHit {
                provider: PROVIDER_ID.to_owned(),
                ids: ids_of(&row.slug, None),
                title: row.title.clone(),
                original_title: row.title,
                year: row.year,
                overview: String::new(),
                poster_url: None,
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
        let Some(slug) = Self::slug_of(ids) else {
            return Ok(None);
        };
        match self.page(&slug, options).await {
            Ok(page) if kind_fits(page.kind, kind) => Ok(Some(build_hit(&slug, &page))),
            Ok(_) | Err(ProviderError::NotFound(_)) => Ok(None),
            Err(other) => Err(other),
        }
    }

    async fn details(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let slug = Self::slug_of(ids).ok_or_else(not_found)?;
        let page = self.page(&slug, options).await?;
        if !kind_fits(page.kind, kind) {
            return Err(not_found());
        }
        let (mut actors, mut directors) = (page.actors.clone(), page.directors.clone());
        if actors.is_empty() {
            // The cast page is a bonus: whatever goes wrong with it, the details stay good.
            let request = self.request(&format!("/{slug}/cast-crew"), options);
            let people = fetch_page(&self.http, DISPLAY_NAME, request, |html| {
                Ok::<_, String>(parse_people(html))
            })
            .await;
            match people {
                Ok((cast, crew)) => {
                    actors = cast;
                    if directors.is_empty() {
                        directors = crew;
                    }
                }
                Err(error) => tracing::debug!(%error, "fernsehserien.de cast page unavailable"),
            }
        }
        Ok(build_item(&slug, &page, &actors, &directors))
    }

    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        if ordering.ordering != EpisodeOrdering::Aired {
            return Err(not_found());
        }
        let slug = Self::slug_of(show).ok_or_else(not_found)?;
        let guide_path = format!("/{slug}/episodenguide");
        let request = self.request(&guide_path, options);
        // The guide page, and the season pages it links to.
        let (mut rows, links) = fetch_page(&self.http, DISPLAY_NAME, request, |html| {
            parse_episodes(html, Some(1)).map(|rows| (rows, parse::season_links(html)))
        })
        .await?;
        for (season, path) in links {
            if rows.iter().any(|r| r.season == season) {
                continue;
            }
            let request = self.request(&path, options);
            match fetch_page(&self.http, DISPLAY_NAME, request, |html| {
                parse_episodes(html, Some(season))
            })
            .await
            {
                Ok(more) => rows.extend(more.into_iter().filter(|r| r.season == season)),
                // A season page that is missing or unreadable leaves that season out; the
                // others are still right.
                Err(error) => {
                    tracing::debug!(season, %error, "fernsehserien.de season page unavailable");
                }
            }
        }
        rows.sort_by_key(|r| (r.season, r.number));
        rows.dedup_by_key(|r| (r.season, r.number));
        Ok(rows.iter().map(episode_item).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> FernsehserienProvider {
        FernsehserienProvider::with_base_url(
            HttpClient::builder().build().unwrap(),
            "http://127.0.0.1:1",
        )
    }

    #[test]
    fn the_query_is_one_escaped_path_segment() {
        let p = provider();
        assert_eq!(p.search_path("Tatort").unwrap(), "/suche/Tatort");
        assert_eq!(
            p.search_path("Die Sendung mit der Maus/Extra ?").unwrap(),
            "/suche/Die%20Sendung%20mit%20der%20Maus%2FExtra%20%3F"
        );
    }

    #[test]
    fn labelled_best_effort_for_movies_and_shows() {
        let capabilities = provider().capabilities();
        assert!(capabilities.best_effort && capabilities.movies && capabilities.shows);
        assert_eq!(capabilities.lookup_ids, vec!["fernsehserien"]);
    }

    #[test]
    fn only_slugs_are_ids() {
        let mut ids = ProviderIds::new();
        ids.set(ID_KEY, "tatort");
        assert_eq!(
            FernsehserienProvider::slug_of(&ids).as_deref(),
            Some("tatort")
        );
        ids.set(ID_KEY, "../etc/passwd");
        assert_eq!(FernsehserienProvider::slug_of(&ids), None);
    }
}

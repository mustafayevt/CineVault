//! The MDBList rating provider: one request returns the title's ratings from several sources
//! (IMDb, TMDB, Trakt, Metacritic, Rotten Tomatoes, Letterboxd, MyAnimeList, Roger Ebert).
//!
//! * Endpoint: `{base}/{provider}/{movie|show}/{id}?apikey=KEY`, where `provider` is the kind of
//!   ID in the path. The title's IDs are tried in turn until MDBList knows one of them.
//! * The key is the user's own and comes from the [`CredentialResolver`] on every request.
//! * MDBList names its sources its own way; [`rating_of`] maps them to the app's rating keys and
//!   scales (Letterboxd out of 5, Roger Ebert out of 4, percentages out of 100). Sources the app
//!   has no key for are ignored. Both of MDBList's Rotten Tomatoes audience sources (`audience`
//!   and `popcorn`) are the audience percentage, stored as `tomatometerallaudience`.

use std::sync::Arc;

use async_trait::async_trait;
use cv_domain::{ProviderIds, Rating, Ratings, id_key, rating_key};
use serde_json::Value;

use crate::catalogue;
use crate::credentials::{CredentialResolver, CredentialSchema, ResolvedCredentials};
use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest};
use crate::model::{MediaKind, RequestOptions};
use crate::traits::{Provider, RatingProvider};

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = catalogue::key::MDBLIST;
/// The API server.
pub const DEFAULT_BASE_URL: &str = "https://api.mdblist.com";

/// The rating key of Metacritic's user score (out of 10).
pub const METACRITIC_USER_RATING_ID: &str = "metacriticuser";

const DISPLAY_NAME: &str = "MDBList";

struct Inner {
    http: HttpClient,
    resolver: CredentialResolver,
    base_url: String,
}

/// The MDBList provider. Cloning is cheap.
#[derive(Clone)]
pub struct MdbListProvider {
    inner: Arc<Inner>,
}

impl MdbListProvider {
    /// A provider for the real API server.
    pub fn new(http: HttpClient, resolver: CredentialResolver) -> Self {
        Self::with_base_url(http, resolver, DEFAULT_BASE_URL)
    }

    /// A provider for another server, such as a local one in tests.
    pub fn with_base_url(http: HttpClient, resolver: CredentialResolver, base_url: &str) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                resolver,
                base_url: base_url.trim_end_matches('/').to_owned(),
            }),
        }
    }

    fn schema() -> Option<&'static CredentialSchema> {
        catalogue::CREDENTIAL_SCHEMAS
            .iter()
            .find(|s| s.provider == PROVIDER_ID)
    }

    /// One request for the title with `id` of the given `provider` kind.
    async fn title(
        &self,
        credentials: &ResolvedCredentials,
        id_provider: &str,
        kind: MediaKind,
        id: &str,
        options: &RequestOptions,
    ) -> Result<Value, ProviderError> {
        let key = credentials
            .get(catalogue::API_KEY)
            .map(|v| v.expose().trim().to_owned())
            .filter(|k| !k.is_empty())
            .ok_or_else(missing_key)?;
        let media = match kind {
            MediaKind::Movie => "movie",
            MediaKind::Show => "show",
        };
        let url = format!("{}/{id_provider}/{media}/{id}", self.inner.base_url);
        let request = HttpRequest::get(PROVIDER_ID, &url)
            .credential_query("apikey", &key)
            .bypass_cache(options.bypass_cache);
        let response = self.inner.http.send(request).await?;
        let body: Option<Value> = response.json().ok();
        let message = body
            .as_ref()
            .and_then(|b| b.get("error"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        match response.status() {
            401 | 403 => Err(ProviderError::Auth {
                provider: DISPLAY_NAME.to_owned(),
                message: message.unwrap_or_else(|| "the API key was not accepted".to_owned()),
            }
            .with_key_hint(credentials, PROVIDER_ID)),
            404 => Err(ProviderError::NotFound(DISPLAY_NAME.to_owned())),
            _ if !response.is_success() => Err(request_error(
                message.unwrap_or_else(|| format!("HTTP {}", response.status())),
            )),
            _ => match body {
                // A 200 can still carry an error text (for example an unknown ID).
                Some(body) if message.is_some() && body.get("ratings").is_none() => {
                    Err(classify_message(message.unwrap_or_default()))
                }
                Some(body) => Ok(body),
                None => Err(request_error("the response was not in the expected format")),
            },
        }
    }
}

fn classify_message(message: String) -> ProviderError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("not found") || lower.contains("no such") {
        ProviderError::NotFound(DISPLAY_NAME.to_owned())
    } else if lower.contains("api key") || lower.contains("apikey") {
        ProviderError::Auth {
            provider: DISPLAY_NAME.to_owned(),
            message,
        }
    } else {
        request_error(message)
    }
}

fn request_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message: message.into(),
    }
}

fn missing_key() -> ProviderError {
    let (url, fields) = MdbListProvider::schema()
        .map(|s| {
            (
                s.registration_url.to_owned(),
                vec![catalogue::API_KEY.to_owned()],
            )
        })
        .unwrap_or_default();
    ProviderError::MissingCredentials {
        provider: DISPLAY_NAME.to_owned(),
        fields,
        registration_url: url,
    }
}

/// The ID kinds MDBList accepts in its path, in the order they are tried, with the key each is
/// stored under in a title's IDs.
fn id_order(kind: MediaKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        MediaKind::Movie => &[
            ("imdb", id_key::IMDB),
            ("tmdb", id_key::TMDB),
            ("trakt", id_key::TRAKT),
        ],
        MediaKind::Show => &[
            ("imdb", id_key::IMDB),
            ("tvdb", id_key::TVDB),
            ("tmdb", id_key::TMDB),
            ("trakt", id_key::TRAKT),
        ],
    }
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The rating one entry of MDBList's `ratings` array stands for, on the app's key and scale.
pub fn rating_of(entry: &Value, kind: MediaKind) -> Option<Rating> {
    let source = entry.get("source")?.as_str()?.trim().to_ascii_lowercase();
    let value = number(entry.get("value")?)?;
    if value <= 0.0 {
        return None;
    }
    let votes = entry
        .get("votes")
        .and_then(number)
        .filter(|v| *v > 0.0)
        .map_or(0, |v| v.min(f64::from(u32::MAX)) as u32);
    let (id, max): (&str, u32) = match source.as_str() {
        "imdb" => (rating_key::IMDB, 10),
        "tmdb" => (rating_key::TMDB, 10),
        "trakt" => (rating_key::TRAKT, 10),
        "myanimelist" => (rating_key::MYANIMELIST, 10),
        "metacriticuser" => (METACRITIC_USER_RATING_ID, 10),
        "metacritic" => (rating_key::METACRITIC, 100),
        "tomatoes" => (rating_key::TOMATOMETER_ALL_CRITICS, 100),
        "audience" | "popcorn" => (rating_key::TOMATOMETER_ALL_AUDIENCE, 100),
        "letterboxd" => (rating_key::LETTERBOXD, 5),
        "rogerebert" => (rating_key::ROGEREBERT, 4),
        _ => return None,
    };
    let mut value = value as f32;
    // TMDB and Trakt come as percentages on some responses: bring them to the 0-10 scale.
    if max == 10 && value > 10.0 {
        value /= 10.0;
    }
    let page = entry
        .get("url")
        .and_then(Value::as_str)
        .map_or(String::new(), |path| page_url(id, path, kind));
    Some(
        Rating::new(id, value, votes, max)
            .with_url(&page)
            .normalised(),
    )
}

/// The title's page on the rating source's site, from the path MDBList gives (`/m/blade_runner`,
/// `/blade-runner`, `/film/blade-runner/`, `blade-runner-1982-1`). Empty for a source whose path
/// is not a page address (IMDb's `url` is a number) or an empty path.
fn page_url(id: &str, path: &str, kind: MediaKind) -> String {
    let path = path.trim();
    if path.is_empty() || path.contains("://") {
        return String::new();
    }
    let slug = path.trim_matches('/');
    let section = match kind {
        MediaKind::Movie => "movie",
        MediaKind::Show => "tv",
    };
    match id {
        rating_key::TOMATOMETER_ALL_CRITICS | rating_key::TOMATOMETER_ALL_AUDIENCE => {
            format!("https://www.rottentomatoes.com/{slug}")
        }
        rating_key::METACRITIC | METACRITIC_USER_RATING_ID => {
            // Metacritic paths are the bare title slug; older answers carry the section.
            let slug = slug
                .strip_prefix("movie/")
                .or_else(|| slug.strip_prefix("tv/"))
                .unwrap_or(slug);
            format!("https://www.metacritic.com/{section}/{slug}/")
        }
        rating_key::LETTERBOXD => format!("https://letterboxd.com/{slug}/"),
        rating_key::ROGEREBERT => {
            let slug = slug.strip_prefix("reviews/").unwrap_or(slug);
            format!("https://www.rogerebert.com/reviews/{slug}")
        }
        _ => String::new(),
    }
}

/// Every known rating of a title answer. When both Rotten Tomatoes audience sources are present,
/// `audience` wins.
pub fn ratings_of(title: &Value, kind: MediaKind) -> Ratings {
    let mut out = Ratings::new();
    let Some(entries) = title.get("ratings").and_then(Value::as_array) else {
        return out;
    };
    let source = |e: &Value| {
        e.get("source")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
    };
    for popcorn_first in [true, false] {
        for entry in entries {
            let is_popcorn = source(entry).as_deref() == Some("popcorn");
            if is_popcorn == popcorn_first
                && let Some(rating) = rating_of(entry, kind)
            {
                out.insert(rating);
            }
        }
    }
    out
}

#[async_trait]
impl Provider for MdbListProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn name(&self) -> &'static str {
        DISPLAY_NAME
    }

    fn credentials(&self) -> Option<&'static CredentialSchema> {
        Self::schema()
    }

    async fn check_credentials(
        &self,
        credentials: &ResolvedCredentials,
    ) -> Result<(), ProviderError> {
        self.title(
            credentials,
            "imdb",
            MediaKind::Movie,
            "tt0133093",
            &RequestOptions {
                bypass_cache: true,
                ..RequestOptions::default()
            },
        )
        .await
        .map(|_| ())
    }
}

#[async_trait]
impl RatingProvider for MdbListProvider {
    async fn ratings(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        let credentials = self.inner.resolver.require(PROVIDER_ID)?;
        for (id_provider, key) in id_order(kind) {
            let Some(id) = ids.get(key) else { continue };
            match self
                .title(&credentials, id_provider, kind, id, options)
                .await
            {
                Ok(title) => return Ok(ratings_of(&title, kind)),
                Err(ProviderError::NotFound(_)) => {}
                Err(other) => return Err(other),
            }
        }
        Err(ProviderError::NotFound(DISPLAY_NAME.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sources_map_to_the_app_keys_and_scales() {
        let title = json!({"ratings": [
            {"source": "imdb", "value": 8.7, "votes": 2100000},
            {"source": "tmdb", "value": 82, "votes": 25000},
            {"source": "metacritic", "value": 73, "votes": null},
            {"source": "metacriticuser", "value": 8.4},
            {"source": "tomatoes", "value": 83, "votes": 200},
            {"source": "popcorn", "value": 85},
            {"source": "audience", "value": 90},
            {"source": "letterboxd", "value": 4.1, "votes": 1500000},
            {"source": "rogerebert", "value": 3.5},
            {"source": "myanimelist", "value": 8.2},
            {"source": "somethingnew", "value": 5},
            {"source": "trakt", "value": 0}
        ]});
        let r = ratings_of(&title, MediaKind::Movie);
        let get = |id: &str| r.get(id).unwrap_or_else(|| panic!("missing {id}")).clone();
        assert_eq!(get("imdb").votes, 2_100_000);
        assert_eq!((get("tmdb").value, get("tmdb").max_value), (8.2, 10));
        assert_eq!(get("metacritic").max_value, 100);
        assert_eq!(get("metacriticuser").max_value, 10);
        assert_eq!(get("tomatometerallcritics").value, 83.0);
        let audience = get("tomatometerallaudience");
        assert_eq!((audience.value, audience.max_value), (90.0, 100));
        assert_eq!(
            (get("letterboxd").value, get("letterboxd").max_value),
            (4.1, 5)
        );
        assert_eq!(get("rogerebert").max_value, 4);
        assert!(r.get("trakt").is_none());
        assert_eq!(r.len(), 9);
    }

    #[test]
    fn bad_entries_are_ignored() {
        assert!(rating_of(&json!({"source": "imdb"}), MediaKind::Movie).is_none());
        assert!(rating_of(&json!({"value": 5}), MediaKind::Movie).is_none());
        assert!(rating_of(&json!({"source": "imdb", "value": "n/a"}), MediaKind::Movie).is_none());
        assert!(ratings_of(&json!({"title": "x"}), MediaKind::Movie).is_empty());
    }

    /// The page paths of the answer (as MDBList sends them for Blade Runner and Breaking Bad)
    /// become the addresses of the title's pages; IMDb's numeric `url` and missing ones none.
    #[test]
    fn page_paths_become_addresses_on_each_site() {
        let movie = json!({"ratings": [
            {"source": "imdb", "value": 8.1, "url": 282},
            {"source": "metacritic", "value": 84, "url": "/blade-runner"},
            {"source": "metacriticuser", "value": 8.6, "url": null},
            {"source": "tomatoes", "value": 89, "url": "/m/blade_runner"},
            {"source": "popcorn", "value": 91, "url": "/m/blade_runner"},
            {"source": "letterboxd", "value": 4.1, "url": "/film/blade-runner/"},
            {"source": "rogerebert", "value": 3.0, "url": "blade-runner-1982-1"},
            {"source": "tmdb", "value": 79, "url": null}
        ]});
        let r = ratings_of(&movie, MediaKind::Movie);
        let url = |id: &str| r.get(id).map(|x| x.url.clone()).unwrap_or_default();
        assert_eq!(url("imdb"), "");
        assert_eq!(url("tmdb"), "");
        assert_eq!(url("metacriticuser"), "");
        assert_eq!(
            url("metacritic"),
            "https://www.metacritic.com/movie/blade-runner/"
        );
        assert_eq!(
            url("tomatometerallcritics"),
            "https://www.rottentomatoes.com/m/blade_runner"
        );
        assert_eq!(
            url("tomatometerallaudience"),
            "https://www.rottentomatoes.com/m/blade_runner"
        );
        assert_eq!(
            url("letterboxd"),
            "https://letterboxd.com/film/blade-runner/"
        );
        assert_eq!(
            url("rogerebert"),
            "https://www.rogerebert.com/reviews/blade-runner-1982-1"
        );

        let show = json!({"ratings": [
            {"source": "metacritic", "value": 87, "url": "/breaking-bad"},
            {"source": "tomatoes", "value": 96, "url": "/tv/breaking_bad"}
        ]});
        let r = ratings_of(&show, MediaKind::Show);
        assert_eq!(
            r.get("metacritic").unwrap().url,
            "https://www.metacritic.com/tv/breaking-bad/"
        );
        assert_eq!(
            r.get("tomatometerallcritics").unwrap().url,
            "https://www.rottentomatoes.com/tv/breaking_bad"
        );
        // A full address in place of a path is not trusted as a page of the site.
        let odd = json!({"ratings": [
            {"source": "tomatoes", "value": 90, "url": "javascript://x"}
        ]});
        assert_eq!(
            ratings_of(&odd, MediaKind::Movie)
                .get("tomatometerallcritics")
                .unwrap()
                .url,
            ""
        );
    }
}

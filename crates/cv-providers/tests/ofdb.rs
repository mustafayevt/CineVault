//! The OFDb provider against a local mock server that replays hand-built pages in the shape of
//! OFDb's search and film pages (`tests/fixtures/ofdb/`). Nothing here touches the network; the
//! opt-in live smoke test is `tests/ofdb_live.rs`.

mod support;

use std::path::{Path, PathBuf};

use cv_domain::{CreditKind, Genre, ItemDetail, MediaFileType, ProviderIds, id_key};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::ofdb::{OfdbProvider, RATING_ID};
use cv_providers::{ArtworkProvider, HttpClient, MetadataProvider, Provider, ProviderError};
use support::{MockServer, Reply};

const FILM_PATH: &str = "/film/12345/";
const PLOT_PATH: &str = "/film/12345,275266,Nachtzug-nach-Nirgendwo/plot/";

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ofdb")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn page(name: &str) -> Reply {
    let mut reply = Reply::ok(&fixture(name));
    reply.headers = vec![("Content-Type".into(), "text/html; charset=utf-8".into())];
    reply
}

struct Harness {
    server: MockServer,
    provider: OfdbProvider,
    cache_dir: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let server = MockServer::start().await;
        let cache_dir = tempfile::tempdir().unwrap();
        let http = HttpClient::builder()
            .cache_dir(cache_dir.path())
            .build()
            .unwrap();
        let provider = OfdbProvider::with_base_url(http, &server.url(""));
        Self {
            server,
            provider,
            cache_dir,
        }
    }

    fn with_film(self) -> Self {
        self.server.route(FILM_PATH, page("film_page.html"));
        self.server.route(PLOT_PATH, page("plot_page.html"));
        self
    }

    fn cached_files(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        walk(self.cache_dir.path(), &mut out);
        out
    }
}

fn ofdb_ids(id: &str) -> ProviderIds {
    let mut ids = ProviderIds::new();
    ids.set("ofdb", id);
    ids
}

fn imdb_ids(id: &str) -> ProviderIds {
    let mut ids = ProviderIds::new();
    ids.set(id_key::IMDB, id);
    ids
}

fn query(kind: MediaKind, title: &str) -> SearchQuery {
    SearchQuery {
        kind,
        title: title.to_owned(),
        year: None,
    }
}

fn opts() -> RequestOptions {
    RequestOptions::default()
}

// --- identity ---------------------------------------------------------------------------------

#[tokio::test]
async fn the_provider_is_a_best_effort_movie_provider() {
    let h = Harness::new().await;
    let capabilities = h.provider.capabilities();
    assert!(capabilities.best_effort);
    assert!(capabilities.movies && !capabilities.shows);
    assert_eq!(capabilities.lookup_ids, vec!["ofdb", id_key::IMDB]);
    assert_eq!(h.provider.id(), "ofdb");
    assert_eq!(h.provider.name(), "OFDb");
    assert!(h.provider.credentials().is_none());
}

// --- search -----------------------------------------------------------------------------------

#[tokio::test]
async fn search_posts_the_title_and_reads_the_movie_rows() {
    let h = Harness::new().await;
    h.server
        .route("/suchergebnis/", page("search_results.html"));

    let hits = h
        .provider
        .search(&query(MediaKind::Movie, "Nachtzug & Co"), &opts())
        .await
        .unwrap();

    let requests = h.server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(
        requests[0].headers.get("content-type").map(String::as_str),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(
        String::from_utf8_lossy(&requests[0].body),
        "QSinput=Nachtzug+%26+Co"
    );

    // The series row and the person row are left out.
    assert_eq!(hits.len(), 3);
    let first = &hits[0];
    assert_eq!(first.provider, "ofdb");
    assert_eq!(first.ids.get("ofdb"), Some("12345"));
    assert_eq!(first.title, "Nachtzug nach Nirgendwo");
    assert_eq!(first.original_title, "Nachtzug nach Nirgendwo");
    assert_eq!(first.year, 1987);
    assert!(!first.exact_id);
    assert_eq!(
        first.poster_url.as_deref(),
        Some(server_url(&h, "/images/film.185px/12/12345.jpg?v=1695381353").as_str())
    );

    // The original title comes from the `[OT: ...]` note; a film without a poster has none.
    assert_eq!(hits[1].title, "Der Zug der Stille");
    assert_eq!(hits[1].original_title, "Night Train to Nowhere");
    assert_eq!(hits[1].poster_url, None);
    // An alternative title (`[AT: ...]`) is not the original one, and a missing year is 0.
    assert_eq!(hits[2].original_title, "Das Ende der Linie");
    assert_eq!(hits[2].year, 0);
}

fn server_url(h: &Harness, path: &str) -> String {
    h.server.url(path)
}

#[tokio::test]
async fn search_for_shows_or_blank_titles_makes_no_request() {
    let h = Harness::new().await;
    assert!(
        h.provider
            .search(&query(MediaKind::Show, "Nachtzug"), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        h.provider
            .search(&query(MediaKind::Movie, "  "), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn a_search_without_results_is_empty_not_an_error() {
    let h = Harness::new().await;
    h.server.route("/suchergebnis/", page("search_empty.html"));
    let hits = h
        .provider
        .search(&query(MediaKind::Movie, "xqzvbnmwk"), &opts())
        .await
        .unwrap();
    assert!(hits.is_empty());
}

// --- details ----------------------------------------------------------------------------------

#[tokio::test]
async fn movie_details_come_from_the_film_page_and_the_full_plot() {
    let h = Harness::new().await.with_film();

    let item = h
        .provider
        .details(MediaKind::Movie, &ofdb_ids("12345"), &opts())
        .await
        .unwrap();

    let core = &item.core;
    assert_eq!(core.title, "Nachtzug nach Nirgendwo");
    assert_eq!(core.original_title, "Night Train to Nowhere");
    assert_eq!(core.year, 1987);
    assert_eq!(core.ids.get("ofdb"), Some("12345"));
    assert_eq!(core.ids.get(id_key::IMDB), Some("tt0123456"));

    // The film page cuts the plot off; the plot page has the rest.
    assert!(
        core.plot
            .starts_with("Ein Schaffner entdeckt im letzten Waggon")
    );
    assert!(core.plot.contains("nach dem Fahrplan nicht geben dürfte"));
    assert!(core.plot.contains("am Ende steht er selbst am Bahnsteig."));
    assert!(core.plot.contains("Ein leiser, düsterer Film."));
    assert!(!core.plot.contains("..."));
    assert!(!core.plot.contains('…'));

    let rating = core.ratings.get(RATING_ID).expect("ofdb rating");
    assert!((rating.value - 7.4).abs() < 1e-6);
    assert_eq!((rating.votes, rating.max_value), (1234, 10));

    // Directors first, then each actor once.
    let credits: Vec<_> = core
        .credits
        .iter()
        .map(|c| (c.kind, c.person.name.as_str()))
        .collect();
    assert_eq!(
        credits,
        [
            (CreditKind::Director, "Erika Beispiel"),
            (CreditKind::Actor, "Max Mustermann"),
            (CreditKind::Actor, "Lena Probe"),
        ]
    );

    assert_eq!(
        core.artwork_urls.get(&MediaFileType::Poster),
        Some(&h.server.url("/images/film.370px/12/12345.jpg?v=1696956915"))
    );
    assert_eq!(
        core.artwork_urls.get(&MediaFileType::Fanart),
        Some(
            &h.server
                .url("/images/backdrop.870px/12/12345.jpg?v=1697107332")
        )
    );

    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("a movie");
    };
    assert_eq!(
        movie.genres,
        [
            Genre::parse("Drama").unwrap(),
            Genre::parse("Comedy").unwrap(),
            Genre::parse("Science Fiction").unwrap(),
        ]
    );
    assert_eq!(movie.countries, ["DE", "FR"]);
}

#[tokio::test]
async fn a_page_with_only_a_title_still_gives_a_movie() {
    let h = Harness::new().await;
    h.server.route(FILM_PATH, page("film_short.html"));

    let item = h
        .provider
        .details(MediaKind::Movie, &ofdb_ids("12345"), &opts())
        .await
        .unwrap();

    assert_eq!(item.core.title, "Kurzes Beispiel");
    assert_eq!(item.core.original_title, "Kurzes Beispiel");
    assert_eq!(item.core.year, 0);
    assert_eq!(item.core.plot, "Ein Film ohne viele Angaben.");
    assert!(item.core.ratings.is_empty());
    assert!(item.core.credits.is_empty());
    assert!(item.core.artwork_urls.is_empty());
    assert!(!item.core.ids.contains(id_key::IMDB));
    // The plot is not cut off, so there is no second request.
    assert_eq!(h.server.requests().len(), 1);
}

#[tokio::test]
async fn an_unreadable_plot_page_keeps_the_short_plot() {
    let h = Harness::new().await;
    h.server.route(FILM_PATH, page("film_page.html"));
    h.server.route(PLOT_PATH, page("film_changed.html"));

    let item = h
        .provider
        .details(MediaKind::Movie, &ofdb_ids("12345"), &opts())
        .await
        .unwrap();
    assert!(item.core.plot.ends_with('…'));
    assert_eq!(item.core.title, "Nachtzug nach Nirgendwo");
}

#[tokio::test]
async fn details_by_imdb_id_search_first_and_check_the_film() {
    let h = Harness::new().await.with_film();
    h.server
        .route("/suchergebnis/", page("search_results.html"));

    let item = h
        .provider
        .details(MediaKind::Movie, &imdb_ids("tt0123456"), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.ids.get("ofdb"), Some("12345"));

    let posts: Vec<_> = h
        .server
        .requests()
        .into_iter()
        .filter(|r| r.method == "POST")
        .collect();
    assert_eq!(posts.len(), 1);
    assert_eq!(String::from_utf8_lossy(&posts[0].body), "QSinput=tt0123456");

    // The search finds a film, but its page names another IMDb ID: that is not the title.
    let other = h
        .provider
        .lookup(MediaKind::Movie, &imdb_ids("tt0999999"), &opts())
        .await
        .unwrap();
    assert_eq!(other, None);
}

#[tokio::test]
async fn lookup_returns_an_exact_hit() {
    let h = Harness::new().await.with_film();
    let hit = h
        .provider
        .lookup(MediaKind::Movie, &ofdb_ids("12345"), &opts())
        .await
        .unwrap()
        .expect("a hit");
    assert!(hit.exact_id);
    assert_eq!(hit.title, "Nachtzug nach Nirgendwo");
    assert_eq!(hit.original_title, "Night Train to Nowhere");
    assert_eq!(hit.year, 1987);
    assert_eq!(hit.ids.get(id_key::IMDB), Some("tt0123456"));

    // No ID the provider understands: nothing to look up, and no request.
    let before = h.server.requests().len();
    let none = h
        .provider
        .lookup(MediaKind::Movie, &ProviderIds::new(), &opts())
        .await
        .unwrap();
    assert_eq!(none, None);
    assert_eq!(h.server.requests().len(), before);
}

#[tokio::test]
async fn shows_and_unknown_films_are_not_found() {
    let h = Harness::new().await;
    assert!(matches!(
        h.provider
            .details(MediaKind::Show, &ofdb_ids("12345"), &opts())
            .await,
        Err(ProviderError::Unsupported { .. })
    ));
    assert_eq!(
        h.provider
            .lookup(MediaKind::Show, &ofdb_ids("12345"), &opts())
            .await
            .unwrap(),
        None
    );
    // The mock answers 404 for a film it does not know.
    assert_eq!(
        h.provider
            .details(MediaKind::Movie, &ofdb_ids("999"), &opts())
            .await,
        Err(ProviderError::NotFound("OFDb".to_owned()))
    );
    assert_eq!(
        h.provider
            .lookup(MediaKind::Movie, &ofdb_ids("999"), &opts())
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        h.provider
            .details(MediaKind::Movie, &ProviderIds::new(), &opts())
            .await,
        Err(ProviderError::NotFound("OFDb".to_owned()))
    );
}

// --- artwork ----------------------------------------------------------------------------------

#[tokio::test]
async fn artwork_lists_the_poster_and_the_backdrop() {
    let h = Harness::new().await.with_film();
    let candidates = h
        .provider
        .artwork(MediaKind::Movie, &ofdb_ids("12345"), &opts())
        .await
        .unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].kind, MediaFileType::Poster);
    assert_eq!(candidates[0].width, 370);
    assert_eq!(candidates[0].language.as_deref(), Some("de"));
    assert_eq!(candidates[1].kind, MediaFileType::Fanart);
    assert_eq!(candidates[1].width, 870);
    assert_eq!(candidates[1].language, None);
    assert!(
        h.provider
            .artwork(MediaKind::Show, &ofdb_ids("12345"), &opts())
            .await
            .unwrap()
            .is_empty()
    );
}

// --- page format changed: typed errors, nothing written ---------------------------------------

#[tokio::test]
async fn a_page_whose_format_changed_fails_and_writes_nothing() {
    let h = Harness::new().await;
    let format_changed = Err(ProviderError::FormatChanged("OFDb".to_owned()));

    h.server.route(FILM_PATH, page("film_changed.html"));
    let result = h
        .provider
        .details(MediaKind::Movie, &ofdb_ids("12345"), &opts())
        .await;
    assert_eq!(result, format_changed);
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("page format changed")
    );
    assert!(h.cached_files().is_empty());

    // A lookup reports the same error rather than "no match".
    assert_eq!(
        h.provider
            .lookup(MediaKind::Movie, &ofdb_ids("12345"), &opts())
            .await,
        Err(ProviderError::FormatChanged("OFDb".to_owned()))
    );

    // The search page can change too.
    h.server
        .route("/suchergebnis/", page("search_changed.html"));
    assert_eq!(
        h.provider
            .search(&query(MediaKind::Movie, "Nachtzug"), &opts())
            .await,
        Err(ProviderError::FormatChanged("OFDb".to_owned()))
    );
    assert!(h.cached_files().is_empty());
}

#[tokio::test]
async fn a_failed_scrape_leaves_the_existing_item_untouched() {
    let h = Harness::new().await.with_film();
    let mut stored = h
        .provider
        .details(MediaKind::Movie, &ofdb_ids("12345"), &opts())
        .await
        .unwrap();
    stored.core.plot = "My own edited plot".to_owned();
    let before = stored.clone();

    h.server.route(FILM_PATH, page("film_changed.html"));
    let mut forced = opts();
    forced.bypass_cache = true;
    let result = h
        .provider
        .details(MediaKind::Movie, &ofdb_ids("12345"), &forced)
        .await;
    assert!(matches!(result, Err(ProviderError::FormatChanged(_))));
    assert_eq!(stored, before);
    assert!(
        h.cached_files().iter().all(|f| !std::fs::read(f)
            .unwrap()
            .windows(10)
            .any(|w| w == b"film-title")),
        "the unreadable page must not stay in the cache"
    );
}

#[tokio::test]
async fn a_page_that_parses_again_is_not_blocked_by_an_earlier_failure() {
    let h = Harness::new().await;
    h.server.route(FILM_PATH, page("film_changed.html"));
    assert!(
        h.provider
            .details(MediaKind::Movie, &ofdb_ids("12345"), &opts())
            .await
            .is_err()
    );
    h.server.route(FILM_PATH, page("film_short.html"));
    let item = h
        .provider
        .details(MediaKind::Movie, &ofdb_ids("12345"), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Kurzes Beispiel");
    assert_eq!(h.server.count(FILM_PATH), 2);
}

// --- bot wall, errors, caching ----------------------------------------------------------------

#[tokio::test]
async fn a_human_check_is_a_failed_request_and_is_not_cached() {
    let h = Harness::new().await;
    for code in [202, 403] {
        h.server.route(FILM_PATH, Reply::status(code));
        let result = h
            .provider
            .details(MediaKind::Movie, &ofdb_ids("12345"), &opts())
            .await;
        let Err(ProviderError::Request { provider, message }) = result else {
            panic!("HTTP {code} should be a failed request");
        };
        assert_eq!(provider, "OFDb");
        assert!(message.contains("human check"), "{message}");
        assert!(h.cached_files().is_empty(), "HTTP {code}");
    }
}

#[tokio::test]
async fn a_server_error_is_a_failed_request() {
    let h = Harness::new().await;
    h.server.route(FILM_PATH, Reply::status(503));
    let result = h
        .provider
        .details(MediaKind::Movie, &ofdb_ids("12345"), &opts())
        .await;
    assert!(matches!(
        result,
        Err(ProviderError::Request { ref message, .. }) if message == "HTTP 503"
    ));
}

#[tokio::test]
async fn pages_are_cached_and_a_forced_rescrape_goes_to_the_network() {
    let h = Harness::new().await.with_film();
    for _ in 0..2 {
        h.provider
            .details(MediaKind::Movie, &ofdb_ids("12345"), &opts())
            .await
            .unwrap();
    }
    assert_eq!(h.server.count(FILM_PATH), 1);
    assert_eq!(h.server.count(PLOT_PATH), 1);

    let mut forced = opts();
    forced.bypass_cache = true;
    h.provider
        .details(MediaKind::Movie, &ofdb_ids("12345"), &forced)
        .await
        .unwrap();
    assert_eq!(h.server.count(FILM_PATH), 2);
}

#[tokio::test]
async fn pages_are_requested_in_german_like_a_browser() {
    let h = Harness::new().await;
    h.server.route(FILM_PATH, page("film_short.html"));
    h.provider
        .details(MediaKind::Movie, &ofdb_ids("12345"), &opts())
        .await
        .unwrap();
    let request = &h.server.requests()[0];
    assert!(request.headers["user-agent"].contains("Mozilla/5.0"));
    assert!(request.headers["accept-language"].starts_with("de-DE"));
}

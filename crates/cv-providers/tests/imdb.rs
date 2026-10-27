//! The IMDb provider against a local mock server that replays hand-built pages in the shape of
//! IMDb's `__NEXT_DATA__` JSON (`tests/fixtures/imdb/`). Nothing here touches the network; the
//! opt-in live smoke test is `tests/imdb_live.rs`.

mod support;

use std::path::{Path, PathBuf};

use cv_domain::{
    AiredStatus, CalendarDate, Certification, CreditKind, Genre, ItemDetail, MediaFileType,
    ProviderIds, id_key,
};
use cv_providers::imdb::{ImdbProvider, RATING_ID, Top250List, Top250Source};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::{
    ArtworkProvider, HttpClient, HttpRequest, MetadataProvider, Provider, ProviderError, SizeBucket,
};
use support::{MockServer, Reply};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/imdb")
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
    provider: ImdbProvider,
    http: HttpClient,
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
        let provider = ImdbProvider::with_base_url(http.clone(), &server.url(""));
        Self {
            server,
            provider,
            http,
            cache_dir,
        }
    }

    /// The cache files written so far.
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

// --- identity ---------------------------------------------------------------------------------

#[tokio::test]
async fn the_provider_is_labelled_best_effort() {
    let h = Harness::new().await;
    let capabilities = h.provider.capabilities();
    assert!(capabilities.best_effort);
    assert!(capabilities.movies && capabilities.shows);
    assert_eq!(capabilities.lookup_ids, vec![id_key::IMDB]);
    assert_eq!(h.provider.id(), "imdb");
    assert_eq!(h.provider.name(), "IMDb");
    assert!(h.provider.credentials().is_none());
}

// --- details ----------------------------------------------------------------------------------

#[tokio::test]
async fn movie_details_come_from_the_title_page() {
    let h = Harness::new().await;
    h.server
        .route("/title/tt0113277/", page("title_movie.html"));

    let item = h
        .provider
        .details(
            MediaKind::Movie,
            &imdb_ids("tt0113277"),
            &RequestOptions::default(),
        )
        .await
        .unwrap();

    let core = &item.core;
    assert_eq!(core.title, "Heat");
    assert_eq!(core.original_title, "Heat (original)");
    assert_eq!(core.year, 1995);
    assert_eq!(core.ids.get(id_key::IMDB), Some("tt0113277"));
    assert!(core.plot.starts_with("A detective hunts a crew"));
    assert_eq!(
        core.production_companies,
        vec!["Regency Enterprises", "Forward Pass"]
    );
    assert_eq!(
        core.artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("https://m.example.test/images/heat_poster._V1_.jpg")
    );
    let rating = core.ratings.get(RATING_ID).expect("imdb rating");
    assert!((rating.value - 8.3).abs() < 1e-6);
    assert_eq!((rating.votes, rating.max_value), (712_345, 10));

    let kinds: Vec<(CreditKind, &str, &str)> = core
        .credits
        .iter()
        .map(|c| (c.kind, c.person.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (CreditKind::Director, "Michael Mann", ""),
            (CreditKind::Writer, "Michael Mann", ""),
            (CreditKind::Actor, "Al Pacino", "Lt. Vincent Hanna"),
            (CreditKind::Actor, "Robert De Niro", "Neil McCauley, Neil"),
            (CreditKind::Actor, "Val Kilmer", ""),
        ]
    );
    assert_eq!(
        core.credits[0].person.ids.get(id_key::IMDB),
        Some("nm0000520")
    );

    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("a movie");
    };
    assert_eq!(movie.english_title, "Heat");
    assert_eq!(movie.tagline, "A Los Angeles crime saga");
    assert_eq!(movie.runtime_minutes, 170);
    assert_eq!(movie.release_date, CalendarDate::new(1995, 12, 15));
    assert_eq!(movie.certification, Certification("US_R".to_owned()));
    assert_eq!(
        movie.genres,
        vec![Genre::Action, Genre::Crime, Genre::Drama]
    );
    assert_eq!(movie.countries, vec!["US"]);
    assert_eq!(movie.spoken_languages, vec!["en", "es"]);
}

#[tokio::test]
async fn show_details_carry_the_run_and_status() {
    let h = Harness::new().await;
    h.server.route("/title/tt0903747/", page("title_show.html"));

    let item = h
        .provider
        .details(
            MediaKind::Show,
            &imdb_ids("TT0903747"),
            &RequestOptions::default(),
        )
        .await
        .unwrap();

    assert_eq!(item.core.title, "Fixture Series");
    assert_eq!(item.core.year, 2008);
    assert_eq!(item.core.production_companies, vec!["Fixture Television"]);
    let ItemDetail::Show(show) = &item.detail else {
        panic!("a show");
    };
    assert_eq!(show.year_end, 2013);
    assert_eq!(show.status, AiredStatus::Ended);
    assert_eq!(show.first_aired, CalendarDate::new(2008, 1, 20));
    assert_eq!(show.runtime_minutes, 45);
    assert_eq!(show.certification, Certification("US_TV_MA".to_owned()));
    assert_eq!(show.country, "US");
    assert_eq!(
        show.genres,
        vec![Genre::Crime, Genre::Drama, Genre::Thriller]
    );
}

#[tokio::test]
async fn requests_look_like_a_browser_in_the_users_language() {
    let h = Harness::new().await;
    h.server
        .route("/title/tt0113277/", page("title_movie.html"));
    let options = RequestOptions {
        language: "tr".into(),
        country: "TR".into(),
        ..RequestOptions::default()
    };

    let item = h
        .provider
        .details(MediaKind::Movie, &imdb_ids("tt0113277"), &options)
        .await
        .unwrap();
    // The title is not claimed to be English when another language was asked for.
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("a movie");
    };
    assert_eq!(movie.english_title, "");

    let request = &h.server.requests()[0];
    assert!(request.headers["user-agent"].starts_with("Mozilla/5.0"));
    assert_eq!(request.headers["accept-language"], "tr-TR,tr;q=0.9");
}

#[tokio::test]
async fn a_title_of_the_other_kind_or_an_unknown_title_is_not_found() {
    let h = Harness::new().await;
    h.server.route("/title/tt0903747/", page("title_show.html"));
    h.server.route("/title/tt9999999/", Reply::status(404));

    let wrong_kind = h
        .provider
        .details(
            MediaKind::Movie,
            &imdb_ids("tt0903747"),
            &RequestOptions::default(),
        )
        .await;
    assert_eq!(wrong_kind, Err(ProviderError::NotFound("IMDb".to_owned())));

    let unknown = h
        .provider
        .details(
            MediaKind::Movie,
            &imdb_ids("tt9999999"),
            &RequestOptions::default(),
        )
        .await;
    assert_eq!(unknown, Err(ProviderError::NotFound("IMDb".to_owned())));

    let no_id = h
        .provider
        .details(
            MediaKind::Movie,
            &ProviderIds::new(),
            &RequestOptions::default(),
        )
        .await;
    assert_eq!(no_id, Err(ProviderError::NotFound("IMDb".to_owned())));
    assert_eq!(
        h.server.requests().len(),
        2,
        "no request without an IMDb ID"
    );
}

// --- lookup -----------------------------------------------------------------------------------

#[tokio::test]
async fn lookup_by_imdb_id_returns_an_exact_hit() {
    let h = Harness::new().await;
    h.server
        .route("/title/tt0113277/", page("title_movie.html"));
    h.server.route("/title/tt0903747/", page("title_show.html"));
    h.server.route("/title/tt9999999/", Reply::status(404));
    let options = RequestOptions::default();

    let hit = h
        .provider
        .lookup(MediaKind::Movie, &imdb_ids("tt0113277"), &options)
        .await
        .unwrap()
        .expect("Heat");
    assert_eq!(
        (hit.title.as_str(), hit.year, hit.exact_id),
        ("Heat", 1995, true)
    );
    assert_eq!(hit.provider, "imdb");
    assert_eq!(hit.ids.get(id_key::IMDB), Some("tt0113277"));
    assert!(hit.poster_url.is_some());

    let other_kind = h
        .provider
        .lookup(MediaKind::Movie, &imdb_ids("tt0903747"), &options)
        .await;
    assert_eq!(other_kind, Ok(None));
    let missing = h
        .provider
        .lookup(MediaKind::Movie, &imdb_ids("tt9999999"), &options)
        .await;
    assert_eq!(missing, Ok(None));

    let mut tmdb_only = ProviderIds::new();
    tmdb_only.set(id_key::TMDB, "949");
    let before = h.server.requests().len();
    let none = h
        .provider
        .lookup(MediaKind::Movie, &tmdb_only, &options)
        .await;
    assert_eq!(none, Ok(None));
    assert_eq!(
        h.server.requests().len(),
        before,
        "no request without an IMDb ID"
    );
}

// --- search -----------------------------------------------------------------------------------

#[tokio::test]
async fn search_returns_the_titles_of_the_asked_kind() {
    let h = Harness::new().await;
    h.server.route("/find/", page("find_results.html"));
    let options = RequestOptions::default();

    let movies = h
        .provider
        .search(&query(MediaKind::Movie, " heat "), &options)
        .await
        .unwrap();
    let titles: Vec<(&str, u16)> = movies.iter().map(|h| (h.title.as_str(), h.year)).collect();
    assert_eq!(titles, vec![("Heat", 1995), ("Heat Rising", 2019)]);
    assert_eq!(movies[0].ids.get(id_key::IMDB), Some("tt0113277"));
    assert_eq!(
        movies[0].poster_url.as_deref(),
        Some("https://m.example.test/images/heat_small.jpg")
    );
    assert!(!movies[0].exact_id);

    let shows = h
        .provider
        .search(&query(MediaKind::Show, "heat"), &options)
        .await
        .unwrap();
    let titles: Vec<(&str, u16)> = shows.iter().map(|h| (h.title.as_str(), h.year)).collect();
    assert_eq!(titles, vec![("Heat Wave", 2008)]);

    let request = &h.server.requests()[0];
    assert!(request.target.contains("q=heat"), "{}", request.target);
    assert!(request.target.contains("s=tt"), "{}", request.target);
}

#[tokio::test]
async fn search_without_results_is_empty_not_an_error() {
    let h = Harness::new().await;
    h.server.route("/find/", page("find_empty.html"));
    let hits = h
        .provider
        .search(
            &query(MediaKind::Movie, "zzzzqqqq"),
            &RequestOptions::default(),
        )
        .await;
    assert_eq!(hits, Ok(Vec::new()));

    let blank = h
        .provider
        .search(&query(MediaKind::Movie, "  "), &RequestOptions::default())
        .await;
    assert_eq!(blank, Ok(Vec::new()));
    assert_eq!(h.server.requests().len(), 1, "a blank title sends nothing");
}

// --- page format changed: typed errors, nothing written ---------------------------------------

#[tokio::test]
async fn a_page_whose_format_changed_fails_and_writes_nothing() {
    let h = Harness::new().await;
    let options = RequestOptions::default();
    let ids = imdb_ids("tt0113277");

    for (name, label) in [
        ("title_changed.html", "different structure"),
        ("title_no_data.html", "no __NEXT_DATA__ block"),
        ("title_bad_json.html", "invalid JSON"),
    ] {
        h.server.route("/title/tt0113277/", page(name));
        let result = h.provider.details(MediaKind::Movie, &ids, &options).await;
        assert_eq!(
            result,
            Err(ProviderError::FormatChanged("IMDb".to_owned())),
            "{label}"
        );
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("page format changed"),
            "{label}: {message}"
        );
        assert!(
            h.cached_files().is_empty(),
            "{label}: the unreadable page must not stay in the cache"
        );

        // A lookup reports the same error rather than "no match".
        let lookup = h.provider.lookup(MediaKind::Movie, &ids, &options).await;
        assert_eq!(
            lookup,
            Err(ProviderError::FormatChanged("IMDb".to_owned())),
            "{label}"
        );
    }

    // The search page can change too.
    h.server.route("/find/", page("title_changed.html"));
    let search = h
        .provider
        .search(&query(MediaKind::Movie, "heat"), &options)
        .await;
    assert_eq!(search, Err(ProviderError::FormatChanged("IMDb".to_owned())));
    assert!(h.cached_files().is_empty());
}

#[tokio::test]
async fn a_failed_scrape_leaves_the_existing_item_untouched() {
    let h = Harness::new().await;
    let options = RequestOptions::default();
    let ids = imdb_ids("tt0113277");

    h.server
        .route("/title/tt0113277/", page("title_movie.html"));
    let mut stored = h
        .provider
        .details(MediaKind::Movie, &ids, &options)
        .await
        .unwrap();
    stored.core.plot = "My own edited plot".to_owned();
    let before = stored.clone();

    // The site changes. A rescrape that bypasses the cache fails, and the caller's item is
    // exactly what it was: the provider returns no partial item to merge from.
    h.server
        .route("/title/tt0113277/", page("title_changed.html"));
    let mut forced = options.clone();
    forced.bypass_cache = true;
    let result = h.provider.details(MediaKind::Movie, &ids, &forced).await;
    assert!(matches!(result, Err(ProviderError::FormatChanged(_))));
    assert_eq!(stored, before);
    assert!(
        h.cached_files().is_empty(),
        "the unreadable page replaced the good cache entry and was removed"
    );
}

#[tokio::test]
async fn a_page_that_parses_again_is_not_blocked_by_an_earlier_failure() {
    let h = Harness::new().await;
    let options = RequestOptions::default();
    let ids = imdb_ids("tt0113277");

    h.server
        .route("/title/tt0113277/", page("title_changed.html"));
    assert!(
        h.provider
            .details(MediaKind::Movie, &ids, &options)
            .await
            .is_err()
    );

    h.server
        .route("/title/tt0113277/", page("title_movie.html"));
    let item = h
        .provider
        .details(MediaKind::Movie, &ids, &options)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
    assert_eq!(h.server.count("/title/tt0113277/"), 2);
}

// --- bot wall, caching ------------------------------------------------------------------------

#[tokio::test]
async fn a_human_check_is_a_failed_request_and_is_not_cached() {
    let h = Harness::new().await;
    for code in [202, 403] {
        h.server.route("/title/tt0113277/", Reply::status(code));
        let result = h
            .provider
            .details(
                MediaKind::Movie,
                &imdb_ids("tt0113277"),
                &RequestOptions::default(),
            )
            .await;
        let Err(ProviderError::Request { provider, message }) = result else {
            panic!("HTTP {code} should be a failed request");
        };
        assert_eq!(provider, "IMDb");
        assert!(message.contains("human check"), "{message}");
        assert!(h.cached_files().is_empty(), "HTTP {code}");
    }
}

#[tokio::test]
async fn a_site_that_asked_for_a_human_check_is_left_alone_for_a_while() {
    let h = Harness::new().await;
    h.server.route("/title/tt0113277/", Reply::status(202));
    let ids = imdb_ids("tt0113277");
    let options = RequestOptions::default();
    let first = h
        .provider
        .details(MediaKind::Movie, &ids, &options)
        .await
        .unwrap_err();
    assert!(first.is_human_check(), "{first}");
    assert_eq!(h.server.count("/title/tt0113277/"), 1);

    // The next searches do not ask again, and still say why.
    for _ in 0..3 {
        let again = h
            .provider
            .details(MediaKind::Movie, &ids, &options)
            .await
            .unwrap_err();
        assert!(again.is_human_check(), "{again}");
    }
    assert_eq!(h.server.count("/title/tt0113277/"), 1);
}

#[tokio::test]
async fn pages_are_cached_and_a_forced_rescrape_goes_to_the_network() {
    let h = Harness::new().await;
    h.server
        .route("/title/tt0113277/", page("title_movie.html"));
    let ids = imdb_ids("tt0113277");
    let options = RequestOptions::default();

    for _ in 0..2 {
        h.provider
            .details(MediaKind::Movie, &ids, &options)
            .await
            .unwrap();
    }
    assert_eq!(h.server.count("/title/tt0113277/"), 1);
    assert_eq!(h.cached_files().len(), 1);

    let mut forced = options.clone();
    forced.bypass_cache = true;
    h.provider
        .details(MediaKind::Movie, &ids, &forced)
        .await
        .unwrap();
    assert_eq!(h.server.count("/title/tt0113277/"), 2);
}

#[tokio::test]
async fn forgetting_a_request_drops_its_cached_response() {
    let h = Harness::new().await;
    h.server.route("/page", Reply::ok("<html></html>"));
    let request = || HttpRequest::get("imdb", &h.server.url("/page")).html();

    h.http.send(request()).await.unwrap();
    assert_eq!(h.cached_files().len(), 1);
    h.http.forget(&request()).await;
    assert!(h.cached_files().is_empty());
    h.http.send(request()).await.unwrap();
    assert_eq!(h.server.count("/page"), 2);
    // Forgetting something that is not cached is fine.
    h.http
        .forget(&HttpRequest::get("imdb", &h.server.url("/other")).html())
        .await;
}

// --- the Top 250 charts -------------------------------------------------------------------------

#[tokio::test]
async fn the_movie_chart_is_read_best_first() {
    let h = Harness::new().await;
    h.server.route("/chart/top/", page("chart_top.html"));

    let entries = h
        .provider
        .top250(Top250List::Movies, &RequestOptions::default())
        .await
        .unwrap();

    assert_eq!(entries.len(), 250);
    assert_eq!(
        (entries[0].rank, entries[0].imdb_id.as_str()),
        (1, "tt1000001")
    );
    assert_eq!(entries[0].title, "Chart film 1");
    assert_eq!(entries[249].rank, 250);
    assert_eq!(h.server.count("/chart/toptv/"), 0);
}

#[tokio::test]
async fn the_tv_chart_is_a_separate_page() {
    let h = Harness::new().await;
    h.server.route("/chart/toptv/", page("chart_toptv.html"));

    let entries = h
        .provider
        .top250(Top250List::Shows, &RequestOptions::default())
        .await
        .unwrap();

    assert_eq!(entries[0].imdb_id, "tt2000001");
    assert_eq!(h.server.count("/chart/top/"), 0);
}

#[tokio::test]
async fn a_chart_whose_format_changed_fails_and_returns_nothing() {
    let h = Harness::new().await;
    h.server.route("/chart/top/", page("chart_changed.html"));

    let error = h
        .provider
        .top250(Top250List::Movies, &RequestOptions::default())
        .await
        .unwrap_err();

    assert!(
        matches!(error, ProviderError::FormatChanged(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_human_check_instead_of_the_chart_is_a_failed_request() {
    let h = Harness::new().await;
    h.server.route("/chart/top/", Reply::status(403));

    let error = h
        .provider
        .top250(Top250List::Movies, &RequestOptions::default())
        .await
        .unwrap_err();

    assert!(matches!(error, ProviderError::Request { .. }), "{error:?}");
}

// --- artwork ----------------------------------------------------------------------------------

const INDEX_PATH: &str = "/title/tt0113277/mediaindex/";

impl Harness {
    /// The title page and both image indexes of the movie.
    fn route_artwork(&self, poster: &str, still_frame: &str) {
        self.server
            .route("/title/tt0113277/", page("title_movie.html"));
        self.server
            .route_query(INDEX_PATH, "contentTypes=poster", page(poster));
        self.server
            .route_query(INDEX_PATH, "contentTypes=still_frame", page(still_frame));
    }
}

fn file_names(list: &[cv_providers::model::ArtworkCandidate], kind: MediaFileType) -> Vec<String> {
    list.iter()
        .filter(|c| c.kind == kind)
        .map(|c| c.url.rsplit('/').next().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn artwork_offers_the_main_poster_first_then_portrait_posters_and_landscape_fanart() {
    let h = Harness::new().await;
    h.route_artwork("mediaindex_poster.html", "mediaindex_still_frame.html");
    let list = h
        .provider
        .artwork(
            MediaKind::Movie,
            &imdb_ids("tt0113277"),
            &RequestOptions::default(),
        )
        .await
        .unwrap();

    // The main poster sorts first with IMDb's score of 10; the landscape "poster", the entry
    // without a URL and the repeated image are dropped.
    assert_eq!(
        file_names(&list, MediaFileType::Poster),
        [
            "heat_poster._V1_.jpg",
            "heat_alt1._V1_.jpg",
            "heat_alt2._V1_.jpg"
        ]
    );
    let poster = &list[0];
    assert_eq!(poster.score, 10.0);
    assert_eq!((poster.width, poster.height), (2000, 3000));
    assert_eq!(poster.size_bucket(), Some(SizeBucket::XLarge));
    assert_eq!(poster.language, None);
    assert_eq!(list[1].score, 0.0);
    assert_eq!(list[1].size_bucket(), Some(SizeBucket::Large));

    // Still frames are fanart; the portrait one is dropped.
    assert_eq!(
        file_names(&list, MediaFileType::Fanart),
        ["heat_still1._V1_.jpg", "heat_still2._V1_.jpg"]
    );
    let fanart: Vec<_> = list
        .iter()
        .filter(|c| c.kind == MediaFileType::Fanart)
        .collect();
    assert_eq!(fanart[0].size_bucket(), Some(SizeBucket::Large));
    assert_eq!(fanart[1].size_bucket(), Some(SizeBucket::XLarge));
    assert_eq!(list.len(), 5);
}

#[tokio::test]
async fn artwork_reads_the_title_page_and_both_indexes_as_a_browser() {
    let h = Harness::new().await;
    h.route_artwork("mediaindex_poster.html", "mediaindex_still_frame.html");
    h.provider
        .artwork(
            MediaKind::Movie,
            &imdb_ids("tt0113277"),
            &RequestOptions::default(),
        )
        .await
        .unwrap();
    let requests = h.server.requests();
    let targets: Vec<&str> = requests.iter().map(|r| r.target.as_str()).collect();
    assert_eq!(targets.len(), 3, "{targets:?}");
    assert_eq!(targets[0], "/title/tt0113277/");
    assert!(targets.contains(&"/title/tt0113277/mediaindex/?contentTypes=poster"));
    assert!(targets.contains(&"/title/tt0113277/mediaindex/?contentTypes=still_frame"));
    assert!(requests.iter().all(|r| {
        r.headers
            .get("user-agent")
            .is_some_and(|ua| ua.contains("Mozilla"))
    }));
}

#[tokio::test]
async fn a_title_without_indexed_images_still_offers_its_main_poster() {
    let h = Harness::new().await;
    h.route_artwork("mediaindex_empty.html", "mediaindex_empty.html");
    let list = h
        .provider
        .artwork(
            MediaKind::Movie,
            &imdb_ids("tt0113277"),
            &RequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].kind, MediaFileType::Poster);
}

#[tokio::test]
async fn one_unreadable_index_does_not_cost_the_other() {
    let h = Harness::new().await;
    h.route_artwork("mediaindex_changed.html", "mediaindex_still_frame.html");
    let list = h
        .provider
        .artwork(
            MediaKind::Movie,
            &imdb_ids("tt0113277"),
            &RequestOptions::default(),
        )
        .await
        .unwrap();
    // The main poster comes from the title page; the poster index is skipped.
    assert_eq!(
        file_names(&list, MediaFileType::Poster),
        ["heat_poster._V1_.jpg"]
    );
    assert_eq!(file_names(&list, MediaFileType::Fanart).len(), 2);
}

#[tokio::test]
async fn when_no_index_can_be_read_the_format_changed() {
    let h = Harness::new().await;
    h.route_artwork("mediaindex_changed.html", "mediaindex_changed.html");
    let err = h
        .provider
        .artwork(
            MediaKind::Movie,
            &imdb_ids("tt0113277"),
            &RequestOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::FormatChanged(_)), "{err:?}");
}

#[tokio::test]
async fn artwork_of_the_wrong_kind_or_an_unknown_title_is_not_found() {
    let h = Harness::new().await;
    h.route_artwork("mediaindex_poster.html", "mediaindex_still_frame.html");
    // The title is a movie.
    let err = h
        .provider
        .artwork(
            MediaKind::Show,
            &imdb_ids("tt0113277"),
            &RequestOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
    // An ID IMDb does not know (the mock answers 404), and an ID that is not a title ID.
    let err = h
        .provider
        .artwork(
            MediaKind::Movie,
            &imdb_ids("tt9999999"),
            &RequestOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
    let err = h
        .provider
        .artwork(
            MediaKind::Movie,
            &imdb_ids("nm0000093"),
            &RequestOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

//! The fernsehserien.de provider against a local mock server that replays hand-built pages in the
//! shape of the site's schema.org structured data (`tests/fixtures/fernsehserien/`). The real
//! site answers programs with a bot-check challenge, so nothing here was recorded from it.
//! Nothing here touches the network; the opt-in live smoke test is
//! `tests/fernsehserien_live.rs`.

mod support;

use std::path::{Path, PathBuf};

use cv_domain::{
    AiredStatus, CalendarDate, CreditKind, EpisodeGroup, Genre, ItemDetail, MediaFileType,
    ProviderIds, id_key,
};
use cv_providers::fernsehserien::{FernsehserienProvider, RATING_ID};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::{HttpClient, MetadataProvider, Provider, ProviderError};
use support::{MockServer, Reply};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fernsehserien")
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
    provider: FernsehserienProvider,
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
        let provider = FernsehserienProvider::with_base_url(http, &server.url(""));
        Self {
            server,
            provider,
            cache_dir,
        }
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

fn slug_ids(slug: &str) -> ProviderIds {
    let mut ids = ProviderIds::new();
    ids.set("fernsehserien", slug);
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
async fn the_provider_is_labelled_best_effort() {
    let h = Harness::new().await;
    let capabilities = h.provider.capabilities();
    assert!(capabilities.best_effort);
    assert!(capabilities.movies && capabilities.shows);
    assert_eq!(capabilities.lookup_ids, vec!["fernsehserien"]);
    assert_eq!(h.provider.id(), "fernsehserien");
    assert_eq!(h.provider.name(), "fernsehserien.de");
    assert!(h.provider.credentials().is_none());
}

// --- search -----------------------------------------------------------------------------------

#[tokio::test]
async fn search_reads_the_links_to_shows_in_the_main_content() {
    let h = Harness::new().await;
    h.server.route("/suche/wache", page("search_results.html"));

    let hits = h
        .provider
        .search(&query(MediaKind::Show, "wache"), &opts())
        .await
        .unwrap();

    // Section links, the "more results" link and the repeated entry are not hits.
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].provider, "fernsehserien");
    assert_eq!(hits[0].ids.get("fernsehserien"), Some("die-beispielwache"));
    assert_eq!(hits[0].title, "Die Beispielwache");
    assert_eq!(hits[0].year, 2005);
    assert!(!hits[0].exact_id);
    assert_eq!(hits[1].ids.get("fernsehserien"), Some("die-stille-wache"));
    assert_eq!(hits[1].year, 0);
}

#[tokio::test]
async fn the_query_is_escaped_as_one_path_segment() {
    let h = Harness::new().await;
    h.server
        .route("/suche/Die%20Sendung%2FExtra", page("search_empty.html"));
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "Die Sendung/Extra"), &opts())
        .await
        .unwrap();
    assert!(hits.is_empty());
    assert_eq!(
        h.server.requests()[0].path(),
        "/suche/Die%20Sendung%2FExtra"
    );
}

#[tokio::test]
async fn a_search_that_jumps_to_the_show_returns_that_show() {
    let h = Harness::new().await;
    // The site redirects a unique match to the show page; the client follows it.
    h.server
        .route("/suche/beispielwache", page("show_page.html"));
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "beispielwache"), &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].ids.get("fernsehserien"), Some("die-beispielwache"));
    assert_eq!(hits[0].title, "Die Beispielwache");
    assert_eq!(hits[0].year, 2005);
}

#[tokio::test]
async fn a_search_without_results_is_empty_and_blank_titles_make_no_request() {
    let h = Harness::new().await;
    h.server.route("/suche/xyz", page("search_empty.html"));
    assert!(
        h.provider
            .search(&query(MediaKind::Show, "xyz"), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    let before = h.server.requests().len();
    assert!(
        h.provider
            .search(&query(MediaKind::Show, "  "), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(h.server.requests().len(), before);
}

// --- details ----------------------------------------------------------------------------------

#[tokio::test]
async fn a_show_page_gives_a_show_from_its_structured_data() {
    let h = Harness::new().await;
    h.server.route("/die-beispielwache", page("show_page.html"));

    let item = h
        .provider
        .details(MediaKind::Show, &slug_ids("die-beispielwache"), &opts())
        .await
        .unwrap();

    let core = &item.core;
    assert_eq!(core.title, "Die Beispielwache");
    assert_eq!(core.year, 2005);
    assert_eq!(core.ids.get("fernsehserien"), Some("die-beispielwache"));
    assert_eq!(core.ids.get(id_key::IMDB), Some("tt0765432"));
    assert_eq!(
        core.plot,
        "Zwei Polizisten betreuen eine Wache am Rand der Stadt & erleben mehr, als ihnen lieb ist."
    );
    assert_eq!(
        core.artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("https://img.example.test/fernsehserien/beispielwache.jpg")
    );
    let rating = core.ratings.get(RATING_ID).expect("a rating");
    assert!((rating.value - 4.2).abs() < 1e-6);
    assert_eq!((rating.votes, rating.max_value), (318, 5));

    let credits: Vec<_> = core
        .credits
        .iter()
        .map(|c| (c.kind, c.person.name.as_str()))
        .collect();
    assert_eq!(
        credits,
        [
            (CreditKind::Director, "Clara Muster"),
            (CreditKind::Actor, "Anna Beispiel"),
            (CreditKind::Actor, "Bernd Probe"),
        ]
    );

    let ItemDetail::Show(show) = &item.detail else {
        panic!("a show");
    };
    assert_eq!(show.first_aired, CalendarDate::new(2005, 3, 14));
    assert_eq!(show.year_end, 2011);
    assert_eq!(show.status, AiredStatus::Ended);
    assert_eq!(show.country, "DE");
    assert_eq!(
        show.genres,
        [
            Genre::parse("Crime").unwrap(),
            Genre::parse("Comedy").unwrap(),
            Genre::parse("Serie").unwrap(),
        ]
    );
    // The page names its cast, so the cast page is not fetched.
    assert_eq!(h.server.count("/die-beispielwache/cast-crew"), 0);
}

#[tokio::test]
async fn a_page_without_cast_asks_the_cast_page() {
    let h = Harness::new().await;
    h.server
        .route("/die-stille-wache", page("show_no_cast.html"));
    h.server
        .route("/die-stille-wache/cast-crew", page("cast_crew.html"));

    let item = h
        .provider
        .details(MediaKind::Show, &slug_ids("die-stille-wache"), &opts())
        .await
        .unwrap();
    let credits: Vec<_> = item
        .core
        .credits
        .iter()
        .map(|c| (c.kind, c.person.name.as_str()))
        .collect();
    assert_eq!(
        credits,
        [
            (CreditKind::Director, "Frieda Beispiel"),
            (CreditKind::Actor, "Dora Probe"),
            (CreditKind::Actor, "Emil Muster"),
        ]
    );
    let ItemDetail::Show(show) = &item.detail else {
        panic!("a show");
    };
    assert_eq!(show.status, AiredStatus::Continuing);
}

#[tokio::test]
async fn an_unreadable_cast_page_does_not_fail_the_details() {
    let h = Harness::new().await;
    h.server
        .route("/die-stille-wache", page("show_no_cast.html"));
    h.server
        .route("/die-stille-wache/cast-crew", Reply::status(403));
    let item = h
        .provider
        .details(MediaKind::Show, &slug_ids("die-stille-wache"), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Die stille Wache");
    assert!(item.core.credits.is_empty());
}

#[tokio::test]
async fn heading_and_meta_tags_stand_in_for_missing_structured_data() {
    let h = Harness::new().await;
    h.server.route("/nur-ein-titel", page("show_bare.html"));
    h.server
        .route("/nur-ein-titel/cast-crew", Reply::status(404));
    let item = h
        .provider
        .details(MediaKind::Show, &slug_ids("nur-ein-titel"), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Nur ein Titel");
    assert_eq!(
        item.core.plot,
        "Eine Kurzbeschreibung aus den Meta-Angaben."
    );
    assert_eq!(
        item.core
            .artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("https://img.example.test/fernsehserien/bare.jpg")
    );
    assert_eq!(item.core.year, 0);
    assert!(item.core.ratings.is_empty());
}

#[tokio::test]
async fn a_tv_film_is_a_movie_and_not_a_show() {
    let h = Harness::new().await;
    h.server.route("/der-beispielfilm", page("film_page.html"));

    let item = h
        .provider
        .details(MediaKind::Movie, &slug_ids("der-beispielfilm"), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Der Beispielfilm");
    assert_eq!(item.core.year, 2012);
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("a movie");
    };
    assert_eq!(movie.release_date, CalendarDate::new(2012, 10, 3));
    assert_eq!(movie.genres, [Genre::parse("Drama").unwrap()]);

    assert_eq!(
        h.provider
            .details(MediaKind::Show, &slug_ids("der-beispielfilm"), &opts())
            .await,
        Err(ProviderError::NotFound("fernsehserien.de".to_owned()))
    );
    assert_eq!(
        h.provider
            .lookup(MediaKind::Show, &slug_ids("der-beispielfilm"), &opts())
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn lookup_returns_an_exact_hit_and_ignores_ids_it_does_not_know() {
    let h = Harness::new().await;
    h.server.route("/die-beispielwache", page("show_page.html"));
    let hit = h
        .provider
        .lookup(MediaKind::Show, &slug_ids("die-beispielwache"), &opts())
        .await
        .unwrap()
        .expect("a hit");
    assert!(hit.exact_id);
    assert_eq!(hit.year, 2005);
    assert_eq!(hit.ids.get(id_key::IMDB), Some("tt0765432"));

    let before = h.server.requests().len();
    let mut other = ProviderIds::new();
    other.set(id_key::IMDB, "tt0765432");
    for ids in [other, ProviderIds::new(), slug_ids("../secret")] {
        assert_eq!(
            h.provider
                .lookup(MediaKind::Show, &ids, &opts())
                .await
                .unwrap(),
            None
        );
    }
    assert_eq!(h.server.requests().len(), before);

    // A show the site does not have (the mock answers 404).
    assert_eq!(
        h.provider
            .lookup(MediaKind::Show, &slug_ids("gibt-es-nicht"), &opts())
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        h.provider
            .details(MediaKind::Show, &slug_ids("gibt-es-nicht"), &opts())
            .await,
        Err(ProviderError::NotFound("fernsehserien.de".to_owned()))
    );
}

// --- episodes ---------------------------------------------------------------------------------

#[tokio::test]
async fn the_episode_guide_and_the_season_pages_it_links_to_give_the_episodes() {
    let h = Harness::new().await;
    h.server
        .route("/die-beispielwache/episodenguide", page("guide.html"));
    // The guide lists seasons 1 and 2 and links a third one.
    h.server.route(
        "/die-beispielwache/episodenguide/staffel-3/1003",
        page("guide_season3.html"),
    );

    let items = h
        .provider
        .episodes(
            &slug_ids("die-beispielwache"),
            &EpisodeGroup::aired(),
            &opts(),
        )
        .await
        .unwrap();

    let rows: Vec<_> = items
        .iter()
        .map(|item| {
            let ItemDetail::Episode(episode) = &item.detail else {
                panic!("an episode");
            };
            let number = episode.numbers.get(&EpisodeGroup::aired()).unwrap();
            (number.season, number.episode, item.core.title.as_str())
        })
        .collect();
    assert_eq!(
        rows,
        [
            (1, 1, "Der erste Tag"),
            (1, 2, "Der zweite Tag"),
            (2, 1, "Neuanfang"),
            (3, 1, "Drittes Jahr"),
            (3, 2, "Noch ein Fall"),
        ]
    );
    assert_eq!(items[0].core.plot, "Die Pilotfolge.");
    let ItemDetail::Episode(first) = &items[0].detail else {
        panic!("an episode");
    };
    assert_eq!(first.first_aired, CalendarDate::new(2005, 3, 14));
    // Seasons 1 and 2 are on the guide page; only season 3 needed its own page.
    assert_eq!(
        h.server
            .count("/die-beispielwache/episodenguide/staffel-1/1001"),
        0
    );
    assert_eq!(
        h.server
            .count("/die-beispielwache/episodenguide/staffel-3/1003"),
        1
    );
}

#[tokio::test]
async fn a_missing_season_page_leaves_that_season_out() {
    let h = Harness::new().await;
    h.server
        .route("/die-beispielwache/episodenguide", page("guide.html"));
    let items = h
        .provider
        .episodes(
            &slug_ids("die-beispielwache"),
            &EpisodeGroup::aired(),
            &opts(),
        )
        .await
        .unwrap();
    assert_eq!(items.len(), 3);
}

#[tokio::test]
async fn microdata_episodes_are_read_when_there_is_no_json() {
    let h = Harness::new().await;
    h.server
        .route("/die-stille-wache/episodenguide", page("guide_micro.html"));
    let items = h
        .provider
        .episodes(
            &slug_ids("die-stille-wache"),
            &EpisodeGroup::aired(),
            &opts(),
        )
        .await
        .unwrap();
    // The guide page names no season, so its episodes are season 1; the entry without a number is
    // skipped.
    let titles: Vec<_> = items.iter().map(|i| i.core.title.as_str()).collect();
    assert_eq!(titles, ["Stille", "Lärm"]);
    assert_eq!(items[0].core.plot, "Es beginnt.");
}

#[tokio::test]
async fn other_orderings_and_unknown_shows_have_no_episodes() {
    let h = Harness::new().await;
    assert_eq!(
        h.provider
            .episodes(
                &slug_ids("die-beispielwache"),
                &EpisodeGroup::dvd(),
                &opts()
            )
            .await,
        Err(ProviderError::NotFound("fernsehserien.de".to_owned()))
    );
    assert_eq!(
        h.provider
            .episodes(&ProviderIds::new(), &EpisodeGroup::aired(), &opts())
            .await,
        Err(ProviderError::NotFound("fernsehserien.de".to_owned()))
    );
}

// --- page format changed: typed errors, nothing written ---------------------------------------

fn assert_format_changed<T: std::fmt::Debug>(result: Result<T, ProviderError>) {
    assert_eq!(
        result.unwrap_err(),
        ProviderError::FormatChanged("fernsehserien.de".to_owned())
    );
}

#[tokio::test]
async fn a_page_whose_format_changed_fails_and_writes_nothing() {
    let h = Harness::new().await;

    h.server
        .route("/die-beispielwache", page("show_changed.html"));
    let result = h
        .provider
        .details(MediaKind::Show, &slug_ids("die-beispielwache"), &opts())
        .await;
    assert!(
        result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("page format changed")
    );
    assert_format_changed(result);
    assert_format_changed(
        h.provider
            .lookup(MediaKind::Show, &slug_ids("die-beispielwache"), &opts())
            .await,
    );

    h.server.route("/suche/wache", page("search_changed.html"));
    assert_format_changed(
        h.provider
            .search(&query(MediaKind::Show, "wache"), &opts())
            .await,
    );

    h.server.route(
        "/die-beispielwache/episodenguide",
        page("guide_changed.html"),
    );
    assert_format_changed(
        h.provider
            .episodes(
                &slug_ids("die-beispielwache"),
                &EpisodeGroup::aired(),
                &opts(),
            )
            .await,
    );
    assert!(
        h.cached_files().is_empty(),
        "unreadable pages must not stay in the cache"
    );
}

#[tokio::test]
async fn a_failed_scrape_leaves_the_existing_item_untouched() {
    let h = Harness::new().await;
    h.server.route("/die-beispielwache", page("show_page.html"));
    let mut stored = h
        .provider
        .details(MediaKind::Show, &slug_ids("die-beispielwache"), &opts())
        .await
        .unwrap();
    stored.core.plot = "My own edited plot".to_owned();
    let before = stored.clone();

    h.server
        .route("/die-beispielwache", page("show_changed.html"));
    let mut forced = opts();
    forced.bypass_cache = true;
    let result = h
        .provider
        .details(MediaKind::Show, &slug_ids("die-beispielwache"), &forced)
        .await;
    assert!(matches!(result, Err(ProviderError::FormatChanged(_))));
    assert_eq!(stored, before);
}

// --- bot check, caching -----------------------------------------------------------------------

#[tokio::test]
async fn the_bot_check_is_a_failed_request_and_is_not_cached() {
    let h = Harness::new().await;
    for code in [202, 403] {
        h.server.route("/die-beispielwache", Reply::status(code));
        let result = h
            .provider
            .details(MediaKind::Show, &slug_ids("die-beispielwache"), &opts())
            .await;
        let Err(ProviderError::Request { provider, message }) = result else {
            panic!("HTTP {code} should be a failed request");
        };
        assert_eq!(provider, "fernsehserien.de");
        assert!(message.contains("human check"), "{message}");
        assert!(h.cached_files().is_empty(), "HTTP {code}");
    }
}

#[tokio::test]
async fn pages_are_cached_and_a_forced_rescrape_goes_to_the_network() {
    let h = Harness::new().await;
    h.server.route("/die-beispielwache", page("show_page.html"));
    for _ in 0..2 {
        h.provider
            .details(MediaKind::Show, &slug_ids("die-beispielwache"), &opts())
            .await
            .unwrap();
    }
    assert_eq!(h.server.count("/die-beispielwache"), 1);

    let mut forced = opts();
    forced.bypass_cache = true;
    h.provider
        .details(MediaKind::Show, &slug_ids("die-beispielwache"), &forced)
        .await
        .unwrap();
    assert_eq!(h.server.count("/die-beispielwache"), 2);
}

#[tokio::test]
async fn pages_are_requested_in_german_like_a_browser() {
    let h = Harness::new().await;
    h.server.route("/die-beispielwache", page("show_page.html"));
    h.provider
        .details(MediaKind::Show, &slug_ids("die-beispielwache"), &opts())
        .await
        .unwrap();
    let request = &h.server.requests()[0];
    assert!(request.headers["user-agent"].contains("Mozilla/5.0"));
    assert!(request.headers["accept-language"].starts_with("de-DE"));
}

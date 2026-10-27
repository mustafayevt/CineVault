//! The TMDB provider against a local mock server that replays hand-built responses in TMDB's
//! documented shape (`tests/fixtures/tmdb/`). Nothing here touches the network; the opt-in live
//! smoke test is `tests/tmdb_live.rs`.

mod support;

use std::path::Path;
use std::sync::Arc;

use cv_domain::{
    AiredStatus, CalendarDate, Certification, CreditKind, EpisodeGroup, EpisodeNumber,
    EpisodeOrdering, Genre, ItemDetail, MediaFileType, ProviderIds, id_key,
};
use cv_providers::completion::IdSource;
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::tmdb::TmdbProvider;
use cv_providers::{
    AnimeCrossReference, ArtworkProvider, CredentialResolver, HttpClient, MemoryUserCredentials,
    MetadataProvider, Provider, ProviderError, ProviderRegistry, RatingProvider, catalogue,
    complete_ids,
};
use support::{MockServer, Recorded, Reply};

const KEY: &str = "test-key-0123456789";

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tmdb")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn ok(name: &str) -> Reply {
    Reply::ok(&fixture(name))
}

fn error(status: u16, name: &str) -> Reply {
    let mut reply = Reply::ok(&fixture(name));
    reply.status = status;
    reply
}

struct Harness {
    server: MockServer,
    provider: TmdbProvider,
    resolver: CredentialResolver,
    _cache: Option<tempfile::TempDir>,
}

impl Harness {
    async fn new() -> Self {
        Self::build(false, true).await
    }

    async fn with_cache() -> Self {
        Self::build(true, true).await
    }

    async fn without_key() -> Self {
        Self::build(false, false).await
    }

    async fn build(cache: bool, key: bool) -> Self {
        let server = MockServer::start().await;
        server.route("/3/configuration", ok("configuration.json"));
        let cache_dir = cache.then(|| tempfile::tempdir().unwrap());
        let mut builder = HttpClient::builder();
        if let Some(dir) = &cache_dir {
            builder = builder.cache_dir(dir.path());
        }
        let http = builder.build().unwrap();
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        if key {
            resolver
                .set_user(catalogue::key::TMDB, catalogue::API_KEY, KEY)
                .unwrap();
        }
        let provider = TmdbProvider::with_base_url(http, resolver.clone(), &server.url("/3"));
        Self {
            server,
            provider,
            resolver,
            _cache: cache_dir,
        }
    }

    /// Requests that were not the one-time `/configuration` call.
    fn calls(&self) -> Vec<Recorded> {
        self.server
            .requests()
            .into_iter()
            .filter(|r| r.path() != "/3/configuration")
            .collect()
    }
}

fn opts() -> RequestOptions {
    RequestOptions::default()
}

fn param(request: &Recorded, name: &str) -> Option<String> {
    let url = reqwest::Url::parse(&format!("http://mock{}", request.target)).unwrap();
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

fn ids(pairs: &[(&str, &str)]) -> ProviderIds {
    pairs.iter().copied().collect()
}

fn movie_query(title: &str, year: Option<u16>) -> SearchQuery {
    SearchQuery {
        kind: MediaKind::Movie,
        title: title.to_owned(),
        year,
    }
}

// --- search: the ID-first cascade -----------------------------------------------------------

#[tokio::test]
async fn an_imdb_id_resolves_the_title_without_a_text_search() {
    let h = Harness::new().await;
    h.server
        .route("/3/find/tt0113277", ok("find_imdb_heat.json"));

    let hit = h
        .provider
        .lookup(MediaKind::Movie, &ids(&[("imdb", "tt0113277")]), &opts())
        .await
        .unwrap()
        .expect("Heat");
    assert_eq!(hit.title, "Heat");
    assert_eq!(hit.year, 1995);
    assert_eq!(hit.provider, "tmdb");
    assert_eq!(hit.ids.get(id_key::TMDB), Some("949"));
    assert_eq!(hit.ids.get(id_key::IMDB), Some("tt0113277"));
    assert!(hit.exact_id);
    assert_eq!(
        hit.poster_url.as_deref(),
        Some("https://image.tmdb.org/t/p/w342/p.jpg")
    );

    let calls = h.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path(), "/3/find/tt0113277");
    assert_eq!(
        param(&calls[0], "external_source").as_deref(),
        Some("imdb_id")
    );
    assert_eq!(param(&calls[0], "api_key").as_deref(), Some(KEY));
}

#[tokio::test]
async fn search_with_an_imdb_id_never_runs_the_text_search() {
    let h = Harness::new().await;
    h.server
        .route("/3/find/tt0113277", ok("find_imdb_heat.json"));

    let hits = h
        .provider
        .search_with_ids(
            &movie_query("Heat", Some(1995)),
            &ids(&[("imdb", "tt0113277")]),
            &opts(),
        )
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].exact_id);
    assert_eq!(h.server.count("/3/search/movie"), 0);
}

#[tokio::test]
async fn a_tmdb_id_is_read_directly_and_settles_the_search() {
    let h = Harness::new().await;
    h.server.route("/3/movie/949", ok("movie_949.json"));

    let hits = h
        .provider
        .search_with_ids(
            &movie_query("Heat", Some(1995)),
            &ids(&[("tmdb", "949"), ("imdb", "tt0113277")]),
            &opts(),
        )
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].ids.get(id_key::TMDB), Some("949"));
    assert_eq!(hits[0].ids.get(id_key::IMDB), Some("tt0113277"));
    assert!(hits[0].exact_id);
    assert_eq!(h.server.count("/3/find/tt0113277"), 0);
    assert_eq!(h.server.count("/3/search/movie"), 0);
}

#[tokio::test]
async fn an_unknown_id_falls_through_to_the_next_step() {
    let h = Harness::new().await;
    h.server.route("/3/movie/1", error(404, "error_404.json"));
    h.server
        .route("/3/find/tt0113277", ok("find_imdb_heat.json"));

    let hit = h
        .provider
        .lookup(
            MediaKind::Movie,
            &ids(&[("tmdb", "1"), ("imdb", "tt0113277")]),
            &opts(),
        )
        .await
        .unwrap()
        .expect("found through IMDb");
    assert_eq!(hit.ids.get(id_key::TMDB), Some("949"));
}

#[tokio::test]
async fn a_tvdb_id_finds_a_show() {
    let h = Harness::new().await;
    h.server
        .route("/3/find/81189", ok("find_tvdb_breaking_bad.json"));

    let hit = h
        .provider
        .lookup(MediaKind::Show, &ids(&[("tvdb", "81189")]), &opts())
        .await
        .unwrap()
        .expect("Breaking Bad");
    assert_eq!(hit.title, "Breaking Bad");
    assert_eq!(hit.ids.get(id_key::TMDB), Some("1396"));
    assert_eq!(hit.ids.get(id_key::TVDB), Some("81189"));
    assert_eq!(
        param(&h.calls()[0], "external_source").as_deref(),
        Some("tvdb_id")
    );
}

#[tokio::test]
async fn text_search_reads_pages_and_drops_results_without_a_title() {
    let h = Harness::new().await;
    h.server.script(
        "/3/search/movie",
        vec![ok("search_movie_page1.json"), ok("search_movie_page2.json")],
    );

    let hits = h
        .provider
        .search(&movie_query("Heat", Some(1995)), &opts())
        .await
        .unwrap();
    let titles: Vec<&str> = hits.iter().map(|h| h.title.as_str()).collect();
    assert_eq!(titles, vec!["Heat", "Heat Wave", "Heat Night"]);
    assert!(hits.iter().all(|h| !h.exact_id));
    assert_eq!(hits[0].year, 1995);
    assert_eq!(hits[1].poster_url, None);

    let calls = h.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(param(&calls[0], "query").as_deref(), Some("Heat"));
    assert_eq!(param(&calls[0], "year").as_deref(), Some("1995"));
    assert_eq!(param(&calls[0], "page").as_deref(), Some("1"));
    assert_eq!(param(&calls[1], "page").as_deref(), Some("2"));
}

#[tokio::test]
async fn a_show_search_uses_the_first_air_year() {
    let h = Harness::new().await;
    h.server.route("/3/search/tv", ok("search_tv.json"));

    let hits = h
        .provider
        .search(
            &SearchQuery {
                kind: MediaKind::Show,
                title: "Breaking Bad".into(),
                year: Some(2008),
            },
            &opts(),
        )
        .await
        .unwrap();
    assert_eq!(hits[0].title, "Breaking Bad");
    assert_eq!(hits[0].year, 2008);
    assert_eq!(
        param(&h.calls()[0], "first_air_date_year").as_deref(),
        Some("2008")
    );
}

#[tokio::test]
async fn a_number_is_also_tried_as_a_tmdb_id_but_is_not_an_exact_match() {
    let h = Harness::new().await;
    h.server.route("/3/movie/1917", ok("movie_1917.json"));
    h.server
        .route("/3/search/movie", ok("search_movie_digits.json"));

    let hits = h
        .provider
        .search(&movie_query("1917", None), &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].title, "A Different Film");
    assert!(!hits[0].exact_id);
    assert_eq!(hits[1].title, "1917");
}

#[tokio::test]
async fn a_text_hit_repeating_the_id_hit_is_listed_once() {
    let h = Harness::new().await;
    h.server.route("/3/movie/949", ok("movie_949.json"));
    h.server
        .route("/3/search/movie", ok("search_movie_single.json"));

    let hits = h
        .provider
        .search(&movie_query("949", None), &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].ids.get(id_key::TMDB), Some("949"));
}

#[tokio::test]
async fn a_title_ending_in_a_year_is_searched_again_without_it() {
    let h = Harness::new().await;
    h.server.script(
        "/3/search/movie",
        vec![
            ok("search_movie_empty.json"),
            ok("search_movie_single.json"),
        ],
    );

    let hits = h
        .provider
        .search(&movie_query("Heat 1995", None), &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    let calls = h.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(param(&calls[0], "query").as_deref(), Some("Heat 1995"));
    assert_eq!(param(&calls[1], "query").as_deref(), Some("Heat"));
}

#[tokio::test]
async fn nothing_found_is_an_empty_list() {
    let h = Harness::new().await;
    h.server
        .route("/3/search/movie", ok("search_movie_empty.json"));
    let hits = h
        .provider
        .search(&movie_query("No Such Film", None), &opts())
        .await
        .unwrap();
    assert!(hits.is_empty());
}

// --- details --------------------------------------------------------------------------------

#[tokio::test]
async fn movie_details_carry_ids_people_ratings_and_the_countrys_certification() {
    let h = Harness::new().await;
    h.server.route("/3/movie/949", ok("movie_949.json"));

    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.id, None);
    assert_eq!(item.core.title, "Heat");
    assert_eq!(item.core.original_title, "Heat");
    assert_eq!(item.core.original_language, "en");
    assert_eq!(item.core.year, 1995);
    assert_eq!(
        item.core.plot.chars().take(9).collect::<String>(),
        "Obsessive"
    );
    assert_eq!(item.core.ids.get(id_key::TMDB), Some("949"));
    assert_eq!(item.core.ids.get(id_key::IMDB), Some("tt0113277"));
    assert_eq!(item.core.ids.get(id_key::WIKIDATA), Some("Q220394"));
    assert!(!item.core.ids.contains(id_key::TMDB_SET));
    assert_eq!(
        item.core.production_companies,
        vec!["Regency Enterprises", "Forward Pass"]
    );
    let rating = item.core.ratings.get("tmdb").expect("tmdb rating");
    assert_eq!(
        (rating.value, rating.votes, rating.max_value),
        (7.9, 7100, 10)
    );
    let tags: Vec<&str> = item.core.tags.iter().collect();
    assert_eq!(tags, vec!["robbery", "Los Angeles"]);
    assert_eq!(
        item.core
            .artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("https://image.tmdb.org/t/p/original/zMyfPUelumio3tiDKPffaUT8Ms.jpg")
    );
    assert_eq!(
        item.core
            .artwork_urls
            .get(&MediaFileType::Fanart)
            .map(String::as_str),
        Some("https://image.tmdb.org/t/p/original/rrBuGu0Pjq7Y2BWSI6teXfXnTEB.jpg")
    );

    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("not a movie");
    };
    assert_eq!(movie.runtime_minutes, 170);
    assert_eq!(movie.tagline, "A Los Angeles Crime Saga");
    assert_eq!(movie.english_title, "Heat");
    // The type 1 premiere on 6 December is ignored; the theatrical release counts.
    assert_eq!(movie.release_date, CalendarDate::new(1995, 12, 15));
    assert_eq!(movie.certification, Certification("US_R".to_owned()));
    assert_eq!(
        movie.genres,
        vec![Genre::Action, Genre::Crime, Genre::Drama, Genre::Thriller]
    );
    assert_eq!(movie.spoken_languages, vec!["en", "es"]);
    assert_eq!(movie.countries, vec!["US"]);

    let request = &h.calls()[0];
    assert_eq!(
        param(request, "append_to_response").as_deref(),
        Some("credits,keywords,release_dates,translations,external_ids,videos")
    );
    assert_eq!(param(request, "language").as_deref(), Some("en-US"));
}

#[tokio::test]
async fn cast_comes_in_billing_order_followed_by_the_kept_crew_roles() {
    let h = Harness::new().await;
    h.server.route("/3/movie/949", ok("movie_949.json"));
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &opts())
        .await
        .unwrap();

    let summary: Vec<(CreditKind, &str, &str)> = item
        .core
        .credits
        .iter()
        .map(|c| (c.kind, c.person.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(
        summary,
        vec![
            (CreditKind::Actor, "Al Pacino", "Lt. Vincent Hanna"),
            (CreditKind::Actor, "Robert De Niro", "Neil McCauley"),
            (CreditKind::Actor, "Val Kilmer", "Chris Shiherlis"),
            (CreditKind::Director, "Michael Mann", "Director"),
            (CreditKind::Writer, "Michael Mann", "Screenplay"),
            // Listed twice by TMDB, kept once; the gaffer is not a kept role.
            (CreditKind::Producer, "Michael Mann", "Producer"),
            (
                CreditKind::Camera,
                "Dante Spinotti",
                "Director of Photography"
            ),
            (CreditKind::Editor, "Pasquale Buba", "Editor"),
            (
                CreditKind::Composer,
                "Elliot Goldenthal",
                "Original Music Composer"
            ),
        ]
    );
    let pacino = &item.core.credits[0].person;
    assert_eq!(pacino.ids.get(id_key::TMDB), Some("1158"));
    assert_eq!(
        pacino.thumb_url,
        "https://image.tmdb.org/t/p/h632/fMDFeVf0pjopTJbyRSLFwNDm8Wr.jpg"
    );
    assert_eq!(item.core.credits[2].person.thumb_url, "");
}

#[tokio::test]
async fn the_certification_and_release_date_follow_the_configured_country() {
    let h = Harness::new().await;
    h.server.route("/3/movie/949", ok("movie_949.json"));
    // By default the premiered date is the original release, whatever the country.
    let options = RequestOptions {
        country: "DE".into(),
        ..opts()
    };
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &options)
        .await
        .unwrap();
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("not a movie");
    };
    assert_eq!(movie.certification, Certification("DE_FSK16".to_owned()));
    assert_eq!(movie.release_date, CalendarDate::new(1995, 12, 15));

    let options = RequestOptions {
        country: "DE".into(),
        local_release_date: true,
        ..opts()
    };
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &options)
        .await
        .unwrap();
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("not a movie");
    };
    assert_eq!(movie.certification, Certification("DE_FSK16".to_owned()));
    assert_eq!(movie.release_date, CalendarDate::new(1996, 2, 29));
    // The year stays the film's own: a later release in the country does not move it.
    assert_eq!(item.core.year, 1995);

    // A country with no entry has no certification and uses the worldwide date.
    let options = RequestOptions {
        country: "FR".into(),
        local_release_date: true,
        ..opts()
    };
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &options)
        .await
        .unwrap();
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("not a movie");
    };
    assert_eq!(movie.certification, Certification::default());
    assert_eq!(movie.release_date, CalendarDate::new(1995, 12, 15));
}

#[tokio::test]
async fn details_by_imdb_id_find_the_tmdb_id_first() {
    let h = Harness::new().await;
    h.server
        .route("/3/find/tt0113277", ok("find_imdb_heat.json"));
    h.server.route("/3/movie/949", ok("movie_949.json"));
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("imdb", "tt0113277")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
}

#[tokio::test]
async fn details_without_any_usable_id_are_not_found() {
    let h = Harness::new().await;
    let err = h
        .provider
        .details(MediaKind::Movie, &ProviderIds::new(), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn a_missing_plot_comes_from_the_fallback_language_but_the_title_stays() {
    let h = Harness::new().await;
    h.server.route("/3/movie/949", ok("movie_949_tr.json"));
    let options = RequestOptions {
        language: "tr".into(),
        fallback_language: "en".into(),
        ..opts()
    };
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &options)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Ateş Hattı");
    assert!(item.core.plot.starts_with("Obsessive master thief"));
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("not a movie");
    };
    assert_eq!(movie.tagline, "A Los Angeles Crime Saga");
    assert_eq!(movie.english_title, "Heat");
    assert_eq!(param(&h.calls()[0], "language").as_deref(), Some("tr-TR"));
}

#[tokio::test]
async fn a_missing_title_comes_from_the_fallback_language_too() {
    let h = Harness::new().await;
    h.server
        .route("/3/movie/949", ok("movie_949_tr_notitle.json"));
    let options = RequestOptions {
        language: "tr".into(),
        fallback_language: "en".into(),
        ..opts()
    };
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &options)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
    assert_eq!(item.core.plot, "English plot.");
}

#[tokio::test]
async fn show_details_include_external_ids_status_certification_and_orderings() {
    let h = Harness::new().await;
    h.server.route("/3/tv/1396", ok("tv_1396.json"));

    let item = h
        .provider
        .details(MediaKind::Show, &ids(&[("tmdb", "1396")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Breaking Bad");
    assert_eq!(item.core.original_language, "en");
    assert_eq!(item.core.year, 2008);
    // Cross-provider ID completion relies on these coming back from the scrape.
    assert_eq!(item.core.ids.get(id_key::TVDB), Some("81189"));
    assert_eq!(item.core.ids.get(id_key::IMDB), Some("tt0903747"));
    assert_eq!(item.core.ids.get(id_key::WIKIDATA), Some("Q1079"));
    assert_eq!(
        item.core.production_companies,
        vec!["Sony Pictures Television Studios"]
    );
    assert_eq!(item.core.credits[0].person.name, "Bryan Cranston");
    assert_eq!(item.core.credits[1].kind, CreditKind::Producer);
    assert_eq!(
        item.core.tags.iter().collect::<Vec<_>>(),
        vec!["drug dealer"]
    );

    let ItemDetail::Show(show) = &item.detail else {
        panic!("not a show");
    };
    assert_eq!(show.status, AiredStatus::Ended);
    assert_eq!(show.first_aired, CalendarDate::new(2008, 1, 20));
    assert_eq!(show.year_end, 2013);
    assert_eq!(show.runtime_minutes, 45);
    assert_eq!(show.country, "US");
    assert_eq!(show.tagline, "Remember my name");
    assert_eq!(show.certification, Certification("US_TV_MA".to_owned()));
    assert_eq!(
        show.genres,
        vec![
            Genre::Drama,
            Genre::Crime,
            Genre::ScienceFiction,
            Genre::Fantasy
        ]
    );
    assert_eq!(show.episode_group, EpisodeGroup::aired());
    assert_eq!(
        show.episode_groups,
        vec![
            EpisodeGroup::aired(),
            EpisodeGroup::dvd(),
            EpisodeGroup::new(EpisodeOrdering::Alternate, "Production Order"),
        ]
    );
    assert_eq!(show.season_texts.len(), 2);
    assert_eq!(show.season_texts[1].season, 1);
    assert_eq!(show.season_texts[1].name, "Season 1");

    assert_eq!(
        param(&h.calls()[0], "append_to_response").as_deref(),
        Some("translations,credits,external_ids,content_ratings,keywords,episode_groups,videos")
    );
}

#[tokio::test]
async fn season_details_list_the_episodes() {
    let h = Harness::new().await;
    h.server
        .route("/3/tv/1396/season/1", ok("tv_1396_season_1.json"));

    let season = h
        .provider
        .season_details(&ids(&[("tmdb", "1396")]), 1, &opts())
        .await
        .unwrap();
    assert_eq!(season.season.core.title, "Season 1");
    assert_eq!(season.season.core.ids.get(id_key::TVDB), Some("30272"));
    assert_eq!(season.season.core.year, 2008);
    assert!(matches!(&season.season.detail, ItemDetail::Season(s) if s.season == 1));
    assert_eq!(
        season
            .season
            .core
            .artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("https://image.tmdb.org/t/p/original/s1.jpg")
    );

    assert_eq!(season.episodes.len(), 2);
    let first = &season.episodes[0];
    assert_eq!(first.core.title, "Pilot");
    assert_eq!(first.core.plot, "A teacher begins cooking.");
    assert_eq!(first.core.ids.get(id_key::TMDB), Some("62085"));
    let ItemDetail::Episode(fields) = &first.detail else {
        panic!("not an episode");
    };
    assert_eq!(fields.first_aired, CalendarDate::new(2008, 1, 20));
    assert_eq!(fields.runtime_minutes, 58);
    assert_eq!(
        fields.numbers.get(&EpisodeGroup::aired()),
        Some(&EpisodeNumber::aired(1, 1))
    );
    assert!(first.core.artwork_urls.contains_key(&MediaFileType::Thumb));
    assert!(first.core.ratings.get("tmdb").is_some());
}

#[tokio::test]
async fn episodes_missing_a_name_or_plot_are_filled_from_the_fallback_language() {
    let h = Harness::new().await;
    h.server.script(
        "/3/tv/1396/season/1",
        vec![
            ok("tv_1396_season_1_gaps.json"),
            ok("tv_1396_season_1.json"),
        ],
    );
    let options = RequestOptions {
        language: "de".into(),
        fallback_language: "en".into(),
        ..opts()
    };
    let season = h
        .provider
        .season_details(&ids(&[("tmdb", "1396")]), 1, &options)
        .await
        .unwrap();
    assert_eq!(season.season.core.title, "Staffel 1");
    assert_eq!(season.season.core.plot, "The first season.");
    // Kept in the preferred language where it exists, filled where it does not.
    assert_eq!(season.episodes[0].core.title, "Pilotfolge");
    assert_eq!(season.episodes[0].core.plot, "A teacher begins cooking.");
    assert!(season.episodes[1].core.title.starts_with("Cat"));
    assert_eq!(season.episodes[1].core.plot, "The aftermath.");

    let calls = h.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(param(&calls[0], "language").as_deref(), Some("de-DE"));
    assert_eq!(param(&calls[1], "language").as_deref(), Some("en-US"));
}

#[tokio::test]
async fn no_second_request_when_the_season_is_complete() {
    let h = Harness::new().await;
    h.server
        .route("/3/tv/1396/season/1", ok("tv_1396_season_1.json"));
    let options = RequestOptions {
        language: "de".into(),
        fallback_language: "en".into(),
        ..opts()
    };
    h.provider
        .season_details(&ids(&[("tmdb", "1396")]), 1, &options)
        .await
        .unwrap();
    assert_eq!(h.calls().len(), 1);
}

#[tokio::test]
async fn episode_details_include_guest_stars_crew_and_external_ids() {
    let h = Harness::new().await;
    h.server.route(
        "/3/tv/1396/season/1/episode/1",
        ok("tv_1396_episode_1_1.json"),
    );
    let episode = h
        .provider
        .episode_details(&ids(&[("tmdb", "1396")]), 1, 1, &opts())
        .await
        .unwrap();
    assert_eq!(episode.core.title, "Pilot");
    assert_eq!(episode.core.ids.get(id_key::TVDB), Some("349232"));
    assert_eq!(episode.core.ids.get(id_key::IMDB), Some("tt0959621"));
    let kinds: Vec<CreditKind> = episode.core.credits.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        vec![CreditKind::Guest, CreditKind::Director, CreditKind::Writer]
    );
}

#[tokio::test]
async fn the_aired_episode_list_reads_every_season_in_order() {
    let h = Harness::new().await;
    h.server.route("/3/tv/1396", ok("tv_1396.json"));
    h.server
        .route("/3/tv/1396/season/0", ok("tv_1396_season_0.json"));
    h.server
        .route("/3/tv/1396/season/1", ok("tv_1396_season_1.json"));

    let episodes = h
        .provider
        .episodes(&ids(&[("tmdb", "1396")]), &EpisodeGroup::aired(), &opts())
        .await
        .unwrap();
    let numbers: Vec<(u32, u32)> = episodes
        .iter()
        .map(|e| {
            let ItemDetail::Episode(f) = &e.detail else {
                panic!("not an episode")
            };
            let n = f.numbers.get(&EpisodeGroup::aired()).unwrap();
            (n.season, n.episode)
        })
        .collect();
    assert_eq!(numbers, vec![(0, 1), (1, 1), (1, 2)]);
}

#[tokio::test]
async fn the_dvd_episode_list_uses_the_episode_group_and_keeps_the_aired_numbers() {
    let h = Harness::new().await;
    h.server.route("/3/tv/1396", ok("tv_1396.json"));
    h.server.route(
        "/3/tv/episode_group/5acf93f1c3a368412c00191e",
        ok("tv_episode_group_dvd.json"),
    );

    let episodes = h
        .provider
        .episodes(&ids(&[("tmdb", "1396")]), &EpisodeGroup::dvd(), &opts())
        .await
        .unwrap();
    assert_eq!(episodes.len(), 3);

    let both = |e: &cv_domain::Item| {
        let ItemDetail::Episode(f) = &e.detail else {
            panic!("not an episode")
        };
        (
            f.numbers.get(&EpisodeGroup::aired()).cloned().unwrap(),
            f.numbers.get(&EpisodeGroup::dvd()).cloned().unwrap(),
        )
    };
    // The group lists "Cat's in the Bag..." first: DVD 1x01, aired 1x02.
    let (aired, dvd) = both(&episodes[0]);
    assert_eq!((aired.season, aired.episode), (1, 2));
    assert_eq!((dvd.season, dvd.episode), (1, 1));
    let (aired, dvd) = both(&episodes[1]);
    assert_eq!((aired.season, aired.episode), (1, 1));
    assert_eq!((dvd.season, dvd.episode), (1, 2));
    // The specials group has order 0.
    let (_, dvd) = both(&episodes[2]);
    assert_eq!((dvd.season, dvd.episode), (0, 1));

    // A DVD ordering stored with a name still finds the show's DVD group.
    let named = EpisodeGroup::new(EpisodeOrdering::Dvd, "DVD Order");
    let again = h
        .provider
        .episodes(&ids(&[("tmdb", "1396")]), &named, &opts())
        .await
        .unwrap();
    assert_eq!(again.len(), 3);
}

#[tokio::test]
async fn an_ordering_the_show_lacks_is_not_found() {
    let h = Harness::new().await;
    h.server.route("/3/tv/1396", ok("tv_1396.json"));
    let err = h
        .provider
        .episodes(
            &ids(&[("tmdb", "1396")]),
            &EpisodeGroup::absolute(),
            &opts(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

// --- collections ----------------------------------------------------------------------------

#[tokio::test]
async fn a_movie_in_a_collection_carries_the_collection_id() {
    let h = Harness::new().await;
    h.server.route("/3/movie/603", ok("movie_603.json"));
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "603")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.ids.get(id_key::TMDB_SET), Some("2344"));
}

#[tokio::test]
async fn collections_can_be_searched_and_read() {
    let h = Harness::new().await;
    h.server
        .route("/3/search/collection", ok("search_collection.json"));
    h.server
        .route("/3/collection/2344", ok("collection_2344.json"));

    let hits = h
        .provider
        .search_collections("Matrix", &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].ids.get(id_key::TMDB_SET), Some("2344"));
    assert!(!hits[0].ids.contains(id_key::TMDB));

    let collection = h.provider.collection(&hits[0].ids, &opts()).await.unwrap();
    assert_eq!(collection.item.core.title, "The Matrix Collection");
    assert_eq!(
        collection.item.core.plot,
        "A hacker discovers reality is a simulation."
    );
    assert_eq!(collection.item.core.ids.get(id_key::TMDB_SET), Some("2344"));
    assert!(matches!(collection.item.detail, ItemDetail::Set(_)));
    assert_eq!(
        collection
            .item
            .core
            .artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("https://image.tmdb.org/t/p/original/matrix-poster.jpg")
    );
    let movies: Vec<(&str, u16, Option<&str>)> = collection
        .movies
        .iter()
        .map(|m| (m.title.as_str(), m.year, m.ids.get(id_key::TMDB)))
        .collect();
    assert_eq!(
        movies,
        vec![
            ("The Matrix", 1999, Some("603")),
            ("The Matrix Reloaded", 2003, Some("604")),
            ("The Matrix Revolutions", 2003, Some("605")),
        ]
    );
    assert_eq!(
        collection.movies[0].release_date,
        CalendarDate::new(1999, 3, 30)
    );
    // The poster of a part is kept so a movie the library lacks can be drawn.
    assert_eq!(
        collection.movies[0].poster_url,
        "https://image.tmdb.org/t/p/w342/m1.jpg"
    );
}

#[tokio::test]
async fn the_metadata_trait_offers_collections_through_the_provider() {
    use cv_providers::MetadataProvider;

    let h = Harness::new().await;
    h.server
        .route("/3/search/collection", ok("search_collection.json"));
    h.server
        .route("/3/collection/2344", ok("collection_2344.json"));
    let provider: &dyn MetadataProvider = &h.provider;
    assert!(provider.offers_collections());

    let hits = provider
        .search_collections("Matrix", &opts())
        .await
        .unwrap();
    assert_eq!(hits[0].ids.get(id_key::TMDB_SET), Some("2344"));
    let collection = provider.collection(&hits[0].ids, &opts()).await.unwrap();
    assert_eq!(collection.item.core.title, "The Matrix Collection");
    assert_eq!(collection.movies.len(), 3);
}

#[tokio::test]
async fn a_collection_without_a_plot_takes_the_fallback_language_plot() {
    let h = Harness::new().await;
    h.server.script(
        "/3/collection/2344",
        vec![
            ok("collection_2344_empty_plot.json"),
            ok("collection_2344.json"),
        ],
    );
    let options = RequestOptions {
        language: "de".into(),
        fallback_language: "en".into(),
        ..opts()
    };
    let collection = h
        .provider
        .collection(&ids(&[("tmdbSet", "2344")]), &options)
        .await
        .unwrap();
    assert_eq!(collection.item.core.title, "Matrix Kollektion");
    assert_eq!(
        collection.item.core.plot,
        "A hacker discovers reality is a simulation."
    );
}

#[tokio::test]
async fn collection_artwork_lists_poster_and_fanart() {
    let h = Harness::new().await;
    h.server.route(
        "/3/collection/2344/images",
        ok("collection_2344_images.json"),
    );
    let list = h
        .provider
        .collection_artwork(&ids(&[("tmdbSet", "2344")]), &opts())
        .await
        .unwrap();
    assert!(list.iter().any(|c| c.kind == MediaFileType::Poster));
    assert!(list.iter().any(|c| c.kind == MediaFileType::Fanart));
}

// --- artwork, trailers, ratings -------------------------------------------------------------

#[tokio::test]
async fn artwork_is_offered_in_every_size_best_language_first() {
    let h = Harness::new().await;
    h.server
        .route("/3/movie/949/images", ok("movie_949_images.json"));
    let options = RequestOptions {
        language: "de".into(),
        ..opts()
    };
    let list = h
        .provider
        .artwork(MediaKind::Movie, &ids(&[("tmdb", "949")]), &options)
        .await
        .unwrap();

    assert_eq!(
        param(&h.calls()[0], "include_image_language").as_deref(),
        Some("de,en,null")
    );

    let originals = |kind: MediaFileType| -> Vec<String> {
        list.iter()
            .filter(|c| c.kind == kind && c.url.contains("/original/"))
            .map(|c| c.url.rsplit('/').next().unwrap().to_owned())
            .collect()
    };
    // German first, then English by votes.
    assert_eq!(
        originals(MediaFileType::Poster),
        vec!["de-poster.jpg", "zMyf.jpg", "en-low.jpg"]
    );
    assert_eq!(
        originals(MediaFileType::Fanart),
        vec!["rrBu.jpg", "bd2.jpg"]
    );

    // All poster sizes of the first poster follow it, widest first.
    let german: Vec<(&str, u32, u32)> = list
        .iter()
        .filter(|c| c.url.ends_with("/de-poster.jpg"))
        .map(|c| (c.url.split('/').nth(5).unwrap(), c.width, c.height))
        .collect();
    assert_eq!(
        german,
        vec![
            ("original", 1000, 1500),
            ("w780", 780, 1170),
            ("w500", 500, 750),
            ("w342", 342, 513),
            ("w185", 185, 278),
        ]
    );
    assert_eq!(list[0].language.as_deref(), Some("de"));

    // Each size lands in its own bucket, from the original (large) down to small.
    let buckets: Vec<u32> = list
        .iter()
        .filter(|c| c.url.ends_with("/de-poster.jpg"))
        .map(|c| c.size_order())
        .collect();
    assert_eq!(buckets, vec![8, 4, 4, 2, 1]);

    // The vector logo is rewritten to PNG.
    let logo = list
        .iter()
        .find(|c| c.kind == MediaFileType::Clearlogo && c.url.contains("logo.png"))
        .expect("logo.svg served as PNG");
    assert!(logo.url.ends_with("/logo.png"));
    assert_eq!(logo.width, 2000);
}

#[tokio::test]
async fn season_poster_and_episode_still_candidates() {
    let h = Harness::new().await;
    h.server.route(
        "/3/tv/1396/season/1/images",
        ok("tv_1396_season_1_images.json"),
    );
    h.server.route(
        "/3/tv/1396/season/1/episode/1/images",
        ok("tv_1396_episode_images.json"),
    );
    let show = ids(&[("tmdb", "1396")]);
    let posters = h.provider.season_artwork(&show, 1, &opts()).await.unwrap();
    assert!(
        posters
            .iter()
            .all(|c| c.kind == MediaFileType::SeasonPoster)
    );
    assert!(!posters.is_empty());
    // The artwork registry hands out trait objects: the same call is on the trait.
    let as_trait: &dyn cv_providers::ArtworkProvider = &h.provider;
    let via_trait = as_trait.season_artwork(&show, 1, &opts()).await.unwrap();
    assert_eq!(via_trait, posters);
    let stills = h
        .provider
        .episode_artwork(&show, 1, 1, &opts())
        .await
        .unwrap();
    assert!(stills.iter().all(|c| c.kind == MediaFileType::Thumb));
    assert_eq!(stills.len(), 2, "the original and w300");
}

#[tokio::test]
async fn ratings_are_the_tmdb_vote_average() {
    let h = Harness::new().await;
    h.server.route("/3/movie/949", ok("movie_949.json"));
    let ratings = h
        .provider
        .ratings(MediaKind::Movie, &ids(&[("tmdb", "949")]), &opts())
        .await
        .unwrap();
    let rating = ratings.get("tmdb").unwrap();
    assert_eq!(
        (rating.value, rating.votes, rating.max_value),
        (7.9, 7100, 10)
    );
}

// --- credentials, errors, caching -----------------------------------------------------------

#[tokio::test]
async fn a_rejected_key_is_an_auth_error_without_the_key_in_the_message() {
    let h = Harness::new().await;
    h.server.route("/3/movie/949", error(401, "error_401.json"));
    let err = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &opts())
        .await
        .unwrap_err();
    let ProviderError::Auth { provider, message } = err.root() else {
        panic!("expected an auth error, got {err:?}");
    };
    assert_eq!(provider, "TMDB");
    assert!(message.contains("Invalid API key"), "{message}");
    assert!(!format!("{err} {err:?}").contains(KEY));
}

#[tokio::test]
async fn a_missing_title_is_not_found_and_a_server_error_hides_the_key() {
    let h = Harness::new().await;
    h.server.route("/3/movie/1", error(404, "error_404.json"));
    h.server.route("/3/movie/2", Reply::status(500));
    let not_found = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "1")]), &opts())
        .await
        .unwrap_err();
    assert!(
        matches!(not_found, ProviderError::NotFound(_)),
        "{not_found:?}"
    );

    let failed = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "2")]), &opts())
        .await
        .unwrap_err();
    assert!(
        matches!(failed, ProviderError::Request { .. }),
        "{failed:?}"
    );
    assert!(failed.to_string().contains("500"));
    assert!(!format!("{failed} {failed:?}").contains(KEY));
}

#[tokio::test]
async fn without_a_key_the_provider_is_inactive_and_sends_nothing() {
    let h = Harness::without_key().await;
    let err = h
        .provider
        .search(&movie_query("Heat", None), &opts())
        .await
        .unwrap_err();
    let ProviderError::MissingCredentials {
        provider,
        registration_url,
        ..
    } = &err
    else {
        panic!("expected missing credentials, got {err:?}");
    };
    assert_eq!(provider, "TMDB");
    assert!(registration_url.contains("themoviedb.org"));
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn a_key_entered_later_applies_to_the_next_request() {
    let h = Harness::without_key().await;
    h.server
        .route("/3/search/movie", ok("search_movie_single.json"));
    assert!(
        h.provider
            .search(&movie_query("Heat", None), &opts())
            .await
            .is_err()
    );

    h.resolver
        .set_user(
            catalogue::key::TMDB,
            catalogue::API_KEY,
            "second-key-9876543",
        )
        .unwrap();
    h.provider
        .search(&movie_query("Heat", None), &opts())
        .await
        .unwrap();
    let request = h.calls().pop().unwrap();
    assert_eq!(
        param(&request, "api_key").as_deref(),
        Some("second-key-9876543")
    );
}

#[tokio::test]
async fn the_test_button_checks_the_key_it_is_given_and_never_uses_the_cache() {
    let h = Harness::with_cache().await;
    let registry = {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(h.provider.clone()));
        registry
    };
    let good = h.resolver.resolve(catalogue::key::TMDB).unwrap();

    h.server.route("/3/configuration", ok("configuration.json"));
    registry
        .check_credentials("tmdb", "TMDB", &good)
        .await
        .unwrap();
    registry
        .check_credentials("tmdb", "TMDB", &good)
        .await
        .unwrap();
    assert_eq!(h.server.count("/3/configuration"), 2);

    h.server
        .route("/3/configuration", error(401, "error_401.json"));
    let err = registry
        .check_credentials("tmdb", "TMDB", &good)
        .await
        .unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
}

#[tokio::test]
async fn a_repeat_scrape_within_the_cache_window_makes_no_network_request() {
    let h = Harness::with_cache().await;
    h.server.route("/3/movie/949", ok("movie_949.json"));
    let id = ids(&[("tmdb", "949")]);

    h.provider
        .details(MediaKind::Movie, &id, &opts())
        .await
        .unwrap();
    h.provider
        .details(MediaKind::Movie, &id, &opts())
        .await
        .unwrap();
    assert_eq!(h.server.count("/3/movie/949"), 1);

    // A forced rescrape goes to the network again.
    let forced = RequestOptions {
        bypass_cache: true,
        ..opts()
    };
    h.provider
        .details(MediaKind::Movie, &id, &forced)
        .await
        .unwrap();
    assert_eq!(h.server.count("/3/movie/949"), 2);
}

#[tokio::test]
async fn the_image_base_is_read_once_and_the_default_is_used_when_it_is_unavailable() {
    let h = Harness::new().await;
    h.server.route("/3/movie/949", ok("movie_949.json"));
    let id = ids(&[("tmdb", "949")]);
    h.provider
        .details(MediaKind::Movie, &id, &opts())
        .await
        .unwrap();
    h.provider
        .details(MediaKind::Movie, &id, &opts())
        .await
        .unwrap();
    assert_eq!(h.server.count("/3/configuration"), 1);

    // A server that does not answer /configuration still yields image URLs.
    let bare = MockServer::start().await;
    bare.route("/3/movie/949", ok("movie_949.json"));
    let provider = TmdbProvider::with_base_url(
        HttpClient::builder().build().unwrap(),
        h.resolver.clone(),
        &bare.url("/3"),
    );
    let item = provider
        .details(MediaKind::Movie, &id, &opts())
        .await
        .unwrap();
    assert!(
        item.core.artwork_urls[&MediaFileType::Poster]
            .starts_with("https://image.tmdb.org/t/p/original/")
    );
}

#[tokio::test]
async fn a_rate_limited_answer_surfaces_after_the_retries() {
    let h = Harness::new().await;
    h.server
        .route("/3/search/movie", Reply::rate_limited(Some("0")));
    let err = h
        .provider
        .search(&movie_query("Heat", None), &opts())
        .await
        .unwrap_err();
    assert!(
        matches!(err.root(), ProviderError::RateLimited { .. }),
        "{err:?}"
    );
    // The first send plus three retries.
    assert_eq!(h.server.count("/3/search/movie"), 4);
}

#[tokio::test]
async fn identity_and_capabilities() {
    let h = Harness::new().await;
    assert_eq!(h.provider.id(), "tmdb");
    assert_eq!(h.provider.name(), "TMDB");
    let schema = h.provider.credentials().expect("TMDB needs a key");
    assert_eq!(schema.provider, "tmdb");
    let capabilities = h.provider.capabilities();
    assert!(capabilities.movies && capabilities.shows && !capabilities.best_effort);
    assert!(capabilities.lookup_ids.contains(&"imdb"));
    assert!(capabilities.lookup_ids.contains(&"tvdb"));
    assert!(capabilities.orderings.contains(&EpisodeOrdering::Dvd));
}

// --- cross-provider ID completion -------------------------------------------------

#[tokio::test]
async fn tmdb_fills_the_imdb_and_tvdb_ids_of_a_scraped_show() {
    let h = Harness::new().await;
    h.server.route("/3/tv/1396", ok("tv_1396.json"));

    let done = complete_ids(
        MediaKind::Show,
        &ids(&[("tmdb", "1396")]),
        &[&h.provider as &dyn MetadataProvider],
        None,
        &opts(),
    )
    .await;

    assert_eq!(done.ids.get(id_key::TMDB), Some("1396"));
    assert_eq!(done.ids.get(id_key::IMDB), Some("tt0903747"));
    assert_eq!(done.ids.get(id_key::TVDB), Some("81189"));
    let added: Vec<_> = done.added.iter().map(|a| a.key.as_str()).collect();
    assert_eq!(added, vec!["imdb", "tvdb"]);
    assert!(
        done.added
            .iter()
            .all(|a| a.source == IdSource::Provider("tmdb".to_owned()))
    );
    assert!(done.failures.is_empty());
}

#[tokio::test]
async fn a_movie_that_has_an_imdb_id_gets_its_tmdb_id_and_collection() {
    let h = Harness::new().await;
    h.server
        .route("/3/find/tt0113277", ok("find_imdb_heat.json"));
    h.server.route("/3/movie/949", ok("movie_949.json"));

    let done = complete_ids(
        MediaKind::Movie,
        &ids(&[("imdb", "tt0113277")]),
        &[&h.provider as &dyn MetadataProvider],
        None,
        &opts(),
    )
    .await;

    assert_eq!(done.ids.get(id_key::TMDB), Some("949"));
    assert_eq!(done.ids.get(id_key::IMDB), Some("tt0113277"));
}

#[tokio::test]
async fn completion_keeps_existing_ids_and_skips_a_complete_title() {
    let h = Harness::new().await;
    h.server.route("/3/tv/1396", ok("tv_1396.json"));

    // A different IMDb ID is the user's; completion never replaces it.
    let done = complete_ids(
        MediaKind::Show,
        &ids(&[("tmdb", "1396"), ("imdb", "tt0000001")]),
        &[&h.provider as &dyn MetadataProvider],
        None,
        &opts(),
    )
    .await;
    assert_eq!(done.ids.get(id_key::IMDB), Some("tt0000001"));
    assert_eq!(done.ids.get(id_key::TVDB), Some("81189"));

    // Nothing missing means no request at all.
    let before = h.calls().len();
    let full = ids(&[
        ("tmdb", "1396"),
        ("imdb", "tt0903747"),
        ("tvdb", "81189"),
        ("trakt", "1"),
    ]);
    let done = complete_ids(
        MediaKind::Show,
        &full,
        &[&h.provider as &dyn MetadataProvider],
        None,
        &opts(),
    )
    .await;
    assert!(done.added.is_empty());
    assert_eq!(h.calls().len(), before);
}

#[tokio::test]
async fn a_failing_provider_is_reported_and_does_not_stop_completion() {
    let h = Harness::build(false, false).await;
    h.server.route("/3/tv/1396", ok("tv_1396.json"));

    let done = complete_ids(
        MediaKind::Show,
        &ids(&[("tmdb", "1396")]),
        &[&h.provider as &dyn MetadataProvider],
        None,
        &opts(),
    )
    .await;
    assert!(done.added.is_empty());
    assert_eq!(done.failures.len(), 1);
    assert_eq!(done.failures[0].0, "tmdb");
}

#[tokio::test]
async fn the_anime_cross_reference_fills_ids_before_any_request() {
    let h = Harness::new().await;
    let anime = AnimeCrossReference::from_json(
        r#"[{"anidb": 23, "tvdb": 76885, "imdb": "tt0213338", "tmdb": 30991, "trakt": 5}]"#,
    )
    .unwrap();

    let done = complete_ids(
        MediaKind::Show,
        &ids(&[("anidb", "23")]),
        &[&h.provider as &dyn MetadataProvider],
        Some(&anime),
        &opts(),
    )
    .await;
    assert_eq!(done.ids.get(id_key::TVDB), Some("76885"));
    assert_eq!(done.ids.get(id_key::IMDB), Some("tt0213338"));
    assert_eq!(done.ids.get(id_key::TMDB), Some("30991"));
    assert_eq!(done.ids.get(id_key::TRAKT), Some("5"));
    assert!(
        done.added
            .iter()
            .all(|a| a.source == IdSource::AnimeCrossReference)
    );
    assert!(h.calls().is_empty());
}

#[tokio::test]
async fn a_person_has_their_imdb_id_from_the_external_ids() {
    let h = Harness::new().await;
    h.server.route(
        "/3/person/10205/external_ids",
        Reply::ok(r#"{"id":10205,"imdb_id":"nm0000244","tvdb_id":null,"instagram_id":null}"#),
    );
    let found = h
        .provider
        .person_ids(&ids(&[("tmdb", "10205")]), &opts())
        .await
        .unwrap();
    assert_eq!(found.get("imdb"), Some("nm0000244"));

    // A person IMDb does not list, and a person without a TMDB id, give nothing.
    h.server.route(
        "/3/person/7/external_ids",
        Reply::ok(r#"{"id":7,"imdb_id":null}"#),
    );
    let none = h
        .provider
        .person_ids(&ids(&[("tmdb", "7")]), &opts())
        .await
        .unwrap();
    assert_eq!(none.get("imdb"), None);
    let nothing = h.provider.person_ids(&ids(&[]), &opts()).await.unwrap();
    assert_eq!(nothing.get("imdb"), None);
}

#[tokio::test]
async fn movie_details_carry_the_trailers_with_the_preferred_one_for_the_nfo() {
    let h = Harness::new().await;
    h.server.route("/3/movie/949", ok("movie_949.json"));
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &opts())
        .await
        .unwrap();
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("not a movie");
    };
    let urls: Vec<&str> = movie.trailers.iter().map(|t| t.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://www.youtube.com/watch?v=enHeat1",
            "https://www.youtube.com/watch?v=deHeat1"
        ],
        "trailers only, the one in the request language first"
    );
    assert!(movie.trailers[0].in_nfo && !movie.trailers[1].in_nfo);
    assert_eq!(
        param(&h.calls()[0], "include_video_language").as_deref(),
        Some("en,null")
    );

    let german = RequestOptions {
        language: "de".into(),
        ..opts()
    };
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &german)
        .await
        .unwrap();
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("not a movie");
    };
    assert_eq!(
        movie.trailers[0].url,
        "https://www.youtube.com/watch?v=deHeat1"
    );
    assert!(movie.trailers[0].in_nfo);
}

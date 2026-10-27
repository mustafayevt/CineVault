//! The Kodi XML scraper interpreter and add-on discovery, against the sample add-ons in
//! `tests/fixtures/kodi/addons` and a local mock server that replays the sample pages in
//! `tests/fixtures/kodi/pages`. The sample scrapers were written for these tests. Nothing here
//! touches the network.

mod support;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cv_domain::{
    CreditKind, EpisodeGroup, Genre, ItemDetail, MediaFileType, ProviderIds, Rating, id_key,
};
use cv_providers::http::ProviderLimits;
use cv_providers::kodi::{
    KodiCatalogue, KodiConfig, KodiContent, KodiProvider, KodiSettings, Reference, ScraperDef,
    Support, discover,
};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::{HttpClient, MetadataProvider, Provider, ProviderError};
use support::{MockServer, Reply};

const FILMS: &str = "metadata.sample.films";
const SERIES: &str = "metadata.sample.series";

fn kodi_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kodi")
}

fn addons_dir() -> PathBuf {
    kodi_fixtures().join("addons")
}

fn page_text(name: &str) -> String {
    let path = kodi_fixtures().join("pages").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn page(name: &str) -> Reply {
    let mut reply = Reply::ok(&page_text(name));
    reply.headers = vec![("Content-Type".into(), "text/html; charset=utf-8".into())];
    reply
}

struct Harness {
    server: MockServer,
    http: HttpClient,
    _cache: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let server = MockServer::start().await;
        let cache = tempfile::tempdir().unwrap();
        let http = HttpClient::builder()
            .cache_dir(cache.path())
            .build()
            .unwrap();
        Self {
            server,
            http,
            _cache: cache,
        }
    }

    /// The user's setting values: every sample add-on reads its site from the mock server.
    fn settings(&self) -> HashMap<String, KodiSettings> {
        let mut settings = KodiSettings::new();
        settings.set("baseurl", &self.server.url(""));
        [FILMS, SERIES]
            .into_iter()
            .map(|id| (id.to_owned(), settings.clone()))
            .collect()
    }

    fn provider(&self, key: &str) -> KodiProvider {
        let catalogue = KodiCatalogue::load(&self.http, &[addons_dir()], &self.settings());
        let provider = catalogue
            .providers
            .into_iter()
            .find(|p| p.id() == key)
            .unwrap_or_else(|| panic!("no provider {key}"));
        self.fast(provider.id());
        provider
    }

    /// The sample scrapers are polite by default (one request a second); tests are not.
    fn fast(&self, key: &str) {
        self.http
            .set_provider_limits(key, ProviderLimits::new(4, 1000, Duration::from_secs(1)));
    }

    fn inline(&self, key: &str, scraper: &str, content: KodiContent) -> KodiProvider {
        let mut settings = KodiSettings::new();
        settings.set("baseurl", &self.server.url(""));
        let provider = KodiProvider::new(
            self.http.clone(),
            key,
            "Inline scraper",
            ScraperDef::parse(scraper).unwrap(),
            settings,
            content,
        );
        self.fast(key);
        provider
    }

    fn with_film(self) -> Self {
        self.server
            .route_query("/search", "q=Night+Train", page("search_night_train.html"));
        self.server.route("/film/7", page("film_7.html"));
        self.server.route("/imdb/tt0100007", page("film_7.html"));
        self
    }

    fn with_series(self) -> Self {
        self.server
            .route("/series/search", page("series_search.html"));
        self.server.route("/series/3", page("series_3.html"));
        self.server
            .route("/series/3/episodes", page("series_3_episodes.html"));
        self.server.route("/episode/301", page("episode_301.html"));
        self
    }
}

fn query(kind: MediaKind, title: &str, year: Option<u16>) -> SearchQuery {
    SearchQuery {
        kind,
        title: title.to_owned(),
        year,
    }
}

fn opts() -> RequestOptions {
    RequestOptions::default()
}

fn own_ids(key: &str, value: &str) -> ProviderIds {
    let mut ids = ProviderIds::new();
    ids.set(key, value);
    ids
}

// --- discovery --------------------------------------------------------------------------------

#[test]
fn discovery_lists_xml_add_ons_and_marks_python_ones_unsupported() {
    let found = discover(&[addons_dir()]);
    let ids: Vec<&str> = found.iter().map(|a| a.id.as_str()).collect();
    // metadata.local is skipped.
    assert_eq!(
        ids,
        [
            "metadata.sample.combo",
            FILMS,
            "metadata.sample.python",
            SERIES
        ]
    );

    let films = found.iter().find(|a| a.id == FILMS).unwrap();
    assert_eq!(films.name, "Sample Film Index");
    assert_eq!(films.scrapers.len(), 1);
    assert_eq!(films.scrapers[0].content, KodiContent::Movies);
    assert_eq!(films.scrapers[0].support, Support::Xml);

    let python = found
        .iter()
        .find(|a| a.id == "metadata.sample.python")
        .unwrap();
    assert_eq!(python.supported().count(), 0);
    assert!(python.unsupported_reasons()[0].contains("Python"));
}

#[tokio::test]
async fn the_catalogue_offers_xml_scrapers_as_providers_and_lists_the_rest() {
    let h = Harness::new().await;
    let catalogue = KodiCatalogue::load(&h.http, &[addons_dir()], &h.settings());

    let keys: Vec<&str> = catalogue.providers.iter().map(|p| p.id()).collect();
    // An add-on with both a movie and a TV scraper gets one key per kind.
    assert_eq!(
        keys,
        [
            "metadata.sample.combo.movies",
            "metadata.sample.combo.tvshows",
            FILMS,
            SERIES
        ]
    );

    assert_eq!(catalogue.unsupported.len(), 1);
    assert_eq!(catalogue.unsupported[0].addon_id, "metadata.sample.python");
    assert!(catalogue.unsupported[0].reason.contains("Python"));

    let films = catalogue
        .providers
        .iter()
        .find(|p| p.id() == FILMS)
        .unwrap();
    assert_eq!(films.name(), "Sample Film Index");
    assert!(films.credentials().is_none());
    let capabilities = films.capabilities();
    assert!(capabilities.movies && !capabilities.shows && !capabilities.best_effort);
    assert_eq!(capabilities.lookup_ids, vec![FILMS]);
    let series = catalogue
        .providers
        .iter()
        .find(|p| p.id() == SERIES)
        .unwrap();
    assert!(series.capabilities().shows);
    assert_eq!(series.capabilities().orderings.len(), 1);
}

// --- movies -----------------------------------------------------------------------------------

#[tokio::test]
async fn search_builds_the_url_with_the_year_and_reads_the_results() {
    let h = Harness::new().await.with_film();
    let provider = h.provider(FILMS);

    let hits = provider
        .search(&query(MediaKind::Movie, "Night Train", Some(1995)), &opts())
        .await
        .unwrap();

    let requests = h.server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].target, "/search?q=Night+Train&y=1995");

    // The sponsored list item is not a hit.
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].title, "Night Train to Nowhere");
    assert_eq!(hits[0].year, 1995);
    assert_eq!(hits[0].provider, FILMS);
    assert_eq!(hits[1].title, "Night Train & Day Bus");
    assert_eq!(
        hits[0].ids.get(FILMS),
        Some(format!("7|{}", h.server.url("/film/7")).as_str())
    );
}

#[tokio::test]
async fn search_without_a_year_leaves_the_year_out_of_the_url() {
    let h = Harness::new().await.with_film();
    let provider = h.provider(FILMS);
    provider
        .search(&query(MediaKind::Movie, "Night Train", None), &opts())
        .await
        .unwrap();
    assert_eq!(h.server.requests()[0].target, "/search?q=Night+Train");
}

#[tokio::test]
async fn a_search_for_the_wrong_kind_finds_nothing_and_asks_nothing() {
    let h = Harness::new().await.with_film();
    let provider = h.provider(FILMS);
    let hits = provider
        .search(&query(MediaKind::Show, "Night Train", None), &opts())
        .await
        .unwrap();
    assert!(hits.is_empty());
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn details_fill_the_movie_from_the_page() {
    let h = Harness::new().await.with_film();
    let provider = h.provider(FILMS);
    let hits = provider
        .search(&query(MediaKind::Movie, "Night Train", Some(1995)), &opts())
        .await
        .unwrap();

    let item = provider
        .details(MediaKind::Movie, &hits[0].ids, &opts())
        .await
        .unwrap();

    assert_eq!(item.core.title, "Night Train to Nowhere");
    assert_eq!(item.core.original_title, "Nachtzug nach Nirgendwo");
    assert_eq!(item.core.year, 1995);
    // Tags are removed, <br /> becomes a line break and entities are decoded.
    assert_eq!(
        item.core.plot,
        "A signalman finds a train that is not on any timetable.\nTom & Jerry would not board it."
    );
    assert_eq!(item.core.last_scraper_id, FILMS);
    assert_eq!(item.core.ids.get(id_key::IMDB), Some("tt0100007"));
    assert_eq!(item.core.ids.get(FILMS), hits[0].ids.get(FILMS));
    assert_eq!(
        item.core.ratings.get("sample"),
        Some(&Rating::new("sample", 7.6, 12345, 10))
    );
    assert_eq!(
        item.core
            .artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("https://img.example.test/film/7.jpg")
    );
    assert_eq!(
        item.core
            .artwork_urls
            .get(&MediaFileType::Fanart)
            .map(String::as_str),
        Some("https://img.example.test/film/7-bg.jpg")
    );

    let directors: Vec<_> = item
        .core
        .credits
        .iter()
        .filter(|c| c.kind == CreditKind::Director)
        .map(|c| c.person.name.as_str())
        .collect();
    assert_eq!(directors, ["Mara Quint"]);
    let cast: Vec<_> = item
        .core
        .credits
        .iter()
        .filter(|c| c.kind == CreditKind::Actor)
        .map(|c| {
            (
                c.person.name.as_str(),
                c.role.as_str(),
                c.person.thumb_url.as_str(),
            )
        })
        .collect();
    assert_eq!(
        cast,
        [
            ("Ines Calder", "Vera", "https://img.example.test/p/ines.jpg"),
            (
                "Bo Hartmann",
                "The Signalman",
                "https://img.example.test/p/bo.jpg"
            ),
        ]
    );

    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("a movie");
    };
    assert_eq!(movie.runtime_minutes, 112);
    assert_eq!(movie.certification.0, "US_PG13");
    assert_eq!(
        movie.genres,
        vec![
            Genre::parse("Thriller").unwrap(),
            Genre::parse("Drama").unwrap()
        ]
    );
    assert_eq!(movie.trailers.len(), 1);
    assert_eq!(
        movie.trailers[0].url,
        "https://video.example.test/trailers/7.mp4"
    );
}

#[tokio::test]
async fn a_chained_function_adds_its_fields_to_the_answer() {
    let h = Harness::new().await.with_film();
    let provider = h.provider(FILMS);
    let item = provider
        .details(
            MediaKind::Movie,
            &own_ids(FILMS, &format!("7|{}", h.server.url("/film/7"))),
            &opts(),
        )
        .await
        .unwrap();
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("a movie");
    };
    assert_eq!(movie.tagline, "Film number 7");
}

#[tokio::test]
async fn a_page_the_scraper_cannot_read_returns_nothing() {
    let h = Harness::new().await;
    h.server.route("/film/9", {
        let mut reply = Reply::ok(&page_text("film_changed.html"));
        reply.headers = vec![("Content-Type".into(), "text/html".into())];
        reply
    });
    let provider = h.provider(FILMS);
    let error = provider
        .details(
            MediaKind::Movie,
            &own_ids(FILMS, &format!("9|{}", h.server.url("/film/9"))),
            &opts(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::FormatChanged(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_missing_page_is_not_found_and_a_server_error_is_a_request_error() {
    let h = Harness::new().await;
    h.server.route("/film/500", Reply::status(500));
    let provider = h.provider(FILMS);
    let missing = provider
        .details(
            MediaKind::Movie,
            &own_ids(FILMS, &format!("1|{}", h.server.url("/film/404"))),
            &opts(),
        )
        .await
        .unwrap_err();
    assert!(matches!(missing, ProviderError::NotFound(_)), "{missing:?}");
    let failed = provider
        .details(
            MediaKind::Movie,
            &own_ids(FILMS, &format!("1|{}", h.server.url("/film/500"))),
            &opts(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(failed, ProviderError::Request { .. }),
        "{failed:?}"
    );
}

#[tokio::test]
async fn pages_are_cached_like_other_scraped_pages() {
    let h = Harness::new().await.with_film();
    let provider = h.provider(FILMS);
    let ids = own_ids(FILMS, &format!("7|{}", h.server.url("/film/7")));
    provider
        .details(MediaKind::Movie, &ids, &opts())
        .await
        .unwrap();
    provider
        .details(MediaKind::Movie, &ids, &opts())
        .await
        .unwrap();
    assert_eq!(h.server.count("/film/7"), 1);
}

#[tokio::test]
async fn an_imdb_id_is_resolved_through_the_scrapers_nfo_function() {
    let h = Harness::new().await.with_film();
    let provider = h.provider(FILMS);
    let mut ids = ProviderIds::new();
    ids.set(id_key::IMDB, "tt0100007");

    let hit = provider
        .lookup(MediaKind::Movie, &ids, &opts())
        .await
        .unwrap()
        .expect("the film is found");

    assert_eq!(hit.title, "Night Train to Nowhere");
    assert!(hit.exact_id);
    assert_eq!(h.server.count("/imdb/tt0100007"), 1);

    // An ID the scraper does not recognise finds nothing.
    let mut other = ProviderIds::new();
    other.set(id_key::TMDB, "603");
    assert!(
        provider
            .lookup(MediaKind::Movie, &other, &opts())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn nfo_text_names_the_title() {
    let h = Harness::new().await;
    let provider = h.provider(FILMS);

    let reference = provider
        .nfo_url("Saved from https://film-index.example/film/7 on Friday")
        .await
        .unwrap()
        .expect("recognised");
    assert_eq!(reference.id, "7");
    assert_eq!(reference.urls[0].url, h.server.url("/film/7"));
    assert_eq!(Reference::decode(&reference.encode()), reference);

    assert!(provider.nfo_url("just some text").await.unwrap().is_none());
    // A scraper with no NfoUrl function has nothing to say about an NFO.
    assert!(
        h.provider(SERIES)
            .nfo_url("https://film-index.example/film/7")
            .await
            .unwrap()
            .is_none()
    );
}

// --- TV shows ---------------------------------------------------------------------------------

#[tokio::test]
async fn a_show_has_details_and_an_episode_list() {
    let h = Harness::new().await.with_series();
    let provider = h.provider(SERIES);

    let hits = provider
        .search(&query(MediaKind::Show, "Harbour Lights", None), &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].year, 2001);

    let show = provider
        .details(MediaKind::Show, &hits[0].ids, &opts())
        .await
        .unwrap();
    assert_eq!(show.core.title, "Harbour Lights");
    assert_eq!(show.core.production_companies, ["Coastal TV"]);
    assert_eq!(show.core.year, 2001);
    let ItemDetail::Show(fields) = &show.detail else {
        panic!("a show");
    };
    assert_eq!(
        fields.first_aired.map(|d| d.to_string()),
        Some("2001-03-04".to_owned())
    );
    assert_eq!(fields.status, cv_domain::AiredStatus::Ended);
    assert_eq!(fields.genres.len(), 2);

    let episodes = provider
        .episodes(&show.core.ids, &EpisodeGroup::aired(), &opts())
        .await
        .unwrap();
    let rows: Vec<(u32, u32, String)> = episodes
        .iter()
        .map(|e| {
            let ItemDetail::Episode(f) = &e.detail else {
                panic!("an episode");
            };
            let n = f.numbers.get(&EpisodeGroup::aired()).unwrap();
            (n.season, n.episode, e.core.title.clone())
        })
        .collect();
    assert_eq!(
        rows,
        [
            (0, 1, "Behind the Lamp".to_owned()),
            (1, 1, "Pilot".to_owned()),
            (1, 2, "Low Tide".to_owned()),
            (2, 1, "Return & Departure".to_owned()),
        ]
    );
    let ItemDetail::Episode(pilot) = &episodes[1].detail else {
        panic!("an episode");
    };
    assert_eq!(
        pilot.first_aired.map(|d| d.to_string()),
        Some("2001-03-04".to_owned())
    );
    assert_eq!(
        episodes[1].core.ids.get(SERIES),
        Some(format!("301|{}", h.server.url("/episode/301")).as_str())
    );
    // Episode details are not fetched unless asked for.
    assert_eq!(h.server.count("/episode/301"), 0);
}

#[tokio::test]
async fn episode_details_can_be_fetched_for_every_episode() {
    let h = Harness::new().await.with_series();
    let provider = h.provider(SERIES).with_config(KodiConfig {
        episode_details: true,
    });
    let episodes = provider
        .episodes(
            &own_ids(SERIES, &format!("3|{}", h.server.url("/series/3"))),
            &EpisodeGroup::aired(),
            &opts(),
        )
        .await
        .unwrap();
    assert_eq!(episodes.len(), 4);

    let pilot = &episodes[1];
    assert_eq!(pilot.core.title, "Pilot");
    assert_eq!(
        pilot.core.plot,
        "The keeper lights the lamp for the first time."
    );
    let ItemDetail::Episode(fields) = &pilot.detail else {
        panic!("an episode");
    };
    assert_eq!(fields.runtime_minutes, 42);
    // The number comes from the list; the details page does not repeat it.
    assert_eq!(
        fields.numbers.get(&EpisodeGroup::aired()).unwrap().episode,
        1
    );
    assert_eq!(
        pilot.core.ratings.get(provider.id()).map(|r| r.value),
        Some(8.1)
    );
    // The other episodes' pages are missing: they keep their list entries.
    assert_eq!(episodes[2].core.title, "Low Tide");
    assert_eq!(h.server.count("/episode/301"), 1);
    assert_eq!(h.server.count("/episode/302"), 1);
}

#[tokio::test]
async fn only_the_aired_ordering_is_offered_and_movies_have_no_episodes() {
    let h = Harness::new().await.with_series();
    let provider = h.provider(SERIES);
    let ids = own_ids(SERIES, &format!("3|{}", h.server.url("/series/3")));
    let dvd = provider.episodes(&ids, &EpisodeGroup::dvd(), &opts()).await;
    assert!(matches!(dvd, Err(ProviderError::NotFound(_))));
    let movies = h.provider(FILMS);
    let none = movies.episodes(&ids, &EpisodeGroup::aired(), &opts()).await;
    assert!(matches!(none, Err(ProviderError::Unsupported { .. })));
}

// --- safety and edge cases --------------------------------------------------------------------

#[tokio::test]
async fn a_scraper_cannot_make_the_app_read_a_local_file() {
    let h = Harness::new().await;
    let provider = h.inline(
        "inline.file",
        r#"<scraper content="movies"><CreateSearchUrl dest="3">
             <RegExp input="$$1" output="&lt;url&gt;file:///C:/Windows/win.ini&lt;/url&gt;" dest="3"><expression noclean="1"/></RegExp>
           </CreateSearchUrl><GetSearchResults dest="8"/></scraper>"#,
        KodiContent::Movies,
    );
    let error = provider
        .search(&query(MediaKind::Movie, "x", None), &opts())
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Other(_)), "{error:?}");
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn post_urls_send_their_query_as_a_form_body_with_the_referer() {
    let h = Harness::new().await;
    h.server.route("/find", page("series_search.html"));
    let provider = h.inline(
        "inline.post",
        r#"<scraper content="tvshows">
             <CreateSearchUrl dest="3">
               <RegExp input="$$1" output="&lt;url post=&quot;yes&quot; spoof=&quot;https://films.example.test/&quot;&gt;$INFO[baseurl]/find?title=\1&amp;kind=tv&lt;/url&gt;" dest="3"><expression noclean="1"/></RegExp>
             </CreateSearchUrl>
             <GetSearchResults dest="8">
               <RegExp input="$$1" output="&lt;results&gt;&lt;entity&gt;&lt;title&gt;Found&lt;/title&gt;&lt;id&gt;1&lt;/id&gt;&lt;/entity&gt;&lt;/results&gt;" dest="8"><expression>(.*)</expression></RegExp>
             </GetSearchResults>
           </scraper>"#,
        KodiContent::TvShows,
    );
    let hits = provider
        .search(&query(MediaKind::Show, "Harbour", None), &opts())
        .await
        .unwrap();
    assert_eq!(hits[0].title, "Found");

    let requests = h.server.requests();
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path(), "/find");
    assert_eq!(
        String::from_utf8_lossy(&requests[0].body),
        "title=Harbour&kind=tv"
    );
    assert_eq!(
        requests[0].headers.get("referer").map(String::as_str),
        Some("https://films.example.test/")
    );
}

#[tokio::test]
async fn a_scraper_without_a_function_says_the_operation_is_unsupported() {
    let h = Harness::new().await;
    let provider = h.inline(
        "inline.bare",
        "<scraper content=\"movies\"/>",
        KodiContent::Movies,
    );
    let error = provider
        .search(&query(MediaKind::Movie, "x", None), &opts())
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::Unsupported { .. }),
        "{error:?}"
    );
    let error = provider
        .details(
            MediaKind::Movie,
            &own_ids("inline.bare", "1|http://x.test/1"),
            &opts(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::Unsupported { .. }),
        "{error:?}"
    );
}

#[test]
fn a_python_add_on_cannot_be_loaded_as_a_scraper() {
    let found = discover(&[addons_dir()]);
    let python = found
        .iter()
        .find(|a| a.id == "metadata.sample.python")
        .unwrap();
    assert!(python.load(&python.scrapers[0]).is_err());
}

#[test]
fn settings_defaults_come_from_the_add_on_in_both_settings_formats() {
    let found = discover(&[addons_dir()]);
    let films = found.iter().find(|a| a.id == FILMS).unwrap();
    let (_, settings) = films.load(&films.scrapers[0]).unwrap();
    assert_eq!(settings.get("baseurl"), Some("https://films.example.test"));
    let series = found.iter().find(|a| a.id == SERIES).unwrap();
    let (_, settings) = series.load(&series.scrapers[0]).unwrap();
    assert_eq!(settings.get("baseurl"), Some("https://series.example.test"));
}

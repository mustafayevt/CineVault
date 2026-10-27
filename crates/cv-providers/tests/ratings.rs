//! Fetching ratings from the scraper, OMDb, MDBList and the IMDb ratings dataset
//! against a local mock server (`tests/fixtures/ratings/`, `tests/fixtures/omdb/`). Nothing here
//! touches the network.

mod support;

use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cv_domain::{Item, ItemKind, ProviderIds, Rating, Ratings};
use cv_providers::model::{MediaKind, RequestOptions};
use cv_providers::ratings::{DatasetOutcome, ImdbRatingsDataset, MdbListProvider};
use cv_providers::{
    CredentialResolver, FetchedRatings, HttpClient, MemoryUserCredentials, OmdbProvider, Provider,
    ProviderError, RatingFetchSettings, RatingFetcher, RatingProvider, RatingSettings,
    RatingSource, RunNotices, SkipReason, SourceStatus, catalogue,
};
use flate2::Compression;
use flate2::write::GzEncoder;
use support::{MockServer, Reply};

const OMDB_KEY: &str = "omdb-test-key-0123456789";
const MDBLIST_KEY: &str = "mdblist-test-key-9876543210";
const HEAT: &str = "tt0113277";

fn fixture(dir: &str, name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir)
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn gzip(text: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(text.as_bytes()).unwrap();
    encoder.finish().unwrap()
}

fn dataset_reply(text: &str, etag: Option<&str>) -> Reply {
    let mut reply = Reply::ok("");
    reply.headers = vec![("Content-Type".into(), "application/gzip".into())];
    if let Some(etag) = etag {
        reply.headers.push(("ETag".into(), etag.into()));
    }
    reply.body = gzip(text);
    reply
}

fn ids(pairs: &[(&str, &str)]) -> ProviderIds {
    pairs.iter().copied().collect()
}

fn opts() -> RequestOptions {
    RequestOptions::default()
}

fn http() -> HttpClient {
    HttpClient::builder().build().unwrap()
}

fn resolver(omdb: bool, mdblist: bool) -> CredentialResolver {
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    if omdb {
        resolver
            .set_user(catalogue::key::OMDB, catalogue::API_KEY, OMDB_KEY)
            .unwrap();
    }
    if mdblist {
        resolver
            .set_user(catalogue::key::MDBLIST, catalogue::API_KEY, MDBLIST_KEY)
            .unwrap();
    }
    resolver
}

/// A scraping provider's rating side with fixed answers.
struct FakeScraper {
    ratings: Vec<Rating>,
}

#[async_trait]
impl Provider for FakeScraper {
    fn id(&self) -> &'static str {
        "fakescraper"
    }
    fn name(&self) -> &'static str {
        "Fake scraper"
    }
}

#[async_trait]
impl RatingProvider for FakeScraper {
    async fn ratings(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        Ok(self.ratings.iter().cloned().collect())
    }
}

// ---- MDBList -------------------------------------------------------------------------------

async fn start() -> MockServer {
    MockServer::start().await
}

#[tokio::test]
async fn mdblist_maps_every_known_source_to_the_app_keys_and_scales() {
    let server = start().await;
    server.route(
        &format!("/imdb/movie/{HEAT}"),
        Reply::ok(&fixture("ratings", "mdblist_heat.json")),
    );
    let provider = MdbListProvider::with_base_url(http(), resolver(false, true), &server.url(""));

    let ratings = provider
        .ratings(MediaKind::Movie, &ids(&[("imdb", HEAT)]), &opts())
        .await
        .unwrap();

    let get = |id: &str| ratings.get(id).unwrap_or_else(|| panic!("no {id}")).clone();
    assert_eq!((get("imdb").value, get("imdb").votes), (8.3, 650_000));
    assert_eq!((get("tmdb").value, get("tmdb").max_value), (7.9, 10));
    assert_eq!((get("trakt").value, get("trakt").max_value), (8.0, 10));
    assert_eq!(
        (get("metacritic").value, get("metacritic").max_value),
        (76.0, 100)
    );
    assert_eq!(get("metacriticuser").max_value, 10);
    assert_eq!(
        (
            get("tomatometerallcritics").value,
            get("tomatometerallcritics").votes
        ),
        (83.0, 87)
    );
    assert_eq!(
        (
            get("tomatometerallaudience").value,
            get("tomatometerallaudience").max_value
        ),
        (94.0, 100)
    );
    let letterboxd = get("letterboxd");
    assert_eq!((letterboxd.value, letterboxd.max_value), (4.1, 5));
    assert_eq!(get("rogerebert").max_value, 4);
    assert!(
        ratings.get("myanimelist").is_none(),
        "a null value is no rating"
    );
    assert!(
        ratings.get("futuresource").is_none(),
        "unknown sources are ignored"
    );

    let request = &server.requests()[0];
    assert!(request.target.contains(&format!("apikey={MDBLIST_KEY}")));
}

#[tokio::test]
async fn mdblist_tries_the_next_id_when_one_is_unknown() {
    let server = start().await;
    server.route(
        "/imdb/show/tt7654321",
        Reply::ok(&fixture("ratings", "mdblist_not_found.json")),
    );
    server.route(
        "/tvdb/show/81189",
        Reply::ok(&fixture("ratings", "mdblist_heat.json")),
    );
    let provider = MdbListProvider::with_base_url(http(), resolver(false, true), &server.url(""));

    let ratings = provider
        .ratings(
            MediaKind::Show,
            &ids(&[("imdb", "tt7654321"), ("tvdb", "81189")]),
            &opts(),
        )
        .await
        .unwrap();
    assert!(ratings.get("letterboxd").is_some());
    let paths: Vec<String> = server
        .requests()
        .iter()
        .map(|r| r.path().to_owned())
        .collect();
    assert_eq!(paths, ["/imdb/show/tt7654321", "/tvdb/show/81189"]);

    // Nothing the provider understands: not found, and nothing is requested.
    let before = server.requests().len();
    let none = provider
        .ratings(MediaKind::Show, &ids(&[("anidb", "1")]), &opts())
        .await;
    assert!(matches!(none, Err(ProviderError::NotFound(_))));
    assert_eq!(server.requests().len(), before);
}

#[tokio::test]
async fn mdblist_without_a_key_makes_no_request() {
    let server = start().await;
    let provider = MdbListProvider::with_base_url(http(), resolver(false, false), &server.url(""));
    let result = provider
        .ratings(MediaKind::Movie, &ids(&[("imdb", HEAT)]), &opts())
        .await;
    assert!(matches!(
        result,
        Err(ProviderError::MissingCredentials { .. })
    ));
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn mdblist_rejects_a_bad_key_without_echoing_it() {
    let server = start().await;
    let mut denied = Reply::ok(&fixture("ratings", "mdblist_invalid_key.json"));
    denied.status = 403;
    server.route(&format!("/imdb/movie/{HEAT}"), denied);
    let provider = MdbListProvider::with_base_url(http(), resolver(false, true), &server.url(""));
    let error = provider
        .ratings(MediaKind::Movie, &ids(&[("imdb", HEAT)]), &opts())
        .await
        .unwrap_err();
    assert!(
        matches!(error.root(), ProviderError::Auth { .. }),
        "{error:?}"
    );
    assert!(!error.to_string().contains(MDBLIST_KEY));
}

// ---- the IMDb ratings dataset --------------------------------------------------------------

const DATASET_PATH: &str = "/title.ratings.tsv.gz";

fn dataset_at(server: &MockServer, dir: &Path) -> ImdbRatingsDataset {
    ImdbRatingsDataset::with_http(http(), dir, &server.url(DATASET_PATH))
}

#[tokio::test]
async fn the_dataset_downloads_once_and_answers_from_disk() {
    let server = start().await;
    server.route(
        DATASET_PATH,
        dataset_reply(&fixture("ratings", "title.ratings.tsv"), Some("\"v1\"")),
    );
    let cache = tempfile::tempdir().unwrap();
    let dataset = dataset_at(&server, cache.path());
    assert!(!dataset.has_copy());

    let ratings = dataset
        .ratings(MediaKind::Movie, &ids(&[("imdb", HEAT)]), &opts())
        .await
        .unwrap();
    let imdb = ratings.get("imdb").unwrap();
    assert_eq!((imdb.value, imdb.votes, imdb.max_value), (8.3, 710_000, 10));
    let other = dataset
        .ratings(MediaKind::Show, &ids(&[("imdb", "tt0903747")]), &opts())
        .await
        .unwrap();
    assert_eq!(other.get("imdb").unwrap().value, 9.5);

    assert_eq!(
        server.count(DATASET_PATH),
        1,
        "the second lookup reads the copy"
    );
    let meta = dataset.meta().unwrap();
    assert_eq!(meta.rows, 6);
    assert_eq!(meta.etag.as_deref(), Some("\"v1\""));
    let mut files: Vec<String> = std::fs::read_dir(cache.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(files, ["meta.json", "ratings.bin"]);

    // A title the dataset lacks, and a title with no IMDb ID.
    assert!(matches!(
        dataset
            .ratings(MediaKind::Movie, &ids(&[("imdb", "tt5555555")]), &opts())
            .await,
        Err(ProviderError::NotFound(_))
    ));
    assert!(matches!(
        dataset
            .ratings(MediaKind::Movie, &ids(&[("tmdb", "949")]), &opts())
            .await,
        Err(ProviderError::NotFound(_))
    ));
    assert_eq!(server.count(DATASET_PATH), 1);
}

#[tokio::test]
async fn an_old_copy_is_revalidated_with_its_etag() {
    let server = start().await;
    server.script(
        DATASET_PATH,
        vec![
            dataset_reply(&fixture("ratings", "title.ratings.tsv"), Some("\"v1\"")),
            Reply::status(304),
        ],
    );
    let cache = tempfile::tempdir().unwrap();
    let first = dataset_at(&server, cache.path());
    assert_eq!(first.ensure().await.unwrap(), DatasetOutcome::Updated);

    // A zero refresh interval makes the copy due at once.
    let second = dataset_at(&server, cache.path()).refresh_interval(Duration::ZERO);
    assert_eq!(second.ensure().await.unwrap(), DatasetOutcome::Unchanged);
    let revalidation = &server.requests()[1];
    assert_eq!(
        revalidation
            .headers
            .get("if-none-match")
            .map(String::as_str),
        Some("\"v1\"")
    );
    assert!(second.lookup(HEAT).is_some(), "the copy is kept");
    // The next check is a day away.
    assert_eq!(first.ensure().await.unwrap(), DatasetOutcome::Fresh);
    assert_eq!(server.count(DATASET_PATH), 2);
}

#[tokio::test]
async fn a_changed_file_replaces_the_copy_and_an_identical_one_keeps_it() {
    let server = start().await;
    let original = fixture("ratings", "title.ratings.tsv");
    let changed = original.replace("tt0113277\t8.3\t710000", "tt0113277\t8.4\t730000");
    server.script(
        DATASET_PATH,
        vec![
            dataset_reply(&original, None),
            dataset_reply(&original, None),
            dataset_reply(&changed, None),
        ],
    );
    let cache = tempfile::tempdir().unwrap();
    let dataset = dataset_at(&server, cache.path()).refresh_interval(Duration::ZERO);
    assert_eq!(dataset.ensure().await.unwrap(), DatasetOutcome::Updated);
    // Same bytes under no ETag: the converted copy stays.
    assert_eq!(dataset.ensure().await.unwrap(), DatasetOutcome::Unchanged);
    assert_eq!(dataset.lookup(HEAT).unwrap().votes, 710_000);
    assert_eq!(dataset.ensure().await.unwrap(), DatasetOutcome::Updated);
    let rating = dataset.lookup(HEAT).unwrap();
    assert_eq!((rating.value, rating.votes), (8.4, 730_000));
}

#[tokio::test]
async fn an_unreachable_server_keeps_the_old_copy_and_is_not_retried_at_once() {
    let server = start().await;
    server.script(
        DATASET_PATH,
        vec![dataset_reply(
            &fixture("ratings", "title.ratings.tsv"),
            None,
        )],
    );
    server.route(DATASET_PATH, Reply::status(500));
    let cache = tempfile::tempdir().unwrap();
    let dataset = dataset_at(&server, cache.path()).refresh_interval(Duration::ZERO);
    dataset.ensure().await.unwrap();

    let outcome = dataset.ensure().await.unwrap();
    assert!(
        matches!(outcome, DatasetOutcome::KeptAfterError(_)),
        "{outcome:?}"
    );
    assert!(dataset.lookup(HEAT).is_some());
    let requests = server.count(DATASET_PATH);
    for _ in 0..3 {
        dataset.ensure().await.unwrap();
    }
    assert_eq!(
        server.count(DATASET_PATH),
        requests,
        "the failure is remembered"
    );
}

#[tokio::test]
async fn without_a_copy_a_failed_download_is_an_error_and_a_foreign_file_is_refused() {
    let server = start().await;
    server.route(DATASET_PATH, Reply::status(503));
    let cache = tempfile::tempdir().unwrap();
    let dataset = dataset_at(&server, cache.path());
    assert!(dataset.ensure().await.is_err());
    let requests = server.count(DATASET_PATH);
    assert!(dataset.ensure().await.is_err());
    assert_eq!(server.count(DATASET_PATH), requests);
    assert!(!dataset.has_copy());

    let other_server = start().await;
    other_server.route(DATASET_PATH, Reply::ok("<html>Access denied</html>"));
    let cache = tempfile::tempdir().unwrap();
    let dataset = dataset_at(&other_server, cache.path());
    assert!(dataset.ensure().await.is_err());
    assert!(
        !dataset.has_copy(),
        "nothing is stored for a file that is not the dataset"
    );
}

// ---- the fetcher ---------------------------------------------------------------------------

struct Rig {
    server: MockServer,
    fetcher: RatingFetcher,
    _cache: tempfile::TempDir,
}

struct Options {
    omdb_key: bool,
    mdblist_key: bool,
    mdblist_status: u16,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            omdb_key: true,
            mdblist_key: true,
            mdblist_status: 200,
        }
    }
}

async fn rig(options: Options) -> Rig {
    let server = start().await;
    server.route_query(
        "/",
        &format!("i={HEAT}"),
        Reply::ok(&fixture("omdb", "title_movie.json")),
    );
    let mut mdblist = Reply::ok(&fixture("ratings", "mdblist_heat.json"));
    mdblist.status = options.mdblist_status;
    server.route(&format!("/imdb/movie/{HEAT}"), mdblist);
    server.route(
        DATASET_PATH,
        dataset_reply(&fixture("ratings", "title.ratings.tsv"), None),
    );
    let resolver = resolver(options.omdb_key, options.mdblist_key);
    let cache = tempfile::tempdir().unwrap();
    let fetcher = RatingFetcher::new(resolver.clone())
        .with_omdb(Arc::new(OmdbProvider::with_base_url(
            http(),
            resolver.clone(),
            &server.url(""),
        )))
        .with_mdblist(Arc::new(MdbListProvider::with_base_url(
            http(),
            resolver,
            &server.url(""),
        )))
        .with_dataset(Arc::new(dataset_at(&server, cache.path())));
    Rig {
        server,
        fetcher,
        _cache: cache,
    }
}

fn status_of(fetched: &FetchedRatings, source: RatingSource) -> &SourceStatus {
    &fetched
        .outcomes
        .iter()
        .find(|o| o.source == source)
        .unwrap_or_else(|| panic!("{source:?} was not asked"))
        .status
}

fn scraper(ratings: &[Rating]) -> FakeScraper {
    FakeScraper {
        ratings: ratings.to_vec(),
    }
}

#[tokio::test]
async fn all_four_sources_are_merged_and_the_better_supported_rating_wins() {
    let rig = rig(Options::default()).await;
    let scraper = scraper(&[
        Rating::new("tmdb", 7.5, 5_000, 10),
        Rating::new("user", 1.0, 0, 10),
    ]);
    let fetched = rig
        .fetcher
        .fetch(
            MediaKind::Movie,
            &ids(&[("imdb", HEAT)]),
            Some(&scraper),
            &RatingSource::ALL,
            &opts(),
        )
        .await;

    let r = &fetched.ratings;
    // IMDb: dataset 710000 votes, OMDb 700123, MDBList 650000: the dataset wins.
    assert_eq!(
        (r.get("imdb").unwrap().value, r.get("imdb").unwrap().votes),
        (8.3, 710_000)
    );
    // TMDB: the scraper has 5000 votes, MDBList 7000.
    assert_eq!(
        (r.get("tmdb").unwrap().value, r.get("tmdb").unwrap().votes),
        (7.9, 7_000)
    );
    // Rotten Tomatoes critics: OMDb has 70 reviews, MDBList 87.
    assert_eq!(r.get("tomatometerallcritics").unwrap().votes, 87);
    // Only MDBList has Letterboxd and Roger Ebert.
    assert_eq!(r.get("letterboxd").unwrap().max_value, 5);
    assert_eq!(r.get("rogerebert").unwrap().max_value, 4);
    // Only OMDb has the Rotten Tomatoes averages.
    assert!(r.get("tomatometeravgcritics").is_some());
    assert!(
        r.get("user").is_none(),
        "a fetch never carries a user rating"
    );
    for source in RatingSource::ALL {
        assert!(
            matches!(status_of(&fetched, source), SourceStatus::Fetched(_)),
            "{source:?}"
        );
    }
    assert!(fetched.notices().is_empty());
}

#[tokio::test]
async fn a_source_without_credentials_is_skipped_with_a_notice_and_the_others_still_run() {
    let rig = rig(Options {
        omdb_key: false,
        ..Options::default()
    })
    .await;
    let fetched = rig
        .fetcher
        .fetch(
            MediaKind::Movie,
            &ids(&[("imdb", HEAT)]),
            None,
            &[
                RatingSource::Omdb,
                RatingSource::Mdblist,
                RatingSource::ImdbDataset,
            ],
            &opts(),
        )
        .await;

    assert!(matches!(
        status_of(&fetched, RatingSource::Omdb),
        SourceStatus::Skipped(SkipReason::MissingCredentials { provider, .. }) if provider == "OMDb"
    ));
    assert!(matches!(
        status_of(&fetched, RatingSource::Mdblist),
        SourceStatus::Fetched(_)
    ));
    assert!(matches!(
        status_of(&fetched, RatingSource::ImdbDataset),
        SourceStatus::Fetched(_)
    ));
    assert!(fetched.ratings.get("letterboxd").is_some());
    assert!(fetched.ratings.get("tomatometeravgcritics").is_none());
    assert!(
        rig.server.requests().iter().all(|r| r.path() != "/"),
        "OMDb was never contacted"
    );
    let notices = fetched.notices();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].source, RatingSource::Omdb);
    assert!(notices[0].message.contains("OMDb was skipped"));
    assert!(!notices[0].message.contains(OMDB_KEY));
}

#[tokio::test]
async fn a_failing_source_does_not_stop_the_others() {
    let rig = rig(Options {
        mdblist_status: 500,
        ..Options::default()
    })
    .await;
    let fetched = rig
        .fetcher
        .fetch(
            MediaKind::Movie,
            &ids(&[("imdb", HEAT)]),
            None,
            &[
                RatingSource::Mdblist,
                RatingSource::ImdbDataset,
                RatingSource::Omdb,
            ],
            &opts(),
        )
        .await;
    assert!(matches!(
        status_of(&fetched, RatingSource::Mdblist),
        SourceStatus::Failed(_)
    ));
    assert!(matches!(
        status_of(&fetched, RatingSource::ImdbDataset),
        SourceStatus::Fetched(_)
    ));
    assert!(matches!(
        status_of(&fetched, RatingSource::Omdb),
        SourceStatus::Fetched(_)
    ));
    assert!(fetched.ratings.get("imdb").is_some());
    assert!(
        fetched
            .notices()
            .iter()
            .any(|n| n.message.starts_with("MDBList failed"))
    );
}

#[tokio::test]
async fn unselected_sources_are_not_asked_and_a_missing_scraper_is_skipped() {
    let rig = rig(Options::default()).await;
    let fetched = rig
        .fetcher
        .fetch(
            MediaKind::Movie,
            &ids(&[("imdb", HEAT)]),
            None,
            &[RatingSource::Scraper, RatingSource::ImdbDataset],
            &opts(),
        )
        .await;
    assert_eq!(fetched.outcomes.len(), 2);
    assert_eq!(
        *status_of(&fetched, RatingSource::Scraper),
        SourceStatus::Skipped(SkipReason::NoScraper)
    );
    assert_eq!(rig.server.count(&format!("/imdb/movie/{HEAT}")), 0);
    assert_eq!(rig.server.count("/"), 0);
}

#[tokio::test]
async fn a_title_a_source_does_not_know_is_not_an_error() {
    let rig = rig(Options::default()).await;
    let fetched = rig
        .fetcher
        .fetch(
            MediaKind::Movie,
            &ids(&[("imdb", "tt7654321")]),
            None,
            &[RatingSource::Mdblist, RatingSource::ImdbDataset],
            &opts(),
        )
        .await;
    assert_eq!(
        *status_of(&fetched, RatingSource::Mdblist),
        SourceStatus::NotFound
    );
    assert_eq!(
        *status_of(&fetched, RatingSource::ImdbDataset),
        SourceStatus::NotFound
    );
    assert!(!fetched.found_any());
    assert!(fetched.notices().is_empty());
}

fn heat() -> Item {
    let mut item = Item::movie("Heat");
    item.core.ids.set("imdb", HEAT);
    item.core.ratings.set_user_rating(9.0).unwrap();
    item.core.ratings.insert(Rating::new("trakt", 6.0, 1, 10));
    item
}

#[tokio::test]
async fn fetching_into_an_item_adds_sources_and_leaves_the_user_rating_alone() {
    let rig = rig(Options::default()).await;
    let mut item = heat();
    let settings = RatingFetchSettings {
        sources: vec![RatingSource::Mdblist, RatingSource::ImdbDataset],
        after_scrape: false,
    };
    let fetched = rig
        .fetcher
        .fetch_item(&mut item, None, &settings, &opts())
        .await
        .unwrap();
    assert!(fetched.found_any());
    let ratings = &item.core.ratings;
    assert_eq!(ratings.user_rating(), Some(9.0));
    assert_eq!(ratings.get("imdb").unwrap().votes, 710_000);
    assert_eq!(ratings.get("letterboxd").unwrap().max_value, 5);
    assert_eq!(
        ratings.get("trakt").unwrap().value,
        8.0,
        "a fetched source replaces the stored one"
    );

    // Seasons and episodes have no fetched ratings.
    let mut episode = Item::episode(cv_domain::ItemId(1), "Pilot");
    assert!(
        rig.fetcher
            .fetch_item(&mut episode, None, &settings, &opts())
            .await
            .is_none()
    );
    assert_eq!(episode.kind(), ItemKind::Episode);
}

#[tokio::test]
async fn the_fetch_after_a_scrape_follows_the_setting_of_the_kind() {
    let rig = rig(Options::default()).await;
    let mut settings = RatingSettings::default();

    // Defaults: movies and shows both fetch after a scrape.
    let mut show = Item::show("Heat: The Series");
    show.core.ids.set("imdb", HEAT);
    assert!(
        rig.fetcher
            .after_scrape(&mut show, None, &settings, &opts())
            .await
            .is_some()
    );
    assert!(show.core.ratings.get("imdb").is_some());

    // Turned off for shows.
    settings.show.after_scrape = false;
    let mut quiet = Item::show("Heat: The Series");
    quiet.core.ids.set("imdb", HEAT);
    assert!(
        rig.fetcher
            .after_scrape(&mut quiet, None, &settings, &opts())
            .await
            .is_none()
    );
    assert!(quiet.core.ratings.is_empty());

    let mut movie = heat();
    let fetched = rig
        .fetcher
        .after_scrape(&mut movie, None, &settings, &opts())
        .await
        .unwrap();
    assert!(fetched.found_any());
    assert_eq!(movie.core.ratings.get("imdb").unwrap().value, 8.3);

    // Turned off for movies.
    settings.movie.after_scrape = false;
    let mut again = heat();
    assert!(
        rig.fetcher
            .after_scrape(&mut again, None, &settings, &opts())
            .await
            .is_none()
    );
    assert!(again.core.ratings.get("imdb").is_none());

    // Turned on for shows, with a source list of the user's choosing.
    settings.show = RatingFetchSettings {
        sources: vec![RatingSource::Mdblist],
        after_scrape: true,
    };
    let _ = rig
        .fetcher
        .after_scrape(&mut show, None, &settings, &opts())
        .await;
    // MDBList has no show page for this ID in the mock, so nothing was added, but it was asked.
    assert!(rig.server.count("/imdb/show/tt0113277") >= 1);
}

#[tokio::test]
async fn a_run_reports_each_skipped_source_once() {
    let rig = rig(Options {
        omdb_key: false,
        ..Options::default()
    })
    .await;
    let mut notices = RunNotices::new();
    for _ in 0..3 {
        let fetched = rig
            .fetcher
            .fetch(
                MediaKind::Movie,
                &ids(&[("imdb", HEAT)]),
                None,
                &[RatingSource::Omdb, RatingSource::ImdbDataset],
                &opts(),
            )
            .await;
        notices.add(&fetched);
    }
    let messages = notices.messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].source, RatingSource::Omdb);
}

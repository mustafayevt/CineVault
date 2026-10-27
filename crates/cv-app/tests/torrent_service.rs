//! `TorrentService`: TMDB title search and details through the real TMDB provider
//! against recorded answers (the fixtures of `cv-providers`, served over loopback), the library
//! link, the source list from the settings, and the responsibility notice that keeps everything
//! quiet until it is accepted.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cv_app::app_settings::SettingsService;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_app::torrent_settings::{ADMIN_PASSWORD_ACCOUNT, TORZNAB_KEY_ACCOUNT};
use cv_app::torrents::{LibraryHolding, TorrentService, TorrentServiceError};
use cv_db::repo::insert_item;
use cv_db::{Database, DbOptions};
use cv_domain::{Item, ItemDetail, MediaFile, MediaFileType, MediaSource};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, ProviderRegistry, TmdbProvider,
    catalogue,
};
use cv_torrent::TorrentError;
use cv_torrent::model::{Hdr, MediaKind, Resolution, Scope};
use cv_torrent::source::TorrentSource;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serves the recorded TMDB answers by path.
struct Tmdb {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../cv-providers/tests/fixtures/tmdb")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

impl Tmdb {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let log = Arc::clone(&requests);
        let routes: HashMap<&'static str, &'static str> = HashMap::from([
            ("/3/configuration", "configuration.json"),
            ("/3/movie/949", "movie_949.json"),
            ("/3/tv/1396", "tv_1396.json"),
            ("/3/tv/1396/season/0", "tv_1396_season_0.json"),
            ("/3/tv/1396/season/1", "tv_1396_season_1.json"),
            ("/3/search/tv", "search_tv.json"),
            ("/3/find/tt0113277", "find_imdb_heat.json"),
        ]);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let routes = routes.clone();
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let mut buffer = vec![0u8; 8192];
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    let head = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    let target = head.split(' ').nth(1).unwrap_or("/").to_owned();
                    log.lock().unwrap().push(target.clone());
                    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                    let file = if path == "/3/search/movie" {
                        Some(if query.contains("page=2") {
                            "search_movie_page2.json"
                        } else {
                            "search_movie_page1.json"
                        })
                    } else {
                        routes.get(path).copied()
                    };
                    let (status, body) = match file {
                        Some(name) => (200, fixture(name)),
                        None => (404, fixture("error_404.json")),
                    };
                    let response = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self { addr, requests }
    }

    fn calls(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|t| !t.starts_with("/3/configuration"))
            .cloned()
            .collect()
    }
}

struct Lab {
    tmdb: Tmdb,
    service: Arc<TorrentService>,
    settings: Arc<SettingsService>,
    secrets: Arc<SecretStore>,
    db: Arc<Database>,
    _dir: tempfile::TempDir,
}

impl Lab {
    async fn new(feature_on: bool) -> Self {
        Self::build(feature_on, true).await
    }

    async fn build(feature_on: bool, with_key: bool) -> Self {
        let tmdb = Tmdb::start().await;
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            Database::open_with(dir.path().join("library.db"), &DbOptions { readers: 2 }).unwrap(),
        );
        let settings = Arc::new(SettingsService::in_memory("test"));
        let secrets = Arc::new(SecretStore::in_memory());
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        if with_key {
            resolver
                .set_user(
                    catalogue::key::TMDB,
                    catalogue::API_KEY,
                    "test-key-0123456789",
                )
                .unwrap();
        }
        let http = HttpClient::builder().build().unwrap();
        let provider =
            TmdbProvider::with_base_url(http, resolver.clone(), &format!("http://{}/3", tmdb.addr));
        let mut registry = ProviderRegistry::new();
        registry.register_metadata(Arc::new(provider));
        let providers = Arc::new(ProviderCredentialsService::with_resolver(
            resolver, registry,
        ));
        let service = Arc::new(TorrentService::new(
            Arc::clone(&settings),
            Arc::clone(&secrets),
            Arc::clone(&db),
            providers,
        ));
        if feature_on {
            settings
                .set("torrents.notice-accepted", serde_json::Value::Bool(true))
                .unwrap();
        }
        Self {
            tmdb,
            service,
            settings,
            secrets,
            db,
            _dir: dir,
        }
    }

    fn library_movie(&self, tmdb: &str, title: &str, year: u16, width: u32, height: u32) {
        let mut item = Item::movie(title);
        item.core.year = year;
        item.core.ids.set("tmdb", tmdb);
        if let ItemDetail::Movie(movie) = &mut item.detail {
            movie.media_source = MediaSource::WebDl;
        }
        let mut video = MediaFile::new(MediaFileType::Video, "C:/movies/x", "x.mkv");
        video.container_format = "Matroska".into();
        video.width = width;
        video.height = height;
        video.size_bytes = 1_000;
        item.core.media_files = vec![video];
        self.db.write(move |c| insert_item(c, &item)).unwrap();
    }
}

// ------------------------------------------------------------------ titles

#[tokio::test]
async fn the_exact_title_and_year_come_first_and_the_library_title_is_linked() {
    let lab = Lab::new(true).await;
    lab.library_movie("949", "Heat", 1995, 1920, 1080);
    let hits = lab
        .service
        .title_hits(MediaKind::Movie, "Heat 1995")
        .await
        .expect("hits");
    assert!(hits.len() >= 2, "{hits:?}");
    assert_eq!(hits[0].tmdb_id, 949);
    assert_eq!(hits[0].title, "Heat");
    assert_eq!(hits[0].year, Some(1995));
    assert!(hits[0].confidence > hits[1].confidence, "{hits:?}");
    assert!(
        hits.windows(2).all(|w| w[0].confidence >= w[1].confidence),
        "best first"
    );
    let link = hits[0].in_library.as_ref().expect("linked");
    assert_eq!(link.name, "Heat (1995)");
    assert!(hits[1].in_library.is_none());
}

#[tokio::test]
async fn a_movie_and_a_show_with_the_same_number_are_not_confused() {
    let lab = Lab::new(true).await;
    // A library SHOW with TMDB id 949 must not mark the movie 949.
    let mut show = Item::show("Some Show");
    show.core.ids.set("tmdb", "949");
    lab.db.write(move |c| insert_item(c, &show)).unwrap();
    let hits = lab
        .service
        .title_hits(MediaKind::Movie, "Heat 1995")
        .await
        .expect("hits");
    assert!(hits[0].in_library.is_none());
}

#[tokio::test]
async fn an_id_shortcut_finds_the_title_and_a_wrong_kind_finds_nothing() {
    let lab = Lab::new(true).await;
    let hits = lab
        .service
        .title_hits(MediaKind::Movie, "tt0113277")
        .await
        .expect("hits");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].tmdb_id, 949);
    assert!(
        (hits[0].confidence - 1.0).abs() < f64::EPSILON,
        "found by ID is the title asked for"
    );
    assert!(
        lab.tmdb
            .calls()
            .iter()
            .any(|t| t.starts_with("/3/find/tt0113277"))
    );
}

#[tokio::test]
async fn text_that_is_too_short_searches_nothing() {
    let lab = Lab::new(true).await;
    assert!(
        lab.service
            .title_hits(MediaKind::Movie, "h")
            .await
            .expect("hits")
            .is_empty()
    );
    assert!(lab.tmdb.calls().is_empty());
}

#[tokio::test]
async fn shows_are_searched_as_shows() {
    let lab = Lab::new(true).await;
    let hits = lab
        .service
        .title_hits(MediaKind::Show, "Breaking Bad")
        .await
        .expect("hits");
    assert!(
        hits.iter()
            .any(|h| h.tmdb_id == 1396 && h.kind == MediaKind::Show),
        "{hits:?}"
    );
}

#[tokio::test]
async fn details_give_ids_titles_year_runtime_and_the_library_link() {
    let lab = Lab::new(true).await;
    lab.library_movie("949", "Heat", 1995, 1920, 1080);
    let details = lab
        .service
        .title_details(MediaKind::Movie, 949)
        .await
        .expect("details");
    assert_eq!(details.title, "Heat");
    assert_eq!(details.year, Some(1995));
    assert_eq!(details.runtime_minutes, Some(170));
    assert_eq!(details.ids.tmdb, Some(949));
    assert_eq!(details.ids.imdb.as_deref(), Some("tt0113277"));
    assert!(!details.anime);
    assert!(details.library_item.is_some());
    // The header facts.
    assert_eq!(details.genres, ["Action", "Crime", "Drama", "Thriller"]);
    assert!((details.rating.expect("rating") - 7.9).abs() < 0.01);
    assert!(details.backdrop_url.is_some());
    assert!(
        details
            .overview
            .as_deref()
            .is_some_and(|o| o.contains("McCauley"))
    );
    match details.holding.expect("the library copy") {
        LibraryHolding::Movie(copy) => {
            assert_eq!(copy.resolution, Resolution::R1080);
            assert_eq!(copy.source, MediaSource::WebDl);
            assert_eq!(copy.size_bytes, 1_000);
            assert_eq!(copy.file_name, "x.mkv");
        }
        other => panic!("a movie copy, not {other:?}"),
    }

    let show = lab
        .service
        .title_details(MediaKind::Show, 1396)
        .await
        .expect("show");
    assert_eq!(show.title, "Breaking Bad");
    assert_eq!(show.ids.imdb.as_deref(), Some("tt0903747"));
    assert_eq!(show.ids.tvdb, Some(81189));
    assert!(show.library_item.is_none());
}

#[tokio::test]
async fn the_landing_lists_movies_below_2160p_and_the_newest_titles() {
    let lab = Lab::new(true).await;
    lab.library_movie("949", "Heat", 1995, 1280, 720);
    lab.library_movie("1091", "The Thing", 1982, 3840, 2160);
    lab.library_movie("872585", "Oppenheimer", 2023, 1920, 1080);
    let landing = lab.service.landing().expect("landing");
    let upgrades: Vec<&str> = landing.upgrades.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(
        upgrades,
        ["Heat", "Oppenheimer"],
        "720p first, 2160p left out"
    );
    assert_eq!(landing.upgrade_total, 2);
    assert_eq!(landing.upgrades[0].tmdb_id, 949);
    assert_eq!(landing.recent.len(), 3);

    lab.settings
        .set("torrents.notice-accepted", serde_json::Value::Bool(false))
        .unwrap();
    assert!(
        lab.service.landing().is_err(),
        "nothing is read before the notice is accepted"
    );
}

#[tokio::test]
async fn an_unknown_tmdb_id_is_not_found() {
    let lab = Lab::new(true).await;
    let error = lab
        .service
        .title_details(MediaKind::Movie, 424_242)
        .await
        .unwrap_err();
    assert!(matches!(error, TorrentServiceError::NotFound), "{error:?}");
}

#[tokio::test]
async fn without_a_tmdb_key_the_service_says_so() {
    let lab = Lab::build(true, false).await;
    let error = lab
        .service
        .title_hits(MediaKind::Movie, "Heat")
        .await
        .unwrap_err();
    assert!(matches!(error, TorrentServiceError::NeedsKey), "{error:?}");
}

// ------------------------------------------------------------------ seasons and episodes

#[tokio::test]
async fn seasons_and_episodes_come_from_one_episode_list() {
    let lab = Lab::new(true).await;
    let seasons = lab.service.seasons(1396).await.expect("seasons");
    assert_eq!(
        seasons.iter().map(|s| s.number).collect::<Vec<_>>(),
        vec![1, 0],
        "specials last"
    );
    assert_eq!(seasons[0].name, "Season 1");
    assert_eq!(seasons[0].air_date.as_deref(), Some("2008-01-20"));
    assert!(seasons[0].episode_count >= 2);
    assert_eq!(seasons[1].name, "Specials");

    let episodes = lab.service.episodes(1396, 1).await.expect("episodes");
    assert_eq!(episodes[0].number, 1);
    assert_eq!(episodes[0].name, "Pilot");
    assert_eq!(episodes[0].air_date.as_deref(), Some("2008-01-20"));
    assert_eq!(episodes.len() as u32, seasons[0].episode_count);
    assert!(
        lab.service
            .episodes(1396, 9)
            .await
            .expect("none")
            .is_empty()
    );

    let season_calls = lab
        .tmdb
        .calls()
        .iter()
        .filter(|t| t.contains("/season/"))
        .count();
    assert_eq!(
        season_calls,
        2,
        "the list was read once for both pickers: {:?}",
        lab.tmdb.calls()
    );
}

// ------------------------------------------------------------------ queries and the library

#[tokio::test]
async fn the_query_carries_the_title_scope_and_the_library_copy() {
    let lab = Lab::new(true).await;
    lab.library_movie("949", "Heat", 1995, 1920, 1080);
    let details = lab
        .service
        .title_details(MediaKind::Movie, 949)
        .await
        .unwrap();
    let query = lab.service.build_query(&details, Scope::Movie).unwrap();
    assert_eq!(query.title, "Heat");
    assert_eq!(query.year, Some(1995));
    assert_eq!(query.runtime_minutes, Some(170));
    assert_eq!(query.ids.imdb.as_deref(), Some("tt0113277"));
    let copy = query.library_copy.expect("the copy");
    assert_eq!(copy.resolution, Resolution::R1080);
    assert_eq!(copy.source, MediaSource::WebDl);
    assert_eq!(copy.hdr, Hdr::None);

    // A movie is always searched as a movie; a show scope of Movie becomes the whole show.
    let show = lab
        .service
        .title_details(MediaKind::Show, 1396)
        .await
        .unwrap();
    assert_eq!(
        lab.service.build_query(&show, Scope::Movie).unwrap().scope,
        Scope::Show
    );
    assert_eq!(
        lab.service
            .build_query(&show, Scope::Episode(1, 2))
            .unwrap()
            .scope,
        Scope::Episode(1, 2)
    );
    assert_eq!(
        lab.service
            .build_query(&details, Scope::Episode(1, 2))
            .unwrap()
            .scope,
        Scope::Movie
    );
}

// ------------------------------------------------------------------ sources

#[tokio::test]
async fn a_fresh_install_searches_all_six_sources() {
    let lab = Lab::new(true).await;
    let details = lab
        .service
        .title_details(MediaKind::Movie, 949)
        .await
        .unwrap();
    let query = lab.service.build_query(&details, Scope::Movie).unwrap();
    let ids: Vec<String> = lab
        .service
        .sources(&query)
        .unwrap()
        .iter()
        .map(|s| s.id().0.clone())
        .collect();
    assert_eq!(
        ids,
        vec!["torrentio", "yts", "eztv", "apibay", "knaben", "nyaa"]
    );
}

#[tokio::test]
async fn a_switched_off_source_is_not_listed_and_the_own_instance_needs_url_and_key() {
    let lab = Lab::new(true).await;
    let query = {
        let details = lab
            .service
            .title_details(MediaKind::Movie, 949)
            .await
            .unwrap();
        lab.service.build_query(&details, Scope::Movie).unwrap()
    };
    lab.settings
        .set("torrents.source.yts", json!(false))
        .unwrap();
    lab.settings
        .set("torrents.torznab.url", json!("http://127.0.0.1:9117/api"))
        .unwrap();
    let ids = |lab: &Lab| -> Vec<String> {
        lab.service
            .sources(&query)
            .unwrap()
            .iter()
            .map(|s| s.id().0.clone())
            .collect()
    };
    assert!(!ids(&lab).contains(&"yts".to_owned()));
    assert!(!ids(&lab).contains(&"torznab".to_owned()), "no key yet");
    lab.secrets
        .set(TORZNAB_KEY_ACCOUNT, "the-torznab-key-1")
        .unwrap();
    assert_eq!(ids(&lab).last().map(String::as_str), Some("torznab"));
    // The admin password of the indexer service is not a source key.
    lab.secrets
        .set(ADMIN_PASSWORD_ACCOUNT, "an-admin-password-1")
        .unwrap();
    assert_eq!(ids(&lab).len(), 6, "five built-in plus the own instance");
}

#[tokio::test]
async fn extra_sources_join_the_list_and_can_be_removed() {
    let lab = Lab::new(true).await;
    let query = {
        let details = lab
            .service
            .title_details(MediaKind::Movie, 949)
            .await
            .unwrap();
        lab.service.build_query(&details, Scope::Movie).unwrap()
    };
    let client = cv_torrent::source::client_builder().build().unwrap();
    let extra: Arc<dyn TorrentSource> = Arc::new(
        cv_torrent::sources::torrentio::TorrentioSource::new(client, "http://127.0.0.1:9"),
    );
    // Any source can be added; here a second Torrentio stands in for an indexer.
    lab.service.set_extra_source(extra);
    assert_eq!(lab.service.sources(&query).unwrap().len(), 7);
    lab.service.remove_extra_source("torrentio");
    assert_eq!(lab.service.sources(&query).unwrap().len(), 6);
}

// ------------------------------------------------------------------ the responsibility notice

#[tokio::test]
async fn before_the_notice_is_accepted_every_method_fails_quietly_and_contacts_nothing() {
    let lab = Lab::new(false).await;
    assert!(!lab.service.notice_accepted());
    let off = |e: &TorrentServiceError| {
        matches!(
            e,
            TorrentServiceError::Torrent(TorrentError::NoticeNotAccepted)
        )
    };
    assert!(off(&lab
        .service
        .title_hits(MediaKind::Movie, "Heat")
        .await
        .unwrap_err()));
    assert!(off(&lab
        .service
        .title_details(MediaKind::Movie, 949)
        .await
        .unwrap_err()));
    assert!(off(&lab.service.seasons(1396).await.unwrap_err()));
    assert!(off(&lab.service.episodes(1396, 1).await.unwrap_err()));
    let query = cv_torrent::model::TorrentQuery {
        kind: MediaKind::Movie,
        ids: cv_torrent::model::TitleIds::default(),
        title: "Heat".into(),
        original_title: None,
        alternative_titles: Vec::new(),
        year: None,
        scope: Scope::Movie,
        runtime_minutes: None,
        anime: false,
        library_copy: None,
    };
    assert!(matches!(
        lab.service.sources(&query),
        Err(TorrentError::NoticeNotAccepted)
    ));
    assert!(
        lab.tmdb.requests.lock().unwrap().is_empty(),
        "not one request reached TMDB"
    );

    // Accepting the notice makes the same calls work with the default settings; taking the
    // answer back stops them again.
    lab.settings
        .set("torrents.notice-accepted", serde_json::Value::Bool(true))
        .unwrap();
    assert!(
        lab.service
            .title_hits(MediaKind::Movie, "Heat 1995")
            .await
            .is_ok()
    );
    lab.settings
        .set("torrents.notice-accepted", serde_json::Value::Bool(false))
        .unwrap();
    assert!(off(&lab
        .service
        .title_hits(MediaKind::Movie, "Heat 1995")
        .await
        .unwrap_err()));
}

// ------------------------------------------------------------------ source status and settings

use cv_app::torrents::{Health, KeyChange};
use cv_torrent::model::{SkipReason, SourceError, SourceOutcome};

#[tokio::test]
async fn the_status_list_has_every_source_with_its_switch_and_no_health_yet() {
    let lab = Lab::new(true).await;
    let list = lab.service.source_statuses().unwrap();
    let ids: Vec<_> = list.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["torrentio", "yts", "eztv", "apibay", "knaben", "nyaa"]
    );
    assert_eq!(list[3].name, "The Pirate Bay");
    assert!(
        list.iter()
            .all(|s| s.on && s.health == Health::Untested && s.checked_at_ms.is_none())
    );

    lab.service.set_source("yts", false).unwrap();
    let list = lab.service.source_statuses().unwrap();
    assert!(!list[1].on);
    assert_eq!(list[1].health, Health::Off);
    assert_eq!(lab.settings.get("torrents.source.yts"), Some(json!(false)));
    lab.service.set_source("yts", true).unwrap();
    assert_eq!(
        lab.service.source_statuses().unwrap()[1].health,
        Health::Untested
    );
}

#[tokio::test]
async fn an_unknown_source_cannot_be_switched_and_the_indexer_service_can() {
    let lab = Lab::new(true).await;
    assert!(lab.service.set_source("nope", false).is_err());
    lab.service.set_source("indexer-service", false).unwrap();
    assert_eq!(
        lab.settings.get("torrents.indexer-service.enabled"),
        Some(json!(false))
    );
}

#[tokio::test]
async fn searches_teach_the_health_column() {
    let lab = Lab::new(true).await;
    lab.service
        .note_outcome("torrentio", &SourceOutcome::Done { releases: 3 });
    lab.service
        .note_outcome("yts", &SourceOutcome::Failed(SourceError::Timeout));
    lab.service
        .note_outcome("knaben", &SourceOutcome::Failed(SourceError::RateLimited));
    lab.service
        .note_outcome("nyaa", &SourceOutcome::Skipped(SkipReason::AnimeOnly));
    let list = lab.service.source_statuses().unwrap();
    let by = |id: &str| list.iter().find(|s| s.id == id).unwrap().clone();
    assert_eq!(by("torrentio").health, Health::Working);
    assert!(by("torrentio").checked_at_ms.is_some());
    assert_eq!(by("yts").health, Health::Unreachable);
    assert_eq!(by("yts").reason.as_deref(), Some("Timed out"));
    assert_eq!(by("knaben").health, Health::Error);
    assert_eq!(by("knaben").reason.as_deref(), Some("Rate limited"));
    assert_eq!(
        by("nyaa").health,
        Health::Untested,
        "a skipped source says nothing"
    );
}

#[tokio::test]
async fn the_own_torznab_instance_is_saved_with_its_key_in_the_store_only() {
    let lab = Lab::new(true).await;
    lab.service
        .set_torznab(
            "http://127.0.0.1:9117/api/",
            KeyChange::Set("super-secret-key-1".into()),
        )
        .unwrap();
    assert_eq!(
        lab.settings.get("torrents.torznab.url"),
        Some(json!("http://127.0.0.1:9117/api"))
    );
    assert!(lab.service.has_torznab_key());
    let everything = serde_json::to_string(&lab.settings.values()).unwrap();
    assert!(
        !everything.contains("super-secret-key-1"),
        "the key is not in the settings"
    );
    assert!(
        format!("{:?}", KeyChange::Set("super-secret-key-1".into()))
            .find("super-secret")
            .is_none()
    );
    let list = lab.service.source_statuses().unwrap();
    assert_eq!(list.last().unwrap().id, "torznab");
    assert!(list.last().unwrap().on);

    // Keep leaves the key; Clear removes it and the instance has no key to search with.
    lab.service
        .set_torznab("http://127.0.0.1:9117/api", KeyChange::Keep)
        .unwrap();
    assert!(lab.service.has_torznab_key());
    lab.service
        .set_torznab("http://127.0.0.1:9117/api", KeyChange::Clear)
        .unwrap();
    assert!(!lab.service.has_torznab_key());
    assert!(!lab.service.source_statuses().unwrap().last().unwrap().on);

    // An empty address removes the instance and any key with it.
    lab.service
        .set_torznab(
            "http://127.0.0.1:9117/api",
            KeyChange::Set("another-key-123".into()),
        )
        .unwrap();
    lab.service.set_torznab("", KeyChange::Keep).unwrap();
    assert!(!lab.service.has_torznab_key());
    assert!(
        lab.service
            .source_statuses()
            .unwrap()
            .iter()
            .all(|s| s.id != "torznab")
    );
}

#[tokio::test]
async fn a_torznab_address_that_is_not_http_is_refused_and_nothing_is_saved() {
    let lab = Lab::new(true).await;
    for bad in [
        "ftp://x.example/api",
        "not a url",
        "https://u:p@x.example/api",
        "https://x.example/api?apikey=1",
    ] {
        assert!(
            lab.service
                .set_torznab(bad, KeyChange::Set("a-key-123456".into()))
                .is_err(),
            "{bad}"
        );
    }
    assert!(!lab.service.has_torznab_key());
    assert_eq!(lab.settings.get("torrents.torznab.url"), None);
}

#[tokio::test]
async fn testing_a_torznab_instance_needs_an_address_and_a_key() {
    let lab = Lab::new(true).await;
    let error = lab
        .service
        .test_torznab("nonsense", Some("k"))
        .await
        .unwrap_err();
    assert!(matches!(error, SourceError::BadAnswer(_)));
    let error = lab
        .service
        .test_torznab("http://127.0.0.1:9/api", None)
        .await
        .unwrap_err();
    assert_eq!(error, SourceError::BadAnswer("Enter the API key.".into()));
    // Nothing listens on port 9: the instance is unreachable.
    let error = lab
        .service
        .test_torznab("http://127.0.0.1:9/api", Some("a-key-123456"))
        .await
        .unwrap_err();
    assert!(matches!(error, SourceError::Unreachable(_)), "{error:?}");
}

#[tokio::test]
async fn torrent_files_are_fetched_only_from_web_addresses() {
    let lab = Lab::new(true).await;
    assert!(
        lab.service
            .fetch_torrent_file("file:///c:/x.torrent")
            .await
            .is_err()
    );
    assert!(
        lab.service
            .fetch_torrent_file("magnet:?xt=urn:btih:aa")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn all_of_it_refuses_while_the_feature_is_off() {
    let lab = Lab::new(false).await;
    assert!(matches!(
        lab.service.source_statuses(),
        Err(TorrentError::NoticeNotAccepted)
    ));
    assert!(lab.service.set_source("yts", false).is_err());
    assert!(
        lab.service
            .set_torznab("http://x.example/api", KeyChange::Keep)
            .is_err()
    );
    assert!(
        lab.service
            .test_torznab("http://x.example/api", Some("k"))
            .await
            .is_err()
    );
    assert!(
        lab.service
            .fetch_torrent_file("http://x.example/a.torrent")
            .await
            .is_err()
    );
    assert_eq!(lab.settings.get("torrents.source.yts"), None);
}

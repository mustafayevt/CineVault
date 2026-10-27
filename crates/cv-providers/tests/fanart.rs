//! The Fanart.tv provider against a local mock server that replays hand-built answers in the
//! service's documented shape (`tests/fixtures/fanart/`). Nothing here touches the network; the
//! opt-in live smoke test is `tests/fanart_live.rs`.

mod support;

use std::path::Path;
use std::sync::Arc;

use cv_domain::{MediaFileType, ProviderIds, id_key};
use cv_providers::model::{ArtworkCandidate, MediaKind, RequestOptions};
use cv_providers::{
    ArtworkProvider, CredentialResolver, FanartProvider, HttpClient, MemoryUserCredentials,
    Provider, ProviderError, ProviderRegistry, ResolvedCredentials, SizeBucket, catalogue,
};
use support::{MockServer, Reply};

const KEY: &str = "fanart-test-key-0123456789";

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fanart")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn ok(name: &str) -> Reply {
    Reply::ok(&fixture(name))
}

fn status(code: u16, name: &str) -> Reply {
    let mut reply = ok(name);
    reply.status = code;
    reply
}

struct Harness {
    server: MockServer,
    provider: FanartProvider,
    resolver: CredentialResolver,
    _cache: Option<tempfile::TempDir>,
}

impl Harness {
    async fn new() -> Self {
        Self::build(Some(KEY), false).await
    }

    async fn build(key: Option<&str>, cache: bool) -> Self {
        let server = MockServer::start().await;
        server.route("/movies/tt0111222", ok("movie_example.json"));
        server.route("/movies/9001", ok("movie_example.json"));
        server.route("/tv/81189", ok("show_example.json"));
        server.route("/movies/latest", ok("latest.json"));
        let cache = cache.then(|| tempfile::tempdir().unwrap());
        let mut builder = HttpClient::builder();
        if let Some(dir) = &cache {
            builder = builder.cache_dir(dir.path());
        }
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        if let Some(key) = key {
            resolver
                .set_user(catalogue::key::FANART, catalogue::API_KEY, key)
                .unwrap();
        }
        let provider = FanartProvider::with_base_url(
            builder.build().unwrap(),
            resolver.clone(),
            &server.url(""),
        );
        Self {
            server,
            provider,
            resolver,
            _cache: cache,
        }
    }
}

fn opts() -> RequestOptions {
    RequestOptions::default()
}

fn movie_ids() -> ProviderIds {
    let mut ids = ProviderIds::new();
    ids.set(id_key::IMDB, "tt0111222");
    ids.set(id_key::TMDB, "9001");
    ids
}

fn show_ids() -> ProviderIds {
    let mut ids = ProviderIds::new();
    ids.set(id_key::TVDB, "81189");
    ids
}

/// `(type, file name without extension)`, in list order.
fn names(list: &[ArtworkCandidate]) -> Vec<(MediaFileType, String)> {
    list.iter()
        .map(|c| {
            let file = c.url.rsplit('/').next().unwrap();
            (
                c.kind,
                file.rsplit_once('.').map_or(file, |(n, _)| n).to_owned(),
            )
        })
        .collect()
}

fn of_kind(list: &[ArtworkCandidate], kind: MediaFileType) -> Vec<String> {
    names(list)
        .into_iter()
        .filter(|(k, _)| *k == kind)
        .map(|(_, n)| n)
        .collect()
}

// --- the provider -----------------------------------------------------------------------------

#[tokio::test]
async fn the_provider_declares_its_key_and_name() {
    let h = Harness::new().await;
    assert_eq!(h.provider.id(), "fanart");
    assert_eq!(h.provider.name(), "Fanart.tv");
    let schema = h.provider.credentials().expect("a credential schema");
    assert_eq!(schema.fields.len(), 1);
    assert!(schema.fields[0].required);
}

// --- movies -----------------------------------------------------------------------------------

#[tokio::test]
async fn movie_artwork_maps_every_fanart_type() {
    let h = Harness::new().await;
    let list = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    let kinds: Vec<MediaFileType> = {
        let mut kinds: Vec<_> = list.iter().map(|c| c.kind).collect();
        kinds.dedup();
        kinds
    };
    assert_eq!(
        kinds,
        [
            MediaFileType::Poster,
            MediaFileType::Fanart,
            MediaFileType::Banner,
            MediaFileType::Clearart,
            MediaFileType::Disc,
            MediaFileType::Clearlogo,
            MediaFileType::Thumb,
            MediaFileType::Keyart,
        ]
    );
    // The entry without a URL is dropped.
    assert_eq!(of_kind(&list, MediaFileType::Poster).len(), 3);
}

#[tokio::test]
async fn posters_are_sorted_by_language_then_likes() {
    let h = Harness::new().await;
    let list = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    // English first (the preferred and fallback language), then the rest.
    assert_eq!(
        of_kind(&list, MediaFileType::Poster),
        ["poster-en-high", "poster-en-low", "poster-de"]
    );
    assert_eq!(
        list.iter()
            .find(|c| c.url.ends_with("poster-en-high.jpg"))
            .unwrap()
            .score,
        5.0
    );

    let german = RequestOptions {
        language: "de".into(),
        ..opts()
    };
    let list = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &german)
        .await
        .unwrap();
    assert_eq!(
        of_kind(&list, MediaFileType::Poster),
        ["poster-de", "poster-en-high", "poster-en-low"]
    );
}

#[tokio::test]
async fn a_poster_without_text_is_key_art() {
    let h = Harness::new().await;
    let list = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    assert_eq!(of_kind(&list, MediaFileType::Keyart), ["poster-textless"]);
    let keyart = list
        .iter()
        .find(|c| c.kind == MediaFileType::Keyart)
        .unwrap();
    assert_eq!(keyart.language, None);
    assert_eq!(keyart.score, 3.0);
    // The likes are kept as likes, so the ranking can weigh them against TMDB votes.
    assert_eq!(
        keyart.popularity,
        cv_providers::model::Popularity::Liked { likes: 3 }
    );
}

#[tokio::test]
async fn textless_backgrounds_have_no_language() {
    let h = Harness::new().await;
    let list = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    let backgrounds: Vec<&ArtworkCandidate> = list
        .iter()
        .filter(|c| c.kind == MediaFileType::Fanart)
        .collect();
    assert_eq!(backgrounds.len(), 2);
    // The one with text in the preferred language comes first; the textless one still carries
    // no language, so the best-image selection can prefer it.
    assert_eq!(backgrounds[0].language.as_deref(), Some("en"));
    assert_eq!(backgrounds[1].language, None);
    assert_eq!(backgrounds[1].score, 6.0);
}

#[tokio::test]
async fn hd_logos_and_clearart_come_before_standard_definition_ones() {
    let h = Harness::new().await;
    let list = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    // The standard-definition logo has the most likes and still comes last.
    assert_eq!(
        of_kind(&list, MediaFileType::Clearlogo),
        ["logo-en", "logo-de", "logo-sd-en"]
    );
    assert_eq!(
        of_kind(&list, MediaFileType::Clearart),
        ["art-en", "art-sd-en"]
    );
}

#[tokio::test]
async fn candidates_carry_the_size_fanart_asks_for_and_a_bucket() {
    let h = Harness::new().await;
    let list = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    let find = |name: &str| list.iter().find(|c| c.url.contains(name)).unwrap();
    let poster = find("poster-en-high");
    assert_eq!((poster.width, poster.height), (1000, 1426));
    assert_eq!(poster.size_bucket(), Some(SizeBucket::Large));
    let background = find("bg-text");
    assert_eq!((background.width, background.height), (1920, 1080));
    assert_eq!(background.size_bucket(), Some(SizeBucket::Large));
    let thumb = find("thumb-en");
    assert_eq!(thumb.size_bucket(), Some(SizeBucket::Big));
    let logo = find("logo-en.png");
    assert_eq!((logo.width, logo.height), (800, 310));
    assert_eq!(logo.size_bucket(), None);
    let disc = find("disc-en");
    assert_eq!((disc.width, disc.height), (1000, 1000));
}

#[tokio::test]
async fn movies_are_asked_for_by_imdb_id_first() {
    let h = Harness::new().await;
    h.provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    let requests = h.server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path(), "/movies/tt0111222");
}

#[tokio::test]
async fn the_tmdb_id_is_tried_when_fanart_does_not_know_the_imdb_id() {
    let h = Harness::new().await;
    h.server
        .route("/movies/tt0111222", status(404, "error_not_found.json"));
    let list = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    assert!(!list.is_empty());
    let paths: Vec<String> = h
        .server
        .requests()
        .iter()
        .map(|r| r.path().to_owned())
        .collect();
    assert_eq!(paths, ["/movies/tt0111222", "/movies/9001"]);

    // A movie with only a TMDB ID goes straight to it.
    let mut only_tmdb = ProviderIds::new();
    only_tmdb.set(id_key::TMDB, "9001");
    let h = Harness::new().await;
    h.provider
        .artwork(MediaKind::Movie, &only_tmdb, &opts())
        .await
        .unwrap();
    assert_eq!(h.server.requests()[0].path(), "/movies/9001");
}

#[tokio::test]
async fn a_title_fanart_does_not_know_offers_nothing() {
    let h = Harness::new().await;
    h.server
        .route("/movies/tt0111222", status(404, "error_not_found.json"));
    h.server
        .route("/movies/9001", status(404, "error_not_found.json"));
    let list = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    assert!(list.is_empty());

    // No usable ID: no request at all.
    let h = Harness::new().await;
    let mut unusable = ProviderIds::new();
    unusable.set(id_key::IMDB, "nm0000093");
    unusable.set(id_key::TVDB, "81189");
    assert!(
        h.provider
            .artwork(MediaKind::Movie, &unusable, &opts())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        h.provider
            .artwork(MediaKind::Show, &movie_ids(), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(h.server.requests().is_empty());
}

// --- shows and seasons ------------------------------------------------------------------------

#[tokio::test]
async fn show_artwork_is_the_shows_own_images_by_tvdb_id() {
    let h = Harness::new().await;
    let list = h
        .provider
        .artwork(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap();
    assert_eq!(h.server.requests()[0].path(), "/tv/81189");
    assert_eq!(of_kind(&list, MediaFileType::Poster), ["poster-en"]);
    assert_eq!(of_kind(&list, MediaFileType::Banner), ["banner-en"]);
    assert_eq!(of_kind(&list, MediaFileType::Thumb), ["thumb-en"]);
    assert_eq!(of_kind(&list, MediaFileType::Characterart), ["char-en"]);
    assert_eq!(
        of_kind(&list, MediaFileType::Clearlogo),
        ["logo-en", "logo-sd-en"]
    );
    assert_eq!(of_kind(&list, MediaFileType::Clearart), ["art-en"]);
    // Only the background that names no season is the show's fanart.
    assert_eq!(of_kind(&list, MediaFileType::Fanart), ["bg-show"]);
    // Season images are asked for separately.
    for season_kind in [
        MediaFileType::SeasonPoster,
        MediaFileType::SeasonThumb,
        MediaFileType::SeasonBanner,
        MediaFileType::SeasonFanart,
    ] {
        assert!(of_kind(&list, season_kind).is_empty(), "{season_kind:?}");
    }
}

#[tokio::test]
async fn season_artwork_is_served_through_the_artwork_provider_trait() {
    let h = Harness::new().await;
    // The registry hands out trait objects, so the per-season call must be on the trait.
    let as_trait: &dyn ArtworkProvider = &h.provider;
    let list = as_trait
        .season_artwork(&show_ids(), 1, &opts())
        .await
        .unwrap();
    assert_eq!(
        of_kind(&list, MediaFileType::SeasonPoster),
        ["s1-en", "s1-de"]
    );
    assert!(
        as_trait
            .season_artwork(&show_ids(), 9, &opts())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn season_artwork_returns_one_seasons_images() {
    let h = Harness::new().await;
    let list = h
        .provider
        .season_artwork(&show_ids(), 1, &opts())
        .await
        .unwrap();
    assert_eq!(
        of_kind(&list, MediaFileType::SeasonPoster),
        ["s1-en", "s1-de"]
    );
    assert_eq!(of_kind(&list, MediaFileType::SeasonThumb), ["s1-en"]);
    assert_eq!(of_kind(&list, MediaFileType::SeasonBanner), ["s1-en"]);
    assert_eq!(of_kind(&list, MediaFileType::SeasonFanart), ["bg-season1"]);
    assert!(list.iter().all(|c| c.kind.is_graphic()));
    assert_eq!(list.len(), 5);

    let specials = h
        .provider
        .season_artwork(&show_ids(), 0, &opts())
        .await
        .unwrap();
    assert_eq!(
        names(&specials),
        [(MediaFileType::SeasonPoster, "specials-en".to_owned())]
    );

    let missing = h
        .provider
        .season_artwork(&show_ids(), 9, &opts())
        .await
        .unwrap();
    assert!(missing.is_empty());
}

#[tokio::test]
async fn images_for_every_season_are_a_separate_list() {
    let h = Harness::new().await;
    let list = h
        .provider
        .all_seasons_artwork(&show_ids(), &opts())
        .await
        .unwrap();
    assert_eq!(
        names(&list),
        [(MediaFileType::SeasonPoster, "all-en".to_owned())]
    );
}

// --- credentials ------------------------------------------------------------------------------

#[tokio::test]
async fn the_key_goes_in_a_header_and_never_in_the_url() {
    let h = Harness::new().await;
    h.provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    let request = &h.server.requests()[0];
    assert_eq!(
        request.headers.get("api-key").map(String::as_str),
        Some(KEY)
    );
    assert!(!request.target.contains(KEY), "{}", request.target);
}

#[tokio::test]
async fn without_a_key_the_provider_names_what_is_missing() {
    let h = Harness::build(None, false).await;
    let err = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap_err();
    let ProviderError::MissingCredentials {
        provider, fields, ..
    } = err
    else {
        panic!("should be missing credentials");
    };
    assert_eq!(provider, "Fanart.tv");
    assert_eq!(fields, ["api_key"]);
    assert!(h.server.requests().is_empty());
    assert!(!h.resolver.status("fanart").unwrap().active);
}

#[tokio::test]
async fn a_rejected_key_is_an_auth_error_without_the_key_in_it() {
    let h = Harness::new().await;
    h.server
        .route("/movies/tt0111222", status(401, "error_invalid_key.json"));
    let err = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
    assert!(err.to_string().contains("Invalid API key"), "{err}");
    assert!(!err.to_string().contains(KEY), "{err}");
}

#[tokio::test]
async fn a_rejected_key_says_so_and_links_to_the_card() {
    let h = Harness::new().await;
    h.server
        .route("/movies/tt0111222", status(401, "error_invalid_key.json"));
    let err = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap_err();
    assert_eq!(err.key_problem_provider(), Some("fanart"), "{err:?}");
    assert!(matches!(err.root(), ProviderError::Auth { .. }));
    let text = err.to_string();
    assert!(text.contains("rejected your key"), "{text}");
    assert!(!text.contains(KEY), "{text}");
    assert_eq!(
        h.resolver.status("fanart").unwrap().state,
        cv_providers::CredentialState::Rejected
    );
    // The user's key is what was sent.
    assert_eq!(
        h.server.requests()[0]
            .headers
            .get("api-key")
            .map(String::as_str),
        Some(KEY)
    );
}

#[tokio::test]
async fn the_rate_limit_is_reported_as_such() {
    let h = Harness::new().await;
    h.server
        .route("/movies/tt0111222", Reply::rate_limited(Some("0")));
    let err = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap_err();
    assert!(
        matches!(err.root(), ProviderError::RateLimited { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn other_failures_are_request_errors() {
    let h = Harness::new().await;
    h.server.route("/movies/tt0111222", Reply::status(503));
    let err = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Request { .. }), "{err:?}");
    assert!(err.to_string().contains("503"), "{err}");

    h.server
        .route("/movies/tt0111222", Reply::ok("<html>not json</html>"));
    let err = h
        .provider
        .artwork(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Request { .. }), "{err:?}");
}

#[tokio::test]
async fn checking_credentials_makes_one_uncached_request_with_the_typed_key() {
    let h = Harness::build(None, true).await;
    let schema = h.provider.credentials().expect("schema");
    let typed =
        ResolvedCredentials::from_user_values(schema, [("api_key", " typed-key-77 ")]).unwrap();
    h.provider.check_credentials(&typed).await.unwrap();
    h.provider.check_credentials(&typed).await.unwrap();
    let requests = h.server.requests();
    assert_eq!(requests.len(), 2, "never served from the cache");
    assert_eq!(requests[0].path(), "/movies/latest");
    assert_eq!(
        requests[0].headers.get("api-key").map(String::as_str),
        Some("typed-key-77")
    );

    h.server
        .route("/movies/latest", status(401, "error_invalid_key.json"));
    let err = h.provider.check_credentials(&typed).await.unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
}

#[tokio::test]
async fn the_provider_is_reachable_through_the_registry() {
    let h = Harness::new().await;
    let mut registry = ProviderRegistry::new();
    registry.register_artwork(Arc::new(h.provider.clone()));
    assert!(registry.get("fanart").is_some());
    assert_eq!(registry.artwork_providers_for(MediaKind::Movie).len(), 1);
    assert_eq!(registry.artwork_providers_for(MediaKind::Show).len(), 1);
    let credentials = h.resolver.resolve("fanart").unwrap();
    registry
        .check_credentials("fanart", "Fanart.tv", &credentials)
        .await
        .unwrap();
}

#[tokio::test]
async fn answers_are_cached_and_a_forced_rescrape_goes_to_the_network() {
    let h = Harness::build(Some(KEY), true).await;
    for _ in 0..2 {
        h.provider
            .artwork(MediaKind::Movie, &movie_ids(), &opts())
            .await
            .unwrap();
    }
    assert_eq!(h.server.requests().len(), 1);
    let forced = RequestOptions {
        bypass_cache: true,
        ..opts()
    };
    h.provider
        .artwork(MediaKind::Movie, &movie_ids(), &forced)
        .await
        .unwrap();
    assert_eq!(h.server.requests().len(), 2);
}

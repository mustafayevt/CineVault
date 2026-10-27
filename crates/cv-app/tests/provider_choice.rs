//! The provider choices of the Providers page reach matching. The default provider
//! per kind, the fallback chain, the universal provider, the Kodi XML scraper add-ons and the
//! anime ID cross-reference. Mock providers only; no network.

#[path = "../../cv-providers/tests/support/fake.rs"]
mod fake;
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cv_app::anime_crosswalk::{AnimeCrosswalk, REFRESH_INTERVAL};
use cv_app::app_settings::SettingsService;
use cv_app::match_service::MatchService;
use cv_app::matching::batch::BatchMode;
use cv_app::notices::NoticeHub;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::provider_setup::register_kodi_addons;
use cv_domain::{Item, ItemId};
use cv_providers::anime_db::{LoadOutcome, Meta};
use cv_providers::model::MediaKind;
use cv_providers::{
    AnimeOfflineDb, CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider,
    ProviderRegistry,
};
use fake::{Fake, ids};
use serde_json::{Value, json};
use support::Lab;

struct Rig {
    lab: Lab,
    service: Arc<MatchService>,
    settings: Arc<SettingsService>,
}

fn rig(registry: ProviderRegistry) -> Rig {
    rig_with(ProviderCredentialsService::with_resolver(
        CredentialResolver::new(Arc::new(MemoryUserCredentials::default())),
        registry,
    ))
}

fn rig_with(providers: ProviderCredentialsService) -> Rig {
    let lab = Lab::new();
    let settings = Arc::new(SettingsService::in_memory(""));
    let service = MatchService::new(
        Arc::clone(&lab.db),
        lab.manager.clone(),
        Arc::new(providers),
        Arc::clone(&settings),
        Arc::new(NoticeHub::new()),
    );
    Rig {
        lab,
        service,
        settings,
    }
}

impl Rig {
    fn set(&self, key: &str, value: Value) {
        self.settings.set(key, value).unwrap();
    }

    async fn match_and_wait(&self, items: &[ItemId]) {
        self.service
            .submit(BatchMode::MatchSelected, Some(items))
            .expect("batch")
            .wait()
            .await;
    }
}

fn movie(title: &str, year: u16, id_pairs: &[(&str, &str)]) -> Item {
    let mut item = Item::movie(title);
    item.core.year = year;
    item.core.ids = ids(id_pairs);
    item
}

fn show(title: &str, year: u16, id_pairs: &[(&str, &str)]) -> Item {
    let mut item = Item::show(title);
    item.core.year = year;
    item.core.ids = ids(id_pairs);
    item
}

fn register(registry: &mut ProviderRegistry, provider: Arc<Fake>) -> Arc<Fake> {
    let shared: Arc<dyn MetadataProvider> = provider.clone();
    registry.register_metadata(shared);
    provider
}

// ---- the provider for each kind ----------------------------------------------------------

/// Two providers that both know a movie and a show; the first registered is the default.
fn two_providers() -> (ProviderRegistry, Arc<Fake>, Arc<Fake>) {
    let titles = |tag: &str| {
        vec![
            movie("Solaris", 1972, &[(tag, "m1")]),
            show("Severance", 2022, &[(tag, "s1")]),
        ]
    };
    let mut registry = ProviderRegistry::new();
    let first = register(
        &mut registry,
        Fake::new("alpha", &["alpha"], titles("alpha")).shared(),
    );
    let second = register(
        &mut registry,
        Fake::new("beta", &["beta"], titles("beta")).shared(),
    );
    (registry, first, second)
}

#[test]
fn without_a_choice_the_first_ready_provider_is_the_default_for_both_kinds() {
    let (registry, _, _) = two_providers();
    let rig = rig(registry);
    let providers = rig.service.match_providers();
    assert_eq!(
        providers.default_for(MediaKind::Movie).unwrap().id(),
        "alpha"
    );
    assert_eq!(
        providers.default_for(MediaKind::Show).unwrap().id(),
        "alpha"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tv_shows_use_the_provider_chosen_for_shows_and_movies_the_one_chosen_for_movies() {
    let (registry, alpha, beta) = two_providers();
    let rig = rig(registry);
    rig.set("providers.metadata-show", json!("beta"));
    let providers = rig.service.match_providers();
    assert_eq!(providers.default_for(MediaKind::Show).unwrap().id(), "beta");
    assert_eq!(
        providers.default_for(MediaKind::Movie).unwrap().id(),
        "alpha"
    );

    let film = rig.lab.movie("Solaris", 1972);
    let series = rig.lab.show("Severance", 2022);
    rig.match_and_wait(&[film, series]).await;
    assert_eq!(rig.lab.item(series).core.last_scraper_id, "beta");
    assert_eq!(rig.lab.item(series).core.ids.get("beta"), Some("s1"));
    assert_eq!(rig.lab.item(film).core.last_scraper_id, "alpha");
    assert!(alpha.calls().iter().all(|c| !c.contains("Severance")));
    assert!(beta.calls().iter().all(|c| !c.contains("Solaris")));

    // The movie choice is separate.
    rig.set("providers.metadata-movie", json!("beta"));
    let again = rig.lab.movie("Solaris", 1972);
    rig.match_and_wait(&[again]).await;
    assert_eq!(rig.lab.item(again).core.last_scraper_id, "beta");
}

#[test]
fn a_choice_that_is_not_ready_or_cannot_handle_the_kind_is_ignored() {
    let mut registry = ProviderRegistry::new();
    register(
        &mut registry,
        Fake::new(
            "alpha",
            &["alpha"],
            vec![movie("Solaris", 1972, &[("alpha", "1")])],
        )
        .shared(),
    );
    // OMDb needs a key and none is saved, so it is not ready.
    register(
        &mut registry,
        Fake::new(
            "omdb",
            &["omdb"],
            vec![movie("Solaris", 1972, &[("omdb", "1")])],
        )
        .shared(),
    );
    register(
        &mut registry,
        Fake::new("filmsonly", &["filmsonly"], Vec::new())
            .movies_only()
            .shared(),
    );
    let rig = rig(registry);
    rig.set("providers.metadata-movie", json!("omdb"));
    rig.set("providers.metadata-show", json!("filmsonly"));
    let providers = rig.service.match_providers();
    assert_eq!(
        providers.default_for(MediaKind::Movie).unwrap().id(),
        "alpha"
    );
    assert_eq!(
        providers.default_for(MediaKind::Show).unwrap().id(),
        "alpha"
    );
    // The page still lists OMDb, marked not ready.
    let catalogue = rig.service.provider_catalogue();
    let omdb = catalogue.providers.iter().find(|p| p.id == "omdb").unwrap();
    assert!(!omdb.ready);
}

// ---- the fallback provider ---------------------------------------------------------------

/// `alpha` knows nothing; `beta` and `gamma` both know "Obscure Film".
fn obscure() -> (ProviderRegistry, Arc<Fake>, Arc<Fake>, Arc<Fake>) {
    let mut registry = ProviderRegistry::new();
    let alpha = register(
        &mut registry,
        Fake::new("alpha", &["alpha"], Vec::new()).shared(),
    );
    let beta = register(
        &mut registry,
        Fake::new(
            "beta",
            &["beta"],
            vec![movie("Obscure Film", 1990, &[("beta", "b1")])],
        )
        .shared(),
    );
    let gamma = register(
        &mut registry,
        Fake::new(
            "gamma",
            &["gamma"],
            vec![movie("Obscure Film", 1990, &[("gamma", "g1")])],
        )
        .shared(),
    );
    (registry, alpha, beta, gamma)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn when_the_provider_finds_nothing_the_fallback_answers_and_is_the_last_scraper() {
    let (registry, alpha, beta, _) = obscure();
    let rig = rig(registry);
    rig.set("providers.fallback-enabled", json!(true));
    rig.set("providers.fallback-order", json!(["beta"]));
    let item = rig.lab.movie("Obscure Film", 1990);
    rig.match_and_wait(&[item]).await;

    let stored = rig.lab.item(item);
    assert_eq!(stored.core.last_scraper_id, "beta");
    assert_eq!(stored.core.ids.get("beta"), Some("b1"));
    assert!(alpha.calls().iter().any(|c| c.starts_with("search")));
    assert!(beta.calls().iter().any(|c| c.starts_with("details")));
    assert_eq!(rig.service.pending_count().unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_fallback_off_the_title_waits_for_review() {
    let (registry, _, beta, _) = obscure();
    let rig = rig(registry);
    // The order is stored but fallback is not enabled.
    rig.set("providers.fallback-order", json!(["beta"]));
    let item = rig.lab.movie("Obscure Film", 1990);
    rig.match_and_wait(&[item]).await;

    assert_eq!(rig.lab.item(item).core.last_scraper_id, "");
    assert_eq!(rig.service.pending_count().unwrap(), 1);
    assert!(beta.calls().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_fallback_providers_are_tried_in_the_users_order() {
    for (order, winner) in [(["beta", "gamma"], "beta"), (["gamma", "beta"], "gamma")] {
        let (registry, _, beta, gamma) = obscure();
        let rig = rig(registry);
        rig.set("providers.fallback-enabled", json!(true));
        rig.set("providers.fallback-order", json!(order));
        let item = rig.lab.movie("Obscure Film", 1990);
        rig.match_and_wait(&[item]).await;
        assert_eq!(rig.lab.item(item).core.last_scraper_id, winner);
        // The first that answers stops the chain.
        let loser = if winner == "beta" { &gamma } else { &beta };
        assert!(loser.calls().is_empty(), "{order:?}: {:?}", loser.calls());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_stored_order_every_ready_provider_is_a_fallback_in_the_default_order() {
    let (registry, _, _, _) = obscure();
    let rig = rig(registry);
    rig.set("providers.fallback-enabled", json!(true));
    // The provider a title runs with is skipped when its own chain comes round to it.
    let chain = rig.service.match_providers();
    assert_eq!(
        chain.fallback().ids().collect::<Vec<_>>(),
        ["alpha", "beta", "gamma"]
    );
    let item = rig.lab.movie("Obscure Film", 1990);
    rig.match_and_wait(&[item]).await;
    assert_eq!(rig.lab.item(item).core.last_scraper_id, "beta");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_review_search_uses_the_fallback_when_the_provider_has_nothing() {
    let (registry, _, _, _) = obscure();
    let rig = rig(registry);
    let item = rig.lab.movie("Obscure Film", 1990);
    rig.match_and_wait(&[item]).await;
    let row = rig.service.pending(10).unwrap()[0].review_id;
    assert!(
        rig.service
            .search_review(row, "Obscure Film")
            .await
            .unwrap()
            .is_empty()
    );

    rig.set("providers.fallback-enabled", json!(true));
    rig.set("providers.fallback-order", json!(["gamma"]));
    let found = rig
        .service
        .search_review(row, "Obscure Film")
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].provider, "gamma");
}

// ---- the universal provider --------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_universal_provider_is_selectable_and_takes_each_field_from_its_source() {
    let mut registry = ProviderRegistry::new();
    let mut from_alpha = movie("Solaris", 1972, &[("alpha", "a1"), ("imdb", "tt0069293")]);
    from_alpha.core.plot = "Alpha plot".into();
    let mut from_beta = movie("Solaris (restored)", 1972, &[("imdb", "tt0069293")]);
    from_beta.core.plot = "Beta plot".into();
    register(
        &mut registry,
        Fake::new("alpha", &["alpha", "imdb"], vec![from_alpha]).shared(),
    );
    register(
        &mut registry,
        Fake::new("beta", &["imdb"], vec![from_beta]).shared(),
    );
    let rig = rig(registry);

    // It is listed after the real providers, on the page and in the scrape panel.
    let catalogue = rig.service.provider_catalogue();
    assert_eq!(catalogue.providers.last().unwrap().id, "universal");
    assert!(catalogue.universal_fields.iter().any(|f| f == "plot"));
    assert!(catalogue.universal_fields.iter().all(|f| f != "poster"));
    let panel = rig.service.scrape_providers(cv_domain::ItemKind::Movie);
    assert_eq!(panel.last().unwrap().key, "universal");
    // It is not the default until chosen.
    assert_eq!(
        rig.service
            .match_providers()
            .default_for(MediaKind::Movie)
            .unwrap()
            .id(),
        "alpha"
    );

    rig.set("providers.metadata-movie", json!("universal"));
    rig.set(
        "providers.universal",
        json!({"search": "alpha", "fields": {"title": "alpha", "plot": "beta"}}),
    );
    let item = rig.lab.movie("Solaris", 1972);
    rig.match_and_wait(&[item]).await;
    let stored = rig.lab.item(item);
    assert_eq!(stored.core.last_scraper_id, "universal");
    assert_eq!(stored.core.title, "Solaris");
    assert_eq!(stored.core.plot, "Beta plot");
}

// ---- Kodi XML scraper add-ons ------------------------------------------------------------

fn kodi_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../cv-providers/tests/fixtures/kodi/addons")
}

fn kodi_service(extra: Option<Arc<Fake>>) -> ProviderCredentialsService {
    let http = HttpClient::builder().build().expect("http client");
    let mut registry = ProviderRegistry::new();
    if let Some(extra) = extra {
        register(&mut registry, extra);
    }
    let kodi = register_kodi_addons(&mut registry, &http, &[kodi_fixtures()]);
    ProviderCredentialsService::with_resolver(
        CredentialResolver::new(Arc::new(MemoryUserCredentials::default())),
        registry,
    )
    .with_kodi(kodi)
}

#[test]
fn installed_xml_scrapers_are_offered_and_python_ones_are_listed_with_the_reason() {
    let alpha = Fake::new("alpha", &["alpha"], Vec::new()).shared();
    let rig = rig_with(kodi_service(Some(alpha)));
    let catalogue = rig.service.provider_catalogue();

    let kodi: Vec<_> = catalogue.providers.iter().filter(|p| p.kodi).collect();
    assert!(!kodi.is_empty());
    assert!(
        kodi.iter()
            .any(|p| p.id == "metadata.sample.films" && p.movies && p.ready)
    );
    assert!(kodi.iter().all(|p| !p.can_fallback));
    // The built-in provider comes first and stays the default.
    assert_eq!(catalogue.providers[0].id, "alpha");
    assert_eq!(
        rig.service
            .match_providers()
            .default_for(MediaKind::Movie)
            .unwrap()
            .id(),
        "alpha"
    );
    // The Python add-on cannot run: it is listed with its reason.
    let python = catalogue
        .unsupported_addons
        .iter()
        .find(|a| a.addon_id == "metadata.sample.python")
        .expect("the python add-on is listed");
    assert!(!python.reason.is_empty());
    assert!(
        catalogue
            .providers
            .iter()
            .all(|p| p.id != "metadata.sample.python")
    );
    // A supported one can be chosen as the default for its kind.
    rig.set("providers.metadata-movie", json!("metadata.sample.films"));
    assert_eq!(
        rig.service
            .match_providers()
            .default_for(MediaKind::Movie)
            .unwrap()
            .id(),
        "metadata.sample.films"
    );
}

#[test]
fn kodi_add_ons_never_join_the_fallback_chain() {
    let alpha = Fake::new("alpha", &["alpha"], Vec::new()).shared();
    let rig = rig_with(kodi_service(Some(alpha)));
    rig.set("providers.fallback-enabled", json!(true));
    rig.set(
        "providers.fallback-order",
        json!(["metadata.sample.films", "alpha"]),
    );
    let chain = rig.service.match_providers();
    assert_eq!(chain.fallback().ids().collect::<Vec<_>>(), ["alpha"]);
}

// ---- the anime ID cross-reference --------------------------------------------------------

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

/// A cache folder that already holds a crosswalk checked `age_days` ago. The download address
/// points at a closed port, so a request would fail.
fn cached_crosswalk(dir: &Path, age_days: u64) -> AnimeCrosswalk {
    let url = "http://127.0.0.1:9/anime-offline-database.jsonl";
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("crosswalk.json"),
        r#"[{"anidb": 4563, "myanimelist": 1535, "anilist": 1535}]"#,
    )
    .unwrap();
    let checked = now_ms() - age_days * 24 * 60 * 60 * 1000;
    let meta = Meta {
        url: url.to_owned(),
        etag: None,
        sha256: String::new(),
        checked_at_ms: checked,
        downloaded_at_ms: checked,
        rows: 1,
    };
    std::fs::write(dir.join("meta.json"), serde_json::to_string(&meta).unwrap()).unwrap();
    let http = HttpClient::builder().build().unwrap();
    AnimeCrosswalk::new(AnimeOfflineDb::with_http(http, dir, url))
}

#[test]
fn the_crosswalk_refreshes_weekly() {
    assert_eq!(REFRESH_INTERVAL.as_secs(), 7 * 24 * 60 * 60);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copy_younger_than_a_week_is_used_without_a_request_and_an_older_one_is_refreshed() {
    let dir = tempfile::tempdir().unwrap();
    let fresh = cached_crosswalk(&dir.path().join("fresh"), 6);
    let (rows, outcome) = fresh.load().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(outcome, LoadOutcome::Fresh);

    // Eight days old: the server is asked (it cannot be reached here) and the old copy is kept.
    let stale = cached_crosswalk(&dir.path().join("stale"), 8);
    let (rows, outcome) = stale.load().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(
        matches!(outcome, LoadOutcome::KeptAfterError(_)),
        "{outcome:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_title_that_is_not_an_anime_never_loads_the_crosswalk() {
    let dir = tempfile::tempdir().unwrap();
    // No copy on disk and an unreachable address: loading would fail and leave files behind.
    let http = HttpClient::builder().build().unwrap();
    let crosswalk = AnimeCrosswalk::new(AnimeOfflineDb::with_http(
        http,
        dir.path().join("none"),
        "http://127.0.0.1:9/x.jsonl",
    ));
    assert!(
        crosswalk
            .for_title(&ids(&[("tmdb", "603"), ("imdb", "tt0133093")]))
            .await
            .is_none()
    );
    assert!(crosswalk.loaded().is_none());
    assert!(!dir.path().join("none").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_anime_scrape_loads_the_crosswalk_and_completes_the_other_anime_ids() {
    let mut registry = ProviderRegistry::new();
    register(
        &mut registry,
        Fake::new(
            "animefake",
            &["anidb"],
            vec![show("Death Note", 2006, &[("anidb", "4563")])],
        )
        .shared(),
    );
    let rig = rig(registry);
    let dir = tempfile::tempdir().unwrap();
    let crosswalk = cached_crosswalk(dir.path(), 1);
    rig.service.set_anime(crosswalk.clone());
    assert!(crosswalk.loaded().is_none(), "nothing is loaded at startup");

    let anime = rig.lab.show("Death Note", 2006);
    rig.match_and_wait(&[anime]).await;
    let stored = rig.lab.item(anime);
    assert_eq!(stored.core.ids.get("anidb"), Some("4563"));
    assert_eq!(stored.core.ids.get("myanimelist"), Some("1535"));
    assert_eq!(stored.core.ids.get("anilist"), Some("1535"));
    assert!(crosswalk.loaded().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_show_without_an_anime_id_is_scraped_with_the_crosswalk_left_alone() {
    let mut registry = ProviderRegistry::new();
    register(
        &mut registry,
        Fake::new(
            "alpha",
            &["alpha"],
            vec![show("Severance", 2022, &[("alpha", "s1")])],
        )
        .shared(),
    );
    let rig = rig(registry);
    let dir = tempfile::tempdir().unwrap();
    let crosswalk = cached_crosswalk(dir.path(), 1);
    rig.service.set_anime(crosswalk.clone());

    let series = rig.lab.show("Severance", 2022);
    rig.match_and_wait(&[series]).await;
    assert_eq!(rig.lab.item(series).core.last_scraper_id, "alpha");
    assert!(crosswalk.loaded().is_none());
}

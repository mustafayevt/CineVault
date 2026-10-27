//! The universal provider combines fields from several providers, resolving IDs
//! across them. The providers are in-memory fakes; no test touches the network.

#[path = "support/fake.rs"]
mod fake;

use std::sync::Arc;

use cv_domain::{Item, ItemDetail, MetaField, Rating, id_key};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::{
    MetadataProvider, Provider, ProviderError, UNIVERSAL_ID, UniversalConfig, UniversalProvider,
};
use fake::{Fake, ids};

fn opts() -> RequestOptions {
    RequestOptions::default()
}

/// What TMDB knows about the film: its title, its own plot, a collection, and the IMDb ID.
fn tmdb_matrix() -> Item {
    let mut item = Item::movie("The Matrix");
    item.core.year = 1999;
    item.core.plot = "TMDB plot".into();
    item.core.ids = ids(&[
        ("tmdb", "603"),
        ("imdb", "tt0133093"),
        (id_key::TMDB_SET, "2344"),
    ]);
    item.core
        .ratings
        .insert(Rating::new("themoviedb", 8.2, 20_000, 10));
    if let ItemDetail::Movie(m) = &mut item.detail {
        m.tagline = "Welcome to the Real World".into();
    }
    item
}

/// IMDb answers only to its own ID. Its title is the local release name, its plot is better.
fn imdb_matrix() -> Item {
    let mut item = Item::movie("Matrix, The");
    item.core.year = 1999;
    item.core.plot = "IMDb plot".into();
    item.core.ids = ids(&[("imdb", "tt0133093")]);
    item.core
        .ratings
        .insert(Rating::new("imdb", 8.7, 2_000_000, 10));
    item
}

fn omdb_matrix() -> Item {
    let mut item = Item::movie("The Matrix (OMDb)");
    item.core.plot = "OMDb plot".into();
    item.core.ids = ids(&[("imdb", "tt0133093")]);
    item.core
        .ratings
        .insert(Rating::new("metacritic", 73.0, 40, 100));
    item
}

fn config(search: &str, fields: &[(MetaField, &str)]) -> UniversalConfig {
    UniversalConfig {
        search: search.to_owned(),
        fields: fields.iter().map(|(f, p)| (*f, (*p).to_owned())).collect(),
        ..UniversalConfig::default()
    }
}

fn shared(fakes: &[&Arc<Fake>]) -> Vec<Arc<dyn MetadataProvider>> {
    fakes
        .iter()
        .map(|f| Arc::clone(f) as Arc<dyn MetadataProvider>)
        .collect()
}

#[tokio::test]
async fn mixed_sources_take_the_title_from_tmdb_and_the_plot_from_imdb() {
    let tmdb = Fake::new("tmdb", &["tmdb", "imdb"], vec![tmdb_matrix()]).shared();
    let imdb = Fake::new("imdb", &["imdb"], vec![imdb_matrix()]).shared();
    let universal = UniversalProvider::new(
        config(
            "tmdb",
            &[(MetaField::Title, "tmdb"), (MetaField::Plot, "imdb")],
        ),
        shared(&[&tmdb, &imdb]),
    );

    // The library item only has its TMDB ID: IMDb's ID comes from TMDB's answer.
    let scrape = universal
        .scrape(MediaKind::Movie, &ids(&[("tmdb", "603")]), &opts())
        .await
        .unwrap();

    assert_eq!(scrape.item.core.title, "The Matrix");
    assert_eq!(scrape.item.core.plot, "IMDb plot");
    assert_eq!(scrape.sources[&MetaField::Title], "tmdb");
    assert_eq!(scrape.sources[&MetaField::Plot], "imdb");
    // Fields with no choice come from the search provider.
    assert_eq!(scrape.item.core.year, 1999);
    assert_eq!(scrape.sources[&MetaField::Year], "tmdb");
    assert_eq!(scrape.item.core.last_scraper_id, UNIVERSAL_ID);
    assert!(scrape.failures.is_empty());

    // IMDb was asked once, with the ID TMDB reported.
    assert_eq!(imdb.calls(), ["details imdb=tt0133093,tmdb=603"]);
    // The IDs of both answers are united.
    assert_eq!(scrape.item.core.ids.get("tmdb"), Some("603"));
    assert_eq!(scrape.item.core.ids.get("imdb"), Some("tt0133093"));
}

#[tokio::test]
async fn the_trait_details_returns_the_same_combined_item() {
    let tmdb = Fake::new("tmdb", &["tmdb", "imdb"], vec![tmdb_matrix()]).shared();
    let imdb = Fake::new("imdb", &["imdb"], vec![imdb_matrix()]).shared();
    let universal = UniversalProvider::new(
        config("tmdb", &[(MetaField::Plot, "imdb")]),
        shared(&[&tmdb, &imdb]),
    );
    assert_eq!(universal.id(), UNIVERSAL_ID);
    let item = universal
        .details(MediaKind::Movie, &ids(&[("imdb", "tt0133093")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "The Matrix");
    assert_eq!(item.core.plot, "IMDb plot");
}

#[tokio::test]
async fn ratings_and_the_collection_come_from_the_providers_chosen_for_them() {
    let tmdb = Fake::new("tmdb", &["tmdb", "imdb"], vec![tmdb_matrix()]).shared();
    let imdb = Fake::new("imdb", &["imdb"], vec![imdb_matrix()]).shared();
    let omdb = Fake::new("omdb", &["imdb"], vec![omdb_matrix()]).shared();
    let universal = UniversalProvider::new(
        config(
            "imdb",
            &[
                (MetaField::Title, "tmdb"),
                (MetaField::Rating, "omdb"),
                (MetaField::Collection, "tmdb"),
            ],
        ),
        shared(&[&imdb, &omdb, &tmdb]),
    );

    // Only the IMDb ID is known; TMDB resolves it directly.
    let scrape = universal
        .scrape(MediaKind::Movie, &ids(&[("imdb", "tt0133093")]), &opts())
        .await
        .unwrap();
    let item = scrape.item;
    assert_eq!(item.core.title, "The Matrix");
    assert_eq!(
        item.core.plot, "IMDb plot",
        "the search provider supplies the rest"
    );
    let ratings: Vec<_> = item.core.ratings.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ratings, ["metacritic"], "only OMDb was chosen for ratings");
    assert_eq!(item.core.ids.get(id_key::TMDB_SET), Some("2344"));
    assert_eq!(scrape.sources[&MetaField::Collection], "tmdb");
    assert_eq!(item.core.ids.get("tmdb"), Some("603"));
}

#[tokio::test]
async fn a_provider_that_cannot_use_any_known_id_is_bridged_through_another() {
    // Only TheTVDB's ID is known. IMDb needs an IMDb ID, which TMDB knows from the TVDB ID.
    let mut show_tmdb = Item::show("Show");
    show_tmdb.core.ids = ids(&[("tmdb", "7"), ("tvdb", "70"), ("imdb", "tt77")]);
    show_tmdb.core.plot = "TMDB show plot".into();
    let mut show_tvdb = Item::show("Show");
    show_tvdb.core.ids = ids(&[("tvdb", "70")]);
    show_tvdb.core.plot = "TVDB show plot".into();
    let mut show_imdb = Item::show("Show");
    show_imdb.core.ids = ids(&[("imdb", "tt77")]);
    show_imdb.core.plot = "IMDb show plot".into();

    let tvdb = Fake::new("tvdb", &["tvdb"], vec![show_tvdb]).shared();
    let imdb = Fake::new("imdb", &["imdb"], vec![show_imdb]).shared();
    // The bridge provider is not a source of any field, but it knows the IDs.
    let tmdb = Fake::new("tmdb", &["tmdb", "tvdb"], vec![show_tmdb]).shared();
    let universal = UniversalProvider::new(
        config("tvdb", &[(MetaField::Plot, "imdb")]),
        shared(&[&tvdb, &imdb, &tmdb]),
    );

    let scrape = universal
        .scrape(MediaKind::Show, &ids(&[("tvdb", "70")]), &opts())
        .await
        .unwrap();
    assert_eq!(scrape.item.core.plot, "IMDb show plot");
    assert_eq!(scrape.item.core.title, "Show");
    assert_eq!(scrape.item.core.ids.get("imdb"), Some("tt77"));
    assert_eq!(scrape.item.core.ids.get("tmdb"), Some("7"));
    assert_eq!(imdb.calls().len(), 1);
}

#[tokio::test]
async fn an_empty_field_falls_back_to_the_next_provider_in_order() {
    let mut sparse = tmdb_matrix();
    sparse.core.plot.clear();
    let tmdb = Fake::new("tmdb", &["tmdb", "imdb"], vec![sparse]).shared();
    let imdb = Fake::new("imdb", &["imdb"], vec![imdb_matrix()]).shared();
    let omdb = Fake::new("omdb", &["imdb"], vec![omdb_matrix()]).shared();
    let mut cfg = config("tmdb", &[]);
    cfg.fallbacks = vec!["omdb".into(), "imdb".into()];
    let universal = UniversalProvider::new(cfg, shared(&[&tmdb, &imdb, &omdb]));

    let scrape = universal
        .scrape(MediaKind::Movie, &ids(&[("tmdb", "603")]), &opts())
        .await
        .unwrap();
    // TMDB has no plot; OMDb is first in the fallback order, so IMDb's plot is not used.
    assert_eq!(scrape.item.core.plot, "OMDb plot");
    assert_eq!(scrape.sources[&MetaField::Plot], "omdb");
    assert_eq!(scrape.item.core.title, "The Matrix");
}

#[tokio::test]
async fn an_inactive_provider_is_skipped_and_the_rest_still_fills_the_item() {
    let tmdb = Fake::new("tmdb", &["tmdb", "imdb"], vec![tmdb_matrix()]).shared();
    let omdb = Fake::new("omdb", &["imdb"], vec![omdb_matrix()])
        .failing(ProviderError::MissingCredentials {
            provider: "OMDb".into(),
            fields: vec!["apikey".into()],
            registration_url: "https://example.invalid/key".into(),
        })
        .shared();
    let universal = UniversalProvider::new(
        config("tmdb", &[(MetaField::Rating, "omdb")]),
        shared(&[&tmdb, &omdb]),
    );
    let scrape = universal
        .scrape(MediaKind::Movie, &ids(&[("tmdb", "603")]), &opts())
        .await
        .unwrap();
    assert_eq!(scrape.item.core.title, "The Matrix");
    assert_eq!(scrape.failures.len(), 1);
    assert_eq!(scrape.failures[0].0, "omdb");
    assert!(matches!(
        scrape.failures[0].1,
        ProviderError::MissingCredentials { .. }
    ));
    assert!(
        scrape.item.core.ratings.is_empty(),
        "OMDb was the only ratings source"
    );
}

#[tokio::test]
async fn it_fails_only_when_no_provider_answers() {
    let tmdb = Fake::new("tmdb", &["tmdb"], vec![]).shared();
    let universal = UniversalProvider::new(config("tmdb", &[]), shared(&[&tmdb]));
    let error = universal
        .scrape(MediaKind::Movie, &ids(&[("tmdb", "1")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::NotFound(_)));

    let down = Fake::new("tmdb", &["tmdb"], vec![])
        .failing(ProviderError::Request {
            provider: "tmdb".into(),
            message: "down".into(),
        })
        .shared();
    let universal = UniversalProvider::new(config("tmdb", &[]), shared(&[&down]));
    let error = universal
        .scrape(MediaKind::Movie, &ids(&[("tmdb", "1")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Request { .. }));
}

#[tokio::test]
async fn search_goes_to_the_search_provider_and_hits_are_labelled_universal() {
    let tmdb = Fake::new("tmdb", &["tmdb", "imdb"], vec![tmdb_matrix()]).shared();
    let imdb = Fake::new("imdb", &["imdb"], vec![imdb_matrix()]).shared();
    let universal = UniversalProvider::new(config("tmdb", &[]), shared(&[&imdb, &tmdb]));
    let query = SearchQuery {
        kind: MediaKind::Movie,
        title: "matrix".into(),
        year: None,
    };
    let hits = universal.search(&query, &opts()).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].provider, UNIVERSAL_ID);
    assert_eq!(hits[0].ids.get("tmdb"), Some("603"));
    assert!(imdb.calls().is_empty());

    // A lookup by an ID the search provider cannot use is bridged first.
    let universal = UniversalProvider::new(config("imdb", &[]), shared(&[&tmdb, &imdb]));
    let hit = universal
        .lookup(MediaKind::Movie, &ids(&[("tmdb", "603")]), &opts())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hit.ids.get("imdb"), Some("tt0133093"));
    let caps = universal.capabilities();
    assert!(caps.movies && caps.lookup_ids.contains(&"tmdb"));
}

#[test]
fn the_configuration_round_trips_and_unknown_sources_are_listed() {
    let cfg = UniversalConfig {
        search: "tmdb".into(),
        fields: [(MetaField::Plot, "imdb".to_owned())].into_iter().collect(),
        fallbacks: vec!["gone".into()],
        episodes: String::new(),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert_eq!(
        json,
        r#"{"search":"tmdb","fields":{"plot":"imdb"},"fallbacks":["gone"],"episodes":""}"#
    );
    assert_eq!(serde_json::from_str::<UniversalConfig>(&json).unwrap(), cfg);
    assert_eq!(
        serde_json::from_str::<UniversalConfig>("{}").unwrap(),
        UniversalConfig::default()
    );

    let tmdb = Fake::new("tmdb", &["tmdb"], vec![]).shared();
    let universal = UniversalProvider::new(cfg, shared(&[&tmdb]));
    assert_eq!(universal.unknown_sources(), ["imdb", "gone"]);
}

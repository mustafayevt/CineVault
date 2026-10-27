//! The ordered fallback-provider chain.

#[path = "support/fake.rs"]
mod fake;

use std::sync::Arc;

use cv_domain::Item;
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::{FallbackChain, MetadataProvider, ProviderError};
use fake::{Fake, ids};

fn query(title: &str) -> SearchQuery {
    SearchQuery {
        kind: MediaKind::Movie,
        title: title.to_owned(),
        year: None,
    }
}

fn titled(title: &str, key: &str, value: &str) -> Item {
    let mut item = Item::movie(title);
    item.core.ids = ids(&[(key, value)]);
    item.core.plot = format!("{title} plot");
    item
}

fn dyn_of(fake: &Arc<Fake>) -> Arc<dyn MetadataProvider> {
    Arc::clone(fake) as Arc<dyn MetadataProvider>
}

fn inactive() -> ProviderError {
    ProviderError::MissingCredentials {
        provider: "x".into(),
        fields: vec!["apikey".into()],
        registration_url: "https://example.invalid".into(),
    }
}

fn outage(provider: &str) -> ProviderError {
    ProviderError::Request {
        provider: provider.into(),
        message: "down".into(),
    }
}

#[tokio::test]
async fn an_obscure_title_is_found_by_the_next_provider_which_becomes_the_last_scraper() {
    let tmdb = Fake::new("tmdb", &["tmdb"], vec![]).shared();
    let ofdb = Fake::new(
        "ofdb",
        &["ofdb"],
        vec![titled("Tatort Spezial", "ofdb", "99")],
    )
    .shared();
    let chain = FallbackChain::new([dyn_of(&ofdb)]);

    let found = chain
        .search(&dyn_of(&tmdb), &query("tatort"), &RequestOptions::default())
        .await
        .unwrap();
    assert!(found.fell_back);
    assert_eq!(found.provider, "ofdb");
    assert_eq!(found.hits[0].ids.get("ofdb"), Some("99"));

    let scrape = chain
        .scrape_first(&dyn_of(&tmdb), &query("tatort"), &RequestOptions::default())
        .await
        .unwrap();
    assert_eq!(scrape.item.core.title, "Tatort Spezial");
    assert_eq!(scrape.item.core.last_scraper_id, "ofdb");
}

#[tokio::test]
async fn the_chain_stops_at_the_first_provider_with_results() {
    let primary = Fake::new("tmdb", &["tmdb"], vec![]).shared();
    let skipped_empty = Fake::new("a", &["a"], vec![]).shared();
    let winner = Fake::new("b", &["b"], vec![titled("Obscure", "b", "1")]).shared();
    let never = Fake::new("c", &["c"], vec![titled("Obscure", "c", "2")]).shared();
    let chain = FallbackChain::new([dyn_of(&skipped_empty), dyn_of(&winner), dyn_of(&never)]);

    let found = chain
        .search(
            &dyn_of(&primary),
            &query("obscure"),
            &RequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(found.provider, "b");
    assert_eq!(skipped_empty.calls().len(), 1);
    assert!(never.calls().is_empty());
}

#[tokio::test]
async fn the_primary_alone_decides_when_it_has_results_or_the_chain_is_off() {
    let primary = Fake::new("tmdb", &["tmdb"], vec![titled("Obscure", "tmdb", "1")]).shared();
    let other = Fake::new("b", &["b"], vec![titled("Obscure", "b", "1")]).shared();
    let chain = FallbackChain::new([dyn_of(&other)]);
    let found = chain
        .search(
            &dyn_of(&primary),
            &query("obscure"),
            &RequestOptions::default(),
        )
        .await
        .unwrap();
    assert!(!found.fell_back);
    assert_eq!(found.provider, "tmdb");
    assert!(other.calls().is_empty());

    let empty_primary = Fake::new("tmdb", &["tmdb"], vec![]).shared();
    let off = FallbackChain::disabled();
    assert!(off.is_empty());
    let found = off
        .search(
            &dyn_of(&empty_primary),
            &query("obscure"),
            &RequestOptions::default(),
        )
        .await
        .unwrap();
    assert!(found.hits.is_empty() && !found.fell_back);
    assert!(other.calls().is_empty());
}

#[tokio::test]
async fn an_inactive_primary_falls_back_but_an_outage_does_not() {
    let inactive_primary = Fake::new("omdb", &["imdb"], vec![])
        .failing(inactive())
        .shared();
    let backup = Fake::new("tmdb", &["tmdb"], vec![titled("Film", "tmdb", "5")]).shared();
    let chain = FallbackChain::new([dyn_of(&backup)]);
    let found = chain
        .search(
            &dyn_of(&inactive_primary),
            &query("film"),
            &RequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(found.provider, "tmdb");
    assert_eq!(found.skipped.len(), 1);

    let down = Fake::new("tmdb", &["tmdb"], vec![])
        .failing(outage("tmdb"))
        .shared();
    let error = chain
        .search(&dyn_of(&down), &query("film"), &RequestOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Request { .. }));
    assert_eq!(
        backup.calls().len(),
        1,
        "the outage did not start the chain"
    );
}

#[tokio::test]
async fn chain_providers_that_are_inactive_or_cannot_handle_the_kind_are_skipped() {
    let primary = Fake::new("tmdb", &["tmdb"], vec![]).shared();
    let no_key = Fake::new("omdb", &["imdb"], vec![])
        .failing(inactive())
        .shared();
    let broken = Fake::new("trakt", &["trakt"], vec![])
        .failing(outage("trakt"))
        .shared();
    let movies_only = Fake::new("ofdb", &["ofdb"], vec![titled("Show", "ofdb", "1")])
        .movies_only()
        .shared();
    let good = Fake::new("tvmaze", &["tvmaze"], vec![]).shared();
    let chain = FallbackChain::new([
        dyn_of(&no_key),
        dyn_of(&broken),
        dyn_of(&movies_only),
        dyn_of(&good),
    ]);

    let show_query = SearchQuery {
        kind: MediaKind::Show,
        title: "show".into(),
        year: None,
    };
    let found = chain
        .search(&dyn_of(&primary), &show_query, &RequestOptions::default())
        .await
        .unwrap();
    assert!(found.hits.is_empty());
    assert!(movies_only.calls().is_empty());
    let skipped: Vec<_> = found.skipped.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(skipped, ["omdb", "trakt"]);
    assert_eq!(good.calls().len(), 1, "an empty answer is not a skip");
}

#[tokio::test]
async fn rows_without_a_title_or_an_id_do_not_count_as_results() {
    let mut untitled = Item::movie("");
    untitled.core.ids = ids(&[("x", "1")]);
    let idless = Item::movie("Idless");
    let primary = Fake::new("tmdb", &["tmdb"], vec![untitled, idless]).shared();
    let backup = Fake::new("b", &["b"], vec![titled("Idless", "b", "9")]).shared();
    let chain = FallbackChain::new([dyn_of(&backup)]);
    let found = chain
        .search(&dyn_of(&primary), &query(""), &RequestOptions::default())
        .await
        .unwrap();
    assert_eq!(found.provider, "b");
}

#[test]
fn the_chain_keeps_its_order_and_leaves_out_universal_and_add_on_providers() {
    let a = Fake::new("a", &["a"], vec![]).shared();
    let kodi = Fake::new("kodi.tmdb", &["a"], vec![]).shared();
    let universal = Fake::new("universal", &["a"], vec![]).shared();
    let b = Fake::new("b", &["b"], vec![]).shared();
    let again = Fake::new("a", &["a"], vec![]).shared();
    let chain = FallbackChain::new([
        dyn_of(&b),
        dyn_of(&kodi),
        dyn_of(&a),
        dyn_of(&universal),
        dyn_of(&again),
    ]);
    assert_eq!(chain.ids().collect::<Vec<_>>(), ["b", "a"]);
}

#[tokio::test]
async fn search_after_tries_only_the_chain_and_skips_the_provider_that_just_failed_to_find() {
    let primary = Fake::new("tmdb", &["tmdb"], vec![]).shared();
    let empty = Fake::new("omdb", &["omdb"], vec![]).shared();
    let ofdb = Fake::new(
        "ofdb",
        &["ofdb"],
        vec![titled("Tatort Spezial", "ofdb", "99")],
    )
    .shared();
    let chain = FallbackChain::new([dyn_of(&primary), dyn_of(&empty), dyn_of(&ofdb)]);

    let (winner, found) = chain
        .search_after("tmdb", &query("tatort"), &RequestOptions::default())
        .await
        .expect("the chain finds it");
    assert_eq!(winner.id(), "ofdb");
    assert!(found.fell_back);
    assert_eq!(found.hits[0].ids.get("ofdb"), Some("99"));
    // The primary is not asked again; the empty provider was.
    assert!(primary.calls().is_empty());
    assert_eq!(empty.calls().len(), 1);

    // Nothing anywhere, and an empty chain, answer `None`.
    assert!(
        chain
            .search_after("tmdb", &query("no such title"), &RequestOptions::default())
            .await
            .is_none()
    );
    assert!(
        FallbackChain::disabled()
            .search_after("tmdb", &query("tatort"), &RequestOptions::default())
            .await
            .is_none()
    );
}

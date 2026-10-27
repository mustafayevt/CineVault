//! Opt-in smoke test against the real Trakt API.
//!
//! It runs only when `CV_TRAKT_CLIENT_ID` is set to a Trakt application's client ID (metadata
//! needs no secret and no sign-in); without it the test prints why it was skipped and passes.
//! It makes a handful of requests: a credential check, a search, an ID lookup, a movie's
//! details and a show's episodes.
//!
//! ```text
//! CV_TRAKT_CLIENT_ID=... cargo test -p cv-providers --test trakt_live -- --nocapture
//! ```

use std::sync::Arc;

use cv_domain::{EpisodeGroup, ItemDetail, ProviderIds, id_key};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::trakt::{RATING_ID, TraktProvider};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider, Provider,
    RatingProvider, catalogue,
};

const ID_VARIABLE: &str = "CV_TRAKT_CLIENT_ID";

#[tokio::test]
async fn live_smoke() {
    let Some(client_id) = std::env::var(ID_VARIABLE)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
    else {
        eprintln!("skipped: set {ID_VARIABLE} to run the live Trakt smoke test");
        return;
    };
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    resolver
        .set_user(catalogue::key::TRAKT, "client_id", &client_id)
        .unwrap();
    let provider = TraktProvider::new(HttpClient::builder().build().unwrap(), resolver.clone());
    let options = RequestOptions::default();

    provider
        .check_credentials(&resolver.resolve(catalogue::key::TRAKT).unwrap())
        .await
        .expect("Trakt accepted the client ID");

    let hits = provider
        .search(
            &SearchQuery {
                kind: MediaKind::Movie,
                title: "Heat".to_owned(),
                year: Some(1995),
            },
            &options,
        )
        .await
        .unwrap();
    assert!(
        hits.iter()
            .any(|h| h.ids.get(id_key::IMDB) == Some("tt0113277")),
        "{hits:#?}"
    );

    let mut by_imdb = ProviderIds::new();
    by_imdb.set(id_key::IMDB, "tt0113277");
    let hit = provider
        .lookup(MediaKind::Movie, &by_imdb, &options)
        .await
        .unwrap()
        .expect("Heat by IMDb ID");
    let item = provider
        .details(MediaKind::Movie, &hit.ids, &options)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
    assert!(!item.core.credits.is_empty());
    let ratings = provider
        .ratings(MediaKind::Movie, &hit.ids, &options)
        .await
        .unwrap();
    assert!(ratings.get(RATING_ID).is_some());

    let mut show = ProviderIds::new();
    show.set(id_key::TVDB, "81189");
    let show_hit = provider
        .lookup(MediaKind::Show, &show, &options)
        .await
        .unwrap()
        .expect("Breaking Bad by TVDB ID");
    let episodes = provider
        .episodes(&show_hit.ids, &EpisodeGroup::aired(), &options)
        .await
        .unwrap();
    assert!(episodes.len() >= 62, "{} episodes", episodes.len());
    assert!(matches!(episodes[0].detail, ItemDetail::Episode(_)));
}

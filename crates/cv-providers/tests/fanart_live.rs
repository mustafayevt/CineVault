//! Opt-in smoke test against the real Fanart.tv service.
//!
//! It runs only when `CV_FANART_API_KEY` is set; without it the test prints why it was skipped and
//! passes. It makes three requests: a credential check, one movie by IMDb ID and one show by TVDB
//! ID, and confirms the answers still have the shape the provider reads.
//!
//! ```text
//! CV_FANART_API_KEY=... cargo test -p cv-providers --test fanart_live -- --nocapture
//! ```

use std::sync::Arc;

use cv_domain::{MediaFileType, ProviderIds, id_key};
use cv_providers::model::{MediaKind, RequestOptions};
use cv_providers::{
    ArtworkProvider, CredentialResolver, FanartProvider, HttpClient, MemoryUserCredentials,
    Provider, catalogue,
};

fn variable(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

#[tokio::test]
async fn live_smoke() {
    let Some(key) = variable("CV_FANART_API_KEY") else {
        eprintln!("skipped: set CV_FANART_API_KEY to run the live Fanart.tv smoke test");
        return;
    };
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    resolver
        .set_user(catalogue::key::FANART, catalogue::API_KEY, &key)
        .unwrap();
    let provider = FanartProvider::new(HttpClient::builder().build().unwrap(), resolver.clone());
    let options = RequestOptions::default();

    provider
        .check_credentials(&resolver.resolve(catalogue::key::FANART).unwrap())
        .await
        .expect("the key is accepted");

    // A well-known film (The Matrix) and a well-known series (Game of Thrones).
    let mut movie = ProviderIds::new();
    movie.set(id_key::IMDB, "tt0133093");
    let list = provider
        .artwork(MediaKind::Movie, &movie, &options)
        .await
        .expect("movie artwork");
    assert!(
        list.iter().any(|c| c.kind == MediaFileType::Poster),
        "a poster"
    );
    assert!(list.iter().all(|c| c.url.starts_with("http")));
    eprintln!("movie: {} candidates", list.len());

    let mut show = ProviderIds::new();
    show.set(id_key::TVDB, "121361");
    let list = provider
        .artwork(MediaKind::Show, &show, &options)
        .await
        .expect("show artwork");
    assert!(
        list.iter().any(|c| c.kind == MediaFileType::Poster),
        "a poster"
    );
    let seasons = provider
        .season_artwork(&show, 1, &options)
        .await
        .expect("season artwork");
    eprintln!(
        "show: {} candidates, season 1: {}",
        list.len(),
        seasons.len()
    );
}

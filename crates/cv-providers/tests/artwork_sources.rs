//! Gathering artwork from several providers: every enabled provider is asked, every candidate is
//! tagged with its provider, and one provider failing never costs the others' candidates.
//! The Fanart.tv provider is real (against the mock server); the others are stand-ins.

mod support;

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use cv_domain::{MediaFileType, ProviderIds, id_key};
use cv_providers::model::{ArtworkCandidate, MediaKind, RequestOptions};
use cv_providers::{
    ArtworkProvider, CredentialResolver, FanartProvider, HttpClient, MemoryUserCredentials,
    Provider, ProviderError, catalogue, gather_artwork, gather_season_artwork,
};
use support::{MockServer, Reply};

/// A provider that answers with a fixed result.
struct Canned {
    id: &'static str,
    answer: Result<Vec<ArtworkCandidate>, ProviderError>,
}

#[async_trait]
impl Provider for Canned {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        "Canned"
    }
}

#[async_trait]
impl ArtworkProvider for Canned {
    async fn artwork(
        &self,
        _: MediaKind,
        _: &ProviderIds,
        _: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        self.answer.clone()
    }
}

fn candidate(kind: MediaFileType, url: &str, language: Option<&str>) -> ArtworkCandidate {
    ArtworkCandidate {
        kind,
        url: url.to_owned(),
        language: language.map(str::to_owned),
        width: 800,
        height: 310,
        score: 1.0,
        popularity: cv_providers::model::Popularity::Unknown,
    }
}

fn canned(
    id: &'static str,
    answer: Result<Vec<ArtworkCandidate>, ProviderError>,
) -> Arc<dyn ArtworkProvider> {
    Arc::new(Canned { id, answer })
}

async fn fanart() -> (MockServer, Arc<dyn ArtworkProvider>) {
    let server = MockServer::start().await;
    let body = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fanart/movie_example.json"),
    )
    .unwrap();
    server.route("/movies/tt0111222", Reply::ok(&body));
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    resolver
        .set_user(catalogue::key::FANART, catalogue::API_KEY, "key-0123456789")
        .unwrap();
    let provider = FanartProvider::with_base_url(
        HttpClient::builder().build().unwrap(),
        resolver,
        &server.url(""),
    );
    (server, Arc::new(provider))
}

fn ids() -> ProviderIds {
    let mut ids = ProviderIds::new();
    ids.set(id_key::IMDB, "tt0111222");
    ids
}

#[tokio::test]
async fn clearlogos_from_both_providers_are_considered() {
    let (_server, fanart) = fanart().await;
    let tmdb = canned(
        "tmdb",
        Ok(vec![candidate(
            MediaFileType::Clearlogo,
            "https://i.test/t/p/original/logo.png",
            Some("en"),
        )]),
    );
    let gathered = gather_artwork(
        &[tmdb, fanart],
        MediaKind::Movie,
        &ids(),
        &RequestOptions::default(),
    )
    .await;
    assert!(gathered.failures.is_empty());
    let logos: Vec<(&str, &str)> = gathered
        .candidates
        .iter()
        .filter(|c| c.candidate.kind == MediaFileType::Clearlogo)
        .map(|c| (c.provider, c.candidate.url.rsplit('/').next().unwrap()))
        .collect();
    assert_eq!(
        logos,
        [
            ("tmdb", "logo.png"),
            ("fanart", "logo-en.png"),
            ("fanart", "logo-de.png"),
            ("fanart", "logo-sd-en.png"),
        ]
    );
}

#[tokio::test]
async fn candidates_keep_the_order_the_providers_were_given_in() {
    let (_server, fanart) = fanart().await;
    let first = canned(
        "first",
        Ok(vec![candidate(
            MediaFileType::Poster,
            "https://i.test/1.jpg",
            None,
        )]),
    );
    let second = canned(
        "second",
        Ok(vec![candidate(
            MediaFileType::Poster,
            "https://i.test/2.jpg",
            None,
        )]),
    );
    let gathered = gather_artwork(
        &[first, fanart, second],
        MediaKind::Movie,
        &ids(),
        &RequestOptions::default(),
    )
    .await;
    let mut providers: Vec<&str> = gathered.candidates.iter().map(|c| c.provider).collect();
    providers.dedup();
    assert_eq!(providers, ["first", "fanart", "second"]);
}

#[tokio::test]
async fn a_failing_provider_is_reported_and_the_others_still_answer() {
    let (_server, fanart) = fanart().await;
    let broken = canned("imdb", Err(ProviderError::FormatChanged("IMDb".to_owned())));
    let unknown = canned("tvdb", Err(ProviderError::NotFound("TheTVDB".to_owned())));
    let gathered = gather_artwork(
        &[broken, unknown, fanart],
        MediaKind::Movie,
        &ids(),
        &RequestOptions::default(),
    )
    .await;
    // Not knowing a title is not a failure.
    assert_eq!(gathered.failures.len(), 1);
    assert_eq!(gathered.failures[0].0, "imdb");
    assert!(matches!(
        gathered.failures[0].1,
        ProviderError::FormatChanged(_)
    ));
    assert!(!gathered.candidates.is_empty());
    assert!(gathered.candidates.iter().all(|c| c.provider == "fanart"));
}

#[tokio::test]
async fn no_providers_gather_nothing() {
    let gathered = gather_artwork(&[], MediaKind::Movie, &ids(), &RequestOptions::default()).await;
    assert!(gathered.candidates.is_empty() && gathered.failures.is_empty());
}

/// A provider with images for some seasons only; other seasons are unknown to it.
struct Seasons {
    id: &'static str,
    seasons: Vec<(u32, Vec<ArtworkCandidate>)>,
}

#[async_trait]
impl Provider for Seasons {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        "Seasons"
    }
}

#[async_trait]
impl ArtworkProvider for Seasons {
    async fn artwork(
        &self,
        _: MediaKind,
        _: &ProviderIds,
        _: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        // The item-level call never returns season images.
        Ok(Vec::new())
    }

    async fn season_artwork(
        &self,
        _: &ProviderIds,
        season: u32,
        _: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        Ok(self
            .seasons
            .iter()
            .find(|(number, _)| *number == season)
            .map(|(_, list)| list.clone())
            .unwrap_or_default())
    }
}

#[tokio::test]
async fn season_images_are_gathered_per_season_and_tagged_with_their_provider() {
    let with_images: Arc<dyn ArtworkProvider> = Arc::new(Seasons {
        id: "tvdb",
        seasons: vec![
            (
                1,
                vec![
                    candidate(
                        MediaFileType::SeasonPoster,
                        "https://i.test/s1.jpg",
                        Some("en"),
                    ),
                    candidate(MediaFileType::SeasonBanner, "https://i.test/b1.jpg", None),
                ],
            ),
            (
                2,
                vec![candidate(
                    MediaFileType::SeasonPoster,
                    "https://i.test/s2.jpg",
                    Some("en"),
                )],
            ),
        ],
    });
    // A provider without season images keeps the trait default and offers nothing.
    let item_only = canned(
        "imdb",
        Ok(vec![candidate(
            MediaFileType::Poster,
            "https://i.test/p.jpg",
            None,
        )]),
    );
    let broken: Arc<dyn ArtworkProvider> = Arc::new(Seasons {
        id: "tmdb",
        seasons: Vec::new(),
    });
    let providers = [with_images, item_only, broken];

    let one = gather_season_artwork(&providers, &ids(), 1, &RequestOptions::default()).await;
    assert!(one.failures.is_empty());
    let urls: Vec<(&str, &str)> = one
        .candidates
        .iter()
        .map(|c| (c.provider, c.candidate.url.as_str()))
        .collect();
    assert_eq!(
        urls,
        [
            ("tvdb", "https://i.test/s1.jpg"),
            ("tvdb", "https://i.test/b1.jpg")
        ]
    );

    let two = gather_season_artwork(&providers, &ids(), 2, &RequestOptions::default()).await;
    assert_eq!(two.candidates.len(), 1);
    assert_eq!(two.candidates[0].candidate.url, "https://i.test/s2.jpg");

    let none = gather_season_artwork(&providers, &ids(), 7, &RequestOptions::default()).await;
    assert!(none.candidates.is_empty() && none.failures.is_empty());
}

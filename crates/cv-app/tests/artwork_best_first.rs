//! Best artwork first (change `artwork-best-first`): the automatic choice and the picker order
//! images by how much people like them (TMDB votes as a Bayesian average, Fanart.tv likes on a log
//! scale), then by size, within the language and size tiers of the settings; images that are too
//! small or misshapen are marked and come last. Mock providers, no network.

mod support;

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::artwork::ArtworkService;
use cv_app::artwork::stream::SourceAnswer;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_artwork::download::fetch::StaticFetcher;
use cv_artwork::thumb::ThumbCache;
use cv_domain::{Item, ItemId, MediaFile, MediaFileType, ProviderIds};
use cv_providers::model::{ArtworkCandidate, MediaKind, Popularity, RequestOptions};
use cv_providers::{
    ArtworkProvider, CredentialResolver, MemoryUserCredentials, Provider, ProviderError,
    ProviderRegistry,
};
use image::{ImageFormat, Rgb, RgbImage};
use support::Lab;

fn jpeg(seed: u8) -> Vec<u8> {
    let image = RgbImage::from_pixel(40, 60, Rgb([seed, 200, 10]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

/// English posters with every kind of popularity, under a key that needs no credentials.
struct Voted;

#[async_trait]
impl Provider for Voted {
    fn id(&self) -> &'static str {
        "imdb"
    }
    fn name(&self) -> &'static str {
        "Voted"
    }
}

const POSTERS: [(&str, u32, u32, Popularity); 5] = [
    (
        "one-ten",
        1000,
        1500,
        Popularity::Rated {
            average: 10.0,
            votes: 1,
        },
    ),
    (
        "fifty",
        1000,
        1500,
        Popularity::Rated {
            average: 5.6,
            votes: 50,
        },
    ),
    ("liked", 1000, 1426, Popularity::Liked { likes: 14 }),
    ("tiny-loved", 342, 513, Popularity::Liked { likes: 60 }),
    ("unvoted", 1000, 1500, Popularity::Unknown),
];

fn url(name: &str) -> String {
    format!("https://img.test/{name}.jpg")
}

#[async_trait]
impl ArtworkProvider for Voted {
    async fn artwork(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        Ok(POSTERS
            .iter()
            .map(|(name, width, height, popularity)| ArtworkCandidate {
                kind: MediaFileType::Poster,
                url: url(name),
                language: Some("en".to_owned()),
                width: *width,
                height: *height,
                score: 0.0,
                popularity: *popularity,
            })
            .collect())
    }
}

struct Rig {
    lab: Lab,
    artwork: Arc<ArtworkService>,
    library: PathBuf,
}

fn rig() -> Rig {
    let lab = Lab::new();
    let mut registry = ProviderRegistry::new();
    registry.register_artwork(Arc::new(Voted));
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let providers = Arc::new(ProviderCredentialsService::with_resolver(
        resolver, registry,
    ));
    let settings = Arc::new(SettingsService::in_memory(""));
    let fetcher = Arc::new(StaticFetcher::new());
    for (n, (name, ..)) in POSTERS.iter().enumerate() {
        fetcher.insert(&url(name), jpeg(u8::try_from(n).unwrap()));
    }
    let thumbs = Arc::new(ThumbCache::open(&lab.dir.path().join("cache"), None).unwrap());
    let artwork =
        ArtworkService::with_fetcher(Arc::clone(&lab.db), providers, settings, thumbs, fetcher);
    let library = lab.dir.path().join("Movies");
    Rig {
        lab,
        artwork,
        library,
    }
}

impl Rig {
    fn movie(&self) -> ItemId {
        let folder = self.library.join("Heat (1995)");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("Heat (1995).mkv"), b"video").unwrap();
        let mut item = Item::movie("Heat");
        item.core.year = 1995;
        item.core.ids.set("tmdb", "949");
        item.core.path = folder.to_string_lossy().into_owned();
        item.core.data_source = self.library.to_string_lossy().into_owned();
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            folder.to_string_lossy(),
            "Heat (1995).mkv",
        ));
        self.lab.add(item)
    }
}

#[tokio::test]
async fn the_automatic_choice_takes_the_image_people_like_most() {
    let rig = rig();
    let movie = rig.movie();
    let summary = rig.artwork.download_missing(&[movie], false).await;
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);
    let item = rig.lab.item(movie);
    assert_eq!(
        item.core.artwork_urls[&MediaFileType::Poster],
        url("liked"),
        "fourteen likes beat fifty middling votes, one perfect vote and a tiny image"
    );
}

#[tokio::test]
async fn the_picker_orders_best_first_and_marks_the_low_quality_images() {
    let rig = rig();
    let movie = rig.movie();
    let stream = rig
        .artwork
        .candidate_stream(movie, MediaFileType::Poster)
        .await
        .unwrap();
    assert_eq!(stream.sources.len(), 1);
    let SourceAnswer::Candidates(mut found) = stream.sources[0].fetch().await else {
        panic!("the provider answers");
    };
    found.sort_by_key(|c| std::cmp::Reverse(c.best));
    let order: Vec<String> = found.iter().map(|c| c.url.clone()).collect();
    assert_eq!(
        order[..4],
        [url("liked"), url("fifty"), url("one-ten"), url("unvoted")]
    );

    let tiny = found.iter().find(|c| c.url == url("tiny-loved")).unwrap();
    assert!(
        tiny.low_quality,
        "a 342 pixel poster is listed behind Show more"
    );
    assert!(found.iter().filter(|c| c.low_quality).count() == 1);

    // The popularity travels with the image for the picker's hint.
    let fifty = found.iter().find(|c| c.url == url("fifty")).unwrap();
    assert_eq!((fifty.votes, fifty.rating, fifty.likes), (50, 56, 0));
    let liked = found.iter().find(|c| c.url == url("liked")).unwrap();
    assert_eq!((liked.votes, liked.likes), (0, 14));
}

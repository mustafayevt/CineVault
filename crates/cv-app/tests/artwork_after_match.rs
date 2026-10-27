//! Artwork after a match: once an item is matched and scraped, its empty artwork
//! types are downloaded after every scrape, and the sources are
//! remembered so that Re-download artwork can fetch them again. Mock providers, no network.

mod support;

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::artwork::ArtworkService;
use cv_app::match_service::MatchService;
use cv_app::matching::batch::BatchMode;
use cv_app::notices::NoticeHub;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_artwork::download::fetch::StaticFetcher;
use cv_artwork::thumb::ThumbCache;
use cv_db::repo::artwork_refs::artwork_refs;
use cv_domain::{Credit, CreditKind, Item, ItemId, MediaFile, MediaFileType, ProviderIds};
use cv_providers::model::{ArtworkCandidate, MediaKind, RequestOptions};
use cv_providers::{
    ArtworkProvider, CredentialResolver, MemoryUserCredentials, Provider, ProviderError,
    ProviderRegistry,
};
use image::{ImageFormat, Rgb, RgbImage};
use serde_json::json;
use support::{Entry, Lab, Scripted, arc};

fn jpeg(seed: u8) -> Vec<u8> {
    let image = RgbImage::from_pixel(40, 60, Rgb([seed, 200, 10]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

/// An artwork provider with a fixed gallery, under a key that needs no credentials.
struct Gallery;

#[async_trait]
impl Provider for Gallery {
    fn id(&self) -> &'static str {
        "imdb"
    }
    fn name(&self) -> &'static str {
        "Gallery"
    }
}

#[async_trait]
impl ArtworkProvider for Gallery {
    async fn artwork(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let image = |kind, url: &str, score| ArtworkCandidate {
            kind,
            url: url.to_owned(),
            language: Some("en".to_owned()),
            width: 2000,
            height: 3000,
            score,
            popularity: cv_providers::model::Popularity::Unknown,
        };
        Ok(vec![
            image(MediaFileType::Poster, "https://img.test/poster.jpg", 8.0),
            image(MediaFileType::Fanart, "https://img.test/fanart.jpg", 8.0),
        ])
    }
}

struct Rig {
    lab: Lab,
    settings: Arc<SettingsService>,
    matching: Arc<MatchService>,
    artwork: Arc<ArtworkService>,
    library: PathBuf,
}

fn rig() -> Rig {
    let lab = Lab::new();
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(arc(Scripted::new(
        "scripted",
        vec![Entry::new("693134", "Dune Part Two", 2024)],
    )));
    registry.register_artwork(Arc::new(Gallery));
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let providers = Arc::new(ProviderCredentialsService::with_resolver(
        resolver, registry,
    ));
    let settings = Arc::new(SettingsService::in_memory(""));
    let matching = MatchService::new(
        Arc::clone(&lab.db),
        lab.manager.clone(),
        Arc::clone(&providers),
        Arc::clone(&settings),
        Arc::new(NoticeHub::new()),
    );
    let fetcher = Arc::new(StaticFetcher::new());
    fetcher.insert("https://img.test/poster.jpg", jpeg(1));
    fetcher.insert("https://img.test/fanart.jpg", jpeg(2));
    fetcher.insert("https://img.test/dune.jpg", jpeg(3));
    let thumbs = Arc::new(ThumbCache::open(&lab.dir.path().join("cache"), None).unwrap());
    let artwork = ArtworkService::with_fetcher(
        Arc::clone(&lab.db),
        providers,
        Arc::clone(&settings),
        thumbs,
        fetcher.clone(),
    );
    {
        let artwork = Arc::clone(&artwork);
        matching.set_after_match(Arc::new(move |item| artwork.after_match(item)));
    }
    let library = lab.dir.path().join("Movies");
    Rig {
        lab,
        settings,
        matching,
        artwork,
        library,
    }
}

impl Rig {
    /// A movie with a folder and a video on disk, not matched yet.
    fn movie(&self, locked: bool, matched: bool) -> (ItemId, PathBuf) {
        let folder = self.library.join("Dune Part Two (2024)");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("Dune Part Two (2024).mkv"), b"video").unwrap();
        let mut item = Item::movie("Dune Part Two");
        item.core.locked = locked;
        if matched {
            item.core.ids.set("tmdb", "693134");
        }
        item.core.path = folder.to_string_lossy().into_owned();
        item.core.data_source = self.library.to_string_lossy().into_owned();
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            folder.to_string_lossy(),
            "Dune Part Two (2024).mkv",
        ));
        let mut cast = Credit::new(CreditKind::Actor, "Zendaya", "Chani");
        cast.person.thumb_url = "https://img.test/dune.jpg".to_owned();
        item.core.credits.push(cast);
        (self.lab.add(item), folder)
    }

    async fn match_and_wait(&self, item: ItemId) {
        self.matching
            .submit(BatchMode::MatchSelected, Some(&[item]))
            .expect("batch")
            .wait()
            .await;
    }

    fn has_source(&self, item: ItemId, kind: MediaFileType) -> bool {
        self.lab
            .db
            .read(|c| artwork_refs(c, item))
            .map(|refs| {
                refs.iter()
                    .any(|r| r.kind == kind && r.cache_hash.is_some())
            })
            .unwrap_or(false)
    }
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

async fn until(what: &str, condition: impl Fn() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out: {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_matched_item_gets_its_missing_artwork_and_remembers_the_sources() {
    let rig = rig();
    let (item, folder) = rig.movie(false, false);
    rig.match_and_wait(item).await;

    until("artwork recorded", || {
        rig.has_source(item, MediaFileType::Poster) && rig.has_source(item, MediaFileType::Fanart)
    })
    .await;

    assert_eq!(
        names(&folder),
        [
            "Dune Part Two (2024)-fanart.jpg",
            "Dune Part Two (2024)-poster.jpg",
            "Dune Part Two (2024).mkv"
        ]
    );
    let stored = rig.lab.item(item);
    assert_eq!(
        stored.core.artwork_urls[&MediaFileType::Poster],
        "https://img.test/poster.jpg"
    );
    assert!(
        stored
            .core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::Poster
                && f.filename == "Dune Part Two (2024)-poster.jpg"),
        "the poster is one of the item's files, so the grid can show it"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_step_after_a_scrape_leaves_a_locked_item_alone_but_an_explicit_run_does_not() {
    let rig = rig();
    let (item, folder) = rig.movie(true, true);

    rig.artwork.after_match(item);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(names(&folder), ["Dune Part Two (2024).mkv"]);

    let summary = rig.artwork.download_missing(&[item], false).await;
    assert_eq!(summary.images(), 2, "{summary:?}");
    assert_eq!(names(&folder).len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redownload_restores_a_deleted_poster_from_its_remembered_source() {
    let rig = rig();
    let (item, folder) = rig.movie(false, false);
    rig.match_and_wait(item).await;
    let poster = folder.join("Dune Part Two (2024)-poster.jpg");
    until("poster recorded", || {
        poster.exists() && rig.has_source(item, MediaFileType::Poster)
    })
    .await;

    fs::remove_file(&poster).unwrap();
    let summary = rig.artwork.redownload(&[item], false).await;

    assert_eq!(summary.images(), 1);
    assert_eq!(fs::read(&poster).unwrap(), jpeg(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actor_portraits_follow_their_setting_and_land_in_the_actors_folder() {
    let rig = rig();
    rig.settings
        .set("artwork.actor-images", json!(true))
        .unwrap();
    let (item, folder) = rig.movie(false, true);

    let summary = rig.artwork.download_missing(&[item], false).await;

    assert_eq!(summary.portraits(), 1);
    assert_eq!(
        fs::read(folder.join(".actors/Zendaya.jpg")).unwrap(),
        jpeg(3)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_item_that_is_not_a_movie_show_or_collection_is_skipped_with_a_reason() {
    let rig = rig();
    let show = rig.lab.show("Dune show", 2020);
    let season = rig.lab.add(Item::season(show, 1));

    let summary = rig.artwork.download_missing(&[season], false).await;

    assert!(summary.outcomes.is_empty());
    assert_eq!(summary.skipped.len(), 1);
    assert!(summary.skipped[0].1.contains("collections"));
}

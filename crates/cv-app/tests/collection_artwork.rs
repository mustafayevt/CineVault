//! Collection artwork locations: the image URLs a collection keeps are written next to
//! its movies (`movieset-<type>`) and/or into the collection data folder in Kodi or Automator
//! style, with the character replacement and the optional TMDB ID suffix of the settings. No
//! network: images come from a static fetcher.

mod support;

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::artwork::ArtworkService;
use cv_app::artwork::stream::SourceAnswer;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_artwork::download::fetch::StaticFetcher;
use cv_artwork::thumb::ThumbCache;
use cv_domain::{Item, ItemDetail, ItemId, MediaFile, MediaFileType, ProviderIds};
use cv_providers::model::{ArtworkCandidate, MediaKind, RequestOptions};
use cv_providers::{ArtworkProvider, Provider, ProviderError};
use cv_providers::{CredentialResolver, MemoryUserCredentials, ProviderRegistry};
use image::{ImageFormat, Rgb, RgbImage};
use serde_json::json;
use support::Lab;

fn jpeg(seed: u8) -> Vec<u8> {
    let image = RgbImage::from_pixel(40, 60, Rgb([seed, 200, 10]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

struct Rig {
    lab: Lab,
    settings: Arc<SettingsService>,
    artwork: Arc<ArtworkService>,
    fetcher: Arc<StaticFetcher>,
    library: PathBuf,
    collections: PathBuf,
}

fn rig() -> Rig {
    rig_with(ProviderRegistry::new())
}

fn rig_with(registry: ProviderRegistry) -> Rig {
    let lab = Lab::new();
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let providers = Arc::new(ProviderCredentialsService::with_resolver(
        resolver, registry,
    ));
    let settings = Arc::new(SettingsService::in_memory(""));
    let fetcher = Arc::new(StaticFetcher::new());
    fetcher.insert("https://img.test/set-poster.jpg", jpeg(1));
    fetcher.insert("https://img.test/set-fanart.jpg", jpeg(2));
    for (seed, url) in [
        (10, "https://img.test/new-poster.jpg"),
        (11, "https://img.test/other-poster.jpg"),
        (12, "https://img.test/other-fanart.jpg"),
    ] {
        fetcher.insert(url, jpeg(seed));
    }
    let thumbs = Arc::new(ThumbCache::open(&lab.dir.path().join("cache"), None).unwrap());
    let artwork = ArtworkService::with_fetcher(
        Arc::clone(&lab.db),
        providers,
        Arc::clone(&settings),
        thumbs,
        fetcher.clone(),
    );
    let library = lab.dir.path().join("Movies");
    let collections = lab.dir.path().join("Collections");
    Rig {
        lab,
        settings,
        artwork,
        fetcher,
        library,
        collections,
    }
}

impl Rig {
    fn movie(&self, folder: &str, title: &str, year: u16, parent: ItemId) -> PathBuf {
        let dir = self.library.join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{folder}.mkv")), b"video").unwrap();
        let mut item = Item::movie(title);
        item.core.year = year;
        item.parent_id = Some(parent);
        item.core.path = dir.to_string_lossy().into_owned();
        item.core.data_source = self.library.to_string_lossy().into_owned();
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            dir.to_string_lossy(),
            format!("{folder}.mkv"),
        ));
        self.lab.add(item);
        dir
    }

    /// A collection with a poster and a fanart URL.
    fn collection(&self, title: &str, tmdb: &str) -> ItemId {
        let mut set = Item::set(title);
        set.core.ids.set("tmdb", tmdb);
        set.core.artwork_urls.insert(
            MediaFileType::Poster,
            "https://img.test/set-poster.jpg".into(),
        );
        set.core.artwork_urls.insert(
            MediaFileType::Fanart,
            "https://img.test/set-fanart.jpg".into(),
        );
        self.lab.add(set)
    }

    fn set(&self, key: &str, value: serde_json::Value) {
        self.settings.set(key, value).unwrap();
    }

    fn use_collection_folder(&self) {
        self.set(
            "metadata.nfo-collection-folder",
            json!(self.collections.to_string_lossy()),
        );
    }
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_default_writes_movieset_files_into_every_movie_folder() {
    let rig = rig();
    let set = rig.collection("Dune Collection", "726871");
    let one = rig.movie("Dune (2021)", "Dune", 2021, set);
    let two = rig.movie("Dune Part Two (2024)", "Dune: Part Two", 2024, set);

    let summary = rig.artwork.download_missing(&[set], false).await;
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);
    assert_eq!(summary.images(), 2);
    for dir in [&one, &two] {
        let files = names(dir);
        assert!(
            files.contains(&"movieset-poster.jpg".to_owned()),
            "{files:?}"
        );
        assert!(
            files.contains(&"movieset-fanart.jpg".to_owned()),
            "{files:?}"
        );
    }
    assert!(!rig.collections.exists());

    // The collection remembers the files, so the library lists them.
    let stored = rig.lab.item(set);
    assert_eq!(
        stored
            .core
            .media_files
            .iter()
            .filter(|f| f.kind == MediaFileType::Poster)
            .count(),
        2
    );
    assert!(matches!(stored.detail, ItemDetail::Set(_)));
    assert_eq!(rig.fetcher.requests().len(), 2);

    // Nothing is fetched or replaced the second time.
    let again = rig.artwork.download_missing(&[set], false).await;
    assert_eq!(again.images(), 0);
    assert_eq!(rig.fetcher.requests().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kodi_style_writes_into_the_collection_folder() {
    let rig = rig();
    rig.use_collection_folder();
    rig.set("artwork.collection-locations", json!("kodi"));
    let set = rig.collection("Dune Collection", "726871");
    let movie = rig.movie("Dune (2021)", "Dune", 2021, set);

    rig.artwork.download_missing(&[set], false).await;
    assert_eq!(
        names(&rig.collections.join("Dune Collection")),
        ["fanart.jpg", "poster.jpg"]
    );
    assert_eq!(
        names(&movie),
        ["Dune (2021).mkv"],
        "the movie folder is untouched"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn automator_style_with_the_tmdb_id_and_a_replacement_character() {
    let rig = rig();
    rig.use_collection_folder();
    rig.set("artwork.collection-locations", json!("automator"));
    rig.set("metadata.nfo-collection-tmdb-id", json!(true));
    rig.set("metadata.collection-name-replacement", json!("-"));
    let set = rig.collection("Mission: Impossible Collection", "87359");
    rig.movie("Mission (1996)", "Mission: Impossible", 1996, set);

    rig.artwork.download_missing(&[set], false).await;
    assert_eq!(
        names(&rig.collections),
        [
            "Mission- Impossible Collection-tmdb-87359-fanart.jpg",
            "Mission- Impossible Collection-tmdb-87359-poster.jpg"
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn several_styles_write_the_same_image_to_each_place() {
    let rig = rig();
    rig.use_collection_folder();
    rig.set("artwork.collection-locations", json!("both"));
    let set = rig.collection("Dune Collection", "726871");
    let movie = rig.movie("Dune (2021)", "Dune", 2021, set);

    rig.artwork.download_missing(&[set], false).await;
    assert!(names(&movie).contains(&"movieset-poster.jpg".to_owned()));
    let a = fs::read(movie.join("movieset-poster.jpg")).unwrap();
    let b = fs::read(rig.collections.join("Dune Collection").join("poster.jpg")).unwrap();
    assert_eq!(a, b);
    // One download per image, whatever the number of files.
    assert_eq!(rig.fetcher.requests().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_folder_style_without_a_collection_folder_writes_nothing_and_says_why() {
    let rig = rig();
    rig.set("artwork.collection-locations", json!("kodi"));
    let set = rig.collection("Dune Collection", "726871");
    rig.movie("Dune (2021)", "Dune", 2021, set);

    let summary = rig.artwork.download_missing(&[set], false).await;
    assert_eq!(summary.images(), 0);
    let problems = &summary.outcomes[0].problems;
    assert!(
        problems.iter().any(|p| p.contains("collection folder")),
        "{problems:?}"
    );
    assert!(!rig.collections.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replacing_moves_the_old_file_to_the_trash_and_a_kept_file_stays() {
    let rig = rig();
    let set = rig.collection("Dune Collection", "726871");
    let movie = rig.movie("Dune (2021)", "Dune", 2021, set);
    fs::write(movie.join("movieset-poster.png"), b"mine").unwrap();

    let kept = rig.artwork.download_missing(&[set], false).await;
    assert_eq!(kept.images(), 1, "only the fanart is new");
    assert_eq!(
        fs::read(movie.join("movieset-poster.png")).unwrap(),
        b"mine"
    );

    let replaced = rig.artwork.download_missing(&[set], true).await;
    assert_eq!(replaced.images(), 2);
    assert!(!movie.join("movieset-poster.png").exists());
    assert!(movie.join("movieset-poster.jpg").is_file());
    assert!(replaced.outcomes[0].trashed >= 1);
    assert!(rig.library.join(".deletedByCineVault").is_dir());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_movie_matched_into_a_collection_fills_the_collection_artwork_behind_the_setting() {
    let rig = rig();
    let set = rig.collection("Dune Collection", "726871");
    let movie = rig.movie("Dune (2021)", "Dune", 2021, set);
    let movie_id = rig
        .lab
        .db
        .read(|c| cv_db::repo::children_of(c, set))
        .unwrap()[0];

    rig.artwork.set_after_scrape_runs(false);
    rig.artwork.after_match(movie_id);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(names(&movie), ["Dune (2021).mkv"], "switched off");

    rig.artwork.set_after_scrape_runs(true);
    rig.artwork.after_match(movie_id);
    let start = std::time::Instant::now();
    // The poster and the fanart are written one after the other; wait for both.
    while !(movie.join("movieset-poster.jpg").is_file()
        && movie.join("movieset-fanart.jpg").is_file())
    {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "timed out: {:?}",
            names(&movie)
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cleanup_moves_collection_artwork_the_locations_no_longer_use_to_the_trash() {
    let rig = rig();
    let set = rig.collection("Dune Collection", "726871");
    let movie = rig.movie("Dune (2021)", "Dune", 2021, set);
    // A movie of no collection with a file of the same name is not the collection's.
    let other = rig.library.join("Arrival (2016)");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("movieset-poster.jpg"), jpeg(9)).unwrap();
    rig.artwork.download_missing(&[set], false).await;
    assert!(movie.join("movieset-poster.jpg").is_file());

    // The settings move to the Kodi style. Until the images are there, the old ones stay.
    rig.use_collection_folder();
    rig.set("artwork.collection-locations", json!("kodi"));
    let early = rig.artwork.cleanup(&[set], true).await;
    assert!(early[0].outcome.as_ref().unwrap().is_empty(), "{early:?}");

    rig.artwork.download_missing(&[set], false).await;
    let kodi = rig.collections.join("Dune Collection");
    assert_eq!(names(&kodi), ["fanart.jpg", "poster.jpg"]);

    let planned = rig.artwork.cleanup(&[set], true).await;
    let mut listed: Vec<String> = planned[0]
        .outcome
        .as_ref()
        .unwrap()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    listed.sort();
    assert_eq!(listed, ["movieset-fanart.jpg", "movieset-poster.jpg"]);
    assert!(
        movie.join("movieset-poster.jpg").is_file(),
        "a dry run moves nothing"
    );

    let done = rig.artwork.cleanup(&[set], false).await;
    assert_eq!(done[0].outcome.as_ref().unwrap().len(), 2, "{done:?}");
    assert_eq!(names(&movie), ["Dune (2021).mkv"]);
    assert_eq!(names(&kodi), ["fanart.jpg", "poster.jpg"]);
    assert!(other.join("movieset-poster.jpg").is_file());
    // Nothing is deleted: the files are in the trash folder of the data source.
    let trash = rig.library.join(".deletedByCineVault");
    assert!(trash.is_dir(), "no trash folder");
    let stored = rig.lab.item(set);
    assert!(
        stored
            .core
            .media_files
            .iter()
            .all(|f| !f.filename.starts_with("movieset-")),
        "{:?}",
        stored.core.media_files
    );

    let again = rig.artwork.cleanup(&[set], false).await;
    assert!(again[0].outcome.as_ref().unwrap().is_empty());
}

// ---- the artwork picker for a collection --------------------------------------------------------

/// A provider with two posters and a fanart for a collection (and for a movie), under a key that
/// needs no credentials.
struct CollectionImages;

#[async_trait]
impl Provider for CollectionImages {
    fn id(&self) -> &'static str {
        "imdb"
    }
    fn name(&self) -> &'static str {
        "Collection images"
    }
}

fn candidate(kind: MediaFileType, url: &str) -> ArtworkCandidate {
    ArtworkCandidate {
        kind,
        url: url.to_owned(),
        language: Some("en".to_owned()),
        width: 2000,
        height: 3000,
        score: 8.0,
        popularity: cv_providers::model::Popularity::Unknown,
    }
}

fn images() -> Vec<ArtworkCandidate> {
    vec![
        candidate(MediaFileType::Poster, "https://img.test/new-poster.jpg"),
        candidate(MediaFileType::Poster, "https://img.test/other-poster.jpg"),
        candidate(MediaFileType::Fanart, "https://img.test/other-fanart.jpg"),
    ]
}

#[async_trait]
impl ArtworkProvider for CollectionImages {
    async fn artwork(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        Ok(images())
    }

    fn offers_collection(&self, slot: MediaFileType) -> bool {
        matches!(slot, MediaFileType::Poster | MediaFileType::Fanart)
    }

    async fn collection_artwork(
        &self,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        // The collection is asked for by its `tmdbSet`.
        assert_eq!(ids.get("tmdbSet"), Some("726871"));
        Ok(images())
    }
}

fn rig_with_images() -> Rig {
    let mut registry = ProviderRegistry::new();
    registry.register_artwork(Arc::new(CollectionImages));
    rig_with(registry)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_picker_offers_a_collection_its_poster_and_fanart() {
    let rig = rig_with_images();
    let set = rig.collection("Dune Collection", "726871");
    rig.movie("Dune (2021)", "Dune", 2021, set);
    rig.artwork.download_missing(&[set], false).await;

    let stream = rig
        .artwork
        .candidate_stream(set, MediaFileType::Poster)
        .await
        .unwrap();
    assert_eq!(stream.title, "Dune Collection");
    assert_eq!(stream.slots, [MediaFileType::Poster, MediaFileType::Fanart]);
    assert!(stream.current.is_some(), "the poster it has is pinned");
    assert_eq!(stream.sources.len(), 1);
    let SourceAnswer::Candidates(found) = stream.sources[0].fetch().await else {
        panic!("the provider did not answer");
    };
    assert_eq!(found.len(), 2, "two posters");

    // A type a collection does not have is refused.
    assert!(
        rig.artwork
            .candidate_stream(set, MediaFileType::Banner)
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_chosen_collection_image_replaces_the_old_one_and_is_remembered() {
    let rig = rig_with_images();
    let set = rig.collection("Dune Collection", "726871");
    let movie = rig.movie("Dune (2021)", "Dune", 2021, set);
    rig.artwork.download_missing(&[set], false).await;
    let fanart = fs::read(movie.join("movieset-fanart.jpg")).unwrap();

    let placed = rig
        .artwork
        .place(
            set,
            MediaFileType::Poster,
            "https://img.test/new-poster.jpg",
            true,
        )
        .await
        .unwrap();
    assert_eq!(placed.images, 1);
    assert_eq!(
        fs::read(movie.join("movieset-poster.jpg")).unwrap(),
        jpeg(10),
        "the chosen image is the poster now"
    );
    assert_eq!(
        fs::read(movie.join("movieset-fanart.jpg")).unwrap(),
        fanart,
        "the other type is untouched"
    );
    assert!(
        rig.library.join(".deletedByCineVault").is_dir(),
        "the old poster went to the trash folder"
    );
    // A later re-download writes the chosen image again, not the one from the first scrape.
    assert_eq!(
        rig.lab
            .item(set)
            .core
            .artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("https://img.test/new-poster.jpg")
    );

    // A type a collection does not have is refused.
    assert!(
        rig.artwork
            .place(
                set,
                MediaFileType::Banner,
                "https://img.test/new-poster.jpg",
                true
            )
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_counts_come_without_downloading_a_thumbnail() {
    let rig = rig_with_images();
    let set = rig.collection("Dune Collection", "726871");
    rig.movie("Dune (2021)", "Dune", 2021, set);
    let before = rig.fetcher.requests().len();

    let counts = rig.artwork.candidate_counts(set).await.unwrap();
    let listed: Vec<_> = counts.iter().map(|c| (c.slot, c.count)).collect();
    assert_eq!(
        listed,
        [(MediaFileType::Poster, 2), (MediaFileType::Fanart, 1)]
    );
    assert!(
        counts.iter().all(|c| c.current.is_none()),
        "nothing written yet"
    );

    // A movie counts what the provider has for each of its types.
    let dir = rig.library.join("Alien (1979)");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Alien.mkv"), b"video").unwrap();
    let mut alien = Item::movie("Alien");
    alien.core.ids.set("tmdb", "348");
    alien.core.path = dir.to_string_lossy().into_owned();
    alien.core.data_source = rig.library.to_string_lossy().into_owned();
    alien.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        dir.to_string_lossy(),
        "Alien.mkv",
    ));
    let alien = rig.lab.add(alien);
    let counts = rig.artwork.candidate_counts(alien).await.unwrap();
    let of = |slot| counts.iter().find(|c| c.slot == slot).map(|c| c.count);
    assert_eq!(of(MediaFileType::Poster), Some(2));
    assert_eq!(of(MediaFileType::Fanart), Some(1));

    assert_eq!(
        rig.fetcher.requests().len(),
        before,
        "nothing was downloaded to count"
    );
}

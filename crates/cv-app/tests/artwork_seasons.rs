//! Season and episode artwork of a show: Download missing artwork asks the artwork
//! providers for each season, writes the images to the season names of the settings and the
//! episode thumbs to the thumb names, and remembers the sources. Mock providers, no network.

mod support;

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::artwork::ArtworkService;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_artwork::download::fetch::StaticFetcher;
use cv_artwork::thumb::ThumbCache;
use cv_db::repo::artwork_refs::artwork_refs;
use cv_domain::{EpisodeNumber, Item, ItemDetail, ItemId, MediaFile, MediaFileType, ProviderIds};
use cv_providers::model::{ArtworkCandidate, MediaKind, RequestOptions};
use cv_providers::{
    ArtworkProvider, CredentialResolver, MemoryUserCredentials, Provider, ProviderError,
    ProviderRegistry,
};
use image::{ImageFormat, Rgb, RgbImage};
use serde_json::json;
use support::Lab;

fn jpeg(seed: u8) -> Vec<u8> {
    let image = RgbImage::from_pixel(40, 60, Rgb([seed, 200, 10]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

/// An artwork provider with images per season, under a key that needs no credentials. Season 2
/// is unknown to it, and it records which seasons it was asked about.
struct Seasonal {
    asked: Mutex<Vec<u32>>,
}

#[async_trait]
impl Provider for Seasonal {
    fn id(&self) -> &'static str {
        "imdb"
    }
    fn name(&self) -> &'static str {
        "Seasonal"
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

#[async_trait]
impl ArtworkProvider for Seasonal {
    async fn artwork(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        Ok(vec![candidate(
            MediaFileType::Poster,
            "https://img.test/show-poster.jpg",
        )])
    }

    fn offers_season(&self, _slot: MediaFileType) -> bool {
        true
    }

    async fn season_artwork(
        &self,
        _show: &ProviderIds,
        season: u32,
        _options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        self.asked.lock().unwrap().push(season);
        Ok(match season {
            0 => vec![candidate(
                MediaFileType::SeasonPoster,
                "https://img.test/specials.jpg",
            )],
            1 => vec![
                candidate(MediaFileType::SeasonPoster, "https://img.test/s1.jpg"),
                candidate(
                    MediaFileType::SeasonBanner,
                    "https://img.test/s1-banner.jpg",
                ),
                candidate(
                    MediaFileType::SeasonFanart,
                    "https://img.test/s1-fanart.jpg",
                ),
            ],
            _ => Vec::new(),
        })
    }
}

struct Rig {
    lab: Lab,
    settings: Arc<SettingsService>,
    artwork: Arc<ArtworkService>,
    fetcher: Arc<StaticFetcher>,
    provider: Arc<Seasonal>,
    show_dir: PathBuf,
}

fn rig() -> Rig {
    let lab = Lab::new();
    let provider = Arc::new(Seasonal {
        asked: Mutex::new(Vec::new()),
    });
    let mut registry = ProviderRegistry::new();
    registry.register_artwork(provider.clone());
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let providers = Arc::new(ProviderCredentialsService::with_resolver(
        resolver, registry,
    ));
    let settings = Arc::new(SettingsService::in_memory(""));
    let fetcher = Arc::new(StaticFetcher::new());
    for (n, url) in [
        (1, "https://img.test/show-poster.jpg"),
        (2, "https://img.test/specials.jpg"),
        (3, "https://img.test/s1.jpg"),
        (4, "https://img.test/s1-banner.jpg"),
        (5, "https://img.test/s1-fanart.jpg"),
        (6, "https://img.test/s2-stored.jpg"),
        (7, "https://img.test/e1.jpg"),
        (8, "https://img.test/e2.jpg"),
    ] {
        fetcher.insert(url, jpeg(n));
    }
    let thumbs = Arc::new(ThumbCache::open(&lab.dir.path().join("cache"), None).unwrap());
    let artwork = ArtworkService::with_fetcher(
        Arc::clone(&lab.db),
        providers,
        Arc::clone(&settings),
        thumbs,
        fetcher.clone(),
    );
    let show_dir = lab.dir.path().join("Shows").join("Severance (2022)");
    Rig {
        lab,
        settings,
        artwork,
        fetcher,
        provider,
        show_dir,
    }
}

struct Show {
    show: ItemId,
    seasons: Vec<(u32, ItemId)>,
    episodes: Vec<ItemId>,
}

impl Rig {
    /// A show with the specials, season 1 (two episodes) and season 2 (one episode), each in a
    /// folder of its own. Season 2 remembers an image URL from its scrape; the first episode
    /// remembers a thumb URL.
    fn show(&self) -> Show {
        let dir = &self.show_dir;
        let root = self.lab.dir.path().join("Shows");
        let mut show = Item::show("Severance");
        show.core.path = dir.to_string_lossy().into_owned();
        show.core.data_source = root.to_string_lossy().into_owned();
        show.core.ids.set("tvdb", "371980");
        let show_id = self.lab.add(show);

        let mut seasons = Vec::new();
        let mut episodes = Vec::new();
        for (number, folder, files) in [
            (0_u32, "Specials", vec!["Severance - S00E01 - Extra.mkv"]),
            (
                1,
                "Season 1",
                vec![
                    "Severance - S01E01 - Good News.mkv",
                    "Severance - S01E02 - Half Loop.mkv",
                ],
            ),
            (2, "Season 2", vec!["Severance - S02E01 - Hello.mkv"]),
        ] {
            let season_dir = dir.join(folder);
            fs::create_dir_all(&season_dir).unwrap();
            let mut season = Item::season(show_id, number);
            season.core.path = dir.to_string_lossy().into_owned();
            season.core.data_source = root.to_string_lossy().into_owned();
            if number == 2 {
                season.core.artwork_urls.insert(
                    MediaFileType::SeasonPoster,
                    "https://img.test/s2-stored.jpg".to_owned(),
                );
            }
            seasons.push((number, self.lab.add(season)));
            for (index, file) in files.iter().enumerate() {
                fs::write(season_dir.join(file), b"video").unwrap();
                let mut episode = Item::episode(show_id, *file);
                episode.core.media_files.push(MediaFile::new(
                    MediaFileType::Video,
                    season_dir.to_string_lossy(),
                    *file,
                ));
                if number == 1 {
                    episode.core.artwork_urls.insert(
                        MediaFileType::Thumb,
                        format!("https://img.test/e{}.jpg", index + 1),
                    );
                }
                if let ItemDetail::Episode(fields) = &mut episode.detail {
                    fields.numbers.set(EpisodeNumber::aired(
                        number,
                        u32::try_from(index).unwrap() + 1,
                    ));
                }
                episodes.push(self.lab.add(episode));
            }
        }
        Show {
            show: show_id,
            seasons,
            episodes,
        }
    }

    fn season(&self, show: &Show, number: u32) -> ItemId {
        show.seasons.iter().find(|(n, _)| *n == number).unwrap().1
    }

    fn sources(&self, item: ItemId) -> Vec<(MediaFileType, String)> {
        self.lab
            .db
            .read(|c| artwork_refs(c, item))
            .unwrap()
            .into_iter()
            .map(|r| (r.kind, r.url))
            .collect()
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

#[tokio::test]
async fn download_missing_writes_season_images_and_episode_thumbs() {
    let rig = rig();
    let show = rig.show();

    let summary = rig.artwork.download_missing(&[show.show], false).await;
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);
    assert!(summary.outcomes[0].problems.is_empty());

    // Each season is asked about separately, the specials included.
    let mut asked = rig.provider.asked.lock().unwrap().clone();
    asked.sort_unstable();
    assert_eq!(asked, [0, 1, 2]);

    // The default names put season images in the show folder; the specials have their own stem.
    assert_eq!(
        names(&rig.show_dir),
        [
            "Season 1",
            "Season 2",
            "Specials",
            "poster.jpg",
            "season-specials-poster.jpg",
            "season01-banner.jpg",
            "season01-fanart.jpg",
            "season01-poster.jpg",
            "season02-poster.jpg",
        ]
    );
    // The episode thumbs sit next to their videos under the thumb names.
    let season_one = names(&rig.show_dir.join("Season 1"));
    assert!(season_one.contains(&"Severance - S01E01 - Good News-thumb.jpg".to_owned()));
    assert!(season_one.contains(&"Severance - S01E02 - Half Loop-thumb.jpg".to_owned()));
    assert!(
        names(&rig.show_dir.join("Season 2")).len() == 1,
        "an episode without a thumb URL gets no thumb"
    );

    // Season 2 was unknown to the provider, so its remembered URL was used.
    assert_eq!(
        rig.sources(rig.season(&show, 2)),
        [(
            MediaFileType::SeasonPoster,
            "https://img.test/s2-stored.jpg".to_owned()
        )]
    );
    let one = rig.lab.item(rig.season(&show, 1));
    assert_eq!(
        one.core.artwork_urls[&MediaFileType::SeasonPoster],
        "https://img.test/s1.jpg"
    );
    assert!(
        one.core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::SeasonBanner && f.filename == "season01-banner.jpg"),
        "the season lists its files, so the pages can show them"
    );
    let episode = rig.lab.item(show.episodes[1]);
    assert_eq!(
        episode.core.artwork_urls[&MediaFileType::Thumb],
        "https://img.test/e1.jpg"
    );
    assert!(
        episode
            .core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::Thumb
                && f.filename == "Severance - S01E01 - Good News-thumb.jpg"),
        "the episode lists its thumb file"
    );
}

#[tokio::test]
async fn a_second_run_fetches_no_image_again_and_never_replaces_a_file() {
    let rig = rig();
    let show = rig.show();
    rig.artwork.download_missing(&[show.show], false).await;
    let poster = rig.show_dir.join("season01-poster.jpg");
    fs::write(&poster, b"my own poster").unwrap();
    let requests = rig.fetcher.requests().len();
    let summary = rig.artwork.download_missing(&[show.show], false).await;
    assert_eq!(summary.images(), 0);
    assert_eq!(fs::read(&poster).unwrap(), b"my own poster");
    assert_eq!(
        rig.fetcher.requests().len(),
        requests,
        "every image is there, so nothing is fetched"
    );
}

#[tokio::test]
async fn a_scanned_season_image_counts_and_only_the_gaps_are_filled() {
    let rig = rig();
    let show = rig.show();
    // The scan found the poster of season 1 under another name, and listed it.
    let own = rig.show_dir.join("Season 1").join("folder.png");
    fs::write(&own, b"scanned").unwrap();
    let mut season = rig.lab.item(rig.season(&show, 1));
    season.core.media_files.push(MediaFile::new(
        MediaFileType::SeasonPoster,
        own.parent().unwrap().to_string_lossy(),
        "folder.png",
    ));
    let season_id = rig.season(&show, 1);
    rig.lab
        .db
        .write(move |tx| {
            cv_db::repo::update_item(tx, &{
                let mut s = season;
                s.id = Some(season_id);
                s
            })
        })
        .unwrap();

    rig.artwork.download_missing(&[show.show], false).await;
    let names = names(&rig.show_dir);
    assert!(!names.contains(&"season01-poster.jpg".to_owned()));
    assert!(names.contains(&"season01-banner.jpg".to_owned()));
    assert_eq!(fs::read(&own).unwrap(), b"scanned");
}

#[tokio::test]
async fn the_season_names_follow_the_settings() {
    let rig = rig();
    let show = rig.show();
    rig.settings
        .set("artwork.names.tv.season-poster", json!(["SEASON_FOLDER"]))
        .expect("settings");
    rig.settings
        .set("artwork.names.tv.episode-thumb", json!(["FILENAME_TBN"]))
        .expect("settings");
    rig.artwork.download_missing(&[show.show], false).await;

    assert!(
        rig.show_dir
            .join("Season 1")
            .join("season01-poster.jpg")
            .is_file(),
        "SEASON_FOLDER puts the poster in the season folder"
    );
    assert!(!rig.show_dir.join("season01-poster.jpg").exists());
    assert!(
        rig.show_dir
            .join("Season 1")
            .join("Severance - S01E01 - Good News.tbn")
            .is_file()
    );
}

#[tokio::test]
async fn a_show_whose_folder_is_gone_gets_nothing() {
    let rig = rig();
    let show = rig.show();
    fs::remove_dir_all(&rig.show_dir).unwrap();
    let summary = rig.artwork.download_missing(&[show.show], false).await;
    assert_eq!(summary.images(), 0);
    assert!(!rig.show_dir.exists(), "no folder is created for it");
}

#[tokio::test]
async fn redownload_fetches_the_images_that_went_missing_from_the_remembered_sources() {
    let rig = rig();
    let show = rig.show();
    rig.artwork.download_missing(&[show.show], false).await;
    fs::remove_file(rig.show_dir.join("season01-banner.jpg")).unwrap();
    rig.provider.asked.lock().unwrap().clear();

    let summary = rig.artwork.redownload(&[show.show], false).await;
    assert!(summary.skipped.is_empty());
    assert!(rig.show_dir.join("season01-banner.jpg").is_file());
    assert!(
        rig.provider.asked.lock().unwrap().is_empty(),
        "a re-download uses the remembered sources, not the providers"
    );
}

// --- after a season's scrape, and a season image chosen by hand ---------------------------------

#[tokio::test]
async fn the_fill_after_a_season_scrape_writes_that_season_only() {
    let rig = rig();
    let show = rig.show();
    let one = rig.season(&show, 1);

    let outcome = rig
        .artwork
        .fill_tv_after_scrape(one, Some(1))
        .await
        .unwrap();
    assert!(outcome.images > 0, "{outcome:?}");
    let names = names(&rig.show_dir);
    assert!(names.contains(&"season01-poster.jpg".to_owned()));
    assert!(
        !names.contains(&"season-specials-poster.jpg".to_owned()),
        "another season is left for its own scrape"
    );
    assert_eq!(*rig.provider.asked.lock().unwrap(), [1]);
    // The episode thumbs of the season come with it.
    assert!(
        names_in(&rig.show_dir.join("Season 1"))
            .contains(&"Severance - S01E01 - Good News-thumb.jpg".to_owned())
    );

    // An episode stands for its own season: the specials episode fills the specials.
    rig.artwork
        .fill_tv_after_scrape(show.episodes[0], None)
        .await
        .unwrap();
    assert!(rig.show_dir.join("season-specials-poster.jpg").is_file());
    assert!(!rig.show_dir.join("season02-poster.jpg").exists());
    // The show stands for every season.
    rig.artwork
        .fill_tv_after_scrape(show.show, None)
        .await
        .unwrap();
    assert!(rig.show_dir.join("season02-poster.jpg").is_file());
}

#[tokio::test]
async fn the_fill_after_a_scrape_follows_the_test_switch_and_never_replaces() {
    let rig = rig();
    let show = rig.show();
    let one = rig.season(&show, 1);
    rig.artwork.set_after_scrape_runs(false);
    let outcome = rig
        .artwork
        .fill_tv_after_scrape(one, Some(1))
        .await
        .unwrap();
    assert_eq!(outcome.images, 0);
    assert!(!rig.show_dir.join("season01-poster.jpg").exists());

    rig.artwork.set_after_scrape_runs(true);
    fs::write(rig.show_dir.join("season01-poster.jpg"), b"my own poster").unwrap();
    rig.artwork
        .fill_tv_after_scrape(one, Some(1))
        .await
        .unwrap();
    assert_eq!(
        fs::read(rig.show_dir.join("season01-poster.jpg")).unwrap(),
        b"my own poster",
        "an image that is there is never replaced by the automatic step"
    );
}

fn names_in(folder: &Path) -> Vec<String> {
    names(folder)
}

#[tokio::test]
async fn the_picker_lists_season_images_and_pins_the_current_one() {
    let rig = rig();
    let show = rig.show();
    rig.artwork.download_missing(&[show.show], false).await;
    let one = rig.season(&show, 1);

    let stream = rig
        .artwork
        .candidate_stream(one, MediaFileType::Poster)
        .await
        .unwrap();
    assert_eq!(
        stream.slots,
        [
            MediaFileType::Poster,
            MediaFileType::Fanart,
            MediaFileType::Banner,
            MediaFileType::Thumb
        ],
        "the season types by their plain names"
    );
    assert!(stream.title.contains("Season 1"), "{}", stream.title);
    assert!(stream.current.is_some(), "the season poster is pinned");
    assert_eq!(stream.sources.len(), 1);
    let cv_app::artwork::stream::SourceAnswer::Candidates(found) = stream.sources[0].fetch().await
    else {
        panic!("the provider answers");
    };
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].url, "https://img.test/s1.jpg");

    // A movie type is not a season type.
    assert!(
        rig.artwork
            .candidate_stream(one, MediaFileType::Clearlogo)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_chosen_season_poster_replaces_the_old_one_and_undo_restores_it() {
    let rig = rig();
    let show = rig.show();
    rig.artwork.download_missing(&[show.show], false).await;
    let one = rig.season(&show, 1);
    let poster = rig.show_dir.join("season01-poster.jpg");
    let before = fs::read(&poster).unwrap();

    // The scan also found the season's poster under another name, in the season folder.
    let other = rig.show_dir.join("Season 1").join("folder.jpg");
    fs::write(&other, jpeg(40)).unwrap();
    let mut season = rig.lab.item(one);
    season.core.media_files.push(MediaFile::new(
        MediaFileType::SeasonPoster,
        other.parent().unwrap().to_string_lossy(),
        "folder.jpg",
    ));
    season.id = Some(one);
    rig.lab
        .db
        .write(move |tx| cv_db::repo::update_item(tx, &season))
        .unwrap();

    rig.fetcher.insert("https://img.test/chosen.jpg", jpeg(99));
    let outcome = rig
        .artwork
        .place(
            one,
            MediaFileType::Poster,
            "https://img.test/chosen.jpg",
            true,
        )
        .await
        .unwrap();
    assert_eq!(outcome.images, 1);
    assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
    assert_ne!(
        fs::read(&poster).unwrap(),
        before,
        "the new poster is in place"
    );
    assert!(
        !other.exists(),
        "the old poster under another name gave way"
    );
    let item = rig.lab.item(one);
    assert_eq!(
        item.core.artwork_urls[&MediaFileType::SeasonPoster],
        "https://img.test/chosen.jpg"
    );
    assert!(
        !item
            .core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::SeasonPoster && f.filename == "folder.jpg")
    );
    // Nothing was deleted: the old files are in the trash folder.
    let kept = found_named(rig.lab.dir.path(), "folder.jpg");
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(
        kept[0].starts_with(
            rig.lab
                .dir
                .path()
                .join("Shows")
                .join(cv_rename::exec::TRASH_DIR)
        ),
        "{kept:?}"
    );

    let token = outcome.undo.expect("an undo token");
    let undone = rig.artwork.undo_place(&token).await.unwrap();
    assert!(undone.problems.is_empty(), "{:?}", undone.problems);
    assert_eq!(fs::read(&poster).unwrap(), before, "the old poster is back");
    assert!(other.is_file(), "the other file is back too");
    let item = rig.lab.item(one);
    assert_eq!(
        item.core.artwork_urls[&MediaFileType::SeasonPoster],
        "https://img.test/s1.jpg"
    );
    assert!(
        item.core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::SeasonPoster && f.filename == "folder.jpg")
    );
}

#[tokio::test]
async fn a_chosen_season_image_follows_the_naming_settings() {
    let rig = rig();
    let show = rig.show();
    rig.settings
        .set("artwork.names.tv.season-poster", json!(["SEASON_FOLDER"]))
        .expect("settings");
    let two = rig.season(&show, 2);
    rig.fetcher.insert("https://img.test/chosen.jpg", jpeg(77));
    rig.artwork
        .place(
            two,
            MediaFileType::Poster,
            "https://img.test/chosen.jpg",
            true,
        )
        .await
        .unwrap();
    assert!(
        rig.show_dir
            .join("Season 2")
            .join("season02-poster.jpg")
            .is_file()
    );
    assert!(!rig.show_dir.join("season02-poster.jpg").exists());

    // Every artwork type keeps a name: the settings refuse to empty its list.
    let refused = rig
        .settings
        .set("artwork.names.tv.season-thumb", json!([]))
        .expect_err("an empty list is refused");
    assert_eq!(
        refused.to_string(),
        "Every artwork type is written; keep at least one name"
    );
    assert!(rig.settings.get("artwork.names.tv.season-thumb").is_none());
}

/// Every file called `name` below `dir`.
fn found_named(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == name) {
                found.push(path);
            }
        }
    }
    found
}

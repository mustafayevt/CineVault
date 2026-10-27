//! The artwork picker's backend: what it offers for a slot, how the candidates are
//! ordered and described, that they land in the thumbnail cache, and that a chosen set of extras
//! replaces the old one. Mock providers and an in-memory image fetcher, no network.

mod support;

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::artwork::{ArtworkError, ArtworkService, PickerOptions, source_url};
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_artwork::download::fetch::StaticFetcher;
use cv_artwork::thumb::{ThumbCache, ThumbKey, ThumbSize};
use cv_db::repo::artwork_refs::artwork_refs;
use cv_db::repo::{get_item, update_item};
use cv_domain::{Item, ItemId, MediaFile, MediaFileType, ProviderIds};
use cv_providers::model::{ArtworkCandidate, MediaKind, RequestOptions};
use cv_providers::{
    ArtworkProvider, CredentialResolver, MemoryUserCredentials, Provider, ProviderError,
    ProviderRegistry,
};
use image::{ImageFormat, Rgb, RgbImage};
use support::Lab;

fn jpeg(seed: u8, width: u32, height: u32) -> Vec<u8> {
    let image = RgbImage::from_pixel(width, height, Rgb([seed, 200, 10]));
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
        let image =
            |kind, url: &str, language: Option<&str>, width, height, score| ArtworkCandidate {
                kind,
                url: url.to_owned(),
                language: language.map(str::to_owned),
                width,
                height,
                score,
                popularity: cv_providers::model::Popularity::Unknown,
            };
        use MediaFileType::{Fanart, Poster};
        Ok(vec![
            // The provider's own score puts the Japanese poster first; the settings prefer English.
            image(
                Poster,
                "https://img.test/p-ja.jpg",
                Some("ja"),
                2000,
                3000,
                9.0,
            ),
            image(
                Poster,
                "https://img.test/p-en.jpg",
                Some("en"),
                2000,
                3000,
                7.0,
            ),
            image(Poster, "https://img.test/p-xx.jpg", Some("xx"), 0, 0, 5.0),
            image(
                Poster,
                "https://img.test/p-gone.jpg",
                Some("en"),
                2000,
                3000,
                8.0,
            ),
            image(Fanart, "https://img.test/f-1.jpg", None, 3840, 2160, 8.0),
            image(Fanart, "https://img.test/f-2.jpg", None, 1920, 1080, 6.0),
            image(
                Fanart,
                "https://img.test/f-3.jpg",
                Some("en"),
                1920,
                1080,
                5.0,
            ),
        ])
    }
}

struct Rig {
    lab: Lab,
    artwork: Arc<ArtworkService>,
    fetcher: Arc<StaticFetcher>,
    thumbs: Arc<ThumbCache>,
    library: PathBuf,
}

fn rig() -> Rig {
    let lab = Lab::new();
    let mut registry = ProviderRegistry::new();
    registry.register_artwork(Arc::new(Gallery));
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let providers = Arc::new(ProviderCredentialsService::with_resolver(
        resolver, registry,
    ));
    let settings = Arc::new(SettingsService::in_memory(""));
    let fetcher = Arc::new(StaticFetcher::new());
    fetcher.insert("https://img.test/p-ja.jpg", jpeg(1, 80, 120));
    fetcher.insert("https://img.test/p-en.jpg", jpeg(2, 80, 120));
    fetcher.insert("https://img.test/p-xx.jpg", jpeg(3, 100, 150));
    for n in 1..=3u8 {
        fetcher.insert(
            &format!("https://img.test/f-{n}.jpg"),
            jpeg(10 + n, 160, 90),
        );
    }
    let thumbs = Arc::new(ThumbCache::open(&lab.dir.path().join("cache"), None).unwrap());
    let artwork = ArtworkService::with_fetcher(
        Arc::clone(&lab.db),
        providers,
        settings,
        Arc::clone(&thumbs),
        fetcher.clone(),
    );
    let library = lab.dir.path().join("Movies");
    Rig {
        lab,
        artwork,
        fetcher,
        thumbs,
        library,
    }
}

impl Rig {
    /// A matched movie with a folder and a video on disk.
    fn movie(&self) -> (ItemId, PathBuf) {
        let folder = self.library.join("Dune Part Two (2024)");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("Dune Part Two (2024).mkv"), b"video").unwrap();
        let mut item = Item::movie("Dune Part Two");
        item.core.year = 2024;
        item.core.ids.set("tmdb", "693134");
        item.core.path = folder.to_string_lossy().into_owned();
        item.core.data_source = self.library.to_string_lossy().into_owned();
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            folder.to_string_lossy(),
            "Dune Part Two (2024).mkv",
        ));
        (self.lab.add(item), folder)
    }
}

fn urls(options: &PickerOptions) -> Vec<&str> {
    options.candidates.iter().map(|c| c.url.as_str()).collect()
}

#[tokio::test]
async fn the_picker_lists_the_candidates_best_fit_first_with_their_thumbnails_cached() {
    let rig = rig();
    let (item, _) = rig.movie();

    let options = rig
        .artwork
        .picker_options(item, MediaFileType::Poster)
        .await
        .unwrap();

    assert_eq!(options.title, "Dune Part Two");
    assert_eq!(options.year, 2024);
    assert_eq!(options.providers.len(), 1);
    assert_eq!(
        (
            options.providers[0].key.as_str(),
            options.providers[0].name.as_str()
        ),
        ("imdb", "Gallery")
    );
    assert!(options.unreachable.is_empty());
    // The English poster fits the settings and goes first even though the provider scores the
    // Japanese one higher; the rest follow by provider score; the poster that could not be
    // fetched is left out.
    assert_eq!(
        urls(&options),
        [
            "https://img.test/p-en.jpg",
            "https://img.test/p-ja.jpg",
            "https://img.test/p-xx.jpg",
        ]
    );
    let scores: Vec<u32> = options.candidates.iter().map(|c| c.score).collect();
    // Language and size fit for English; a 2000-pixel poster is bigger than "large" and counts as
    // that size; the Japanese one fits only the size, the one of unknown size nothing.
    assert_eq!(scores, [90, 40, 0]);
    let languages: Vec<Option<&str>> = options
        .candidates
        .iter()
        .map(|c| c.language.as_deref())
        .collect();
    assert_eq!(
        languages,
        [Some("en"), Some("ja"), None],
        "`xx` means no text"
    );
    // A provider that does not know the size is asked of the image itself.
    let sizes: Vec<(u32, u32)> = options
        .candidates
        .iter()
        .map(|c| (c.width, c.height))
        .collect();
    assert_eq!(sizes, [(2000, 3000), (2000, 3000), (100, 150)]);
    for candidate in &options.candidates {
        assert!(
            rig.thumbs.has(&candidate.key, ThumbSize::Grid),
            "{} is in the cache",
            candidate.url
        );
    }
    assert_eq!(
        options.candidates[0].key,
        ThumbKey::of_bytes(&jpeg(2, 80, 120))
    );
}

#[tokio::test]
async fn the_slots_are_the_types_of_the_item_and_the_extras() {
    let rig = rig();
    let (item, _) = rig.movie();

    let options = rig
        .artwork
        .picker_options(item, MediaFileType::Fanart)
        .await
        .unwrap();

    assert_eq!(options.slots[0], MediaFileType::Poster);
    assert_eq!(options.slots[1], MediaFileType::Fanart);
    assert!(options.slots.contains(&MediaFileType::Clearlogo));
    assert_eq!(
        &options.slots[options.slots.len() - 2..],
        [MediaFileType::Extrafanart, MediaFileType::Extrathumb]
    );
    assert_eq!(
        urls(&options),
        [
            "https://img.test/f-1.jpg",
            "https://img.test/f-3.jpg",
            "https://img.test/f-2.jpg"
        ],
        "the best textless fanart, the 4K one, is moved to the front, then the automatic order"
    );
}

#[tokio::test]
async fn extra_fanart_is_chosen_from_the_fanart_candidates() {
    let rig = rig();
    let (item, _) = rig.movie();

    let options = rig
        .artwork
        .picker_options(item, MediaFileType::Extrafanart)
        .await
        .unwrap();

    assert_eq!(options.candidates.len(), 3);
    assert!(options.candidates[0].url.contains("/f-"));
}

#[tokio::test]
async fn a_type_the_item_cannot_have_is_refused() {
    let rig = rig();
    let (item, _) = rig.movie();

    let error = rig
        .artwork
        .picker_options(item, MediaFileType::Characterart)
        .await
        .unwrap_err();

    assert!(matches!(error, ArtworkError::UnknownSlot), "{error}");
}

#[tokio::test]
async fn opening_the_picker_again_fetches_nothing_again() {
    let rig = rig();
    let (item, _) = rig.movie();
    rig.artwork
        .picker_options(item, MediaFileType::Poster)
        .await
        .unwrap();
    let first = rig.fetcher.requests().len();
    assert!(first >= 3);

    rig.artwork
        .picker_options(item, MediaFileType::Poster)
        .await
        .unwrap();

    // Only the image that could not be fetched is asked for again.
    assert_eq!(rig.fetcher.requests().len(), first + 1);
}

#[tokio::test]
async fn an_item_without_provider_ids_has_nothing_to_offer() {
    let rig = rig();
    let folder = rig.library.join("Unknown (2001)");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("Unknown (2001).mkv"), b"video").unwrap();
    let mut item = Item::movie("Unknown");
    item.core.path = folder.to_string_lossy().into_owned();
    item.core.data_source = rig.library.to_string_lossy().into_owned();
    item.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        folder.to_string_lossy(),
        "Unknown (2001).mkv",
    ));
    let id = rig.lab.add(item);

    let options = rig
        .artwork
        .picker_options(id, MediaFileType::Poster)
        .await
        .unwrap();

    assert!(options.candidates.is_empty() && options.providers.is_empty());
}

#[tokio::test]
async fn a_chosen_poster_replaces_the_old_one_and_remembers_its_source() {
    let rig = rig();
    let (item, folder) = rig.movie();
    fs::write(
        folder.join("Dune Part Two (2024)-poster.jpg"),
        jpeg(99, 40, 60),
    )
    .unwrap();

    let outcome = rig
        .artwork
        .place(
            item,
            MediaFileType::Poster,
            "https://img.test/p-en.jpg",
            true,
        )
        .await
        .unwrap();

    assert_eq!(outcome.images, 1, "{:?}", outcome.problems);
    assert_eq!(
        fs::read(folder.join("Dune Part Two (2024)-poster.jpg")).unwrap(),
        jpeg(2, 80, 120)
    );
    let remembered = rig.lab.db.read(|c| artwork_refs(c, item)).unwrap();
    assert!(
        remembered
            .iter()
            .any(|r| r.kind == MediaFileType::Poster && r.url == "https://img.test/p-en.jpg")
    );
}

#[tokio::test]
async fn a_chosen_poster_replaces_one_under_another_name_and_the_undo_brings_it_back() {
    let rig = rig();
    let (item, folder) = rig.movie();
    // The poster has a name the settings do not write; the library lists it.
    let legacy = folder.join("poster.jpg");
    fs::write(&legacy, jpeg(99, 40, 60)).unwrap();
    let mut movie = rig.lab.db.read(|c| get_item(c, item)).unwrap().unwrap();
    movie.core.media_files.push(MediaFile::new(
        MediaFileType::Poster,
        folder.to_string_lossy(),
        "poster.jpg",
    ));
    rig.lab.db.write(move |tx| update_item(tx, &movie)).unwrap();

    let outcome = rig
        .artwork
        .place(
            item,
            MediaFileType::Poster,
            "https://img.test/p-en.jpg",
            true,
        )
        .await
        .unwrap();

    assert_eq!(outcome.images, 1, "{:?}", outcome.problems);
    assert!(!legacy.exists(), "the old poster went to the trash");
    let posters = |id| -> Vec<String> {
        rig.lab
            .db
            .read(|c| get_item(c, id))
            .unwrap()
            .unwrap()
            .core
            .media_files
            .iter()
            .filter(|f| f.kind == MediaFileType::Poster)
            .map(|f| f.filename.clone())
            .collect()
    };
    assert_eq!(posters(item), ["Dune Part Two (2024)-poster.jpg"]);

    rig.artwork
        .undo_place(&outcome.undo.unwrap())
        .await
        .unwrap();
    assert_eq!(fs::read(&legacy).unwrap(), jpeg(99, 40, 60));
    assert_eq!(posters(item), ["poster.jpg"]);
}

#[tokio::test]
async fn chosen_extras_replace_the_set_and_are_remembered_in_order() {
    let rig = rig();
    let (item, folder) = rig.movie();
    for n in 1..=3 {
        fs::write(
            folder.join(format!("Dune Part Two (2024)-fanart{n}.jpg")),
            jpeg(50 + n, 40, 20),
        )
        .unwrap();
    }

    let outcome = rig
        .artwork
        .place_extras(
            item,
            MediaFileType::Extrafanart,
            &[
                "https://img.test/f-3.jpg".to_owned(),
                "https://img.test/f-1.jpg".to_owned(),
            ],
        )
        .await
        .unwrap();

    assert_eq!(outcome.images, 2, "{:?}", outcome.problems);
    assert_eq!(outcome.trashed, 3, "the old set went to the trash");
    assert_eq!(
        fs::read(folder.join("Dune Part Two (2024)-fanart1.jpg")).unwrap(),
        jpeg(13, 160, 90)
    );
    assert_eq!(
        fs::read(folder.join("Dune Part Two (2024)-fanart2.jpg")).unwrap(),
        jpeg(11, 160, 90)
    );
    assert!(!folder.join("Dune Part Two (2024)-fanart3.jpg").exists());
    let remembered = rig.lab.db.read(|c| artwork_refs(c, item)).unwrap();
    let mut extras: Vec<(u32, &str)> = remembered
        .iter()
        .filter(|r| r.kind == MediaFileType::Extrafanart)
        .map(|r| (r.position, r.url.as_str()))
        .collect();
    extras.sort_unstable();
    assert_eq!(
        extras,
        [
            (1, "https://img.test/f-3.jpg"),
            (2, "https://img.test/f-1.jpg")
        ]
    );
}

#[tokio::test]
async fn undoing_a_replaced_poster_brings_the_old_file_and_its_source_back() {
    let rig = rig();
    let (item, folder) = rig.movie();
    let poster = folder.join("Dune Part Two (2024)-poster.jpg");
    fs::write(&poster, jpeg(99, 40, 60)).unwrap();

    let outcome = rig
        .artwork
        .place(
            item,
            MediaFileType::Poster,
            "https://img.test/p-en.jpg",
            true,
        )
        .await
        .unwrap();
    let token = outcome.undo.expect("a placement that wrote can be undone");
    assert_eq!(fs::read(&poster).unwrap(), jpeg(2, 80, 120));

    let undone = rig.artwork.undo_place(&token).await.unwrap();

    assert!(undone.problems.is_empty(), "{:?}", undone.problems);
    assert_eq!(fs::read(&poster).unwrap(), jpeg(99, 40, 60));
    // The chosen image was moved to the trash folder, not deleted.
    let trash = rig.library.join(".deletedByCineVault");
    let mut kept = 0;
    for entry in walk(&trash) {
        if fs::read(&entry).is_ok_and(|b| b == jpeg(2, 80, 120)) {
            kept += 1;
        }
    }
    assert_eq!(kept, 1, "the replacement waits in the trash folder");
    let remembered = rig.lab.db.read(|c| artwork_refs(c, item)).unwrap();
    assert!(
        remembered.iter().all(|r| r.kind != MediaFileType::Poster),
        "the source of the undone image is forgotten"
    );
    // The token works once.
    assert!(matches!(
        rig.artwork.undo_place(&token).await,
        Err(ArtworkError::UndoGone)
    ));
}

/// Review 14.7: the undo of a placement holds the title like the placement (no move or rename
/// runs meanwhile), and once the title was renamed or moved it no longer puts files back into
/// the folder the title left: it made that folder again and split the title. The old image
/// stays in the trash folder.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_undo_of_a_placement_waits_for_a_rename_and_never_makes_the_old_folder_again() {
    use cv_app::title_guard::{self, Work};
    use std::time::Duration;

    let rig = rig();
    let (item, folder) = rig.movie();
    let poster = folder.join("Dune Part Two (2024)-poster.jpg");
    fs::write(&poster, jpeg(99, 40, 60)).unwrap();
    let place = |rig: &Rig| {
        let artwork = Arc::clone(&rig.artwork);
        async move {
            artwork
                .place(
                    item,
                    MediaFileType::Poster,
                    "https://img.test/p-en.jpg",
                    true,
                )
                .await
                .unwrap()
                .undo
                .unwrap()
        }
    };

    // An undo asked while the title is renamed waits for the rename.
    let token = place(&rig).await;
    let renaming = title_guard::hold(&rig.lab.db, [item], Work::Rename, Duration::ZERO).unwrap();
    let undo = {
        let artwork = Arc::clone(&rig.artwork);
        tokio::spawn(async move { artwork.undo_place(&token).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        fs::read(&poster).unwrap() == jpeg(2, 80, 120),
        "nothing is put back while the title is renamed"
    );
    drop(renaming);
    let undone = undo.await.unwrap().unwrap();
    assert!(undone.problems.is_empty(), "{:?}", undone.problems);
    assert!(
        fs::read(&poster).unwrap() == jpeg(99, 40, 60),
        "the old poster is back"
    );

    // After a rename of the folder the undo leaves the new folder as it is.
    let token = place(&rig).await;
    let renamed = folder.with_file_name("Dune Part Two (2024) renamed");
    fs::rename(&folder, &renamed).unwrap();
    rig.lab
        .db
        .write({
            let old = folder.to_string_lossy().into_owned();
            let new = renamed.to_string_lossy().into_owned();
            move |tx| {
                let mut stored = cv_db::repo::get_item(tx, item)?.unwrap();
                stored.core.path.clone_from(&new);
                for file in &mut stored.core.media_files {
                    file.directory = file.directory.replace(&old, &new);
                }
                cv_db::repo::update_item(tx, &stored)
            }
        })
        .unwrap();
    let refused = rig.artwork.undo_place(&token).await;
    assert!(refused.is_err(), "{refused:?}");
    assert!(!folder.exists(), "the old folder is not made again");
    assert!(
        fs::read(renamed.join("Dune Part Two (2024)-poster.jpg")).unwrap() == jpeg(2, 80, 120),
        "the chosen poster stays in the renamed folder"
    );
    let trash = rig.library.join(".deletedByCineVault");
    assert!(
        walk(&trash)
            .iter()
            .any(|f| fs::read(f).is_ok_and(|b| b == jpeg(99, 40, 60))),
        "the earlier poster waits in the trash folder"
    );
}

#[tokio::test]
async fn undoing_a_poster_set_on_an_empty_slot_leaves_the_slot_empty() {
    let rig = rig();
    let (item, folder) = rig.movie();
    let outcome = rig
        .artwork
        .place(
            item,
            MediaFileType::Poster,
            "https://img.test/p-en.jpg",
            true,
        )
        .await
        .unwrap();
    assert!(folder.join("Dune Part Two (2024)-poster.jpg").exists());

    rig.artwork
        .undo_place(&outcome.undo.unwrap())
        .await
        .unwrap();

    assert!(!folder.join("Dune Part Two (2024)-poster.jpg").exists());
}

#[tokio::test]
async fn undoing_chosen_extras_brings_back_the_old_set() {
    let rig = rig();
    let (item, folder) = rig.movie();
    for n in 1..=3 {
        fs::write(
            folder.join(format!("Dune Part Two (2024)-fanart{n}.jpg")),
            jpeg(50 + n, 40, 20),
        )
        .unwrap();
    }
    let outcome = rig
        .artwork
        .place_extras(
            item,
            MediaFileType::Extrafanart,
            &["https://img.test/f-3.jpg".to_owned()],
        )
        .await
        .unwrap();
    assert!(!folder.join("Dune Part Two (2024)-fanart3.jpg").exists());

    rig.artwork
        .undo_place(&outcome.undo.unwrap())
        .await
        .unwrap();

    for n in 1..=3 {
        assert_eq!(
            fs::read(folder.join(format!("Dune Part Two (2024)-fanart{n}.jpg"))).unwrap(),
            jpeg(50 + n, 40, 20),
            "fanart{n} is back as it was"
        );
    }
}

#[tokio::test]
async fn a_placement_that_wrote_nothing_has_nothing_to_undo() {
    let rig = rig();
    let (item, _folder) = rig.movie();
    let outcome = rig
        .artwork
        .place(
            item,
            MediaFileType::Poster,
            "https://img.test/missing.jpg",
            true,
        )
        .await
        .unwrap();
    assert_eq!(outcome.images, 0);
    assert!(outcome.undo.is_none());
}

/// Every file below `dir`.
fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn a_source_is_a_web_address_a_file_address_or_a_path_on_this_computer() {
    assert_eq!(
        source_url("  https://img.test/a.jpg ").unwrap(),
        "https://img.test/a.jpg"
    );
    assert_eq!(
        source_url("file:///C:/Art/a.png").unwrap(),
        "file:///C:/Art/a.png"
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("my poster.jpg");
    let quoted = format!("\"{}\"", path.display());
    let url = source_url(&quoted).unwrap();
    assert!(
        url.starts_with("file://") && url.ends_with("my%20poster.jpg"),
        "{url}"
    );
    assert!(matches!(
        source_url("poster.jpg"),
        Err(ArtworkError::BadSource(_))
    ));
    assert!(matches!(source_url(""), Err(ArtworkError::BadSource(_))));
    assert!(matches!(
        source_url("ftp://host/a.jpg"),
        Err(ArtworkError::BadSource(_))
    ));
}

#[tokio::test]
async fn remote_thumbs_download_candidate_posters_into_the_cache_and_share_them_with_the_picker() {
    let rig = rig();

    let found = rig
        .artwork
        .remote_thumbs(
            vec![
                "https://img.test/p-en.jpg".to_owned(),
                "https://img.test/p-en.jpg".to_owned(),
                "https://img.test/p-gone.jpg".to_owned(),
                String::new(),
            ],
            std::time::Duration::from_secs(30),
        )
        .await;

    // Only the poster that can be fetched gets a key, and its thumbnail is in the cache.
    assert_eq!(found.len(), 1, "{found:?}");
    let key = &found["https://img.test/p-en.jpg"];
    assert!(rig.thumbs.meta(key).is_some());
    assert!(rig.thumbs.has(key, ThumbSize::Grid));

    // The same address is not fetched again, here or by the picker.
    let before = rig.fetcher.requests().len();
    let again = rig
        .artwork
        .remote_thumbs(
            vec!["https://img.test/p-en.jpg".to_owned()],
            std::time::Duration::from_secs(30),
        )
        .await;
    assert_eq!(again["https://img.test/p-en.jpg"], *key);
    assert_eq!(rig.fetcher.requests().len(), before);
}

#[tokio::test]
async fn remote_thumbs_with_nothing_to_ask_return_nothing() {
    let rig = rig();
    let found = rig
        .artwork
        .remote_thumbs(Vec::new(), std::time::Duration::from_millis(10))
        .await;
    assert!(found.is_empty());
    assert!(rig.fetcher.requests().is_empty());
}

#[tokio::test]
async fn a_poster_from_a_file_on_this_computer_stays_out_of_the_nfo() {
    let rig = rig();
    let (item, folder) = rig.movie();
    let mut movie = rig
        .lab
        .db
        .read(move |c| get_item(c, item))
        .unwrap()
        .unwrap();
    movie.core.artwork_urls.insert(
        MediaFileType::Poster,
        "https://img.test/p-en.jpg".to_owned(),
    );
    rig.lab.db.write(move |tx| update_item(tx, &movie)).unwrap();

    let own = rig.lab.dir.path().join("qa").join("new-poster-magenta.png");
    fs::create_dir_all(own.parent().unwrap()).unwrap();
    fs::write(&own, jpeg(7, 80, 120)).unwrap();
    let url = source_url(&own.to_string_lossy()).unwrap();
    rig.fetcher.insert(&url, jpeg(7, 80, 120));
    let outcome = rig
        .artwork
        .place(item, MediaFileType::Poster, &url, true)
        .await
        .unwrap();
    assert_eq!(outcome.images, 1, "{:?}", outcome.problems);

    let nfo =
        cv_app::nfo::NfoService::new(Arc::clone(&rig.lab.db), rig.lab.dir.path().join("undo"));
    let preview = nfo
        .preview_movie(item, &cv_app::nfo::NfoSettings::default())
        .unwrap()
        .unwrap();
    assert!(!preview.text.contains("file:"), "{}", preview.text);
    assert!(!preview.text.contains("p-en.jpg"), "{}", preview.text);
    assert!(
        folder.join("Dune Part Two (2024)-poster.jpg").exists()
            || folder.join("poster.jpg").exists()
    );
}

#[tokio::test]
async fn placing_a_poster_and_undoing_it_announce_the_artwork_and_files_that_changed() {
    use std::sync::Mutex;
    use std::time::Duration;

    use cv_app::changes::{ChangeHub, ChangePart, ItemsChanged};

    let rig = rig();
    let (item, _folder) = rig.movie();
    let hub = ChangeHub::new(Duration::ZERO);
    let seen = Arc::new(Mutex::new(Vec::<ItemsChanged>::new()));
    let sink = Arc::clone(&seen);
    hub.subscribe(Arc::new(move |change| {
        sink.lock().unwrap().push(change.clone())
    }));
    rig.artwork.set_changes(hub);

    let outcome = rig
        .artwork
        .place(
            item,
            MediaFileType::Poster,
            "https://img.test/p-en.jpg",
            true,
        )
        .await
        .unwrap();
    let expected = ItemsChanged {
        items: vec![item],
        parts: vec![ChangePart::Artwork, ChangePart::Files],
    };
    assert_eq!(*seen.lock().unwrap(), vec![expected.clone()]);

    rig.artwork
        .undo_place(&outcome.undo.unwrap())
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![expected.clone(), expected]);
}

/// A user report: after a backdrop was chosen in the picker, the NFO still named the old
/// backdrop's address. A chosen image, and its Undo, now write the NFO again.
#[tokio::test]
async fn a_chosen_image_and_its_undo_update_the_address_in_the_nfo() {
    use cv_app::nfo::{NfoScope, NfoService, NfoSettings};

    let rig = rig();
    let (item, folder) = rig.movie();
    let nfo = Arc::new(NfoService::new(
        Arc::clone(&rig.lab.db),
        rig.lab.dir.path().join("nfo-undo"),
    ));
    rig.artwork.set_nfo(Arc::clone(&nfo));
    let nfo_file = folder.join("Dune Part Two (2024).nfo");

    // Without an NFO, choosing a picture does not make one.
    rig.artwork
        .place(
            item,
            MediaFileType::Fanart,
            "https://img.test/f-1.jpg",
            true,
        )
        .await
        .unwrap();
    assert!(!nfo_file.exists(), "no NFO is made by choosing a picture");

    // The title gets its NFO (a scrape writes it), naming the first backdrop.
    nfo.rewrite(&NfoScope::Items(vec![item]), &NfoSettings::default())
        .unwrap();
    let written = fs::read_to_string(&nfo_file).unwrap();
    assert!(written.contains("https://img.test/f-1.jpg"), "{written}");

    let outcome = rig
        .artwork
        .place(
            item,
            MediaFileType::Fanart,
            "https://img.test/f-2.jpg",
            true,
        )
        .await
        .unwrap();
    let after = fs::read_to_string(&nfo_file).unwrap();
    assert!(after.contains("https://img.test/f-2.jpg"), "{after}");
    assert!(!after.contains("https://img.test/f-1.jpg"), "{after}");

    rig.artwork
        .undo_place(&outcome.undo.expect("undo token"))
        .await
        .unwrap();
    let undone = fs::read_to_string(&nfo_file).unwrap();
    assert!(undone.contains("https://img.test/f-1.jpg"), "{undone}");
    assert!(!undone.contains("https://img.test/f-2.jpg"), "{undone}");
}

/// A placement is a History entry; its Undo works after a restart (a new service over
/// the same library) and marks the entry undone.
#[tokio::test]
async fn a_placement_is_a_history_entry_that_can_be_undone_after_a_restart() {
    use cv_app::history::{HistoryFilter, HistoryService, UndoState};

    let rig = rig();
    let (item, folder) = rig.movie();
    let poster = folder.join("Dune Part Two (2024)-poster.jpg");
    fs::write(&poster, jpeg(99, 40, 60)).unwrap();
    let outcome = rig
        .artwork
        .place(
            item,
            MediaFileType::Poster,
            "https://img.test/p-en.jpg",
            true,
        )
        .await
        .unwrap();
    let token = outcome.undo.expect("an entry");

    let history = HistoryService::new(Arc::clone(&rig.lab.db));
    let listed = history
        .list(HistoryFilter::ArtworkAndMatches, 0, 10)
        .unwrap();
    assert_eq!(listed.entries.len(), 1);
    assert_eq!(listed.entries[0].summary, "Changed poster · Dune Part Two");
    assert_eq!(listed.entries[0].id.to_string(), token);

    // The app restarts: nothing is in memory any more.
    let restarted = ArtworkService::with_fetcher(
        Arc::clone(&rig.lab.db),
        Arc::new(ProviderCredentialsService::with_resolver(
            CredentialResolver::new(Arc::new(MemoryUserCredentials::default())),
            ProviderRegistry::new(),
        )),
        Arc::new(SettingsService::in_memory("")),
        Arc::clone(&rig.thumbs),
        rig.fetcher.clone(),
    );
    let report = restarted
        .undo_entry(listed.entries[0].id, None)
        .await
        .unwrap();
    assert_eq!(report.state, UndoState::Undone);
    assert_eq!(report.undone, vec![item]);
    assert_eq!(fs::read(&poster).unwrap(), jpeg(99, 40, 60));
    assert!(restarted.undo_place(&token).await.is_err(), "works once");
    let after = history.entry(listed.entries[0].id).unwrap();
    assert_eq!(after.undo_state, UndoState::Undone);
    assert!(after.undone_at.is_some());
}

/// A later change of the same title wins: the placement is not taken back silently.
#[tokio::test]
async fn an_older_placement_is_not_undone_before_a_newer_one() {
    use cv_app::history::HistoryFilter;
    use cv_app::history::HistoryService;

    let rig = rig();
    let (item, folder) = rig.movie();
    fs::write(
        folder.join("Dune Part Two (2024)-poster.jpg"),
        jpeg(99, 40, 60),
    )
    .unwrap();
    for url in ["https://img.test/p-en.jpg", "https://img.test/p-ja.jpg"] {
        rig.artwork
            .place(item, MediaFileType::Poster, url, true)
            .await
            .unwrap();
    }
    let history = HistoryService::new(Arc::clone(&rig.lab.db));
    let listed = history
        .list(HistoryFilter::ArtworkAndMatches, 0, 10)
        .unwrap();
    let (newer, older) = (listed.entries[0].id, listed.entries[1].id);
    let report = rig.artwork.undo_entry(older, None).await.unwrap();
    assert!(report.undone.is_empty());
    assert!(
        report.left[0].reason.starts_with("Undo \""),
        "{:?}",
        report.left
    );
    let report = rig.artwork.undo_entry(newer, None).await.unwrap();
    assert_eq!(report.undone, vec![item]);
}

#[tokio::test]
async fn the_full_image_of_a_candidate_is_kept_in_the_cache_and_fetched_once() {
    let rig = rig();
    let url = "https://img.test/p-en.jpg";
    let mut original = jpeg(2, 80, 120);
    // Not the thumbnail's bytes: a bigger original at the same address.
    original.extend_from_slice(b"padding that changes the hash");
    rig.fetcher.insert(url, original.clone());

    let first = rig.artwork.full_image(url).await.unwrap();

    assert_eq!((first.width, first.height), (80, 120));
    let path = rig.artwork.full_image_path(&first.key).unwrap();
    assert!(path.starts_with(rig.thumbs.root()), "{}", path.display());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(rig.thumbs.has(&first.key, ThumbSize::Grid));
    assert_eq!(rig.fetcher.requests(), vec![url.to_owned()]);

    // The second call answers from the cache and asks nobody.
    let second = rig.artwork.full_image(url).await.unwrap();
    assert_eq!(second, first);
    assert_eq!(rig.fetcher.requests().len(), 1);

    // An address that cannot be fetched, and one that is not a picture, are errors, not keys.
    assert!(matches!(
        rig.artwork.full_image("https://img.test/none.jpg").await,
        Err(ArtworkError::Download(_))
    ));
    rig.fetcher
        .insert("https://img.test/text.jpg", b"hello".to_vec());
    assert!(matches!(
        rig.artwork.full_image("https://img.test/text.jpg").await,
        Err(ArtworkError::Download(_))
    ));
    assert!(matches!(
        rig.artwork.full_image("not an address").await,
        Err(ArtworkError::BadSource(_))
    ));
}

#[tokio::test]
async fn a_tmdb_candidate_is_fetched_at_its_original_size() {
    let rig = rig();
    let small = "https://image.test/t/p/w500/abc.jpg";
    let big = "https://image.test/t/p/original/abc.jpg";
    rig.fetcher.insert(small, jpeg(4, 50, 75));
    rig.fetcher.insert(big, jpeg(5, 400, 600));

    let full = rig.artwork.full_image(small).await.unwrap();

    assert_eq!((full.width, full.height), (400, 600));
    assert_eq!(rig.fetcher.requests(), vec![big.to_owned()]);
}

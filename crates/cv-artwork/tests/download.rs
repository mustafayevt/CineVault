//! File-tree tests of the artwork downloads: download missing, extra fanart and
//! extra thumbs, actor portraits, remembered source URLs and re-download.
//!
//! Every test builds a movie or show folder in a temporary directory, serves the images from
//! memory and compares the whole tree of files afterwards.

use std::collections::BTreeSet;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_artwork::download::actors::ACTORS_DIR;
use cv_artwork::download::fetch::{HttpImageFetcher, StaticFetcher};
use cv_artwork::download::record::record;
use cv_artwork::download::{
    ArtworkConfig, ArtworkDownloader, ArtworkTarget, DownloadReport, DownloadRequest, Subject,
    present_artwork,
};
use cv_artwork::frame_source::file_url;
use cv_artwork::naming::FolderKind;
use cv_artwork::thumb::{ThumbCache, ThumbKey, ThumbSize};
use cv_db::repo::artwork_refs::{ArtworkRef, artwork_refs};
use cv_db::repo::{get_item, insert_item};
use cv_db::{Database, DbOptions};
use cv_domain::{Credit, CreditKind, Item, MediaFileType};
use cv_providers::SourcedArtwork;
use cv_providers::model::ArtworkCandidate;
use cv_rename::exec::TRASH_DIR;
use image::{ImageFormat, Rgb, RgbImage};
use serde_json::{Value, json};

// ---- helpers ---------------------------------------------------------------------------------

fn picture(format: ImageFormat, width: u32, height: u32, seed: u8) -> Vec<u8> {
    let image = RgbImage::from_pixel(width, height, Rgb([seed, 255 - seed, seed / 2]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, format).expect("encode");
    out.into_inner()
}

fn jpeg(seed: u8) -> Vec<u8> {
    picture(ImageFormat::Jpeg, 64, 96, seed)
}

fn png(seed: u8) -> Vec<u8> {
    picture(ImageFormat::Png, 64, 64, seed)
}

fn candidate(kind: MediaFileType, url: &str, score: f32) -> SourcedArtwork {
    SourcedArtwork {
        provider: "tmdb",
        candidate: ArtworkCandidate {
            kind,
            url: url.to_owned(),
            language: Some("en".to_owned()),
            width: 2000,
            height: 3000,
            score,
            popularity: cv_providers::model::Popularity::Unknown,
        },
    }
}

/// Every file below `root`, relative, with `/` separators, sorted. The trash is shown as
/// `<trash>/...` so tests do not depend on the batch name.
fn tree(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    let mut out: Vec<String> = out
        .into_iter()
        .map(|rel| match rel.strip_prefix(&format!("{TRASH_DIR}/")) {
            Some(rest) => {
                let below_batch = rest.split_once('/').map_or(rest, |(_, r)| r);
                format!("<trash>/{below_batch}")
            }
            None => rel,
        })
        .collect();
    out.sort();
    out
}

/// A movie folder in a data source, with the downloader's inputs.
struct Case {
    _dir: tempfile::TempDir,
    root: PathBuf,
    movie: PathBuf,
    target: ArtworkTarget,
    config: ArtworkConfig,
    candidates: Vec<SourcedArtwork>,
    recorded: Vec<ArtworkRef>,
    present: BTreeSet<MediaFileType>,
    cast: Vec<Credit>,
    fetcher: Arc<StaticFetcher>,
}

impl Case {
    fn movie(settings: &[(&str, Value)]) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("Movies");
        let movie = root.join("Heat (1995)");
        fs::create_dir_all(&movie).unwrap();
        fs::write(movie.join("Heat (1995).mkv"), b"video").unwrap();
        let target = ArtworkTarget {
            subject: Subject::Movie,
            dir: movie.clone(),
            root: movie.clone(),
            base: "Heat (1995)".to_owned(),
            folder: FolderKind::Single,
            data_source: Some(root.clone()),
        };
        Self::with(dir, root, movie, target, settings)
    }

    fn show(settings: &[(&str, Value)]) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("Shows");
        let show = root.join("Severance");
        fs::create_dir_all(&show).unwrap();
        let target = ArtworkTarget {
            subject: Subject::Show,
            dir: show.clone(),
            root: show.clone(),
            base: String::new(),
            folder: FolderKind::Single,
            data_source: Some(root.clone()),
        };
        Self::with(dir, root, show, target, settings)
    }

    fn with(
        dir: tempfile::TempDir,
        root: PathBuf,
        movie: PathBuf,
        target: ArtworkTarget,
        settings: &[(&str, Value)],
    ) -> Self {
        let values = settings
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect();
        Self {
            _dir: dir,
            root,
            movie,
            target,
            config: ArtworkConfig::from_values(&values),
            candidates: Vec::new(),
            recorded: Vec::new(),
            present: BTreeSet::new(),
            cast: Vec::new(),
            fetcher: Arc::new(StaticFetcher::new()),
        }
    }

    /// Offers an image at `url`.
    fn offer(&mut self, kind: MediaFileType, url: &str, score: f32, bytes: Vec<u8>) {
        self.fetcher.insert(url, bytes);
        self.candidates.push(candidate(kind, url, score));
    }

    fn downloader(&self) -> ArtworkDownloader {
        ArtworkDownloader::new(self.fetcher.clone())
    }

    fn request(&self, replace: bool) -> DownloadRequest<'_> {
        DownloadRequest {
            target: &self.target,
            config: &self.config,
            candidates: &self.candidates,
            recorded: &self.recorded,
            present: &self.present,
            cast: &self.cast,
            replace,
        }
    }

    async fn download_missing(&self) -> DownloadReport {
        self.downloader()
            .download_missing(&self.request(false))
            .await
    }

    fn tree(&self) -> Vec<String> {
        tree(&self.root)
    }

    fn read(&self, rel: &str) -> Vec<u8> {
        fs::read(self.movie.join(rel)).unwrap()
    }
}

fn actor(name: &str, url: &str) -> Credit {
    let mut credit = Credit::new(CreditKind::Actor, name, "");
    credit.person.thumb_url = url.to_owned();
    credit
}

// ---- download missing ------------------------------------------------------------------------

#[tokio::test]
async fn only_the_empty_types_are_filled_and_the_poster_is_untouched() {
    let mut case = Case::movie(&[]);
    let poster = jpeg(1);
    fs::write(case.movie.join("Heat (1995)-poster.jpg"), &poster).unwrap();
    case.present = BTreeSet::from([MediaFileType::Poster]);
    case.offer(
        MediaFileType::Poster,
        "https://i.test/poster.jpg",
        9.0,
        jpeg(2),
    );
    case.offer(
        MediaFileType::Fanart,
        "https://i.test/fanart.jpg",
        9.0,
        jpeg(3),
    );
    case.offer(
        MediaFileType::Clearlogo,
        "https://i.test/logo.png",
        9.0,
        png(4),
    );
    case.offer(
        MediaFileType::Thumb,
        "https://i.test/thumb.jpg",
        9.0,
        jpeg(5),
    );

    let report = case.download_missing().await;

    assert_eq!(
        case.tree(),
        [
            "Heat (1995)/Heat (1995)-clearlogo.png",
            "Heat (1995)/Heat (1995)-fanart.jpg",
            "Heat (1995)/Heat (1995)-landscape.jpg",
            "Heat (1995)/Heat (1995)-poster.jpg",
            "Heat (1995)/Heat (1995).mkv",
        ]
    );
    assert_eq!(
        case.read("Heat (1995)-poster.jpg"),
        poster,
        "the poster was not touched"
    );
    assert!(
        !case
            .fetcher
            .requests()
            .contains(&"https://i.test/poster.jpg".to_owned()),
        "the poster is not even downloaded"
    );
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let kinds: BTreeSet<MediaFileType> = report.images.iter().map(|i| i.kind).collect();
    assert_eq!(
        kinds,
        BTreeSet::from([
            MediaFileType::Fanart,
            MediaFileType::Clearlogo,
            MediaFileType::Thumb
        ])
    );
}

#[tokio::test]
async fn a_folder_without_artwork_gets_the_best_image_of_every_offered_type() {
    let mut case = Case::movie(&[]);
    case.offer(
        MediaFileType::Poster,
        "https://i.test/low.jpg",
        5.0,
        jpeg(10),
    );
    case.offer(
        MediaFileType::Poster,
        "https://i.test/best.jpg",
        8.5,
        jpeg(11),
    );
    case.offer(MediaFileType::Fanart, "https://i.test/f.jpg", 7.0, jpeg(12));
    case.offer(MediaFileType::Disc, "https://i.test/disc.png", 7.0, png(13));

    let report = case.download_missing().await;

    assert_eq!(
        case.read("Heat (1995)-poster.jpg"),
        jpeg(11),
        "the higher score wins"
    );
    assert_eq!(
        case.tree(),
        [
            "Heat (1995)/Heat (1995)-discart.png",
            "Heat (1995)/Heat (1995)-fanart.jpg",
            "Heat (1995)/Heat (1995)-poster.jpg",
            "Heat (1995)/Heat (1995).mkv",
        ]
    );
    let poster = report
        .images
        .iter()
        .find(|i| i.kind == MediaFileType::Poster)
        .unwrap();
    assert_eq!(poster.url, "https://i.test/best.jpg");
    assert_eq!((poster.width, poster.height), (64, 96));
    assert_eq!(poster.key, ThumbKey::of_bytes(&jpeg(11)).as_str());
}

#[tokio::test]
async fn several_configured_names_receive_the_same_image() {
    let mut case = Case::movie(&[(
        "artwork.names.movie.poster",
        json!(["FILENAME_POSTER", "POSTER", "FOLDER"]),
    )]);
    case.offer(MediaFileType::Poster, "https://i.test/p.jpg", 1.0, jpeg(20));

    case.download_missing().await;

    assert_eq!(
        case.tree(),
        [
            "Heat (1995)/Heat (1995)-poster.jpg",
            "Heat (1995)/Heat (1995).mkv",
            "Heat (1995)/folder.jpg",
            "Heat (1995)/poster.jpg",
        ]
    );
}

#[tokio::test]
async fn an_image_under_another_name_or_extension_counts_as_there() {
    let mut case = Case::movie(&[]);
    // A hand-picked poster with a name and format the settings do not write.
    fs::write(case.movie.join("poster.png"), png(30)).unwrap();
    // A fanart saved under the configured name but as PNG.
    fs::write(case.movie.join("Heat (1995)-fanart.png"), png(31)).unwrap();
    case.present = BTreeSet::from([MediaFileType::Poster]);
    case.offer(MediaFileType::Poster, "https://i.test/p.jpg", 1.0, jpeg(32));
    case.offer(MediaFileType::Fanart, "https://i.test/f.jpg", 1.0, jpeg(33));

    let report = case.download_missing().await;

    assert_eq!(
        case.tree(),
        [
            "Heat (1995)/Heat (1995)-fanart.png",
            "Heat (1995)/Heat (1995).mkv",
            "Heat (1995)/poster.png",
        ]
    );
    assert!(report.images.is_empty());
    assert!(case.fetcher.requests().is_empty(), "nothing was downloaded");
}

#[tokio::test]
async fn a_multi_movie_folder_gets_filename_based_names_and_no_extras_or_portraits() {
    let mut case = Case::movie(&[
        ("artwork.names.movie.poster", json!(["POSTER"])),
        ("artwork.extra-fanart-enabled", json!(true)),
        ("artwork.extra-thumbs", json!(true)),
        ("artwork.actor-images", json!(true)),
    ]);
    case.target.folder = FolderKind::MultiMovie;
    case.cast = vec![actor("Al Pacino", "https://i.test/al.jpg")];
    case.offer(MediaFileType::Poster, "https://i.test/p.jpg", 1.0, jpeg(40));
    for i in 0..4u8 {
        case.offer(
            MediaFileType::Fanart,
            &format!("https://i.test/f{i}.jpg"),
            f32::from(9 - i),
            jpeg(41 + i),
        );
        case.offer(
            MediaFileType::Thumb,
            &format!("https://i.test/t{i}.jpg"),
            f32::from(9 - i),
            jpeg(51 + i),
        );
    }
    case.fetcher.insert("https://i.test/al.jpg", jpeg(60));

    case.download_missing().await;

    assert_eq!(
        case.tree(),
        [
            "Heat (1995)/Heat (1995)-fanart.jpg",
            "Heat (1995)/Heat (1995)-landscape.jpg",
            "Heat (1995)/Heat (1995)-poster.jpg",
            "Heat (1995)/Heat (1995).mkv",
        ],
        "`poster` became `<base>-poster`; no extrafanart, extrathumbs or .actors"
    );
}

#[tokio::test]
async fn a_folder_that_is_gone_is_not_recreated() {
    let mut case = Case::movie(&[]);
    case.offer(MediaFileType::Poster, "https://i.test/p.jpg", 1.0, jpeg(1));
    fs::remove_dir_all(&case.movie).unwrap();

    let report = case.download_missing().await;

    assert!(!case.movie.exists());
    assert!(report.images.is_empty());
    assert!(case.fetcher.requests().is_empty());
}

#[tokio::test]
async fn a_bad_answer_is_skipped_and_the_next_candidate_is_used() {
    let mut case = Case::movie(&[]);
    case.offer(
        MediaFileType::Poster,
        "https://i.test/error-page.jpg",
        9.0,
        b"<html><body>Rate limit exceeded</body></html>".to_vec(),
    );
    case.candidates.push(candidate(
        MediaFileType::Poster,
        "https://i.test/gone.jpg",
        8.0,
    ));
    case.offer(
        MediaFileType::Poster,
        "https://i.test/ok.jpg",
        7.0,
        jpeg(70),
    );

    let report = case.download_missing().await;

    assert_eq!(case.read("Heat (1995)-poster.jpg"), jpeg(70));
    assert_eq!(report.failures.len(), 2, "{:?}", report.failures);
    assert!(report.failures.iter().all(|f| f.what == "poster"));
    assert!(
        case.tree().iter().all(|f| !f.ends_with(".cvpart")),
        "no half-written file is left"
    );
}

#[tokio::test]
async fn nothing_is_written_when_every_answer_is_bad() {
    let mut case = Case::movie(&[]);
    case.offer(
        MediaFileType::Poster,
        "https://i.test/a.jpg",
        9.0,
        b"nope".to_vec(),
    );

    let report = case.download_missing().await;

    assert_eq!(case.tree(), ["Heat (1995)/Heat (1995).mkv"]);
    assert_eq!(report.failures.len(), 1);
}

#[tokio::test]
async fn replacement_moves_the_old_file_to_the_trash_and_never_deletes_it() {
    let mut case = Case::movie(&[]);
    let old = jpeg(80);
    fs::write(case.movie.join("Heat (1995)-poster.jpg"), &old).unwrap();
    // The same poster as a PNG under another extension is also displaced.
    fs::write(case.movie.join("Heat (1995)-poster.png"), png(81)).unwrap();
    case.present = BTreeSet::from([MediaFileType::Poster]);
    case.offer(
        MediaFileType::Poster,
        "https://i.test/new.jpg",
        1.0,
        jpeg(82),
    );

    let kept = case
        .downloader()
        .download_missing(&case.request(false))
        .await;
    assert!(
        kept.images.is_empty(),
        "without the option the poster is kept"
    );
    assert_eq!(case.read("Heat (1995)-poster.jpg"), old);

    let report = case
        .downloader()
        .download_missing(&case.request(true))
        .await;

    assert_eq!(case.read("Heat (1995)-poster.jpg"), jpeg(82));
    assert_eq!(
        case.tree(),
        [
            "<trash>/Heat (1995)/Heat (1995)-poster.jpg",
            "<trash>/Heat (1995)/Heat (1995)-poster.png",
            "Heat (1995)/Heat (1995)-poster.jpg",
            "Heat (1995)/Heat (1995).mkv",
        ]
    );
    assert_eq!(report.trashed.len(), 2);
    let trashed_jpg = report
        .trashed
        .iter()
        .find(|p| p.extension().is_some_and(|e| e == "jpg"))
        .unwrap();
    assert_eq!(
        fs::read(trashed_jpg).unwrap(),
        old,
        "the old bytes are in the trash"
    );
}

/// The old poster under another extension leaves only once the new one is in place: a write
/// that fails (here a folder is in the way of the new name) keeps the old poster where it was.
#[tokio::test]
async fn a_replacement_that_fails_keeps_the_old_poster_under_its_other_extension() {
    let mut case = Case::movie(&[]);
    let old = png(60);
    fs::write(case.movie.join("Heat (1995)-poster.png"), &old).unwrap();
    fs::create_dir(case.movie.join("Heat (1995)-poster.jpg")).unwrap();
    case.present = BTreeSet::from([MediaFileType::Poster]);
    case.offer(
        MediaFileType::Poster,
        "https://i.test/new.jpg",
        1.0,
        jpeg(61),
    );

    let report = case
        .downloader()
        .download_missing(&case.request(true))
        .await;

    assert!(report.images.is_empty(), "nothing could be written");
    assert_eq!(
        case.read("Heat (1995)-poster.png"),
        old,
        "the old poster is still in place"
    );
    assert!(report.trashed.is_empty(), "{:?}", report.trashed);
}

#[tokio::test]
async fn a_chosen_local_file_replaces_the_poster_with_the_configured_names() {
    let case = Case::movie(&[]);
    let old = jpeg(90);
    fs::write(case.movie.join("Heat (1995)-poster.jpg"), &old).unwrap();
    let dropped = case.root.join("dropped.png");
    fs::write(&dropped, png(91)).unwrap();

    // A dropped file reaches the downloader as a `file:` address, which the app's fetcher reads.
    let http = cv_providers::http::HttpClient::builder().build().unwrap();
    let downloader = ArtworkDownloader::new(Arc::new(HttpImageFetcher::new(http)));
    let report = downloader
        .place(
            &case.target,
            &case.config,
            MediaFileType::Poster,
            &file_url(&dropped),
            true,
        )
        .await;

    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(
        case.tree(),
        [
            "<trash>/Heat (1995)/Heat (1995)-poster.jpg",
            "Heat (1995)/Heat (1995)-poster.png",
            "Heat (1995)/Heat (1995).mkv",
            "dropped.png",
        ],
        "the dropped file is copied; the old poster is trashed whatever its extension"
    );
    assert_eq!(report.images[0].url, file_url(&dropped));
}

#[tokio::test]
async fn a_chosen_image_also_replaces_the_poster_under_a_name_no_longer_written() {
    let case = Case::movie(&[]);
    // The item's poster has a name the settings do not write (they write `<base>-poster`).
    let legacy = case.movie.join("poster.jpg");
    fs::write(&legacy, jpeg(90)).unwrap();
    let dropped = case.root.join("dropped.png");
    fs::write(&dropped, png(91)).unwrap();

    let http = cv_providers::http::HttpClient::builder().build().unwrap();
    let downloader = ArtworkDownloader::new(Arc::new(HttpImageFetcher::new(http)));
    let report = downloader
        .place_over(
            &case.target,
            &case.config,
            MediaFileType::Poster,
            &file_url(&dropped),
            true,
            std::slice::from_ref(&legacy),
        )
        .await;

    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(
        case.tree(),
        [
            "<trash>/Heat (1995)/poster.jpg",
            "Heat (1995)/Heat (1995)-poster.png",
            "Heat (1995)/Heat (1995).mkv",
            "dropped.png",
        ],
        "the old poster goes to the trash, so one poster is left"
    );
    assert!(
        report.trash_moves.iter().any(|(from, _)| *from == legacy),
        "the undo can bring it back: {:?}",
        report.trash_moves
    );
}

#[tokio::test]
async fn a_chosen_file_that_is_not_a_picture_is_named_as_such() {
    let case = Case::movie(&[]);
    let text = case.root.join("notes.jpg");
    fs::write(&text, b"this is text, not a picture").unwrap();
    let http = cv_providers::http::HttpClient::builder().build().unwrap();
    let downloader = ArtworkDownloader::new(Arc::new(HttpImageFetcher::new(http)));
    let report = downloader
        .place(
            &case.target,
            &case.config,
            MediaFileType::Poster,
            &file_url(&text),
            true,
        )
        .await;
    assert_eq!(report.failures.len(), 1, "{:?}", report.failures);
    assert_eq!(
        report.failures[0].reason,
        "the file is not a picture; use a JPEG, PNG or WebP file"
    );
}

#[tokio::test]
async fn posters_reach_the_thumbnail_cache_as_they_are_written() {
    let mut case = Case::movie(&[]);
    case.offer(MediaFileType::Poster, "https://i.test/p.jpg", 1.0, jpeg(95));
    case.offer(MediaFileType::Fanart, "https://i.test/f.jpg", 1.0, jpeg(96));
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(ThumbCache::open(cache_dir.path(), None).unwrap());
    let downloader = case.downloader().with_thumbs(cache.clone());

    downloader.download_missing(&case.request(false)).await;

    let poster = ThumbKey::of_bytes(&jpeg(95));
    assert!(cache.has(&poster, ThumbSize::Grid) && cache.has(&poster, ThumbSize::List));
    assert!(
        !cache.has(&ThumbKey::of_bytes(&jpeg(96)), ThumbSize::Grid),
        "fanart is generated on demand"
    );
}

// ---- extras ----------------------------------------------------------------------------------

fn offer_fanart(case: &mut Case, count: u8) {
    for i in 0..count {
        case.offer(
            MediaFileType::Fanart,
            &format!("https://i.test/f{i}.jpg"),
            f32::from(20 - i),
            jpeg(100 + i),
        );
    }
}

#[tokio::test]
async fn extra_fanart_writes_the_asked_count_after_the_main_image() {
    let mut case = Case::movie(&[
        ("artwork.extra-fanart-enabled", json!(true)),
        ("artwork.extra-fanart", json!(4)),
    ]);
    offer_fanart(&mut case, 8);

    let report = case.download_missing().await;

    assert_eq!(
        case.tree(),
        [
            "Heat (1995)/Heat (1995)-fanart.jpg",
            "Heat (1995)/Heat (1995)-fanart1.jpg",
            "Heat (1995)/Heat (1995)-fanart2.jpg",
            "Heat (1995)/Heat (1995)-fanart3.jpg",
            "Heat (1995)/Heat (1995)-fanart4.jpg",
            "Heat (1995)/Heat (1995).mkv",
        ]
    );
    // The main image is the best one; the extras are the next four, in order.
    assert_eq!(case.read("Heat (1995)-fanart.jpg"), jpeg(100));
    for n in 1..=4u8 {
        assert_eq!(
            case.read(&format!("Heat (1995)-fanart{n}.jpg")),
            jpeg(100 + n)
        );
    }
    let extras: Vec<(u32, &str)> = report
        .images
        .iter()
        .filter(|i| i.kind == MediaFileType::Extrafanart)
        .map(|i| (i.position, i.url.as_str()))
        .collect();
    assert_eq!(
        extras,
        [
            (1, "https://i.test/f1.jpg"),
            (2, "https://i.test/f2.jpg"),
            (3, "https://i.test/f3.jpg"),
            (4, "https://i.test/f4.jpg"),
        ]
    );
}

#[tokio::test]
async fn extra_fanart_uses_the_extrafanart_folder_of_the_xbmc_preset() {
    let mut case = Case::movie(&[
        ("metadata.artwork-names", json!("xbmc")),
        ("artwork.extra-fanart-enabled", json!(true)),
        ("artwork.extra-fanart", json!(2)),
    ]);
    offer_fanart(&mut case, 5);

    case.download_missing().await;

    let tree = case.tree();
    assert!(
        tree.contains(&"Heat (1995)/extrafanart/fanart1.jpg".to_owned()),
        "{tree:?}"
    );
    assert!(tree.contains(&"Heat (1995)/extrafanart/fanart2.jpg".to_owned()));
    assert!(!tree.iter().any(|f| f.ends_with("fanart3.jpg")));
}

#[tokio::test]
async fn previous_extras_are_replaced_and_the_leftovers_go_to_the_trash() {
    let mut case = Case::movie(&[
        ("metadata.artwork-names", json!("xbmc")),
        ("artwork.extra-fanart-enabled", json!(true)),
        ("artwork.extra-fanart", json!(2)),
    ]);
    fs::create_dir_all(case.movie.join("extrafanart")).unwrap();
    for n in 1..=5u8 {
        fs::write(
            case.movie.join(format!("extrafanart/fanart{n}.jpg")),
            jpeg(200 + n),
        )
        .unwrap();
    }
    case.present = BTreeSet::from([MediaFileType::Fanart]);
    fs::write(case.movie.join("Heat (1995)-fanart.jpg"), jpeg(199)).unwrap();
    offer_fanart(&mut case, 4);

    // Five extras exist and two are wanted: nothing to do unless replacement is asked for.
    let untouched = case
        .downloader()
        .download_missing(&case.request(false))
        .await;
    assert!(untouched.images.is_empty() && untouched.trashed.is_empty());

    let report = case
        .downloader()
        .download_missing(&case.request(true))
        .await;

    let tree = case.tree();
    let extras: Vec<&String> = tree
        .iter()
        .filter(|f| f.starts_with("Heat (1995)/extrafanart/"))
        .collect();
    assert_eq!(
        extras,
        [
            "Heat (1995)/extrafanart/fanart1.jpg",
            "Heat (1995)/extrafanart/fanart2.jpg"
        ]
    );
    let trashed: Vec<&String> = tree
        .iter()
        .filter(|f| f.starts_with("<trash>/Heat (1995)/extrafanart/"))
        .collect();
    assert_eq!(
        trashed.len(),
        5,
        "all five old extras are in the trash: {tree:?}"
    );
    assert!(tree.contains(&"<trash>/Heat (1995)/Heat (1995)-fanart.jpg".to_owned()));
    assert_eq!(report.replaced_extras, [MediaFileType::Extrafanart]);
    // The user's main fanart was replaced too, since replacement was asked for.
    assert_eq!(case.read("Heat (1995)-fanart.jpg"), jpeg(100));
}

#[tokio::test]
async fn extras_are_not_fetched_again_when_the_last_run_already_found_all_there_are() {
    let mut case = Case::movie(&[
        ("artwork.extra-fanart-enabled", json!(true)),
        ("artwork.extra-fanart", json!(5)),
    ]);
    offer_fanart(&mut case, 3); // the main image and two extras: fewer than asked for

    let first = case.download_missing().await;
    assert_eq!(
        first
            .images
            .iter()
            .filter(|i| i.kind == MediaFileType::Extrafanart)
            .count(),
        2
    );

    // The library now remembers two extras; with both on disk the run has nothing to do.
    case.recorded = first
        .images
        .iter()
        .map(|i| ArtworkRef::fetched(i.kind, i.position, &i.url, &i.key, i.width, i.height))
        .collect();
    case.present = BTreeSet::from([MediaFileType::Fanart]);
    let before = case.fetcher.requests().len();
    let second = case.download_missing().await;

    assert!(second.images.is_empty() && second.trashed.is_empty());
    assert_eq!(case.fetcher.requests().len(), before);
}

#[tokio::test]
async fn extra_thumbs_are_resized_to_the_configured_width_and_a_small_one_is_not_enlarged() {
    let mut case = Case::movie(&[
        ("artwork.extra-thumbs", json!(true)),
        ("artwork.extra-thumbs-count", json!(3)),
        ("artwork.extra-thumbs-resize", json!(true)),
        ("artwork.extra-thumbs-width", json!(100)),
    ]);
    case.offer(
        MediaFileType::Thumb,
        "https://i.test/main.jpg",
        9.0,
        picture(ImageFormat::Jpeg, 400, 200, 1),
    );
    case.offer(
        MediaFileType::Thumb,
        "https://i.test/wide.png",
        8.0,
        picture(ImageFormat::Png, 400, 200, 2),
    );
    case.offer(
        MediaFileType::Thumb,
        "https://i.test/tall.jpg",
        7.0,
        picture(ImageFormat::Jpeg, 300, 600, 3),
    );
    let small = picture(ImageFormat::Jpeg, 80, 40, 4);
    case.offer(
        MediaFileType::Thumb,
        "https://i.test/small.jpg",
        6.0,
        small.clone(),
    );

    case.download_missing().await;

    assert_eq!(
        case.tree(),
        [
            "Heat (1995)/Heat (1995)-landscape.jpg",
            "Heat (1995)/Heat (1995).mkv",
            "Heat (1995)/extrathumbs/thumb1.jpg",
            "Heat (1995)/extrathumbs/thumb2.jpg",
            "Heat (1995)/extrathumbs/thumb3.jpg",
        ]
    );
    let size = |rel: &str| image::image_dimensions(case.movie.join(rel)).unwrap();
    assert_eq!(
        size("Heat (1995)-landscape.jpg"),
        (400, 200),
        "the main thumb is not resized"
    );
    assert_eq!(size("extrathumbs/thumb1.jpg"), (100, 50));
    assert_eq!(size("extrathumbs/thumb2.jpg"), (100, 200));
    assert_eq!(
        case.read("extrathumbs/thumb3.jpg"),
        small,
        "not enlarged, not re-encoded"
    );
}

#[tokio::test]
async fn extra_thumbs_keep_their_size_when_resizing_is_off() {
    let mut case = Case::movie(&[
        ("artwork.extra-thumbs", json!(true)),
        ("artwork.extra-thumbs-count", json!(1)),
    ]);
    case.offer(
        MediaFileType::Thumb,
        "https://i.test/main.jpg",
        9.0,
        jpeg(1),
    );
    let wide = picture(ImageFormat::Jpeg, 1200, 600, 2);
    case.offer(
        MediaFileType::Thumb,
        "https://i.test/wide.jpg",
        8.0,
        wide.clone(),
    );

    case.download_missing().await;

    assert_eq!(case.read("extrathumbs/thumb1.jpg"), wide);
}

// ---- actors ----------------------------------------------------------------------------------

#[tokio::test]
async fn actor_portraits_are_named_after_the_actor_and_stale_ones_are_trashed() {
    let mut case = Case::movie(&[("artwork.actor-images", json!(true))]);
    case.cast = vec![
        actor("Timothée Chalamet", "https://i.test/tc.jpg"),
        actor("Zendaya", "https://i.test/z.png"),
        actor("No Photo", ""),
        Credit::new(CreditKind::Director, "Denis Villeneuve", ""),
    ];
    case.fetcher.insert("https://i.test/tc.jpg", jpeg(110));
    case.fetcher.insert("https://i.test/z.png", png(111));
    // A portrait of an actor who left the cast, one that stays, and a file that is no portrait.
    let actors = case.movie.join(ACTORS_DIR);
    fs::create_dir_all(&actors).unwrap();
    fs::write(actors.join("Old_Actor.jpg"), b"old").unwrap();
    let kept = jpeg(112);
    fs::write(actors.join("No_Photo.jpg"), &kept).unwrap();
    fs::write(actors.join("notes.txt"), b"mine").unwrap();

    let report = case.download_missing().await;

    assert_eq!(
        case.tree(),
        [
            "<trash>/Heat (1995)/.actors/Old_Actor.jpg",
            "Heat (1995)/.actors/No_Photo.jpg",
            "Heat (1995)/.actors/Timothée_Chalamet.jpg",
            "Heat (1995)/.actors/Zendaya.png",
            "Heat (1995)/.actors/notes.txt",
            "Heat (1995)/Heat (1995).mkv",
        ]
    );
    assert_eq!(case.read(".actors/No_Photo.jpg"), kept);
    assert_eq!(report.actors.len(), 2);
    assert_eq!(report.trashed.len(), 1);
    assert_eq!(fs::read(&report.trashed[0]).unwrap(), b"old");
}

#[tokio::test]
async fn existing_portraits_are_not_downloaded_again_unless_replacement_is_asked_for() {
    let mut case = Case::movie(&[("artwork.actor-images", json!(true))]);
    case.cast = vec![actor("Al Pacino", "https://i.test/al.jpg")];
    case.fetcher.insert("https://i.test/al.jpg", jpeg(120));

    let first = case.download_missing().await;
    assert_eq!(first.actors.len(), 1);
    assert_eq!(case.fetcher.requests().len(), 1);

    let again = case.download_missing().await;
    assert!(again.actors.is_empty());
    assert_eq!(case.fetcher.requests().len(), 1, "no second request");

    case.fetcher.insert("https://i.test/al.jpg", jpeg(121));
    let replaced = case
        .downloader()
        .download_missing(&case.request(true))
        .await;
    assert_eq!(replaced.actors.len(), 1);
    assert_eq!(case.read(".actors/Al_Pacino.jpg"), jpeg(121));
    assert!(
        case.tree()
            .iter()
            .any(|f| f == "<trash>/Heat (1995)/.actors/Al_Pacino.jpg")
    );
}

#[tokio::test]
async fn portraits_are_off_by_default() {
    let mut case = Case::movie(&[]);
    case.cast = vec![actor("Al Pacino", "https://i.test/al.jpg")];
    case.fetcher.insert("https://i.test/al.jpg", jpeg(1));

    case.download_missing().await;

    assert_eq!(case.tree(), ["Heat (1995)/Heat (1995).mkv"]);
}

#[cfg(unix)]
#[tokio::test]
async fn an_actors_folder_that_is_a_link_is_left_alone() {
    let mut case = Case::movie(&[("artwork.actor-images", json!(true))]);
    let elsewhere = case.root.join("shared-actors");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("Someone_Else.jpg"), b"x").unwrap();
    std::os::unix::fs::symlink(&elsewhere, case.movie.join(ACTORS_DIR)).unwrap();
    case.cast = vec![actor("Al Pacino", "https://i.test/al.jpg")];
    case.fetcher.insert("https://i.test/al.jpg", jpeg(1));

    case.download_missing().await;

    assert!(elsewhere.join("Someone_Else.jpg").exists());
    assert!(!elsewhere.join("Al_Pacino.jpg").exists());
}

// ---- shows -----------------------------------------------------------------------------------

#[tokio::test]
async fn a_show_gets_its_poster_fanart_banner_and_extras_in_the_show_folder() {
    let mut case = Case::show(&[
        ("artwork.extra-fanart-enabled", json!(true)),
        ("artwork.extra-fanart", json!(2)),
        ("artwork.actor-images", json!(true)),
    ]);
    case.cast = vec![actor("Adam Scott", "https://i.test/adam.jpg")];
    case.fetcher.insert("https://i.test/adam.jpg", jpeg(130));
    case.offer(
        MediaFileType::Poster,
        "https://i.test/p.jpg",
        9.0,
        jpeg(131),
    );
    case.offer(
        MediaFileType::Banner,
        "https://i.test/b.jpg",
        9.0,
        picture(ImageFormat::Jpeg, 200, 40, 132),
    );
    offer_fanart(&mut case, 4);

    case.download_missing().await;

    assert_eq!(
        case.tree(),
        [
            "Severance/.actors/Adam_Scott.jpg",
            "Severance/banner.jpg",
            "Severance/extrafanart/fanart1.jpg",
            "Severance/extrafanart/fanart2.jpg",
            "Severance/fanart.jpg",
            "Severance/poster.jpg",
        ]
    );
}

// ---- remembered URLs and re-download ---------------------------------------------------------

fn library(dir: &Path) -> Database {
    Database::open_with(dir.join("library.db"), &DbOptions { readers: 2 }).expect("database")
}

#[tokio::test]
async fn a_chosen_local_file_leaves_one_poster_row_and_remembers_its_source() {
    let case = Case::movie(&[]);
    let legacy = case.movie.join("poster.jpg");
    fs::write(&legacy, jpeg(90)).unwrap();
    let db_dir = tempfile::tempdir().unwrap();
    let db = library(db_dir.path());
    let mut movie = Item::movie("Heat");
    movie.core.path = case.movie.to_string_lossy().into_owned();
    movie.core.media_files.push(cv_domain::MediaFile::new(
        MediaFileType::Poster,
        case.movie.to_string_lossy(),
        "poster.jpg",
    ));
    movie
        .core
        .artwork_urls
        .insert(MediaFileType::Poster, "https://i.test/old.jpg".to_owned());
    let id = db.write(move |tx| insert_item(tx, &movie)).unwrap();
    let dropped = case.root.join("dropped.png");
    fs::write(&dropped, png(91)).unwrap();

    let http = cv_providers::http::HttpClient::builder().build().unwrap();
    let downloader = ArtworkDownloader::new(Arc::new(HttpImageFetcher::new(http)));
    let report = downloader
        .place_over(
            &case.target,
            &case.config,
            MediaFileType::Poster,
            &file_url(&dropped),
            true,
            std::slice::from_ref(&legacy),
        )
        .await;
    record(&db, id, &report).unwrap();

    let item = db.read(|c| get_item(c, id)).unwrap().unwrap();
    let posters: Vec<&str> = item
        .core
        .media_files
        .iter()
        .filter(|f| f.kind == MediaFileType::Poster)
        .map(|f| f.filename.as_str())
        .collect();
    assert_eq!(posters, ["Heat (1995)-poster.png"]);
    // The chosen file is remembered, so Re-download can read it again; the NFO leaves it out.
    assert_eq!(
        item.core.artwork_urls[&MediaFileType::Poster],
        file_url(&dropped)
    );
}

#[tokio::test]
async fn the_library_remembers_every_source_and_a_deleted_poster_is_fetched_again_from_it() {
    let mut case = Case::movie(&[
        ("artwork.extra-fanart-enabled", json!(true)),
        ("artwork.extra-fanart", json!(2)),
    ]);
    case.offer(
        MediaFileType::Poster,
        "https://i.test/poster.jpg",
        9.0,
        jpeg(140),
    );
    offer_fanart(&mut case, 3);
    let db_dir = tempfile::tempdir().unwrap();
    let db = library(db_dir.path());
    let mut movie = Item::movie("Heat");
    movie.core.path = case.movie.to_string_lossy().into_owned();
    let id = db.write(move |tx| insert_item(tx, &movie)).unwrap();

    // 1. Download and remember.
    let report = case.download_missing().await;
    record(&db, id, &report).unwrap();

    let item = db.read(|c| get_item(c, id)).unwrap().unwrap();
    assert_eq!(
        item.core.artwork_urls[&MediaFileType::Poster],
        "https://i.test/poster.jpg"
    );
    assert_eq!(
        item.core.artwork_urls[&MediaFileType::Fanart],
        "https://i.test/f0.jpg"
    );
    let listed: BTreeSet<String> = item
        .core
        .media_files
        .iter()
        .filter(|f| f.kind.is_graphic())
        .map(|f| format!("{:?}:{}", f.kind, f.filename))
        .collect();
    assert_eq!(
        listed,
        BTreeSet::from([
            "Poster:Heat (1995)-poster.jpg".to_owned(),
            "Fanart:Heat (1995)-fanart.jpg".to_owned(),
            "Extrafanart:Heat (1995)-fanart1.jpg".to_owned(),
            "Extrafanart:Heat (1995)-fanart2.jpg".to_owned(),
        ]),
        "the new files are the item's files"
    );
    let refs = db.read(|c| artwork_refs(c, id)).unwrap();
    let poster_ref = refs
        .iter()
        .find(|r| r.kind == MediaFileType::Poster)
        .unwrap();
    assert_eq!(
        poster_ref.cache_hash.as_deref(),
        Some(ThumbKey::of_bytes(&jpeg(140)).as_str())
    );
    assert_eq!((poster_ref.width, poster_ref.height), (64, 96));
    let extra_urls: Vec<(u32, &str)> = refs
        .iter()
        .filter(|r| r.kind == MediaFileType::Extrafanart)
        .map(|r| (r.position, r.url.as_str()))
        .collect();
    assert_eq!(
        extra_urls,
        [(1, "https://i.test/f1.jpg"), (2, "https://i.test/f2.jpg")]
    );

    // 2. The poster and one extra are deleted on disk; the providers are gone.
    fs::remove_file(case.movie.join("Heat (1995)-poster.jpg")).unwrap();
    fs::remove_file(case.movie.join("Heat (1995)-fanart2.jpg")).unwrap();
    let fanart_before = case.read("Heat (1995)-fanart.jpg");
    case.candidates.clear();
    case.recorded = db.read(|c| artwork_refs(c, id)).unwrap();
    case.present = present_artwork(&item);
    assert!(
        !case.present.contains(&MediaFileType::Poster),
        "the deleted poster is not 'present'"
    );
    assert!(case.present.contains(&MediaFileType::Fanart));
    let before = case.fetcher.requests().len();

    // 3. Re-download.
    let again = case.downloader().redownload(&case.request(false)).await;
    record(&db, id, &again).unwrap();

    assert_eq!(
        case.read("Heat (1995)-poster.jpg"),
        jpeg(140),
        "fetched again from its source"
    );
    assert_eq!(case.read("Heat (1995)-fanart2.jpg"), jpeg(102));
    assert_eq!(case.read("Heat (1995)-fanart.jpg"), fanart_before);
    assert_eq!(
        case.fetcher.requests()[before..],
        ["https://i.test/poster.jpg", "https://i.test/f2.jpg"],
        "only the two missing images were requested"
    );
    // The remembered sources survived.
    let refs = db.read(|c| artwork_refs(c, id)).unwrap();
    assert_eq!(
        refs.iter()
            .filter(|r| r.kind == MediaFileType::Extrafanart)
            .count(),
        2
    );
    let item = db.read(|c| get_item(c, id)).unwrap().unwrap();
    assert_eq!(
        item.core.artwork_urls[&MediaFileType::Poster],
        "https://i.test/poster.jpg"
    );
}

#[tokio::test]
async fn redownload_leaves_the_files_that_are_there_and_the_user_poster_alone() {
    let mut case = Case::movie(&[]);
    // The user's poster has a name the settings do not write; the library still remembers a URL
    // from an earlier scrape.
    fs::write(case.movie.join("poster.jpg"), jpeg(150)).unwrap();
    case.present = BTreeSet::from([MediaFileType::Poster]);
    case.fetcher.insert("https://i.test/scraped.jpg", jpeg(151));
    case.recorded = vec![ArtworkRef::fetched(
        MediaFileType::Poster,
        0,
        "https://i.test/scraped.jpg",
        "h",
        1,
        1,
    )];

    let report = case.downloader().redownload(&case.request(false)).await;

    assert!(report.images.is_empty());
    assert_eq!(
        case.tree(),
        ["Heat (1995)/Heat (1995).mkv", "Heat (1995)/poster.jpg"]
    );
    assert!(case.fetcher.requests().is_empty());
}

#[tokio::test]
async fn redownload_with_replacement_fetches_everything_again() {
    let mut case = Case::movie(&[]);
    fs::write(case.movie.join("Heat (1995)-poster.jpg"), jpeg(160)).unwrap();
    case.present = BTreeSet::from([MediaFileType::Poster]);
    case.fetcher.insert("https://i.test/p.jpg", jpeg(161));
    case.recorded = vec![ArtworkRef::fetched(
        MediaFileType::Poster,
        0,
        "https://i.test/p.jpg",
        "h",
        1,
        1,
    )];

    let report = case.downloader().redownload(&case.request(true)).await;

    assert_eq!(report.images.len(), 1);
    assert_eq!(case.read("Heat (1995)-poster.jpg"), jpeg(161));
    assert!(
        case.tree()
            .iter()
            .any(|f| f == "<trash>/Heat (1995)/Heat (1995)-poster.jpg")
    );
}

#[tokio::test]
async fn redownload_restores_missing_portraits_from_the_credits() {
    let mut case = Case::movie(&[("artwork.actor-images", json!(true))]);
    case.cast = vec![
        actor("Al Pacino", "https://i.test/al.jpg"),
        actor("Val Kilmer", "https://i.test/vk.jpg"),
    ];
    case.fetcher.insert("https://i.test/al.jpg", jpeg(170));
    case.fetcher.insert("https://i.test/vk.jpg", jpeg(171));
    case.download_missing().await;
    fs::remove_file(case.movie.join(".actors/Val_Kilmer.jpg")).unwrap();
    let before = case.fetcher.requests().len();

    let report = case.downloader().redownload(&case.request(false)).await;

    assert_eq!(report.actors.len(), 1);
    assert_eq!(case.fetcher.requests()[before..], ["https://i.test/vk.jpg"]);
    assert!(case.movie.join(".actors/Val_Kilmer.jpg").exists());
}

#[tokio::test]
async fn with_automatic_selection_off_only_the_remembered_urls_are_used() {
    let mut case = Case::movie(&[("artwork.auto-select", json!(false))]);
    case.offer(
        MediaFileType::Poster,
        "https://i.test/offered.jpg",
        9.0,
        jpeg(180),
    );
    case.offer(
        MediaFileType::Fanart,
        "https://i.test/remembered.jpg",
        1.0,
        jpeg(181),
    );
    case.recorded = vec![ArtworkRef::fetched(
        MediaFileType::Fanart,
        0,
        "https://i.test/remembered.jpg",
        "h",
        1,
        1,
    )];

    case.download_missing().await;

    assert_eq!(
        case.tree(),
        [
            "Heat (1995)/Heat (1995)-fanart.jpg",
            "Heat (1995)/Heat (1995).mkv"
        ]
    );
    assert_eq!(case.fetcher.requests(), ["https://i.test/remembered.jpg"]);
}

// ---- chosen extras (the picker) ----------------------------------------------------

#[tokio::test]
async fn chosen_extras_replace_the_whole_set_in_order_and_the_old_ones_go_to_the_trash() {
    let mut case = Case::movie(&[]);
    for n in 1..=4u8 {
        fs::write(
            case.movie.join(format!("Heat (1995)-fanart{n}.jpg")),
            jpeg(200 + n),
        )
        .unwrap();
    }
    for i in 0..2u8 {
        case.offer(
            MediaFileType::Fanart,
            &format!("https://i.test/c{i}.jpg"),
            1.0,
            jpeg(60 + i),
        );
    }
    let urls = vec![
        "https://i.test/c1.jpg".to_owned(),
        "https://i.test/missing.jpg".to_owned(),
        "https://i.test/c0.jpg".to_owned(),
        // Chosen twice (a candidate and the same address pasted): written once.
        "https://i.test/c1.jpg".to_owned(),
    ];

    let report = case
        .downloader()
        .place_extras(
            &case.target,
            &case.config,
            MediaFileType::Extrafanart,
            &urls,
        )
        .await;

    assert_eq!(
        case.tree(),
        [
            "<trash>/Heat (1995)/Heat (1995)-fanart1.jpg",
            "<trash>/Heat (1995)/Heat (1995)-fanart2.jpg",
            "<trash>/Heat (1995)/Heat (1995)-fanart3.jpg",
            "<trash>/Heat (1995)/Heat (1995)-fanart4.jpg",
            "Heat (1995)/Heat (1995)-fanart1.jpg",
            "Heat (1995)/Heat (1995)-fanart2.jpg",
            "Heat (1995)/Heat (1995).mkv",
        ],
        "two images replace four; all four old extras are in the trash"
    );
    assert_eq!(case.read("Heat (1995)-fanart1.jpg"), jpeg(61));
    assert_eq!(case.read("Heat (1995)-fanart2.jpg"), jpeg(60));
    let written: Vec<(u32, &str)> = report
        .images
        .iter()
        .map(|i| (i.position, i.url.as_str()))
        .collect();
    assert_eq!(
        written,
        [(1, "https://i.test/c1.jpg"), (2, "https://i.test/c0.jpg")],
        "the order of the choice is the order of the numbers"
    );
    assert_eq!(report.replaced_extras, [MediaFileType::Extrafanart]);
    assert_eq!(report.failures.len(), 1, "the unreachable one is reported");
}

#[tokio::test]
async fn a_set_of_extras_that_cannot_be_loaded_leaves_the_old_set_alone() {
    let case = Case::movie(&[]);
    fs::write(case.movie.join("Heat (1995)-fanart1.jpg"), jpeg(210)).unwrap();

    let report = case
        .downloader()
        .place_extras(
            &case.target,
            &case.config,
            MediaFileType::Extrafanart,
            &["https://i.test/gone.jpg".to_owned()],
        )
        .await;

    assert!(report.images.is_empty() && report.trashed.is_empty());
    assert!(report.replaced_extras.is_empty());
    assert_eq!(report.failures.len(), 1);
    assert_eq!(case.read("Heat (1995)-fanart1.jpg"), jpeg(210));
}

#[tokio::test]
async fn extras_are_refused_for_a_multi_movie_folder_and_for_other_kinds() {
    let mut case = Case::movie(&[]);
    case.offer(MediaFileType::Fanart, "https://i.test/f.jpg", 1.0, jpeg(70));
    let urls = vec!["https://i.test/f.jpg".to_owned()];

    let other = case
        .downloader()
        .place_extras(&case.target, &case.config, MediaFileType::Poster, &urls)
        .await;
    assert_eq!(other.failures.len(), 1);

    case.target.folder = FolderKind::MultiMovie;
    let multi = case
        .downloader()
        .place_extras(
            &case.target,
            &case.config,
            MediaFileType::Extrafanart,
            &urls,
        )
        .await;
    assert_eq!(multi.failures.len(), 1);
    assert_eq!(
        multi.failures[0].reason,
        "extras are not written for this folder"
    );
    assert_eq!(case.tree(), ["Heat (1995)/Heat (1995).mkv"]);
}

#[tokio::test]
async fn chosen_extra_thumbs_are_shrunk_to_the_configured_width() {
    let case = Case::movie(&[
        ("artwork.extra-thumbs-resize", json!(true)),
        ("artwork.extra-thumbs-width", json!(32)),
    ]);
    case.fetcher.insert(
        "https://i.test/t.jpg",
        picture(ImageFormat::Jpeg, 128, 72, 80),
    );

    let report = case
        .downloader()
        .place_extras(
            &case.target,
            &case.config,
            MediaFileType::Extrathumb,
            &["https://i.test/t.jpg".to_owned()],
        )
        .await;

    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.images.len(), 1);
    assert_eq!(report.images[0].width, 32);
}

// ---- bytes as served -------------------------------------------------------------------------

/// A JPEG the way image servers send them: an EXIF block and an ICC profile after the start
/// marker, with a gradient so a re-encode would change the pixels. A decode and encode would
/// drop both blocks and change the quantisation tables.
fn served_jpeg(width: u32, height: u32) -> Vec<u8> {
    let image = RgbImage::from_fn(width, height, |x, y| {
        Rgb([
            (x * 7 % 256) as u8,
            (y * 5 % 256) as u8,
            ((x + y) % 256) as u8,
        ])
    });
    let mut plain = Cursor::new(Vec::new());
    image.write_to(&mut plain, ImageFormat::Jpeg).unwrap();
    let plain = plain.into_inner();
    let segment = |marker: u8, body: &[u8]| -> Vec<u8> {
        let length = u16::try_from(body.len() + 2).unwrap();
        let mut out = vec![0xFF, marker];
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(body);
        out
    };
    let mut exif = b"Exif\0\0MM\0*\0\0\0\x08\0\0".to_vec();
    exif.extend_from_slice(&[0; 8]);
    let mut icc = b"ICC_PROFILE\0\x01\x01".to_vec();
    icc.extend_from_slice(&[0x42; 128]);
    let mut out = plain[..2].to_vec();
    out.extend(segment(0xE1, &exif));
    out.extend(segment(0xE2, &icc));
    out.extend_from_slice(&plain[2..]);
    out
}

/// Serves `body` as `image/jpeg` to every request, as an image server does.
async fn serve_image(body: Vec<u8>) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let body = body.clone();
            tokio::spawn(async move {
                let mut request = vec![0u8; 8192];
                let _ = stream.read(&mut request).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    format!("http://{address}/t/p/original/backdrop.jpg")
}

fn http_downloader() -> ArtworkDownloader {
    let http = cv_providers::http::HttpClient::builder().build().unwrap();
    ArtworkDownloader::new(Arc::new(HttpImageFetcher::new(http)))
}

/// A user report: a chosen backdrop looked soft. Whatever the server sends is what lands in
/// the folder, byte for byte: no decode, no re-encode, no metadata stripped, no smaller variant.
#[tokio::test]
async fn a_chosen_image_is_written_byte_for_byte_as_served() {
    let case = Case::movie(&[]);
    fs::write(case.movie.join("Heat (1995)-fanart.jpg"), jpeg(5)).unwrap();
    let served = served_jpeg(1920, 1080);
    let url = serve_image(served.clone()).await;

    let report = http_downloader()
        .place(
            &case.target,
            &case.config,
            MediaFileType::Fanart,
            &url,
            true,
        )
        .await;

    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let written = case.read("Heat (1995)-fanart.jpg");
    assert_eq!(written.len(), served.len());
    assert!(
        written == served,
        "the written file differs from the download"
    );
    assert_eq!(report.images[0].url, url);
    assert_eq!(
        (report.images[0].width, report.images[0].height),
        (1920, 1080)
    );
    assert_eq!(report.images[0].key, ThumbKey::of_bytes(&served).as_str());
}

/// The automatic path (Download missing, the step after a scrape, Re-download) writes the bytes
/// as served too.
#[tokio::test]
async fn downloaded_artwork_is_written_byte_for_byte_as_served() {
    let mut case = Case::movie(&[]);
    let served = served_jpeg(2000, 3000);
    let url = serve_image(served.clone()).await;
    case.candidates
        .push(candidate(MediaFileType::Poster, &url, 1.0));

    let report = http_downloader()
        .download_missing(&case.request(false))
        .await;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(case.read("Heat (1995)-poster.jpg") == served);

    fs::remove_file(case.movie.join("Heat (1995)-poster.jpg")).unwrap();
    case.candidates.clear();
    case.recorded = vec![ArtworkRef::fetched(
        MediaFileType::Poster,
        0,
        url.clone(),
        ThumbKey::of_bytes(&served).as_str().to_owned(),
        2000,
        3000,
    )];
    let again = http_downloader().redownload(&case.request(false)).await;
    assert!(again.failures.is_empty(), "{:?}", again.failures);
    assert!(case.read("Heat (1995)-poster.jpg") == served);
}

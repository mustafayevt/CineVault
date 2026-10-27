use super::*;
use crate::settings::SettingsStore;
use cv_artwork::thumb::ThumbSize;
use serde_json::json;

struct Rig {
    _dir: tempfile::TempDir,
    settings: Arc<SettingsService>,
    thumbs: Arc<ThumbCache>,
    http: HttpClient,
    service: CacheService,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::open(SettingsStore::new(
        dir.path().join("settings.json"),
    )));
    let thumbs = Arc::new(ThumbCache::open(&dir.path().join("cache"), None).unwrap());
    let http = HttpClient::builder()
        .cache_dir(dir.path().join("http"))
        .build()
        .unwrap();
    let service = CacheService::new(
        Arc::clone(&thumbs),
        Some(http.clone()),
        Arc::clone(&settings),
    );
    Rig {
        _dir: dir,
        settings,
        thumbs,
        http,
        service,
    }
}

/// A distinct, valid image per `n`.
fn picture(n: u8) -> Vec<u8> {
    let mut image = image::RgbImage::new(64, 64);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = image::Rgb([n.wrapping_mul(40), (x * 4) as u8, (y * 4) as u8]);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

#[test]
fn the_cap_follows_the_setting_and_stays_in_range() {
    let rig = rig();
    assert_eq!(image_cache_cap(&rig.settings), DEFAULT_MAX_BYTES);
    rig.settings.set(IMAGE_CACHE_SIZE_KEY, json!(512)).unwrap();
    assert_eq!(image_cache_cap(&rig.settings), 512 * BYTES_PER_MB);
    rig.settings.set(IMAGE_CACHE_SIZE_KEY, json!(1)).unwrap();
    assert_eq!(
        image_cache_cap(&rig.settings),
        MIN_IMAGE_CACHE_MB * BYTES_PER_MB
    );
    rig.settings
        .set(IMAGE_CACHE_SIZE_KEY, json!("lots"))
        .unwrap();
    assert_eq!(image_cache_cap(&rig.settings), DEFAULT_MAX_BYTES);
}

#[test]
fn applying_the_settings_sets_the_cap_on_the_open_cache() {
    let rig = rig();
    assert_eq!(rig.thumbs.max_bytes(), None);
    rig.service.apply_settings();
    assert_eq!(rig.thumbs.max_bytes(), Some(DEFAULT_MAX_BYTES));
    rig.settings.set(IMAGE_CACHE_SIZE_KEY, json!(200)).unwrap();
    rig.service.apply_settings();
    assert_eq!(rig.thumbs.max_bytes(), Some(200 * BYTES_PER_MB));
}

#[tokio::test]
async fn the_image_cache_reports_its_usage_and_clears() {
    let rig = rig();
    for n in 0..3 {
        rig.thumbs
            .ensure_bytes(&picture(n), &[ThumbSize::Grid])
            .unwrap();
    }
    let usage = rig.service.usage(CacheKind::Images).await.unwrap();
    assert_eq!(usage.entries, 3);
    assert!(usage.bytes > 0);

    let cleared = rig.service.clear(CacheKind::Images).await.unwrap();
    assert_eq!(cleared.entries, 3);
    assert_eq!(cleared.bytes, usage.bytes);
    assert_eq!(
        rig.service.usage(CacheKind::Images).await.unwrap(),
        CacheSize::default()
    );
}

#[tokio::test]
async fn the_response_cache_reports_its_usage_and_clears() {
    let rig = rig();
    let cache = rig.http.cache().expect("the client has a cache");
    std::fs::create_dir_all(cache.root().join("tmdb").join("aa")).unwrap();
    std::fs::write(
        cache.root().join("tmdb").join("aa").join("aabb.bin"),
        b"CVH1",
    )
    .unwrap();

    let usage = rig.service.usage(CacheKind::Responses).await.unwrap();
    assert_eq!(usage.entries, 1);

    let cleared = rig.service.clear(CacheKind::Responses).await.unwrap();
    assert_eq!(cleared.entries, 1);
    assert_eq!(
        rig.service.usage(CacheKind::Responses).await.unwrap(),
        CacheSize::default()
    );
    // Clearing an empty cache is not an error.
    rig.service.clear(CacheKind::Responses).await.unwrap();
}

#[tokio::test]
async fn without_a_response_cache_there_is_nothing_to_show_or_clear() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::in_memory("test"));
    let thumbs = Arc::new(ThumbCache::open(dir.path(), None).unwrap());
    let service = CacheService::new(thumbs, None, settings);
    assert_eq!(
        service.usage(CacheKind::Responses).await.unwrap(),
        CacheSize::default()
    );
    assert_eq!(
        service.clear(CacheKind::Responses).await.unwrap(),
        CacheSize::default()
    );
}

#[tokio::test]
async fn grabbed_frames_count_as_images_and_go_when_the_cache_is_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let frames = dir.path().join("frames");
    std::fs::create_dir_all(frames.join("7")).unwrap();
    std::fs::write(frames.join("7").join("frame-01.jpg"), vec![1u8; 1000]).unwrap();
    std::fs::write(frames.join("7").join("frame-02.jpg"), vec![2u8; 500]).unwrap();
    let thumbs = Arc::new(ThumbCache::open(&dir.path().join("cache"), None).unwrap());
    let settings = Arc::new(SettingsService::in_memory("test"));
    let service = CacheService::new(thumbs, None, settings).with_frames_dir(&frames);

    let usage = service.usage(CacheKind::Images).await.unwrap();
    assert_eq!(
        usage,
        CacheSize {
            entries: 2,
            bytes: 1500
        }
    );
    let cleared = service.clear(CacheKind::Images).await.unwrap();
    assert_eq!(
        cleared,
        CacheSize {
            entries: 2,
            bytes: 1500
        }
    );
    assert!(frames.is_dir(), "the folder stays for the next frames");
    assert_eq!(std::fs::read_dir(&frames).unwrap().count(), 0);
}

#[test]
fn old_frames_are_removed_and_recent_ones_stay() {
    let dir = tempfile::tempdir().unwrap();
    let frames = dir.path().join("frames");
    for item in ["1", "2"] {
        std::fs::create_dir_all(frames.join(item)).unwrap();
        std::fs::write(frames.join(item).join("frame-01.jpg"), vec![0u8; 100]).unwrap();
    }
    let thumbs = Arc::new(ThumbCache::open(&dir.path().join("cache"), None).unwrap());
    let settings = Arc::new(SettingsService::in_memory("test"));
    let service = CacheService::new(thumbs, None, settings).with_frames_dir(&frames);

    // Nothing is a day old yet.
    assert_eq!(
        service.prune_frames(SystemTime::now()),
        CacheSize::default()
    );
    // Two days on, both folders are old.
    let later = SystemTime::now() + Duration::from_secs(2 * 24 * 60 * 60);
    let removed = service.prune_frames(later);
    assert_eq!(
        removed,
        CacheSize {
            entries: 2,
            bytes: 200
        }
    );
    assert_eq!(std::fs::read_dir(&frames).unwrap().count(), 0);
}

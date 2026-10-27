//! Opt-in smoke test against the real anime-offline-database release.
//!
//! It runs only when `CV_ANIME_DB_LIVE=1`; without it the test prints why it was skipped and
//! passes. It downloads the latest release (about 60 MB) into a temporary folder, converts it and
//! checks that the well-known anime Cowboy Bebop links its AniDB, MyAnimeList and AniList IDs.
//! A second load then uses the local copy.
//!
//! ```text
//! CV_ANIME_DB_LIVE=1 cargo test -p cv-providers --test anime_offline_db_live -- --nocapture
//! ```

use cv_domain::ProviderIds;
use cv_providers::AnimeOfflineDb;
use cv_providers::anime_db::LoadOutcome;

#[tokio::test]
async fn live_smoke() {
    if std::env::var("CV_ANIME_DB_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipped: set CV_ANIME_DB_LIVE=1 to run the live anime ID database smoke test");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let db = AnimeOfflineDb::new(dir.path().join("anime-offline-database")).unwrap();

    let loaded = db.load().await.expect("the release is downloadable");
    assert_eq!(loaded.outcome, LoadOutcome::Updated);
    assert!(
        loaded.crosswalk.len() > 10_000,
        "{} rows",
        loaded.crosswalk.len()
    );

    let mut mal = ProviderIds::new();
    mal.set("myanimelist", "1");
    let row = loaded.crosswalk.lookup(&mal);
    assert_eq!(row.get("anidb"), Some("23"));
    assert_eq!(row.get("anilist"), Some("1"));

    let again = db.load().await.unwrap();
    assert_eq!(again.outcome, LoadOutcome::Fresh);
}

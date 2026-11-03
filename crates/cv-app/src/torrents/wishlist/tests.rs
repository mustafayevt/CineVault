use cv_domain::{CalendarDate, MediaSource};
use cv_torrent::model::{
    Hdr, LibraryCopy, MediaKind, Release, Resolution, Scope, TitleIds, TorrentQuery,
};
use cv_torrent::rank::assess;
use cv_torrent::search::ResultRow;

use super::*;

fn date(text: &str) -> CalendarDate {
    text.parse().expect("date")
}

fn query() -> TorrentQuery {
    TorrentQuery {
        kind: MediaKind::Movie,
        ids: TitleIds::default(),
        title: "Example Movie".to_owned(),
        original_title: None,
        alternative_titles: Vec::new(),
        year: Some(2020),
        scope: Scope::Movie,
        runtime_minutes: Some(120),
        anime: false,
        library_copy: None,
    }
}

fn row(name: &str, size_gb: u64, seeders: Option<u32>) -> ResultRow {
    let release = Release {
        id: name.to_lowercase(),
        name: name.to_owned(),
        size: Some(size_gb * 1_000_000_000),
        seeders,
        ..Release::default()
    };
    let assessment = assess(&release, &query());
    ResultRow {
        id: release.id.clone(),
        absorbs: Vec::new(),
        release,
        assessment,
    }
}

const REMUX_DV: &str = "Example.Movie.2020.2160p.UHD.BluRay.REMUX.DV.HDR.HEVC.TrueHD.Atmos.7.1-GRP";
const WEB_1080: &str = "Example.Movie.2020.1080p.WEB-DL.DDP5.1.H.264-GRP";

fn criteria(min: Option<MinResolution>, sources: &[WishSource], hdr: bool) -> WishCriteria {
    WishCriteria {
        min_resolution: min,
        sources: sources.to_vec(),
        hdr,
        max_size: None,
        min_seeders: None,
    }
}

#[test]
fn a_release_meets_the_resolution_source_and_hdr_asked_for() {
    let remux = row(REMUX_DV, 60, Some(50));
    let web = row(WEB_1080, 6, Some(200));
    let wants = criteria(Some(MinResolution::R2160), &[WishSource::Remux], true);
    assert!(wants.met_by_release(&remux));
    assert!(!wants.met_by_release(&web), "1080p is below 2160p");

    let any_1080 = criteria(Some(MinResolution::R1080), &[], false);
    assert!(any_1080.met_by_release(&remux), "2160p is at least 1080p");
    assert!(any_1080.met_by_release(&web));

    let web_only = criteria(None, &[WishSource::WebDl], false);
    assert!(web_only.met_by_release(&web));
    assert!(!web_only.met_by_release(&remux));

    let hdr = criteria(None, &[], true);
    assert!(!hdr.met_by_release(&web), "no HDR in the name");
}

#[test]
fn size_and_seeders_limits_fail_unknown_values() {
    let mut wants = criteria(None, &[], false);
    wants.max_size = Some(40_000_000_000);
    assert!(
        !wants.met_by_release(&row(REMUX_DV, 60, Some(50))),
        "58 GB > 40 GB"
    );
    assert!(wants.met_by_release(&row(WEB_1080, 6, Some(50))));
    let mut unknown = row(WEB_1080, 6, Some(50));
    unknown.release.size = None;
    assert!(
        !wants.met_by_release(&unknown),
        "an unknown size fails a set limit"
    );

    let mut seeded = criteria(None, &[], false);
    seeded.min_seeders = Some(10);
    assert!(seeded.met_by_release(&row(WEB_1080, 6, Some(10))));
    assert!(!seeded.met_by_release(&row(WEB_1080, 6, Some(9))));
    assert!(!seeded.met_by_release(&row(WEB_1080, 6, None)));
}

#[test]
fn a_flagged_or_other_title_release_never_meets() {
    let any = WishCriteria::default();
    assert!(!any.met_by_release(&row("Example.Movie.2020.HDCAM.x264-GRP", 2, Some(90))));
    assert!(
        !any.met_by_release(&row(WEB_1080, 6, Some(0))),
        "no seeders is a warning"
    );
    assert!(
        !any.met_by_release(&row("Another.Film.1999.1080p.BluRay.x264-GRP", 8, Some(40))),
        "another title"
    );
    assert!(any.met_by_release(&row(WEB_1080, 6, Some(40))));
}

#[test]
fn a_library_copy_meets_the_criteria_and_a_bluray_counts_as_a_remux() {
    let copy = |resolution, source, hdr| LibraryCopy {
        resolution,
        source,
        hdr,
    };
    let uhd = copy(Resolution::R2160, MediaSource::Bluray, Hdr::DolbyVision);
    let web = copy(Resolution::R1080, MediaSource::WebDl, Hdr::None);
    let remux_hdr = criteria(Some(MinResolution::R2160), &[WishSource::Remux], true);
    assert!(remux_hdr.met_by_copy(Some(&uhd)));
    assert!(!remux_hdr.met_by_copy(Some(&web)));
    assert!(criteria(Some(MinResolution::R1080), &[], false).met_by_copy(Some(&web)));
    assert!(!criteria(None, &[WishSource::BluRay], false).met_by_copy(Some(&web)));
    let unread = copy(Resolution::Unknown, MediaSource::Unknown, Hdr::None);
    assert!(!criteria(Some(MinResolution::R720), &[], false).met_by_copy(Some(&unread)));
    assert!(
        WishCriteria::default().met_by_copy(None),
        "asks for nothing"
    );
    assert!(!criteria(Some(MinResolution::R720), &[], false).met_by_copy(None));
}

#[test]
fn criteria_round_trip_and_damaged_text_gives_the_defaults() {
    let wants = WishCriteria {
        min_resolution: Some(MinResolution::R2160),
        sources: vec![WishSource::Remux, WishSource::WebDl],
        hdr: true,
        max_size: Some(80_000_000_000),
        min_seeders: Some(5),
    };
    let json = wants.to_json();
    assert!(json.contains("\"minResolution\":\"2160p\""), "{json}");
    assert!(json.contains("\"WEB-DL\""), "{json}");
    assert_eq!(WishCriteria::from_json(&json), wants);
    assert!(WishCriteria::from_json("{\"hdr\":true}").hdr);
    assert_eq!(WishCriteria::from_json("not json"), WishCriteria::default());
}

#[test]
fn scopes_and_notes_read_back_what_was_written() {
    for scope in [
        WishScope::Movie,
        WishScope::Season(2),
        WishScope::Episode(3, 6),
        WishScope::Next,
    ] {
        assert_eq!(WishScope::parse(&scope.as_text()), Some(scope));
    }
    assert_eq!(WishScope::parse("season:x"), None);
    assert_eq!(WishScope::parse("episode:1"), None);
    assert_eq!(WishScope::parse("movie:1"), None);
    assert!(WishScope::Movie.fits(MediaKind::Movie));
    assert!(!WishScope::Next.fits(MediaKind::Movie));
    assert!(!WishScope::Movie.fits(MediaKind::Show));

    for note in [
        CheckNote::NoSource,
        CheckNote::NoNewEpisode,
        CheckNote::Airs(date("2026-11-10")),
        CheckNote::Failed("TMDB rejected the saved key.".to_owned()),
    ] {
        assert_eq!(CheckNote::parse(&note.as_text()), note);
    }
    assert_eq!(parse_code(&episode_code((3, 6))), Some((3, 6)));
}

fn episode(season: u16, episode: u16, aired: Option<&str>) -> EpisodeRow {
    EpisodeRow {
        season,
        episode,
        name: String::new(),
        air_date: aired.map(date),
    }
}

#[test]
fn the_next_episode_follows_the_newest_one_held() {
    let today = date("2026-11-03");
    let list = vec![
        episode(0, 1, Some("2020-01-01")),
        episode(3, 5, Some("2026-10-20")),
        episode(3, 6, Some("2026-10-27")),
        episode(3, 7, Some("2026-11-10")),
        episode(3, 8, None),
    ];
    assert_eq!(
        next_episode(&list, &[(3, 4), (3, 5)], today),
        NextEpisode::Look((3, 6))
    );
    assert_eq!(
        next_episode(&list, &[(3, 6)], today),
        NextEpisode::Airs((3, 7), date("2026-11-10"))
    );
    assert_eq!(next_episode(&list, &[(3, 7)], today), NextEpisode::None);
    assert_eq!(next_episode(&list, &[(3, 8)], today), NextEpisode::None);
    assert_eq!(
        next_episode(&list, &[], today),
        NextEpisode::Look((3, 6)),
        "nothing held: the newest aired one"
    );
    assert_eq!(
        next_episode(&list, &[(0, 1)], today),
        NextEpisode::Look((3, 6)),
        "specials do not count"
    );
}

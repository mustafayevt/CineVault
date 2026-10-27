//! Main-title selection from sample disc structures: a DVD with a decoy title set, Blu-ray
//! discs with several playlists, and the fallbacks. Each structure is checked as a folder and
//! inside ISO images, with identical results. No MediaInfo needed.

mod disc_support;

use std::io::Cursor;

use cv_media::disc::bluray::parse_playlist;
use cv_media::disc::dvd::{parse_titles, playback_ms, title_playback};
use cv_media::disc::{
    DiscError, DiscFs, DiscKind, FolderFs, IsoImage, MainTitle, find_main_title, open_path,
};
use disc_support::images::{UdfFlavor, iso9660, udf};
use disc_support::structure::{dvd_time, mpls, vmg_ifo, vts_ifo};
use disc_support::{pattern, write_tree};

type Files = Vec<(&'static str, Vec<u8>)>;

/// 45 kHz ticks in `seconds` and a half second.
const fn ticks(seconds: u32) -> u32 {
    seconds * 45_000
}

/// A DVD whose first title is a short trailer in title set 2 and whose second is the film in
/// title set 1, where the film is the *second* title of its set and plays the *second* chain.
fn dvd_files() -> Files {
    let pal = 1;
    vec![
        (
            "VIDEO_TS/VIDEO_TS.IFO",
            vmg_ifo(&[(2, 2, 1), (12, 1, 1), (1, 1, 2)]),
        ),
        ("VIDEO_TS/VIDEO_TS.VOB", pattern(9000, 1)),
        (
            "VIDEO_TS/VTS_01_0.IFO",
            vts_ifo(
                &[dvd_time(0, 2, 10, 0, pal), dvd_time(1, 31, 12, 12, pal)],
                &[(2, 12), (1, 1)],
            ),
        ),
        ("VIDEO_TS/VTS_01_1.VOB", pattern(7000, 2)),
        ("VIDEO_TS/VTS_01_2.VOB", pattern(6000, 3)),
        ("VIDEO_TS/VTS_01_0.VOB", pattern(1000, 4)),
        (
            "VIDEO_TS/VTS_02_0.IFO",
            vts_ifo(&[dvd_time(0, 3, 30, 0, pal)], &[(1, 2)]),
        ),
        ("VIDEO_TS/VTS_02_1.VOB", pattern(20_000, 5)),
    ]
}

/// A Blu-ray with a menu loop, an extras playlist, the film in two clips, and a clip on the
/// disc that is bigger than any the film uses.
fn bluray_files() -> Files {
    vec![
        ("BDMV/index.bdmv", b"INDX0200".to_vec()),
        (
            "BDMV/PLAYLIST/00000.mpls",
            mpls(&[("00001", 0, ticks(30))], &[1]),
        ),
        (
            "BDMV/PLAYLIST/00001.mpls",
            mpls(&[("00004", 0, ticks(300))], &[1, 1]),
        ),
        (
            "BDMV/PLAYLIST/00800.mpls",
            mpls(
                &[
                    ("00002", 0, ticks(3600)),
                    ("00003", 90_000, 90_000 + ticks(1800) + 22_500),
                ],
                &[1, 1, 1, 2],
            ),
        ),
        (
            "BDMV/PLAYLIST/00099.mpls",
            mpls(&[("00001", 0, ticks(1200))], &[]),
        ),
        ("BDMV/STREAM/00001.m2ts", pattern(5_000, 1)),
        ("BDMV/STREAM/00002.m2ts", pattern(20_000, 2)),
        ("BDMV/STREAM/00003.m2ts", pattern(30_000, 3)),
        ("BDMV/STREAM/00004.m2ts", pattern(90_000, 4)),
    ]
}

fn folder(files: &Files) -> (tempfile::TempDir, FolderFs) {
    let dir = tempfile::tempdir().unwrap();
    write_tree(dir.path(), files);
    let fs = FolderFs::new(dir.path());
    (dir, fs)
}

/// The same disc as a folder, an ISO 9660 image and both UDF flavours.
fn every_way(files: &Files) -> Vec<(&'static str, Box<dyn DiscFs>)> {
    // Keep the folder alive for the whole test by leaking the temp dir handle (tests are
    // short-lived processes).
    let (dir, fs) = folder(files);
    std::mem::forget(dir);
    let open = |bytes: Vec<u8>| -> Box<dyn DiscFs> {
        Box::new(IsoImage::open(Cursor::new(bytes)).unwrap())
    };
    vec![
        ("folder", Box::new(fs)),
        ("ISO 9660", open(iso9660(files))),
        ("UDF", open(udf(files, UdfFlavor::Plain))),
        (
            "UDF with metadata partition",
            open(udf(files, UdfFlavor::Metadata)),
        ),
    ]
}

fn titles(files: &Files) -> Vec<(&'static str, MainTitle)> {
    every_way(files)
        .into_iter()
        .map(|(label, fs)| {
            let title = find_main_title(fs.as_ref()).unwrap_or_else(|e| panic!("{label}: {e}"));
            (label, title)
        })
        .collect()
}

fn same_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[test]
fn a_dvd_main_title_is_the_longest_chain_of_the_right_title_set() {
    for (label, title) in titles(&dvd_files()) {
        assert_eq!(title.kind, DiscKind::Dvd, "{label}");
        // 1:31:12 and 12 PAL frames of 40 ms.
        assert_eq!(title.duration_ms, Some(5_472_480), "{label}");
        assert_eq!(title.chapters, 12, "{label}");
        assert!(
            title.source.starts_with("VTS_01_0.IFO"),
            "{label}: {}",
            title.source
        );
        assert!(
            title.source.ends_with("(title 2)"),
            "{label}: {}",
            title.source
        );
        let wanted = ["VIDEO_TS/VTS_01_1.VOB", "VIDEO_TS/VTS_01_2.VOB"];
        assert_eq!(title.files.len(), 2, "{label}");
        assert!(
            title.files.iter().zip(wanted).all(|(a, b)| same_name(a, b)),
            "{label}"
        );
        assert!(same_name(&title.stream_file, wanted[0]), "{label}");
    }
}

#[test]
fn a_blu_ray_main_title_is_the_longest_playlist() {
    for (label, title) in titles(&bluray_files()) {
        assert_eq!(title.kind, DiscKind::BluRay, "{label}");
        assert!(same_name(&title.source, "00800.mpls"), "{label}");
        // 3600 s plus 1800.5 s.
        assert_eq!(title.duration_ms, Some(5_400_500), "{label}");
        assert_eq!(title.chapters, 3, "{label}");
        assert_eq!(title.files.len(), 2, "{label}");
        assert!(
            same_name(&title.files[0], "BDMV/STREAM/00002.m2ts"),
            "{label}"
        );
        assert!(
            same_name(&title.files[1], "BDMV/STREAM/00003.m2ts"),
            "{label}"
        );
        // The biggest clip of the playlist, not the biggest on the disc (00004).
        assert!(
            same_name(&title.stream_file, "BDMV/STREAM/00003.m2ts"),
            "{label}"
        );
    }
}

#[test]
fn a_looping_playlist_does_not_outrun_the_film() {
    // A real disc has `00020.mpls`: one 74-second clip played 302 times (6 h 12 min), next to the
    // 2 h 21 min film. The film is the main title.
    let mut files = bluray_files();
    let loop_items: Vec<(&str, u32, u32)> = (0..302).map(|_| ("00004", 0, ticks(74))).collect();
    files.push(("BDMV/PLAYLIST/00020.mpls", mpls(&loop_items, &[])));
    for (label, title) in titles(&files) {
        assert!(same_name(&title.source, "00800.mpls"), "{label}");
        assert_eq!(title.duration_ms, Some(5_400_500), "{label}");
    }
}

#[test]
fn equally_long_playlists_pick_the_first_by_name() {
    let mut files = bluray_files();
    files.retain(|(path, _)| !path.contains("PLAYLIST"));
    files.push((
        "BDMV/PLAYLIST/00020.mpls",
        mpls(&[("00002", 0, ticks(600))], &[]),
    ));
    files.push((
        "BDMV/PLAYLIST/00010.mpls",
        mpls(&[("00003", 0, ticks(600))], &[]),
    ));
    for (label, title) in titles(&files) {
        assert!(same_name(&title.source, "00010.mpls"), "{label}");
        assert_eq!(title.duration_ms, Some(600_000), "{label}");
    }
}

#[test]
fn a_blu_ray_without_readable_playlists_falls_back_to_the_largest_clip() {
    let mut files = bluray_files();
    for (path, data) in &mut files {
        if path.contains("PLAYLIST") {
            *data = b"not a playlist".to_vec();
        }
    }
    for (label, title) in titles(&files) {
        assert_eq!(title.kind, DiscKind::BluRay, "{label}");
        assert_eq!(title.duration_ms, None, "{label}");
        assert!(
            same_name(&title.stream_file, "BDMV/STREAM/00004.m2ts"),
            "{label}"
        );
    }
}

#[test]
fn a_dvd_with_a_broken_video_manager_uses_the_longest_chain_of_any_title_set() {
    let mut files = dvd_files();
    files[0].1 = b"garbage".to_vec();
    for (label, title) in titles(&files) {
        assert_eq!(title.kind, DiscKind::Dvd, "{label}");
        assert_eq!(title.duration_ms, Some(5_472_480), "{label}");
        assert!(
            same_name(&title.stream_file, "VIDEO_TS/VTS_01_1.VOB"),
            "{label}"
        );
    }
}

#[test]
fn a_dvd_without_any_structure_file_falls_back_to_the_largest_vob_but_not_the_menu() {
    let files: Files = vec![
        ("VIDEO_TS/VIDEO_TS.VOB", pattern(90_000, 1)),
        ("VIDEO_TS/VTS_01_1.VOB", pattern(20_000, 2)),
        ("VIDEO_TS/VTS_02_1.VOB", pattern(30_000, 3)),
    ];
    for (label, title) in titles(&files) {
        assert_eq!(title.duration_ms, None, "{label}");
        assert!(
            same_name(&title.stream_file, "VIDEO_TS/VTS_02_1.VOB"),
            "{label}"
        );
    }
}

#[test]
fn an_hd_dvd_uses_its_largest_evo_file() {
    let files: Files = vec![
        ("HVDVD_TS/HV000I01.IFO", pattern(3000, 1)),
        ("HVDVD_TS/HVA00001.EVO", pattern(50_000, 2)),
        ("HVDVD_TS/HVA00002.EVO", pattern(10_000, 3)),
    ];
    for (label, title) in titles(&files) {
        assert_eq!(title.kind, DiscKind::HdDvd, "{label}");
        assert_eq!(title.duration_ms, None, "{label}");
        assert!(
            same_name(&title.stream_file, "HVDVD_TS/HVA00001.EVO"),
            "{label}"
        );
    }
}

#[test]
fn a_hybrid_disc_prefers_the_blu_ray_side() {
    let mut files = dvd_files();
    files.extend(bluray_files());
    for (label, title) in titles(&files) {
        assert_eq!(title.kind, DiscKind::BluRay, "{label}");
    }
}

#[test]
fn a_tree_without_a_disc_folder_is_not_a_disc() {
    let files: Files = vec![("Movies/film.mkv", pattern(100, 1))];
    for (label, fs) in every_way(&files) {
        let error = find_main_title(fs.as_ref()).unwrap_err();
        assert!(matches!(error, DiscError::NoDisc), "{label}: {error}");
    }
}

#[test]
fn a_disc_folder_with_nothing_usable_reports_why() {
    let files: Files = vec![("BDMV/index.bdmv", b"INDX".to_vec())];
    let (_dir, fs) = folder(&files);
    let error = find_main_title(&fs).unwrap_err();
    assert!(matches!(error, DiscError::Structure(_)), "{error}");
    assert!(error.to_string().contains("PLAYLIST"), "{error}");
}

#[test]
fn paths_open_as_the_disc_folder_its_parent_or_an_image() {
    let files = bluray_files();
    let dir = tempfile::tempdir().unwrap();
    let movie = dir.path().join("Film (2001)");
    write_tree(&movie, &files);
    let image = dir.path().join("Film.iso");
    std::fs::write(&image, udf(&files, UdfFlavor::Metadata)).unwrap();

    for path in [movie.clone(), movie.join("BDMV"), image] {
        let fs = open_path(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let title = find_main_title(fs.as_ref()).unwrap();
        assert_eq!(title.duration_ms, Some(5_400_500), "{}", path.display());
    }
    assert!(open_path(&dir.path().join("missing.iso")).is_err());
    let text = dir.path().join("notes.iso");
    std::fs::write(&text, b"just text").unwrap();
    assert!(matches!(open_path(&text), Err(DiscError::Io(_))));
}

// ---------------------------------------------------------------------------------------------
// The parsers on their own
// ---------------------------------------------------------------------------------------------

#[test]
fn dvd_times_are_bcd_with_a_frame_rate_flag() {
    assert_eq!(playback_ms(dvd_time(1, 31, 12, 0, 1)), 5_472_000);
    assert_eq!(playback_ms(dvd_time(0, 0, 1, 12, 1)), 1_480);
    // 29.97 fps: 15 frames are half a second (plus the 0.1% of the NTSC clock).
    assert_eq!(playback_ms(dvd_time(0, 0, 0, 15, 3)), 500);
    assert_eq!(playback_ms(dvd_time(10, 59, 59, 0, 0)), 39_599_000);
    // Without a rate flag the frames cannot be converted and are dropped.
    assert_eq!(playback_ms(dvd_time(0, 1, 0, 20, 0)), 60_000);
}

#[test]
fn the_video_manager_lists_titles_and_a_title_set_maps_them_to_chains() {
    let manager = parse_titles(&vmg_ifo(&[(2, 2, 1), (12, 1, 1), (1, 1, 2)])).unwrap();
    assert_eq!(manager.len(), 3);
    assert_eq!(
        (
            manager[1].title_set,
            manager[1].title_in_set,
            manager[1].chapters
        ),
        (1, 1, 12)
    );
    assert_eq!(manager[2].number, 3);

    let set = vts_ifo(
        &[dvd_time(0, 2, 10, 0, 1), dvd_time(1, 31, 12, 12, 1)],
        &[(2, 12), (1, 1)],
    );
    assert_eq!(title_playback(&set, 1).unwrap(), (5_472_480, 12));
    assert_eq!(title_playback(&set, 2).unwrap(), (130_000, 1));
    assert!(title_playback(&set, 3).is_err());
    assert!(title_playback(&set, 0).is_err());
    assert!(title_playback(&set[..3000], 1).is_err());
    assert!(parse_titles(b"DVDVIDEO-VTS").is_err());
    assert!(parse_titles(&vmg_ifo(&[(1, 1, 1)])[..2052]).is_err());
}

#[test]
fn playlists_sum_their_items_and_count_entry_marks() {
    let file = mpls(
        &[("00007", 45_000, 45_000 + ticks(10)), ("00008", 0, 22_500)],
        &[1, 2, 1, 1],
    );
    let list = parse_playlist(&file).unwrap();
    assert_eq!(list.items.len(), 2);
    assert_eq!(list.items[0].clip, "00007");
    assert_eq!(list.items[1].out_ticks, 22_500);
    assert_eq!(list.duration_ms(), 10_500);
    assert_eq!(list.chapters, 3);

    // An item whose out time is before its in time counts as nothing, not as a huge number.
    let odd = parse_playlist(&mpls(&[("00001", 1000, 10)], &[])).unwrap();
    assert_eq!(odd.duration_ms(), 0);
}

#[test]
fn damaged_playlists_are_errors_not_panics() {
    let file = mpls(&[("00001", 0, 45_000)], &[1]);
    assert!(parse_playlist(b"").is_err());
    assert!(parse_playlist(b"MPLS0200").is_err());
    assert!(parse_playlist(&pattern(300, 1)).is_err());
    for keep in [10, 40, 44, 50, 60] {
        // Every cut is either an error or a shorter but valid read; never a panic.
        let _ = parse_playlist(&file[..keep.min(file.len())]);
    }
}

//! Disc analysis with the real MediaInfo library: sample DVD and Blu-ray structures around
//! small FFmpeg-made streams, as folders and as ISO images, must report the main title's
//! duration from the disc structure and its streams from the stream file read through the
//! buffer interface.
//!
//! The tests need the tools (`bun run tools:fetch -- mediainfo ffmpeg`). Without them each one
//! prints why and returns. The images are built in the test (about a megabyte), so nothing is
//! committed.

mod disc_support;

use std::io::Cursor;
use std::path::Path;

use cv_domain::{MediaFile, MediaFileType};
use cv_media::disc::{
    DiscAnalysis, DiscKind, analyze_disc, analyze_path, disc_folder_of, open_path,
};
use cv_media::mediainfo::{MediaSnapshot, StreamKind, apply_snapshot};
use disc_support::images::{UdfFlavor, iso9660, udf};
use disc_support::structure::{dvd_time, mpls, vmg_ifo, vts_ifo};
use disc_support::{Tools, make_m2ts, make_vob, tools, write_tree};

type Files = Vec<(&'static str, Vec<u8>)>;

const fn ticks(seconds: u32) -> u32 {
    seconds * 45_000
}

/// A DVD: a short trailer in title set 2 and the film (1:31:12, 12 PAL frames) in title set 1.
/// The film's VOB files are tiny real MPEG-2 streams; the duration can only come from the IFO.
fn dvd(tools: &Tools, work: &Path) -> Files {
    let vob = make_vob(&tools.ffmpeg, work, 2);
    vec![
        ("VIDEO_TS/VIDEO_TS.IFO", vmg_ifo(&[(2, 2, 1), (12, 1, 1)])),
        (
            "VIDEO_TS/VTS_01_0.IFO",
            vts_ifo(&[dvd_time(1, 31, 12, 12, 1)], &[(1, 12)]),
        ),
        ("VIDEO_TS/VTS_01_1.VOB", vob.clone()),
        (
            "VIDEO_TS/VTS_02_0.IFO",
            vts_ifo(&[dvd_time(0, 3, 30, 0, 1)], &[(1, 2)]),
        ),
        ("VIDEO_TS/VTS_02_1.VOB", vob),
    ]
}

/// A Blu-ray with a menu playlist, extras and the film (5400.5 s) in two clips.
fn bluray(tools: &Tools, work: &Path) -> Files {
    let film = make_m2ts(&tools.ffmpeg, work, 3);
    let part = make_m2ts(&tools.ffmpeg, work, 2);
    vec![
        ("BDMV/index.bdmv", b"INDX0200".to_vec()),
        (
            "BDMV/PLAYLIST/00000.mpls",
            mpls(&[("00001", 0, ticks(30))], &[1]),
        ),
        (
            "BDMV/PLAYLIST/00800.mpls",
            mpls(
                &[
                    ("00002", 0, ticks(3600)),
                    ("00003", 90_000, 90_000 + ticks(1800) + 22_500),
                ],
                &[1, 1, 1],
            ),
        ),
        ("BDMV/STREAM/00001.m2ts", part.clone()),
        ("BDMV/STREAM/00002.m2ts", part),
        ("BDMV/STREAM/00003.m2ts", film),
    ]
}

fn mapped(snapshot: &MediaSnapshot) -> MediaFile {
    let mut file = MediaFile::new(MediaFileType::Video, "D:\\Movies", "Film (2001).iso");
    apply_snapshot(&mut file, snapshot);
    file
}

fn field<'a>(snapshot: &'a MediaSnapshot, kind: StreamKind, name: &str) -> &'a str {
    snapshot
        .streams(kind)
        .first()
        .and_then(|stream| stream.get(name))
        .unwrap_or_default()
}

/// The facts about the streams that must not depend on how the disc was opened.
fn stream_facts(snapshot: &MediaSnapshot) -> Vec<String> {
    let video = |name| field(snapshot, StreamKind::Video, name).to_owned();
    let audio = |name| field(snapshot, StreamKind::Audio, name).to_owned();
    vec![
        video("Format"),
        video("Width"),
        video("Height"),
        audio("Format"),
        audio("Channel(s)"),
        snapshot.streams(StreamKind::Audio).len().to_string(),
    ]
}

fn analyze_image(tools: &Tools, image: Vec<u8>) -> DiscAnalysis {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("disc.iso");
    std::fs::write(&path, image).unwrap();
    analyze_path(&tools.library, &path).expect("image analysis")
}

#[test]
fn a_dvd_folder_reports_the_main_title_duration_and_its_streams() {
    let Some(tools) = tools() else { return };
    let work = tempfile::tempdir().unwrap();
    let disc = tempfile::tempdir().unwrap();
    write_tree(disc.path(), &dvd(&tools, work.path()));

    let analysis = analyze_path(&tools.library, disc.path()).expect("analysis");
    assert_eq!(analysis.title.kind, DiscKind::Dvd);
    assert_eq!(analysis.title.duration_ms, Some(5_472_480));
    assert_eq!(analysis.title.chapters, 12);

    let snapshot = &analysis.snapshot;
    assert_eq!(field(snapshot, StreamKind::General, "Duration"), "5472480");
    assert_eq!(
        field(snapshot, StreamKind::General, "Duration/String3"),
        "01:31:12.480"
    );
    assert_eq!(field(snapshot, StreamKind::Video, "Format"), "MPEG Video");
    assert_eq!(field(snapshot, StreamKind::Video, "Width"), "720");
    assert_eq!(field(snapshot, StreamKind::Video, "Height"), "576");
    assert_eq!(field(snapshot, StreamKind::Audio, "Format"), "AC-3");

    let file = mapped(snapshot);
    assert_eq!(file.duration_secs, 5472);
    assert_eq!(file.exact_video_format, "576p");
    assert_eq!(file.audio_streams[0].codec, "AC3");
}

#[test]
fn a_blu_ray_folder_takes_runtime_from_the_longest_playlist() {
    let Some(tools) = tools() else { return };
    let work = tempfile::tempdir().unwrap();
    let disc = tempfile::tempdir().unwrap();
    write_tree(disc.path(), &bluray(&tools, work.path()));

    // Given the BDMV folder itself, as a scanner finds it.
    let analysis = analyze_path(&tools.library, &disc.path().join("BDMV")).expect("analysis");
    assert_eq!(analysis.title.kind, DiscKind::BluRay);
    assert_eq!(analysis.title.duration_ms, Some(5_400_500));
    assert!(analysis.title.stream_file.ends_with("00003.m2ts"));

    let snapshot = &analysis.snapshot;
    assert_eq!(field(snapshot, StreamKind::General, "Duration"), "5400500");
    assert_eq!(field(snapshot, StreamKind::Video, "Format"), "AVC");
    assert_eq!(field(snapshot, StreamKind::Video, "Width"), "640");
    assert_eq!(field(snapshot, StreamKind::Video, "Height"), "360");
    assert_eq!(field(snapshot, StreamKind::Audio, "Format"), "AC-3");
    let file = mapped(snapshot);
    assert_eq!(file.duration_secs, 5401);
    assert_eq!(file.video_codec, "AVC");

    // Given `BDMV/index.bdmv`, the video the library stores for a disc: its disc folder gives the
    // same main title, while the marker read as a file of its own holds no streams.
    let marker = disc.path().join("BDMV").join("index.bdmv");
    let folder = disc_folder_of(&marker).expect("a disc marker");
    let again = analyze_path(&tools.library, folder).expect("analysis");
    assert_eq!(again.title, analysis.title);
    assert_eq!(field(&again.snapshot, StreamKind::Video, "Width"), "640");
    let alone = tools.library.read(&marker).expect("the marker opens");
    assert!(!alone.has_media_streams());
}

#[test]
fn the_buffer_interface_sees_the_same_streams_as_reading_the_file() {
    let Some(tools) = tools() else { return };
    let work = tempfile::tempdir().unwrap();
    let disc = tempfile::tempdir().unwrap();
    write_tree(disc.path(), &bluray(&tools, work.path()));

    let direct = tools
        .library
        .read(&disc.path().join("BDMV/STREAM/00003.m2ts"))
        .expect("direct read");
    let analysis = analyze_path(&tools.library, disc.path()).expect("analysis");
    assert_eq!(
        stream_facts(&analysis.snapshot),
        stream_facts(&direct),
        "same streams through the buffer interface"
    );
    // Without the structure's duration the stream file alone is about three seconds.
    let seconds = field(&direct, StreamKind::General, "Duration")
        .parse::<f64>()
        .unwrap()
        / 1000.0;
    assert!((seconds - 3.0).abs() < 0.3, "{seconds}");
}

#[test]
fn iso_images_report_the_expected_main_title_duration() {
    let Some(tools) = tools() else { return };
    let work = tempfile::tempdir().unwrap();
    let dvd_files = dvd(&tools, work.path());
    let bluray_files = bluray(&tools, work.path());

    // The folder analysis is the baseline; every image of the same disc must agree with it.
    let folder = tempfile::tempdir().unwrap();
    write_tree(folder.path(), &dvd_files);
    let reference = analyze_path(&tools.library, folder.path()).unwrap();

    for (label, image) in [
        ("DVD, ISO 9660", iso9660(&dvd_files)),
        ("DVD, UDF 1.02", udf(&dvd_files, UdfFlavor::Plain)),
        ("DVD, UDF 2.50", udf(&dvd_files, UdfFlavor::Metadata)),
    ] {
        let analysis = analyze_image(&tools, image);
        assert_eq!(analysis.title.kind, DiscKind::Dvd, "{label}");
        assert_eq!(analysis.title.duration_ms, Some(5_472_480), "{label}");
        assert_eq!(
            field(&analysis.snapshot, StreamKind::General, "Duration"),
            "5472480",
            "{label}"
        );
        assert_eq!(
            stream_facts(&analysis.snapshot),
            stream_facts(&reference.snapshot),
            "{label}"
        );
    }

    for (label, image) in [
        ("Blu-ray, UDF 2.50", udf(&bluray_files, UdfFlavor::Metadata)),
        ("Blu-ray, UDF 1.02", udf(&bluray_files, UdfFlavor::Plain)),
    ] {
        let size = image.len();
        let analysis = analyze_image(&tools, image);
        assert_eq!(analysis.title.kind, DiscKind::BluRay, "{label}");
        assert_eq!(analysis.title.duration_ms, Some(5_400_500), "{label}");
        assert_eq!(
            field(&analysis.snapshot, StreamKind::General, "Duration"),
            "5400500",
            "{label}"
        );
        assert_eq!(
            field(&analysis.snapshot, StreamKind::Video, "Format"),
            "AVC"
        );
        assert!(size < 1024 * 1024, "{label}: the fixture is {size} bytes");
    }
}

#[test]
fn an_image_opened_in_memory_gives_the_same_answer_as_one_on_disk() {
    let Some(tools) = tools() else { return };
    let work = tempfile::tempdir().unwrap();
    let files = bluray(&tools, work.path());
    let image = udf(&files, UdfFlavor::Metadata);

    let in_memory = cv_media::disc::IsoImage::open(Cursor::new(image.clone())).unwrap();
    let from_memory = analyze_disc(&tools.library, &in_memory).unwrap();
    let on_disk = analyze_image(&tools, image);
    assert_eq!(from_memory.snapshot, on_disk.snapshot);
    assert_eq!(from_memory.title, on_disk.title);
}

#[test]
fn a_blu_ray_without_readable_playlists_still_reports_streams() {
    let Some(tools) = tools() else { return };
    let work = tempfile::tempdir().unwrap();
    let mut files = bluray(&tools, work.path());
    for (path, data) in &mut files {
        if path.contains("PLAYLIST") {
            *data = b"unreadable".to_vec();
        }
    }
    let disc = tempfile::tempdir().unwrap();
    write_tree(disc.path(), &files);

    let analysis = analyze_path(&tools.library, disc.path()).expect("fallback analysis");
    assert_eq!(analysis.title.duration_ms, None);
    assert!(analysis.title.stream_file.ends_with("00003.m2ts"));
    assert_eq!(
        field(&analysis.snapshot, StreamKind::Video, "Format"),
        "AVC"
    );
    assert!(mapped(&analysis.snapshot).duration_secs > 0);
}

#[test]
fn a_folder_that_is_not_a_disc_is_an_error() {
    let Some(tools) = tools() else { return };
    let disc = tempfile::tempdir().unwrap();
    write_tree(disc.path(), &[("film.mkv", vec![0_u8; 10])]);
    let error = analyze_path(&tools.library, disc.path()).unwrap_err();
    assert!(error.to_string().contains("No DVD, Blu-ray"), "{error}");
    assert!(open_path(&disc.path().join("nothing.iso")).is_err());
}

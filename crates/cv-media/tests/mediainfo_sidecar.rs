//! The MediaInfo XML sidecar: reading it as a cache, writing it, and the round trip against a
//! fixture sidecar.
//!
//! `fixtures/mediainfo/sidecar-uhd-feature-mediainfo.xml` is written in the exact form
//! [`render_sidecar`] produces, so reading it and writing it back must give the same bytes. The
//! recorded field sets of `mediainfo_mapping.rs` go through the same trip to show that nothing
//! the stream mapping needs is lost when a snapshot is kept as XML.

use std::path::{Path, PathBuf};

use cv_domain::{MediaFile, MediaFileType, StreamFlag, Video3dFormat};
use cv_media::mediainfo::{
    MediaSnapshot, SidecarState, SidecarWrite, StreamInfo, StreamKind, apply_snapshot,
    parse_sidecar, read_sidecar, render_sidecar, sidecar_paths, write_sidecar,
};

const FIXTURE: &str = "sidecar-uhd-feature-mediainfo.xml";
const FIXTURE_SIZE: u64 = 48_318_382_080;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mediainfo")
}

/// The fixture text with line endings normalised (a checkout may convert them).
fn fixture_text() -> String {
    std::fs::read_to_string(fixtures().join(FIXTURE))
        .expect("the fixture sidecar")
        .replace("\r\n", "\n")
}

fn parse_recorded(text: &str) -> MediaSnapshot {
    let mut snapshot = MediaSnapshot::new();
    let mut current: Option<(StreamKind, StreamInfo)> = None;
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if let Some((kind, stream)) = current.take() {
                snapshot.push(kind, stream);
            }
            let kind = StreamKind::ALL
                .into_iter()
                .find(|k| k.as_str() == name)
                .unwrap_or_else(|| panic!("unknown section [{name}]"));
            current = Some((kind, StreamInfo::new()));
        } else if let Some((name, value)) = line.split_once(':') {
            let (_, stream) = current.as_mut().expect("a field before any section");
            stream.push(name.trim(), value.trim());
        }
    }
    if let Some((kind, stream)) = current {
        snapshot.push(kind, stream);
    }
    snapshot
}

fn mapped(snapshot: &MediaSnapshot, filename: &str) -> MediaFile {
    let mut file = MediaFile::new(MediaFileType::Video, "D:\\Movies", filename);
    apply_snapshot(&mut file, snapshot);
    file
}

// ---------------------------------------------------------------------------------------------
// Round trip against the fixture sidecar
// ---------------------------------------------------------------------------------------------

#[test]
fn the_fixture_sidecar_maps_to_the_expected_streams() {
    let snapshot = parse_sidecar(&fixture_text()).expect("the fixture parses");
    let file = mapped(&snapshot, "Sample Feature (2021).mkv");

    assert_eq!(file.container_format, "mkv");
    assert_eq!(file.duration_secs, 7020, "seconds with decimals, rounded");
    assert_eq!(file.overall_bitrate_kbps, 55_062);
    assert_eq!(file.title, "Sample & Feature");
    assert_eq!(file.video_codec, "HEVC");
    assert_eq!((file.width, file.height), (3840, 1608));
    assert_eq!(file.exact_video_format, "1608p");
    assert_eq!(file.hdr_formats, ["Dolby Vision", "HDR10"]);
    assert_eq!(file.video_3d, Video3dFormat::None);
    assert_eq!(file.video_bitrate_kbps, 45_000);
    assert_eq!(file.bit_depth, 10);

    assert_eq!(file.audio_streams.len(), 2);
    assert_eq!(file.audio_streams[0].codec, "TrueHD Atmos");
    assert_eq!(
        file.audio_streams[0].channels, 8,
        "`Channels` is `Channel(s)`"
    );
    assert_eq!(file.audio_streams[0].language, "en");
    assert_eq!(file.audio_streams[1].codec, "AC3");
    assert_eq!(file.audio_streams[1].language, "fr");
    assert!(file.audio_streams[1].flags.contains(&StreamFlag::Comment));

    assert_eq!(file.subtitles.len(), 2);
    assert_eq!(file.subtitles[0].codec, "PGS");
    assert_eq!(file.subtitles[1].title, "Français (SDH)");
    assert!(
        file.subtitles[1]
            .flags
            .contains(&StreamFlag::HearingImpaired)
    );

    // The tag data the library reports as `extra` is kept as `extra/` fields.
    assert_eq!(
        snapshot.general().and_then(|g| g.get("extra/IMDB")),
        Some("tt0000001")
    );
}

#[test]
fn writing_the_parsed_fixture_gives_back_the_same_bytes() {
    let text = fixture_text();
    let snapshot = parse_sidecar(&text).expect("the fixture parses");
    let written = render_sidecar(
        &snapshot,
        "D:\\Movies\\Sample Feature (2021)\\Sample Feature (2021).mkv",
        "26.10",
    );
    assert_eq!(written, text);
}

#[test]
fn a_snapshot_survives_xml_and_maps_to_the_same_file() {
    let snapshot = parse_sidecar(&fixture_text()).unwrap();
    let again = parse_sidecar(&render_sidecar(&snapshot, "x.mkv", "")).unwrap();
    assert_eq!(again, snapshot);
    assert_eq!(
        mapped(&again, "Sample Feature (2021).mkv"),
        mapped(&snapshot, "Sample Feature (2021).mkv")
    );
}

#[test]
fn every_recorded_sample_maps_the_same_after_a_trip_through_xml() {
    let mut seen = 0;
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        let snapshot = parse_recorded(&std::fs::read_to_string(&path).unwrap());
        let xml = render_sidecar(&snapshot, "movie.mkv", "26.10");
        let back = parse_sidecar(&xml).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let name = "Movie (2000).mkv";
        assert_eq!(
            mapped(&back, name),
            mapped(&snapshot, name),
            "{}",
            path.display()
        );
        seen += 1;
    }
    assert!(seen >= 7, "{seen} samples");
}

// ---------------------------------------------------------------------------------------------
// The sidecar as a cache
// ---------------------------------------------------------------------------------------------

/// A video of the fixture's recorded size would be 45 GB, so tests use a small file and a
/// sidecar whose `FileSize` is set to match it.
fn video_with_sidecar(dir: &Path, size_text: &str) -> PathBuf {
    let video = dir.join("Sample Feature (2021).mkv");
    std::fs::write(&video, vec![0_u8; 4096]).unwrap();
    let text = fixture_text().replace(&FIXTURE_SIZE.to_string(), size_text);
    std::fs::write(&sidecar_paths(&video)[0], text).unwrap();
    video
}

#[test]
fn an_up_to_date_sidecar_stands_in_for_reading_the_video() {
    let dir = tempfile::tempdir().unwrap();
    let video = video_with_sidecar(dir.path(), "4096");
    match read_sidecar(&video).unwrap() {
        SidecarState::Fresh { path, snapshot } => {
            assert_eq!(path, sidecar_paths(&video)[0]);
            let file = mapped(&snapshot, "Sample Feature (2021).mkv");
            assert_eq!(file.duration_secs, 7020);
            assert_eq!(file.audio_streams.len(), 2);
        }
        other => panic!("expected a fresh sidecar, got {other:?}"),
    }
}

#[test]
fn the_dotted_name_is_read_too_and_the_dashed_one_wins() {
    let dir = tempfile::tempdir().unwrap();
    let video = video_with_sidecar(dir.path(), "4096");
    let [dashed, dotted] = sidecar_paths(&video);
    assert!(dashed.ends_with("Sample Feature (2021)-mediainfo.xml"));
    assert!(dotted.ends_with("Sample Feature (2021).mkv.mediainfo.xml"));

    std::fs::rename(&dashed, &dotted).unwrap();
    assert!(matches!(
        read_sidecar(&video).unwrap(),
        SidecarState::Fresh { path, .. } if path == dotted
    ));
}

#[test]
fn a_changed_video_makes_the_sidecar_stale() {
    let dir = tempfile::tempdir().unwrap();
    let video = video_with_sidecar(dir.path(), "5000");
    assert!(matches!(
        read_sidecar(&video).unwrap(),
        SidecarState::Stale { .. }
    ));
}

#[test]
fn a_missing_broken_or_empty_sidecar_is_never_used() {
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("a.mkv");
    std::fs::write(&video, b"video").unwrap();
    assert!(matches!(
        read_sidecar(&video).unwrap(),
        SidecarState::Missing
    ));

    let sidecar = &sidecar_paths(&video)[0];
    for text in [
        "not xml at all <",
        "<other/>",
        "<MediaInfo><media ref=\"a\"><track type=\"General\"><FileSize>5</FileSize></track></media></MediaInfo>",
    ] {
        std::fs::write(sidecar, text).unwrap();
        assert!(
            matches!(read_sidecar(&video).unwrap(), SidecarState::Unusable { .. }),
            "{text}"
        );
        // A sidecar that cannot be used is left exactly as it was.
        assert_eq!(std::fs::read_to_string(sidecar).unwrap(), text);
    }
}

// ---------------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------------

#[test]
fn nothing_is_written_unless_the_setting_is_on() {
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("a.mkv");
    std::fs::write(&video, b"video").unwrap();
    let snapshot = parse_sidecar(&fixture_text()).unwrap();

    assert_eq!(
        write_sidecar(&video, &snapshot, "26.10", false).unwrap(),
        SidecarWrite::Disabled
    );
    assert!(!sidecar_paths(&video)[0].exists());

    // A sidecar the user already has stays untouched while the setting is off.
    std::fs::write(&sidecar_paths(&video)[0], "mine").unwrap();
    assert_eq!(
        write_sidecar(&video, &snapshot, "26.10", false).unwrap(),
        SidecarWrite::Disabled
    );
    assert_eq!(
        std::fs::read_to_string(&sidecar_paths(&video)[0]).unwrap(),
        "mine"
    );
}

#[test]
fn an_enabled_write_makes_a_sidecar_that_reads_back_as_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("a.mkv");
    std::fs::write(&video, vec![1_u8; 777]).unwrap();
    let mut snapshot = parse_sidecar(&fixture_text()).unwrap();
    // The library's snapshot carries the size of the file it read; a fixture's is stale.
    snapshot.general_mut().unwrap().set("FileSize", "");

    let outcome = write_sidecar(&video, &snapshot, "26.10", true).unwrap();
    let SidecarWrite::Written(path) = outcome else {
        panic!("expected a write, got {outcome:?}");
    };
    assert_eq!(path, sidecar_paths(&video)[0]);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("<FileSize>777</FileSize>"));
    assert!(text.contains("xmlns=\"https://mediaarea.net/mediainfo\""));
    assert!(!dir.path().join("a-mediainfo.xml.tmp").exists());

    let SidecarState::Fresh { snapshot: back, .. } = read_sidecar(&video).unwrap() else {
        panic!("the new sidecar should be fresh");
    };
    assert_eq!(
        mapped(&back, "a.mkv"),
        mapped(&snapshot, "a.mkv"),
        "what was written maps like what was given"
    );
}

#[test]
fn an_existing_dotted_sidecar_is_updated_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("a.mkv");
    std::fs::write(&video, b"video").unwrap();
    let dotted = &sidecar_paths(&video)[1];
    std::fs::write(dotted, "old").unwrap();
    let snapshot = parse_sidecar(&fixture_text()).unwrap();

    let outcome = write_sidecar(&video, &snapshot, "", true).unwrap();
    assert_eq!(outcome, SidecarWrite::Written(dotted.clone()));
    assert!(!sidecar_paths(&video)[0].exists());
    assert!(
        std::fs::read_to_string(dotted)
            .unwrap()
            .starts_with("<?xml")
    );
}

#[test]
fn a_general_only_snapshot_is_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("a.mkv");
    std::fs::write(&video, b"").unwrap();
    let mut snapshot = MediaSnapshot::new();
    let mut general = StreamInfo::new();
    general.push("Format", "Matroska");
    snapshot.push(StreamKind::General, general);

    assert_eq!(
        write_sidecar(&video, &snapshot, "26.10", true).unwrap(),
        SidecarWrite::NothingToWrite
    );
    assert!(!sidecar_paths(&video)[0].exists());
}

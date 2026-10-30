//! Blu-ray clip information (`CLIPINF/*.clpi`): the stream attributes a disc keeps beside its
//! stream files, and what the disc analysis takes from them (the languages of the audio and
//! subtitle streams, and the Dolby Vision enhancement layer of an Ultra HD Blu-ray).
//!
//! The stream coding bytes are the ones recorded from real discs: an Ultra HD Blu-ray with Dolby
//! Vision (HEVC base layer 0x1011, enhancement layer 0x1015, AC-3 and TrueHD, four PGS tracks)
//! and a Blu-ray with many languages. No MediaInfo needed: the MediaInfo side is the field set
//! recorded from the same Ultra HD disc.

mod disc_support;

use cv_domain::{MediaFile, MediaFileType};
use cv_media::disc::bluray::{ClipStream, DynamicRange, clip_streams, parse_clip_info};
use cv_media::disc::{DiscFs, FolderFs, IsoImage, apply_clip_info};
use cv_media::mediainfo::{MediaSnapshot, StreamInfo, StreamKind, apply_snapshot};
use disc_support::images::iso9660;
use disc_support::structure::clpi;
use disc_support::write_tree;

/// The streams of the Ultra HD disc's main clip, as its clip information stores them.
fn uhd_clip() -> Vec<u8> {
    clpi(&[
        // HEVC 2160p, 23.976, 16:9, HDR10, BT.2020.
        (0x1011, &[0x24, 0x81, 0x30, 0x12, 0x00]),
        // HEVC 1080p, 23.976, 16:9, Dolby Vision, BT.2020: the enhancement layer.
        (0x1015, &[0x24, 0x61, 0x30, 0x22, 0x00]),
        // AC-3 and TrueHD with their language.
        (0x1100, &[0x81, 0x61, b'r', b'u', b's']),
        (0x1101, &[0x83, 0x61, b'e', b'n', b'g']),
        // PGS subtitles.
        (0x12a0, &[0x90, b'r', b'u', b's']),
        (0x12a1, &[0x90, b'r', b'u', b's']),
        (0x12a2, &[0x90, b'e', b'n', b'g']),
        (0x12a3, &[0x90, b'e', b'n', b'g']),
    ])
}

#[test]
fn clip_information_gives_languages_and_the_dynamic_range() {
    let streams = parse_clip_info(&uhd_clip()).unwrap();
    assert_eq!(streams.len(), 8);
    assert_eq!(
        streams[0],
        ClipStream {
            pid: 0x1011,
            coding_type: 0x24,
            language: None,
            dynamic_range: Some(DynamicRange::Hdr10),
            hdr10_plus: false,
        }
    );
    assert_eq!(streams[1].dynamic_range, Some(DynamicRange::DolbyVision));
    assert_eq!(streams[2].language.as_deref(), Some("rus"));
    assert_eq!(streams[3].language.as_deref(), Some("eng"));
    assert_eq!(streams[7].language.as_deref(), Some("eng"));

    // An HDR10+ stream sets the flag after the colour space.
    let plus = parse_clip_info(&clpi(&[(0x1011, &[0x24, 0x81, 0x30, 0x12, 0x80])])).unwrap();
    assert!(plus[0].hdr10_plus);
    // An HD Blu-ray's AVC stream has no dynamic range; text subtitles keep a character code
    // before the language.
    let hd = parse_clip_info(&clpi(&[
        (0x1011, &[0x1b, 0x61, 0x30, 0x00, 0x00]),
        (0x1800, &[0x92, 0x01, b'f', b'r', b'a']),
        (0x1100, &[0x86, 0x61, 0, 0, 0]),
    ]))
    .unwrap();
    assert_eq!(hd[0].dynamic_range, None);
    assert_eq!(hd[1].language.as_deref(), Some("fra"));
    assert_eq!(hd[2].language, None, "no letters, no language");
}

#[test]
fn damaged_clip_information_is_an_error_not_a_panic() {
    let whole = uhd_clip();
    for cut in [0, 4, 8, 15, 40, 45, 50, 60, whole.len() - 1] {
        let _ = parse_clip_info(&whole[..cut]);
    }
    assert!(parse_clip_info(b"MPLS0200").is_err());
    assert!(parse_clip_info(&whole[..50]).is_err());
}

fn disc_files() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("BDMV/index.bdmv", b"INDX0300".to_vec()),
        ("BDMV/CLIPINF/00000.clpi", uhd_clip()),
        ("BDMV/STREAM/00000.m2ts", vec![0; 192]),
    ]
}

#[test]
fn the_clip_of_a_stream_file_is_found_in_a_folder_an_image_and_the_backup() {
    let dir = tempfile::tempdir().unwrap();
    write_tree(dir.path(), &disc_files());
    let folder = FolderFs::new(dir.path());
    let image = IsoImage::open(std::io::Cursor::new(iso9660(&disc_files()))).unwrap();
    for fs in [&folder as &dyn DiscFs, &image] {
        let streams = clip_streams(fs, "BDMV/STREAM/00000.m2ts").unwrap();
        assert_eq!(streams.len(), 8);
        assert!(clip_streams(fs, "BDMV/STREAM/00001.m2ts").is_none());
    }

    // Only the backup copy is left.
    let backup = tempfile::tempdir().unwrap();
    write_tree(
        backup.path(),
        &[
            ("BDMV/index.bdmv", b"INDX0300".to_vec()),
            ("BDMV/BACKUP/CLIPINF/00000.clpi", uhd_clip()),
            ("BDMV/STREAM/00000.m2ts", vec![0; 192]),
        ],
    );
    let streams = clip_streams(&FolderFs::new(backup.path()), "BDMV/STREAM/00000.m2ts").unwrap();
    assert_eq!(streams[1].dynamic_range, Some(DynamicRange::DolbyVision));
}

fn recorded_uhd_snapshot() -> MediaSnapshot {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mediainfo/real-uhd-bluray-dolby-vision-dual-layer.txt");
    let text = std::fs::read_to_string(path).unwrap();
    let mut snapshot = MediaSnapshot::new();
    let mut current: Option<(StreamKind, StreamInfo)> = None;
    for line in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if let Some((kind, stream)) = current.take() {
                snapshot.push(kind, stream);
            }
            let kind = StreamKind::ALL
                .into_iter()
                .find(|k| k.as_str() == name)
                .unwrap();
            current = Some((kind, StreamInfo::new()));
        } else if let Some((name, value)) = line.split_once(':') {
            current.as_mut().unwrap().1.push(name.trim(), value.trim());
        }
    }
    if let Some((kind, stream)) = current {
        snapshot.push(kind, stream);
    }
    snapshot
}

fn mapped(snapshot: &MediaSnapshot) -> MediaFile {
    let mut file = MediaFile::new(MediaFileType::Video, "D:\\Movies\\Example (2025)", "BDMV");
    apply_snapshot(&mut file, snapshot);
    file
}

#[test]
fn an_ultra_hd_blu_ray_gets_its_dolby_vision_layer_and_languages_from_the_clip() {
    let mut snapshot = recorded_uhd_snapshot();
    // MediaInfo alone: no languages; the second HEVC stream already tells Dolby Vision.
    let before = mapped(&snapshot);
    assert!(before.audio_streams.iter().all(|a| a.language.is_empty()));

    apply_clip_info(&mut snapshot, &parse_clip_info(&uhd_clip()).unwrap());

    let enhancement = &snapshot.streams(StreamKind::Video)[1];
    assert_eq!(
        enhancement.get("HDR_Format"),
        Some("Dolby Vision / SMPTE ST 2086")
    );
    assert_eq!(enhancement.get("HDR_Format_Profile"), Some("dvhe.07 / "));
    // The base layer is left as MediaInfo reported it.
    assert_eq!(
        snapshot.streams(StreamKind::Video)[0].get("HDR_Format"),
        Some("SMPTE ST 2086")
    );

    let file = mapped(&snapshot);
    assert_eq!((file.width, file.height), (3840, 2160));
    assert_eq!(file.video_codec, "HEVC");
    assert_eq!(file.bit_depth, 10);
    assert_eq!(file.hdr_formats, ["Dolby Vision", "HDR10"]);
    assert_eq!(file.hdr_details.dolby_vision_profile, "7");
    assert_eq!(file.hdr_details.dolby_vision_compatibility, "Blu-ray");
    assert_eq!(file.hdr_details.mastering_primaries, "Display P3");
    assert_eq!(file.duration_secs, 5113);
    let audio: Vec<_> = file
        .audio_streams
        .iter()
        .map(|a| (a.codec.as_str(), a.channels, a.language.as_str()))
        .collect();
    assert_eq!(audio, [("AC3", 6, "ru"), ("TrueHD Atmos", 8, "en")]);
    let subtitles: Vec<_> = file
        .subtitles
        .iter()
        .map(|s| (s.codec.as_str(), s.language.as_str()))
        .collect();
    assert_eq!(
        subtitles,
        [("PGS", "ru"), ("PGS", "ru"), ("PGS", "en"), ("PGS", "en")]
    );
}

#[test]
fn what_media_info_reports_wins_over_the_clip() {
    let mut snapshot = MediaSnapshot::new();
    let mut audio = StreamInfo::new();
    audio.push("ID", "4352");
    audio.push("Format", "AC-3");
    audio.push("Language", "de");
    snapshot.push(StreamKind::Audio, audio);
    let mut video = StreamInfo::new();
    video.push("ID", "4117");
    video.push("Format", "HEVC");
    video.push("HDR_Format", "Dolby Vision / SMPTE ST 2086");
    video.push("HDR_Format_Profile", "dvhe.07 / ");
    snapshot.push(StreamKind::Video, video.clone());

    apply_clip_info(&mut snapshot, &parse_clip_info(&uhd_clip()).unwrap());

    assert_eq!(
        snapshot.streams(StreamKind::Audio)[0].get("Language"),
        Some("de")
    );
    assert_eq!(snapshot.streams(StreamKind::Video)[0], video);
}

#[test]
fn an_undetermined_or_unknown_clip_language_is_kept_as_it_is_or_left_out() {
    let mut snapshot = MediaSnapshot::new();
    for id in ["4352", "4353"] {
        let mut audio = StreamInfo::new();
        audio.push("ID", id);
        snapshot.push(StreamKind::Audio, audio);
    }
    let clip = parse_clip_info(&clpi(&[
        (0x1100, &[0x81, 0x61, b'u', b'n', b'd']),
        (0x1101, &[0x81, 0x61, b'c', b'm', b'n']),
    ]))
    .unwrap();
    apply_clip_info(&mut snapshot, &clip);
    let audio = snapshot.streams(StreamKind::Audio);
    assert_eq!(audio[0].get("Language"), None);
    assert_eq!(audio[1].get("Language"), Some("cmn"));
}

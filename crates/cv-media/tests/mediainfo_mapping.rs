//! MediaInfo fields to CineVault streams, checked against sample files with expected values.
//!
//! Two kinds of sample:
//!
//! * **Recorded field sets** (`tests/fixtures/mediainfo/*.txt`): the raw MediaInfo fields of a
//!   file, one `[Kind]` section per stream and `Name: value` per field. Exotic codecs (Atmos,
//!   DTS:X, Dolby Vision, HDR10+, 3D) cannot be generated, so these are written to match the
//!   library's output; the two `real-*` files were recorded from MediaInfo 26.10 on generated
//!   samples.
//! * **Generated files**: small files made with the FFmpeg from the development tools cache and
//!   read with the real library. Those tests need `bun run tools:fetch -- mediainfo ffmpeg`;
//!   without the tools each one prints why and returns.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cv_domain::{MediaFile, MediaFileType, StreamFlag, Video3dFormat};
use cv_media::mediainfo::{
    MediaInfoLibrary, MediaSnapshot, StreamInfo, StreamKind, apply_snapshot, dolby_vision_profile,
};
use cv_media::tools::{ToolId, ToolsManager, dev_cache_dir};

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn parse_fixture(text: &str) -> MediaSnapshot {
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
        } else {
            panic!("not a field: {line}");
        }
    }
    if let Some((kind, stream)) = current {
        snapshot.push(kind, stream);
    }
    snapshot
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mediainfo")
        .join(name)
}

fn load(name: &str) -> MediaSnapshot {
    let path = fixture_path(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    parse_fixture(&text)
}

fn mapped(name: &str, filename: &str) -> MediaFile {
    let mut file = MediaFile::new(MediaFileType::Video, "D:\\Movies", filename);
    apply_snapshot(&mut file, &load(name));
    file
}

fn flags(list: &[StreamFlag]) -> std::collections::BTreeSet<StreamFlag> {
    list.iter().copied().collect()
}

fn close(value: f32, wanted: f32) -> bool {
    (value - wanted).abs() < 0.005
}

// ---------------------------------------------------------------------------------------------
// Recorded exotic samples
// ---------------------------------------------------------------------------------------------

#[test]
fn dolby_vision_hdr10_with_truehd_atmos_and_subtitles() {
    let file = mapped(
        "uhd-dolby-vision-hdr10-truehd-atmos.txt",
        "Alien (1979).mkv",
    );

    assert!(file.has_media_info());
    assert_eq!(file.container_format, "mkv");
    assert_eq!(file.duration_secs, 7020);
    assert_eq!(file.overall_bitrate_kbps, 58_123);
    assert_eq!(file.title, "Alien");

    assert_eq!(file.video_codec, "HEVC");
    assert_eq!(file.codec_id, "V_MPEGH/ISO/HEVC");
    assert_eq!((file.width, file.height), (3840, 1608));
    assert_eq!(file.exact_video_format, "1608p");
    assert!(close(file.aspect_ratio, 2.388), "{}", file.aspect_ratio);
    assert!(close(file.pixel_aspect_ratio, 1.0));
    assert!((file.frame_rate - 23.976).abs() < 1e-9);
    assert_eq!(file.bit_depth, 10);
    assert_eq!(file.video_bitrate_kbps, 45_000);
    assert_eq!(file.hdr_formats, ["Dolby Vision", "HDR10"]);
    assert_eq!(file.video_3d, Video3dFormat::None);

    let audio = &file.audio_streams;
    assert_eq!(audio.len(), 3);
    assert_eq!(audio[0].codec, "TrueHD Atmos");
    assert_eq!(audio[0].codec_id, "A_TRUEHD");
    assert_eq!(audio[0].channels, 8, "7.1 bed; the 16 objects are ignored");
    assert_eq!(audio[0].bitrate_kbps, 4567);
    assert_eq!(audio[0].bit_depth, 24);
    assert_eq!(audio[0].language, "en");
    assert_eq!(audio[0].title, "Dolby TrueHD Atmos 7.1");
    assert_eq!(audio[0].flags, flags(&[StreamFlag::Default]));

    assert_eq!(audio[1].codec, "AC3");
    assert_eq!(audio[1].channels, 6);
    assert_eq!(audio[1].bitrate_kbps, 640);
    assert_eq!(audio[1].flags, flags(&[StreamFlag::Comment]));

    assert_eq!(audio[2].codec, "E-AC-3 Atmos");
    assert_eq!(audio[2].channels, 6);
    assert_eq!(audio[2].language, "fr");
    assert!(audio[2].flags.is_empty());

    let subs = &file.subtitles;
    assert_eq!(subs.len(), 4);
    assert_eq!(
        (subs[0].codec.as_str(), subs[0].language.as_str()),
        ("PGS", "en")
    );
    assert_eq!(subs[0].flags, flags(&[StreamFlag::Default]));
    assert_eq!(subs[1].title, "Forced");
    assert_eq!(subs[1].flags, flags(&[StreamFlag::Forced]));
    assert_eq!(subs[2].codec, "SubRip");
    assert_eq!(subs[2].language, "fr");
    assert_eq!(subs[2].flags, flags(&[StreamFlag::HearingImpaired]));
    assert_eq!(subs[3].language, "tr");
    assert!(subs[3].flags.is_empty());

    let snapshot = load("uhd-dolby-vision-hdr10-truehd-atmos.txt");
    assert_eq!(dolby_vision_profile(snapshot.video().unwrap()), Some(8));
}

#[test]
fn every_dts_variant_and_plain_truehd() {
    let file = mapped("bluray-dts-variants.txt", "Heat (1995).mkv");
    assert_eq!(file.video_codec, "AVC");
    assert_eq!(file.exact_video_format, "1080p");
    assert!(file.hdr_formats.is_empty());

    // (codec, channels, kb/s, language). The DTS-HD rates add the core to the extension.
    let wanted = [
        ("DTS-HD MA", 8, 5770, "en"),
        ("DTS-X", 8, 4509, "en"),
        ("DTS-X IMAX", 8, 4509, "de"),
        ("DTS-HD HRA", 8, 2046, "it"),
        ("DTS-ES", 7, 1509, "es"),
        ("DTS 96/24", 6, 1509, "ja"),
        ("DTS", 6, 754, "pt"),
        ("TrueHD", 6, 2900, "ko"),
    ];
    assert_eq!(file.audio_streams.len(), wanted.len());
    for (stream, (codec, channels, kbps, language)) in file.audio_streams.iter().zip(wanted) {
        assert_eq!(stream.codec, codec);
        assert_eq!(stream.channels, channels, "{codec}");
        assert_eq!(stream.bitrate_kbps, kbps, "{codec}");
        assert_eq!(stream.language, language, "{codec}");
    }
    assert_eq!(file.audio_streams[0].bit_depth, 24);
    assert_eq!(file.audio_streams[0].flags, flags(&[StreamFlag::Default]));

    assert_eq!(file.subtitles.len(), 1);
    assert_eq!(file.subtitles[0].codec, "VobSub");
    assert_eq!(file.subtitles[0].language, "fr");
    assert_eq!(file.subtitles[0].flags, flags(&[StreamFlag::Forced]));
}

#[test]
fn dolby_vision_profile_7_with_hdr10_and_hdr10_plus_lists_the_dynamic_formats() {
    let file = mapped("hdr10plus-dolby-vision-profile7.txt", "Movie.mkv");
    assert_eq!(file.hdr_formats, ["Dolby Vision", "HDR10+"]);
    assert_eq!(file.exact_video_format, "2160p");
    assert_eq!(file.video_3d, Video3dFormat::None);
    assert_eq!(file.audio_streams[0].codec, "E-AC-3 Atmos");
    let snapshot = load("hdr10plus-dolby-vision-profile7.txt");
    assert_eq!(dolby_vision_profile(snapshot.video().unwrap()), Some(7));
}

#[test]
fn a_side_by_side_multi_view_file_is_3d_with_the_size_of_one_eye() {
    let file = mapped("side-by-side-3d.txt", "Avatar (2009).mkv");
    assert_eq!(file.video_3d, Video3dFormat::SideBySide);
    assert_eq!((file.width, file.height), (1920, 1080));
    assert_eq!(file.exact_video_format, "1080p");
    assert!(close(file.aspect_ratio, 1.778), "{}", file.aspect_ratio);
    assert_eq!(file.audio_streams[0].codec, "DTS-HD MA");
}

#[test]
fn a_half_side_by_side_filename_marks_a_1080p_file() {
    let file = mapped(
        "uhd-dolby-vision-hdr10-truehd-atmos.txt",
        "Movie 3D HSBS.mkv",
    );
    // MediaInfo reports no multi-view layout; the filename names the half variant.
    assert_eq!(file.video_3d, Video3dFormat::HalfSideBySide);
    assert_eq!((file.width, file.height), (3840, 1608));
}

#[test]
fn an_interlaced_pal_dvd_title() {
    let file = mapped("dvd-pal-mpeg2-interlaced.txt", "VTS_01_1.VOB");
    assert_eq!(file.video_codec, "MPEG-2");
    assert_eq!(file.container_format, "vob");
    assert_eq!(file.exact_video_format, "576i");
    assert!(close(file.aspect_ratio, 1.7775), "{}", file.aspect_ratio);
    assert_eq!(file.video_bitrate_kbps, 6000);
    assert_eq!(file.overall_bitrate_kbps, 6500);
    assert_eq!(file.duration_secs, 5400);
    assert!((file.frame_rate - 25.0).abs() < 1e-9);
    assert_eq!(file.audio_streams[0].codec, "AC3");
    assert_eq!(file.audio_streams[0].channels, 2);
    assert_eq!(file.audio_streams[0].bitrate_kbps, 192);
}

// ---------------------------------------------------------------------------------------------
// Recorded real output, and the same files generated live
// ---------------------------------------------------------------------------------------------

/// HEVC 10-bit HDR10, E-AC-3 5.1 English (default), AC-3 2.0 French commentary.
fn check_hdr_sample(file: &MediaFile) {
    assert_eq!(file.container_format, "mkv");
    assert_eq!(file.duration_secs, 2);
    assert_eq!(file.video_codec, "HEVC");
    assert_eq!((file.width, file.height), (320, 180));
    assert_eq!(file.exact_video_format, "180p");
    assert!(close(file.aspect_ratio, 1.778), "{}", file.aspect_ratio);
    assert_eq!(file.bit_depth, 10);
    assert!((file.frame_rate - 24.0).abs() < 1e-9);
    assert_eq!(file.hdr_formats, ["HDR10"]);
    assert_eq!(file.video_3d, Video3dFormat::None);

    assert_eq!(file.audio_streams.len(), 2);
    let first = &file.audio_streams[0];
    assert_eq!(first.codec, "E-AC-3");
    assert_eq!(first.codec_id, "A_EAC3");
    assert_eq!((first.channels, first.bitrate_kbps), (6, 384));
    assert_eq!(first.language, "en");
    assert_eq!(first.flags, flags(&[StreamFlag::Default]));
    let second = &file.audio_streams[1];
    assert_eq!(second.codec, "AC3");
    assert_eq!((second.channels, second.bitrate_kbps), (2, 192));
    assert_eq!(second.language, "fr");
    assert_eq!(second.title, "Commentary by the director");
    assert_eq!(second.flags, flags(&[StreamFlag::Comment]));
    assert!(file.subtitles.is_empty());
}

/// AVC, DTS core 5.1 German, MP3 mono without a language, and three subtitle tracks.
fn check_mix_sample(file: &MediaFile) {
    assert_eq!(file.container_format, "mkv");
    assert_eq!(file.duration_secs, 2);
    assert_eq!(file.video_codec, "AVC");
    assert!(file.hdr_formats.is_empty());

    assert_eq!(file.audio_streams.len(), 2);
    let dts = &file.audio_streams[0];
    assert_eq!(dts.codec, "DTS");
    assert_eq!(
        (dts.channels, dts.bitrate_kbps, dts.bit_depth),
        (6, 1413, 16)
    );
    assert_eq!(dts.language, "de");
    let mp3 = &file.audio_streams[1];
    assert_eq!(mp3.codec, "MP3");
    assert_eq!((mp3.channels, mp3.bitrate_kbps), (1, 64));
    assert_eq!(mp3.language, "");

    assert_eq!(file.subtitles.len(), 3);
    let turkish = &file.subtitles[0];
    assert_eq!(
        (turkish.codec.as_str(), turkish.language.as_str()),
        ("SubRip", "tr")
    );
    assert_eq!(turkish.title, "Turkish SDH");
    assert_eq!(
        turkish.flags,
        flags(&[StreamFlag::Forced, StreamFlag::HearingImpaired])
    );
    let english = &file.subtitles[1];
    assert_eq!(
        (english.codec.as_str(), english.language.as_str()),
        ("ASS", "en")
    );
    assert!(english.flags.is_empty());
    let french = &file.subtitles[2];
    assert_eq!(french.language, "fr");
    assert_eq!(
        french.flags,
        flags(&[
            StreamFlag::Default,
            StreamFlag::Forced,
            StreamFlag::HearingImpaired
        ])
    );
}

/// A Matroska file whose container flags mark Japanese as Original and Default, and an English
/// track as Commentary although its title ("Director") does not say so.
fn check_flagged_sample(file: &MediaFile) {
    assert_eq!(file.container_format, "mkv");
    assert_eq!(file.audio_streams.len(), 3);
    let [japanese, english, director] = &file.audio_streams[..] else {
        panic!("three audio tracks")
    };
    assert_eq!(japanese.language, "ja");
    assert_eq!(
        japanese.flags,
        flags(&[StreamFlag::Default, StreamFlag::Original])
    );
    assert_eq!(english.language, "en");
    assert!(english.flags.is_empty());
    assert_eq!(
        (director.language.as_str(), director.title.as_str()),
        ("en", "Director")
    );
    assert_eq!(director.flags, flags(&[StreamFlag::Comment]));
}

#[test]
fn recorded_matroska_original_and_commentary_flags() {
    check_flagged_sample(&mapped("real-mkv-original-commentary.txt", "flags.mkv"));
}

#[test]
fn generated_matroska_original_and_commentary_flags() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("flags.mkv");
    ffmpeg(
        &tools,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x90:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:sample_rate=48000",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=220:sample_rate=48000",
            "-t",
            "1",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-map",
            "3:a",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac",
            "-metadata:s:a:0",
            "language=jpn",
            "-metadata:s:a:1",
            "language=eng",
            "-metadata:s:a:2",
            "language=eng",
            "-metadata:s:a:2",
            "title=Director",
            "-disposition:a:0",
            "default+original",
            "-disposition:a:1",
            "0",
            "-disposition:a:2",
            "comment",
        ],
        &path,
    );
    check_flagged_sample(&read_mapped(&tools, &path));
}

#[test]
fn recorded_hevc_hdr10_sample() {
    check_hdr_sample(&mapped("real-hevc-hdr10-eac3.txt", "hdr.mkv"));
}

#[test]
fn recorded_avc_dts_mp3_subtitle_sample() {
    let file = mapped("real-avc-dts-mp3-subtitles.txt", "mix.mkv");
    check_mix_sample(&file);
    // The Technical level's Audio list shows the sample rate MediaInfo reports.
    let rates: Vec<u32> = file
        .audio_streams
        .iter()
        .map(|a| a.sample_rate_hz)
        .collect();
    assert_eq!(rates, [48_000, 48_000]);
}

struct Tools {
    manager: ToolsManager,
    ffmpeg: PathBuf,
}

fn tools() -> Option<Tools> {
    let manager = ToolsManager::new(dev_cache_dir());
    let missing = [ToolId::MediaInfo, ToolId::Ffmpeg]
        .into_iter()
        .find(|tool| manager.resolve(*tool).is_err());
    if let Some(tool) = missing {
        eprintln!("skipped: {tool} is not in the dev tools cache; run `bun run tools:fetch`");
        return None;
    }
    let ffmpeg = manager.resolve(ToolId::Ffmpeg).ok()?.path;
    Some(Tools { manager, ffmpeg })
}

fn ffmpeg(tools: &Tools, args: &[&str], output: &Path) {
    let status = Command::new(&tools.ffmpeg)
        .args(["-v", "error", "-y"])
        .args(args)
        .arg(output)
        .stdin(Stdio::null())
        .status()
        .expect("FFmpeg starts");
    assert!(status.success(), "FFmpeg failed for {}", output.display());
}

fn read_mapped(tools: &Tools, path: &Path) -> MediaFile {
    let library = MediaInfoLibrary::from_tools(&tools.manager).expect("library loads");
    let snapshot = library.read(path).expect("file reads");
    let name = path.file_name().unwrap().to_string_lossy();
    let mut file = MediaFile::new(
        MediaFileType::Video,
        path.parent().unwrap().to_string_lossy(),
        name,
    );
    apply_snapshot(&mut file, &snapshot);
    file
}

#[test]
fn generated_hevc_hdr10_file_maps_like_the_recording() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("hdr.mkv");
    ffmpeg(
        &tools,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x180:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:sample_rate=48000",
            "-t",
            "2",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-c:v",
            "libx265",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p10le",
            "-x265-params",
            "colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400:log-level=none",
            "-c:a:0",
            "eac3",
            "-ac:a:0",
            "6",
            "-b:a:0",
            "384k",
            "-c:a:1",
            "ac3",
            "-ac:a:1",
            "2",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:a:1",
            "language=fre",
            "-metadata:s:a:1",
            "title=Commentary by the director",
            "-disposition:a:0",
            "default",
        ],
        &path,
    );
    check_hdr_sample(&read_mapped(&tools, &path));
}

#[test]
fn generated_dts_mp3_and_subtitle_file_maps_like_the_recording() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let srt = dir.path().join("a.srt");
    std::fs::write(&srt, "1\n00:00:00,000 --> 00:00:01,000\nHello\n").unwrap();
    let ass = dir.path().join("a.ass");
    std::fs::write(
        &ass,
        "[Script Info]\nScriptType: v4.00+\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize\nStyle: Default,Arial,20\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,Hello\n",
    )
    .unwrap();
    let (srt, ass) = (
        srt.to_string_lossy().into_owned(),
        ass.to_string_lossy().into_owned(),
    );
    let path = dir.path().join("mix.mkv");
    ffmpeg(
        &tools,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x180:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:sample_rate=48000",
            "-i",
            &srt,
            "-i",
            &ass,
            "-i",
            &srt,
            "-t",
            "2",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-map",
            "3",
            "-map",
            "4",
            "-map",
            "5",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a:0",
            "dca",
            "-strict",
            "-2",
            "-ac:a:0",
            "6",
            "-c:a:1",
            "libmp3lame",
            "-c:s",
            "srt",
            "-c:s:1",
            "ass",
            "-metadata:s:a:0",
            "language=ger",
            "-metadata:s:s:0",
            "language=tur",
            "-metadata:s:s:0",
            "title=Turkish SDH",
            "-disposition:s:0",
            "forced",
            "-metadata:s:s:1",
            "language=eng",
            "-metadata:s:s:2",
            "language=fre",
            "-metadata:s:s:2",
            "title=Forced",
            "-disposition:s:2",
            "default+forced+hearing_impaired",
        ],
        &path,
    );
    check_mix_sample(&read_mapped(&tools, &path));
}

#[test]
fn generated_mp4_with_aac_and_timed_text_maps_the_common_path() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let srt = dir.path().join("a.srt");
    std::fs::write(&srt, "1\n00:00:00,000 --> 00:00:01,000\nHello\n").unwrap();
    let srt = srt.to_string_lossy().into_owned();
    let path = dir.path().join("common.mp4");
    ffmpeg(
        &tools,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=1280x720:rate=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-i",
            &srt,
            "-t",
            "3",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-ac",
            "2",
            "-c:s",
            "mov_text",
            "-metadata:s:a:0",
            "language=spa",
            "-metadata:s:s:0",
            "language=eng",
        ],
        &path,
    );
    let file = read_mapped(&tools, &path);
    assert_eq!(file.container_format, "mp4");
    assert_eq!(file.duration_secs, 3);
    assert_eq!(file.video_codec, "AVC");
    assert_eq!((file.width, file.height), (1280, 720));
    assert_eq!(file.exact_video_format, "720p");
    assert!(close(file.aspect_ratio, 1.778));
    assert!((file.frame_rate - 25.0).abs() < 1e-9);
    assert_eq!(file.bit_depth, 8);
    assert!(file.hdr_formats.is_empty());
    assert_eq!(file.audio_streams.len(), 1);
    assert_eq!(file.audio_streams[0].codec, "AAC");
    assert_eq!(file.audio_streams[0].channels, 2);
    assert_eq!(file.audio_streams[0].language, "es");
    assert_eq!(file.subtitles.len(), 1);
    assert_eq!(file.subtitles[0].language, "en");
}

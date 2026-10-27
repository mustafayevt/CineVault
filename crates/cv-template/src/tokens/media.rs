//! The technical fields of an item's files, shared by the movie and TV models.

use std::collections::BTreeSet;

use cv_domain::{MediaFile, MediaFileType, Video3dFormat};
use cv_media::resolution::resolution_label;

use super::labels::audio_channels_dot;
use crate::value::Value;

/// The fields `mediaInfo*`, `video*` and `CRC32` of the model, read from the item's files.
///
/// The stream data comes from the first video file; the file size adds up all video files, so a
/// stacked movie reports its full size. Missing data is empty text, an empty list or `Null`.
pub(super) fn media_fields(files: &[MediaFile]) -> Vec<(&'static str, Value)> {
    // A disc's marker file next to its disc folder is not a video of its own.
    let videos = || {
        files
            .iter()
            .filter(|f| f.kind == MediaFileType::Video && !f.is_covered_disc_marker(files))
    };
    let main = videos().next();
    let text = |f: fn(&MediaFile) -> String| Value::text(main.map(f).unwrap_or_default());
    let audio = main.map_or(&[][..], |m| m.audio_streams.as_slice());
    let strings = |items: Vec<String>| Value::list(items.into_iter().map(Value::from));

    let languages: Vec<String> = audio
        .iter()
        .map(|a| a.language.clone())
        .filter(|l| !l.is_empty())
        .collect();
    // Subtitle languages are listed once each, in alphabetical order.
    let subtitle_languages: BTreeSet<String> = main
        .map(|m| {
            m.subtitles
                .iter()
                .map(|s| s.language.clone())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let hdr: Vec<&str> = main.map_or_else(Vec::new, |m| {
        m.hdr_formats.iter().map(String::as_str).collect()
    });
    let size: u64 = videos().map(|f| f.size_bytes).sum();

    vec![
        (
            "mediaInfoVideoCodec",
            text(|f| video_codec_name(&f.video_codec)),
        ),
        (
            "mediaInfoVideoFormat",
            text(|f| {
                resolution_label(f.width, f.height)
                    .unwrap_or_default()
                    .to_owned()
            }),
        ),
        (
            "mediaInfoAspectRatioAsString",
            text(|f| aspect_ratio_text(f.aspect_ratio)),
        ),
        (
            "mediaInfoAspectRatio2AsString",
            text(|f| aspect_ratio_text(f.aspect_ratio2)),
        ),
        (
            "mediaInfoVideoResolution",
            text(|f| {
                if f.width > 0 && f.height > 0 {
                    format!("{}x{}", f.width, f.height)
                } else {
                    String::new()
                }
            }),
        ),
        (
            "mediaInfoVideoBitDepth",
            Value::Int(main.map_or(0, |f| i64::from(f.bit_depth))),
        ),
        (
            "mediaInfoVideoBitrate",
            main.filter(|f| f.video_bitrate_kbps > 0)
                .map_or(Value::Null, |f| Value::Int(i64::from(f.video_bitrate_kbps))),
        ),
        (
            "mediaInfoFrameRate",
            main.filter(|f| f.frame_rate > 0.0)
                .map_or(Value::Null, |f| Value::Float(f.frame_rate)),
        ),
        (
            "mediaInfoAudioCodec",
            Value::text(audio.first().map_or("", |a| a.codec.as_str())),
        ),
        (
            "mediaInfoAudioCodecList",
            strings(
                audio
                    .iter()
                    .map(|a| a.codec.clone())
                    .filter(|c| !c.is_empty())
                    .collect(),
            ),
        ),
        (
            "mediaInfoAudioChannels",
            Value::text(
                audio
                    .first()
                    .map_or_else(String::new, |a| channels(a.channels)),
            ),
        ),
        (
            "mediaInfoAudioChannelList",
            strings(audio.iter().map(|a| channels(a.channels)).collect()),
        ),
        (
            "mediaInfoAudioChannelsDot",
            Value::text(
                audio
                    .first()
                    .map_or_else(String::new, |a| audio_channels_dot(a.channels)),
            ),
        ),
        (
            "mediaInfoAudioChannelDotList",
            strings(
                audio
                    .iter()
                    .map(|a| audio_channels_dot(a.channels))
                    .collect(),
            ),
        ),
        (
            "mediaInfoAudioLanguage",
            Value::text(audio.first().map_or("", |a| a.language.as_str())),
        ),
        ("mediaInfoAudioLanguageList", strings(languages)),
        (
            "mediaInfoSubtitleLanguageList",
            strings(subtitle_languages.into_iter().collect()),
        ),
        (
            "video3DFormat",
            text(|f| three_d_name(f.video_3d, false).to_owned()),
        ),
        (
            "video3DFormat2",
            text(|f| three_d_name(f.video_3d, true).to_owned()),
        ),
        (
            "videoHDR",
            Value::text(if hdr.is_empty() { "" } else { "HDR" }),
        ),
        ("videoHDRFormat", Value::text(hdr.join(", "))),
        (
            "videoFilesize",
            Value::Int(i64::try_from(size).unwrap_or(i64::MAX)),
        ),
        (
            "CRC32",
            Value::text(
                main.and_then(|f| f.checksums.get("crc32"))
                    .map_or("", String::as_str),
            ),
        ),
    ]
}

/// `8ch` for eight channels; nothing for none.
fn channels(count: u32) -> String {
    if count == 0 {
        String::new()
    } else {
        format!("{count}ch")
    }
}

/// The codec as file names write it: `h264` and `h265` for AVC and HEVC, the stored name for
/// everything else (`MPEG-2`, `VC-1`, `AV1`, `VP9`, `XviD`).
fn video_codec_name(codec: &str) -> String {
    match codec.to_ascii_lowercase().as_str() {
        "avc" | "h264" | "h.264" => "h264".to_owned(),
        "hevc" | "h265" | "h.265" => "h265".to_owned(),
        _ => codec.to_owned(),
    }
}

/// The ratio times 100 as a whole number: 1.78 is `178`, 2.39 is `239`.
fn aspect_ratio_text(ratio: f32) -> String {
    if ratio > 0.0 {
        format!("{}", (f64::from(ratio) * 100.0).round() as i64)
    } else {
        String::new()
    }
}

/// The 3D layout. `video3DFormat` names the half-width and half-height variants for the plain
/// side-by-side and top-and-bottom layouts (`3D HSBS`, `3D HTAB`); `video3DFormat2` is the
/// layout as stored.
fn three_d_name(format: Video3dFormat, as_stored: bool) -> &'static str {
    match (format, as_stored) {
        (Video3dFormat::None, _) => "",
        (Video3dFormat::ThreeD, _) => "3D",
        (Video3dFormat::SideBySide, true) => "3D SBS",
        (Video3dFormat::SideBySide, false) | (Video3dFormat::HalfSideBySide, _) => "3D HSBS",
        (Video3dFormat::TopAndBottom, true) => "3D TAB",
        (Video3dFormat::TopAndBottom, false) | (Video3dFormat::HalfTopAndBottom, _) => "3D HTAB",
        (Video3dFormat::Mvc, _) => "3D MVC",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_domain::{AudioStream, SubtitleStream};

    fn field(fields: &[(&'static str, Value)], name: &str) -> String {
        fields
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.to_text(true))
            .unwrap_or_else(|| panic!("no field {name}"))
    }

    fn video() -> MediaFile {
        let mut file = MediaFile::new(MediaFileType::Video, "/m", "a.mkv");
        file.video_codec = "HEVC".into();
        file.width = 3840;
        file.height = 1608;
        file.aspect_ratio = 2.39;
        file.bit_depth = 10;
        file.frame_rate = 23.976;
        file.video_bitrate_kbps = 18_000;
        file.size_bytes = 100;
        file.hdr_formats = vec!["Dolby Vision".into(), "HDR10".into()];
        file.audio_streams = vec![
            AudioStream {
                codec: "TrueHD Atmos".into(),
                language: "en".into(),
                channels: 8,
                ..AudioStream::default()
            },
            AudioStream {
                codec: "AC3".into(),
                language: "de".into(),
                channels: 6,
                ..AudioStream::default()
            },
        ];
        file.subtitles = ["fr", "en", "fr"]
            .iter()
            .map(|l| SubtitleStream {
                language: (*l).into(),
                ..SubtitleStream::default()
            })
            .collect();
        file
    }

    #[test]
    fn reads_the_main_video_file() {
        let mut second = MediaFile::new(MediaFileType::Video, "/m", "b.mkv");
        second.size_bytes = 50;
        let fields = media_fields(&[
            MediaFile::new(MediaFileType::Poster, "/m", "p.jpg"),
            video(),
            second,
        ]);
        assert_eq!(field(&fields, "mediaInfoVideoCodec"), "h265");
        assert_eq!(field(&fields, "mediaInfoVideoFormat"), "2160p");
        assert_eq!(field(&fields, "mediaInfoVideoResolution"), "3840x1608");
        assert_eq!(field(&fields, "mediaInfoAspectRatioAsString"), "239");
        assert_eq!(field(&fields, "mediaInfoAudioCodec"), "TrueHD Atmos");
        assert_eq!(field(&fields, "mediaInfoAudioChannels"), "8ch");
        assert_eq!(field(&fields, "mediaInfoAudioChannelsDot"), "7.1");
        assert_eq!(field(&fields, "mediaInfoAudioLanguage"), "en");
        assert_eq!(field(&fields, "videoHDR"), "HDR");
        assert_eq!(field(&fields, "videoHDRFormat"), "Dolby Vision, HDR10");
        assert_eq!(field(&fields, "videoFilesize"), "150");
        assert_eq!(field(&fields, "mediaInfoVideoBitDepth"), "10");
        // Subtitle languages are unique and sorted; audio languages keep stream order.
        let subs = fields
            .iter()
            .find(|(n, _)| *n == "mediaInfoSubtitleLanguageList")
            .map(|(_, v)| v.items().len());
        assert_eq!(subs, Some(2));
    }

    #[test]
    fn no_video_gives_empty_values() {
        let fields = media_fields(&[]);
        for name in [
            "mediaInfoVideoCodec",
            "mediaInfoVideoFormat",
            "mediaInfoAudioCodec",
            "mediaInfoAudioChannelsDot",
            "videoHDR",
            "videoHDRFormat",
            "video3DFormat",
            "CRC32",
            "mediaInfoVideoBitDepth",
            "videoFilesize",
        ] {
            assert_eq!(field(&fields, name), "", "{name}");
        }
        assert!(matches!(
            fields.iter().find(|(n, _)| *n == "mediaInfoFrameRate"),
            Some((_, Value::Null))
        ));
    }

    #[test]
    fn three_d_names() {
        assert_eq!(three_d_name(Video3dFormat::SideBySide, false), "3D HSBS");
        assert_eq!(three_d_name(Video3dFormat::SideBySide, true), "3D SBS");
        assert_eq!(three_d_name(Video3dFormat::None, true), "");
        assert_eq!(three_d_name(Video3dFormat::Mvc, false), "3D MVC");
    }
}

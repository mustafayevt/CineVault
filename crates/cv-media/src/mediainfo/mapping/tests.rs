use cv_domain::MediaFileType;

use super::*;

fn stream(fields: &[(&str, &str)]) -> StreamInfo {
    let mut stream = StreamInfo::new();
    for (name, value) in fields {
        stream.push(*name, *value);
    }
    stream
}

fn hdr(fields: &[(&str, &str)]) -> Vec<String> {
    hdr_formats(&stream(fields))
}

fn flag_set(list: &[StreamFlag]) -> BTreeSet<StreamFlag> {
    list.iter().copied().collect()
}

#[test]
fn languages_are_codes_with_an_upper_case_region() {
    let code = |fields: &[(&str, &str)]| language(&stream(fields));
    assert_eq!(code(&[("Language", "en")]), "en");
    assert_eq!(code(&[("Language", "pt-BR")]), "pt-BR");
    assert_eq!(code(&[("Language", "PT_br")]), "pt-BR");
    assert_eq!(code(&[("Language", "zh-hans")]), "zh-Hans");
    assert_eq!(code(&[("Language", "und")]), "");
    assert_eq!(code(&[("Language/String3", "tur")]), "tur");
    assert_eq!(
        code(&[("Language", "und"), ("Language/String3", "fra")]),
        "fra"
    );
    // A display name is not a code.
    assert_eq!(code(&[("Language/String", "English")]), "");
    assert_eq!(code(&[]), "");
}

#[test]
fn channels_take_the_larger_count_and_ignore_objects() {
    let channels = |fields: &[(&str, &str)]| audio_channels(&stream(fields));
    assert_eq!(channels(&[("Channel(s)", "6")]), 6);
    assert_eq!(
        channels(&[("Channel(s)", "6"), ("Channel(s)_Original", "8")]),
        8
    );
    assert_eq!(
        channels(&[("Channel(s)", "8"), ("Channel(s)_Original", "16 objects")]),
        8
    );
    assert_eq!(channels(&[("Channel(s)", "8 / 6")]), 8);
    assert_eq!(channels(&[("Channel(s)", "6 channels")]), 6);
    assert_eq!(channels(&[]), 0);
}

#[test]
fn audio_codecs_tell_the_variants_apart() {
    let codec = |fields: &[(&str, &str)]| audio_codec(&stream(fields));
    assert_eq!(codec(&[("Format", "AC-3")]), "AC3");
    assert_eq!(
        codec(&[
            ("Format", "AC-3"),
            ("Format_Commercial_IfAny", "Dolby Digital EX")
        ]),
        "AC3 EX"
    );
    assert_eq!(codec(&[("Format", "E-AC-3")]), "E-AC-3");
    assert_eq!(
        codec(&[("Format", "E-AC-3"), ("Format_AdditionalFeatures", "JOC")]),
        "E-AC-3 Atmos"
    );
    assert_eq!(
        codec(&[
            ("Format", "E-AC-3"),
            (
                "Format_Commercial_IfAny",
                "Dolby Digital Plus with Dolby Atmos"
            )
        ]),
        "E-AC-3 Atmos"
    );
    assert_eq!(codec(&[("Format", "MLP FBA")]), "TrueHD");
    assert_eq!(codec(&[("Format", "MLP FBA 16-ch")]), "TrueHD Atmos");
    assert_eq!(
        codec(&[
            ("Format", "MLP FBA"),
            ("Format_Commercial_IfAny", "Dolby TrueHD + Dolby Atmos")
        ]),
        "TrueHD Atmos"
    );
    assert_eq!(
        codec(&[("Format", "DTS"), ("Format_Commercial_IfAny", "DTS-X")]),
        "DTS-X"
    );
    assert_eq!(
        codec(&[("Format", "DTS"), ("Format_Profile", "MA / Core")]),
        "DTS-HD MA",
        "the profile alone is enough"
    );
    assert_eq!(
        codec(&[("Format", "DTS"), ("Format_Profile", "Express")]),
        "DTS Express"
    );
    assert_eq!(
        codec(&[("Format", "MPEG Audio"), ("Format_Profile", "Layer 2")]),
        "MP2"
    );
    assert_eq!(
        codec(&[("Format", "MPEG Audio"), ("Format_Profile", "Layer 3")]),
        "MP3"
    );
    assert_eq!(codec(&[("Format", "FLAC")]), "FLAC");
    assert_eq!(codec(&[("Format", "Opus")]), "Opus");
    assert_eq!(codec(&[("CodecID", "A_VORBIS")]), "A_VORBIS");
}

#[test]
fn bit_rates_are_summed_only_for_mixed_variable_and_constant_parts() {
    let audio = |mode: &str| {
        map_audio_stream(&stream(&[
            ("Format", "DTS"),
            ("BitRate_Mode", mode),
            ("BitRate", "3000000 / 1509000"),
        ]))
    };
    assert_eq!(audio("VBR / CBR").bitrate_kbps, 4509);
    assert_eq!(audio("CBR").bitrate_kbps, 3000);
    // Video never sums.
    let mut file = MediaFile::new(MediaFileType::Video, "d", "a.mkv");
    let mut snapshot = MediaSnapshot::new();
    snapshot.push(
        StreamKind::Video,
        stream(&[("BitRate_Mode", "VBR / CBR"), ("BitRate", "8000000 / 1000")]),
    );
    apply_snapshot(&mut file, &snapshot);
    assert_eq!(file.video_bitrate_kbps, 8000);
}

#[test]
fn audio_flags_come_from_the_flags_service_kind_and_title() {
    let map = |fields: &[(&str, &str)]| map_audio_stream(&stream(fields)).flags;
    assert_eq!(
        map(&[("Default", "Yes"), ("Forced", "No")]),
        flag_set(&[StreamFlag::Default])
    );
    assert_eq!(
        map(&[("ServiceKind", "VI")]),
        flag_set(&[StreamFlag::VisualImpaired])
    );
    assert_eq!(
        map(&[("ServiceKind/String", "Hearing Impaired")]),
        flag_set(&[StreamFlag::HearingImpaired])
    );
    assert!(map(&[("ServiceKind", "CM")]).is_empty());
    assert_eq!(
        map(&[("Title", "English (Dubbed) - Original Mix")]),
        flag_set(&[StreamFlag::Dub, StreamFlag::Original])
    );
    assert_eq!(
        map(&[("Title", "Audio Description")]),
        flag_set(&[StreamFlag::VisualImpaired])
    );
}

#[test]
fn the_container_flags_and_the_title_both_mark_original_and_commentary() {
    let map = |fields: &[(&str, &str)]| map_audio_stream(&stream(fields)).flags;
    // Matroska FlagOriginal and FlagCommentary arrive as ServiceKind, in either spelling.
    assert_eq!(
        map(&[("ServiceKind", "O")]),
        flag_set(&[StreamFlag::Original])
    );
    assert_eq!(
        map(&[("ServiceKind/String", "Original")]),
        flag_set(&[StreamFlag::Original])
    );
    assert_eq!(
        map(&[("ServiceKind", "C")]),
        flag_set(&[StreamFlag::Comment])
    );
    assert_eq!(
        map(&[("ServiceKind/String", "Commentary")]),
        flag_set(&[StreamFlag::Comment])
    );
    // A title that names a commentary marks it without any container flag.
    for title in [
        "Commentary",
        "Director's Commentary",
        "Audio commentary by the cast",
        "Cast Commentaries",
    ] {
        assert_eq!(
            map(&[("Title", title)]),
            flag_set(&[StreamFlag::Comment]),
            "{title}"
        );
    }
    // Both together are one flag, and an ordinary title marks nothing.
    assert_eq!(
        map(&[("ServiceKind", "C"), ("Title", "Commentary")]),
        flag_set(&[StreamFlag::Comment])
    );
    assert!(map(&[("Title", "English 5.1")]).is_empty());
    assert!(map(&[("Title", "Comments")]).is_empty());
}

#[test]
fn subtitle_sdh_comes_from_the_title_on_word_boundaries() {
    let flags = |title: &str| map_subtitle_stream(&stream(&[("Title", title)])).flags;
    for title in [
        "English SDH",
        "English [CC]",
        "Hearing-impaired",
        "hearing impaired",
    ] {
        assert!(
            flags(title).contains(&StreamFlag::HearingImpaired),
            "{title}"
        );
    }
    for title in ["Accent", "Michael cut", "English"] {
        assert!(flags(title).is_empty(), "{title}");
    }
}

#[test]
fn subtitle_codecs_are_named_for_people() {
    let codec = |fields: &[(&str, &str)]| map_subtitle_stream(&stream(fields)).codec;
    assert_eq!(
        codec(&[("Format", "UTF-8"), ("CodecID", "S_TEXT/UTF8")]),
        "SubRip"
    );
    assert_eq!(codec(&[("Format", "PGS")]), "PGS");
    assert_eq!(codec(&[("Format", "VobSub")]), "VobSub");
    assert_eq!(codec(&[("Format", "Timed Text")]), "Timed Text");
    assert_eq!(codec(&[("CodecID", "S_TEXT/WEBVTT")]), "S_TEXT/WEBVTT");
}

#[test]
fn video_codecs() {
    let codec = |fields: &[(&str, &str)]| video_codec(&stream(fields));
    assert_eq!(codec(&[("Format", "AVC")]), "AVC");
    assert_eq!(codec(&[("Format", "HEVC")]), "HEVC");
    assert_eq!(codec(&[("Format", "AV1")]), "AV1");
    assert_eq!(
        codec(&[("Format", "MPEG Video"), ("Format_Version", "Version 1")]),
        "MPEG-1"
    );
    assert_eq!(
        codec(&[("Format", "MPEG Video"), ("Format_Version", "Version 2")]),
        "MPEG-2"
    );
    assert_eq!(
        codec(&[("Format", "MPEG-4 Visual"), ("CodecID", "FMP4")]),
        "MPEG-4 Visual"
    );
    assert_eq!(
        codec(&[
            ("Format", "MPEG-4 Visual"),
            ("CodecID", "DX50"),
            ("CodecID/Hint", "DivX 5")
        ]),
        "DivX 5"
    );
    assert_eq!(
        codec(&[("Format", "MPEG-4 Visual"), ("CodecID", "XVID")]),
        "XviD"
    );
    assert_eq!(
        codec(&[("Format", "MPEG-4 Visual"), ("CodecID", "xvid")]),
        "XviD"
    );
    assert_eq!(codec(&[("Format", "VC-1"), ("CodecID", "WVC1")]), "VC-1");
    assert_eq!(codec(&[("Format", "WMV"), ("CodecID", "WMV3")]), "VC-1");
}

#[test]
fn hdr_combinations() {
    // Dolby Vision with an HDR10 base layer, in the new and the old MediaInfo layouts.
    assert_eq!(
        hdr(&[
            ("HDR_Format", "Dolby Vision / SMPTE ST 2086"),
            ("HDR_Format_Compatibility", "HDR10 / HDR10"),
        ]),
        ["Dolby Vision", "HDR10"]
    );
    assert_eq!(
        hdr(&[
            ("HDR_Format", "Dolby Vision"),
            ("HDR_Format_Compatibility", "HDR10")
        ]),
        ["Dolby Vision", "HDR10"]
    );
    // Dolby Vision alone (profile 5), and with an SDR or HLG base.
    assert_eq!(
        hdr(&[
            ("HDR_Format", "Dolby Vision"),
            ("HDR_Format_Profile", "dvhe.05")
        ]),
        ["Dolby Vision"]
    );
    assert_eq!(
        hdr(&[
            ("HDR_Format", "Dolby Vision"),
            ("HDR_Format_Compatibility", "SDR")
        ]),
        ["Dolby Vision"]
    );
    assert_eq!(
        hdr(&[
            ("HDR_Format", "Dolby Vision"),
            ("HDR_Format_Compatibility", "HLG"),
            ("transfer_characteristics", "HLG"),
        ]),
        ["Dolby Vision", "HLG"]
    );
    // HDR10+ carries an HDR10 base and replaces it in the list.
    assert_eq!(
        hdr(&[
            ("HDR_Format", "SMPTE ST 2086 / SMPTE ST 2094 App 4"),
            ("HDR_Format_Compatibility", "HDR10 / HDR10+ Profile B"),
        ]),
        ["HDR10+"]
    );
    assert_eq!(
        hdr(&[
            ("HDR_Format", "SMPTE ST 2094 App 4"),
            ("HDR_Format_Compatibility", "HDR10+ Profile B"),
        ]),
        ["HDR10+"]
    );
    assert_eq!(
        hdr(&[
            ("HDR_Format", "SMPTE ST 2086"),
            ("HDR_Format_Compatibility", "HDR10")
        ]),
        ["HDR10"]
    );
    assert_eq!(hdr(&[("HDR_Format_Commercial", "HDR10+")]), ["HDR10+"]);
}

#[test]
fn hdr_falls_back_to_the_colour_description() {
    assert_eq!(
        hdr(&[
            ("transfer_characteristics", "HLG"),
            ("colour_primaries", "BT.2020")
        ]),
        ["HLG"]
    );
    assert_eq!(
        hdr(&[("transfer_characteristics", "ARIB STD-B67 (HLG)")]),
        ["HLG"]
    );
    assert_eq!(
        hdr(&[
            ("transfer_characteristics", "PQ"),
            ("colour_primaries", "BT.2020")
        ]),
        ["HDR"]
    );
    assert_eq!(hdr(&[("colour_primaries", "BT.2100")]), ["HDR"]);
    // Wide gamut with an SDR transfer is not HDR.
    assert!(
        hdr(&[
            ("transfer_characteristics", "BT.709"),
            ("colour_primaries", "BT.2020")
        ])
        .is_empty()
    );
    assert!(hdr(&[("colour_primaries", "BT.709")]).is_empty());
    assert!(hdr(&[]).is_empty());
    // The old combined string works when the split fields are missing.
    assert_eq!(
        hdr(&[(
            "HDR_Format/String",
            "Dolby Vision, Version 1.0, Profile 8.1 (dvhe.08), BL+RPU / SMPTE ST 2086, HDR10 compatible"
        )]),
        ["Dolby Vision", "HDR10"]
    );
}

#[test]
fn dolby_vision_profile_follows_the_layer_order() {
    let profile = |fields: &[(&str, &str)]| dolby_vision_profile(&stream(fields));
    assert_eq!(
        profile(&[
            ("HDR_Format", "Dolby Vision"),
            ("HDR_Format_Profile", "dvhe.05")
        ]),
        Some(5)
    );
    assert_eq!(
        profile(&[
            ("HDR_Format", "SMPTE ST 2086 / Dolby Vision"),
            ("HDR_Format_Profile", " / dvhe.08")
        ]),
        Some(8)
    );
    assert_eq!(profile(&[("HDR_Format", "SMPTE ST 2086")]), None);
    assert_eq!(profile(&[]), None);
}

#[test]
fn three_d_names() {
    let name = video_3d_from_name;
    assert_eq!(
        name("Movie (2012) 3D HSBS 1080p.mkv"),
        Video3dFormat::HalfSideBySide
    );
    assert_eq!(name("Movie.3D.Half-SBS.mkv"), Video3dFormat::HalfSideBySide);
    assert_eq!(name("Movie 3D SBS.mkv"), Video3dFormat::SideBySide);
    assert_eq!(name("Movie 3D TAB.mkv"), Video3dFormat::TopAndBottom);
    assert_eq!(name("Movie 3D OU.mkv"), Video3dFormat::TopAndBottom);
    assert_eq!(name("Movie 3D HOU.mkv"), Video3dFormat::HalfTopAndBottom);
    assert_eq!(
        name("Movie.3D.Half-OU.mkv"),
        Video3dFormat::HalfTopAndBottom
    );
    assert_eq!(name("Movie 3D MVC.mkv"), Video3dFormat::Mvc);
    assert_eq!(name("Jackass 3D.mkv"), Video3dFormat::ThreeD);
    // The short layout names mean nothing without 3D.
    assert_eq!(name("The Tab (2020).mkv"), Video3dFormat::None);
    assert_eq!(name("Movie SBS.mkv"), Video3dFormat::None);
    assert_eq!(name("Movie.mkv"), Video3dFormat::None);
}

#[test]
fn three_d_detection_order() {
    // Multi-view fields win and halve the right side.
    let sbs = stream(&[
        ("MultiView_Count", "2"),
        ("MultiView_Layout", "Side by Side (left eye first)"),
    ]);
    assert_eq!(
        detect_3d(&sbs, 3840, 1080, "x 3D HOU.mkv"),
        (Video3dFormat::SideBySide, Halve::Width)
    );
    let tab = stream(&[("MultiView_Count", "2"), ("MultiView_Layout", "Top-Bottom")]);
    assert_eq!(
        detect_3d(&tab, 1920, 2160, "x.mkv"),
        (Video3dFormat::TopAndBottom, Halve::Height)
    );
    let mvc = stream(&[("MultiView_Count", "2")]);
    assert_eq!(
        detect_3d(&mvc, 1920, 1080, "x.mkv"),
        (Video3dFormat::Mvc, Halve::Neither)
    );
    // A multi-view count of 1 is an ordinary file.
    let single = stream(&[("MultiView_Count", "1")]);
    assert_eq!(
        detect_3d(&single, 1920, 1080, "x.mkv").0,
        Video3dFormat::None
    );
    // Without them: the filename, then the frame size, then a bare 3D.
    let plain = stream(&[]);
    assert_eq!(
        detect_3d(&plain, 1920, 1080, "x 3D HSBS.mkv").0,
        Video3dFormat::HalfSideBySide
    );
    assert_eq!(
        detect_3d(&plain, 3840, 1080, "x.mkv"),
        (Video3dFormat::SideBySide, Halve::Neither)
    );
    assert_eq!(
        detect_3d(&plain, 3840, 800, "x.mkv").0,
        Video3dFormat::SideBySide
    );
    assert_eq!(
        detect_3d(&plain, 1920, 2160, "x.mkv").0,
        Video3dFormat::TopAndBottom
    );
    assert_eq!(
        detect_3d(&plain, 1920, 1080, "x 3D.mkv").0,
        Video3dFormat::ThreeD
    );
    // A letterboxed UHD film is not 3D.
    assert_eq!(
        detect_3d(&plain, 3840, 1608, "x.mkv").0,
        Video3dFormat::None
    );
    assert_eq!(
        detect_3d(&plain, 3840, 2160, "x.mkv").0,
        Video3dFormat::None
    );
}

fn file_from(general: &[(&str, &str)], video: &[(&str, &str)], name: &str) -> MediaFile {
    let mut snapshot = MediaSnapshot::new();
    snapshot.push(StreamKind::General, stream(general));
    snapshot.push(StreamKind::Video, stream(video));
    let mut file = MediaFile::new(MediaFileType::Video, "d", name);
    apply_snapshot(&mut file, &snapshot);
    file
}

#[test]
fn durations_come_from_the_clock_text_then_milliseconds_or_seconds() {
    let secs = |general: &[(&str, &str)]| file_from(general, &[], "a.mkv").duration_secs;
    assert_eq!(
        secs(&[("Duration/String3", "01:30:00.400"), ("Duration", "1")]),
        5400
    );
    assert_eq!(secs(&[("Duration", "5400000")]), 5400);
    // The XML export writes seconds with decimals.
    assert_eq!(secs(&[("Duration", "5400.250000")]), 5400);
    assert_eq!(secs(&[("Duration", "90")]), 90);
    assert_eq!(secs(&[]), 0);
    // With no general duration the video stream's is used.
    let from_video = file_from(
        &[("Format", "Matroska")],
        &[("Duration", "61000.000000")],
        "a.mkv",
    );
    assert_eq!(from_video.duration_secs, 61);
}

#[test]
fn container_names() {
    let container =
        |fields: &[(&str, &str)], name: &str| file_from(fields, &[], name).container_format;
    let mkv = [
        ("Format", "Matroska"),
        ("Format/Extensions", "mkv mk3d mka mks"),
    ];
    assert_eq!(container(&mkv, "a.mkv"), "mkv");
    assert_eq!(container(&mkv, "a.mka"), "mka");
    assert_eq!(
        container(&mkv, "a.bin"),
        "mkv",
        "an unlisted extension falls back to the first"
    );
    let mp4 = [
        ("Format", "MPEG-4"),
        ("Format/Extensions", "braw mov mp4 m4v m4a"),
    ];
    assert_eq!(container(&mp4, "A.MP4"), "mp4");
    assert_eq!(container(&mp4, "a.mov"), "mov");
    assert_eq!(
        container(&[("Format", "BDAV")], "00800.m2ts"),
        "Blu-Ray Video"
    );
    assert_eq!(
        container(
            &[("Format", "MPEG-PS"), ("Menu_Format_List", "DVD-Video")],
            "VIDEO_TS.IFO"
        ),
        "DVD Video"
    );
    assert_eq!(container(&[("Format", "AVI")], "a.avi"), "AVI");
    assert_eq!(container(&mkv, "no-extension"), "mkv");
}

#[test]
fn aspect_ratio_uses_pixel_aspect_then_display_aspect_then_the_frame() {
    let ratio = |video: &[(&str, &str)]| {
        let mut all = vec![("Width", "720"), ("Height", "576")];
        all.extend_from_slice(video);
        file_from(&[], &all, "a.mkv").aspect_ratio
    };
    assert!((ratio(&[("PixelAspectRatio", "1.422")]) - 1.7775).abs() < 0.001);
    assert!((ratio(&[("DisplayAspectRatio", "1.778")]) - 1.778).abs() < 0.001);
    assert!((ratio(&[]) - 1.25).abs() < 0.001);
}

#[test]
fn scan_type_and_frame_rate_fallbacks() {
    let file = file_from(
        &[],
        &[
            ("Width", "1920"),
            ("Height", "1080"),
            ("ScanType", "MBAFF"),
            ("FrameRate_Original", "29.970"),
        ],
        "a.ts",
    );
    assert_eq!(file.exact_video_format, "1080i");
    assert!((file.frame_rate - 29.97).abs() < 1e-9);
    let no_video = file_from(&[("Format", "Matroska")], &[], "a.mkv");
    assert_eq!(no_video.exact_video_format, "");
    assert!(no_video.frame_rate.abs() < f64::EPSILON);
}

#[test]
fn an_attached_cover_is_not_the_main_video() {
    let mut snapshot = MediaSnapshot::new();
    snapshot.push(
        StreamKind::Video,
        stream(&[("Format", "JPEG"), ("Width", "600"), ("Height", "900")]),
    );
    snapshot.push(
        StreamKind::Video,
        stream(&[("Format", "AVC"), ("Width", "1920"), ("Height", "1080")]),
    );
    let mut file = MediaFile::new(MediaFileType::Video, "d", "a.mp4");
    apply_snapshot(&mut file, &snapshot);
    assert_eq!((file.video_codec.as_str(), file.width), ("AVC", 1920));
}

#[test]
fn reapplying_replaces_the_earlier_streams_and_keeps_the_identity_fields() {
    let mut file = MediaFile::new(MediaFileType::Video, "d", "Movie 3D HSBS.mkv");
    file.size_bytes = 42;
    file.stacking = 2;
    file.aspect_ratio2 = 2.39;
    file.checksums.insert("crc32".into(), "DEADBEEF".into());

    let mut first = MediaSnapshot::new();
    first.push(
        StreamKind::General,
        stream(&[("Format", "Matroska"), ("Title", "Old")]),
    );
    first.push(
        StreamKind::Video,
        stream(&[
            ("Format", "HEVC"),
            ("Width", "1920"),
            ("Height", "1080"),
            ("HDR_Format", "Dolby Vision"),
        ]),
    );
    first.push(
        StreamKind::Audio,
        stream(&[("Format", "AC-3"), ("Channel(s)", "6")]),
    );
    first.push(StreamKind::Text, stream(&[("Format", "PGS")]));
    apply_snapshot(&mut file, &first);
    assert_eq!(file.video_3d, Video3dFormat::HalfSideBySide);
    assert_eq!(file.hdr_formats, ["Dolby Vision"]);
    assert_eq!(file.title, "Old");

    let mut second = MediaSnapshot::new();
    second.push(StreamKind::General, stream(&[("Format", "Matroska")]));
    second.push(
        StreamKind::Video,
        stream(&[("Format", "AVC"), ("Width", "1280"), ("Height", "720")]),
    );
    second.push(
        StreamKind::Audio,
        stream(&[("Format", "AAC"), ("Channel(s)", "2")]),
    );
    apply_snapshot(&mut file, &second);

    assert_eq!(file.video_codec, "AVC");
    assert_eq!((file.width, file.height), (1280, 720));
    assert!(file.hdr_formats.is_empty());
    assert!(file.subtitles.is_empty());
    assert_eq!(file.audio_streams.len(), 1);
    assert_eq!(file.audio_streams[0].codec, "AAC");
    assert_eq!(file.title, "");
    // The name still says 3D, so the flag stays; the identity fields are untouched.
    assert_eq!(file.video_3d, Video3dFormat::HalfSideBySide);
    assert_eq!((file.size_bytes, file.stacking), (42, 2));
    assert!((file.aspect_ratio2 - 2.39).abs() < f32::EPSILON);
    assert_eq!(file.checksums.len(), 1);
}

#[test]
fn a_file_with_streams_but_no_format_name_still_counts_as_read() {
    let mut snapshot = MediaSnapshot::new();
    snapshot.push(StreamKind::General, stream(&[("FileSize", "10")]));
    snapshot.push(StreamKind::Audio, stream(&[("Format", "MP3")]));
    let mut file = MediaFile::new(MediaFileType::Audio, "d", "a.xyz");
    apply_snapshot(&mut file, &snapshot);
    assert!(file.has_media_info());

    let mut file = MediaFile::new(MediaFileType::Video, "d", "a.mkv");
    apply_snapshot(&mut file, &MediaSnapshot::new());
    assert!(!file.has_media_info());
}

fn details(fields: &[(&str, &str)]) -> HdrDetails {
    hdr_details(&stream(fields))
}

#[test]
fn dolby_vision_profile_is_written_with_its_base_layer() {
    let dv = |profile: &str, compat: &str| {
        let d = details(&[
            ("HDR_Format", "Dolby Vision"),
            ("HDR_Format_Profile", profile),
            ("HDR_Format_Compatibility", compat),
        ]);
        (d.dolby_vision_profile, d.dolby_vision_compatibility)
    };
    assert_eq!(dv("dvhe.08", "HDR10"), ("8.1".into(), "HDR10".into()));
    assert_eq!(dv("dvhe.08.06", "HDR10"), ("8.1".into(), "HDR10".into()));
    assert_eq!(dv("dvhe.08", "SDR"), ("8.2".into(), "SDR".into()));
    assert_eq!(dv("dvhe.08", "HLG"), ("8.4".into(), "HLG".into()));
    assert_eq!(
        dv("dvhe.08", "HDR10+ Profile B"),
        ("8.1".into(), "HDR10+".into())
    );
    assert_eq!(dv("dvhe.07", "Blu-ray"), ("7".into(), "Blu-ray".into()));
    assert_eq!(dv("dvhe.05", ""), ("5".into(), String::new()));
    // Layers in the order of HDR_Format.
    let layered = details(&[
        ("HDR_Format", "SMPTE ST 2086 / Dolby Vision"),
        ("HDR_Format_Profile", " / dvhe.08"),
        ("HDR_Format_Compatibility", "HDR10 / HDR10"),
    ]);
    assert_eq!(layered.dolby_vision_profile, "8.1");
    // No Dolby Vision, no profile.
    let plain = details(&[
        ("HDR_Format", "SMPTE ST 2086"),
        ("HDR_Format_Compatibility", "HDR10"),
    ]);
    assert_eq!(plain.dolby_vision_profile, "");
    assert_eq!(plain.dolby_vision_compatibility, "");
}

#[test]
fn the_mastering_display_and_light_levels_are_read() {
    let d = details(&[
        ("HDR_Format", "SMPTE ST 2086"),
        ("MasteringDisplay_ColorPrimaries", "Display P3"),
        (
            "MasteringDisplay_Luminance",
            "min: 0.0050 cd/m2, max: 1000 cd/m2",
        ),
        ("MaxCLL", "1000 cd/m2"),
        ("MaxFALL", "399.5 cd/m2"),
    ]);
    assert_eq!(d.mastering_primaries, "Display P3");
    assert_eq!(d.mastering_min_nits, Some(0.005));
    assert_eq!(d.mastering_max_nits, Some(1000.0));
    assert_eq!(d.max_cll_nits, Some(1000));
    assert_eq!(d.max_fall_nits, Some(400));
    assert!(details(&[("Format", "HEVC")]).is_empty());
}

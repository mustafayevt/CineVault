//! Release names as the sources return them, read by `parse_release`.
//!
//! Every row names what the parser must find; anything a row does not mention must be left
//! unknown, so a pattern that fires where it should not fails here.

use cv_domain::MediaSource;
use cv_torrent::model::{Hdr, Resolution};
use cv_torrent::release::{Audio, Codec, EpisodeSpan, LowQuality, parse_release};

#[derive(Debug)]
struct Want {
    title: &'static str,
    year: Option<u16>,
    res: Resolution,
    src: MediaSource,
    codec: Codec,
    hdr: Hdr,
    audio: Audio,
    span: EpisodeSpan,
    remux: bool,
    low: Option<LowQuality>,
}

impl Default for Want {
    fn default() -> Self {
        Self {
            title: "",
            year: None,
            res: Resolution::Unknown,
            src: MediaSource::Unknown,
            codec: Codec::Unknown,
            hdr: Hdr::None,
            audio: Audio::Unknown,
            span: EpisodeSpan::None,
            remux: false,
            low: None,
        }
    }
}

fn single(season: u16, episode: u16) -> EpisodeSpan {
    EpisodeSpan::Single { season, episode }
}

fn multi(season: u16, first: u16, last: u16) -> EpisodeSpan {
    EpisodeSpan::Multi {
        season,
        first,
        last,
    }
}

fn range(first: u16, last: u16) -> EpisodeSpan {
    EpisodeSpan::SeasonRange { first, last }
}

#[allow(clippy::too_many_lines)]
fn rows() -> Vec<(&'static str, Want)> {
    use Resolution::{R480, R720, R1080, R2160};
    let d = Want::default;
    vec![
        // Movies: resolutions, sources, codecs.
        (
            "Night.of.the.Living.Dead.1968.1080p.BluRay.x264-GRP",
            Want {
                title: "Night of the Living Dead",
                year: Some(1968),
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                ..d()
            },
        ),
        (
            "Dune.Part.Two.2024.2160p.WEB-DL.DDP5.1.Atmos.DV.HDR10+.H.265-FLUX",
            Want {
                title: "Dune Part Two",
                year: Some(2024),
                res: R2160,
                src: MediaSource::WebDl,
                codec: Codec::Hevc,
                hdr: Hdr::DolbyVision,
                audio: Audio::Atmos,
                ..d()
            },
        ),
        (
            "Dune 2021 2160p UHD BluRay REMUX HDR10 HEVC TrueHD 7.1 Atmos-EPSiLON",
            Want {
                title: "Dune",
                year: Some(2021),
                res: R2160,
                src: MediaSource::UhdBluray,
                codec: Codec::Hevc,
                hdr: Hdr::Hdr10,
                audio: Audio::Atmos,
                remux: true,
                ..d()
            },
        ),
        (
            "Dune (1984) 720p BrRip x264 - YIFY",
            Want {
                title: "Dune",
                year: Some(1984),
                res: R720,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                ..d()
            },
        ),
        (
            "The.Matrix.1999.REMASTERED.1080p.BluRay.DTS-HD.MA.5.1.x265-RARBG",
            Want {
                title: "The Matrix",
                year: Some(1999),
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Hevc,
                audio: Audio::DtsHd,
                ..d()
            },
        ),
        (
            "Amelie.2001.1080p.BluRay.DTS.x264-CtrlHD",
            Want {
                title: "Amelie",
                year: Some(2001),
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                audio: Audio::Dts,
                ..d()
            },
        ),
        (
            "Le.Fabuleux.Destin.d'Amelie.Poulain.2001.720p.HDTV.AC3.x264",
            Want {
                title: "Le Fabuleux Destin d'Amelie Poulain",
                year: Some(2001),
                res: R720,
                src: MediaSource::Tv,
                codec: Codec::Avc,
                audio: Audio::Ac3,
                ..d()
            },
        ),
        (
            "Mission.Impossible.Fallout.2018.1080p.WEBRip.x264.AAC5.1-[YTS.MX]",
            Want {
                title: "Mission Impossible Fallout",
                year: Some(2018),
                res: R1080,
                src: MediaSource::WebRip,
                codec: Codec::Avc,
                audio: Audio::Aac,
                ..d()
            },
        ),
        (
            "Fast.&.Furious.2009.720p.BluRay.x264.DD5.1-NoGroup",
            Want {
                title: "Fast & Furious",
                year: Some(2009),
                res: R720,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                audio: Audio::Ac3,
                ..d()
            },
        ),
        (
            "Widow.s.Bay.2019.1080p.WEB-DL.H264-CMRG",
            Want {
                title: "Widow s Bay",
                year: Some(2019),
                res: R1080,
                src: MediaSource::WebDl,
                codec: Codec::Avc,
                ..d()
            },
        ),
        (
            "Amelie (2001) [1080p] [BluRay] [5.1] [YTS.MX]",
            Want {
                title: "Amelie",
                year: Some(2001),
                res: R1080,
                src: MediaSource::Bluray,
                ..d()
            },
        ),
        (
            "Blade Runner 2049 (2017) 2160p UHD BluRay x265 10bit HDR TrueHD 7.1 Atmos",
            Want {
                title: "Blade Runner 2049",
                year: Some(2017),
                res: R2160,
                src: MediaSource::UhdBluray,
                codec: Codec::Hevc,
                hdr: Hdr::Hdr10,
                audio: Audio::Atmos,
                ..d()
            },
        ),
        (
            "Alien.1979.Directors.Cut.1080p.BluRay.AV1.Opus-GRP",
            Want {
                title: "Alien",
                year: Some(1979),
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Av1,
                ..d()
            },
        ),
        (
            "Oppenheimer.2023.IMAX.2160p.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-APEX",
            Want {
                title: "Oppenheimer",
                year: Some(2023),
                res: R2160,
                src: MediaSource::WebDl,
                codec: Codec::Hevc,
                hdr: Hdr::DolbyVision,
                audio: Audio::Atmos,
                ..d()
            },
        ),
        (
            "Planet.Earth.II.2016.2160p.HLG.HEVC.AAC-GRP",
            Want {
                title: "Planet Earth II",
                year: Some(2016),
                res: R2160,
                codec: Codec::Hevc,
                hdr: Hdr::Hlg,
                audio: Audio::Aac,
                ..d()
            },
        ),
        (
            "Gladiator.2000.4K.HDR10Plus.BluRay.x265.DTS-X",
            Want {
                title: "Gladiator",
                year: Some(2000),
                res: R2160,
                src: MediaSource::Bluray,
                codec: Codec::Hevc,
                hdr: Hdr::Hdr10Plus,
                audio: Audio::DtsHd,
                ..d()
            },
        ),
        (
            "Heat.1995.480p.DVDRip.XviD-GRP",
            Want {
                title: "Heat",
                year: Some(1995),
                res: R480,
                src: MediaSource::Dvd,
                codec: Codec::Xvid,
                ..d()
            },
        ),
        (
            "Heat.1995.576p.DVD.DivX.AC3",
            Want {
                title: "Heat",
                year: Some(1995),
                res: R480,
                src: MediaSource::Dvd,
                codec: Codec::Xvid,
                audio: Audio::Ac3,
                ..d()
            },
        ),
        (
            "Ghostbusters.1984.1080p.BluRay.REMUX.AVC.DTS-HD.MA.5.1-GRP",
            Want {
                title: "Ghostbusters",
                year: Some(1984),
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                audio: Audio::DtsHd,
                remux: true,
                ..d()
            },
        ),
        (
            "Parasite.2019.KOREAN.1080p.AMZN.WEB-DL.DDP5.1.H.264-GRP",
            Want {
                title: "Parasite",
                year: Some(2019),
                res: R1080,
                src: MediaSource::WebDl,
                codec: Codec::Avc,
                audio: Audio::DdPlus,
                ..d()
            },
        ),
        (
            "Parasite 2019 E-AC3 720p HDTV",
            Want {
                title: "Parasite",
                year: Some(2019),
                res: R720,
                src: MediaSource::Tv,
                audio: Audio::DdPlus,
                ..d()
            },
        ),
        // Low quality recordings.
        (
            "Some.New.Movie.2025.CAM.x264-GRP",
            Want {
                title: "Some New Movie",
                year: Some(2025),
                src: MediaSource::Cam,
                codec: Codec::Avc,
                low: Some(LowQuality::Cam),
                ..d()
            },
        ),
        (
            "Some.New.Movie.2025.HDCAM.XviD",
            Want {
                title: "Some New Movie",
                year: Some(2025),
                src: MediaSource::Cam,
                codec: Codec::Xvid,
                low: Some(LowQuality::Cam),
                ..d()
            },
        ),
        (
            "Some.New.Movie.2025.HDTS.x264.AAC",
            Want {
                title: "Some New Movie",
                year: Some(2025),
                src: MediaSource::Ts,
                codec: Codec::Avc,
                audio: Audio::Aac,
                low: Some(LowQuality::Telesync),
                ..d()
            },
        ),
        (
            "Some.New.Movie.2025.TELESYNC.720p",
            Want {
                title: "Some New Movie",
                year: Some(2025),
                res: R720,
                src: MediaSource::Ts,
                low: Some(LowQuality::Telesync),
                ..d()
            },
        ),
        (
            "Some.New.Movie.2025.HDTC.x264",
            Want {
                title: "Some New Movie",
                year: Some(2025),
                src: MediaSource::Tc,
                codec: Codec::Avc,
                low: Some(LowQuality::Telecine),
                ..d()
            },
        ),
        (
            "Some.New.Movie.2025.DVDSCR.XviD",
            Want {
                title: "Some New Movie",
                year: Some(2025),
                src: MediaSource::DvdScr,
                codec: Codec::Xvid,
                low: Some(LowQuality::Screener),
                ..d()
            },
        ),
        (
            "Some.New.Movie.2025.SCREENER.x264",
            Want {
                title: "Some New Movie",
                year: Some(2025),
                codec: Codec::Avc,
                low: Some(LowQuality::Screener),
                ..d()
            },
        ),
        // Episodes.
        (
            "Severance.S01E02.Half.Loop.1080p.ATVP.WEB-DL.DDP5.1.H.264-GRP",
            Want {
                title: "Severance",
                res: R1080,
                src: MediaSource::WebDl,
                codec: Codec::Avc,
                audio: Audio::DdPlus,
                span: single(1, 2),
                ..d()
            },
        ),
        (
            "Severance.S02E10.2160p.WEB-DL.DV.HDR10+.Atmos.H265",
            Want {
                title: "Severance",
                res: R2160,
                src: MediaSource::WebDl,
                codec: Codec::Hevc,
                hdr: Hdr::DolbyVision,
                audio: Audio::Atmos,
                span: single(2, 10),
                ..d()
            },
        ),
        (
            "The.Mandalorian.S03E08.720p.HDTV.x264-SYNCOPY",
            Want {
                title: "The Mandalorian",
                res: R720,
                src: MediaSource::Tv,
                codec: Codec::Avc,
                span: single(3, 8),
                ..d()
            },
        ),
        (
            "Marvels.Agents.of.SHIELD.S02E05.1080p.BluRay.x265",
            Want {
                title: "Marvels Agents of SHIELD",
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Hevc,
                span: single(2, 5),
                ..d()
            },
        ),
        (
            "Show Name S01E01E02 720p HDTV x264",
            Want {
                title: "Show Name",
                res: R720,
                src: MediaSource::Tv,
                codec: Codec::Avc,
                span: multi(1, 1, 2),
                ..d()
            },
        ),
        (
            "Show.Name.S01E01-E03.1080p.WEB.H264",
            Want {
                title: "Show Name",
                res: R1080,
                src: MediaSource::WebDl,
                codec: Codec::Avc,
                span: multi(1, 1, 3),
                ..d()
            },
        ),
        (
            "Show.Name.S04E07-08.1080p.WEB.H264",
            Want {
                title: "Show Name",
                res: R1080,
                src: MediaSource::WebDl,
                codec: Codec::Avc,
                span: multi(4, 7, 8),
                ..d()
            },
        ),
        (
            "Show.Name.1x05.DVDRip.XviD",
            Want {
                title: "Show Name",
                src: MediaSource::Dvd,
                codec: Codec::Xvid,
                span: single(1, 5),
                ..d()
            },
        ),
        (
            "Show.Name.2019.S01E03.1080p.WEBRip.x264",
            Want {
                title: "Show Name",
                year: Some(2019),
                res: R1080,
                src: MediaSource::WebRip,
                codec: Codec::Avc,
                span: single(1, 3),
                ..d()
            },
        ),
        (
            "show name s05e12 internal 720p web x264",
            Want {
                title: "show name",
                res: R720,
                src: MediaSource::WebDl,
                codec: Codec::Avc,
                span: single(5, 12),
                ..d()
            },
        ),
        (
            "Show.Name.S01E01.mkv",
            Want {
                title: "Show Name",
                span: single(1, 1),
                ..d()
            },
        ),
        (
            "Show.Name.S100E1000.1080p",
            Want {
                title: "Show Name",
                res: R1080,
                span: single(100, 1000),
                ..d()
            },
        ),
        // Packs.
        (
            "Show.Name.S01.1080p.BluRay.x264-GRP",
            Want {
                title: "Show Name",
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                span: EpisodeSpan::SeasonPack(1),
                ..d()
            },
        ),
        (
            "Show.Name.S02.COMPLETE.2160p.WEB-DL.H265",
            Want {
                title: "Show Name",
                res: R2160,
                src: MediaSource::WebDl,
                codec: Codec::Hevc,
                span: EpisodeSpan::SeasonPack(2),
                ..d()
            },
        ),
        (
            "Show Name Season 3 Complete 720p HDTV",
            Want {
                title: "Show Name",
                res: R720,
                src: MediaSource::Tv,
                span: EpisodeSpan::SeasonPack(3),
                ..d()
            },
        ),
        (
            "Show.Name.S01-S03.1080p.BluRay.x264",
            Want {
                title: "Show Name",
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                span: range(1, 3),
                ..d()
            },
        ),
        (
            "Show.Name.S01-05.COMPLETE.720p.WEB-DL",
            Want {
                title: "Show Name",
                res: R720,
                src: MediaSource::WebDl,
                span: range(1, 5),
                ..d()
            },
        ),
        (
            "Show Name Seasons 1-4 720p",
            Want {
                title: "Show Name",
                res: R720,
                span: range(1, 4),
                ..d()
            },
        ),
        (
            "Show.Name.Complete.Series.1080p.BluRay.x265",
            Want {
                title: "Show Name",
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Hevc,
                span: EpisodeSpan::Complete,
                ..d()
            },
        ),
        // Anime group releases.
        (
            "[SubsPlease] Sousou no Frieren - 05 (1080p) [A1B2C3D4].mkv",
            Want {
                title: "Sousou no Frieren",
                res: R1080,
                span: single(1, 5),
                ..d()
            },
        ),
        (
            "[Erai-raws] Kaiju No. 8 - 03 [1080p][HEVC][Multiple Subtitle]",
            Want {
                title: "Kaiju No 8",
                res: R1080,
                codec: Codec::Hevc,
                span: single(1, 3),
                ..d()
            },
        ),
        // Site prefixes, brackets, extensions.
        (
            "www.Torrenting.com - Heat.1995.1080p.BluRay.x264",
            Want {
                title: "Heat",
                year: Some(1995),
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                ..d()
            },
        ),
        (
            "Heat.1995.1080p.BluRay.x264.mkv",
            Want {
                title: "Heat",
                year: Some(1995),
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                ..d()
            },
        ),
        (
            "Heat.1995",
            Want {
                title: "Heat",
                year: Some(1995),
                ..d()
            },
        ),
        (
            "Heat",
            Want {
                title: "Heat",
                ..d()
            },
        ),
        (
            "  Heat  (1995)  ",
            Want {
                title: "Heat",
                year: Some(1995),
                ..d()
            },
        ),
        (
            "2001.A.Space.Odyssey.1968.1080p.BluRay.x264",
            Want {
                title: "2001 A Space Odyssey",
                year: Some(1968),
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                ..d()
            },
        ),
        (
            "1917.2019.1080p.BluRay.x264-GRP",
            Want {
                title: "1917",
                year: Some(2019),
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                ..d()
            },
        ),
        (
            "Se7en.1995.720p.BluRay.x264",
            Want {
                title: "Se7en",
                year: Some(1995),
                res: R720,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                ..d()
            },
        ),
        // Payload and disc image names.
        (
            "Some.Movie.2020.1080p.WEB-DL.x264.exe",
            Want {
                title: "Some Movie",
                year: Some(2020),
                res: R1080,
                src: MediaSource::WebDl,
                codec: Codec::Avc,
                ..d()
            },
        ),
        (
            "Some.Movie.2020.1080p.BluRay.x264.iso",
            Want {
                title: "Some Movie",
                year: Some(2020),
                res: R1080,
                src: MediaSource::Bluray,
                codec: Codec::Avc,
                ..d()
            },
        ),
        // Words that look like tags but are part of titles.
        (
            "Tiger.King.2020.1080p.WEB",
            Want {
                title: "Tiger King",
                year: Some(2020),
                res: R1080,
                src: MediaSource::WebDl,
                ..d()
            },
        ),
        (
            "Camera.Obscura.2019.1080p.WEB-DL",
            Want {
                title: "Camera Obscura",
                year: Some(2019),
                res: R1080,
                src: MediaSource::WebDl,
                ..d()
            },
        ),
        (
            "Duke.Nukem.1996.720p.BluRay",
            Want {
                title: "Duke Nukem",
                year: Some(1996),
                res: R720,
                src: MediaSource::Bluray,
                ..d()
            },
        ),
        (
            "Dvorak.Lives.2011.1080p",
            Want {
                title: "Dvorak Lives",
                year: Some(2011),
                res: R1080,
                ..d()
            },
        ),
        (
            "Hevcular.2010.720p",
            Want {
                title: "Hevcular",
                year: Some(2010),
                res: R720,
                ..d()
            },
        ),
    ]
}

#[test]
fn the_table_has_at_least_sixty_names() {
    assert!(rows().len() >= 60, "{} rows", rows().len());
}

#[test]
fn every_release_name_is_read_as_expected() {
    let mut failures = Vec::new();
    for (name, want) in rows() {
        let got = parse_release(name, &[]);
        let mut wrong = Vec::new();
        if got.title != want.title {
            wrong.push(format!("title {:?} != {:?}", got.title, want.title));
        }
        if got.year != want.year {
            wrong.push(format!("year {:?} != {:?}", got.year, want.year));
        }
        if got.resolution != want.res {
            wrong.push(format!("resolution {:?} != {:?}", got.resolution, want.res));
        }
        if got.source != want.src {
            wrong.push(format!("source {:?} != {:?}", got.source, want.src));
        }
        if got.codec != want.codec {
            wrong.push(format!("codec {:?} != {:?}", got.codec, want.codec));
        }
        if got.hdr != want.hdr {
            wrong.push(format!("hdr {:?} != {:?}", got.hdr, want.hdr));
        }
        if got.audio != want.audio {
            wrong.push(format!("audio {:?} != {:?}", got.audio, want.audio));
        }
        if got.span != want.span {
            wrong.push(format!("span {:?} != {:?}", got.span, want.span));
        }
        if got.remux != want.remux {
            wrong.push(format!("remux {} != {}", got.remux, want.remux));
        }
        if got.low_quality != want.low {
            wrong.push(format!(
                "low quality {:?} != {:?}",
                got.low_quality, want.low
            ));
        }
        if !wrong.is_empty() {
            failures.push(format!("{name}\n    {}", wrong.join("\n    ")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} names misread:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn an_exe_payload_is_flagged_and_an_iso_is_not() {
    assert!(parse_release("Some.Movie.2020.1080p.WEB-DL.x264.exe", &[]).payload_warning);
    assert!(!parse_release("Some.Movie.2020.1080p.BluRay.x264.iso", &[]).payload_warning);
    assert!(!parse_release("Some.Movie.2020.1080p.BluRay.x264", &[]).payload_warning);
}

#[test]
fn a_file_list_with_an_executable_or_archive_is_flagged() {
    let files = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
    let clean = parse_release(
        "Movie.2020.1080p",
        &files(&["Movie.2020.1080p.mkv", "Movie.nfo", "Sample/sample.mkv"]),
    );
    assert!(!clean.payload_warning);
    for bad in [
        "Setup.exe",
        "Movie.2020.rar",
        "extras.zip",
        "run.bat",
        "play.lnk",
        "x.7z",
        "player.SCR",
        "install.msi",
    ] {
        let r = parse_release("Movie.2020.1080p", &files(&["Movie.2020.1080p.mkv", bad]));
        assert!(r.payload_warning, "{bad}");
    }
    let iso = parse_release("Movie.2020.1080p", &files(&["Movie.2020.iso"]));
    assert!(!iso.payload_warning);
}

#[test]
fn channels_are_read_next_to_the_audio_format() {
    assert_eq!(
        parse_release("M.2020.1080p.DDP5.1.H264", &[])
            .channels
            .as_deref(),
        Some("5.1")
    );
    assert_eq!(
        parse_release("M.2020.1080p.TrueHD.7.1.Atmos", &[])
            .channels
            .as_deref(),
        Some("7.1")
    );
    assert_eq!(
        parse_release("M.2020.1080p.AAC2.0", &[])
            .channels
            .as_deref(),
        Some("2.0")
    );
    assert_eq!(parse_release("M.2020.1080p.x264", &[]).channels, None);
}

#[test]
fn the_parser_never_panics_on_odd_text() {
    let long = "A".repeat(5000);
    for name in [
        "\u{0}",
        "S",
        "S01E",
        "E01",
        "S99999E99999",
        "----",
        "[]",
        "[[[",
        "\u{1F3AC}.2020",
        "Show.S01E02-E03-E04-E05-E06",
        "Show.S01E01-E999",
        "Show.S3000",
        long.as_str(),
    ] {
        let _ = parse_release(name, &[]);
    }
}

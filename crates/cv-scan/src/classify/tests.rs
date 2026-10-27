//! Classification rules. The byte-exact fixtures live in `tests/compat` and run through
//! `cv-compat`; these tests pin the rule order and the edge cases next to the code.

use super::*;
use MediaFileType as T;

fn t(path: &str) -> MediaFileType {
    media_file_type(path)
}

#[test]
fn extras_folders_win_over_every_extension() {
    assert_eq!(t("Heat (1995)/extras/poster.jpg"), T::Extra);
    assert_eq!(t("Heat (1995)/Behind The Scenes/clip.mp4"), T::Extra);
    assert_eq!(t("Heat (1995)/extras/sub/deep.mkv"), T::Extra);
    assert_eq!(t("Heat (1995)/trickplay/Heat-320-10.bif"), T::Extra);
    assert_eq!(t("Show/Season 1/extras/Making of.mkv"), T::Extra);
}

#[test]
fn the_first_segment_and_the_file_are_not_extras_folders() {
    assert_eq!(t("extras/Loose.mkv"), T::Video);
    assert_eq!(t("extras/Loose.nfo"), T::Nfo);
    assert_eq!(t("Heat (1995)/extras"), T::Unknown);
}

#[test]
fn both_separators_work() {
    assert_eq!(t("Heat (1995)\\extras\\Making of.mkv"), T::Extra);
    assert_eq!(t("Heat (1995)\\poster.jpg"), T::Poster);
}

#[test]
fn extras_by_name() {
    assert_eq!(t("M/M-featurette.mkv"), T::Extra);
    assert_eq!(t("M/M_behindthescenes.mkv"), T::Extra);
    assert_eq!(t("M/M-deleted2.mkv"), T::Extra);
    assert_eq!(t("M/M [Interview].mkv"), T::Extra);
    assert_eq!(t("M/M (Extras).mkv"), T::Extra);
    assert_eq!(t("M/M.EXTRAS.Interviews.mkv"), T::Extra);
    // A word inside a title is not a marker.
    assert_eq!(t("M/Other People.mkv"), T::Video);
    assert_eq!(t("M/The Short Film.mkv"), T::Video);
}

#[test]
fn trailers() {
    assert_eq!(t("M/M-trailer.mp4"), T::Trailer);
    assert_eq!(t("M/M.trailer2.mp4"), T::Trailer);
    assert_eq!(t("M/M[trailer].mkv"), T::Trailer);
    assert_eq!(t("M/m.tRaiLeR.mkv"), T::Trailer);
    assert_eq!(t("M/trailer.mp4"), T::Trailer);
    assert_eq!(t("M/movie-trailer.mkv"), T::Trailer);
    assert_eq!(t("M/trailers/teaser.mp4"), T::Trailer);
    assert_eq!(t("M/trailer/teaser.mkv"), T::Trailer);
    // The marker needs a delimiter right before it.
    assert_eq!(t("M/M - Trailer.mkv"), T::Video);
    assert_eq!(t("M/The Trailer Park Heist.mkv"), T::Video);
}

#[test]
fn samples() {
    assert_eq!(t("M/M-sample.mkv"), T::Sample);
    assert_eq!(t("M/sample.mkv"), T::Sample);
    assert_eq!(t("M/Sample/anything.mkv"), T::Sample);
    assert_eq!(t("M/M.Sample.mkv"), T::Sample);
    assert_eq!(t("M/Samples of Love.mkv"), T::Video);
}

#[test]
fn metadata_and_other_files() {
    assert_eq!(t("M/M.NFO"), T::Nfo);
    assert_eq!(t("M/M.vsmeta"), T::Vsmeta);
    assert_eq!(t("M/M-mediainfo.xml"), T::Mediainfo);
    assert_eq!(t("M/M.mediainfo.xml"), T::Mediainfo);
    assert_eq!(t("M/M.xml"), T::Unknown);
    assert_eq!(t("M/readme.txt"), T::Text);
    assert_eq!(t("M/M.bif"), T::Bif);
    assert_eq!(t("M/M.rar"), T::Unknown);
    assert_eq!(t("M/.hidden"), T::Unknown);
    assert_eq!(t("M/noextension"), T::Unknown);
    assert_eq!(t(""), T::Unknown);
}

#[test]
fn audio_subtitles_and_themes() {
    assert_eq!(t("M/M.flac"), T::Audio);
    assert_eq!(t("M/M.de.forced.srt"), T::Subtitle);
    assert_eq!(t("M/M.idx"), T::Unknown);
    assert_eq!(t("M/theme.mp3"), T::Theme);
    assert_eq!(t("M/M-theme.mp3"), T::Theme);
    assert_eq!(t("M/M.theme2.mp3"), T::Theme);
    assert_eq!(t("M/M_soundtrack1.flac"), T::Theme);
    assert_eq!(t("M/theme.wav"), T::Theme);
    assert_eq!(t("M/Themes of Love.mp3"), T::Audio);
}

#[test]
fn video_extensions_are_matched_without_regard_to_case() {
    for ext in ["mkv", "MP4", "m2ts", "iso", "strm", "wmv", "avi"] {
        assert_eq!(t(&format!("M/M.{ext}")), T::Video, "{ext}");
    }
}

#[test]
fn artwork_names() {
    let cases = [
        ("poster.jpg", T::Poster),
        ("Poster.JPG", T::Poster),
        ("folder.jpg", T::Poster),
        ("movie.jpg", T::Poster),
        ("cover.png", T::Poster),
        ("M_poster.webp", T::Poster),
        ("poster.tbn", T::Poster),
        ("fanart.jpg", T::Fanart),
        ("backdrop.jpg", T::Fanart),
        ("M-background.png", T::Fanart),
        ("fanart1.jpg", T::Extrafanart),
        ("M-fanart3.jpg", T::Extrafanart),
        ("banner.jpg", T::Banner),
        ("thumb.jpg", T::Thumb),
        ("landscape.jpg", T::Thumb),
        ("thumb12.jpg", T::Thumb),
        ("clearart.png", T::Clearart),
        ("logo.png", T::Clearlogo),
        ("M-clearlogo.png", T::Clearlogo),
        ("disc.png", T::Disc),
        ("M-discart.png", T::Disc),
        ("characterart2.png", T::Characterart),
        ("keyart.jpg", T::Keyart),
        ("square.jpg", T::Squareart),
        ("backgroundsquare.jpg", T::Squareart),
        ("random.jpg", T::Graphic),
        ("M-screenshot.jpg", T::Graphic),
    ];
    for (name, want) in cases {
        assert_eq!(t(&format!("M/{name}")), want, "{name}");
    }
}

#[test]
fn artwork_rule_order_decides_overlapping_names() {
    // "backgroundsquare" ends with "square" but not with "background" plus a dot.
    assert_eq!(t("M/backgroundsquare.png"), T::Squareart);
    // numbered fanart is checked before plain fanart
    assert_eq!(t("M/fanart12.jpg"), T::Extrafanart);
    // season rules come before the poster rule
    assert_eq!(t("Show/season01-poster.jpg"), T::SeasonPoster);
    assert_eq!(t("Show/season01-fanart.jpg"), T::SeasonFanart);
    assert_eq!(t("Show/season-specials-banner.jpg"), T::SeasonBanner);
    assert_eq!(t("Show/season-all-thumb.jpg"), T::SeasonThumb);
    assert_eq!(t("Show/season-specials.jpg"), T::SeasonPoster);
    // a thumb for an episode is a plain thumb
    assert_eq!(t("Show/Season 1/Show - S01E01-thumb.jpg"), T::Thumb);
}

#[test]
fn movie_set_artwork_is_graphic() {
    for kind in [
        "poster", "fanart", "backdrop", "banner", "disc", "logo", "thumb",
    ] {
        assert_eq!(t(&format!("Collections/movieset-{kind}.jpg")), T::Graphic);
    }
}

#[test]
fn artwork_folders() {
    assert_eq!(t("M/extrafanart/fanart.jpg"), T::Extrafanart);
    assert_eq!(t("M/extrafanart/fanart2.jpg"), T::Extrafanart);
    assert_eq!(t("M/extrathumbs/thumb1.jpg"), T::Extrathumb);
    // Other names in those folders stay graphics.
    assert_eq!(t("M/extrafanart/anything.jpg"), T::Graphic);
    assert_eq!(t("M/extrathumbs/frame.jpg"), T::Graphic);
    assert_eq!(t("M/extrathumb/frame.jpg"), T::Graphic);
    // Everything in a characterart folder is character art.
    assert_eq!(t("M/characterart/hero.png"), T::Characterart);
}

#[test]
fn a_file_named_like_its_folder_is_the_poster() {
    assert_eq!(t("Heat (1995)/Heat (1995)/Heat (1995).jpg"), T::Poster);
    assert_eq!(t("Show/Season 01/Season 01.jpg"), T::Poster);
    assert_eq!(t("Heat (1995)/Other Name.jpg"), T::Graphic);
}

#[test]
fn dvd_structure() {
    let c = classify("Disc/VIDEO_TS/VIDEO_TS.IFO");
    assert_eq!(c.file_type, T::Video);
    assert!(c.disc_file && c.main_disc_identifier);

    let c = classify("Disc/VIDEO_TS/VTS_01_1.VOB");
    assert_eq!(c.file_type, T::Video);
    assert!(c.disc_file && !c.main_disc_identifier);

    let c = classify("Disc/VIDEO_TS/VTS_01_0.BUP");
    assert_eq!(c.file_type, T::Unknown);
    assert!(c.disc_file);

    let c = classify("Disc/VIDEO_TS");
    assert_eq!(c.file_type, T::Video);
    assert!(c.disc_file && !c.main_disc_identifier);
}

#[test]
fn bluray_structure() {
    let c = classify("Disc/BDMV/index.bdmv");
    assert_eq!(c.file_type, T::Video);
    assert!(c.disc_file && c.main_disc_identifier);

    let c = classify("Disc/BDMV/STREAM/00000.m2ts");
    assert_eq!(c.file_type, T::Video);
    assert!(c.disc_file && !c.main_disc_identifier);

    for name in ["PLAYLIST/00000.mpls", "CLIPINF/00000.clpi"] {
        let c = classify(&format!("Disc/BDMV/{name}"));
        assert_eq!(c.file_type, T::Unknown, "{name}");
        assert!(c.disc_file, "{name}");
    }
    assert_eq!(classify("Disc/BDMV").file_type, T::Video);
}

#[test]
fn hd_dvd_structure() {
    let c = classify("Disc/HVDVD_TS/HV000I01.IFO");
    assert_eq!(c.file_type, T::Video);
    assert!(c.disc_file && c.main_disc_identifier);

    let c = classify("Disc/title001.evo");
    assert_eq!(c.file_type, T::Video);
    assert!(c.disc_file && !c.main_disc_identifier);
    assert_eq!(classify("Disc/HVDVD_TS").file_type, T::Video);
}

#[test]
fn ordinary_files_are_not_disc_files() {
    for path in [
        "M/M.mkv",
        "M/M.iso",
        "M/index.html",
        "M/M.nfo",
        "M/12345.mkv",
    ] {
        let c = classify(path);
        assert!(!c.disc_file && !c.main_disc_identifier, "{path}");
    }
}

#[test]
fn trickplay_index_names() {
    assert!(is_trickplay_index("Heat-320-10.bif"));
    assert!(is_trickplay_index("HEAT-320-10.BIF"));
    assert!(!is_trickplay_index("Heat.bif"));
}

#[test]
fn extension_lists_have_no_duplicates_and_are_lower_case() {
    for (name, list) in [
        ("artwork", ARTWORK_EXTENSIONS),
        ("video", VIDEO_EXTENSIONS),
        ("audio", AUDIO_EXTENSIONS),
        ("subtitle", SUBTITLE_EXTENSIONS),
    ] {
        let mut sorted = list.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), list.len(), "{name} has duplicates");
        assert!(list.iter().all(|e| *e == e.to_lowercase()), "{name}");
    }
}

#[test]
fn a_ts_file_counts_as_video_only_when_it_is_a_transport_stream() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let mut stream = vec![0u8; 400];
    stream[0] = 0x47;
    stream[188] = 0x47;
    let mut m2ts = vec![0u8; 400];
    m2ts[4] = 0x47;
    m2ts[196] = 0x47;
    assert!(is_video_content(&write("Recording.ts", &stream)));
    assert!(is_video_content(&write("Recording 2.TS", &m2ts)));
    assert!(!is_video_content(&write(
        "index.ts",
        b"export const answer = 42;\n"
    )));
    assert!(!is_video_content(&write("types.d.ts", &stream)));
    // Nothing to judge: an empty or unreadable file is kept.
    assert!(is_video_content(&write("Stub.ts", b"")));
    assert!(is_video_content(&dir.path().join("missing.ts")));
    // Other extensions are not read at all.
    assert!(is_video_content(&dir.path().join("missing.mkv")));
}

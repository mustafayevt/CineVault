use super::*;

fn flags(list: &[StreamFlag]) -> BTreeSet<StreamFlag> {
    list.iter().copied().collect()
}

#[test]
fn the_spec_example_is_turkish_and_forced() {
    let info = detect_external_stream("Heat (1995)", "Heat (1995).tr.forced.srt");
    assert_eq!(info.language, "tr");
    assert_eq!(info.flags, flags(&[StreamFlag::Forced]));
    assert_eq!(info.title, "");
}

#[test]
fn language_names_codes_and_locales_normalise() {
    let n = |text: &str| normalize_language(text);
    assert_eq!(n("eng").as_deref(), Some("en"));
    assert_eq!(n("English").as_deref(), Some("en"));
    assert_eq!(n("GER").as_deref(), Some("de"));
    assert_eq!(n("pt_br").as_deref(), Some("pt-BR"));
    assert_eq!(n("zho").as_deref(), Some("zh"));
    assert_eq!(n("fil").as_deref(), Some("fil"));
    assert_eq!(n("xx"), None);
    assert_eq!(n(""), None);
    assert_eq!(n("en-forced"), None);
}

#[test]
fn a_locale_needs_a_real_region() {
    assert_eq!(locale("pt-BR").as_deref(), Some("pt-BR"));
    assert_eq!(locale("en-US").as_deref(), Some("en-US"));
    assert_eq!(locale("en"), None);
    assert_eq!(locale("en-xx"), None);
    assert_eq!(locale("forced-en"), None);
    assert_eq!(locale("en-US-extra"), None);
}

#[test]
fn idx_files_name_the_language_of_the_first_track() {
    let idx = "# VobSub index file, v7 (do not modify this line!)\nsize: 720x576\n\nlangidx: 0\n\nid: tr, index: 0\ntimestamp: 00:00:01:000, filepos: 000000000\nid: en, index: 1\n";
    assert_eq!(idx_language(idx).as_deref(), Some("tr"));
    assert_eq!(idx_language("id: eng, index: 0").as_deref(), Some("en"));
    assert_eq!(
        idx_language("id: pt-br, index: 0").as_deref(),
        Some("pt-BR")
    );
    // An unknown language leaves the language empty.
    assert_eq!(idx_language("id: , index: 0"), None);
    assert_eq!(idx_language("id: --, index: 0"), None);
    assert_eq!(idx_language("size: 720x576"), None);
    assert_eq!(idx_language(""), None);
}

#[test]
fn a_sub_file_takes_its_language_from_the_idx_beside_it() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("Heat (1995).forced.sub");
    std::fs::write(&sub, b"\0").unwrap();
    std::fs::write(
        dir.path().join("Heat (1995).forced.idx"),
        "id: fr, index: 0\n",
    )
    .unwrap();
    let info = external_subtitle("Heat (1995)", &sub);
    assert_eq!(info.language, "fr");
    assert_eq!(info.flags, flags(&[StreamFlag::Forced]));
}

#[test]
fn a_sub_file_without_an_idx_uses_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("Heat (1995).de.sub");
    std::fs::write(&sub, b"\0").unwrap();
    assert_eq!(external_subtitle("Heat (1995)", &sub).language, "de");
}

#[test]
fn an_idx_with_no_language_keeps_the_name_language() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("Heat (1995).es.sub");
    std::fs::write(&sub, b"\0").unwrap();
    std::fs::write(dir.path().join("Heat (1995).es.idx"), "id: , index: 0\n").unwrap();
    assert_eq!(external_subtitle("Heat (1995)", &sub).language, "es");
}

#[test]
fn only_real_extensions_are_stripped() {
    assert_eq!(strip_extension("a.b.srt"), "a.b");
    assert_eq!(strip_extension("Movie.en"), "Movie");
    assert_eq!(strip_extension("noext"), "noext");
    assert_eq!(strip_extension(".hidden"), ".hidden");
    assert_eq!(strip_extension("Dr. Strange"), "Dr. Strange");
}

#[test]
fn the_video_prefix_is_matched_without_case() {
    assert_eq!(strip_prefix_ignore_case("HEAT.en", "heat"), Some(".en"));
    assert_eq!(strip_prefix_ignore_case("heat", "Heat"), Some(""));
    assert_eq!(strip_prefix_ignore_case("hea", "Heat"), None);
    assert_eq!(strip_prefix_ignore_case("Élan.fr", "élan"), Some(".fr"));
}

#[test]
fn brazilian_portuguese_round_trips_through_the_written_name() {
    assert_eq!(normalize_language("pob").as_deref(), Some("pt-BR"));
    assert_eq!(normalize_language("PB").as_deref(), Some("pt-BR"));
    let info = detect_external_stream("Heat (1995)", "Heat (1995).pob.srt");
    assert_eq!(info.language, "pt-BR");
    assert_eq!(info.title, "");
    let info = detect_external_stream("Heat (1995)", "Heat (1995).pb.forced.srt");
    assert_eq!(info.language, "pt-BR");
    assert_eq!(info.flags, flags(&[StreamFlag::Forced]));
    // European Portuguese stays plain `pt`.
    assert_eq!(
        detect_external_stream("Heat (1995)", "Heat (1995).por.srt").language,
        "pt"
    );
}

/// reg2-6: with nothing read from the file (no MediaInfo), the name gives the language and the
/// marks; what was read wins where it says something.
#[test]
fn a_subtitle_file_stream_fills_its_gaps_from_the_file_name() {
    use cv_domain::MediaFileType;
    let file = |name: &str| MediaFile::new(MediaFileType::Subtitle, "D:/Movies/Heat (1995)", name);

    let forced = subtitle_file_stream(&file("Heat (1995).deu.forced.srt"), "Heat (1995)");
    assert_eq!(forced.language, "de");
    assert_eq!(forced.flags, flags(&[StreamFlag::Forced]));
    assert_eq!(
        subtitle_file_stream(&file("Heat (1995).jpn.srt"), "Heat (1995)").language,
        "ja"
    );

    let mut read = file("Heat (1995).eng.srt");
    read.subtitles.push(SubtitleStream {
        codec: "SRT".to_owned(),
        language: "fr".to_owned(),
        ..SubtitleStream::default()
    });
    let stream = subtitle_file_stream(&read, "Heat (1995)");
    assert_eq!(
        (stream.language.as_str(), stream.codec.as_str()),
        ("fr", "SRT")
    );

    assert_eq!(
        subtitle_file_stream(&file("Heat (1995).srt"), "Heat (1995)").language,
        ""
    );
}

/// A second track of one language is saved as `Title.eng.2.srt` (subtitle
/// extraction): the language and the flags still read, and the number stays as the title so a
/// rename can write it back.
#[test]
fn an_index_before_the_extension_leaves_the_language_and_flags_alone() {
    let info = detect_external_stream("Heat (1995)", "Heat (1995).eng.2.srt");
    assert_eq!(info.language, "en");
    assert!(info.flags.is_empty(), "{:?}", info.flags);
    assert_eq!(info.title, "2");

    let info = detect_external_stream("Heat (1995)", "Heat (1995).tur.forced.sdh.3.srt");
    assert_eq!(info.language, "tr");
    assert_eq!(
        info.flags,
        flags(&[StreamFlag::Forced, StreamFlag::HearingImpaired])
    );
    assert_eq!(info.title, "3");

    // An unknown language with an index still reads as unknown, not as a number.
    let info = detect_external_stream("Heat (1995)", "Heat (1995).und.2.srt");
    assert_eq!(info.language, "");
    assert!(info.flags.is_empty());
}

use super::xml::{field_name, tag_name};
use super::*;
use crate::mediainfo::StreamInfo;

#[test]
fn tag_names_drop_parentheses_and_replace_separators() {
    assert_eq!(tag_name("Channel(s)"), "Channels");
    assert_eq!(tag_name("Channel(s)_Original"), "Channels_Original");
    assert_eq!(tag_name("Duration/String3"), "Duration_String3");
    assert_eq!(tag_name("Format/Extensions"), "Format_Extensions");
    assert_eq!(tag_name("colour_primaries"), "colour_primaries");
    assert_eq!(tag_name("a.b:c*d e"), "a_b_c_d_e");
    assert_eq!(tag_name("3D"), "_3D");
}

#[test]
fn field_names_undo_the_tag_names_the_mapping_reads() {
    for name in [
        "Channel(s)",
        "Channel(s)_Original",
        "Duration/String3",
        "Language/String",
        "Language/String3",
        "HDR_Format/String",
        "Format/Extensions",
        "Codec/Extensions",
        "CodecID/Hint",
        "Format/Info",
        "Format_Profile",
        "Audio_Channels_Total",
        "ChannelLayout",
        "BitRate_Mode",
    ] {
        assert_eq!(field_name(&tag_name(name)), name);
    }
}

fn parsed(xml: &str) -> MediaSnapshot {
    parse_sidecar(xml).unwrap()
}

#[test]
fn durations_are_converted_between_seconds_and_milliseconds() {
    let snapshot = parsed(
        r#"<MediaInfo><media ref="a"><track type="General"><Duration>5400.500000000</Duration></track>
           <track type="Video"><Duration>5400.5</Duration></track></media></MediaInfo>"#,
    );
    let general = snapshot.general().unwrap();
    assert_eq!(general.get("Duration"), Some("5400500"));
    assert_eq!(general.get("Duration/String3"), Some("01:30:00.500"));
    assert_eq!(snapshot.video().unwrap().get("Duration"), Some("5400500"));

    let xml = render_sidecar(&snapshot, "a", "");
    assert!(xml.contains("<Duration>5400.500000000</Duration>"), "{xml}");
    assert!(!xml.contains("String3"));
}

#[test]
fn entities_cdata_and_character_references_are_read() {
    let snapshot = parsed(
        "<MediaInfo><media ref=\"a\"><track type=\"General\">\
         <Title>Tom &amp; Jerry &#233; &#x41; &lt;1&gt;</Title>\
         <Comment><![CDATA[a < b]]></Comment></track></media></MediaInfo>",
    );
    let general = snapshot.general().unwrap();
    assert_eq!(general.get("Title"), Some("Tom & Jerry é A <1>"));
    assert_eq!(general.get("Comment"), Some("a < b"));
}

#[test]
fn written_text_is_escaped_and_reads_back() {
    let mut general = StreamInfo::new();
    general.push("Title", "A & B <\"C\"> 'd'\u{1}");
    let mut snapshot = MediaSnapshot::new();
    snapshot.push(StreamKind::General, general);
    let xml = render_sidecar(&snapshot, "x & \"y\".mkv", "");
    assert!(xml.contains("<Title>A &amp; B &lt;&quot;C&quot;&gt; 'd'</Title>"));
    assert!(xml.contains("ref=\"x &amp; &quot;y&quot;.mkv\""));
    assert_eq!(
        parsed(&xml).general().unwrap().get("Title"),
        Some("A & B <\"C\"> 'd'")
    );
}

#[test]
fn the_older_layout_and_unknown_tracks_are_handled() {
    // No `media` element, a different case, an unknown track type and an empty track.
    let snapshot = parsed(
        "<Mediainfo><File><track type=\"General\"><Format>MPEG-4</Format></track>\
         <track type=\"Bogus\"><Format>x</Format></track>\
         <track type=\"Video\"/><track type=\"Audio\"><Format>AAC</Format></track></File></Mediainfo>",
    );
    assert_eq!(snapshot.general().unwrap().get("Format"), Some("MPEG-4"));
    assert_eq!(snapshot.streams(StreamKind::Video).len(), 1);
    assert_eq!(snapshot.streams(StreamKind::Audio).len(), 1);
    assert_eq!(snapshot.streams(StreamKind::Other).len(), 0);
}

#[test]
fn only_the_first_media_element_is_read() {
    let snapshot = parsed(
        "<MediaInfo><media ref=\"a\"><track type=\"General\"><Format>one</Format></track></media>\
         <media ref=\"b\"><track type=\"General\"><Format>two</Format></track></media></MediaInfo>",
    );
    assert_eq!(snapshot.streams(StreamKind::General).len(), 1);
    assert_eq!(snapshot.general().unwrap().get("Format"), Some("one"));
}

#[test]
fn documents_that_are_not_mediainfo_are_rejected() {
    assert!(matches!(
        parse_sidecar("<movie><title>x</title></movie>"),
        Err(SidecarError::NotMediaInfo)
    ));
    assert!(matches!(parse_sidecar(""), Err(SidecarError::NotMediaInfo)));
    assert!(matches!(
        parse_sidecar("<MediaInfo><media></MediaInfo>"),
        Err(SidecarError::Xml { .. })
    ));
}

#[test]
fn a_byte_order_mark_is_ignored() {
    let snapshot = parsed(
        "\u{feff}<?xml version=\"1.0\"?><MediaInfo><media ref=\"a\">\
         <track type=\"Audio\"><Format>AAC</Format></track></media></MediaInfo>",
    );
    assert_eq!(snapshot.streams(StreamKind::Audio).len(), 1);
}

#[test]
fn typeorder_numbers_the_second_stream_of_a_kind_on() {
    let mut snapshot = MediaSnapshot::new();
    for _ in 0..3 {
        let mut audio = StreamInfo::new();
        audio.push("Format", "AAC");
        snapshot.push(StreamKind::Audio, audio);
    }
    let xml = render_sidecar(&snapshot, "a", "");
    assert!(xml.contains("<track type=\"Audio\">"));
    assert!(xml.contains("<track type=\"Audio\" typeorder=\"2\">"));
    assert!(xml.contains("<track type=\"Audio\" typeorder=\"3\">"));
    assert_eq!(parsed(&xml).streams(StreamKind::Audio).len(), 3);
}

//! The MediaInfo XML format (`https://mediaarea.net/mediainfo`, version 2.0): reading it into a
//! [`MediaSnapshot`] and writing a snapshot out.
//!
//! Snapshots keep the library's own conventions (`Channel(s)`, `Duration/String3`, durations in
//! milliseconds), the XML keeps the export's: tag names have no `()` and no `/`, and durations
//! are seconds with decimals. Both directions convert, so a snapshot read from a sidecar maps
//! to streams exactly like one read from the library.

use std::fmt::Write as _;

use quick_xml::Reader;
use quick_xml::events::BytesStart;
use quick_xml::events::Event;

use super::SidecarError;
use crate::mediainfo::{MediaSnapshot, StreamInfo, StreamKind};

/// Fields the library reports in milliseconds and the XML in seconds.
const DURATION_FIELDS: [&str; 2] = ["Duration", "Source_Duration"];

/// Display renderings the XML export does not contain (`Duration/String3`, `Language/String`).
fn is_display_field(name: &str) -> bool {
    name.contains("/String")
}

// ---------------------------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------------------------

/// The XML tag for a field name: `()` removed, anything else that is not valid in a tag
/// (`/`, `*`, `:`, `.`, spaces) becomes `_`.
pub(super) fn tag_name(field: &str) -> String {
    let mut tag: String = field
        .chars()
        .filter(|c| !matches!(c, '(' | ')'))
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if tag.is_empty() || tag.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
        tag.insert(0, '_');
    }
    tag
}

/// The library's name for an XML tag: the inverse of [`tag_name`] for the names that change.
pub(super) fn field_name(tag: &str) -> String {
    // `Channels` is `Channel(s)`: the only name with parentheses.
    if tag == "Channels" {
        return "Channel(s)".to_owned();
    }
    if let Some(rest) = tag.strip_prefix("Channels_") {
        return format!("Channel(s)_{rest}");
    }
    for suffix in ["_Extensions", "_Hint", "_Url", "_Info"] {
        if let Some(base) = tag.strip_suffix(suffix) {
            return format!("{base}/{}", &suffix[1..]);
        }
    }
    if let Some(at) = tag.rfind("_String") {
        let digits = &tag[at + "_String".len()..];
        if digits.len() <= 1 && digits.bytes().all(|b| b.is_ascii_digit()) {
            return format!("{}/String{digits}", &tag[..at]);
        }
    }
    tag.to_owned()
}

// ---------------------------------------------------------------------------------------------
// Seconds and milliseconds
// ---------------------------------------------------------------------------------------------

fn trim_decimals(mut text: String) -> String {
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

/// Seconds as the XML writes them to milliseconds as the library reports them.
fn seconds_to_milliseconds(text: &str) -> Option<f64> {
    let seconds: f64 = text.trim().parse().ok()?;
    seconds.is_finite().then_some(seconds * 1000.0)
}

fn milliseconds_to_seconds(text: &str) -> Option<String> {
    let milliseconds: f64 = text.trim().parse().ok()?;
    milliseconds
        .is_finite()
        .then(|| format!("{:.9}", milliseconds / 1000.0))
}

/// `hh:mm:ss.mmm`, which is what `Duration/String3` holds.
fn clock(milliseconds: f64) -> String {
    let total = milliseconds.round().max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        total / 3_600_000,
        total / 60_000 % 60,
        total / 1000 % 60,
        total % 1000
    )
}

// ---------------------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------------------

fn xml_error(reader: &Reader<&[u8]>, error: impl std::fmt::Display) -> SidecarError {
    SidecarError::Xml {
        position: reader.buffer_position(),
        message: error.to_string(),
    }
}

fn local_name(start: &BytesStart<'_>) -> String {
    start.local_name().as_ref().to_owned()
}

fn kind_of(start: &BytesStart<'_>) -> Option<StreamKind> {
    let attribute = start.try_get_attribute("type").ok()??;
    let value = attribute
        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
        .ok()?;
    StreamKind::ALL
        .into_iter()
        .find(|kind| kind.as_str().eq_ignore_ascii_case(value.trim()))
}

/// Converts a finished stream from the XML's conventions to the library's.
fn finish_stream(stream: &mut StreamInfo) {
    for name in DURATION_FIELDS {
        let Some(milliseconds) = stream.get(name).and_then(seconds_to_milliseconds) else {
            continue;
        };
        stream.set(name, trim_decimals(format!("{milliseconds:.6}")));
        if name == "Duration" {
            stream.set("Duration/String3", clock(milliseconds));
        }
    }
}

/// The track being read: where it sits in the element path, its kind and its fields so far.
type OpenTrack = (usize, StreamKind, StreamInfo);

/// The field being read: its name and its text so far.
type OpenField = (String, String);

/// Reads a MediaInfo XML document: the tracks of its first `media` element (or of the whole
/// document, for the older layout without one).
///
/// # Errors
/// [`SidecarError::Xml`] for text that is not well-formed XML,
/// [`SidecarError::NotMediaInfo`] when the root element is not `MediaInfo`.
pub(super) fn parse(xml: &str) -> Result<MediaSnapshot, SidecarError> {
    let mut reader = Reader::from_str(xml.trim_start_matches('\u{feff}'));
    let mut snapshot = MediaSnapshot::new();
    let mut path: Vec<String> = Vec::new();
    let mut medias = 0_usize;
    let mut track: Option<OpenTrack> = None;
    let mut field: Option<OpenField> = None;
    let mut seen_root = false;

    loop {
        let event = reader.read_event().map_err(|e| xml_error(&reader, e))?;
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                let name = local_name(start);
                if path.is_empty() {
                    seen_root = true;
                    if !name.eq_ignore_ascii_case("MediaInfo") {
                        return Err(SidecarError::NotMediaInfo);
                    }
                }
                begin(&name, start, &path, &mut medias, &mut track, &mut field);
                path.push(name);
                if matches!(event, Event::Empty(_)) {
                    end(&mut path, &mut track, &mut field, &mut snapshot);
                }
            }
            Event::Text(ref text) => {
                if let Some((_, buffer)) = field.as_mut() {
                    buffer.push_str(&text.xml10_content());
                }
            }
            Event::CData(ref data) => {
                if let Some((_, buffer)) = field.as_mut() {
                    buffer.push_str(&data.xml10_content());
                }
            }
            Event::GeneralRef(ref reference) => {
                if let Some((_, buffer)) = field.as_mut() {
                    match reference.resolve_char_ref() {
                        Ok(Some(c)) => buffer.push(c),
                        Ok(None) => match quick_xml::escape::resolve_xml_entity(reference) {
                            Some(text) => buffer.push_str(text),
                            None => {
                                let message = format!("unknown entity &{};", &**reference);
                                return Err(xml_error(&reader, message));
                            }
                        },
                        Err(e) => return Err(xml_error(&reader, e)),
                    }
                }
            }
            Event::End(_) => end(&mut path, &mut track, &mut field, &mut snapshot),
            Event::Eof => break,
            _ => {}
        }
        // Everything after the first `media` element belongs to another file.
        if medias > 1 {
            break;
        }
    }
    if !seen_root {
        return Err(SidecarError::NotMediaInfo);
    }
    Ok(snapshot)
}

/// Handles an element start: a `media` element, a `track`, or a field inside a track.
fn begin(
    name: &str,
    start: &BytesStart<'_>,
    path: &[String],
    medias: &mut usize,
    track: &mut Option<OpenTrack>,
    field: &mut Option<OpenField>,
) {
    match track {
        None => {
            if name.eq_ignore_ascii_case("media") {
                *medias += 1;
            } else if name == "track"
                && *medias <= 1
                && let Some(kind) = kind_of(start)
            {
                *track = Some((path.len(), kind, StreamInfo::new()));
            }
        }
        Some((depth, _, _)) => {
            let inside = path.len() - *depth;
            // A field is a direct child of the track; the children of `extra` are fields too.
            if inside == 1 {
                *field = (name != "extra").then(|| (field_name(name), String::new()));
            } else if inside == 2 && path.last().is_some_and(|parent| parent == "extra") {
                *field = Some((format!("extra/{name}"), String::new()));
            }
        }
    }
}

/// Handles an element end: stores a finished field, or a finished track.
fn end(
    path: &mut Vec<String>,
    track: &mut Option<OpenTrack>,
    field: &mut Option<OpenField>,
    snapshot: &mut MediaSnapshot,
) {
    path.pop();
    let Some((depth, _, stream)) = track.as_mut() else {
        return;
    };
    if let Some((name, text)) = field.take() {
        stream.push(name, text.trim());
    }
    if path.len() == *depth
        && let Some((_, kind, mut stream)) = track.take()
    {
        finish_stream(&mut stream);
        snapshot.push(kind, stream);
    }
}

// ---------------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------------

fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Control characters other than tab and line breaks are not valid in XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
}

/// Writes `snapshot` as a MediaInfo XML document. `media_ref` names the file (the `ref`
/// attribute), `library_version` the library that read it (left out when empty).
///
/// Fields keep the library's order. The display renderings (`Duration/String3`) are left out,
/// durations are written in seconds, and the `extra/` fields go into an `extra` element.
/// Streams of one kind are numbered with `typeorder`, from the second one on.
pub(super) fn render(snapshot: &MediaSnapshot, media_ref: &str, library_version: &str) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<MediaInfo\n");
    out.push_str("    xmlns=\"https://mediaarea.net/mediainfo\"\n");
    out.push_str("    xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\n");
    out.push_str(
        "    xsi:schemaLocation=\"https://mediaarea.net/mediainfo https://mediaarea.net/mediainfo/mediainfo_2_0.xsd\"\n",
    );
    out.push_str("    version=\"2.0\">\n");
    if !library_version.is_empty() {
        out.push_str("<creatingLibrary version=\"");
        escape(library_version, &mut out);
        out.push_str("\" url=\"https://mediaarea.net/MediaInfo\">MediaInfoLib</creatingLibrary>\n");
    }
    out.push_str("<media ref=\"");
    escape(media_ref, &mut out);
    out.push_str("\">\n");
    for kind in StreamKind::ALL {
        for (order, stream) in snapshot.streams(kind).iter().enumerate() {
            render_track(kind, order, stream, &mut out);
        }
    }
    out.push_str("</media>\n</MediaInfo>\n");
    out
}

fn render_track(kind: StreamKind, order: usize, stream: &StreamInfo, out: &mut String) {
    let _ = write!(out, "<track type=\"{kind}\"");
    if order > 0 {
        let _ = write!(out, " typeorder=\"{}\"", order + 1);
    }
    out.push_str(">\n");
    let mut extra: Vec<(&str, &str)> = Vec::new();
    for (name, value) in stream.iter() {
        if let Some(extra_name) = name.strip_prefix("extra/") {
            extra.push((extra_name, value));
            continue;
        }
        if is_display_field(name) {
            continue;
        }
        let seconds;
        let value = if DURATION_FIELDS.contains(&name) {
            seconds = milliseconds_to_seconds(value);
            seconds.as_deref().unwrap_or(value)
        } else {
            value
        };
        render_field(&tag_name(name), value, out);
    }
    if !extra.is_empty() {
        out.push_str("<extra>\n");
        for (name, value) in extra {
            render_field(&tag_name(name), value, out);
        }
        out.push_str("</extra>\n");
    }
    out.push_str("</track>\n");
}

fn render_field(tag: &str, value: &str, out: &mut String) {
    let _ = write!(out, "<{tag}>");
    escape(value, out);
    let _ = writeln!(out, "</{tag}>");
}

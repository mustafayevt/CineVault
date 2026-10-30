//! What MediaInfo reported about one file: every stream, every non-empty field.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The kinds of stream MediaInfo describes. The order is the library's own numbering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    General,
    Video,
    Audio,
    Text,
    Other,
    Image,
    Menu,
}

impl StreamKind {
    /// Every kind, in the library's order.
    pub const ALL: [Self; 7] = [
        Self::General,
        Self::Video,
        Self::Audio,
        Self::Text,
        Self::Other,
        Self::Image,
        Self::Menu,
    ];

    /// The number the library uses for this kind (`MediaInfo_stream_C`).
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The name MediaInfo uses for the kind in its XML (`General`, `Video`, `Text`, ...).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Text => "Text",
            Self::Other => "Other",
            Self::Image => "Image",
            Self::Menu => "Menu",
        }
    }
}

impl fmt::Display for StreamKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One stream: its fields as `(name, text)` pairs in the library's order, empty values dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamInfo {
    fields: Vec<(String, String)>,
}

impl StreamInfo {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a field. Empty values are dropped, like the library snapshot does.
    pub fn push(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let value = value.into();
        if !value.is_empty() {
            self.fields.push((name.into(), value));
        }
    }

    /// Sets the field called `name`, replacing its first occurrence, or adds it. An empty value
    /// removes the field.
    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        let position = self.fields.iter().position(|(field, _)| field == name);
        match (position, value.is_empty()) {
            (Some(at), true) => {
                self.fields.remove(at);
            }
            (Some(at), false) => self.fields[at].1 = value,
            (None, false) => self.fields.push((name.to_owned(), value)),
            (None, true) => {}
        }
    }

    /// The text of the field called `name` (the first one, when a name repeats).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }

    /// A field read as a number, e.g. `Width`, `Duration` (milliseconds) or `Channels`.
    #[must_use]
    pub fn get_u64(&self, name: &str) -> Option<u64> {
        let text = self.get(name)?;
        // Some fields hold several values ("6 / 2" for channel layouts); the first one counts.
        let first = text.split(['/', ' ']).next()?;
        first
            .parse::<u64>()
            .ok()
            .or_else(|| first.parse::<f64>().ok().map(|v| v.round() as u64))
    }

    /// A field read as a decimal, e.g. `FrameRate`.
    #[must_use]
    pub fn get_f64(&self, name: &str) -> Option<f64> {
        self.get(name)?.split(['/', ' ']).next()?.parse().ok()
    }

    /// The fields in the order the library reported them.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.fields.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

/// Everything MediaInfo reported about one file.
///
/// Field names are MediaInfo's own (`Format`, `Duration`, `Width`, `Channel(s)`, ...). Mapping
/// them to CineVault's stream model is [`super::apply_snapshot`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaSnapshot {
    streams: [Vec<StreamInfo>; 7],
}

impl MediaSnapshot {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a stream of `kind`.
    pub fn push(&mut self, kind: StreamKind, stream: StreamInfo) {
        self.streams[kind.index()].push(stream);
    }

    /// The streams of one kind, in file order.
    #[must_use]
    pub fn streams(&self, kind: StreamKind) -> &[StreamInfo] {
        &self.streams[kind.index()]
    }

    /// The streams of one kind, for changing them.
    pub fn streams_mut(&mut self, kind: StreamKind) -> &mut [StreamInfo] {
        &mut self.streams[kind.index()]
    }

    /// The container-level section.
    #[must_use]
    pub fn general(&self) -> Option<&StreamInfo> {
        self.streams(StreamKind::General).first()
    }

    /// The container-level section, for changing it.
    pub fn general_mut(&mut self) -> Option<&mut StreamInfo> {
        self.streams[StreamKind::General.index()].first_mut()
    }

    /// The first video stream.
    #[must_use]
    pub fn video(&self) -> Option<&StreamInfo> {
        self.streams(StreamKind::Video).first()
    }

    /// True when the file has at least one video, audio, text or image stream. A file MediaInfo
    /// opened but found nothing playable in (a corrupt or empty file) has only a general section.
    #[must_use]
    pub fn has_media_streams(&self) -> bool {
        [
            StreamKind::Video,
            StreamKind::Audio,
            StreamKind::Text,
            StreamKind::Image,
        ]
        .into_iter()
        .any(|kind| !self.streams(kind).is_empty())
    }
}

/// The outcome for one file of a batch.
#[derive(Debug)]
pub struct BatchItem {
    pub path: PathBuf,
    pub result: Result<MediaSnapshot, super::MediaInfoError>,
}

impl BatchItem {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_values_are_dropped_and_the_first_duplicate_wins() {
        let mut stream = StreamInfo::new();
        stream.push("Format", "AVC");
        stream.push("Title", "");
        stream.push("Format", "other");
        assert_eq!(stream.len(), 2);
        assert_eq!(stream.get("Format"), Some("AVC"));
        assert_eq!(stream.get("Title"), None);
    }

    #[test]
    fn set_replaces_adds_and_removes_a_field() {
        let mut stream = StreamInfo::new();
        stream.push("Duration", "1000");
        stream.push("Duration/String3", "00:00:01.000");
        stream.set("Duration", "5000");
        stream.set("Format", "BDAV");
        stream.set("Duration/String3", "");
        stream.set("Missing", "");
        assert_eq!(stream.get("Duration"), Some("5000"));
        assert_eq!(stream.get("Format"), Some("BDAV"));
        assert_eq!(stream.get("Duration/String3"), None);
        assert_eq!(stream.len(), 2);
    }

    #[test]
    fn numbers_are_read_from_the_first_value() {
        let mut stream = StreamInfo::new();
        stream.push("Width", "1920");
        stream.push("Channels", "6 / 2");
        stream.push("FrameRate", "23.976");
        stream.push("Duration", "5400000.000000");
        assert_eq!(stream.get_u64("Width"), Some(1920));
        assert_eq!(stream.get_u64("Channels"), Some(6));
        assert_eq!(stream.get_u64("Duration"), Some(5_400_000));
        assert!((stream.get_f64("FrameRate").unwrap() - 23.976).abs() < 1e-9);
        assert_eq!(stream.get_u64("Missing"), None);
    }

    #[test]
    fn a_general_only_snapshot_has_no_media_streams() {
        let mut snapshot = MediaSnapshot::new();
        snapshot.push(StreamKind::General, StreamInfo::new());
        assert!(!snapshot.has_media_streams());
        snapshot.push(StreamKind::Audio, StreamInfo::new());
        assert!(snapshot.has_media_streams());
    }

    #[test]
    fn the_kind_numbers_match_the_library() {
        for (number, kind) in StreamKind::ALL.into_iter().enumerate() {
            assert_eq!(kind.index(), number);
        }
    }
}

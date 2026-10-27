//! Data sources: the media folders a library is built from.

use serde::{Deserialize, Serialize};

/// Which library a data source feeds.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// A folder of movies.
    #[default]
    Movie,
    /// A folder of TV shows.
    Tv,
}

impl SourceKind {
    /// Both kinds, movies first.
    pub const ALL: [Self; 2] = [Self::Movie, Self::Tv];

    /// The stable text used in storage and IPC.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Tv => "tv",
        }
    }

    /// Parses the text produced by [`SourceKind::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == text)
    }
}

impl std::fmt::Display for SourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stored data source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSource {
    pub id: i64,
    pub kind: SourceKind,
    /// Absolute root path in the native syntax of the operating system it was added on.
    pub path: String,
    /// The name the user sees for the source ("NAS 4K"). It defaults to the folder's short name
    /// and is unique within its kind. Changing it never touches a file.
    #[serde(default)]
    pub label: String,
    /// Display order inside its kind, starting at 0.
    pub position: i64,
    /// When the source was added, in epoch milliseconds.
    pub created_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_text_round_trips() {
        for kind in SourceKind::ALL {
            assert_eq!(SourceKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(SourceKind::parse("music"), None);
    }
}

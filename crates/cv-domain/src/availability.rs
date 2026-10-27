//! Availability: whether an item's files can currently be reached.

use serde::{Deserialize, Serialize};

/// Where an item stands relative to the file system.
///
/// Availability is computed from the file system and stored next to the item, together with a
/// human-readable reason. No state ever causes an item to be deleted; the user decides whether
/// to remove or relocate an item that is not [`Availability::Online`].
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// The item's folder (and video files) are reachable.
    #[default]
    Online,
    /// The item's data source cannot be reached, for example an unplugged drive.
    Offline,
    /// The data source is reachable but the item's folder or video files are gone.
    Missing,
    /// The item cannot be located at all: no path, a path from another operating system, or a
    /// show without any episodes.
    Invalid,
}

impl Availability {
    /// All states, in severity order.
    pub const ALL: [Self; 4] = [Self::Online, Self::Offline, Self::Missing, Self::Invalid];

    /// The stable text used in storage, filters and IPC.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
        }
    }

    /// Parses the text produced by [`Availability::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == text)
    }

    /// True for every state except [`Availability::Online`].
    pub const fn is_unavailable(self) -> bool {
        !matches!(self, Self::Online)
    }
}

impl std::fmt::Display for Availability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips_and_matches_serde() {
        for a in Availability::ALL {
            assert_eq!(Availability::parse(a.as_str()), Some(a));
            assert_eq!(serde_json::to_string(&a).unwrap(), format!("\"{a}\""));
        }
        assert_eq!(Availability::parse("gone"), None);
        assert!(!Availability::Online.is_unavailable());
        assert!(Availability::Offline.is_unavailable());
    }
}

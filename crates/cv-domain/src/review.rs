//! Review state: whether a match needs a human decision.

use serde::{Deserialize, Serialize};

/// Where an item stands in the review workflow. Stored on the item so it survives restarts.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    /// Nothing to review.
    #[default]
    None,
    /// Matching could not decide on its own; the item waits in the review queue.
    NeedsReview,
    /// A human looked at it and decided.
    Resolved,
}

impl ReviewState {
    /// All states.
    pub const ALL: [Self; 3] = [Self::None, Self::NeedsReview, Self::Resolved];

    /// The stable text used in storage, filters and IPC.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::NeedsReview => "needs_review",
            Self::Resolved => "resolved",
        }
    }

    /// Parses the text produced by [`ReviewState::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }
}

impl std::fmt::Display for ReviewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips_and_matches_serde() {
        for s in ReviewState::ALL {
            assert_eq!(ReviewState::parse(s.as_str()), Some(s));
            assert_eq!(serde_json::to_string(&s).unwrap(), format!("\"{s}\""));
        }
        assert_eq!(ReviewState::parse("later"), None);
        assert_eq!(ReviewState::default(), ReviewState::None);
    }
}

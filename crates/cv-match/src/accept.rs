//! Automatic acceptance and the data of a review-queue item.
//!
//! During automatic matching the top candidate is accepted only when its confidence is at or
//! above the threshold (default 0.75) and it is not tied with the second candidate. Anything
//! else is queued for a human with a reason. There is no special case for a single result: a
//! TV show with exactly one search hit still has to reach the threshold, and so does a movie.
//!
//! Ties are decided on the unclamped `raw_score` (see [`ScoredCandidate::ties_with`]), because
//! two clamped scores of 1.0 can still be separated by verified evidence.

use serde::{Deserialize, Serialize};

use crate::score::{Candidate, ScoreBreakdown, ScoredCandidate};

/// The default automatic-acceptance threshold.
pub const DEFAULT_THRESHOLD: f64 = 0.75;

/// What "confident" means on the Scrape page: a top candidate at or above this score that is not
/// tied with the second (spec `title-matching` "Confident means 90 percent and clear"). Fixed,
/// no setting.
pub const CONFIDENT: f64 = 0.90;

/// How many candidates a review item keeps (the review keys pick 1 to 5).
pub const MAX_REVIEW_CANDIDATES: usize = 5;

/// Scores are compared with this slack so a threshold of 0.75 accepts a score computed as
/// 0.7499999999.
const EPSILON: f64 = 1e-9;

/// The automatic-acceptance settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceptRules {
    threshold: f64,
}

impl AcceptRules {
    /// Rules with the given threshold, clamped to 0..=1. A value that is not a number gives the
    /// default.
    pub fn new(threshold: f64) -> Self {
        let threshold = if threshold.is_nan() {
            DEFAULT_THRESHOLD
        } else {
            threshold.clamp(0.0, 1.0)
        };
        Self { threshold }
    }

    /// The confidence a top candidate needs to be accepted without a human.
    pub fn threshold(&self) -> f64 {
        self.threshold
    }
}

impl Default for AcceptRules {
    fn default() -> Self {
        Self::new(DEFAULT_THRESHOLD)
    }
}

/// Why an item goes to the Needs review inbox.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ReviewReason {
    /// The search found nothing.
    NoCandidates,
    /// The best candidate is under the threshold.
    BelowThreshold { best: f64, threshold: f64 },
    /// The top candidates score the same, at or above the threshold. `count` is how many.
    Tie { count: usize },
    /// The file or folder name has no year, and without one the candidates could not be told
    /// apart: `tied` strong candidates (at least 2), or the best scored only `best` (`tied` 0).
    /// See [`ReviewReason::without_year`].
    NoYear { best: f64, tied: usize },
}

impl ReviewReason {
    /// The text shown in the inbox, for example `2 strong candidates`.
    pub fn text(&self) -> String {
        match self {
            Self::NoCandidates => "No candidates found".to_owned(),
            Self::BelowThreshold { best, threshold } => format!(
                "Best match {} is below {}",
                percent(*best),
                percent(*threshold)
            ),
            Self::Tie { count } => format!("{count} strong candidates"),
            Self::NoYear { tied, .. } if *tied > 1 => {
                format!("No year in the file name ({tied} strong candidates)")
            }
            Self::NoYear { best, .. } => {
                format!("No year in the file name (best match {})", percent(*best))
            }
        }
    }

    /// The reason to show when the search had no year: a match that was too weak or tied says
    /// that the year is missing, since a year is what usually tells such candidates apart. "No
    /// candidates" stays as it is. `best` is the score of the best candidate.
    #[must_use]
    pub fn without_year(self, best: f64) -> Self {
        match self {
            Self::BelowThreshold { .. } => Self::NoYear { best, tied: 0 },
            Self::Tie { count } => Self::NoYear { best, tied: count },
            other => other,
        }
    }
}

impl std::fmt::Display for ReviewReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text())
    }
}

fn percent(score: f64) -> String {
    format!("{}%", (score * 100.0).round())
}

/// The outcome of the acceptance rules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Decision {
    /// Accept the top candidate (index 0 of the ranked list).
    Accept { score: f64 },
    /// Queue the item for review.
    Review(ReviewReason),
}

/// Applies the acceptance rules to candidates ranked best first (see
/// [`rank_candidates`](crate::rank_candidates)).
///
/// Checks run in this order: no candidates; the best score under the threshold; a tie of the top
/// candidate with the second. A tie below the threshold is therefore reported as "below
/// threshold", because "strong candidates" would be wrong for it.
pub fn decide(ranked: &[ScoredCandidate], rules: &AcceptRules) -> Decision {
    let Some(top) = ranked.first() else {
        return Decision::Review(ReviewReason::NoCandidates);
    };
    let best = top.breakdown.score;
    if best + EPSILON < rules.threshold {
        return Decision::Review(ReviewReason::BelowThreshold {
            best,
            threshold: rules.threshold,
        });
    }
    // Ranking puts the better known of two equal scores first, so equal scores need not sit
    // next to each other.
    let tied = ranked.iter().skip(1).filter(|c| top.ties_with(c)).count();
    if tied > 0 {
        return Decision::Review(ReviewReason::Tie { count: tied + 1 });
    }
    Decision::Accept { score: best }
}

/// Whether `ranked` candidates (best first) have a confident top candidate: it scores
/// [`CONFIDENT`] or more and is not tied with another candidate. Accept all confident acts only
/// on such titles.
pub fn is_confident(ranked: &[ScoredCandidate]) -> bool {
    matches!(
        decide(ranked, &AcceptRules::new(CONFIDENT)),
        Decision::Accept { .. }
    )
}

/// One candidate as stored with a review item: the candidate and why it scored as it did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCandidate {
    #[serde(flatten)]
    pub candidate: Candidate,
    pub breakdown: ScoreBreakdown,
}

impl From<&ScoredCandidate> for ReviewCandidate {
    fn from(scored: &ScoredCandidate) -> Self {
        Self {
            candidate: scored.candidate.clone(),
            breakdown: scored.breakdown.clone(),
        }
    }
}

impl From<ReviewCandidate> for ScoredCandidate {
    fn from(stored: ReviewCandidate) -> Self {
        Self {
            candidate: stored.candidate,
            breakdown: stored.breakdown,
        }
    }
}

/// What the review queue stores for an item that was not auto-matched.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewDraft {
    pub reason: ReviewReason,
    /// Confidence of the best candidate, `None` when there was none.
    pub best_score: Option<f64>,
    /// The best candidates, best first, at most [`MAX_REVIEW_CANDIDATES`].
    pub candidates: Vec<ReviewCandidate>,
}

impl ReviewDraft {
    /// A draft from ranked candidates and the reason they were not accepted.
    pub fn new(reason: ReviewReason, ranked: &[ScoredCandidate]) -> Self {
        Self {
            reason,
            best_score: ranked.first().map(|c| c.breakdown.score),
            candidates: ranked
                .iter()
                .take(MAX_REVIEW_CANDIDATES)
                .map(ReviewCandidate::from)
                .collect(),
        }
    }

    /// The breakdown of the best candidate, the one the inbox row explains.
    pub fn top_breakdown(&self) -> Option<&ScoreBreakdown> {
        self.candidates.first().map(|c| &c.breakdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weak_or_tied_match_without_a_year_says_the_year_is_missing() {
        let below = ReviewReason::BelowThreshold {
            best: 0.62,
            threshold: 0.75,
        };
        assert_eq!(
            below.without_year(0.62).text(),
            "No year in the file name (best match 62%)"
        );
        assert_eq!(
            ReviewReason::Tie { count: 2 }.without_year(0.9).text(),
            "No year in the file name (2 strong candidates)"
        );
        assert_eq!(
            ReviewReason::NoCandidates.without_year(0.0),
            ReviewReason::NoCandidates
        );
    }
    use crate::query::normalize_query;
    use crate::score::{LocalEvidence, rank_candidates};

    fn cand(title: &str, year: u16) -> Candidate {
        Candidate {
            provider: "tmdb".to_owned(),
            title: title.to_owned(),
            year,
            has_poster: true,
            ..Candidate::default()
        }
    }

    /// A scored candidate with a fixed score, no evidence.
    fn fixed(title: &str, score: f64) -> ScoredCandidate {
        ScoredCandidate {
            candidate: cand(title, 2000),
            breakdown: ScoreBreakdown {
                similarity: score,
                similarity_source: crate::score::SimilaritySource::Title,
                year_penalty: 0.0,
                poster_penalty: 0.0,
                base: score,
                evidence: Vec::new(),
                raw_score: score,
                score: score.clamp(0.0, 1.0),
            },
        }
    }

    #[test]
    fn confident_match_is_accepted() {
        let ranked = [fixed("Dune: Part Two", 0.94), fixed("Dune", 0.41)];
        assert_eq!(
            decide(&ranked, &AcceptRules::default()),
            Decision::Accept { score: 0.94 }
        );
    }

    #[test]
    fn confident_needs_ninety_percent_and_a_clear_lead() {
        assert_eq!(CONFIDENT, 0.90);
        // 0.89 is not confident, 0.90 is.
        assert!(!is_confident(&[fixed("A", 0.89)]));
        assert!(is_confident(&[fixed("A", 0.90)]));
        // 0.94 against 0.41 is confident.
        assert!(is_confident(&[fixed("A", 0.94), fixed("B", 0.41)]));
        // A tie at 0.94 is not.
        assert!(!is_confident(&[fixed("A", 0.94), fixed("B", 0.94)]));
        assert!(!is_confident(&[]));
        // The automatic rule is looser: 0.80 is accepted there but not confident here.
        assert!(matches!(
            decide(&[fixed("A", 0.80)], &AcceptRules::default()),
            Decision::Accept { .. }
        ));
        assert!(!is_confident(&[fixed("A", 0.80)]));
    }

    #[test]
    fn threshold_boundary_is_inclusive() {
        let rules = AcceptRules::default();
        assert_eq!(rules.threshold(), 0.75);
        assert!(matches!(
            decide(&[fixed("A", 0.75)], &rules),
            Decision::Accept { .. }
        ));
        // Just under the threshold, beyond the rounding slack.
        assert!(matches!(
            decide(&[fixed("A", 0.7499)], &rules),
            Decision::Review(ReviewReason::BelowThreshold { .. })
        ));
        // Float noise at the boundary still accepts.
        assert!(matches!(
            decide(&[fixed("A", 0.75 - 1e-12)], &rules),
            Decision::Accept { .. }
        ));
    }

    #[test]
    fn a_custom_threshold_moves_the_boundary() {
        let ranked = [fixed("A", 0.8), fixed("B", 0.2)];
        assert!(matches!(
            decide(&ranked, &AcceptRules::new(0.8)),
            Decision::Accept { .. }
        ));
        assert!(matches!(
            decide(&ranked, &AcceptRules::new(0.85)),
            Decision::Review(ReviewReason::BelowThreshold { .. })
        ));
        assert_eq!(AcceptRules::new(7.0).threshold(), 1.0);
        assert_eq!(AcceptRules::new(-1.0).threshold(), 0.0);
        assert_eq!(AcceptRules::new(f64::NAN).threshold(), DEFAULT_THRESHOLD);
    }

    #[test]
    fn below_threshold_reason_names_both_numbers() {
        let reason = match decide(&[fixed("A", 0.62)], &AcceptRules::default()) {
            Decision::Review(r) => r,
            other => panic!("expected review, got {other:?}"),
        };
        assert_eq!(reason.text(), "Best match 62% is below 75%");
    }

    #[test]
    fn a_tie_goes_to_review_with_the_strong_candidates_reason() {
        let ranked = [
            fixed("Crash", 0.9),
            fixed("Crash", 0.9),
            fixed("Other", 0.3),
        ];
        let decision = decide(&ranked, &AcceptRules::default());
        assert_eq!(decision, Decision::Review(ReviewReason::Tie { count: 2 }));
        match decision {
            Decision::Review(r) => assert_eq!(r.text(), "2 strong candidates"),
            Decision::Accept { .. } => unreachable!(),
        }
    }

    #[test]
    fn a_three_way_tie_counts_every_tied_candidate() {
        let ranked = [
            fixed("A", 0.9),
            fixed("B", 0.9),
            fixed("C", 0.9),
            fixed("D", 0.1),
        ];
        assert_eq!(
            decide(&ranked, &AcceptRules::default()),
            Decision::Review(ReviewReason::Tie { count: 3 })
        );
    }

    #[test]
    fn a_narrow_lead_is_not_a_tie() {
        let ranked = [fixed("A", 0.91), fixed("B", 0.90)];
        assert!(matches!(
            decide(&ranked, &AcceptRules::default()),
            Decision::Accept { .. }
        ));
    }

    #[test]
    fn a_tie_below_the_threshold_reports_the_threshold() {
        let ranked = [fixed("A", 0.5), fixed("B", 0.5)];
        assert!(matches!(
            decide(&ranked, &AcceptRules::default()),
            Decision::Review(ReviewReason::BelowThreshold { .. })
        ));
    }

    #[test]
    fn clamped_scores_of_one_are_a_tie_only_when_the_raw_scores_match() {
        let mut a = fixed("A", 1.0);
        let mut b = fixed("B", 1.0);
        a.breakdown.raw_score = 1.08;
        b.breakdown.raw_score = 1.03;
        assert!(matches!(
            decide(&[a.clone(), b.clone()], &AcceptRules::default()),
            Decision::Accept { .. }
        ));
        b.breakdown.raw_score = 1.08;
        assert_eq!(
            decide(&[a, b], &AcceptRules::default()),
            Decision::Review(ReviewReason::Tie { count: 2 })
        );
    }

    #[test]
    fn no_candidates_go_to_review() {
        assert_eq!(
            decide(&[], &AcceptRules::default()),
            Decision::Review(ReviewReason::NoCandidates)
        );
        assert_eq!(ReviewReason::NoCandidates.text(), "No candidates found");
    }

    #[test]
    fn the_tv_rule_applies_to_a_single_result() {
        // One search hit is no reason to accept it: it must still reach the threshold.
        let weak = [fixed("Some Show", 0.6)];
        assert!(matches!(
            decide(&weak, &AcceptRules::default()),
            Decision::Review(ReviewReason::BelowThreshold { .. })
        ));
        let strong = [fixed("Some Show", 0.97)];
        assert!(matches!(
            decide(&strong, &AcceptRules::default()),
            Decision::Accept { .. }
        ));
    }

    #[test]
    fn identical_titles_from_real_scoring_tie_and_runtime_breaks_it() {
        let query = normalize_query("Solaris");
        let mut old = cand("Solaris", 1972);
        old.runtime_minutes = Some(167);
        let mut new = cand("Solaris", 1972);
        new.runtime_minutes = Some(99);

        let none = LocalEvidence::default();
        let ranked = rank_candidates(&query, vec![old.clone(), new.clone()], &none);
        assert_eq!(
            decide(&ranked, &AcceptRules::default()),
            Decision::Review(ReviewReason::Tie { count: 2 })
        );

        let evidence = LocalEvidence {
            runtime_minutes: Some(167),
            ..LocalEvidence::default()
        };
        let ranked = rank_candidates(&query, vec![old, new], &evidence);
        assert!(matches!(
            decide(&ranked, &AcceptRules::default()),
            Decision::Accept { .. }
        ));
    }

    #[test]
    fn a_draft_keeps_five_candidates_and_the_top_breakdown() {
        let ranked: Vec<_> = (0..8)
            .map(|i| fixed(&format!("T{i}"), 0.9 - f64::from(i) * 0.01))
            .collect();
        let draft = ReviewDraft::new(ReviewReason::Tie { count: 2 }, &ranked);
        assert_eq!(draft.candidates.len(), MAX_REVIEW_CANDIDATES);
        assert_eq!(draft.best_score, Some(0.9));
        assert_eq!(draft.top_breakdown().unwrap().score, 0.9);

        let empty = ReviewDraft::new(ReviewReason::NoCandidates, &[]);
        assert_eq!(empty.best_score, None);
        assert!(empty.top_breakdown().is_none());
    }

    #[test]
    fn a_stored_candidate_round_trips_through_json() {
        let mut scored = fixed("Solaris", 0.88);
        scored.candidate.ids.set("tmdb", "593");
        scored.candidate.poster_url = Some("https://img/p.jpg".to_owned());
        let stored = ReviewCandidate::from(&scored);
        let json = serde_json::to_value(&stored).unwrap();
        assert_eq!(json["title"], "Solaris");
        assert_eq!(json["provider"], "tmdb");
        assert_eq!(json["ids"]["tmdb"], "593");
        assert_eq!(json["breakdown"]["score"], 0.88);
        let back: ReviewCandidate = serde_json::from_value(json).unwrap();
        assert_eq!(ScoredCandidate::from(back), scored);
    }

    #[test]
    fn a_reason_serialises_with_a_code() {
        let json = serde_json::to_value(ReviewReason::Tie { count: 2 }).unwrap();
        assert_eq!(json["code"], "tie");
        assert_eq!(json["count"], 2);
    }
}

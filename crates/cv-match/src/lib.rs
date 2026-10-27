//! Title matching: confidence scoring, evidence and the review queue.
//!
//! - [`query`]: search input normalisation, ID shortcuts and the leading-year retry.
//! - [`similarity`]: Dice bigram and Jaro-Winkler string similarity.
//! - [`score`]: base scoring, local-evidence refinement and the stored score breakdown.
//! - [`accept`]: the automatic-acceptance rules (threshold, tie rejection) and the data of a review item.

pub mod accept;
pub mod query;
pub mod score;
pub mod similarity;

pub use accept::{
    AcceptRules, CONFIDENT, DEFAULT_THRESHOLD, Decision, MAX_REVIEW_CANDIDATES, ReviewCandidate,
    ReviewDraft, ReviewReason, decide, is_confident,
};
pub use query::{IdShortcut, MediaHint, NormalizedQuery, normalize_query, search_with_retry};
pub use score::{
    Candidate, EvidenceKind, EvidenceTerm, LocalEvidence, ScoreBreakdown, ScoredCandidate,
    SimilaritySource, Verdict, popularity_bonus, primary_audio_language, rank_candidates, rank_key,
    score_candidate, year_penalty,
};
pub use similarity::{dice_bigram, jaro_winkler, title_similarity};

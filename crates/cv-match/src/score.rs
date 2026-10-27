//! Candidate scoring with a stored, explainable breakdown.
//!
//! The base score is the best title similarity (query vs title or original title) minus a year
//! penalty and a missing-poster penalty. Local evidence then adds signed terms for runtime,
//! language and partial-ID agreement. Every term is kept in a [`ScoreBreakdown`] that is stored
//! with the review item and shown when the user hovers a confidence bar.
//!
//! The reported `score` is clamped to 0..=1. Ranking and tie checks use the unclamped
//! `raw_score`, so verified evidence still separates two candidates whose clamped scores are
//! both 1.0.

use std::cmp::Ordering;

use cv_domain::{AudioStream, ProviderIds, StreamFlag};
use serde::{Deserialize, Serialize};

use crate::query::NormalizedQuery;
use crate::similarity::title_similarity;

/// Year penalty when the candidate year is unknown or more than 100 years off.
const YEAR_PENALTY_FAR: f64 = 0.11;
/// Penalty for a candidate without a poster.
const POSTER_PENALTY: f64 = 0.01;
/// Search years at or below this are treated as "no year".
const MIN_SEARCH_YEAR: u16 = 1900;
/// Runtime agreement tolerance, relative to the local file.
const RUNTIME_TOLERANCE: f64 = 0.05;

/// Evidence adjustments (added to the score).
const RUNTIME_AGREE: f64 = 0.05;
const RUNTIME_CONFLICT: f64 = -0.08;
const LANGUAGE_AGREE: f64 = 0.03;
const LANGUAGE_CONFLICT: f64 = -0.03;
const ID_AGREE: f64 = 0.10;
const ID_CONFLICT: f64 = -0.10;

/// A search result to score. Built from a provider hit plus whatever details are known.
///
/// Stored with a review item, so it serialises (camelCase) and tolerates missing fields.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Candidate {
    /// Key of the provider that found it (`tmdb`, `tvdb`, ...). Accepting the candidate asks
    /// this provider for the details.
    pub provider: String,
    pub title: String,
    pub original_title: String,
    /// Release year, `0` when unknown.
    pub year: u16,
    pub has_poster: bool,
    /// Remote poster URL, kept so the review card can show it without another lookup.
    pub poster_url: Option<String>,
    /// Every ID the provider knows for it.
    pub ids: ProviderIds,
    /// Runtime in minutes, when known.
    pub runtime_minutes: Option<u32>,
    /// Original language (ISO 639 code), when known.
    pub original_language: Option<String>,
    /// Found by one of the item's own IDs: the requested title, scored 1.0.
    pub exact_id: bool,
    /// The provider's popularity figure (TMDB's `popularity`), `0.0` when unknown. Only ranking
    /// reads it, to put the likelier of two same-named titles first; the score ignores it.
    pub popularity: f32,
    /// How many votes the provider has for it, `0` when unknown. Used like `popularity`.
    pub votes: u32,
    /// The director, when the provider's details were read: with the runtime, what tells two
    /// candidates of one name apart in the review inbox. Not part of the score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub director: Option<String>,
}

/// What is known locally about the item being matched.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalEvidence {
    /// Runtime of the main video file, in minutes (from media info).
    pub runtime_minutes: Option<u32>,
    /// ISO 639 code of the primary audio stream (from media info).
    pub audio_language: Option<String>,
    /// IDs already known for the item (NFO, folder or file name).
    pub ids: ProviderIds,
    /// The year of the local title (from the file name or the NFO). It is the search year when the
    /// query has none, so a candidate years away from it is ranked lower and scored lower.
    pub year: Option<u16>,
}

impl LocalEvidence {
    /// Evidence from the item's main video: duration in seconds and its audio streams.
    pub fn from_media(duration_seconds: u32, audio: &[AudioStream], ids: ProviderIds) -> Self {
        Self {
            runtime_minutes: (duration_seconds > 0).then(|| duration_seconds.div_ceil(60)),
            audio_language: primary_audio_language(audio),
            ids,
            year: None,
        }
    }
}

/// The language of the primary audio stream: the default-flagged stream if it has a language,
/// else the first stream with one. Commentary tracks are skipped.
pub fn primary_audio_language(audio: &[AudioStream]) -> Option<String> {
    let usable = || {
        audio
            .iter()
            .filter(|s| !s.flags.contains(&StreamFlag::Comment) && !s.language.trim().is_empty())
    };
    usable()
        .find(|s| s.flags.contains(&StreamFlag::Default))
        .or_else(|| usable().next())
        .map(|s| s.language.trim().to_owned())
}

/// Which local evidence a term is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Runtime,
    Language,
    ProviderId,
}

/// Whether the candidate agrees with the local evidence (the review UI's tick or cross).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Agree,
    Conflict,
}

/// One evidence adjustment. Evidence that is unknown on either side produces no term.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceTerm {
    pub kind: EvidenceKind,
    pub verdict: Verdict,
    /// Signed amount added to the score.
    pub delta: f64,
    /// Short human text such as `167 min vs 167 min`.
    pub detail: String,
}

/// Which title the similarity came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimilaritySource {
    Title,
    OriginalTitle,
    /// Not compared: the candidate was found by ID.
    ExactId,
}

/// The stored explanation of a confidence score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreBreakdown {
    pub similarity: f64,
    pub similarity_source: SimilaritySource,
    pub year_penalty: f64,
    pub poster_penalty: f64,
    /// Similarity minus the year and poster penalties, before evidence.
    pub base: f64,
    pub evidence: Vec<EvidenceTerm>,
    /// Base plus evidence, not clamped.
    pub raw_score: f64,
    /// Final confidence, clamped to 0..=1.
    pub score: f64,
}

/// A candidate with its breakdown.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredCandidate {
    pub candidate: Candidate,
    pub breakdown: ScoreBreakdown,
}

impl ScoredCandidate {
    /// Whether two candidates score the same, using the unclamped scores.
    pub fn ties_with(&self, other: &ScoredCandidate) -> bool {
        (self.breakdown.raw_score - other.breakdown.raw_score).abs() < 1e-9
    }
}

/// A year this far or more from the known one is a conflict, not a rounding difference.
const YEAR_CONFLICT_FROM: u16 = 2;
/// The base penalty of a year conflict, before the distance term.
const YEAR_CONFLICT_PENALTY: f64 = 0.15;
/// The most a candidate with a year conflict can show as its confidence: under the automatic
/// acceptance threshold, so a person looks at it.
const YEAR_CONFLICT_CEILING: f64 = 0.74;

/// The year penalty: 0 without a usable search year or on an exact year; 0.11 for an unknown
/// candidate year or more than 100 years off; `0.01 + years_off / 1000` for a year one off (a
/// release-date rounding); `0.15 + years_off / 1000` (distance capped at 30) for a real
/// conflict, two or more years off.
pub fn year_penalty(search_year: Option<u16>, candidate_year: u16) -> f64 {
    let Some(search) = search_year.filter(|y| *y > MIN_SEARCH_YEAR) else {
        return 0.0;
    };
    if candidate_year == search {
        return 0.0;
    }
    if candidate_year == 0 {
        return YEAR_PENALTY_FAR;
    }
    let diff = search.abs_diff(candidate_year);
    if diff > 100 {
        YEAR_PENALTY_FAR
    } else if diff >= YEAR_CONFLICT_FROM {
        YEAR_CONFLICT_PENALTY + f64::from(diff.min(30)) / 1000.0
    } else {
        0.01 + f64::from(diff) / 1000.0
    }
}

/// Scores one candidate against the query and the local evidence.
pub fn score_candidate(
    query: &NormalizedQuery,
    candidate: &Candidate,
    evidence: &LocalEvidence,
) -> ScoreBreakdown {
    if candidate.exact_id {
        return ScoreBreakdown {
            similarity: 1.0,
            similarity_source: SimilaritySource::ExactId,
            year_penalty: 0.0,
            poster_penalty: 0.0,
            base: 1.0,
            evidence: Vec::new(),
            raw_score: 1.0,
            score: 1.0,
        };
    }

    let by_title = title_similarity(&query.text, &candidate.title);
    let by_original = if candidate.original_title.trim().is_empty() {
        0.0
    } else {
        title_similarity(&query.text, &candidate.original_title)
    };
    let (similarity, similarity_source) = if by_original > by_title {
        (by_original, SimilaritySource::OriginalTitle)
    } else {
        (by_title, SimilaritySource::Title)
    };
    // The typed year wins; without one the local title's year is the signal.
    let year_penalty = year_penalty(query.year.or(evidence.year), candidate.year);
    let poster_penalty = if candidate.has_poster {
        0.0
    } else {
        POSTER_PENALTY
    };
    let base = similarity - year_penalty - poster_penalty;

    let terms: Vec<EvidenceTerm> = [
        runtime_term(evidence.runtime_minutes, candidate.runtime_minutes),
        language_term(
            evidence.audio_language.as_deref(),
            candidate.original_language.as_deref(),
        ),
        id_term(&evidence.ids, &candidate.ids),
    ]
    .into_iter()
    .flatten()
    .collect();

    let raw_score = base + terms.iter().map(|t| t.delta).sum::<f64>();
    ScoreBreakdown {
        similarity,
        similarity_source,
        year_penalty,
        poster_penalty,
        base,
        evidence: terms,
        raw_score,
        // A candidate whose year is off is never a full match, however well the rest agrees, and
        // one whose year clearly conflicts stays under the automatic-acceptance threshold.
        score: if year_penalty >= YEAR_CONFLICT_PENALTY {
            raw_score.clamp(0.0, YEAR_CONFLICT_CEILING)
        } else if year_penalty > 0.0 {
            raw_score.clamp(0.0, YEAR_OFF_CEILING)
        } else {
            raw_score.clamp(0.0, 1.0)
        },
    }
}

/// The most a candidate with a year conflict can show as its confidence.
const YEAR_OFF_CEILING: f64 = 0.99;
/// The most the popularity of a title adds to its place in the ranking (not to its score). It is
/// worth more than a year one off and than the runtime evidence, so a classic outranks a
/// brand-new title of the same name, yet less than a real year conflict.
const POPULARITY_WEIGHT: f64 = 0.10;

/// How well known a candidate is, 0 to `POPULARITY_WEIGHT`: its vote count on a log scale (10 000
/// votes is the top). The provider's popularity figure is only used when there are no votes, and
/// at half weight: it follows the hype of a new release, which says little about which film is
/// meant.
pub fn popularity_bonus(candidate: &Candidate) -> f64 {
    let share = if candidate.votes > 0 {
        (f64::from(candidate.votes) + 1.0).ln() / 10_001f64.ln()
    } else {
        0.5 * (f64::from(candidate.popularity.max(0.0)) + 1.0).ln() / 101f64.ln()
    };
    POPULARITY_WEIGHT * share.clamp(0.0, 1.0)
}

/// The value candidates are ordered by: the unclamped score plus the popularity bonus. Higher
/// ranks first. The UI sorts merged results with it, so the order of the ranking survives.
pub fn rank_key(scored: &ScoredCandidate) -> f64 {
    scored.breakdown.raw_score + popularity_bonus(&scored.candidate) + exact_title_bonus(scored)
}

/// What a candidate whose title is exactly the searched text adds to its place in the ranking
/// (not to its score): "Memento" stays above "Memento Mori" when the folder's year points at the
/// longer title, since a wrong year in a folder name is common and a whole title is strong
/// evidence. Smaller than the year conflict, so an exact title of the right year still leads.
const EXACT_TITLE_RANK: f64 = 0.08;

fn exact_title_bonus(scored: &ScoredCandidate) -> f64 {
    let breakdown = &scored.breakdown;
    let exact = breakdown.similarity_source != SimilaritySource::ExactId
        && breakdown.similarity >= 1.0 - f64::EPSILON;
    if exact { EXACT_TITLE_RANK } else { 0.0 }
}

/// Scores and ranks candidates: score plus a small popularity bonus descending (so of two titles
/// with the same name the better known comes first), then year ascending, then input order. The
/// bonus orders candidates only: it is not part of the score, so two candidates that score the
/// same still tie for automatic matching.
pub fn rank_candidates(
    query: &NormalizedQuery,
    candidates: Vec<Candidate>,
    evidence: &LocalEvidence,
) -> Vec<ScoredCandidate> {
    let mut scored: Vec<ScoredCandidate> = candidates
        .into_iter()
        .map(|candidate| {
            let breakdown = score_candidate(query, &candidate, evidence);
            ScoredCandidate {
                candidate,
                breakdown,
            }
        })
        .collect();
    scored.sort_by(|a, b| {
        rank_key(b)
            .partial_cmp(&rank_key(a))
            .unwrap_or(Ordering::Equal)
            .then(a.candidate.year.cmp(&b.candidate.year))
    });
    scored
}

fn runtime_term(local: Option<u32>, candidate: Option<u32>) -> Option<EvidenceTerm> {
    let (local, candidate) = (local.filter(|m| *m > 0)?, candidate.filter(|m| *m > 0)?);
    let agrees = f64::from(local.abs_diff(candidate)) <= f64::from(local) * RUNTIME_TOLERANCE;
    let (verdict, delta) = if agrees {
        (Verdict::Agree, RUNTIME_AGREE)
    } else {
        (Verdict::Conflict, RUNTIME_CONFLICT)
    };
    Some(EvidenceTerm {
        kind: EvidenceKind::Runtime,
        verdict,
        delta,
        detail: format!("{candidate} min vs {local} min"),
    })
}

fn language_term(local: Option<&str>, candidate: Option<&str>) -> Option<EvidenceTerm> {
    let (local, candidate) = (
        local.filter(|l| !l.trim().is_empty())?,
        candidate.filter(|l| !l.trim().is_empty())?,
    );
    let agrees = canonical_language(local) == canonical_language(candidate);
    let (verdict, delta) = if agrees {
        (Verdict::Agree, LANGUAGE_AGREE)
    } else {
        (Verdict::Conflict, LANGUAGE_CONFLICT)
    };
    Some(EvidenceTerm {
        kind: EvidenceKind::Language,
        verdict,
        delta,
        detail: format!("{} vs {}", candidate.trim(), local.trim()),
    })
}

/// Compares every ID both sides know. Agreement needs a shared key with equal values and no
/// shared key with different values; the reverse is a conflict; a mix produces no term.
fn id_term(local: &ProviderIds, candidate: &ProviderIds) -> Option<EvidenceTerm> {
    let (mut agree, mut conflict) = (Vec::new(), Vec::new());
    for (key, value) in local.iter() {
        if let Some(other) = candidate.get(key) {
            if other.eq_ignore_ascii_case(value) {
                agree.push(key);
            } else {
                conflict.push(key);
            }
        }
    }
    match (agree.is_empty(), conflict.is_empty()) {
        (false, true) => Some(EvidenceTerm {
            kind: EvidenceKind::ProviderId,
            verdict: Verdict::Agree,
            delta: ID_AGREE,
            detail: agree.join(", "),
        }),
        (true, false) => Some(EvidenceTerm {
            kind: EvidenceKind::ProviderId,
            verdict: Verdict::Conflict,
            delta: ID_CONFLICT,
            detail: conflict.join(", "),
        }),
        _ => None,
    }
}

/// Language groups: ISO 639-1, 639-2/B, 639-2/T and the English name. The first entry is the
/// canonical key.
const LANGUAGES: &[&[&str]] = &[
    &["en", "eng", "english"],
    &["ru", "rus", "russian"],
    &["fr", "fre", "fra", "french"],
    &["de", "ger", "deu", "german"],
    &["es", "spa", "spanish"],
    &["it", "ita", "italian"],
    &["pt", "por", "portuguese"],
    &["ja", "jpn", "japanese"],
    &["ko", "kor", "korean"],
    &["zh", "chi", "zho", "chinese", "cmn", "yue"],
    &["hi", "hin", "hindi"],
    &["ar", "ara", "arabic"],
    &["tr", "tur", "turkish"],
    &["nl", "dut", "nld", "dutch"],
    &["sv", "swe", "swedish"],
    &["no", "nor", "nob", "nb", "norwegian"],
    &["da", "dan", "danish"],
    &["fi", "fin", "finnish"],
    &["pl", "pol", "polish"],
    &["cs", "cze", "ces", "czech"],
    &["sk", "slo", "slk", "slovak"],
    &["hu", "hun", "hungarian"],
    &["ro", "rum", "ron", "romanian"],
    &["el", "gre", "ell", "greek"],
    &["he", "heb", "hebrew"],
    &["uk", "ukr", "ukrainian"],
    &["bg", "bul", "bulgarian"],
    &["sr", "srp", "serbian"],
    &["hr", "hrv", "croatian"],
    &["th", "tha", "thai"],
    &["vi", "vie", "vietnamese"],
    &["id", "ind", "indonesian"],
    &["fa", "per", "fas", "persian"],
    &["az", "aze", "azerbaijani"],
    &["ka", "geo", "kat", "georgian"],
    &["hy", "arm", "hye", "armenian"],
    &["ca", "cat", "catalan"],
    &["is", "ice", "isl", "icelandic"],
    &["la", "lat", "latin"],
];

/// Reduces an ISO 639 code, a regional tag (`pt-BR`) or an English name to one key.
fn canonical_language(code: &str) -> String {
    let lower = code.trim().to_lowercase();
    let primary = lower.split(['-', '_']).next().unwrap_or(&lower);
    for group in LANGUAGES {
        if group.contains(&primary) || group.contains(&lower.as_str()) {
            return group[0].to_owned();
        }
    }
    primary.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::normalize_query;
    use cv_domain::id_key;

    fn cand(title: &str, year: u16) -> Candidate {
        Candidate {
            title: title.to_owned(),
            year,
            has_poster: true,
            ..Candidate::default()
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn known(title: &str, year: u16, votes: u32) -> Candidate {
        Candidate {
            votes,
            ..cand(title, year)
        }
    }

    /// The same-named titles a search for "Memento" returns: an obscure 1967 film comes first in
    /// the provider's order, the popular 2000 one later.
    fn memento() -> Vec<Candidate> {
        vec![
            known("Memento", 1967, 3),
            known("Memento", 2000, 15_000),
            known("Memento", 2002, 40),
            known("Memento", 2022, 12),
        ]
    }

    #[test]
    fn the_better_known_title_leads_when_the_search_has_no_year() {
        let q = normalize_query("Memento");
        let ranked = rank_candidates(&q, memento(), &LocalEvidence::default());
        assert_eq!(ranked[0].candidate.year, 2000);
        // Popularity orders them only: the scores still tie, so automatic matching asks a person.
        assert!(ranked[0].ties_with(&ranked[1]));
    }

    #[test]
    fn the_exact_title_outranks_a_longer_one_whose_year_agrees() {
        // The folder says 2020: the year of Memento Mori, not of Memento.
        let q = normalize_query("Memento");
        let ev = LocalEvidence {
            year: Some(2020),
            ..LocalEvidence::default()
        };
        let candidates = vec![
            known("Memento Mori", 2020, 30),
            known("Mementos", 2019, 5),
            known("Memento", 2000, 15_000),
        ];
        let ranked = rank_candidates(&q, candidates, &ev);
        assert_eq!(ranked[0].candidate.title, "Memento");
        // The ranking only: the year conflict still keeps it from being accepted by itself.
        assert!(ranked[0].breakdown.score < 0.75);
    }

    #[test]
    fn the_local_year_ranks_and_scores_candidates_by_distance() {
        let q = normalize_query("Memento");
        let ev = LocalEvidence {
            year: Some(2005),
            ..LocalEvidence::default()
        };
        let ranked = rank_candidates(&q, memento(), &ev);
        let years: Vec<u16> = ranked.iter().map(|c| c.candidate.year).collect();
        assert_eq!(years, vec![2000, 2002, 2022, 1967]);
        // A year conflict stays under the automatic-acceptance threshold, and the farther the
        // year the larger the penalty (the distance counts up to 30 years).
        for c in &ranked {
            assert!(c.breakdown.score < 0.75, "{}", c.candidate.year);
        }
        assert!(ranked[0].breakdown.year_penalty < ranked[3].breakdown.year_penalty);
        assert!(close(
            ranked[3].breakdown.year_penalty,
            0.15 + 30.0 / 1000.0
        ));
    }

    #[test]
    fn a_new_title_with_few_votes_never_beats_the_classic_of_the_same_name() {
        // The folder says 2020 (wrong), the search has no year: the 2025 release is the nearer
        // year, but it has 31 votes and the 2000 film has 15 000.
        let q = normalize_query("Memento");
        let ev = LocalEvidence {
            year: Some(2020),
            ..LocalEvidence::default()
        };
        let ranked = rank_candidates(
            &q,
            vec![
                known("Memento", 2025, 31),
                known("Memento", 2022, 12),
                known("Memento", 2000, 15_000),
            ],
            &ev,
        );
        assert_eq!(ranked[0].candidate.year, 2000);
        // Every candidate conflicts with the known year: none shows more than 74%.
        assert!(ranked.iter().all(|c| c.breakdown.score <= 0.74));
        assert!(rank_key(&ranked[0]) > rank_key(&ranked[1]));
    }

    #[test]
    fn popularity_without_votes_counts_for_less_than_votes() {
        let hyped = Candidate {
            popularity: 90.0,
            ..cand("Memento", 2025)
        };
        let classic = known("Memento", 2000, 15_000);
        assert!(popularity_bonus(&classic) > popularity_bonus(&hyped));
        assert!(popularity_bonus(&hyped) <= POPULARITY_WEIGHT / 2.0 + 1e-9);
    }

    #[test]
    fn a_typed_year_beats_the_local_one_and_an_exact_year_beats_popularity() {
        let q = normalize_query("Heat (2005)");
        let ev = LocalEvidence {
            year: Some(1995),
            ..LocalEvidence::default()
        };
        let ranked = rank_candidates(
            &q,
            vec![known("Heat", 1995, 9_000), known("Heat", 2005, 5)],
            &ev,
        );
        assert_eq!(ranked[0].candidate.year, 2005);
        assert!(close(ranked[0].breakdown.score, 1.0));
    }

    #[test]
    fn a_year_conflict_never_shows_one_hundred_percent() {
        let q = normalize_query("Crash (2004)");
        let ev = LocalEvidence {
            runtime_minutes: Some(100),
            audio_language: Some("en".to_owned()),
            ..LocalEvidence::default()
        };
        let mut c = cand("Crash", 2005);
        c.runtime_minutes = Some(100);
        c.original_language = Some("en".to_owned());
        let b = score_candidate(&q, &c, &ev);
        assert!(b.raw_score > 1.0 - 0.011 + 0.05);
        assert!(b.score <= 0.99);
    }

    #[test]
    fn year_penalty_table() {
        assert_eq!(year_penalty(None, 2004), 0.0);
        assert_eq!(year_penalty(Some(1900), 2004), 0.0);
        assert_eq!(year_penalty(Some(2004), 2004), 0.0);
        assert!(close(year_penalty(Some(2004), 2005), 0.011));
        assert!(close(year_penalty(Some(2004), 1999), 0.155));
        assert!(close(year_penalty(Some(2004), 1996), 0.158));
        assert_eq!(year_penalty(Some(2004), 0), 0.11);
        assert_eq!(year_penalty(Some(2004), 1900), 0.11);
        assert!(close(year_penalty(Some(2000), 1901), 0.18));
    }

    #[test]
    fn base_score_has_year_and_poster_penalties() {
        let q = normalize_query("Crash (2004)");
        let ev = LocalEvidence::default();
        let exact = score_candidate(&q, &cand("Crash", 2004), &ev);
        assert!(close(exact.score, 1.0));
        assert_eq!(exact.similarity_source, SimilaritySource::Title);

        let old = score_candidate(&q, &cand("Crash", 1996), &ev);
        assert!(close(old.year_penalty, 0.158));
        assert!(close(old.score, 0.74));

        let mut no_poster = cand("Crash", 2004);
        no_poster.has_poster = false;
        let b = score_candidate(&q, &no_poster, &ev);
        assert!(close(b.poster_penalty, 0.01));
        assert!(close(b.score, 0.99));
    }

    #[test]
    fn crash_year_ranks_2004_first() {
        let q = normalize_query("Crash (2004)");
        let ranked = rank_candidates(
            &q,
            vec![cand("Crash", 1996), cand("Crash", 2004)],
            &LocalEvidence::default(),
        );
        assert_eq!(ranked[0].candidate.year, 2004);
        assert!(ranked[0].breakdown.score > ranked[1].breakdown.score);
    }

    #[test]
    fn original_title_can_win() {
        let q = normalize_query("Solyaris");
        let mut c = cand("Solaris", 1972);
        c.original_title = "Solyaris".to_owned();
        let b = score_candidate(&q, &c, &LocalEvidence::default());
        assert_eq!(b.similarity_source, SimilaritySource::OriginalTitle);
        assert!(close(b.similarity, 1.0));
    }

    #[test]
    fn exact_id_scores_one() {
        let q = normalize_query("tt0069293");
        let mut c = cand("Solaris", 1972);
        c.exact_id = true;
        c.has_poster = false;
        let b = score_candidate(&q, &c, &LocalEvidence::default());
        assert_eq!(b.score, 1.0);
        assert_eq!(b.similarity_source, SimilaritySource::ExactId);
    }

    #[test]
    fn solaris_runtime_and_language_rank_1972_above_2002() {
        let q = normalize_query("Solaris");
        let audio = vec![AudioStream {
            language: "rus".to_owned(),
            ..AudioStream::default()
        }];
        let ev = LocalEvidence::from_media(167 * 60, &audio, ProviderIds::new());
        assert_eq!(ev.runtime_minutes, Some(167));
        assert_eq!(ev.audio_language.as_deref(), Some("rus"));

        let mut solaris_1972 = cand("Solaris", 1972);
        solaris_1972.runtime_minutes = Some(167);
        solaris_1972.original_language = Some("ru".to_owned());
        let mut solaris_2002 = cand("Solaris", 2002);
        solaris_2002.runtime_minutes = Some(99);
        solaris_2002.original_language = Some("en".to_owned());

        // Input order puts 2002 first, so the ranking must come from the evidence.
        let ranked = rank_candidates(&q, vec![solaris_2002, solaris_1972], &ev);
        assert_eq!(ranked[0].candidate.year, 1972);
        assert_eq!(ranked[1].candidate.year, 2002);
        let (top, other) = (&ranked[0].breakdown, &ranked[1].breakdown);
        assert!(top.raw_score > other.raw_score);
        assert!(top.score > other.score);
        assert!(!ranked[0].ties_with(&ranked[1]));
        assert_eq!(top.evidence.len(), 2);
        assert!(
            top.evidence
                .iter()
                .all(|t| t.verdict == Verdict::Agree && t.delta > 0.0)
        );
        assert_eq!(other.evidence.len(), 2);
        assert!(
            other
                .evidence
                .iter()
                .all(|t| t.verdict == Verdict::Conflict && t.delta < 0.0)
        );
    }

    #[test]
    fn solaris_ranks_1972_first_even_when_the_other_has_no_evidence() {
        let q = normalize_query("Solaris");
        let ev = LocalEvidence {
            runtime_minutes: Some(167),
            ..LocalEvidence::default()
        };
        let mut known = cand("Solaris", 1972);
        known.runtime_minutes = Some(167);
        let unknown = cand("Solaris", 2002);
        let ranked = rank_candidates(&q, vec![unknown, known], &ev);
        assert_eq!(ranked[0].candidate.year, 1972);
        assert!(!ranked[0].ties_with(&ranked[1]));
    }

    #[test]
    fn without_evidence_identical_titles_tie() {
        let q = normalize_query("Solaris");
        let ranked = rank_candidates(
            &q,
            vec![cand("Solaris", 2002), cand("Solaris", 1972)],
            &LocalEvidence::default(),
        );
        assert!(ranked[0].ties_with(&ranked[1]));
        // Ties fall back to the older year.
        assert_eq!(ranked[0].candidate.year, 1972);
    }

    #[test]
    fn runtime_tolerance_is_five_percent_of_the_file() {
        assert_eq!(
            runtime_term(Some(100), Some(105)).unwrap().verdict,
            Verdict::Agree
        );
        assert_eq!(
            runtime_term(Some(100), Some(95)).unwrap().verdict,
            Verdict::Agree
        );
        assert_eq!(
            runtime_term(Some(100), Some(106)).unwrap().verdict,
            Verdict::Conflict
        );
        assert!(runtime_term(None, Some(100)).is_none());
        assert!(runtime_term(Some(100), None).is_none());
        assert!(runtime_term(Some(0), Some(100)).is_none());
    }

    #[test]
    fn language_codes_are_equivalent_across_forms() {
        for (a, b) in [
            ("ru", "rus"),
            ("fre", "fr"),
            ("fra", "fre"),
            ("pt-BR", "por"),
            ("EN", "English"),
            ("zh", "chi"),
        ] {
            assert_eq!(
                language_term(Some(a), Some(b)).unwrap().verdict,
                Verdict::Agree
            );
        }
        assert_eq!(
            language_term(Some("rus"), Some("en")).unwrap().verdict,
            Verdict::Conflict
        );
        assert!(language_term(None, Some("en")).is_none());
        assert!(language_term(Some(" "), Some("en")).is_none());
        // Unknown codes still compare by equality.
        assert_eq!(
            language_term(Some("xx"), Some("xx")).unwrap().verdict,
            Verdict::Agree
        );
    }

    #[test]
    fn partial_id_agreement_and_conflict() {
        let local: ProviderIds = [(id_key::IMDB, "tt0069293")].into_iter().collect();
        let ev = LocalEvidence {
            ids: local,
            ..LocalEvidence::default()
        };
        let q = normalize_query("Solaris");

        let mut same = cand("Solaris", 1972);
        same.ids = [(id_key::IMDB, "tt0069293"), (id_key::TMDB, "593")]
            .into_iter()
            .collect();
        let b = score_candidate(&q, &same, &ev);
        assert_eq!(b.evidence[0].kind, EvidenceKind::ProviderId);
        assert_eq!(b.evidence[0].verdict, Verdict::Agree);
        assert_eq!(b.evidence[0].detail, "imdb");

        let mut other = cand("Solaris", 2002);
        other.ids = [(id_key::IMDB, "tt0307479")].into_iter().collect();
        let b = score_candidate(&q, &other, &ev);
        assert_eq!(b.evidence[0].verdict, Verdict::Conflict);

        // No shared provider: no term.
        let mut none = cand("Solaris", 2002);
        none.ids = [(id_key::TMDB, "593")].into_iter().collect();
        assert!(score_candidate(&q, &none, &ev).evidence.is_empty());
    }

    #[test]
    fn score_is_clamped_but_raw_is_kept() {
        let q = normalize_query("Solaris");
        let ev = LocalEvidence {
            runtime_minutes: Some(100),
            ..LocalEvidence::default()
        };
        let mut c = cand("Solaris", 1972);
        c.runtime_minutes = Some(100);
        let b = score_candidate(&q, &c, &ev);
        assert_eq!(b.score, 1.0);
        assert!(close(b.raw_score, 1.05));
    }

    #[test]
    fn breakdown_round_trips_through_json() {
        let q = normalize_query("Crash (2004)");
        let ev = LocalEvidence {
            runtime_minutes: Some(112),
            ..LocalEvidence::default()
        };
        let mut c = cand("Crash", 1996);
        c.runtime_minutes = Some(98);
        let b = score_candidate(&q, &c, &ev);
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"yearPenalty\""));
        assert!(json.contains("\"similaritySource\":\"title\""));
        let back: ScoreBreakdown = serde_json::from_str(&json).unwrap();
        assert_eq!(back, b);
    }

    #[test]
    fn primary_audio_language_prefers_default_and_skips_commentary() {
        let mk = |lang: &str, flags: &[StreamFlag]| AudioStream {
            language: lang.to_owned(),
            flags: flags.iter().copied().collect(),
            ..AudioStream::default()
        };
        assert_eq!(primary_audio_language(&[]), None);
        let streams = [
            mk("eng", &[StreamFlag::Comment, StreamFlag::Default]),
            mk("", &[]),
            mk("rus", &[]),
            mk("fra", &[StreamFlag::Default]),
        ];
        assert_eq!(primary_audio_language(&streams).as_deref(), Some("fra"));
        assert_eq!(
            primary_audio_language(&streams[..3]).as_deref(),
            Some("rus")
        );
    }
}

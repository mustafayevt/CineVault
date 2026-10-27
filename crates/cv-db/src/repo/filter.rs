//! Filter building blocks.
//!
//! A [`Filter`] is a small serialisable tree (so IPC can carry it) that compiles
//! to a SQL condition over the `item` table with bound parameters. Leaf filters map to indexed
//! columns or `EXISTS` sub-queries over the child tables; [`Filter::And`], [`Filter::Or`] and
//! [`Filter::Not`] combine them. No user text is ever spliced into the SQL.
//!
//! Conventions: text comparisons ignore case; an empty value list matches nothing; an empty
//! `And` matches everything and an empty `Or` nothing.

use cv_domain::{CreditKind, ItemId, ItemKind, RatingPreference};
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};

use super::codec::enum_text;
use super::facet::FacetCondition;
use super::fts;
use crate::error::DbResult;

/// An inclusive integer range; a missing bound is open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IntRange {
    pub min: Option<i64>,
    pub max: Option<i64>,
}

impl IntRange {
    pub fn between(min: i64, max: i64) -> Self {
        Self {
            min: Some(min),
            max: Some(max),
        }
    }

    pub fn at_least(min: i64) -> Self {
        Self {
            min: Some(min),
            max: None,
        }
    }

    pub fn at_most(max: i64) -> Self {
        Self {
            min: None,
            max: Some(max),
        }
    }
}

/// An inclusive range over ISO dates (`YYYY`, `YYYY-MM` or `YYYY-MM-DD` compare correctly as
/// text); a missing bound is open.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextRange {
    pub from: Option<String>,
    pub to: Option<String>,
}

/// How a multi-value filter combines its values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    /// At least one of the values.
    #[default]
    Any,
    /// Every one of the values.
    All,
}

/// A yes/no property of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemFlag {
    Watched,
    /// The NEW indicator set by a scan.
    New,
    Locked,
    Disc,
    Stacked,
    ThreeD,
    MultiMovieDir,
    /// Marked for review.
    NeedsReview,
}

impl ItemFlag {
    fn condition(self) -> &'static str {
        match self {
            Self::Watched => "item.watched = 1",
            Self::New => crate::repo::query::new_marker!(),
            Self::Locked => "item.locked = 1",
            Self::Disc => "item.is_disc = 1",
            Self::Stacked => "item.stacked = 1",
            Self::ThreeD => "item.video_in_3d = 1",
            Self::MultiMovieDir => "item.multi_movie_dir = 1",
            Self::NeedsReview => "item.review_state = 'needs_review'",
        }
    }
}

/// Which of an item's titles a [`Filter::TitleContains`] looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleColumn {
    Title,
    OriginalTitle,
    SortTitle,
}

impl TitleColumn {
    const fn column(self) -> &'static str {
        match self {
            Self::Title => "item.title",
            Self::OriginalTitle => "item.original_title",
            Self::SortTitle => "item.sort_title",
        }
    }
}

/// A condition on items.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Filter {
    /// Matches everything.
    #[default]
    All,
    /// Full-text search over titles, plot, people, tags and file names.
    Search {
        text: String,
    },
    /// Direct children of an item; `None` selects top-level items (no parent).
    Parent {
        parent: Option<ItemId>,
    },
    Year {
        range: IntRange,
    },
    RuntimeMinutes {
        range: IntRange,
    },
    DateAddedMs {
        range: IntRange,
    },
    /// Premiere date (movies) or first-aired date (shows and episodes).
    ReleaseDate {
        range: TextRange,
    },
    /// Rank in the top-250 list (`1` = best); unranked items never match.
    Top250 {
        range: IntRange,
    },
    DataSource {
        values: Vec<String>,
    },
    /// The item lives on a source with one of these labels (ignoring case): the filter the
    /// library offers as "Source".
    SourceLabel {
        values: Vec<String>,
    },
    Certification {
        values: Vec<String>,
    },
    /// The pipeline stage of the item: `new`, `scraped` or `renamed` (any of `stages`). A title
    /// is New until a provider has scraped it, Renamed once its names match the naming profile
    /// whose cache key is `profile_key`, and Scraped in between: the same rule as the lifecycle.
    Stage {
        stages: Vec<String>,
        profile_key: String,
    },
    /// The title carries any of the lifecycle `flags`, by the rules of the lifecycle and the
    /// pipeline counts (`cv-app::lifecycle`), so a count opens exactly the titles it counted:
    ///
    /// * `needs-review`: what Needs review counts: a match waiting for a decision (uncertain, or a
    ///   search that found nothing);
    /// * `not-matched`: a scrape was tried and found no confident match: the search found
    ///   nothing (it waits in the review queue), or no provider ID, no decision waiting and the
    ///   stored scrape outcome is a miss. A title no scrape was tried for has no flag (just New);
    /// * `rename-pending`: scraped, unlocked and a rename would change its names (a conflict too);
    /// * `missing`: its files or its library folder cannot be reached;
    /// * `locked`.
    ///
    /// `profile_key` selects the organize state of the active naming profile, as for
    /// [`Filter::Stage`]. An unknown flag matches nothing.
    Lifecycle {
        flags: Vec<String>,
        profile_key: String,
    },
    /// One of the item's titles contains any of `terms`, ignoring case (`title:heat`).
    TitleContains {
        column: TitleColumn,
        terms: Vec<String>,
    },
    MediaSource {
        values: Vec<String>,
    },
    Edition {
        values: Vec<String>,
    },
    Availability {
        values: Vec<String>,
    },
    ReviewState {
        values: Vec<String>,
    },
    Flag {
        flag: ItemFlag,
        value: bool,
    },
    Genre {
        names: Vec<String>,
        mode: MatchMode,
    },
    /// Credits a person by name, optionally only in one role (director, actor, ...).
    Person {
        name: String,
        kind: Option<CreditKind>,
    },
    /// The item has (or lacks) an ID from this provider.
    HasProviderId {
        provider: String,
        present: bool,
    },
    /// The item's ID from this provider equals `value`.
    ProviderId {
        provider: String,
        value: String,
    },
    /// A rating from `source`, converted to a 0-10 scale, within the range.
    Rating {
        source: String,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// The rating the item shows (the preferred source's, or the next available one), converted
    /// to a 0-10 scale, within the range. Items without any source rating never match.
    PreferredRating {
        preference: RatingPreference,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// Height in pixels of a video file.
    VideoHeight {
        range: IntRange,
    },
    VideoCodec {
        values: Vec<String>,
    },
    /// HDR formats of a video file (`Dolby Vision`, `HDR10`, ...).
    Hdr {
        values: Vec<String>,
        mode: MatchMode,
    },
    AudioLanguage {
        values: Vec<String>,
    },
    SubtitleLanguage {
        values: Vec<String>,
    },
    /// One entry of the filter catalogue, with its own negation,
    /// ANY/ALL mode and numeric operator. See [`super::facet`].
    Facet(FacetCondition),
    And {
        filters: Vec<Filter>,
    },
    Or {
        filters: Vec<Filter>,
    },
    Not {
        filter: Box<Filter>,
    },
}

impl Filter {
    /// Combines filters with AND.
    pub fn and(filters: impl IntoIterator<Item = Filter>) -> Self {
        Self::And {
            filters: filters.into_iter().collect(),
        }
    }

    /// Combines filters with OR.
    pub fn or(filters: impl IntoIterator<Item = Filter>) -> Self {
        Self::Or {
            filters: filters.into_iter().collect(),
        }
    }

    /// Negates a filter.
    #[allow(clippy::should_implement_trait)]
    pub fn not(filter: Filter) -> Self {
        Self::Not {
            filter: Box::new(filter),
        }
    }

    /// Compiles the filter to a SQL condition, appending to `out`.
    pub(crate) fn write(&self, out: &mut Sql) -> DbResult<()> {
        match self {
            Self::All => out.push("1"),
            Self::Search { text } => {
                // Chinese, Japanese and Korean are written without spaces, so the index holds a
                // whole title as one word and finds only its start. A word in those scripts also
                // matches anywhere in a title.
                let (spaceless, spaced): (Vec<&str>, Vec<&str>) = text
                    .split_whitespace()
                    .partition(|word| has_spaceless_script(word));
                let words = super::search::Words::parse(&spaced.join(" "));
                if words.is_empty() && spaceless.is_empty() {
                    // Nothing typed yet: do not restrict.
                    out.push("1");
                    return Ok(());
                }
                out.push("(");
                if words.is_empty() {
                    out.push("1");
                } else if spaceless.is_empty() {
                    super::search::write_condition(out, &words);
                } else {
                    // Mixed scripts: no typo tolerance, every word must be found.
                    super::search::write_exact_condition(out, &words);
                }
                for word in spaceless {
                    out.push(" AND (instr(item.title || ' ' || item.original_title || ' ' || ");
                    out.push("item.sort_title || ' ' || item.english_title, ");
                    out.bind(word.to_owned());
                    out.push(") > 0");
                    if let Some(expr) = fts::match_expression(word) {
                        out.push(
                            " OR item.id IN (SELECT rowid FROM item_fts WHERE item_fts MATCH ",
                        );
                        out.bind(expr);
                        out.push(")");
                    }
                    out.push(")");
                }
                out.push(")");
            }
            Self::Parent { parent } => match parent {
                Some(p) => {
                    out.push("item.parent_id = ");
                    out.bind(p.0);
                }
                None => out.push("item.parent_id IS NULL"),
            },
            Self::Year { range } => int_range(out, "item.year", range),
            Self::RuntimeMinutes { range } => int_range(out, "item.runtime_minutes", range),
            Self::DateAddedMs { range } => int_range(out, "item.date_added_ms", range),
            Self::ReleaseDate { range } => {
                let mut parts: Vec<(&str, &String)> = Vec::new();
                if let Some(from) = &range.from {
                    parts.push((">=", from));
                }
                if let Some(to) = &range.to {
                    parts.push(("<=", to));
                }
                if parts.is_empty() {
                    out.push("item.release_date IS NOT NULL");
                } else {
                    out.push("(");
                    for (i, (op, v)) in parts.into_iter().enumerate() {
                        if i > 0 {
                            out.push(" AND ");
                        }
                        out.push("item.release_date ");
                        out.push(op);
                        out.push(" ");
                        out.bind(v.clone());
                    }
                    out.push(")");
                }
            }
            Self::Top250 { range } => {
                out.push("(item.top250 > 0 AND ");
                int_range(out, "item.top250", range);
                out.push(")");
            }
            Self::DataSource { values } => in_list(out, "item.data_source", values),
            Self::SourceLabel { values } => in_list(out, super::query::SOURCE_LABEL_SQL, values),
            Self::Certification { values } => in_list(out, "item.certification", values),
            Self::Stage {
                stages,
                profile_key,
            } => stage_condition(out, stages, profile_key),
            Self::Lifecycle { flags, profile_key } => lifecycle_condition(out, flags, profile_key),
            Self::TitleContains { column, terms } => contains_any(out, column.column(), terms),
            Self::MediaSource { values } => in_list(out, "item.media_source", values),
            Self::Edition { values } => in_list(out, "item.edition", values),
            Self::Availability { values } => in_list(out, "item.availability", values),
            Self::ReviewState { values } => in_list(out, "item.review_state", values),
            Self::Flag { flag, value } => {
                if *value {
                    out.push(flag.condition());
                } else {
                    out.push("NOT (");
                    out.push(flag.condition());
                    out.push(")");
                }
            }
            Self::Genre { names, mode } => linked_names(
                out,
                names,
                *mode,
                "item_genre l JOIN genre n ON n.id = l.genre_id",
            ),
            Self::Person { name, kind } => {
                out.push(
                    "EXISTS (SELECT 1 FROM credit c JOIN person p ON p.id = c.person_id \
                     WHERE c.item_id = item.id AND p.name = ",
                );
                out.bind(name.trim().to_owned());
                out.push(" COLLATE NOCASE");
                if let Some(kind) = kind {
                    out.push(" AND c.kind = ");
                    out.bind(enum_text(kind)?);
                }
                out.push(")");
            }
            Self::HasProviderId { provider, present } => {
                out.push(if *present { "EXISTS" } else { "NOT EXISTS" });
                out.push(" (SELECT 1 FROM ext_id x WHERE x.item_id = item.id AND x.provider = ");
                out.bind(provider.clone());
                out.push(")");
            }
            Self::ProviderId { provider, value } => {
                out.push(
                    "EXISTS (SELECT 1 FROM ext_id x WHERE x.item_id = item.id AND x.provider = ",
                );
                out.bind(provider.clone());
                out.push(" AND x.value = ");
                out.bind(value.clone());
                out.push(")");
            }
            Self::Rating { source, min, max } => {
                out.push(
                    "EXISTS (SELECT 1 FROM rating r WHERE r.item_id = item.id AND r.source = ",
                );
                out.bind(source.clone());
                out.push(" AND r.max_value > 0");
                if let Some(min) = min {
                    out.push(" AND r.value * 10.0 / r.max_value >= ");
                    out.bind(*min);
                }
                if let Some(max) = max {
                    out.push(" AND r.value * 10.0 / r.max_value <= ");
                    out.bind(*max);
                }
                out.push(")");
            }
            Self::PreferredRating {
                preference,
                min,
                max,
            } => {
                out.push("EXISTS (SELECT 1 FROM (");
                out.push(&format!(
                    "SELECT r.value * 10.0 / r.max_value AS v FROM rating r                      WHERE r.item_id = item.id AND r.source <> 'user' AND r.max_value > 0                      ORDER BY {}, r.source LIMIT 1",
                    preference.rank_sql("r.source")
                ));
                out.push(") best WHERE 1");
                if let Some(min) = min {
                    out.push(" AND best.v >= ");
                    out.bind(*min);
                }
                if let Some(max) = max {
                    out.push(" AND best.v <= ");
                    out.bind(*max);
                }
                out.push(")");
            }
            Self::VideoHeight { range } => {
                out.push("EXISTS (SELECT 1 FROM media_file f WHERE f.item_id = item.id AND f.kind = 'video' AND ");
                int_range(out, "f.height", range);
                out.push(")");
            }
            Self::VideoCodec { values } => {
                if values.is_empty() {
                    out.push("0");
                } else {
                    out.push("EXISTS (SELECT 1 FROM media_file f WHERE f.item_id = item.id AND f.kind = 'video' AND ");
                    in_list(out, "f.video_codec", values);
                    out.push(")");
                }
            }
            Self::Hdr { values, mode } => hdr(out, values, *mode),
            Self::AudioLanguage { values } => stream_language(out, "audio", values),
            Self::SubtitleLanguage { values } => stream_language(out, "subtitle", values),
            Self::Facet(condition) => condition.write(out)?,
            Self::And { filters } => combine(out, filters, " AND ", "1")?,
            Self::Or { filters } => combine(out, filters, " OR ", "0")?,
            Self::Not { filter } => {
                out.push("NOT (");
                filter.write(out)?;
                out.push(")");
            }
        }
        Ok(())
    }
}

/// SQL text with its bound parameters, built left to right so `?` order matches `params`.
#[derive(Debug, Default)]
pub(crate) struct Sql {
    pub text: String,
    pub params: Vec<Value>,
    /// The kind of item the condition is written for. Catalogue filters mean different things
    /// for movies, shows and sets, so they need it; the query builders set it.
    pub kind: Option<ItemKind>,
}

impl Sql {
    pub(crate) fn push(&mut self, s: &str) {
        self.text.push_str(s);
    }

    pub(crate) fn bind(&mut self, v: impl Into<Value>) {
        self.text.push('?');
        self.params.push(v.into());
    }
}

fn combine(out: &mut Sql, filters: &[Filter], glue: &str, empty: &str) -> DbResult<()> {
    if filters.is_empty() {
        out.push(empty);
        return Ok(());
    }
    out.push("(");
    for (i, f) in filters.iter().enumerate() {
        if i > 0 {
            out.push(glue);
        }
        f.write(out)?;
    }
    out.push(")");
    Ok(())
}

fn int_range(out: &mut Sql, column: &str, range: &IntRange) {
    match (range.min, range.max) {
        (None, None) => {
            out.push(column);
            out.push(" IS NOT NULL");
        }
        (Some(min), None) => {
            out.push(column);
            out.push(" >= ");
            out.bind(min);
        }
        (None, Some(max)) => {
            out.push(column);
            out.push(" <= ");
            out.bind(max);
        }
        (Some(min), Some(max)) => {
            out.push(column);
            out.push(" BETWEEN ");
            out.bind(min);
            out.push(" AND ");
            out.bind(max);
        }
    }
}

/// `column IN (...)`, ignoring case; no values means no match.
fn in_list(out: &mut Sql, column: &str, values: &[String]) {
    if values.is_empty() {
        out.push("0");
        return;
    }
    out.push(column);
    out.push(" COLLATE NOCASE IN (");
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push(", ");
        }
        out.bind(v.clone());
    }
    out.push(")");
}

/// The stage of `item` is one of `stages` (`new`, `scraped`, `renamed`); an empty list matches
/// nothing. A title whose cached organize state was computed for another naming profile is not
/// yet known to be tidy, so it counts as Scraped, as the lifecycle reads it. An empty
/// `profile_key` stands for the profile of the newest computation, for callers that do not know
/// the active one.
/// The naming profile an organize-state condition reads: `profile_key`, or when it is empty the
/// profile of the newest computation.
fn push_profile(out: &mut Sql, profile_key: &str) {
    if profile_key.is_empty() {
        out.push(
            "(SELECT n.profile_key FROM item_organize_state n ORDER BY n.computed_ms DESC LIMIT 1)",
        );
    } else {
        out.bind(profile_key.to_owned());
    }
}

fn stage_condition(out: &mut Sql, stages: &[String], profile_key: &str) {
    if stages.is_empty() {
        out.push("0");
        return;
    }
    out.push("(CASE WHEN NOT ");
    out.push(&super::lifecycle::scraped_sql("item"));
    out.push(
        " THEN 'new' WHEN EXISTS (SELECT 1 FROM item_organize_state s          WHERE s.item_id = item.id AND s.state = 'tidy' AND s.profile_key = ",
    );
    push_profile(out, profile_key);
    out.push(") THEN 'renamed' ELSE 'scraped' END) IN (");
    for (i, stage) in stages.iter().enumerate() {
        if i > 0 {
            out.push(", ");
        }
        out.bind(stage.to_lowercase());
    }
    out.push(")");
}

const PENDING_REVIEW: &str =
    "EXISTS (SELECT 1 FROM review_queue r WHERE r.item_id = item.id AND r.state = 'pending'";
const HAS_PROVIDER_ID: &str =
    "EXISTS (SELECT 1 FROM ext_id e WHERE e.item_id = item.id AND e.provider <> 'tmdbSet')";

/// The organize state of the item for the profile is `state`.
fn organize_is(out: &mut Sql, state: &str, profile_key: &str) {
    out.push(
        "EXISTS (SELECT 1 FROM item_organize_state s WHERE s.item_id = item.id AND s.state = '",
    );
    out.push(state);
    out.push("' AND s.profile_key = ");
    push_profile(out, profile_key);
    out.push(")");
}

/// One lifecycle flag as a condition; the rules of `cv-app::lifecycle::flags_of`.
fn lifecycle_flag(out: &mut Sql, flag: &str, profile_key: &str) {
    let scraped = super::lifecycle::scraped_sql("item");
    match flag {
        "needs-review" => {
            out.push("(");
            out.push(PENDING_REVIEW);
            out.push(")");
            out.push(")");
        }
        "not-matched" => {
            // The search found nothing and waits in the review queue, or the last scrape found
            // no confident match and nothing waits. A title no scrape was tried for is not.
            out.push("(");
            out.push(PENDING_REVIEW);
            out.push(" AND r.best_score IS NULL) OR (NOT ");
            out.push(PENDING_REVIEW);
            out.push(") AND NOT ");
            out.push(HAS_PROVIDER_ID);
            out.push(" AND ");
            out.push(&super::lifecycle::scrape_missed_sql("item"));
            out.push("))");
        }
        "rename-pending" => {
            out.push("(item.locked = 0 AND ");
            out.push(&scraped);
            out.push(" AND (");
            organize_is(out, "pending", profile_key);
            out.push(" OR ");
            organize_is(out, "conflict", profile_key);
            out.push("))");
        }
        "missing" => out.push("item.availability IN ('missing', 'offline')"),
        "locked" => out.push("item.locked = 1"),
        _ => out.push("0"),
    }
}

fn lifecycle_condition(out: &mut Sql, flags: &[String], profile_key: &str) {
    if flags.is_empty() {
        out.push("0");
        return;
    }
    out.push("(");
    for (i, flag) in flags.iter().enumerate() {
        if i > 0 {
            out.push(" OR ");
        }
        out.push("(");
        lifecycle_flag(out, &flag.to_lowercase(), profile_key);
        out.push(")");
    }
    out.push(")");
}

/// `column` contains any of `terms` (ignoring case); no term matches nothing.
fn contains_any(out: &mut Sql, column: &str, terms: &[String]) {
    let terms: Vec<&str> = terms
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty())
        .collect();
    if terms.is_empty() {
        out.push("0");
        return;
    }
    out.push("(");
    for (i, term) in terms.iter().enumerate() {
        if i > 0 {
            out.push(" OR ");
        }
        out.push(column);
        out.push(" LIKE ");
        let mut pattern = String::from("%");
        for ch in term.chars() {
            if matches!(ch, '%' | '_' | '\\') {
                pattern.push('\\');
            }
            pattern.push(ch);
        }
        pattern.push('%');
        out.bind(pattern);
        out.push(" ESCAPE '\\'");
    }
    out.push(")");
}

pub(super) fn distinct_lowercase(values: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    values
        .iter()
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty() && seen.insert(v.clone()))
        .collect()
}

/// Genre/tag membership through a link table aliased `l` joined to the name table `n`.
fn linked_names(out: &mut Sql, names: &[String], mode: MatchMode, from: &str) {
    let names = distinct_lowercase(names);
    if names.is_empty() {
        out.push("0");
        return;
    }
    let placeholders = |out: &mut Sql| {
        for (i, n) in names.iter().enumerate() {
            if i > 0 {
                out.push(", ");
            }
            out.bind(n.clone());
        }
    };
    match mode {
        MatchMode::Any => {
            out.push("EXISTS (SELECT 1 FROM ");
            out.push(from);
            out.push(" WHERE l.item_id = item.id AND n.name COLLATE NOCASE IN (");
            placeholders(out);
            out.push("))");
        }
        MatchMode::All => {
            out.push("(SELECT count(*) FROM ");
            out.push(from);
            out.push(" WHERE l.item_id = item.id AND n.name COLLATE NOCASE IN (");
            placeholders(out);
            out.push(&format!(")) = {}", names.len()));
        }
    }
}

fn hdr(out: &mut Sql, values: &[String], mode: MatchMode) {
    let values = distinct_lowercase(values);
    if values.is_empty() {
        out.push("0");
        return;
    }
    let one = |out: &mut Sql, v: &str| {
        out.push("EXISTS (SELECT 1 FROM media_file f, json_each(f.hdr_formats) h WHERE f.item_id = item.id AND f.kind = 'video' AND lower(h.value) = ");
        out.bind(v.to_owned());
        out.push(")");
    };
    out.push("(");
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push(match mode {
                MatchMode::Any => " OR ",
                MatchMode::All => " AND ",
            });
        }
        one(out, v);
    }
    out.push(")");
}

fn stream_language(out: &mut Sql, kind: &str, values: &[String]) {
    if values.is_empty() {
        out.push("0");
        return;
    }
    out.push("EXISTS (SELECT 1 FROM stream s JOIN media_file f ON f.id = s.media_file_id WHERE f.item_id = item.id AND s.kind = '");
    out.push(kind);
    out.push("' AND ");
    in_list(out, "s.language", values);
    out.push(")");
}

/// The word is in a script written without spaces between words (Han, Hiragana, Katakana,
/// Hangul), where the full-text index cannot find a word inside a title.
fn has_spaceless_script(word: &str) -> bool {
    word.chars().any(|c| {
        matches!(c as u32,
            0x3040..=0x30FF       // Hiragana, Katakana
            | 0x3400..=0x4DBF     // CJK extension A
            | 0x4E00..=0x9FFF     // CJK unified ideographs
            | 0xAC00..=0xD7AF     // Hangul syllables
            | 0xF900..=0xFAFF     // CJK compatibility ideographs
            | 0x20000..=0x2FA1F   // CJK extensions B and later
        )
    })
}

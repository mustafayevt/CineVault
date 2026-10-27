//! The reads behind the title lifecycle: the facts of a list of items in a few queries, and the
//! pipeline counts of a library type in one grouped pass.
//!
//! The rules that turn these facts into a stage and flags live in `cv-app::lifecycle`; this
//! module only reads. "Scraped" is the SQL expression [`scraped_sql`]: the item has a provider
//! ID (not just a collection ID) and its details came from a provider or from an NFO file.

use std::collections::{BTreeMap, BTreeSet};

use cv_domain::{Availability, ItemId, ItemKind};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use super::codec::{enum_text, parse_enum};
use super::organize_state::OrganizeStateKind;
use super::step_outcome::StepStatus;
use crate::error::{DbError, DbResult};

/// The SQL for "this item is scraped", over the table alias `alias`: a provider ID other than the
/// collection ID, and either a scraper that wrote the details or an NFO file.
#[must_use]
pub fn scraped_sql(alias: &str) -> String {
    format!(
        "(EXISTS (SELECT 1 FROM ext_id e WHERE e.item_id = {alias}.id AND e.provider <> 'tmdbSet') \
         AND ({alias}.last_scraper_id <> '' \
              OR EXISTS (SELECT 1 FROM media_file m WHERE m.item_id = {alias}.id AND m.kind = 'nfo')))"
    )
}

/// The SQL for "the last scrape of this item found no confident match", over the table alias
/// `alias`: its stored scrape outcome (`item_step_outcome`, step `scrape`) is a miss (see
/// `StepStatus::is_scrape_miss`). A title without that row has never had a scrape tried.
#[must_use]
pub fn scrape_missed_sql(alias: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM item_step_outcome o WHERE o.item_id = {alias}.id \
         AND o.step = 'scrape' AND o.status IN ('not_found', 'needs_key', 'failed'))"
    )
}

/// Which of `ids` are scraped (see [`scraped_sql`]). An id the library does not know is not.
///
/// # Errors
/// The database fails.
pub fn scraped_items(conn: &Connection, ids: &[ItemId]) -> DbResult<BTreeSet<ItemId>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {} FROM item i WHERE i.id = ?1",
        scraped_sql("i")
    ))?;
    let mut out = BTreeSet::new();
    for id in ids {
        let scraped: Option<bool> = stmt.query_row([id.0], |r| r.get(0)).optional()?;
        if scraped == Some(true) {
            out.insert(*id);
        }
    }
    Ok(out)
}

/// A match waiting for a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReview {
    /// The text the inbox shows.
    pub reason: String,
    /// The search found nothing: the title could not be matched.
    pub no_candidates: bool,
}

/// What the lifecycle reads about one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleRow {
    pub id: ItemId,
    pub kind: ItemKind,
    pub parent_id: Option<ItemId>,
    pub title: String,
    pub year: u16,
    pub locked: bool,
    pub availability: Availability,
    pub last_scraper_id: String,
    pub date_added_ms: i64,
    pub updated_ms: i64,
    /// Every provider ID, by provider key.
    pub ids: BTreeMap<String, String>,
    /// An NFO file is listed for the item.
    pub has_nfo: bool,
    pub review: Option<PendingReview>,
    /// The item's folder, as stored.
    pub path: String,
    /// The root of its library folder, as stored.
    pub data_source: String,
    /// The label of that library folder; empty when the folder is not a known one.
    pub source_label: String,
}

/// The lifecycle facts of `ids`; an id the library does not know is absent.
pub fn lifecycle_rows(
    conn: &Connection,
    ids: &[ItemId],
) -> DbResult<BTreeMap<ItemId, LifecycleRow>> {
    let mut out = BTreeMap::new();
    for chunk in ids.chunks(400) {
        let marks = vec!["?"; chunk.len()].join(",");
        let binds = || params_from_iter(chunk.iter().map(|id| id.0));

        let mut stmt = conn.prepare(&format!(
            "SELECT i.id, i.kind, i.parent_id, i.title, i.year, i.locked, i.availability,
                    i.last_scraper_id, i.date_added_ms, i.updated_ms,
                    EXISTS (SELECT 1 FROM media_file m WHERE m.item_id = i.id AND m.kind = 'nfo'),
                    r.reason, r.best_score IS NULL, i.path, i.data_source,
                    COALESCE((SELECT d.label FROM data_source d
                              WHERE d.path = i.data_source COLLATE NOCASE
                                AND d.kind = CASE WHEN i.kind IN ('movie', 'set') THEN 'movie'
                                                  ELSE 'tv' END
                              LIMIT 1), '')
             FROM item i
             LEFT JOIN review_queue r ON r.item_id = i.id AND r.state = 'pending'
             WHERE i.id IN ({marks})"
        ))?;
        let rows = stmt.query_map(binds(), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, i64>(9)?,
                r.get::<_, bool>(10)?,
                r.get::<_, Option<String>>(11)?,
                r.get::<_, bool>(12)?,
                r.get::<_, String>(13)?,
                r.get::<_, String>(14)?,
                r.get::<_, String>(15)?,
            ))
        })?;
        for row in rows {
            let (
                id,
                kind,
                parent,
                title,
                year,
                locked,
                avail,
                scraper,
                added,
                updated,
                nfo,
                reason,
                none,
                path,
                data_source,
                source_label,
            ) = row?;
            let availability = Availability::parse(&avail)
                .ok_or_else(|| DbError::InvalidData(format!("unknown availability `{avail}`")))?;
            out.insert(
                ItemId(id),
                LifecycleRow {
                    id: ItemId(id),
                    kind: parse_enum(&kind, "item kind")?,
                    parent_id: parent.map(ItemId),
                    title,
                    year: u16::try_from(year).unwrap_or(0),
                    locked,
                    availability,
                    last_scraper_id: scraper,
                    date_added_ms: added,
                    updated_ms: updated,
                    ids: BTreeMap::new(),
                    has_nfo: nfo,
                    review: reason.map(|reason| PendingReview {
                        reason,
                        no_candidates: none,
                    }),
                    path,
                    data_source,
                    source_label,
                },
            );
        }

        let mut stmt = conn.prepare(&format!(
            "SELECT item_id, provider, value FROM ext_id WHERE item_id IN ({marks})"
        ))?;
        let rows = stmt.query_map(binds(), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, provider, value) = row?;
            if let Some(found) = out.get_mut(&ItemId(id)) {
                found.ids.insert(provider, value);
            }
        }
    }
    Ok(out)
}

/// Whether a pending review is an uncertain match or a title that could not be matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewGroup {
    None,
    /// Candidates exist but matching could not decide.
    Uncertain,
    /// The search found nothing.
    NotMatched,
}

/// A group of titles that share every fact the pipeline counts depend on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleGroup {
    /// The titles have a provider ID (not just a collection ID).
    pub has_id: bool,
    pub scraped: bool,
    /// The cached organize state for the profile; `None` while it is being computed.
    pub organize: Option<OrganizeStateKind>,
    pub review: ReviewGroup,
    /// The reason of a title that could not be matched; empty otherwise.
    pub not_matched_reason: String,
    /// The last scrape of the titles found no confident match: how it ended and the reason it
    /// stored. `None` when no scrape was tried, or the last one matched.
    pub scrape_miss: Option<(StepStatus, String)>,
    pub locked: bool,
    /// The file or the folder is missing or offline.
    pub missing: bool,
    pub count: u32,
}

/// The titles of `kind` (movies or shows) grouped by everything the counts need, in one pass.
/// `profile_key` selects the organize state rows of the active naming profile.
pub fn lifecycle_groups(
    conn: &Connection,
    kind: ItemKind,
    profile_key: &str,
) -> DbResult<Vec<LifecycleGroup>> {
    let kind = enum_text(&kind)?;
    let scraped = scraped_sql("i");
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT EXISTS (SELECT 1 FROM ext_id e WHERE e.item_id = i.id AND e.provider <> 'tmdbSet'),
                {scraped} AS scraped,
                s.state,
                CASE WHEN r.id IS NULL THEN 0 WHEN r.best_score IS NULL THEN 2 ELSE 1 END AS review,
                CASE WHEN r.id IS NOT NULL AND r.best_score IS NULL THEN r.reason ELSE '' END,
                i.locked,
                i.availability IN ('missing', 'offline'),
                o.status,
                o.reason,
                COUNT(*)
         FROM item i
         LEFT JOIN item_organize_state s ON s.item_id = i.id AND s.profile_key = ?2
         LEFT JOIN review_queue r ON r.item_id = i.id AND r.state = 'pending'
         LEFT JOIN item_step_outcome o ON o.item_id = i.id AND o.step = 'scrape'
                                      AND o.status IN ('not_found', 'needs_key', 'failed')
         WHERE i.kind = ?1 AND i.trashed_entry IS NULL
         GROUP BY 1, 2, 3, 4, 5, 6, 7, 8, 9"
    ))?;
    let rows = stmt.query_map(params![kind, profile_key], |r| {
        Ok((
            (r.get::<_, bool>(0)?, r.get::<_, bool>(1)?),
            r.get::<_, Option<String>>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, bool>(5)?,
            r.get::<_, bool>(6)?,
            (
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, i64>(9)?,
            ),
        ))
    })?;
    let mut groups = Vec::new();
    for row in rows {
        let ((has_id, scraped), organize, review, reason, locked, missing, (status, why, count)) =
            row?;
        let scrape_miss = status
            .as_deref()
            .and_then(StepStatus::parse)
            .map(|status| (status, why.unwrap_or_default()));
        groups.push(LifecycleGroup {
            has_id,
            scraped,
            organize: organize.as_deref().and_then(OrganizeStateKind::parse),
            review: match review {
                1 => ReviewGroup::Uncertain,
                2 => ReviewGroup::NotMatched,
                _ => ReviewGroup::None,
            },
            not_matched_reason: reason,
            scrape_miss,
            locked,
            missing,
            count: u32::try_from(count).unwrap_or(u32::MAX),
        });
    }
    Ok(groups)
}

//! The missing/duplicate review (spec `library-scanning`).
//!
//! Lists the titles that need a decision, each with its reason, size and last-seen date:
//!
//! * **Missing**: the folder or every video of the title is gone from a reachable source.
//! * **Offline**: the title's data source cannot be reached (an unplugged drive).
//! * **Duplicate**: another row holds the same video path, or the same show folder.
//! * **Superseded**: a rescan read the show's folder as part of another show and set the row
//!   aside.
//!
//! The user can remove chosen rows (with Undo), relocate a missing or offline row to the folder
//! its files moved to, or keep a row. Nothing is removed by this module on its own: listing only
//! reads, and a restart or a rescan changes no row that is listed here.
//!
//! Removal keeps every file on disk (it is [`RemovalService::remove_from_library`] without an
//! ignore marker). Undo puts the removed rows back from a snapshot kept in memory for the session:
//! the title, its seasons and episodes with their files, ratings, tags, credits, flags and locks.
//! The rows get new ids; scan history of the removed files is not restored, which only costs a
//! re-read of those files at the next scan.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use cv_db::Database;
use cv_db::repo::data_sources::list_sources;
use cv_db::repo::library_ops::{lies_under, tree_ids};
use cv_db::repo::library_review::{self, ReviewCandidate};
use cv_db::repo::rename_journal::owner_of_folder;
use cv_db::repo::{
    AvailabilityChange, PathStyle, StdFileProbe, apply_availability, field_locks, get_availability,
    get_flags, get_item, insert_item, plan_availability, set_availability, set_field_locks,
    set_new, set_review_state,
};
use cv_domain::{Availability, Item, ItemId, ItemKind, ReviewState, SourceKind};
use cv_scan::rescan::reasons::{self, Superseded};

use super::removal::{RemovalService, RemoveReport};
use super::{LibraryOpsError, TitleKind, now_millis, unique};

/// What is wrong with a listed title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewReason {
    /// Its files are gone from a reachable data source.
    Missing,
    /// Its data source cannot be reached.
    Offline,
    /// Another row holds the same video path or show folder.
    Duplicate,
    /// A rescan read its folder as part of another show.
    Superseded,
}

impl ReviewReason {
    /// The stable word stored with a "keep" and sent to the UI.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Offline => "offline",
            Self::Duplicate => "duplicate",
            Self::Superseded => "superseded",
        }
    }
}

/// The row a duplicate duplicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateOf {
    pub id: ItemId,
    pub title: String,
    pub year: u16,
}

/// One row of the review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRow {
    pub id: ItemId,
    pub kind: TitleKind,
    pub title: String,
    pub year: u16,
    /// The data source the title is in.
    pub source: String,
    pub folder: String,
    pub reason: ReviewReason,
    /// One plain sentence: why the row is listed.
    pub detail: String,
    /// The row this one duplicates (for `Duplicate`, and for a set-aside row that names one).
    pub duplicate_of: Option<DuplicateOf>,
    /// Distinct files the library recorded for the title.
    pub files: u32,
    pub bytes: u64,
    /// When a scan last saw the title's files, else when the row last changed (epoch ms).
    pub last_seen_ms: i64,
    /// A missing or offline title with a folder can be pointed at the folder its files moved to.
    pub can_relocate: bool,
}

/// The review list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewList {
    pub rows: Vec<ReviewRow>,
    /// Rows the user chose to keep that are left out of `rows`.
    pub kept: u32,
}

/// The answer of "remove from the review".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewRemoval {
    pub report: RemoveReport,
    /// Pass to [`ReviewService::undo_removal`] to put the removed rows back.
    pub undo_token: Option<String>,
}

/// A removed title put back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredTitle {
    /// The id the title had.
    pub was: ItemId,
    /// The id it has now.
    pub id: ItemId,
    pub title: String,
}

/// The answer of "relocate".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relocated {
    pub id: ItemId,
    pub from: String,
    pub to: String,
    /// The availability after the move, so the row can leave the list at once.
    pub availability: Availability,
}

struct Snapshot {
    item: Item,
    availability: Availability,
    reason: String,
    is_new: bool,
    review_state: ReviewState,
    locks: cv_domain::FieldSet,
}

struct Undo {
    token: String,
    /// Per removed title, its tree parent first.
    trees: Vec<Vec<Snapshot>>,
}

/// How many removals can be undone in one session.
const UNDO_DEPTH: usize = 20;

/// Finds the rows that need a decision and carries out the decisions.
pub struct ReviewService {
    db: Arc<Database>,
    removal: RemovalService,
    undo: Mutex<VecDeque<Undo>>,
}

impl ReviewService {
    #[must_use]
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            removal: RemovalService::new(Arc::clone(&db)),
            db,
            undo: Mutex::new(VecDeque::new()),
        }
    }

    /// The rows that need a decision, oldest first. Reads only.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn list(&self) -> Result<ReviewList, LibraryOpsError> {
        let candidates = self.db.read(library_review::review_candidates)?;
        let titles: BTreeMap<ItemId, (String, u16)> = candidates
            .iter()
            .map(|c| (c.id, (c.title.clone(), c.year)))
            .collect();
        let mut list = ReviewList::default();
        for candidate in candidates {
            let reason = classify(&candidate);
            if candidate.kept_reason.as_deref() == Some(reason.as_str()) {
                list.kept += 1;
                continue;
            }
            let original = match candidate.duplicate_of {
                Some(id) => Some(self.original(id, &titles)?),
                None => None,
            };
            list.rows.push(row_of(candidate, reason, original));
        }
        Ok(list)
    }

    fn original(
        &self,
        id: ItemId,
        known: &BTreeMap<ItemId, (String, u16)>,
    ) -> Result<DuplicateOf, LibraryOpsError> {
        if let Some((title, year)) = known.get(&id) {
            return Ok(DuplicateOf {
                id,
                title: title.clone(),
                year: *year,
            });
        }
        let found = self.db.read(move |conn| {
            Ok(get_item(conn, id)?.map(|item| (item.core.title, item.core.year)))
        })?;
        let (title, year) = found.unwrap_or_default();
        Ok(DuplicateOf { id, title, year })
    }

    /// Takes the chosen rows out of the library; their files stay on disk. Only rows that are
    /// listed can be removed: a title that is online and not a duplicate is refused. Returns a
    /// token that [`Self::undo_removal`] takes back.
    ///
    /// # Errors
    /// The library cannot be read or written.
    pub fn remove(&self, ids: &[ItemId]) -> Result<ReviewRemoval, LibraryOpsError> {
        let ids = unique(ids);
        let listed: BTreeMap<ItemId, ReviewReason> = self
            .db
            .read(library_review::review_candidates)?
            .iter()
            .map(|c| (c.id, classify(c)))
            .collect();
        let (allowed, refused): (Vec<ItemId>, Vec<ItemId>) =
            ids.iter().partition(|id| listed.contains_key(id));
        let mut snapshots = Vec::new();
        for id in &allowed {
            snapshots.push(self.snapshot(*id)?);
        }
        let mut report = self.removal.remove_from_library(&allowed, false)?;
        for id in refused {
            report.problems.push(super::removal::Problem {
                id,
                title: String::new(),
                reason: "This title is not in the review, so it was not removed.".to_owned(),
            });
        }
        // Keep the snapshot of what was really removed.
        let removed: Vec<ItemId> = report.removed.iter().map(|r| r.id).collect();
        let trees: Vec<Vec<Snapshot>> = allowed
            .iter()
            .zip(snapshots)
            .filter(|(id, _)| removed.contains(id))
            .map(|(_, tree)| tree)
            .collect();
        let undo_token = if trees.is_empty() {
            None
        } else {
            let token = format!("review-{}", now_millis());
            let mut stack = self.undo.lock().unwrap_or_else(|e| e.into_inner());
            stack.push_back(Undo {
                token: token.clone(),
                trees,
            });
            while stack.len() > UNDO_DEPTH {
                stack.pop_front();
            }
            Some(token)
        };
        Ok(ReviewRemoval { report, undo_token })
    }

    /// Reads a title and everything below it, for the Undo of a removal.
    fn snapshot(&self, id: ItemId) -> Result<Vec<Snapshot>, LibraryOpsError> {
        Ok(self.db.read(move |conn| {
            let mut out = Vec::new();
            for member in tree_ids(conn, id)? {
                let Some(item) = get_item(conn, member)? else {
                    continue;
                };
                let (availability, reason) = get_availability(conn, member)?;
                let flags = get_flags(conn, member)?;
                out.push(Snapshot {
                    item,
                    availability,
                    reason,
                    is_new: flags.is_new,
                    review_state: flags.review_state,
                    locks: field_locks(conn, member)?,
                });
            }
            Ok(out)
        })?)
    }

    /// Puts the titles of a removal back. Each gets a new id; its seasons and episodes follow it.
    /// An unknown or used token restores nothing.
    ///
    /// # Errors
    /// The library cannot be written.
    pub fn undo_removal(&self, token: &str) -> Result<Vec<RestoredTitle>, LibraryOpsError> {
        let undo = {
            let mut stack = self.undo.lock().unwrap_or_else(|e| e.into_inner());
            let Some(slot) = stack.iter().position(|u| u.token == token) else {
                return Ok(Vec::new());
            };
            stack.remove(slot)
        };
        let Some(undo) = undo else {
            return Ok(Vec::new());
        };
        Ok(self.db.write(move |tx| {
            let mut restored = Vec::new();
            for tree in undo.trees {
                let mut mapped: BTreeMap<ItemId, ItemId> = BTreeMap::new();
                for (index, snap) in tree.into_iter().enumerate() {
                    let was = snap.item.id.unwrap_or(ItemId(0));
                    let mut item = snap.item;
                    item.id = None;
                    if let Some(parent) = item.parent_id {
                        item.parent_id = Some(mapped.get(&parent).copied().unwrap_or(parent));
                    }
                    let id = insert_item(tx, &item)?;
                    set_availability(tx, id, snap.availability, &snap.reason)?;
                    if snap.is_new {
                        set_new(tx, &[id], true)?;
                    }
                    if snap.review_state != ReviewState::None {
                        set_review_state(tx, id, snap.review_state)?;
                    }
                    if !snap.locks.is_empty() {
                        set_field_locks(tx, id, &snap.locks)?;
                    }
                    mapped.insert(was, id);
                    if index == 0 {
                        restored.push(RestoredTitle {
                            was,
                            id,
                            title: item.core.title.clone(),
                        });
                    }
                }
            }
            Ok(restored)
        })?)
    }

    /// Leaves the chosen rows as they are and out of the review list until they have a different
    /// problem. Returns how many rows were kept.
    ///
    /// # Errors
    /// The library cannot be written.
    pub fn keep(&self, ids: &[ItemId]) -> Result<usize, LibraryOpsError> {
        let ids = unique(ids);
        let reasons: BTreeMap<ItemId, ReviewReason> = self
            .db
            .read(library_review::review_candidates)?
            .iter()
            .map(|c| (c.id, classify(c)))
            .collect();
        let now = now_millis();
        Ok(self.db.write(move |tx| {
            let mut kept = 0;
            for id in ids {
                if let Some(reason) = reasons.get(&id) {
                    library_review::keep(tx, id, reason.as_str(), now)?;
                    kept += 1;
                }
            }
            Ok(kept)
        })?)
    }

    /// Takes back a "keep": the row is listed again if it still has a problem.
    ///
    /// # Errors
    /// The library cannot be written.
    pub fn unkeep(&self, ids: &[ItemId]) -> Result<(), LibraryOpsError> {
        let ids = unique(ids);
        self.db.write(move |tx| {
            for id in ids {
                library_review::unkeep(tx, id)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Points a missing or offline movie or show at the folder its files moved to. The folder
    /// must exist, lie in a data source of the title's type and not belong to another title.
    /// Files are not touched; the next scan reads the new location.
    ///
    /// # Errors
    /// The request breaks one of those rules, or the library cannot be read or written.
    pub fn relocate(&self, id: ItemId, folder: &str) -> Result<Relocated, LibraryOpsError> {
        let target = folder.trim().trim_end_matches(['\\', '/']).to_owned();
        let found = self.db.read(move |conn| {
            let Some(item) = get_item(conn, id)? else {
                return Ok(None);
            };
            let (availability, _) = get_availability(conn, id)?;
            Ok(Some((item, availability, list_sources(conn, None)?)))
        })?;
        let Some((item, availability, sources)) = found else {
            return Err(LibraryOpsError::rejected(
                "title",
                "This title is no longer in the library.",
            ));
        };
        let kind = TitleKind::of(&item)
            .filter(|k| k.owns_folder())
            .ok_or_else(|| {
                LibraryOpsError::rejected("title", "Only a movie or a show can be relocated.")
            })?;
        if availability == Availability::Online {
            return Err(LibraryOpsError::rejected(
                "unavailable",
                "This title's files are where the library expects them.",
            ));
        }
        let from = item.core.path.clone();
        if from.is_empty() {
            return Err(LibraryOpsError::rejected(
                "data_source",
                "This title has no folder to relocate.",
            ));
        }
        if target.is_empty() || !Path::new(&target).is_dir() {
            return Err(LibraryOpsError::rejected(
                "exists",
                "That folder was not found on disk.",
            ));
        }
        let wanted = if kind == TitleKind::Show {
            SourceKind::Tv
        } else {
            SourceKind::Movie
        };
        let inside = sources
            .iter()
            .any(|s| s.kind == wanted && lies_under(&target, &s.path) && !same(&target, &s.path));
        if !inside {
            return Err(LibraryOpsError::rejected(
                "data_source",
                "That folder is not inside one of the library folders for this type.",
            ));
        }
        let owner_target = target.clone();
        let owner = self
            .db
            .read(move |conn| owner_of_folder(conn, &owner_target))?;
        if owner.is_some_and(|owner| owner != id.0) {
            return Err(LibraryOpsError::rejected(
                "exists",
                "Another title already uses that folder.",
            ));
        }

        let (from_for_write, to_for_write) = (from.clone(), target.clone());
        let after = self.db.write(move |tx| {
            library_review::relocate_tree(tx, id, &from_for_write, &to_for_write)?;
            library_review::unkeep(tx, id)?;
            // Re-check the files at the new place for this title only.
            let tree = tree_ids(tx, id)?;
            let plan = plan_availability(tx, &StdFileProbe, PathStyle::current())?;
            let changes: Vec<AvailabilityChange> = plan
                .changes
                .into_iter()
                .filter(|change| tree.contains(&change.id))
                .collect();
            apply_availability(tx, &changes)?;
            Ok(get_availability(tx, id)?.0)
        })?;
        Ok(Relocated {
            id,
            from,
            to: target,
            availability: after,
        })
    }
}

fn same(a: &str, b: &str) -> bool {
    lies_under(a, b) && lies_under(b, a)
}

/// The reason a candidate is listed for.
fn classify(candidate: &ReviewCandidate) -> ReviewReason {
    if candidate.duplicate_of.is_some() {
        return ReviewReason::Duplicate;
    }
    match candidate.availability {
        Availability::Offline => ReviewReason::Offline,
        _ => match reasons::parse(&candidate.availability_reason) {
            Some(Superseded::PartOfShow) => ReviewReason::Superseded,
            _ => ReviewReason::Missing,
        },
    }
}

fn row_of(
    candidate: ReviewCandidate,
    reason: ReviewReason,
    original: Option<DuplicateOf>,
) -> ReviewRow {
    let kind = match candidate.kind {
        ItemKind::Show => TitleKind::Show,
        ItemKind::Season => TitleKind::Season,
        ItemKind::Episode => TitleKind::Episode,
        ItemKind::Movie | ItemKind::Set => TitleKind::Movie,
    };
    let stored = candidate.availability_reason.trim();
    let detail = match reason {
        ReviewReason::Duplicate if lost_files_of(&candidate) => match &original {
            Some(of) if of.year > 0 => format!(
                "Possible duplicate of {} ({}); this row's files are missing",
                of.title, of.year
            ),
            Some(of) => format!(
                "Possible duplicate of {}; this row's files are missing",
                of.title
            ),
            None => "Possible duplicate of another row; this row's files are missing".to_owned(),
        },
        ReviewReason::Duplicate => match &original {
            Some(of) if of.year > 0 => format!("Duplicate of {} ({})", of.title, of.year),
            Some(of) => format!("Duplicate of {}", of.title),
            None => "Duplicate of another row".to_owned(),
        },
        ReviewReason::Superseded => {
            if stored.is_empty() {
                "A rescan read this folder as part of another show".to_owned()
            } else {
                format!("A rescan set this row aside: {stored}")
            }
        }
        ReviewReason::Missing if stored.is_empty() => "The files were not found on disk".to_owned(),
        ReviewReason::Offline if stored.is_empty() => {
            "The folder this title is in cannot be reached".to_owned()
        }
        ReviewReason::Missing | ReviewReason::Offline => capitalised(stored),
    };
    let lost_files = lost_files_of(&candidate);
    let can_relocate = (matches!(reason, ReviewReason::Missing | ReviewReason::Offline)
        || (reason == ReviewReason::Duplicate && lost_files))
        && matches!(kind, TitleKind::Movie | TitleKind::Show)
        && !candidate.path.is_empty();
    ReviewRow {
        id: candidate.id,
        kind,
        title: candidate.title,
        year: candidate.year,
        source: candidate.data_source,
        folder: candidate.path,
        reason,
        detail,
        duplicate_of: original,
        files: candidate.files,
        bytes: candidate.bytes,
        last_seen_ms: candidate.last_seen_ms,
        can_relocate,
    }
}

fn lost_files_of(candidate: &ReviewCandidate) -> bool {
    matches!(
        candidate.availability,
        Availability::Missing | Availability::Offline
    )
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

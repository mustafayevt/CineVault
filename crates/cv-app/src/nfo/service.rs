//! The NFO actions: Read NFO and Rewrite NFO with undo.
//!
//! * **Rewrite NFO** renders the NFO of each selected item from the library and writes it at every
//!   configured name (see [`super::paths`]), through a [`Journal`]: a file CineVault does not own
//!   is never overwritten, an unchanged file is not touched, a replaced file is backed up, and a
//!   managed NFO that the naming set dropped is moved to the journal's trash folder. An NFO that
//!   CineVault did not write itself (written by hand or by another tool, so it has no CineVault
//!   header comment) is also copied to the trash folder of the data source before it is replaced
//!   or moved away, where the user finds it next to the media even without the undo record. This
//!   covers the NFO a scrape writes by itself (`ScrapeFollowUp`), whose undo token no toast shows.
//! * **Read NFO** reads the NFO of each selected item back into the library
//!   (see [`super::merge`] for the rules) and remembers the values it replaced.
//! * **Undo** puts back the files and the library values of one action, by its token. Anything
//!   that changed since, a file edited in a text editor or an item edited in the app, is left
//!   alone and reported.
//!
//! An action's record (the journal and the item values before and after) is a JSON file under
//! `<undo folder>/<token>/`, so an undo still works after a restart. Old records are pruned at
//! start-up and after an action (see [`super::retention`]): kept for 30 days, at most 200 and
//! 512 MB, and the records of the last hour and the newest 20 are never removed, so an Undo a
//! toast offers always works.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cv_db::repo::{
    ItemQuery, children_of, field_locks_of, get_item, get_summary, list_items, update_item,
};
use cv_db::{Database, DbError};
use cv_domain::{
    FieldSet, Item, ItemDetail, ItemId, ItemKind, MediaFile, MediaFileType, revert_locked,
};
use cv_nfo::{Journal, NfoOwnership, classify_nfo_file, decode_nfo_bytes};
use serde::{Deserialize, Serialize};

use super::paths::NfoTargets;
use super::report::{ActionKind, ActionReport, FileOutcome, SkipReason, UndoSummary, UnitReport};
use super::retention::{self, PruneReport, RetentionPolicy};
use super::settings::NfoSettings;
use super::time::local_now;

/// Why an action could not run at all. Problems with single items are in the report instead.
#[derive(Debug, thiserror::Error)]
pub enum NfoError {
    #[error("the library could not be read or written: {0}")]
    Db(#[from] DbError),
    #[error("a file operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("the undo record is damaged: {0}")]
    Record(String),
    #[error("there is nothing to undo for {0:?}")]
    UnknownToken(String),
    #[error("this action was undone already")]
    AlreadyUndone,
    /// A title of the action is being moved or renamed; nothing was undone.
    #[error("{0}")]
    Busy(String),
}

/// The locked fields of the items an action works on. Items without a lock are not in the map.
pub(crate) type Locks = BTreeMap<ItemId, FieldSet>;

/// Undoes what a Read NFO merge did to a protected item: a locked item is put back as it was,
/// and the locked fields of any other item get their old values back.
pub(crate) fn protect(updated: &mut Item, original: &Item, locks: &Locks) {
    if original.core.locked {
        updated.clone_from(original);
    } else if let Some(set) = original.id.and_then(|id| locks.get(&id)) {
        revert_locked(updated, original, set);
    }
}

/// What an action runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NfoScope {
    /// The selected items. A show covers its seasons and episodes, a season its episodes.
    Items(Vec<ItemId>),
    /// Every movie, set and show of the library.
    All,
}

/// The library values of one item before and after a Read NFO.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ItemChange {
    pub before: Item,
    pub after: Item,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActionRecord {
    token: String,
    kind: ActionKind,
    created: String,
    journal: Journal,
    items: Vec<ItemChange>,
    undone: bool,
    /// The items whose files the action wrote: the undo holds their titles (see
    /// [`crate::title_guard`]). Empty in records written before it was kept.
    #[serde(default)]
    titles: Vec<ItemId>,
}

/// The state of one running action.
pub(crate) struct Run {
    /// The action's token, also the batch name of the originals it keeps in a trash folder.
    pub token: String,
    pub settings: NfoSettings,
    pub journal: Journal,
    pub units: Vec<UnitReport>,
    pub changes: Vec<ItemChange>,
    /// The import of a scan: an NFO an earlier CineVault wrote marks its title as scraped by
    /// CineVault (see `readback::adopt_cinevault_scrape`). Read NFO leaves the scraper alone.
    pub adopt_scrape: bool,
}

/// What of a show an action covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShowScope {
    Whole,
    Parts {
        show_nfo: bool,
        seasons: BTreeSet<u32>,
        episodes: BTreeSet<ItemId>,
    },
}

impl ShowScope {
    pub fn show_nfo(&self) -> bool {
        match self {
            Self::Whole => true,
            Self::Parts { show_nfo, .. } => *show_nfo,
        }
    }

    pub fn wants_season(&self, number: u32) -> bool {
        match self {
            Self::Whole => true,
            Self::Parts { seasons, .. } => seasons.contains(&number),
        }
    }

    /// Whether the episode is covered, by its own id or by its (aired) season.
    pub fn wants_episode(&self, id: Option<ItemId>, aired_season: u32) -> bool {
        match self {
            Self::Whole => true,
            Self::Parts {
                seasons, episodes, ..
            } => id.is_some_and(|i| episodes.contains(&i)) || seasons.contains(&aired_season),
        }
    }

    fn parts(&mut self) -> Option<(&mut bool, &mut BTreeSet<u32>, &mut BTreeSet<ItemId>)> {
        match self {
            Self::Whole => None,
            Self::Parts {
                show_nfo,
                seasons,
                episodes,
            } => Some((show_nfo, seasons, episodes)),
        }
    }
}

/// The items an action covers, in selection order.
#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub movies: Vec<ItemId>,
    pub sets: Vec<ItemId>,
    pub shows: BTreeMap<ItemId, ShowScope>,
}

/// A show with its seasons and episodes.
pub(crate) struct ShowData {
    pub show: Item,
    pub seasons: Vec<Item>,
    pub episodes: Vec<Item>,
}

/// Read NFO, Rewrite NFO and undo over one library.
pub struct NfoService {
    pub(crate) db: Arc<Database>,
    undo_dir: PathBuf,
    counter: AtomicU64,
    /// When the undo folder was last pruned (milliseconds since 1970), so actions that come in
    /// quick succession do not walk it each time.
    last_prune: AtomicU64,
}

impl std::fmt::Debug for NfoService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NfoService")
            .field("undo_dir", &self.undo_dir)
            .finish_non_exhaustive()
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 64
        && token
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

impl NfoService {
    /// A service over `db`. Undo records are kept under `undo_dir` (created when needed).
    pub fn new(db: Arc<Database>, undo_dir: impl Into<PathBuf>) -> Self {
        Self {
            db,
            undo_dir: undo_dir.into(),
            counter: AtomicU64::new(0),
            last_prune: AtomicU64::new(0),
        }
    }

    /// Rewrites the NFOs of the scope from the library.
    ///
    /// # Errors
    /// Fails only when the library cannot be read or the undo record cannot be saved; a problem
    /// with one item is in the report.
    pub fn rewrite(
        &self,
        scope: &NfoScope,
        settings: &NfoSettings,
    ) -> Result<ActionReport, NfoError> {
        let (token, mut run) = self.start(settings);
        let plan = self.plan(scope, &mut run.units)?;
        for id in &plan.movies {
            self.rewrite_movie(&mut run, *id);
        }
        for id in &plan.sets {
            self.rewrite_set(&mut run, *id);
        }
        for (id, show_scope) in &plan.shows {
            self.rewrite_show(&mut run, *id, show_scope);
        }
        self.finish(ActionKind::Rewrite, token, run)
    }

    /// Lists the NFO files a rewrite wrote among the files of their movie or episode, and drops
    /// the ones it moved to the trash. Until a rescan sees them, a rename planned from the library
    /// would leave a new NFO behind under the old name.
    /// A problem is logged: the files on disk are right either way, and a rescan lists them.
    /// Called while the title is still held, so no rename or move has changed its folder since
    /// the NFOs were written; a written file that is not there is never listed.
    pub(super) fn record_nfo_files(&self, units: &[UnitReport]) {
        for unit in units {
            let written: Vec<PathBuf> = unit
                .files
                .iter()
                .filter(|f| {
                    matches!(
                        f.outcome,
                        FileOutcome::Created | FileOutcome::Updated | FileOutcome::Unchanged
                    ) && f.path.is_file()
                })
                .map(|f| f.path.clone())
                .collect();
            let trashed: Vec<PathBuf> = unit
                .files
                .iter()
                .filter(|f| f.outcome == FileOutcome::MovedToTrash)
                .map(|f| f.path.clone())
                .collect();
            if written.is_empty() && trashed.is_empty() {
                continue;
            }
            let id = unit.item;
            let recorded = self.db.write(move |tx| {
                let Some(mut item) = get_item(tx, id)? else {
                    return Ok(());
                };
                // A show's and a season's NFOs keep their names on a rename; a collection's live
                // outside the movies. Only a movie's and an episode's follow the video.
                if !matches!(item.detail, ItemDetail::Movie(_) | ItemDetail::Episode(_)) {
                    return Ok(());
                }
                let before = item.core.media_files.len();
                item.core.media_files.retain(|f| {
                    !(f.kind == MediaFileType::Nfo
                        && trashed.iter().any(|path| same_path(&f.path(), path)))
                });
                let mut changed = item.core.media_files.len() != before;
                for path in &written {
                    if item
                        .core
                        .media_files
                        .iter()
                        .any(|f| same_path(&f.path(), path))
                    {
                        continue;
                    }
                    let (Some(directory), Some(name)) = (path.parent(), path.file_name()) else {
                        continue;
                    };
                    let mut row = MediaFile::new(
                        MediaFileType::Nfo,
                        directory.to_string_lossy(),
                        name.to_string_lossy(),
                    );
                    if let Ok(meta) = fs::metadata(path) {
                        row.size_bytes = meta.len();
                    }
                    item.core.media_files.push(row);
                    changed = true;
                }
                if changed {
                    update_item(tx, &item)?;
                }
                Ok(())
            });
            if let Err(error) = recorded {
                tracing::warn!(item = id.0, %error, "the written NFO was not listed with the title");
            }
        }
    }

    /// Reads the NFOs of the scope into the library.
    ///
    /// # Errors
    /// See [`NfoService::rewrite`].
    pub fn read(&self, scope: &NfoScope, settings: &NfoSettings) -> Result<ActionReport, NfoError> {
        let (token, mut run) = self.start(settings);
        let plan = self.plan(scope, &mut run.units)?;
        for id in &plan.movies {
            self.read_movie(&mut run, *id);
        }
        for id in &plan.sets {
            self.read_set(&mut run, *id);
        }
        for (id, show_scope) in &plan.shows {
            self.read_show(&mut run, *id, show_scope);
        }
        self.finish(ActionKind::Read, token, run)
    }

    /// Reads the NFOs of `items` into the library without keeping an undo record: the import of
    /// local files that follows a scan. Only managed NFOs are read (see
    /// `cv_nfo::ownership`), locks are honoured, and no file is written or moved, so there is
    /// nothing to undo; a title that was wrong can still be changed with Read NFO or the editor.
    ///
    /// # Errors
    /// See [`NfoService::rewrite`].
    pub fn import_local(
        &self,
        items: &[ItemId],
        settings: &NfoSettings,
    ) -> Result<ActionReport, NfoError> {
        let (_, mut run) = self.start(settings);
        run.adopt_scrape = true;
        let plan = self.plan(&NfoScope::Items(items.to_vec()), &mut run.units)?;
        for id in &plan.movies {
            self.read_movie(&mut run, *id);
        }
        for (id, show_scope) in &plan.shows {
            self.read_show(&mut run, *id, show_scope);
        }
        Ok(ActionReport::new(ActionKind::Read, None, run.units))
    }

    /// Reverses the action `token` names.
    ///
    /// # Errors
    /// Fails when the token is unknown or was used already, or the record cannot be read. Files
    /// and items that changed since are not an error: they are listed in the summary.
    pub fn undo(&self, token: &str) -> Result<UndoSummary, NfoError> {
        let mut record = self.load_record(token)?;
        if record.undone {
            return Err(NfoError::AlreadyUndone);
        }
        // No move or rename of the titles runs while their NFOs go back, as with the writes.
        let _titles = crate::title_guard::hold_items(
            &self.db,
            &record.titles,
            crate::title_guard::Work::Write,
            crate::title_guard::WRITE_WAIT,
        )
        .map_err(|error| NfoError::Busy(error.to_string()))?;
        let mut summary = UndoSummary::default();

        let files = record.journal.undo();
        summary.restored += u32::try_from(files.restored()).unwrap_or(u32::MAX);
        for step in files.steps.iter().filter(|s| !s.restored) {
            summary.skipped.push(format!(
                "{}: {}",
                step.path.display(),
                step.reason.as_deref().unwrap_or("not restored")
            ));
        }

        for change in record.items.iter().rev() {
            let id = change.after.id;
            let before = change.before.clone();
            let after = change.after.clone();
            let outcome = self.db.write(move |tx| {
                let Some(id) = id else { return Ok(false) };
                if get_item(tx, id)?.as_ref() != Some(&after) {
                    return Ok(false);
                }
                update_item(tx, &before)?;
                Ok(true)
            })?;
            if outcome {
                summary.restored += 1;
            } else {
                summary.skipped.push(format!(
                    "{}: changed since the action",
                    change.after.core.title
                ));
            }
        }

        record.undone = true;
        self.save_record(&record)?;
        Ok(summary)
    }

    // ---- records ---------------------------------------------------------------------------

    fn start(&self, settings: &NfoSettings) -> (String, Run) {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let token = format!("nfo-{ms}-{n}");
        let run = Run {
            token: token.clone(),
            settings: settings.stamped(&local_now()),
            journal: Journal::new(self.undo_dir.join(&token).join("files")),
            units: Vec::new(),
            changes: Vec::new(),
            adopt_scrape: false,
        };
        (token, run)
    }

    fn finish(&self, kind: ActionKind, token: String, run: Run) -> Result<ActionReport, NfoError> {
        let nothing = run.journal.is_empty() && run.changes.is_empty();
        let mut titles: Vec<ItemId> = run.units.iter().map(|unit| unit.item).collect();
        titles.sort_unstable_by_key(|id| id.0);
        titles.dedup();
        let record = ActionRecord {
            token: token.clone(),
            kind,
            created: local_now(),
            journal: run.journal,
            items: run.changes,
            undone: false,
            titles,
        };
        if !nothing {
            self.save_record(&record)?;
            self.prune_now_and_then();
        }
        Ok(ActionReport::new(
            kind,
            (!nothing).then_some(token),
            run.units,
        ))
    }

    /// Removes old undo records by the default [`RetentionPolicy`]. The app calls it at start-up;
    /// actions call it at most every ten minutes.
    pub fn prune_undo_records(&self) -> PruneReport {
        self.last_prune.store(now_millis(), Ordering::Relaxed);
        retention::prune(
            &self.undo_dir,
            u128::from(now_millis()),
            &RetentionPolicy::default(),
        )
    }

    fn prune_now_and_then(&self) {
        const EVERY_MS: u64 = 10 * 60 * 1000;
        let last = self.last_prune.load(Ordering::Relaxed);
        if now_millis().saturating_sub(last) >= EVERY_MS {
            self.prune_undo_records();
        }
    }

    fn record_path(&self, token: &str) -> PathBuf {
        self.undo_dir.join(token).join("action.json")
    }

    fn save_record(&self, record: &ActionRecord) -> Result<(), NfoError> {
        let path = self.record_path(&record.token);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json =
            serde_json::to_vec_pretty(record).map_err(|e| NfoError::Record(e.to_string()))?;
        cv_fs::write_atomic(&path, &json)?;
        Ok(())
    }

    fn load_record(&self, token: &str) -> Result<ActionRecord, NfoError> {
        if !valid_token(token) {
            return Err(NfoError::UnknownToken(token.to_owned()));
        }
        let bytes = match fs::read(self.record_path(token)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(NfoError::UnknownToken(token.to_owned()));
            }
            Err(e) => return Err(e.into()),
        };
        serde_json::from_slice(&bytes).map_err(|e| NfoError::Record(e.to_string()))
    }

    // ---- planning and loading ---------------------------------------------------------------

    fn plan(&self, scope: &NfoScope, units: &mut Vec<UnitReport>) -> Result<Plan, NfoError> {
        let mut plan = Plan::default();
        match scope {
            NfoScope::All => {
                plan.movies = self.all_ids(ItemKind::Movie)?;
                plan.sets = self.all_ids(ItemKind::Set)?;
                for id in self.all_ids(ItemKind::Show)? {
                    plan.shows.insert(id, ShowScope::Whole);
                }
            }
            NfoScope::Items(ids) => {
                let mut seen: BTreeSet<ItemId> = BTreeSet::new();
                for id in ids.iter().filter(|id| seen.insert(**id)) {
                    self.plan_item(*id, &mut plan, units)?;
                }
            }
        }
        Ok(plan)
    }

    fn plan_item(
        &self,
        id: ItemId,
        plan: &mut Plan,
        units: &mut Vec<UnitReport>,
    ) -> Result<(), NfoError> {
        let Some(summary) = self.db.read(|c| get_summary(c, id))? else {
            units.push(UnitReport::skipped(
                id,
                id.0.to_string(),
                SkipReason::NotFound,
            ));
            return Ok(());
        };
        match summary.kind {
            ItemKind::Movie => plan.movies.push(id),
            ItemKind::Set => plan.sets.push(id),
            ItemKind::Show => {
                plan.shows.insert(id, ShowScope::Whole);
            }
            ItemKind::Season | ItemKind::Episode => {
                let Some(show) = summary.parent_id else {
                    units.push(UnitReport::skipped(
                        id,
                        summary.title,
                        SkipReason::Unsupported,
                    ));
                    return Ok(());
                };
                let scope = plan.shows.entry(show).or_insert(ShowScope::Parts {
                    show_nfo: false,
                    seasons: BTreeSet::new(),
                    episodes: BTreeSet::new(),
                });
                if let Some((_, seasons, episodes)) = scope.parts() {
                    if summary.kind == ItemKind::Episode {
                        episodes.insert(id);
                    } else if let Some(item) = self.db.read(|c| get_item(c, id))?
                        && let ItemDetail::Season(s) = &item.detail
                    {
                        seasons.insert(s.season);
                    }
                }
            }
        }
        Ok(())
    }

    fn all_ids(&self, kind: ItemKind) -> Result<Vec<ItemId>, NfoError> {
        let query = ItemQuery::new(kind);
        let mut ids = Vec::new();
        let mut cursor = None;
        loop {
            let page = self
                .db
                .read(|c| list_items(c, &query, cursor.as_ref(), 500))?;
            ids.extend(page.items.iter().map(|s| s.id));
            match page.next {
                Some(next) => cursor = Some(next),
                None => return Ok(ids),
            }
        }
    }

    /// A movie and the set it belongs to.
    pub(crate) fn load_movie(&self, id: ItemId) -> Result<Option<(Item, Option<Item>)>, DbError> {
        self.db.read(|c| {
            let Some(item) = get_item(c, id)? else {
                return Ok(None);
            };
            let set = match item.parent_id {
                Some(parent) => {
                    get_item(c, parent)?.filter(|p| matches!(p.detail, ItemDetail::Set(_)))
                }
                None => None,
            };
            Ok(Some((item, set)))
        })
    }

    /// The field locks of the given items.
    pub(crate) fn locks_of(&self, ids: &[ItemId]) -> Result<Locks, DbError> {
        self.db.read(|c| field_locks_of(c, ids))
    }

    pub(crate) fn load_item(&self, id: ItemId) -> Result<Option<Item>, DbError> {
        self.db.read(|c| get_item(c, id))
    }

    pub(crate) fn load_show(&self, id: ItemId) -> Result<Option<ShowData>, DbError> {
        self.db.read(|c| {
            let Some(show) = get_item(c, id)? else {
                return Ok(None);
            };
            if !matches!(show.detail, ItemDetail::Show(_)) {
                return Ok(None);
            }
            let mut seasons = Vec::new();
            let mut episodes = Vec::new();
            for child in children_of(c, id)? {
                if let Some(item) = get_item(c, child)? {
                    match item.detail {
                        ItemDetail::Season(_) => seasons.push(item),
                        ItemDetail::Episode(_) => episodes.push(item),
                        _ => {}
                    }
                }
            }
            Ok(Some(ShowData {
                show,
                seasons,
                episodes,
            }))
        })
    }

    /// Stores changed items in one transaction and remembers the values for undo.
    pub(crate) fn commit(&self, run: &mut Run, changed: Vec<(Item, Item)>) -> Result<(), NfoError> {
        if changed.is_empty() {
            return Ok(());
        }
        let news: Vec<Item> = changed.iter().map(|(_, new)| new.clone()).collect();
        let afters = self.db.write(move |tx| {
            let mut out = Vec::with_capacity(news.len());
            for item in &news {
                update_item(tx, item)?;
                if let Some(id) = item.id
                    && let Some(stored) = get_item(tx, id)?
                {
                    out.push(stored);
                }
            }
            Ok(out)
        })?;
        for ((before, _), after) in changed.into_iter().zip(afters) {
            run.changes.push(ItemChange { before, after });
        }
        Ok(())
    }
}

/// What an existing NFO of an item is.
pub(crate) enum Found {
    /// The text of the first managed candidate.
    Managed(String),
    /// A candidate exists, but it is not a managed NFO.
    NotManaged,
    None,
}

/// Finds the NFO to read: the first managed candidate, writing names first.
pub(crate) fn find_nfo(targets: &NfoTargets) -> Found {
    let mut foreign = false;
    for path in &targets.candidates {
        match classify_nfo_file(path) {
            Ok(NfoOwnership::Managed) => {
                if let Ok(bytes) = fs::read(path) {
                    return Found::Managed(decode_nfo_bytes(&bytes));
                }
            }
            Ok(NfoOwnership::Missing) | Err(_) => {}
            Ok(_) => foreign = true,
        }
    }
    if foreign {
        Found::NotManaged
    } else {
        Found::None
    }
}

/// The text of the first managed candidate, for the values a rewrite carries over.
pub(crate) fn old_text(targets: &NfoTargets) -> Option<String> {
    match find_nfo(targets) {
        Found::Managed(text) => Some(text),
        _ => None,
    }
}

/// Whether the folder of the path exists.
pub(crate) fn parent_exists(path: &Path) -> bool {
    path.parent().is_some_and(Path::is_dir)
}

/// Whether two paths name the same file, ignoring the separator and the letter case.
fn same_path(a: &Path, b: &Path) -> bool {
    let normal = |p: &Path| p.to_string_lossy().replace('\\', "/");
    normal(a).eq_ignore_ascii_case(&normal(b))
}

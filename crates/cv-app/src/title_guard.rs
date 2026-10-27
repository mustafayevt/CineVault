//! One file operation per title at a time.
//!
//! A move to another data source copies a title folder for minutes or hours, and a rename moves
//! it in one step. Anything else that writes into the same title folder in the meantime (an NFO
//! written after a scrape, artwork, a subtitle, a theme, a trailer, or a second move or rename)
//! would land in a folder that is about to be left, or be copied half-written. So every such
//! piece of work holds its title here first, and work on the same title waits its turn:
//!
//! * a move or a rename waits a while for a short write to finish ([`MOVE_WAIT`]);
//! * a write waits a little ([`WRITE_WAIT`]) and then gives up with a plain reason
//!   ([`Busy`]), so a scrape does not sit behind a move of an hour.
//!
//! The steps that follow a scrape (the NFO, then the artwork: `ScrapeFollowUp`) are several
//! writes in a row, and the folder must stay put between them too. They **pin** the title for
//! their whole run ([`pin`], [`Work::Scrape`]): a pin is granted at once, never waits and never
//! fails. While a title is pinned no move or rename of it starts, and one that asks is told at
//! once that the title is busy (it is not made to wait: the follow-up downloads artwork and may
//! take minutes), so Organize reports the title and renames it the next time. The follow-up's
//! own writes are not held up by its pin (a pin only keeps moves and renames out), so the
//! follow-up never waits on itself. This registry is the one place that says a title is busy.
//!
//! A title is a movie or a TV show: a season or an episode stands for its show. Titles are kept
//! per library (the database file), so two libraries never hold each other up. The holder loads
//! the title's paths from the library *after* it holds the title, so a move that finished while
//! it waited is seen. The guard is not re-entrant: a holder must not ask for a title it holds
//! already (a pin is not a hold in this sense: the pinned follow-up's writes hold the title).
//!
//! No call waits while it holds titles: [`hold`] takes all its titles at once, and
//! [`hold_each`] waits holding none and then takes what is free, so two holders never wait on
//! each other in a circle.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Condvar, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use cv_db::Database;
use cv_domain::ItemId;
use rusqlite::OptionalExtension;

/// How long a move or a rename waits for the writes on its titles to finish.
pub const MOVE_WAIT: Duration = Duration::from_secs(30);

/// How long a write waits for a move or a rename of its title.
pub const WRITE_WAIT: Duration = Duration::from_secs(10);

/// What holds a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Work {
    /// A move to another data source, its resume, roll back or undo.
    Move,
    /// An Organize or automatic rename, or its undo.
    Rename,
    /// A write of one or a few files into the title folder.
    Write,
    /// A delete of the title's files, to the bin or for good: like a move, it needs the title
    /// to itself, so no NFO or artwork lands in a folder that is going.
    Delete,
    /// The NFO and artwork steps that follow a scrape (a [`pin`]): no move or rename starts
    /// until they are over; their own writes go ahead.
    Scrape,
}

/// The title is busy with other work; nothing was done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Busy {
    /// What holds the title.
    pub work: Work,
}

impl fmt::Display for Busy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.work {
            Work::Move => {
                "The title is being moved to another source right now. Try again when the move \
                 has finished."
            }
            Work::Rename => {
                "The title is being renamed right now. Try again when the rename has finished."
            }
            Work::Write => {
                "CineVault is writing files of this title right now. Try again in a moment."
            }
            Work::Delete => {
                "The files of this title are being deleted right now. Try again when that has                  finished."
            }
            Work::Scrape => {
                "Its NFO or artwork is still being written after its scrape. Try again in a \
                 moment."
            }
        })
    }
}

impl std::error::Error for Busy {}

/// Why a title could not be held.
#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    #[error(transparent)]
    Busy(#[from] Busy),
    #[error("the library could not be read: {0}")]
    Db(#[from] cv_db::DbError),
}

/// A held title: the library it is in (the database path, folded) and its id.
type Key = (String, i64);

/// Who holds one title: at most one move, rename or write, and any number of scrape pins.
#[derive(Debug, Default, Clone, Copy)]
struct Slot {
    owner: Option<Work>,
    pins: usize,
}

impl Slot {
    /// What keeps `work` from holding this title now, if anything.
    fn blocks(self, work: Work) -> Option<Work> {
        match work {
            // A move or a rename needs the title to itself. A pinned title is named as pinned
            // even while one of the follow-up's writes holds it, so it is not waited for.
            Work::Move | Work::Rename | Work::Delete => {
                (self.pins > 0).then_some(Work::Scrape).or(self.owner)
            }
            // One write at a time; a scrape's pin does not keep its own writes out.
            Work::Write => self.owner,
            // A pin is always granted (see [`pin`]).
            Work::Scrape => None,
        }
    }

    fn take(&mut self, work: Work) {
        match work {
            Work::Scrape => self.pins += 1,
            other => self.owner = Some(other),
        }
    }

    fn give_back(&mut self, work: Work) {
        match work {
            Work::Scrape => self.pins = self.pins.saturating_sub(1),
            _ => self.owner = None,
        }
    }

    fn is_free(self) -> bool {
        self.owner.is_none() && self.pins == 0
    }

    fn holder(self) -> Option<Work> {
        self.owner.or((self.pins > 0).then_some(Work::Scrape))
    }
}

struct Registry {
    held: Mutex<HashMap<Key, Slot>>,
    released: Condvar,
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| Registry {
    held: Mutex::new(HashMap::new()),
    released: Condvar::new(),
});

fn held() -> MutexGuard<'static, HashMap<Key, Slot>> {
    REGISTRY
        .held
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn library_of(db: &Database) -> String {
    db.path().to_string_lossy().to_lowercase()
}

/// What keeps `work` from holding `key` now.
fn blocker(map: &HashMap<Key, Slot>, key: &Key, work: Work) -> Option<Work> {
    map.get(key).and_then(|slot| slot.blocks(work))
}

fn take_all(map: &mut HashMap<Key, Slot>, keys: &[Key], work: Work) {
    for key in keys {
        map.entry(key.clone()).or_default().take(work);
    }
}

fn keys_of(library: &str, titles: impl IntoIterator<Item = ItemId>) -> Vec<Key> {
    let mut wanted: Vec<Key> = titles
        .into_iter()
        .map(|id| (library.to_owned(), id.0))
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    wanted
}

/// Titles held by one piece of work; released when dropped.
#[derive(Debug)]
#[must_use = "the titles are released as soon as the guard is dropped"]
pub struct TitleGuard {
    keys: Vec<Key>,
    work: Work,
}

impl TitleGuard {
    /// The title ids this guard holds.
    #[must_use]
    pub fn ids(&self) -> Vec<ItemId> {
        self.keys.iter().map(|(_, id)| ItemId(*id)).collect()
    }

    /// Lets go of the titles for which `keep` is false, and keeps holding the others.
    pub fn retain(&mut self, mut keep: impl FnMut(ItemId) -> bool) {
        let (kept, released): (Vec<Key>, Vec<Key>) = std::mem::take(&mut self.keys)
            .into_iter()
            .partition(|(_, id)| keep(ItemId(*id)));
        self.keys = kept;
        if released.is_empty() {
            return;
        }
        let mut map = held();
        for key in &released {
            if let Some(slot) = map.get_mut(key) {
                slot.give_back(self.work);
                if slot.is_free() {
                    map.remove(key);
                }
            }
        }
        drop(map);
        REGISTRY.released.notify_all();
    }
}

impl Drop for TitleGuard {
    fn drop(&mut self) {
        let mut map = held();
        for key in &self.keys {
            if let Some(slot) = map.get_mut(key) {
                slot.give_back(self.work);
                if slot.is_free() {
                    map.remove(key);
                }
            }
        }
        drop(map);
        REGISTRY.released.notify_all();
    }
}

fn hold_in(
    library: &str,
    titles: impl IntoIterator<Item = ItemId>,
    work: Work,
    wait: Duration,
) -> Result<TitleGuard, Busy> {
    let wanted = keys_of(library, titles);
    let deadline = Instant::now() + wait;
    let mut map = held();
    loop {
        let taken = wanted.iter().find_map(|key| blocker(&map, key, work));
        let Some(other) = taken else { break };
        let now = Instant::now();
        // A scrape's follow-up can take minutes: a move or a rename is told at once.
        if now >= deadline || other == Work::Scrape {
            return Err(Busy { work: other });
        }
        map = REGISTRY
            .released
            .wait_timeout(map, deadline - now)
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0;
    }
    take_all(&mut map, &wanted, work);
    Ok(TitleGuard { keys: wanted, work })
}

/// Holds the titles `titles` (movie or show ids) of the library `db` for `work`, waiting up to
/// `wait` for any other holder of one of them. All are held at once, so two holders never wait
/// on each other in a circle. A title pinned by a scrape's follow-up is not waited for.
///
/// # Errors
/// [`Busy`] names what still holds a title after `wait`.
pub fn hold(
    db: &Database,
    titles: impl IntoIterator<Item = ItemId>,
    work: Work,
    wait: Duration,
) -> Result<TitleGuard, Busy> {
    hold_in(&library_of(db), titles, work, wait)
}

fn hold_each_in(
    library: &str,
    titles: impl IntoIterator<Item = ItemId>,
    work: Work,
    wait: Duration,
) -> (TitleGuard, Vec<(ItemId, Busy)>) {
    let wanted = keys_of(library, titles);
    let deadline = Instant::now() + wait;
    let mut map = held();
    loop {
        // Nothing is held while this waits, so no other holder ever waits on it meanwhile.
        let waiting = wanted
            .iter()
            .any(|key| blocker(&map, key, work).is_some_and(|other| other != Work::Scrape));
        let now = Instant::now();
        if !waiting || now >= deadline {
            break;
        }
        map = REGISTRY
            .released
            .wait_timeout(map, deadline - now)
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0;
    }
    let mut free = Vec::new();
    let mut busy = Vec::new();
    for key in wanted {
        match blocker(&map, &key, work) {
            Some(other) => busy.push((ItemId(key.1), Busy { work: other })),
            None => free.push(key),
        }
    }
    take_all(&mut map, &free, work);
    (TitleGuard { keys: free, work }, busy)
}

/// Holds as many of `titles` (movie or show ids) as it can for `work`: it waits up to `wait`,
/// holding none of them meanwhile, for the other holders to finish, then takes every title
/// that is free and names the others with what holds them. A title pinned by a scrape's
/// follow-up is not waited for. For a batch (Organize, a move of several titles) that runs the
/// titles it got and reports the rest.
pub fn hold_each(
    db: &Database,
    titles: impl IntoIterator<Item = ItemId>,
    work: Work,
    wait: Duration,
) -> (TitleGuard, Vec<(ItemId, Busy)>) {
    hold_each_in(&library_of(db), titles, work, wait)
}

/// Pins `titles` (movie or show ids) for the steps that follow a scrape: granted at once, never
/// waits and never fails. While the guard lives no move or rename of the titles starts (one that
/// asks is told at once, with [`Work::Scrape`]); writes into them still go ahead, one at a time.
/// A move or a rename that runs already is not stopped: the follow-up's writes wait for it as
/// any write does, and give up after [`WRITE_WAIT`] with the reason.
pub fn pin(db: &Database, titles: impl IntoIterator<Item = ItemId>) -> TitleGuard {
    pin_in(&library_of(db), titles)
}

fn pin_in(library: &str, titles: impl IntoIterator<Item = ItemId>) -> TitleGuard {
    let wanted = keys_of(library, titles);
    take_all(&mut held(), &wanted, Work::Scrape);
    TitleGuard {
        keys: wanted,
        work: Work::Scrape,
    }
}

/// What holds the title `id` of the library `db` now, if anything: a move, rename or write
/// before a scrape's pin.
#[must_use]
pub fn holder(db: &Database, id: ItemId) -> Option<Work> {
    held()
        .get(&(library_of(db), id.0))
        .and_then(|slot| slot.holder())
}

/// Whether a scrape's follow-up pins the title `id` of the library `db` now (see [`pin`]).
#[must_use]
pub fn is_pinned(db: &Database, id: ItemId) -> bool {
    held()
        .get(&(library_of(db), id.0))
        .is_some_and(|slot| slot.pins > 0)
}

/// The title an item belongs to: a season or an episode belongs to its show; anything else is
/// its own title. An id the library does not know is returned as it is.
///
/// # Errors
/// The library cannot be read.
pub fn title_of(db: &Database, id: ItemId) -> Result<ItemId, cv_db::DbError> {
    db.read(move |conn| {
        let parent: Option<Option<i64>> = conn
            .query_row(
                "SELECT CASE WHEN kind IN ('season', 'episode') THEN parent_id ELSE id END \
                 FROM item WHERE id = ?1",
                [id.0],
                |row| row.get(0),
            )
            .optional()?;
        Ok(ItemId(parent.flatten().unwrap_or(id.0)))
    })
}

fn titles_of(db: &Database, ids: &[ItemId]) -> Result<Vec<ItemId>, cv_db::DbError> {
    ids.iter().map(|id| title_of(db, *id)).collect()
}

/// [`hold`] for any items: each stands for its title (see [`title_of`]).
///
/// # Errors
/// The library cannot be read, or a title is still busy after `wait`.
pub fn hold_items(
    db: &Database,
    ids: &[ItemId],
    work: Work,
    wait: Duration,
) -> Result<TitleGuard, GuardError> {
    let titles = titles_of(db, ids)?;
    Ok(hold(db, titles, work, wait)?)
}

/// [`hold_items`] for async work: it never blocks the runtime, it looks again every 100 ms.
///
/// # Errors
/// See [`hold_items`].
pub async fn hold_items_async(
    db: &std::sync::Arc<Database>,
    ids: &[ItemId],
    work: Work,
    wait: Duration,
) -> Result<TitleGuard, GuardError> {
    // The library is read on a blocking thread: the runtime keeps answering commands.
    let reader = std::sync::Arc::clone(db);
    let wanted = ids.to_vec();
    let titles = match tokio::task::spawn_blocking(move || titles_of(&reader, &wanted)).await {
        Ok(titles) => titles?,
        Err(_) => ids.to_vec(),
    };
    let library = library_of(db);
    let deadline = Instant::now() + wait;
    loop {
        match hold_in(&library, titles.iter().copied(), work, Duration::ZERO) {
            Ok(guard) => return Ok(guard),
            Err(busy) if Instant::now() >= deadline || busy.work == Work::Scrape => {
                return Err(busy.into());
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    const A: ItemId = ItemId(1);
    const B: ItemId = ItemId(2);

    fn slot(library: &str, id: i64) -> Option<Slot> {
        held().get(&(library.to_owned(), id)).copied()
    }

    #[test]
    fn a_held_title_makes_a_write_give_up_with_the_reason() {
        let _move = hold_in("lib-a", [A], Work::Move, Duration::ZERO).unwrap();
        let busy = hold_in("lib-a", [A], Work::Write, Duration::from_millis(50)).unwrap_err();
        assert_eq!(busy.work, Work::Move);
        assert!(busy.to_string().contains("being moved"), "{busy}");
        assert_eq!(slot("lib-a", 1).and_then(Slot::holder), Some(Work::Move));
    }

    #[test]
    fn retain_lets_go_of_the_titles_left_out_and_keeps_the_others() {
        let mut rename = hold_in("lib-r", [A, B], Work::Rename, Duration::ZERO).unwrap();
        rename.retain(|id| id == A);
        assert_eq!(rename.ids(), [A]);
        assert!(hold_in("lib-r", [B], Work::Write, Duration::ZERO).is_ok());
        let busy = hold_in("lib-r", [A], Work::Write, Duration::ZERO).unwrap_err();
        assert_eq!(busy.work, Work::Rename);
        drop(rename);
        assert!(slot("lib-r", 1).is_none(), "all released");
    }

    #[test]
    fn other_titles_and_other_libraries_are_not_held_up() {
        let _move = hold_in("lib-b", [A], Work::Move, Duration::ZERO).unwrap();
        assert!(hold_in("lib-b", [B], Work::Write, Duration::ZERO).is_ok());
        assert!(hold_in("lib-c", [A], Work::Write, Duration::ZERO).is_ok());
    }

    #[test]
    fn a_waiting_move_starts_when_the_write_is_done() {
        let write = hold_in("lib-d", [A], Work::Write, Duration::ZERO).unwrap();
        let started = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&started);
        let mover = std::thread::spawn(move || {
            let _guard = hold_in("lib-d", [A], Work::Move, Duration::from_secs(5)).unwrap();
            flag.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(!started.load(Ordering::SeqCst), "the write still holds it");
        drop(write);
        mover.join().unwrap();
        assert!(started.load(Ordering::SeqCst));
        assert!(slot("lib-d", 1).is_none(), "released on drop");
    }

    #[test]
    fn a_pinned_title_keeps_moves_and_renames_out_at_once_and_lets_its_writes_in() {
        let pinned = pin_in("lib-e", [A]);
        let started = Instant::now();
        // A rename asking with a long wait is told at once: a follow-up may take minutes.
        let busy = hold_in("lib-e", [A], Work::Rename, Duration::from_secs(20)).unwrap_err();
        assert_eq!(busy.work, Work::Scrape);
        assert!(busy.to_string().contains("NFO or artwork"), "{busy}");
        let busy = hold_in("lib-e", [A], Work::Move, Duration::from_secs(20)).unwrap_err();
        assert_eq!(busy.work, Work::Scrape);
        assert!(started.elapsed() < Duration::from_secs(5), "not waited for");
        // The follow-up's own writes go ahead, one at a time.
        let nfo = hold_in("lib-e", [A], Work::Write, Duration::ZERO).unwrap();
        let second = hold_in("lib-e", [A], Work::Write, Duration::ZERO).unwrap_err();
        assert_eq!(second.work, Work::Write);
        drop(nfo);
        // A second pin (a second scrape of the title) is granted too; both must end.
        let again = pin_in("lib-e", [A]);
        drop(pinned);
        assert_eq!(
            hold_in("lib-e", [A], Work::Rename, Duration::ZERO)
                .unwrap_err()
                .work,
            Work::Scrape
        );
        drop(again);
        assert!(hold_in("lib-e", [A], Work::Rename, Duration::ZERO).is_ok());
        assert!(slot("lib-e", 1).is_none(), "released on drop");
    }

    #[test]
    fn a_pin_never_waits_even_while_the_title_is_renamed() {
        let rename = hold_in("lib-f", [A], Work::Rename, Duration::ZERO).unwrap();
        let pinned = pin_in("lib-f", [A]);
        // Its writes wait for the rename that runs, as any write does.
        assert_eq!(
            hold_in("lib-f", [A], Work::Write, Duration::ZERO)
                .unwrap_err()
                .work,
            Work::Rename
        );
        drop(rename);
        assert!(hold_in("lib-f", [A], Work::Write, Duration::ZERO).is_ok());
        drop(pinned);
        assert!(slot("lib-f", 1).is_none());
    }

    #[test]
    fn hold_each_takes_the_free_titles_and_names_the_busy_ones() {
        let _pinned = pin_in("lib-g", [A]);
        let (guard, busy) = hold_each_in("lib-g", [A, B], Work::Rename, Duration::from_secs(20));
        assert_eq!(guard.ids(), vec![B]);
        assert_eq!(busy, vec![(A, Busy { work: Work::Scrape })]);
    }

    #[test]
    fn hold_each_holds_nothing_while_it_waits() {
        // A short write holds B; a batch of A and B waits for it. Meanwhile A stays free, so a
        // follow-up write into A is never held up by the batch that waits (no circle).
        let write_b = hold_in("lib-h", [B], Work::Write, Duration::ZERO).unwrap();
        let batch = std::thread::spawn(|| {
            let (guard, busy) =
                hold_each_in("lib-h", [A, B], Work::Rename, Duration::from_secs(10));
            (guard.ids(), busy)
        });
        std::thread::sleep(Duration::from_millis(150));
        let write_a = hold_in("lib-h", [A], Work::Write, Duration::ZERO)
            .expect("the waiting batch holds nothing");
        drop(write_a);
        drop(write_b);
        let (ids, busy) = batch.join().unwrap();
        assert_eq!(ids, vec![A, B]);
        assert!(busy.is_empty(), "{busy:?}");
    }

    #[test]
    fn a_season_or_an_episode_stands_for_its_show() {
        use cv_domain::Item;
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        let show = db
            .write(|tx| cv_db::repo::insert_item(tx, &Item::show("Lost")))
            .unwrap();
        let episode = Item::episode(show, "Pilot");
        let episode = db
            .write(move |tx| cv_db::repo::insert_item(tx, &episode))
            .unwrap();
        assert_eq!(title_of(&db, episode).unwrap(), show);
        assert_eq!(title_of(&db, show).unwrap(), show);
        let _guard = hold_items(&db, &[episode], Work::Write, Duration::ZERO).unwrap();
        assert_eq!(holder(&db, show), Some(Work::Write));
    }
}

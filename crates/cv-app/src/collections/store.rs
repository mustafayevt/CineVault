//! The library side of collections: reading a collection with its movies, deleting and
//! restoring one, and moving movies between collections when they are scraped.
//!
//! Everything here takes a connection, so the caller decides the transaction. A collection is an
//! item of kind `set`; its movies are the movies whose `parent_id` is the set, so a movie is in at
//! most one collection. The movies of a linked collection that the library does not have are kept
//! in the set's `missing_movies`, refreshed whenever the collection is fetched from the provider.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use cv_db::repo::query::MIN_OWNED_IN_COLLECTION;
use cv_db::repo::{
    children_of, delete_item, field_locks, find_set_by_title, get_item, insert_item, item_exists,
    items_by_provider_id, update_item,
};
use cv_db::{Database, DbError};
use cv_domain::{
    CalendarDate, Item, ItemDetail, ItemId, ItemKind, MetaField, MissingMovie, id_key,
};
use cv_providers::model::ProviderCollection;
use rusqlite::Connection;

use super::CollectionError;
use super::model::{CollectionEntry, CollectionPage, CollectionSummary};

/// The longest collection name accepted, in characters.
pub const MAX_TITLE_LEN: usize = 200;

pub(crate) type StoreResult<T> = Result<T, CollectionError>;

/// Runs `work` in one write transaction. A refusal (`Invalid`, `NotFound`) rolls everything it
/// wrote back, like a database error does.
pub(crate) fn write<T: Send + 'static>(
    db: &Database,
    work: impl FnOnce(&Connection) -> StoreResult<T> + Send + 'static,
) -> StoreResult<T> {
    let refusal: Arc<Mutex<Option<CollectionError>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&refusal);
    let result = db.write(move |tx| match work(tx) {
        Ok(value) => Ok(value),
        Err(CollectionError::Db(error)) => Err(error),
        Err(other) => {
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(other);
            Err(DbError::InvalidData(
                "the collection change was refused".to_owned(),
            ))
        }
    });
    match result {
        Ok(value) => Ok(value),
        Err(error) => Err(refusal
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .unwrap_or(CollectionError::Db(error))),
    }
}

/// The set with this ID, or an error when the item is missing or is not a collection.
pub(crate) fn load_set(conn: &Connection, id: ItemId) -> StoreResult<Item> {
    match get_item(conn, id)? {
        Some(item) if item.kind() == ItemKind::Set => Ok(item),
        _ => Err(CollectionError::NotFound),
    }
}

fn set_fields_mut(item: &mut Item) -> Option<&mut cv_domain::SetFields> {
    match &mut item.detail {
        ItemDetail::Set(fields) => Some(fields),
        _ => None,
    }
}

/// The movies of a collection, in id order.
pub(crate) fn members(conn: &Connection, set: ItemId) -> StoreResult<Vec<Item>> {
    let mut movies = Vec::new();
    for id in children_of(conn, set)? {
        if let Some(item) = get_item(conn, id)?
            && item.kind() == ItemKind::Movie
        {
            movies.push(item);
        }
    }
    Ok(movies)
}

/// The TMDB collection a collection is linked to: its own `tmdb` ID (a movie carries the ID of
/// its collection as `tmdbSet`).
fn set_tmdb_id(item: &Item) -> Option<String> {
    item.core
        .ids
        .get(id_key::TMDB)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// The collection linked to this TMDB collection ID, if the library has one.
pub(crate) fn find_set_by_tmdb(conn: &Connection, tmdb_set: &str) -> StoreResult<Option<ItemId>> {
    for id in items_by_provider_id(conn, id_key::TMDB, tmdb_set)? {
        if let Some(item) = get_item(conn, id)?
            && item.kind() == ItemKind::Set
        {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

// ---- reading ---------------------------------------------------------------------------------

/// The collections the library lists, with their counts, sorted by name ignoring case. A
/// collection with fewer than [`MIN_OWNED_IN_COLLECTION`] owned movies is kept but not listed.
pub(crate) fn summaries(conn: &Connection) -> StoreResult<Vec<CollectionSummary>> {
    let mut stmt = conn
        .prepare_cached(&format!(
            "SELECT s.id, s.title,
                    (SELECT value FROM ext_id e WHERE e.item_id = s.id AND e.provider = ?1),
                    (SELECT COUNT(*) FROM item c WHERE c.parent_id = s.id AND c.kind = 'movie'),
                    COALESCE(json_array_length(s.ext, '$.missingMovies'), 0)
               FROM item s WHERE s.kind = 'set'
                AND (SELECT COUNT(*) FROM item c WHERE c.parent_id = s.id AND c.kind = 'movie')
                    >= {MIN_OWNED_IN_COLLECTION}
              ORDER BY s.title COLLATE NOCASE, s.id"
        ))
        .map_err(DbError::from)?;
    let count = |n: i64| u32::try_from(n).unwrap_or(0);
    let rows = stmt
        .query_map([id_key::TMDB], |r| {
            Ok(CollectionSummary {
                id: ItemId(r.get(0)?),
                title: r.get(1)?,
                tmdb_id: r.get(2)?,
                owned: count(r.get(3)?),
                missing: count(r.get(4)?),
            })
        })
        .map_err(DbError::from)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(DbError::from)?;
    Ok(rows)
}

/// The movies that are in no collection and have a TMDB or IMDb ID, so a provider can be asked
/// about them, in id order.
pub(crate) fn ungrouped_movies(conn: &Connection) -> StoreResult<Vec<ItemId>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT i.id FROM item i
              WHERE i.kind = 'movie' AND i.parent_id IS NULL
                AND EXISTS (SELECT 1 FROM ext_id e WHERE e.item_id = i.id
                               AND e.provider IN ('tmdb', 'imdb'))
              ORDER BY i.id",
        )
        .map_err(DbError::from)?;
    let ids = stmt
        .query_map([], |r| r.get::<_, i64>(0).map(ItemId))
        .map_err(DbError::from)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(DbError::from)?;
    Ok(ids)
}

/// Whether a movie has come out by `today`. A movie with no date is released when its year has
/// begun; one with neither is not announced yet.
pub(crate) fn is_released(movie: &MissingMovie, today: CalendarDate) -> bool {
    match movie.release_date {
        Some(date) => date <= today,
        None => movie.year != 0 && i32::from(movie.year) <= today.year(),
    }
}

fn same_title(a: &str, b: &str) -> bool {
    !a.trim().is_empty() && a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Whether the library already has `missing` among `owned`: by the TMDB or IMDb ID, or, for a
/// movie without IDs, by title and year.
fn is_owned(missing: &MissingMovie, owned: &[Item]) -> bool {
    owned.iter().any(|movie| {
        let ids = &movie.core.ids;
        let by_id = [id_key::TMDB, id_key::IMDB].iter().any(|key| {
            missing.ids.get(key).is_some_and(|wanted| {
                !wanted.is_empty() && ids.get(key).is_some_and(|have| have == wanted)
            })
        });
        by_id
            || (same_title(&missing.title, &movie.core.title)
                && (missing.year == 0 || movie.core.year == 0 || missing.year == movie.core.year))
    })
}

/// The movies of a provider collection that the library lacks and that have come out.
pub(crate) fn missing_of(
    listed: &[MissingMovie],
    owned: &[Item],
    today: CalendarDate,
) -> Vec<MissingMovie> {
    listed
        .iter()
        .filter(|m| is_released(m, today) && !is_owned(m, owned))
        .cloned()
        .collect()
}

type SortKey = (bool, i32, u8, u8, String);

fn sort_key(date: Option<CalendarDate>, year: u16, title: &str) -> SortKey {
    let (unknown, y, m, d) = match date {
        Some(date) => (false, date.year(), date.month(), date.day()),
        None if year != 0 => (false, i32::from(year), 0, 0),
        None => (true, 0, 0, 0),
    };
    (unknown, y, m, d, title.to_lowercase())
}

/// The collection with its movies in release order. Missing movies are the stored ones that the
/// library does not have after all and that have come out by `today`.
pub(crate) fn page(
    conn: &Connection,
    id: ItemId,
    today: CalendarDate,
) -> StoreResult<CollectionPage> {
    let set = load_set(conn, id)?;
    let owned = members(conn, id)?;
    let listed: &[MissingMovie] = match &set.detail {
        ItemDetail::Set(fields) => &fields.missing_movies,
        _ => &[],
    };
    let missing = missing_of(listed, &owned, today);

    let mut entries: Vec<(SortKey, CollectionEntry)> = Vec::new();
    for movie in &owned {
        let ItemDetail::Movie(fields) = &movie.detail else {
            continue;
        };
        let entry = CollectionEntry {
            id: movie.id,
            title: movie.core.title.clone(),
            year: movie.core.year,
            release_date: fields.release_date,
            watched: fields.watched,
            poster_url: String::new(),
            tmdb_id: movie.core.ids.get(id_key::TMDB).map(str::to_owned),
        };
        entries.push((
            sort_key(fields.release_date, movie.core.year, &movie.core.title),
            entry,
        ));
    }
    for movie in &missing {
        let entry = CollectionEntry {
            id: None,
            title: movie.title.clone(),
            year: movie.year,
            release_date: movie.release_date,
            watched: false,
            poster_url: movie.poster_url.clone(),
            tmdb_id: movie.ids.get(id_key::TMDB).map(str::to_owned),
        };
        entries.push((
            sort_key(movie.release_date, movie.year, &movie.title),
            entry,
        ));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let owned_count = u32::try_from(owned.len()).unwrap_or(u32::MAX);
    let missing_count = u32::try_from(missing.len()).unwrap_or(u32::MAX);
    let sort_title = match &set.detail {
        ItemDetail::Set(fields) => fields.sort_title.clone(),
        _ => String::new(),
    };
    Ok(CollectionPage {
        id,
        tmdb_id: set_tmdb_id(&set),
        title: set.core.title,
        sort_title,
        plot: set.core.plot,
        entries: entries.into_iter().map(|(_, entry)| entry).collect(),
        owned: owned_count,
        total: owned_count + missing_count,
    })
}

/// The movies of a collection in play order (release order), as the naming tokens need them: the
/// IDs of the movies in the library, and the same list with a negative placeholder for every
/// released movie the collection lacks, in the place it would take.
pub(crate) fn play_order(
    conn: &Connection,
    id: ItemId,
    today: CalendarDate,
) -> StoreResult<(Vec<i64>, Vec<i64>)> {
    let page = page(conn, id, today)?;
    let mut owned = Vec::new();
    let mut with_dummy = Vec::new();
    let mut dummy = 0_i64;
    for entry in page.entries {
        match entry.id {
            Some(movie) => {
                owned.push(movie.0);
                with_dummy.push(movie.0);
            }
            None => {
                dummy -= 1;
                with_dummy.push(dummy);
            }
        }
    }
    Ok((owned, with_dummy))
}

// ---- creating and editing --------------------------------------------------------------------

fn clean_title(title: &str) -> StoreResult<String> {
    let title = title.trim();
    if title.is_empty() {
        return Err(CollectionError::Invalid(
            "A collection needs a name.".to_owned(),
        ));
    }
    if title.chars().count() > MAX_TITLE_LEN {
        return Err(CollectionError::Invalid(format!(
            "A collection name is at most {MAX_TITLE_LEN} characters."
        )));
    }
    Ok(title.to_owned())
}

fn ensure_name_free(conn: &Connection, title: &str, except: Option<ItemId>) -> StoreResult<()> {
    match find_set_by_title(conn, title)? {
        Some(other) if Some(other) != except => Err(CollectionError::Invalid(format!(
            "There already is a collection called \"{title}\"."
        ))),
        _ => Ok(()),
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The collection a movie NFO names in its `<set>`: the one linked to the NFO's TMDB collection
/// ID, else the one with that name, else a new one that takes the NFO's name, overview and ID.
/// A found collection is not changed.
pub(crate) fn ensure_named(
    conn: &Connection,
    title: &str,
    overview: &str,
    tmdb_id: Option<u64>,
) -> StoreResult<ItemId> {
    let title = clean_title(title)?;
    let tmdb = tmdb_id.filter(|id| *id > 0).map(|id| id.to_string());
    if let Some(tmdb) = &tmdb
        && let Some(id) = find_set_by_tmdb(conn, tmdb)?
    {
        return Ok(id);
    }
    if let Some(id) = find_set_by_title(conn, &title)? {
        return Ok(id);
    }
    let mut set = Item::set(title);
    set.core.date_added_ms = now_ms();
    set.core.plot = overview.trim().to_owned();
    if let Some(tmdb) = tmdb {
        set.core.ids.set(id_key::TMDB, tmdb);
    }
    Ok(insert_item(conn, &set)?)
}

// ---- membership ------------------------------------------------------------------------------

/// A movie that changed collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Moved {
    pub movie: ItemId,
    pub previous: Option<ItemId>,
}

/// What an assignment did.
#[derive(Debug, Default)]
pub(crate) struct AssignOutcome {
    pub moved: Vec<Moved>,
    pub unchanged: u32,
    pub locked: u32,
    pub not_movies: u32,
}

/// Puts `movies` into `target`, or takes them out of any collection (`None`).
///
/// With `respect_locks`, a movie whose item lock or collection field lock is on stays where it
/// is: that is what an automatic assignment does. An explicit choice by the user passes `false`.
pub(crate) fn assign(
    conn: &Connection,
    movies: &[ItemId],
    target: Option<ItemId>,
    respect_locks: bool,
) -> StoreResult<AssignOutcome> {
    if let Some(target) = target {
        load_set(conn, target)?;
    }
    let mut outcome = AssignOutcome::default();
    let mut seen = HashSet::new();
    for id in movies {
        if !seen.insert(*id) {
            continue;
        }
        let Some(mut movie) = get_item(conn, *id)? else {
            continue;
        };
        if movie.kind() != ItemKind::Movie {
            outcome.not_movies += 1;
            continue;
        }
        if respect_locks
            && (movie.core.locked || field_locks(conn, *id)?.contains(MetaField::Collection))
        {
            outcome.locked += 1;
            continue;
        }
        if movie.parent_id == target {
            outcome.unchanged += 1;
            continue;
        }
        let previous = movie.parent_id;
        movie.parent_id = target;
        update_item(conn, &movie)?;
        outcome.moved.push(Moved {
            movie: *id,
            previous,
        });
    }
    if let Some(target) = target {
        drop_owned_from_missing(conn, target)?;
    }
    Ok(outcome)
}

/// Takes the movies the library has off the set's missing list.
fn drop_owned_from_missing(conn: &Connection, set_id: ItemId) -> StoreResult<()> {
    let mut set = load_set(conn, set_id)?;
    let owned = members(conn, set_id)?;
    let Some(fields) = set_fields_mut(&mut set) else {
        return Ok(());
    };
    let before = fields.missing_movies.len();
    fields.missing_movies.retain(|m| !is_owned(m, &owned));
    if fields.missing_movies.len() != before {
        update_item(conn, &set)?;
    }
    Ok(())
}

/// Sets each movie's collection back to what `before` records. Movies that are gone are skipped.
/// Returns how many were put back.
pub(crate) fn restore_parents(
    conn: &Connection,
    before: &[(ItemId, Option<ItemId>)],
) -> StoreResult<u32> {
    let mut restored = 0;
    for (id, previous) in before {
        let Some(mut movie) = get_item(conn, *id)? else {
            continue;
        };
        let previous = match previous {
            Some(set) if item_exists(conn, *set)? => Some(*set),
            _ => None,
        };
        if movie.parent_id != previous {
            movie.parent_id = previous;
            update_item(conn, &movie)?;
            restored += 1;
        }
    }
    Ok(restored)
}

/// Deletes the sets in `ids` that have no movies left (a collection a find run created, for
/// its undo). Returns how many were deleted.
pub(crate) fn delete_empty_sets(conn: &Connection, ids: &[ItemId]) -> StoreResult<u32> {
    let mut deleted = 0;
    for id in ids {
        if item_exists(conn, *id)? && children_of(conn, *id)?.is_empty() {
            delete_item(conn, *id)?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

// ---- delete and restore ----------------------------------------------------------------------

/// A deleted collection, kept so it can be restored.
#[derive(Debug, Clone)]
pub(crate) struct DeletedSet {
    pub set: Item,
    pub members: Vec<ItemId>,
}

/// Deletes a collection. Its movies stay in the library without a collection.
pub(crate) fn delete(conn: &Connection, id: ItemId) -> StoreResult<DeletedSet> {
    let set = load_set(conn, id)?;
    let members = children_of(conn, id)?;
    delete_item(conn, id)?;
    Ok(DeletedSet { set, members })
}

/// Puts a deleted collection back, with its movies that have no collection now. Returns the new
/// collection and how many movies went back into it.
pub(crate) fn restore(conn: &Connection, deleted: &DeletedSet) -> StoreResult<(ItemId, u32)> {
    let mut set = deleted.set.clone();
    set.id = None;
    set.parent_id = None;
    // A collection with the same name was made in the meantime: the restored one gets a clear name.
    let mut title = set.core.title.clone();
    let mut n = 2;
    while find_set_by_title(conn, &title)?.is_some() {
        title = format!("{} ({n})", deleted.set.core.title);
        n += 1;
    }
    set.core.title = title;
    let id = insert_item(conn, &set)?;
    let mut back = 0;
    for member in &deleted.members {
        let Some(mut movie) = get_item(conn, *member)? else {
            continue;
        };
        if movie.parent_id.is_none() {
            movie.parent_id = Some(id);
            update_item(conn, &movie)?;
            back += 1;
        }
    }
    Ok((id, back))
}

// ---- provider collections --------------------------------------------------------------------

fn apply_remote(set: &mut Item, remote: &ProviderCollection, owned: &[Item], today: CalendarDate) {
    if let Ok(tmdb) = remote_id(remote) {
        set.core.ids.set(id_key::TMDB, tmdb);
    }
    if set.core.plot.trim().is_empty() {
        set.core.plot.clone_from(&remote.item.core.plot);
    }
    for (kind, url) in &remote.item.core.artwork_urls {
        set.core.artwork_urls.insert(*kind, url.clone());
    }
    if let Some(fields) = set_fields_mut(set) {
        fields.missing_movies = missing_of(&remote.movies, owned, today);
    }
}

/// The ID of a provider collection (the provider names it `tmdbSet`).
fn remote_id(remote: &ProviderCollection) -> StoreResult<String> {
    let ids = &remote.item.core.ids;
    ids.get(id_key::TMDB_SET)
        .or_else(|| ids.get(id_key::TMDB))
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CollectionError::Invalid("The provider collection has no ID.".to_owned()))
}

/// The collection for a provider collection, created when the library has none: the one linked
/// to it, else the one with the same name (which gets linked), else a new one. The set's plot (when
/// empty), artwork URLs and missing movies are brought up to date. Returns the set and whether it
/// was created.
pub(crate) fn ensure_from_remote(
    conn: &Connection,
    remote: &ProviderCollection,
    today: CalendarDate,
) -> StoreResult<(ItemId, bool)> {
    let tmdb = remote_id(remote)?;
    let by_id = find_set_by_tmdb(conn, &tmdb)?;
    let existing = match by_id {
        Some(id) => Some(id),
        None => match find_set_by_title(conn, &remote.item.core.title)? {
            Some(id) if set_tmdb_id(&load_set(conn, id)?).is_none() => Some(id),
            _ => None,
        },
    };
    if let Some(id) = existing {
        let mut set = load_set(conn, id)?;
        let owned = members(conn, id)?;
        apply_remote(&mut set, remote, &owned, today);
        if by_id.is_some() {
            take_remote_name(conn, id, &mut set, remote)?;
        }
        update_item(conn, &set)?;
        return Ok((id, false));
    }
    let mut set = Item::set(remote.item.core.title.trim());
    set.core.date_added_ms = now_ms();
    apply_remote(&mut set, remote, &[], today);
    Ok((insert_item(conn, &set)?, true))
}

/// The collection linked to a provider collection by its ID takes the provider's name (and, with
/// a new name, its plot), so a name read from an NFO that belongs to another collection ("Psycho
/// Collection" on the Heat Collection) is put right. A name or plot the user gave by hand (a
/// locked field) stays, and so does the name when another collection already has the new one.
fn take_remote_name(
    conn: &Connection,
    id: ItemId,
    set: &mut Item,
    remote: &ProviderCollection,
) -> StoreResult<()> {
    let name = remote.item.core.title.trim();
    if name.is_empty() || name.eq_ignore_ascii_case(set.core.title.trim()) {
        return Ok(());
    }
    let locks = cv_db::repo::field_locks(conn, id)?;
    if locks.contains(MetaField::Title) || ensure_name_free(conn, name, Some(id)).is_err() {
        return Ok(());
    }
    set.core.title = name.to_owned();
    let plot = remote.item.core.plot.trim();
    if !locks.contains(MetaField::Plot) && !plot.is_empty() {
        set.core.plot = plot.to_owned();
    }
    Ok(())
}

/// Takes a movie out of its collection when that collection is a provider collection (it has a
/// TMDB ID) and the provider now lists no collection for the movie, as after a change of match.
/// A collection made by hand keeps the movie. Returns the collection the movie left.
pub(crate) fn leave_provider_set(conn: &Connection, movie: ItemId) -> StoreResult<Option<ItemId>> {
    let Some(mut item) = get_item(conn, movie)? else {
        return Ok(None);
    };
    let Some(parent) = item.parent_id else {
        return Ok(None);
    };
    let Ok(set) = load_set(conn, parent) else {
        return Ok(None);
    };
    if set_tmdb_id(&set).is_none() {
        return Ok(None);
    }
    item.parent_id = None;
    update_item(conn, &item)?;
    Ok(Some(parent))
}

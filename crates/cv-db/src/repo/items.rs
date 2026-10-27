//! Item CRUD: whole [`Item`]s with every related row.
//!
//! The `item` row holds the filterable columns; rarely queried nested data lives in its `ext`
//! JSON column, and lists (files, credits, ratings, IDs, tags, genres, artwork URLs, trailers,
//! episode numbers) in their own tables (see [`super::children`]). `insert_item` and
//! `update_item` write all of it and refresh the full-text index in the same call, so run them
//! inside [`Database::write`](crate::Database::write) to make the change atomic.
//!
//! Fields owned by other parts of the system (the NEW flag, review state, availability) are
//! never touched by `update_item`; they have their own setters.

use std::sync::LazyLock;

use cv_domain::{
    CalendarDate, Certification, Edition, EpisodeFields, Item, ItemCore, ItemDetail, ItemId,
    ItemKind, MediaSource, MovieFields, SeasonFields, SetFields, ShowFields,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, ToSql, params};

use super::codec::{Ext, enum_text, group_text, parse_enum, parse_group, to_u16, to_u32};
use super::query::ItemSummary;
use super::{children, fts};
use crate::error::{DbError, DbResult};

/// Columns written from an [`Item`], in a fixed order. `kind` is written on insert only.
const COLUMNS: &[&str] = &[
    "parent_id",
    "title",
    "original_title",
    "original_language",
    "english_title",
    "sort_title",
    "year",
    "plot",
    "outline",
    "tagline",
    "note",
    "runtime_minutes",
    "certification",
    "release_date",
    "top250",
    "production_companies",
    "data_source",
    "path",
    "original_filename",
    "date_added_ms",
    "locked",
    "last_scraper_id",
    "last_scrape_language",
    "rename_history",
    "watched",
    "play_count",
    "last_watched_ms",
    "media_source",
    "edition",
    "is_disc",
    "stacked",
    "video_in_3d",
    "multi_movie_dir",
    "year_end",
    "aired_status",
    "country",
    "episode_group",
    "season_number",
    "multi_episode",
    "ext",
];

static INSERT_SQL: LazyLock<String> = LazyLock::new(|| {
    let names = COLUMNS.join(", ");
    let marks = COLUMNS
        .iter()
        .map(|c| format!(":{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO item (kind, {names}) VALUES (:kind, {marks})")
});

static UPDATE_SQL: LazyLock<String> = LazyLock::new(|| {
    let sets = COLUMNS
        .iter()
        .map(|c| format!("{c} = :{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "UPDATE item SET {sets}, updated_ms = CAST(unixepoch('subsec') * 1000 AS INTEGER) \
         WHERE id = :id AND kind = :kind"
    )
});

/// The kind-specific column values of an item.
struct KindColumns {
    english_title: String,
    sort_title: String,
    tagline: String,
    runtime_minutes: u32,
    certification: String,
    release_date: Option<String>,
    top250: u32,
    watched: bool,
    play_count: u32,
    last_watched_ms: Option<i64>,
    media_source: String,
    edition: String,
    is_disc: bool,
    stacked: bool,
    video_in_3d: bool,
    multi_movie_dir: bool,
    year_end: u16,
    aired_status: String,
    country: String,
    episode_group: String,
    season_number: Option<u32>,
    multi_episode: bool,
    ext: Ext,
}

impl Default for KindColumns {
    fn default() -> Self {
        Self {
            english_title: String::new(),
            sort_title: String::new(),
            tagline: String::new(),
            runtime_minutes: 0,
            certification: String::new(),
            release_date: None,
            top250: 0,
            watched: false,
            play_count: 0,
            last_watched_ms: None,
            media_source: String::new(),
            edition: String::new(),
            is_disc: false,
            stacked: false,
            video_in_3d: false,
            multi_movie_dir: false,
            year_end: 0,
            aired_status: "unknown".into(),
            country: String::new(),
            episode_group: "aired".into(),
            season_number: None,
            multi_episode: false,
            ext: Ext::default(),
        }
    }
}

/// `Edition::None` and `MediaSource::Unknown` are stored as empty text so "unset" filters work.
fn edition_text(e: &Edition) -> String {
    if *e == Edition::None {
        String::new()
    } else {
        e.name().to_owned()
    }
}

fn media_source_text(m: &MediaSource) -> String {
    if *m == MediaSource::Unknown {
        String::new()
    } else {
        m.name().to_owned()
    }
}

fn kind_columns(detail: &ItemDetail) -> DbResult<KindColumns> {
    let mut k = KindColumns::default();
    match detail {
        ItemDetail::Movie(m) => {
            k.english_title = m.english_title.clone();
            k.sort_title = m.sort_title.clone();
            k.tagline = m.tagline.clone();
            k.runtime_minutes = m.runtime_minutes;
            k.certification = m.certification.0.clone();
            k.release_date = m.release_date.map(|d| d.to_string());
            k.top250 = m.top250;
            k.watched = m.watched;
            k.play_count = m.play_count;
            k.last_watched_ms = m.last_watched_ms;
            k.media_source = media_source_text(&m.media_source);
            k.edition = edition_text(&m.edition);
            k.is_disc = m.is_disc;
            k.stacked = m.stacked;
            k.video_in_3d = m.video_in_3d;
            k.multi_movie_dir = m.multi_movie_dir;
            k.ext.spoken_languages = m.spoken_languages.clone();
            k.ext.countries = m.countries.clone();
            k.ext.extra_thumbs = m.extra_thumbs.clone();
            k.ext.extra_fanarts = m.extra_fanarts.clone();
            k.ext.show_links = m.show_links.clone();
        }
        ItemDetail::Set(s) => {
            k.english_title = s.english_title.clone();
            k.sort_title = s.sort_title.clone();
            k.ext.missing_movies = s.missing_movies.clone();
        }
        ItemDetail::Show(s) => {
            k.english_title = s.english_title.clone();
            k.sort_title = s.sort_title.clone();
            k.tagline = s.tagline.clone();
            k.runtime_minutes = s.runtime_minutes;
            k.certification = s.certification.0.clone();
            k.release_date = s.first_aired.map(|d| d.to_string());
            k.top250 = s.top250;
            k.year_end = s.year_end;
            k.aired_status = enum_text(&s.status)?;
            k.country = s.country.clone();
            k.episode_group = group_text(&s.episode_group)?;
            k.ext.episode_groups = s.episode_groups.clone();
            k.ext.extra_fanart_urls = s.extra_fanart_urls.clone();
            k.ext.season_texts = s.season_texts.clone();
            k.ext.missing_episodes = s.missing_episodes.clone();
            k.ext.episode_list_fetched = s.episode_list_fetched;
        }
        ItemDetail::Season(s) => {
            k.season_number = Some(s.season);
        }
        ItemDetail::Episode(e) => {
            k.english_title = e.english_title.clone();
            k.runtime_minutes = e.runtime_minutes;
            k.release_date = e.first_aired.map(|d| d.to_string());
            k.watched = e.watched;
            k.play_count = e.play_count;
            k.last_watched_ms = e.last_watched_ms;
            k.media_source = media_source_text(&e.media_source);
            k.edition = edition_text(&e.edition);
            k.is_disc = e.is_disc;
            k.stacked = e.stacked;
            k.multi_episode = e.multi_episode;
        }
    }
    Ok(k)
}

/// Named parameters for [`INSERT_SQL`] / [`UPDATE_SQL`] (without `:kind` and `:id`).
fn column_params(item: &Item) -> DbResult<Vec<(String, Value)>> {
    let c = &item.core;
    let k = kind_columns(&item.detail)?;
    let int = |v: i64| Value::Integer(v);
    let text = Value::Text;
    let flag = |v: bool| Value::Integer(i64::from(v));
    let opt_text = |v: Option<String>| v.map_or(Value::Null, Value::Text);
    let opt_int = |v: Option<i64>| v.map_or(Value::Null, Value::Integer);

    let rename_history = match &c.rename_history {
        Some(h) => Value::Text(serde_json::to_string(h)?),
        None => Value::Null,
    };
    let ext = serde_json::to_string(&k.ext)?;

    let values: Vec<(&str, Value)> = vec![
        ("parent_id", opt_int(item.parent_id.map(|p| p.0))),
        ("title", text(c.title.clone())),
        ("original_title", text(c.original_title.clone())),
        // Only a movie or a show owns the value; an episode reads its show's.
        (
            "original_language",
            text(if matches!(item.kind(), ItemKind::Movie | ItemKind::Show) {
                c.original_language.clone()
            } else {
                String::new()
            }),
        ),
        ("english_title", text(k.english_title)),
        ("sort_title", text(k.sort_title)),
        ("year", int(i64::from(c.year))),
        ("plot", text(c.plot.clone())),
        ("outline", text(c.outline.clone())),
        ("tagline", text(k.tagline)),
        ("note", text(c.note.clone())),
        ("runtime_minutes", int(i64::from(k.runtime_minutes))),
        ("certification", text(k.certification)),
        ("release_date", opt_text(k.release_date)),
        ("top250", int(i64::from(k.top250))),
        (
            "production_companies",
            text(serde_json::to_string(&c.production_companies)?),
        ),
        ("data_source", text(c.data_source.clone())),
        ("path", text(c.path.clone())),
        ("original_filename", text(c.original_filename.clone())),
        ("date_added_ms", int(c.date_added_ms)),
        ("locked", flag(c.locked)),
        ("last_scraper_id", text(c.last_scraper_id.clone())),
        ("last_scrape_language", text(c.last_scrape_language.clone())),
        ("rename_history", rename_history),
        ("watched", flag(k.watched)),
        ("play_count", int(i64::from(k.play_count))),
        ("last_watched_ms", opt_int(k.last_watched_ms)),
        ("media_source", text(k.media_source)),
        ("edition", text(k.edition)),
        ("is_disc", flag(k.is_disc)),
        ("stacked", flag(k.stacked)),
        ("video_in_3d", flag(k.video_in_3d)),
        ("multi_movie_dir", flag(k.multi_movie_dir)),
        ("year_end", int(i64::from(k.year_end))),
        ("aired_status", text(k.aired_status)),
        ("country", text(k.country)),
        ("episode_group", text(k.episode_group)),
        ("season_number", opt_int(k.season_number.map(i64::from))),
        ("multi_episode", flag(k.multi_episode)),
        ("ext", text(ext)),
    ];
    debug_assert_eq!(values.len(), COLUMNS.len());
    debug_assert!(values.iter().zip(COLUMNS).all(|((n, _), c)| n == c));
    Ok(values
        .into_iter()
        .map(|(name, v)| (format!(":{name}"), v))
        .collect())
}

fn as_named(params: &[(String, Value)]) -> Vec<(&str, &dyn ToSql)> {
    params
        .iter()
        .map(|(n, v)| (n.as_str(), v as &dyn ToSql))
        .collect()
}

/// Stores a new item with all its related rows and indexes it for full-text search.
///
/// A new id is assigned; `item.id` is ignored. Returns that id.
pub fn insert_item(conn: &Connection, item: &Item) -> DbResult<ItemId> {
    let mut named = column_params(item)?;
    named.push((":kind".into(), Value::Text(enum_text(&item.kind())?)));
    conn.prepare_cached(&INSERT_SQL)?
        .execute(&*as_named(&named))?;
    let id = conn.last_insert_rowid();
    children::replace_all(conn, id, item, false)?;
    fts::reindex_item(conn, ItemId(id))?;
    Ok(ItemId(id))
}

/// Overwrites a stored item (and all its related rows) with `item`.
///
/// The item's kind cannot change. The NEW flag, review state, availability and the user rating
/// are left alone (the user rating changes only through [`super::ratings::set_user_rating`]).
pub fn update_item(conn: &Connection, item: &Item) -> DbResult<()> {
    let id = item.id.ok_or(DbError::ItemWithoutId)?;
    let mut named = column_params(item)?;
    named.push((":id".into(), Value::Integer(id.0)));
    named.push((":kind".into(), Value::Text(enum_text(&item.kind())?)));
    let changed = conn
        .prepare_cached(&UPDATE_SQL)?
        .execute(&*as_named(&named))?;
    if changed == 0 {
        return Err(if item_exists(conn, id)? {
            DbError::InvalidData(format!("item {} cannot change its kind", id.0))
        } else {
            DbError::ItemNotFound(id.0)
        });
    }
    children::replace_all(conn, id.0, item, true)?;
    fts::reindex_item(conn, id)?;
    Ok(())
}

/// True when an item with this id exists.
pub fn item_exists(conn: &Connection, id: ItemId) -> DbResult<bool> {
    Ok(conn
        .prepare_cached("SELECT 1 FROM item WHERE id = ?1")?
        .exists([id.0])?)
}

/// Adds a provider ID (an IMDb `nm` ID found later) to the people with this TMDB person ID.
pub fn add_person_id(
    conn: &Connection,
    tmdb_person: &str,
    key: &str,
    value: &str,
) -> DbResult<usize> {
    children::add_person_id(conn, tmdb_person, key, value)
}

/// Loads an item with all its related rows.
pub fn get_item(conn: &Connection, id: ItemId) -> DbResult<Option<Item>> {
    let mut stmt = conn.prepare_cached("SELECT * FROM item WHERE id = ?1")?;
    let mut rows = stmt.query([id.0])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let mut item = read_item_row(row)?;
    drop(rows);
    drop(stmt);
    children::load_into(conn, id.0, &mut item)?;
    // A season or an episode reads its show's original language.
    if let (ItemKind::Season | ItemKind::Episode, Some(parent)) = (item.kind(), item.parent_id) {
        item.core.original_language = original_language_of(conn, parent)?;
    }
    Ok(Some(item))
}

/// The original language stored for a movie or a show (empty when unknown); for a season or an
/// episode, its show's.
pub fn original_language_of(conn: &Connection, id: ItemId) -> DbResult<String> {
    let found: Option<(String, String, Option<i64>)> = conn
        .prepare_cached("SELECT kind, original_language, parent_id FROM item WHERE id = ?1")?
        .query_row([id.0], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    match found {
        Some((kind, _, Some(parent))) if kind == "season" || kind == "episode" => conn
            .prepare_cached("SELECT original_language FROM item WHERE id = ?1")?
            .query_row([parent], |r| r.get(0))
            .optional()
            .map(Option::unwrap_or_default)
            .map_err(Into::into),
        Some((_, language, _)) => Ok(language),
        None => Ok(String::new()),
    }
}

/// Loads the list-row view of an item (no related rows).
pub fn get_summary(conn: &Connection, id: ItemId) -> DbResult<Option<ItemSummary>> {
    let sql = format!("SELECT {} FROM item WHERE id = ?1", ItemSummary::COLUMNS);
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query([id.0])?;
    match rows.next()? {
        Some(row) => Ok(Some(ItemSummary::from_row(row)?)),
        None => Ok(None),
    }
}

/// The ids of an item's direct children (seasons of a show, episodes of a season, movies of a
/// set), in id order.
pub fn children_of(conn: &Connection, parent: ItemId) -> DbResult<Vec<ItemId>> {
    let mut stmt = conn.prepare_cached("SELECT id FROM item WHERE parent_id = ?1 ORDER BY id")?;
    let ids = stmt
        .query_map([parent.0], |r| r.get::<_, i64>(0).map(ItemId))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// The first collection (movie set) whose title is `title`, ignoring case.
pub fn find_set_by_title(conn: &Connection, title: &str) -> DbResult<Option<ItemId>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id FROM item WHERE kind = 'set' AND title = ?1 COLLATE NOCASE ORDER BY id LIMIT 1",
    )?;
    let mut rows = stmt.query_map([title], |r| r.get::<_, i64>(0).map(ItemId))?;
    Ok(rows.next().transpose()?)
}

/// The titles of all collections (movie sets), sorted ignoring case.
pub fn set_titles(conn: &Connection) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT title FROM item WHERE kind = 'set' ORDER BY title COLLATE NOCASE",
    )?;
    let titles = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(titles)
}

/// Items carrying the given provider ID, for example `("imdb", "tt0078748")`.
pub fn items_by_provider_id(
    conn: &Connection,
    provider: &str,
    value: &str,
) -> DbResult<Vec<ItemId>> {
    let mut stmt = conn.prepare_cached(
        "SELECT item_id FROM ext_id WHERE provider = ?1 AND value = ?2 ORDER BY item_id",
    )?;
    let ids = stmt
        .query_map(params![provider, value], |r| r.get::<_, i64>(0).map(ItemId))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// Deletes one item with its files, credits, ratings and other related rows.
///
/// This is an explicit user action, never part of loading or scanning. Deleting a movie set
/// detaches its member movies (they stay in the library). An item that still has other children
/// (a show with seasons, a season with episodes) is refused with [`DbError::HasChildren`]; use
/// [`delete_tree`] to remove those too.
pub fn delete_item(conn: &Connection, id: ItemId) -> DbResult<()> {
    let kind: String = conn
        .query_row("SELECT kind FROM item WHERE id = ?1", [id.0], |r| r.get(0))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::ItemNotFound(id.0),
            other => other.into(),
        })?;
    if kind == "set" {
        conn.execute(
            "UPDATE item SET parent_id = NULL WHERE parent_id = ?1",
            [id.0],
        )?;
    }
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM item WHERE parent_id = ?1",
        [id.0],
        |r| r.get(0),
    )?;
    if count > 0 {
        return Err(DbError::HasChildren { id: id.0, count });
    }
    conn.execute("DELETE FROM item WHERE id = ?1", [id.0])?;
    Ok(())
}

/// Deletes an item and every descendant (a show with its seasons and episodes).
///
/// Returns the number of items removed. Like [`delete_item`], this is only for explicit actions.
pub fn delete_tree(conn: &Connection, id: ItemId) -> DbResult<usize> {
    if !item_exists(conn, id)? {
        return Err(DbError::ItemNotFound(id.0));
    }
    // Deepest first, so the RESTRICT on `parent_id` is never violated.
    let mut stmt = conn.prepare(
        "WITH RECURSIVE tree (id, depth) AS (
             SELECT ?1, 0
             UNION ALL
             SELECT i.id, t.depth + 1 FROM item i JOIN tree t ON i.parent_id = t.id
         )
         SELECT id FROM tree ORDER BY depth DESC, id DESC",
    )?;
    let ids = stmt
        .query_map([id.0], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for i in &ids {
        conn.execute("DELETE FROM item WHERE id = ?1", [i])?;
    }
    Ok(ids.len())
}

/// Removes people, tags and genres that no item refers to any more.
///
/// Updates leave these lookup rows behind (they are cheap and usually reused); run this from
/// housekeeping, never as part of a user-visible edit. Returns the number of rows removed.
pub fn prune_unused_lookups(conn: &Connection) -> DbResult<usize> {
    let mut removed = 0;
    removed += conn.execute(
        "DELETE FROM person WHERE id NOT IN (SELECT person_id FROM credit)",
        [],
    )?;
    removed += conn.execute(
        "DELETE FROM tag WHERE id NOT IN (SELECT tag_id FROM item_tag)",
        [],
    )?;
    removed += conn.execute(
        "DELETE FROM genre WHERE id NOT IN (SELECT genre_id FROM item_genre)",
        [],
    )?;
    Ok(removed)
}

// ---- reading ---------------------------------------------------------------------------------

fn read_item_row(row: &Row<'_>) -> DbResult<Item> {
    let kind: ItemKind = parse_enum(&row.get::<_, String>("kind")?, "item kind")?;
    let ext: Ext = serde_json::from_str(&row.get::<_, String>("ext")?)?;
    let rename_history = match row.get::<_, Option<String>>("rename_history")? {
        Some(json) => Some(serde_json::from_str(&json)?),
        None => None,
    };

    let core = ItemCore {
        locked: row.get("locked")?,
        data_source: row.get("data_source")?,
        title: row.get("title")?,
        original_title: row.get("original_title")?,
        original_language: row.get("original_language")?,
        year: to_u16(row.get("year")?),
        plot: row.get("plot")?,
        outline: row.get("outline")?,
        path: row.get("path")?,
        date_added_ms: row.get("date_added_ms")?,
        production_companies: serde_json::from_str(&row.get::<_, String>("production_companies")?)?,
        note: row.get("note")?,
        original_filename: row.get("original_filename")?,
        last_scraper_id: row.get("last_scraper_id")?,
        last_scrape_language: row.get("last_scrape_language")?,
        rename_history,
        ..ItemCore::default()
    };

    let date = |col: &str| -> DbResult<Option<CalendarDate>> {
        match row.get::<_, Option<String>>(col)? {
            Some(text) => text
                .parse()
                .map(Some)
                .map_err(|e| DbError::InvalidData(format!("{col}: {e}"))),
            None => Ok(None),
        }
    };
    let media_source = || -> DbResult<MediaSource> {
        Ok(MediaSource::parse(&row.get::<_, String>("media_source")?).unwrap_or_default())
    };
    let edition = || -> DbResult<Edition> {
        Ok(Edition::parse(&row.get::<_, String>("edition")?).unwrap_or_default())
    };
    let certification =
        || -> DbResult<Certification> { Ok(Certification(row.get::<_, String>("certification")?)) };
    let last_watched_ms: Option<i64> = row.get("last_watched_ms")?;

    let detail = match kind {
        ItemKind::Movie => ItemDetail::Movie(MovieFields {
            english_title: row.get("english_title")?,
            sort_title: row.get("sort_title")?,
            tagline: row.get("tagline")?,
            runtime_minutes: to_u32(row.get("runtime_minutes")?),
            watched: row.get("watched")?,
            play_count: to_u32(row.get("play_count")?),
            last_watched_ms,
            is_disc: row.get("is_disc")?,
            spoken_languages: ext.spoken_languages,
            countries: ext.countries,
            release_date: date("release_date")?,
            multi_movie_dir: row.get("multi_movie_dir")?,
            top250: to_u32(row.get("top250")?),
            media_source: media_source()?,
            video_in_3d: row.get("video_in_3d")?,
            certification: certification()?,
            edition: edition()?,
            stacked: row.get("stacked")?,
            extra_thumbs: ext.extra_thumbs,
            extra_fanarts: ext.extra_fanarts,
            show_links: ext.show_links,
            genres: Vec::new(),
            trailers: Vec::new(),
        }),
        ItemKind::Set => ItemDetail::Set(SetFields {
            english_title: row.get("english_title")?,
            sort_title: row.get("sort_title")?,
            missing_movies: ext.missing_movies,
        }),
        ItemKind::Show => ItemDetail::Show(ShowFields {
            english_title: row.get("english_title")?,
            sort_title: row.get("sort_title")?,
            tagline: row.get("tagline")?,
            runtime_minutes: to_u32(row.get("runtime_minutes")?),
            year_end: to_u16(row.get("year_end")?),
            first_aired: date("release_date")?,
            status: parse_enum(&row.get::<_, String>("aired_status")?, "aired status")?,
            country: row.get("country")?,
            certification: certification()?,
            episode_group: parse_group(&row.get::<_, String>("episode_group")?)?,
            episode_groups: ext.episode_groups,
            top250: to_u32(row.get("top250")?),
            extra_fanart_urls: ext.extra_fanart_urls,
            season_texts: ext.season_texts,
            missing_episodes: ext.missing_episodes,
            episode_list_fetched: ext.episode_list_fetched,
            genres: Vec::new(),
            trailers: Vec::new(),
        }),
        ItemKind::Season => ItemDetail::Season(SeasonFields {
            season: to_u32(row.get::<_, Option<i64>>("season_number")?.unwrap_or(0)),
        }),
        ItemKind::Episode => ItemDetail::Episode(EpisodeFields {
            english_title: row.get("english_title")?,
            numbers: Default::default(),
            first_aired: date("release_date")?,
            is_disc: row.get("is_disc")?,
            multi_episode: row.get("multi_episode")?,
            watched: row.get("watched")?,
            play_count: to_u32(row.get("play_count")?),
            last_watched_ms,
            media_source: media_source()?,
            edition: edition()?,
            stacked: row.get("stacked")?,
            runtime_minutes: to_u32(row.get("runtime_minutes")?),
        }),
    };

    Ok(Item {
        id: Some(ItemId(row.get("id")?)),
        parent_id: row.get::<_, Option<i64>>("parent_id")?.map(ItemId),
        core,
        detail,
    })
}

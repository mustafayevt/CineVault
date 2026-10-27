//! Sorted, filtered, keyset-paged listing of items.
//!
//! A page is requested with the last row of the previous page as an opaque [`Cursor`]; the query
//! resumes right after it using the sort columns, so page N costs the same as page 1 (no
//! `OFFSET`) and stays correct while rows are added or removed. Ties are broken by item id, so the
//! order is total and stable.
//!
//! With the default title sort and a kind filter, the first page walks the
//! `item (kind, title COLLATE NOCASE, id)` index and stops after `limit` matches.

use std::borrow::Cow;

use cv_domain::{ItemId, ItemKind, RatingPreference};
use rusqlite::types::Value;
use rusqlite::{Connection, Row, params_from_iter};
use serde::{Deserialize, Serialize};

use super::codec::{enum_text, parse_enum, to_u16, to_u32};
use super::filter::{Filter, Sql};
use crate::error::{DbError, DbResult};

/// The first video file of the item, read through one column expression.
/// Whether the row `item` carries the NEW marker. A show carries it while one of its episodes is
/// NEW too: opening the show clears its own flag but not its episodes', and Home, the sidebar
/// and the "New episodes" filter count such a show as one with new episodes.
macro_rules! new_marker {
    () => {
        "(item.is_new = 1 OR (item.kind = 'show' AND EXISTS (SELECT 1 FROM item e \
         WHERE e.parent_id = item.id AND e.kind = 'episode' AND e.is_new = 1)))"
    };
}
pub(crate) use new_marker;

macro_rules! first_video {
    ($column:literal) => {
        concat!(
            "(SELECT ",
            $column,
            " FROM media_file m WHERE m.item_id = item.id AND m.kind = 'video' \
             ORDER BY m.position, m.id LIMIT 1)"
        )
    };
}

/// A rating of one source on the 0 to 10 scale.
macro_rules! rating_of {
    ($source:literal) => {
        concat!(
            "(SELECT r.value * 10.0 / r.max_value FROM rating r \
             WHERE r.item_id = item.id AND r.source = '",
            $source,
            "')"
        )
    };
}

/// A column of the rating an item shows: the one from the preferred source, or else the first
/// available source in the fallback order (never the user's own). The "rating" and "votes"
/// columns and their sorts read it.
pub(crate) fn preferred_rating_sql(column: &str, preference: &RatingPreference) -> String {
    format!(
        "(SELECT {column} FROM rating r WHERE r.item_id = item.id AND r.source <> 'user'          AND r.max_value > 0 ORDER BY {}, r.source LIMIT 1)",
        preference.rank_sql("r.source")
    )
}

/// SQL for "the metadata of this item is complete": identified by a provider, with a plot, a year
/// and a runtime. The table's metadata column shows it and sorts by it.
pub(crate) const METADATA_COMPLETE: &str = "(item.plot <> '' AND item.year > 0 \
     AND item.runtime_minutes > 0 \
     AND EXISTS (SELECT 1 FROM ext_id e WHERE e.item_id = item.id))";

/// SQL for the number of seasons of the show `{owner}`, as the show page counts them: the
/// seasons that have an episode in the library, without the specials (season 0). An episode's
/// season is its number in the show's chosen ordering, else its aired one; an episode with
/// neither (uncategorised) has no season.
pub(crate) fn season_count_sql(owner: &str) -> String {
    let season = format!(
        "COALESCE(\
         (SELECT n.season FROM episode_number n WHERE n.item_id = c.id \
          AND (CASE WHEN n.group_name = '' OR n.ordering = 'aired' THEN n.ordering \
               ELSE n.ordering || ':' || n.group_name END) = {owner}.episode_group LIMIT 1), \
         (SELECT n.season FROM episode_number n WHERE n.item_id = c.id \
          AND n.ordering = 'aired' LIMIT 1))"
    );
    format!(
        "(SELECT COUNT(DISTINCT CASE WHEN {season} > 0 THEN {season} END) FROM item c \
         WHERE c.parent_id = {owner}.id AND c.kind = 'episode' AND c.trashed_entry IS NULL)"
    )
}

/// [`season_count_sql`] of the row `item`, for the sort and the table.
pub(crate) static SEASON_COUNT: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| season_count_sql("item"));

/// SQL for the number of episodes of the show row `owner`: one per season and episode number
/// in the show's ordering (else the aired one), so a second file of an episode (a REPACK next
/// to the original) counts once and an episode without a number (uncategorised) not at all,
/// as on the show page.
pub(crate) fn episode_count_sql(owner: &str) -> String {
    let number = |filter: &str| {
        format!(
            "(SELECT n.season || 'x' || n.episode FROM episode_number n \
             WHERE n.item_id = c.id AND {filter} LIMIT 1)"
        )
    };
    let active = number(&format!(
        "(CASE WHEN n.group_name = '' OR n.ordering = 'aired' THEN n.ordering \
         ELSE n.ordering || ':' || n.group_name END) = {owner}.episode_group"
    ));
    let aired = number("n.ordering = 'aired'");
    format!(
        "(SELECT COUNT(DISTINCT COALESCE({active}, {aired})) FROM item c \
         WHERE c.parent_id = {owner}.id AND c.kind = 'episode' AND c.trashed_entry IS NULL)"
    )
}

/// [`episode_count_sql`] of the row `item`, for the sort and the table.
pub(crate) static EPISODE_COUNT: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| episode_count_sql("item"));

/// The Total size of the row `item` under the one counting rule, for the sort and the table.
static TOTAL_FILE_SIZE: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| super::size_totals::counted_size_sql("item.id"));

/// [`TOTAL_FILE_SIZE`], or NULL for a title with no counted bytes.
static TOTAL_FILE_SIZE_OR_NULL: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("NULLIF({}, 0)", TOTAL_FILE_SIZE.as_str()));

/// The label of the source an item lives on, empty when the source was removed. Movies match the
/// movie sources and everything else the TV sources.
macro_rules! source_label_sql {
    () => {
        "COALESCE((SELECT ds.label FROM data_source ds          WHERE ds.path = item.data_source COLLATE NOCASE            AND ds.kind = CASE WHEN item.kind = 'movie' THEN 'movie' ELSE 'tv' END), '')"
    };
}
pub(crate) const SOURCE_LABEL_SQL: &str = source_label_sql!();

/// A column to sort by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    /// Title, ignoring case (indexed).
    Title,
    /// Sort title, falling back to the title when empty; ignores case.
    SortTitle,
    Year,
    DateAdded,
    /// Premiere or first-aired date; items without one sort first when ascending.
    ReleaseDate,
    Runtime,
    Path,
    /// Season number of a season item.
    SeasonNumber,
    /// Original title, falling back to the title; ignores case.
    OriginalTitle,
    /// File name of the first video file; ignores case.
    Filename,
    /// IMDb Top 250 position; items outside the list sort first when ascending.
    Top250,
    Certification,
    Edition,
    MediaSource,
    /// The label of the source the item lives on, ignoring case.
    Source,
    /// The pipeline stage: New, then Scraped, then Renamed (see [`ItemQuery::profile_key`]).
    Stage,
    Watched,
    IsNew,
    /// Name of the movie set the item belongs to.
    MovieSet,
    /// The preferred rating (or its fallback) on the 0 to 10 scale; see [`ItemQuery::rating`].
    Rating,
    /// Votes of the rating that [`SortField::Rating`] uses.
    Votes,
    /// The rating the user entered.
    UserRating,
    ImdbRating,
    TmdbRating,
    TraktRating,
    /// Rotten Tomatoes, all critics.
    RottenTomatoesRating,
    /// Rotten Tomatoes, audience score.
    RottenTomatoesAudienceRating,
    MetacriticRating,
    LetterboxdRating,
    MyAnimeListRating,
    RogerEbertRating,
    TvdbRating,
    /// Frame height of the first video file.
    VideoHeight,
    AspectRatio,
    /// The secondary aspect ratio of the first video file.
    AspectRatio2,
    VideoCodec,
    VideoBitrate,
    /// Whether the first video file has an HDR format.
    Hdr,
    /// The resolution rank of the first video file (2160p, 1080p, 720p, 576p, below), then HDR
    /// above no HDR; titles without a known frame height sort last.
    Quality,
    /// The folder the title's own folder sits in (its path without the last part), ignoring case.
    Folder,
    /// Size of the first video file.
    FileSize,
    /// Size of every file of the item; titles without files sort last.
    TotalFileSize,
    /// Modification time of the first video file.
    FileModified,
    AudioStreams,
    SubtitleStreams,
    Trailers,
    /// How many of poster and fan art exist as files.
    Images,
    /// Whether the metadata is complete.
    Metadata,
    /// Seasons of a show.
    Seasons,
    /// Episodes of a show.
    Episodes,
    /// Missing episodes of a show.
    MissingEpisodes,
    /// Movies of a set that are in the library.
    MoviesInSet,
    /// Movies of a set that are not in the library.
    MissingMovies,
    /// Airing status of a show.
    AiredStatus,
    /// 0 for an item whose title, original title or English title holds every word of
    /// [`ItemQuery::title_match`], 1 for the others. Only [`ItemQuery::title_first`] adds it.
    TitleMatch,
}

impl SortField {
    /// The SQL expression and whether it can be NULL.
    fn expression(
        self,
        preference: &RatingPreference,
        profile_key: &str,
    ) -> (Cow<'static, str>, bool) {
        match self {
            Self::Stage => (Cow::Owned(stage_rank_sql(profile_key)), false),
            Self::Rating => (
                Cow::Owned(preferred_rating_sql(
                    "r.value * 10.0 / r.max_value",
                    preference,
                )),
                true,
            ),
            Self::Votes => (
                Cow::Owned(preferred_rating_sql("r.votes", preference)),
                true,
            ),
            other => {
                let (expression, nullable) = other.fixed_expression();
                (Cow::Borrowed(expression), nullable)
            }
        }
    }

    /// The expression of a field that does not depend on the query.
    fn fixed_expression(self) -> (&'static str, bool) {
        match self {
            Self::Title => ("item.title COLLATE NOCASE", false),
            Self::SortTitle => (
                "COALESCE(NULLIF(item.sort_title, ''), item.title) COLLATE NOCASE",
                false,
            ),
            // Unknown (0) is "nothing", so it sorts last in both directions like a NULL.
            Self::Year => ("NULLIF(item.year, 0)", true),
            Self::DateAdded => ("item.date_added_ms", false),
            Self::ReleaseDate => ("item.release_date", true),
            Self::Runtime => ("NULLIF(item.runtime_minutes, 0)", true),
            Self::Path => ("item.path", false),
            Self::SeasonNumber => ("item.season_number", true),
            Self::OriginalTitle => (
                "COALESCE(NULLIF(item.original_title, ''), item.title) COLLATE NOCASE",
                false,
            ),
            Self::Filename => (concat!(first_video!("m.filename"), " COLLATE NOCASE"), true),
            Self::Top250 => ("NULLIF(item.top250, 0)", true),
            Self::Certification => ("item.certification COLLATE NOCASE", false),
            Self::Edition => ("item.edition COLLATE NOCASE", false),
            Self::MediaSource => ("item.media_source COLLATE NOCASE", false),
            Self::Source => (concat!("(", source_label_sql!(), ") COLLATE NOCASE"), false),
            Self::Watched => ("item.watched", false),
            Self::IsNew => (new_marker!(), false),
            Self::MovieSet => (
                "(SELECT p.title FROM item p WHERE p.id = item.parent_id) COLLATE NOCASE",
                true,
            ),
            // Built per query by `expression`: they follow the preferred source or the profile.
            Self::Rating | Self::Votes => ("NULL", true),
            Self::Stage => ("0", false),
            Self::UserRating => (rating_of!("user"), true),
            Self::ImdbRating => (rating_of!("imdb"), true),
            Self::TmdbRating => (rating_of!("tmdb"), true),
            Self::TraktRating => (rating_of!("trakt"), true),
            Self::RottenTomatoesRating => (rating_of!("tomatometerallcritics"), true),
            Self::RottenTomatoesAudienceRating => (rating_of!("tomatometerallaudience"), true),
            Self::MetacriticRating => (rating_of!("metacritic"), true),
            Self::LetterboxdRating => (rating_of!("letterboxd"), true),
            Self::MyAnimeListRating => (rating_of!("myanimelist"), true),
            Self::RogerEbertRating => (rating_of!("rogerebert"), true),
            Self::TvdbRating => (rating_of!("tvdb"), true),
            Self::VideoHeight => (first_video!("m.height"), true),
            Self::AspectRatio => (first_video!("m.aspect_ratio"), true),
            Self::AspectRatio2 => (first_video!("m.aspect_ratio2"), true),
            Self::VideoCodec => (
                concat!(first_video!("m.video_codec"), " COLLATE NOCASE"),
                true,
            ),
            Self::VideoBitrate => (first_video!("m.video_bitrate_kbps"), true),
            Self::Hdr => (first_video!("(m.hdr_formats <> '[]')"), true),
            Self::FileSize => (first_video!("m.size_bytes"), true),
            Self::TotalFileSize => (TOTAL_FILE_SIZE_OR_NULL.as_str(), true),
            Self::Quality => (
                first_video!(
                    "CASE WHEN m.height >= 1500 THEN 8 WHEN m.height >= 760 THEN 6                      WHEN m.height >= 600 THEN 4 WHEN m.height >= 1 THEN 2 END                      + (m.hdr_formats <> '[]')"
                ),
                true,
            ),
            Self::Folder => (
                r"rtrim(rtrim(replace(item.path, '\', '/'), replace(replace(item.path, '\', '/'), '/', '')), '/') COLLATE NOCASE",
                false,
            ),
            Self::FileModified => (first_video!("m.modified_ms"), true),
            Self::AudioStreams => (
                "(SELECT COUNT(*) FROM stream s JOIN media_file m ON s.media_file_id = m.id \
                 WHERE m.item_id = item.id AND m.kind = 'video' AND s.kind = 'audio')",
                false,
            ),
            Self::SubtitleStreams => (
                "(SELECT COUNT(*) FROM stream s JOIN media_file m ON s.media_file_id = m.id \
                 WHERE m.item_id = item.id AND m.kind = 'video' AND s.kind = 'subtitle')",
                false,
            ),
            Self::Trailers => (
                "(SELECT COUNT(*) FROM trailer t WHERE t.item_id = item.id)",
                false,
            ),
            Self::Images => (
                "(SELECT COUNT(DISTINCT m.kind) FROM media_file m \
                 WHERE m.item_id = item.id AND m.kind IN ('poster', 'fanart'))",
                false,
            ),
            Self::Metadata => (METADATA_COMPLETE, false),
            Self::Seasons => (SEASON_COUNT.as_str(), false),
            Self::Episodes => (EPISODE_COUNT.as_str(), false),
            Self::MissingEpisodes => (
                "COALESCE(json_array_length(item.ext, '$.missingEpisodes'), 0)",
                false,
            ),
            Self::MoviesInSet => (
                "(SELECT COUNT(*) FROM item c WHERE c.parent_id = item.id AND c.kind = 'movie' AND c.trashed_entry IS NULL)",
                false,
            ),
            Self::MissingMovies => (
                "COALESCE(json_array_length(item.ext, '$.missingMovies'), 0)",
                false,
            ),
            Self::AiredStatus => ("item.aired_status", false),
            // Built per query by `list_items_inner`: it needs the words searched for.
            Self::TitleMatch => ("0", false),
        }
    }
}

/// Sort direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Asc,
    Desc,
}

impl Direction {
    fn sql(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

/// One sort column with its direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SortKey {
    pub field: SortField,
    #[serde(default)]
    pub direction: Direction,
}

impl SortKey {
    pub fn asc(field: SortField) -> Self {
        Self {
            field,
            direction: Direction::Asc,
        }
    }

    pub fn desc(field: SortField) -> Self {
        Self {
            field,
            direction: Direction::Desc,
        }
    }
}

/// A listing of items of one kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemQuery {
    pub kind: ItemKind,
    #[serde(default)]
    pub filter: Filter,
    /// Sort columns in priority order; empty means title ascending. The item id always breaks
    /// the final tie, in the direction of the last column.
    #[serde(default)]
    pub sort: Vec<SortKey>,
    /// The preferred rating source: [`SortField::Rating`] and [`SortField::Votes`] sort by the
    /// rating from it, or by the next available source when an item has none.
    #[serde(default)]
    pub rating: RatingPreference,
    /// The key of the naming profile the organize-state cache is read against, for
    /// [`SortField::Stage`]. Empty means the profile of the newest computation.
    #[serde(default)]
    pub profile_key: String,
    /// The words of a quick search. With [`ItemQuery::title_first`] set, the items whose titles
    /// hold all of them come first.
    #[serde(default)]
    pub title_match: String,
    /// Puts the items whose titles match [`ItemQuery::title_match`] before those that match only
    /// by their plot, people, tags or file names, ahead of the sort.
    #[serde(default)]
    pub title_first: bool,
}

/// The sort rank of a title's pipeline stage: 0 New, 1 Scraped, 2 Renamed. `profile_key` names the
/// naming profile whose cached state says a title is tidy; a key that is not plain text (profile
/// keys are hashes) or an empty one reads the profile of the newest computation instead.
fn stage_rank_sql(profile_key: &str) -> String {
    let plain = !profile_key.is_empty()
        && profile_key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'));
    let key = if plain {
        format!("'{profile_key}'")
    } else {
        "(SELECT n.profile_key FROM item_organize_state n ORDER BY n.computed_ms DESC LIMIT 1)"
            .to_owned()
    };
    format!(
        "(CASE WHEN NOT {} THEN 0 WHEN EXISTS (SELECT 1 FROM item_organize_state s          WHERE s.item_id = item.id AND s.state = 'tidy' AND s.profile_key = {key})          THEN 2 ELSE 1 END)",
        super::lifecycle::scraped_sql("item")
    )
}

impl ItemQuery {
    pub fn new(kind: ItemKind) -> Self {
        Self {
            kind,
            filter: Filter::All,
            sort: Vec::new(),
            rating: RatingPreference::default(),
            profile_key: String::new(),
            title_match: String::new(),
            title_first: false,
        }
    }

    /// Lists the items whose titles hold every word of `text` before the others.
    #[must_use]
    pub fn title_first(mut self, text: impl Into<String>) -> Self {
        self.title_match = text.into();
        self.title_first = !self.title_match.trim().is_empty();
        self
    }

    /// Reads the stage sort against this naming profile.
    #[must_use]
    pub fn profile_key(mut self, key: impl Into<String>) -> Self {
        self.profile_key = key.into();
        self
    }

    /// Sorts the rating columns by this preferred source.
    #[must_use]
    pub fn rating_preference(mut self, preference: RatingPreference) -> Self {
        self.rating = preference;
        self
    }

    #[must_use]
    pub fn filter(mut self, filter: Filter) -> Self {
        self.filter = filter;
        self
    }

    #[must_use]
    pub fn sort_by(mut self, key: SortKey) -> Self {
        self.sort.push(key);
        self
    }

    fn keys(&self) -> Vec<SortKey> {
        let mut keys = if self.sort.is_empty() {
            vec![SortKey::asc(SortField::Title)]
        } else {
            self.sort.clone()
        };
        if self.title_first {
            keys.insert(0, SortKey::asc(SortField::TitleMatch));
        }
        keys
    }
}

/// One value of the sort columns of the last row of a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum CursorValue {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

impl CursorValue {
    fn from_sql(v: Value) -> DbResult<Self> {
        match v {
            Value::Null => Ok(Self::Null),
            Value::Integer(i) => Ok(Self::Int(i)),
            Value::Real(r) => Ok(Self::Real(r)),
            Value::Text(t) => Ok(Self::Text(t)),
            Value::Blob(_) => Err(DbError::InvalidData("blob in a sort column".into())),
        }
    }

    fn to_sql(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Int(i) => Value::Integer(*i),
            Self::Real(r) => Value::Real(*r),
            Self::Text(t) => Value::Text(t.clone()),
        }
    }

    fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

/// Where the next page starts: the sort values and id of the last row of the previous page.
///
/// Opaque to callers; it serialises, so it can travel through IPC. It only fits the query (same
/// filter and sort) that produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    values: Vec<CursorValue>,
    id: i64,
}

/// One page of a listing.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub items: Vec<ItemSummary>,
    /// Pass this to [`list_items`] for the following page; `None` on the last page.
    pub next: Option<Cursor>,
}

/// The columns a list or grid row needs, without loading related rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSummary {
    pub id: ItemId,
    pub kind: ItemKind,
    pub parent_id: Option<ItemId>,
    pub title: String,
    pub original_title: String,
    pub sort_title: String,
    pub year: u16,
    /// Premiere or first-aired date as ISO text.
    pub release_date: Option<String>,
    pub runtime_minutes: u32,
    pub path: String,
    pub data_source: String,
    /// The label of the source the title lives on; empty when the source was removed.
    pub source_label: String,
    pub date_added_ms: i64,
    pub watched: bool,
    pub locked: bool,
    pub is_new: bool,
    pub review_state: String,
    pub availability: String,
    pub top250: u32,
    pub season_number: Option<u32>,
}

impl ItemSummary {
    pub(crate) const COLUMNS: &'static str = concat!(
        "item.id, item.kind, item.parent_id, item.title, \
        item.original_title, item.sort_title, item.year, item.release_date, \
        item.runtime_minutes, item.path, item.data_source, item.date_added_ms, item.watched, \
        item.locked, ",
        new_marker!(),
        " AS is_new, item.review_state, item.availability, item.top250, item.season_number, ",
        source_label_sql!(),
        " AS source_label"
    );

    pub(crate) fn from_row(row: &Row<'_>) -> DbResult<Self> {
        Ok(Self {
            id: ItemId(row.get("id")?),
            kind: parse_enum(&row.get::<_, String>("kind")?, "item kind")?,
            parent_id: row.get::<_, Option<i64>>("parent_id")?.map(ItemId),
            title: row.get("title")?,
            original_title: row.get("original_title")?,
            sort_title: row.get("sort_title")?,
            year: to_u16(row.get("year")?),
            release_date: row.get("release_date")?,
            runtime_minutes: to_u32(row.get("runtime_minutes")?),
            path: row.get("path")?,
            data_source: row.get("data_source")?,
            source_label: row.get("source_label")?,
            date_added_ms: row.get("date_added_ms")?,
            watched: row.get("watched")?,
            locked: row.get("locked")?,
            is_new: row.get("is_new")?,
            review_state: row.get("review_state")?,
            availability: row.get("availability")?,
            top250: to_u32(row.get("top250")?),
            season_number: row.get::<_, Option<i64>>("season_number")?.map(to_u32),
        })
    }
}

/// Writes `WHERE item.kind = ? AND (filter)` into `sql`.
fn write_where(sql: &mut Sql, query: &ItemQuery) -> DbResult<()> {
    sql.kind = Some(query.kind);
    sql.push(" WHERE item.trashed_entry IS NULL AND item.kind = ");
    sql.bind(enum_text(&query.kind)?);
    // A collection is listed only while the library holds two or more of its movies; with fewer
    // it is kept but hidden, and returns when a second movie arrives.
    if query.kind == ItemKind::Set {
        sql.push(&format!(
            " AND (SELECT COUNT(*) FROM item c WHERE c.parent_id = item.id \
             AND c.kind = 'movie') >= {MIN_OWNED_IN_COLLECTION}"
        ));
    }
    sql.push(" AND ");
    query.filter.write(sql)?;
    Ok(())
}

/// The movies a collection needs in the library to be listed.
pub const MIN_OWNED_IN_COLLECTION: u32 = 2;

/// The number of items matching the query.
pub fn count_items(conn: &Connection, query: &ItemQuery) -> DbResult<u64> {
    let mut sql = Sql::default();
    sql.push("SELECT count(*) FROM item");
    write_where(&mut sql, query)?;
    let n: i64 = conn
        .prepare_cached(&sql.text)?
        .query_row(params_from_iter(sql.params.iter()), |r| r.get(0))?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// One group of a listing grouped by source: the label and how many matching items it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceGroup {
    /// The source's label; empty for titles whose source was removed.
    pub label: String,
    pub count: u64,
}

/// The matching items grouped by the label of their source, in label order (ignoring case). With
/// the listing sorted by [`SortField::Source`] the groups follow each other in the same order, so
/// the grid, list and table draw their group headers from one result.
pub fn source_groups(conn: &Connection, query: &ItemQuery) -> DbResult<Vec<SourceGroup>> {
    let mut sql = Sql::default();
    sql.push(&format!(
        "SELECT {SOURCE_LABEL_SQL} AS label, count(*) FROM item"
    ));
    write_where(&mut sql, query)?;
    sql.push(" GROUP BY label ORDER BY label COLLATE NOCASE");
    let mut stmt = conn.prepare_cached(&sql.text)?;
    let rows = stmt.query_map(params_from_iter(sql.params.iter()), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.map(|row| {
        let (label, count) = row?;
        Ok(SourceGroup {
            label,
            count: u64::try_from(count).unwrap_or(0),
        })
    })
    .collect()
}

/// Returns up to `limit` items of the query, starting after `after` (or from the start).
///
/// `limit` is clamped to at least 1.
pub fn list_items(
    conn: &Connection,
    query: &ItemQuery,
    after: Option<&Cursor>,
    limit: usize,
) -> DbResult<Page> {
    list_items_inner(conn, query, after, 0, limit)
}

/// Returns up to `limit` items starting at row `offset` of the query's order.
///
/// This is the random-access form a virtualised grid needs when the user drags the scrollbar to
/// row 15,000: SQLite walks the index to the offset (no rows are materialised), which stays well
/// under a frame for libraries of tens of thousands of items. Prefer [`list_items`] with a cursor
/// for sequential reads; the two agree on order. `limit` is clamped to at least 1.
pub fn list_items_at(
    conn: &Connection,
    query: &ItemQuery,
    offset: usize,
    limit: usize,
) -> DbResult<Vec<ItemSummary>> {
    Ok(list_items_inner(conn, query, None, offset, limit)?.items)
}

fn list_items_inner(
    conn: &Connection,
    query: &ItemQuery,
    after: Option<&Cursor>,
    offset: usize,
    limit: usize,
) -> DbResult<Page> {
    let limit = limit.max(1);
    let keys = query.keys();
    let exprs: Vec<(Cow<'static, str>, bool)> = keys
        .iter()
        .map(|k| match k.field {
            SortField::TitleMatch => (
                Cow::Owned(super::search::rank_sql(&query.title_match)),
                false,
            ),
            field => field.expression(&query.rating, &query.profile_key),
        })
        .collect();
    let id_dir = keys.last().map_or(Direction::Asc, |k| k.direction);

    let mut sql = Sql::default();
    sql.push("SELECT ");
    sql.push(ItemSummary::COLUMNS);
    for (i, (expr, _)) in exprs.iter().enumerate() {
        sql.push(&format!(", {expr} AS s{i}"));
    }
    sql.push(" FROM item");
    write_where(&mut sql, query)?;

    if let Some(cursor) = after {
        if cursor.values.len() != keys.len() {
            return Err(DbError::InvalidCursor);
        }
        sql.push(" AND ");
        write_keyset(&mut sql, &keys, &exprs, cursor, id_dir);
    }

    sql.push(" ORDER BY ");
    for (i, (key, (expr, nullable))) in keys.iter().zip(&exprs).enumerate() {
        if i > 0 {
            sql.push(", ");
        }
        // A value that can be NULL puts the NULLs last whichever way the column is sorted.
        if *nullable {
            sql.push(expr);
            sql.push(" IS NULL, ");
        }
        sql.push(expr);
        sql.push(" ");
        sql.push(key.direction.sql());
    }
    sql.push(", item.id ");
    sql.push(id_dir.sql());
    sql.push(" LIMIT ");
    sql.bind(i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX));
    if offset > 0 {
        sql.push(" OFFSET ");
        sql.bind(i64::try_from(offset).unwrap_or(i64::MAX));
    }

    let mut stmt = conn.prepare_cached(&sql.text)?;
    let mut rows = stmt.query(params_from_iter(sql.params.iter()))?;
    let mut items = Vec::with_capacity(limit.min(1024));
    let mut last: Option<Cursor> = None;
    let mut more = false;
    while let Some(row) = rows.next()? {
        if items.len() == limit {
            more = true;
            break;
        }
        let item = ItemSummary::from_row(row)?;
        let values = (0..keys.len())
            .map(|i| CursorValue::from_sql(row.get::<_, Value>(format!("s{i}").as_str())?))
            .collect::<DbResult<Vec<_>>>()?;
        last = Some(Cursor {
            values,
            id: item.id.0,
        });
        items.push(item);
    }
    Ok(Page {
        items,
        next: if more { last } else { None },
    })
}

/// Appends the condition "strictly after the cursor row in sort order".
fn write_keyset(
    sql: &mut Sql,
    keys: &[SortKey],
    exprs: &[(Cow<'static, str>, bool)],
    cursor: &Cursor,
    id_dir: Direction,
) {
    sql.push("(");
    for i in 0..=keys.len() {
        if i > 0 {
            sql.push(" OR ");
        }
        sql.push("(");
        // Equal on every earlier column ...
        for (expr, value) in exprs.iter().zip(&cursor.values).take(i) {
            sql.push(&expr.0);
            sql.push(" IS ");
            sql.bind(value.to_sql());
            sql.push(" AND ");
        }
        // ... and strictly after on this one (the last step compares the id).
        if i < keys.len() {
            write_after(
                sql,
                &exprs[i].0,
                exprs[i].1,
                keys[i].direction,
                &cursor.values[i],
            );
        } else {
            sql.push("item.id ");
            sql.push(match id_dir {
                Direction::Asc => "> ",
                Direction::Desc => "< ",
            });
            sql.bind(cursor.id);
        }
        sql.push(")");
    }
    sql.push(")");
}

/// "`expr` sorts strictly after `value`". A NULL sorts after every value in both directions, so
/// nothing follows a NULL on this column and every NULL follows a value of a nullable one.
fn write_after(sql: &mut Sql, expr: &str, nullable: bool, dir: Direction, value: &CursorValue) {
    if value.is_null() {
        sql.push("0");
        return;
    }
    sql.push("(");
    sql.push(expr);
    sql.push(match dir {
        Direction::Asc => " > ",
        Direction::Desc => " < ",
    });
    sql.bind(value.to_sql());
    if nullable {
        sql.push(" OR ");
        sql.push(expr);
        sql.push(" IS NULL");
    }
    sql.push(")");
}

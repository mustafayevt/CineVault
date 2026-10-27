//! Per-row facts for the library list and table views.
//!
//! A page of rows needs far more than a poster card: ratings per source, the main video file's
//! technical facts, stream and trailer counts and, for shows and sets, counts of their children.
//! [`table_facts_for`] reads all of it for a page of item ids in a handful of indexed queries, so
//! a page of 120 rows costs the same whatever the size of the library.
//!
//! A movie's video facts come from its own first video file. A show or set has no video file of
//! its own, so it reports those of its first episode or movie: enough for the format, codec and
//! size columns to say something useful.

use std::collections::HashMap;

use std::collections::BTreeSet;

use cv_domain::{AudioStream, ItemId, RatingPreference, StreamFlag};
use rusqlite::Connection;

use super::query::{EPISODE_COUNT, METADATA_COMPLETE, SEASON_COUNT};
use super::size_totals::counted_size_sql;
use crate::error::DbResult;

/// The main video file of an item, as the table shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoRow {
    pub media_file_id: i64,
    pub filename: String,
    pub codec: String,
    pub container: String,
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: f64,
    /// The secondary aspect ratio found by the letterbox detector, `0` when none.
    pub aspect_ratio2: f64,
    pub bitrate_kbps: u32,
    pub size_bytes: u64,
    pub modified_ms: i64,
    /// HDR formats such as `Dolby Vision`; empty for SDR.
    pub hdr_formats: Vec<String>,
}

/// Everything the list and table views show beyond a poster card.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableFacts {
    pub plot: String,
    pub note: String,
    pub certification: String,
    pub edition: String,
    pub media_source: String,
    pub video_in_3d: bool,
    pub aired_status: String,
    pub original_filename: String,
    /// Name of the movie set a movie belongs to.
    pub movie_set: Option<String>,
    pub metadata_complete: bool,
    /// The rating the item shows on the 0 to 10 scale, with its source and votes: the preferred
    /// source's, or the next available one.
    pub rating: Option<f64>,
    /// The source of `rating`.
    pub rating_source: Option<String>,
    /// True when `rating` comes from a source other than the preferred one.
    pub rating_is_fallback: bool,
    pub votes: u32,
    pub user_rating: Option<f64>,
    pub imdb_rating: Option<f64>,
    pub tmdb_rating: Option<f64>,
    pub trakt_rating: Option<f64>,
    /// Rotten Tomatoes, all critics.
    pub rotten_tomatoes_rating: Option<f64>,
    /// Rotten Tomatoes, audience score.
    pub rotten_tomatoes_audience_rating: Option<f64>,
    pub metacritic_rating: Option<f64>,
    pub letterboxd_rating: Option<f64>,
    pub myanimelist_rating: Option<f64>,
    pub rogerebert_rating: Option<f64>,
    pub video: Option<VideoRow>,
    /// Size of every file of the item (and of its episodes or movies).
    pub total_size_bytes: u64,
    pub audio_streams: u32,
    /// Codec of the first audio stream. The library list shows the hero track's instead (see
    /// `cv_app::hero_audio`), picked from `audio_tracks`.
    pub audio_codec: String,
    pub audio_channels: u32,
    /// The audio streams of the main video file, in file order.
    pub audio_tracks: Vec<AudioStream>,
    /// The title's original language (ISO 639-1), empty when unknown; with `audio_tracks` it
    /// decides the hero audio track.
    pub original_language: String,
    pub subtitle_streams: u32,
    pub trailers: u32,
    pub has_poster: bool,
    pub has_fanart: bool,
    pub has_logo: bool,
    /// A Kodi-style NFO file sits next to the item.
    pub has_nfo: bool,
    /// Subtitle files next to the video; the embedded ones are `subtitle_streams`.
    pub subtitle_files: u32,
    pub seasons: u32,
    pub episodes: u32,
    pub missing_episodes: u32,
    pub movies_in_set: u32,
    pub missing_movies: u32,
}

fn ids_json(items: &[ItemId]) -> DbResult<String> {
    Ok(serde_json::to_string(
        &items.iter().map(|id| id.0).collect::<Vec<_>>(),
    )?)
}

fn count(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

fn bytes(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// The table facts of `items`; the "rating" column follows `preference`. Items that do not exist
/// are absent from the map.
pub fn table_facts_for(
    conn: &Connection,
    items: &[ItemId],
    preference: &RatingPreference,
) -> DbResult<HashMap<ItemId, TableFacts>> {
    let mut facts: HashMap<ItemId, TableFacts> = HashMap::new();
    if items.is_empty() {
        return Ok(facts);
    }
    let ids = ids_json(items)?;

    read_item_columns(conn, &ids, &mut facts)?;
    read_ratings(conn, &ids, preference, &mut facts)?;
    read_main_video(conn, &ids, &mut facts)?;
    read_total_sizes(conn, &ids, &mut facts)?;
    read_streams(conn, &mut facts)?;
    read_trailers_and_artwork(conn, &ids, &mut facts)?;
    Ok(facts)
}

fn read_item_columns(
    conn: &Connection,
    ids: &str,
    facts: &mut HashMap<ItemId, TableFacts>,
) -> DbResult<()> {
    let sql = format!(
        "SELECT item.id, item.plot, item.note, item.certification, item.edition, \
                item.media_source, item.video_in_3d, item.aired_status, item.original_filename, \
                (SELECT p.title FROM item p WHERE p.id = item.parent_id AND p.kind = 'set'), \
                {METADATA_COMPLETE}, \
                {season_count}, \
                {episode_count}, \
                COALESCE(json_array_length(item.ext, '$.missingEpisodes'), 0), \
                (SELECT COUNT(*) FROM item c WHERE c.parent_id = item.id AND c.kind = 'movie'), \
                COALESCE(json_array_length(item.ext, '$.missingMovies'), 0), \
                item.original_language \
         FROM item WHERE item.id IN (SELECT value FROM json_each(?1))",
        season_count = SEASON_COUNT.as_str(),
        episode_count = EPISODE_COUNT.as_str(),
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query([ids])?;
    while let Some(r) = rows.next()? {
        let id = ItemId(r.get(0)?);
        facts.insert(
            id,
            TableFacts {
                plot: r.get(1)?,
                note: r.get(2)?,
                certification: r.get(3)?,
                edition: r.get(4)?,
                media_source: r.get(5)?,
                video_in_3d: r.get(6)?,
                aired_status: r.get(7)?,
                original_filename: r.get(8)?,
                movie_set: r.get(9)?,
                metadata_complete: r.get(10)?,
                seasons: count(r.get(11)?),
                episodes: count(r.get(12)?),
                missing_episodes: count(r.get(13)?),
                movies_in_set: count(r.get(14)?),
                missing_movies: count(r.get(15)?),
                original_language: r.get(16)?,
                ..TableFacts::default()
            },
        );
    }
    Ok(())
}

fn read_ratings(
    conn: &Connection,
    ids: &str,
    preference: &RatingPreference,
    facts: &mut HashMap<ItemId, TableFacts>,
) -> DbResult<()> {
    let mut stmt = conn.prepare_cached(
        "SELECT item_id, source, value * 10.0 / max_value, votes FROM rating \
         WHERE max_value > 0 AND item_id IN (SELECT value FROM json_each(?1))",
    )?;
    let mut rows = stmt.query([ids])?;
    // The rating an item shows is the one whose source ranks first for the preference.
    let mut best: HashMap<ItemId, (usize, String)> = HashMap::new();
    while let Some(r) = rows.next()? {
        let id = ItemId(r.get(0)?);
        let source: String = r.get(1)?;
        let value: f64 = r.get(2)?;
        let votes = count(r.get(3)?);
        let Some(entry) = facts.get_mut(&id) else {
            continue;
        };
        if source == "user" {
            entry.user_rating = Some(value);
            continue;
        }
        match source.as_str() {
            "imdb" => entry.imdb_rating = Some(value),
            "tmdb" => entry.tmdb_rating = Some(value),
            "trakt" => entry.trakt_rating = Some(value),
            "tomatometerallcritics" => entry.rotten_tomatoes_rating = Some(value),
            "tomatometerallaudience" => entry.rotten_tomatoes_audience_rating = Some(value),
            "metacritic" => entry.metacritic_rating = Some(value),
            "letterboxd" => entry.letterboxd_rating = Some(value),
            "myanimelist" => entry.myanimelist_rating = Some(value),
            "rogerebert" => entry.rogerebert_rating = Some(value),
            _ => {}
        }
        let rank = (preference.rank(&source), source.clone());
        if best.get(&id).is_none_or(|current| rank < *current) {
            entry.rating = Some(value);
            entry.rating_is_fallback = preference.is_fallback(&source);
            entry.rating_source = Some(source);
            entry.votes = votes;
            best.insert(id, rank);
        }
    }
    Ok(())
}

fn read_main_video(
    conn: &Connection,
    ids: &str,
    facts: &mut HashMap<ItemId, TableFacts>,
) -> DbResult<()> {
    let mut stmt = conn.prepare_cached(
        "SELECT owner, id, filename, video_codec, container_format, width, height, aspect_ratio, aspect_ratio2, \
                video_bitrate_kbps, size_bytes, modified_ms, hdr_formats \
         FROM ( \
             SELECT j.value AS owner, m.id, m.filename, m.video_codec, m.container_format, m.width, \
                    m.height, m.aspect_ratio, m.aspect_ratio2, m.video_bitrate_kbps, m.size_bytes, \
                    m.modified_ms, m.hdr_formats, \
                    ROW_NUMBER() OVER ( \
                        PARTITION BY j.value \
                        ORDER BY (m.item_id <> j.value), m.item_id, m.position, m.id \
                    ) AS rn \
             FROM json_each(?1) j \
             JOIN media_file m ON m.kind = 'video' AND ( \
                 m.item_id = j.value OR m.item_id IN ( \
                     SELECT c.id FROM item c \
                     WHERE c.parent_id = j.value AND c.kind IN ('episode', 'movie'))) \
         ) WHERE rn = 1",
    )?;
    let mut rows = stmt.query([ids])?;
    while let Some(r) = rows.next()? {
        let owner = ItemId(r.get(0)?);
        let hdr: String = r.get(12)?;
        let video = VideoRow {
            media_file_id: r.get(1)?,
            filename: r.get(2)?,
            codec: r.get(3)?,
            container: r.get(4)?,
            width: count(r.get(5)?),
            height: count(r.get(6)?),
            aspect_ratio: r.get(7)?,
            aspect_ratio2: r.get(8)?,
            bitrate_kbps: count(r.get(9)?),
            size_bytes: bytes(r.get(10)?),
            modified_ms: r.get(11)?,
            hdr_formats: serde_json::from_str(&hdr)?,
        };
        if let Some(entry) = facts.get_mut(&owner) {
            entry.video = Some(video);
        }
    }
    Ok(())
}

fn read_total_sizes(
    conn: &Connection,
    ids: &str,
    facts: &mut HashMap<ItemId, TableFacts>,
) -> DbResult<()> {
    // The shared rule: distinct online files, each path counted once.
    let sql = format!(
        "SELECT j.value, {counted} FROM json_each(?1) j",
        counted = counted_size_sql("j.value")
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query([ids])?;
    while let Some(r) = rows.next()? {
        if let Some(entry) = facts.get_mut(&ItemId(r.get(0)?)) {
            entry.total_size_bytes = bytes(r.get(1)?);
        }
    }
    Ok(())
}

/// Audio and subtitle streams of each item's main video file.
fn read_streams(conn: &Connection, facts: &mut HashMap<ItemId, TableFacts>) -> DbResult<()> {
    let by_file: HashMap<i64, Vec<ItemId>> =
        facts.iter().fold(HashMap::new(), |mut map, (id, entry)| {
            if let Some(video) = &entry.video {
                map.entry(video.media_file_id).or_default().push(*id);
            }
            map
        });
    if by_file.is_empty() {
        return Ok(());
    }
    let files = serde_json::to_string(&by_file.keys().collect::<Vec<_>>())?;
    let mut stmt = conn.prepare_cached(
        "SELECT media_file_id, kind, codec, channels, language, title, flags FROM stream \
         WHERE media_file_id IN (SELECT value FROM json_each(?1)) \
         ORDER BY media_file_id, kind, position, id",
    )?;
    let mut rows = stmt.query([&files])?;
    while let Some(r) = rows.next()? {
        let file: i64 = r.get(0)?;
        let kind: String = r.get(1)?;
        let codec: String = r.get(2)?;
        let channels = count(r.get(3)?);
        let audio = (kind == "audio")
            .then(|| -> DbResult<AudioStream> {
                Ok(AudioStream {
                    codec: codec.clone(),
                    language: r.get(4)?,
                    title: r.get(5)?,
                    flags: serde_json::from_str::<BTreeSet<StreamFlag>>(&r.get::<_, String>(6)?)?,
                    channels,
                    ..AudioStream::default()
                })
            })
            .transpose()?;
        for owner in by_file.get(&file).into_iter().flatten() {
            let Some(entry) = facts.get_mut(owner) else {
                continue;
            };
            if kind == "audio" {
                if entry.audio_streams == 0 {
                    entry.audio_codec.clone_from(&codec);
                    entry.audio_channels = channels;
                }
                entry.audio_streams += 1;
                entry.audio_tracks.extend(audio.clone());
            } else {
                entry.subtitle_streams += 1;
            }
        }
    }
    Ok(())
}

fn read_trailers_and_artwork(
    conn: &Connection,
    ids: &str,
    facts: &mut HashMap<ItemId, TableFacts>,
) -> DbResult<()> {
    let mut trailers = conn.prepare_cached(
        "SELECT item_id, COUNT(*) FROM trailer \
         WHERE item_id IN (SELECT value FROM json_each(?1)) GROUP BY item_id",
    )?;
    let mut rows = trailers.query([ids])?;
    while let Some(r) = rows.next()? {
        if let Some(entry) = facts.get_mut(&ItemId(r.get(0)?)) {
            entry.trailers = count(r.get(1)?);
        }
    }
    let mut images = conn.prepare_cached(
        "SELECT item_id, kind, COUNT(*) FROM media_file          WHERE kind IN ('poster', 'fanart', 'clearlogo', 'nfo', 'subtitle')            AND item_id IN (SELECT value FROM json_each(?1)) GROUP BY item_id, kind",
    )?;
    let mut rows = images.query([ids])?;
    while let Some(r) = rows.next()? {
        let kind: String = r.get(1)?;
        if let Some(entry) = facts.get_mut(&ItemId(r.get(0)?)) {
            match kind.as_str() {
                "poster" => entry.has_poster = true,
                "fanart" => entry.has_fanart = true,
                "clearlogo" => entry.has_logo = true,
                "nfo" => entry.has_nfo = true,
                _ => entry.subtitle_files = count(r.get(2)?),
            }
        }
    }
    Ok(())
}

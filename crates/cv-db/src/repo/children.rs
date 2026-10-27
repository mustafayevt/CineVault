//! Reading and writing the rows that belong to one item: provider IDs, ratings, tags, genres,
//! artwork URLs, trailers, episode numbers, media files with their streams, and credits.

use std::collections::{BTreeMap, BTreeSet};

use cv_domain::{
    AudioStream, CalendarDate, Credit, EpisodeGroup, EpisodeNumber, EpisodeOrdering, Genre, Item,
    ItemDetail, MediaFile, MediaFileType, Person, ProviderIds, Rating, StreamFlag, SubtitleStream,
    Trailer, rating_key,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::codec::{enum_text, parse_enum, to_i64, to_u32, to_u64};
use crate::error::{DbError, DbResult};

/// Replaces every related row of item `id` with the contents of `item`.
///
/// `keep_user_rating` leaves the stored user rating as it is, whatever `item` carries: an update
/// writes a copy of the item that may be stale, and the user rating is only ever changed by
/// [`super::ratings::set_user_rating`].
pub(crate) fn replace_all(
    conn: &Connection,
    id: i64,
    item: &Item,
    keep_user_rating: bool,
) -> DbResult<()> {
    save_ids(conn, id, &item.core.ids)?;
    save_ratings(conn, id, item, keep_user_rating)?;
    save_tags(conn, id, item)?;
    save_genres(conn, id, item)?;
    save_artwork(conn, id, item)?;
    save_trailers(conn, id, item)?;
    save_episode_numbers(conn, id, item)?;
    save_media_files(conn, id, item)?;
    save_credits(conn, id, item)?;
    Ok(())
}

/// Fills the related lists of `item` (whose own columns are already loaded).
pub(crate) fn load_into(conn: &Connection, id: i64, item: &mut Item) -> DbResult<()> {
    item.core.ids = load_ids(conn, id)?;
    item.core.ratings = load_ratings(conn, id)?.into_iter().collect();
    item.core.tags = load_names(
        conn,
        "SELECT t.name FROM item_tag it JOIN tag t ON t.id = it.tag_id
         WHERE it.item_id = ?1 ORDER BY it.position",
        id,
    )?
    .into();
    item.core.artwork_urls = load_artwork(conn, id)?;
    item.core.media_files = load_media_files(conn, id)?;
    item.core.credits = load_credits(conn, id)?;

    let genres = || -> DbResult<Vec<Genre>> {
        load_names(
            conn,
            "SELECT g.name FROM item_genre ig JOIN genre g ON g.id = ig.genre_id
             WHERE ig.item_id = ?1 ORDER BY ig.position",
            id,
        )?
        .iter()
        .map(|n| Genre::parse(n).ok_or_else(|| DbError::InvalidData("blank genre".into())))
        .collect()
    };
    match &mut item.detail {
        ItemDetail::Movie(m) => {
            m.genres = genres()?;
            m.trailers = load_trailers(conn, id)?;
        }
        ItemDetail::Show(s) => {
            s.genres = genres()?;
            s.trailers = load_trailers(conn, id)?;
        }
        ItemDetail::Episode(e) => {
            for n in load_episode_numbers(conn, id)? {
                e.numbers.set(n);
            }
        }
        ItemDetail::Set(_) | ItemDetail::Season(_) => {}
    }
    Ok(())
}

fn load_names(conn: &Connection, sql: &str, id: i64) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare_cached(sql)?;
    let names = stmt
        .query_map([id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names)
}

// ---- provider IDs ----------------------------------------------------------------------------

fn save_ids(conn: &Connection, id: i64, ids: &ProviderIds) -> DbResult<()> {
    conn.prepare_cached("DELETE FROM ext_id WHERE item_id = ?1")?
        .execute([id])?;
    let mut stmt =
        conn.prepare_cached("INSERT INTO ext_id (item_id, provider, value) VALUES (?1, ?2, ?3)")?;
    for (provider, value) in ids.iter() {
        stmt.execute(params![id, provider, value])?;
    }
    Ok(())
}

fn load_ids(conn: &Connection, id: i64) -> DbResult<ProviderIds> {
    let mut stmt = conn.prepare_cached("SELECT provider, value FROM ext_id WHERE item_id = ?1")?;
    let ids = stmt
        .query_map([id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids.into_iter().collect())
}

// ---- ratings ---------------------------------------------------------------------------------

fn save_ratings(conn: &Connection, id: i64, item: &Item, keep_user_rating: bool) -> DbResult<()> {
    if keep_user_rating {
        conn.prepare_cached("DELETE FROM rating WHERE item_id = ?1 AND source <> ?2")?
            .execute(params![id, rating_key::USER])?;
    } else {
        conn.prepare_cached("DELETE FROM rating WHERE item_id = ?1")?
            .execute([id])?;
    }
    let mut stmt = conn.prepare_cached(
        "INSERT INTO rating (item_id, source, value, votes, max_value, fetched_at_ms, url)          VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let now = super::ratings::now_ms();
    for r in item.core.ratings.iter() {
        if keep_user_rating && r.is_user() {
            continue;
        }
        // A rating that arrives without a fetch date was just fetched.
        let fetched = if r.fetched_at_ms > 0 {
            r.fetched_at_ms
        } else {
            now
        };
        stmt.execute(params![
            id,
            r.id,
            r.value,
            r.votes,
            r.max_value,
            fetched,
            r.url
        ])?;
    }
    Ok(())
}

pub(super) fn load_ratings(conn: &Connection, id: i64) -> DbResult<Vec<Rating>> {
    let mut stmt = conn.prepare_cached(
        "SELECT source, value, votes, max_value, fetched_at_ms, url FROM rating          WHERE item_id = ?1 ORDER BY source",
    )?;
    let ratings = stmt
        .query_map([id], |r| {
            Ok(Rating {
                id: r.get(0)?,
                value: r.get(1)?,
                votes: to_u32(r.get(2)?),
                max_value: to_u32(r.get(3)?),
                fetched_at_ms: r.get(4)?,
                url: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ratings)
}

// ---- tags and genres -------------------------------------------------------------------------

fn save_tags(conn: &Connection, id: i64, item: &Item) -> DbResult<()> {
    conn.prepare_cached("DELETE FROM item_tag WHERE item_id = ?1")?
        .execute([id])?;
    for (pos, name) in item.core.tags.iter().enumerate() {
        conn.prepare_cached("INSERT OR IGNORE INTO tag (name) VALUES (?1)")?
            .execute([name])?;
        conn.prepare_cached(
            "INSERT OR IGNORE INTO item_tag (item_id, tag_id, position)
             VALUES (?1, (SELECT id FROM tag WHERE name = ?2), ?3)",
        )?
        .execute(params![id, name, pos as i64])?;
    }
    Ok(())
}

fn save_genres(conn: &Connection, id: i64, item: &Item) -> DbResult<()> {
    conn.prepare_cached("DELETE FROM item_genre WHERE item_id = ?1")?
        .execute([id])?;
    let genres: &[Genre] = match &item.detail {
        ItemDetail::Movie(m) => &m.genres,
        ItemDetail::Show(s) => &s.genres,
        _ => &[],
    };
    for (pos, genre) in genres.iter().enumerate() {
        let name = genre.name();
        conn.prepare_cached("INSERT OR IGNORE INTO genre (name) VALUES (?1)")?
            .execute([name])?;
        conn.prepare_cached(
            "INSERT OR IGNORE INTO item_genre (item_id, genre_id, position)
             VALUES (?1, (SELECT id FROM genre WHERE name = ?2), ?3)",
        )?
        .execute(params![id, name, pos as i64])?;
    }
    Ok(())
}

// ---- artwork URLs ----------------------------------------------------------------------------

/// Writes the single-image artwork URLs (position 0). A URL that did not change keeps its cache
/// hash and size; a changed URL clears them so the image is fetched again.
fn save_artwork(conn: &Connection, id: i64, item: &Item) -> DbResult<()> {
    let wanted: BTreeMap<String, &String> = item
        .core
        .artwork_urls
        .iter()
        .map(|(kind, url)| Ok((enum_text(kind)?, url)))
        .collect::<DbResult<_>>()?;

    let existing = {
        let mut stmt = conn
            .prepare_cached("SELECT kind FROM artwork_ref WHERE item_id = ?1 AND position = 0")?;
        stmt.query_map([id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    for kind in existing.iter().filter(|k| !wanted.contains_key(*k)) {
        conn.prepare_cached(
            "DELETE FROM artwork_ref WHERE item_id = ?1 AND kind = ?2 AND position = 0",
        )?
        .execute(params![id, kind])?;
    }
    let mut upsert = conn.prepare_cached(
        "INSERT INTO artwork_ref (item_id, kind, position, url) VALUES (?1, ?2, 0, ?3)
         ON CONFLICT (item_id, kind, position) DO UPDATE SET
             url = excluded.url,
             cache_hash = CASE WHEN artwork_ref.url = excluded.url THEN artwork_ref.cache_hash END,
             width = CASE WHEN artwork_ref.url = excluded.url THEN artwork_ref.width ELSE 0 END,
             height = CASE WHEN artwork_ref.url = excluded.url THEN artwork_ref.height ELSE 0 END",
    )?;
    for (kind, url) in wanted {
        upsert.execute(params![id, kind, url])?;
    }
    Ok(())
}

fn load_artwork(conn: &Connection, id: i64) -> DbResult<BTreeMap<MediaFileType, String>> {
    let mut stmt = conn
        .prepare_cached("SELECT kind, url FROM artwork_ref WHERE item_id = ?1 AND position = 0")?;
    let rows = stmt
        .query_map([id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(kind, url)| Ok((parse_enum(&kind, "artwork type")?, url)))
        .collect()
}

// ---- trailers --------------------------------------------------------------------------------

fn save_trailers(conn: &Connection, id: i64, item: &Item) -> DbResult<()> {
    conn.prepare_cached("DELETE FROM trailer WHERE item_id = ?1")?
        .execute([id])?;
    let trailers: &[Trailer] = match &item.detail {
        ItemDetail::Movie(m) => &m.trailers,
        ItemDetail::Show(s) => &s.trailers,
        _ => &[],
    };
    let mut stmt = conn.prepare_cached(
        "INSERT INTO trailer (item_id, position, external_id, name, url, quality, language,
                              provider, in_nfo, date, scraped_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;
    for (pos, t) in trailers.iter().enumerate() {
        stmt.execute(params![
            id,
            pos as i64,
            t.id,
            t.name,
            t.url,
            t.quality,
            t.language,
            t.provider,
            t.in_nfo,
            t.date.map(|d| d.to_string()),
            t.scraped_by,
        ])?;
    }
    Ok(())
}

fn load_trailers(conn: &Connection, id: i64) -> DbResult<Vec<Trailer>> {
    let mut stmt = conn.prepare_cached(
        "SELECT external_id, name, url, quality, language, provider, in_nfo, date, scraped_by
         FROM trailer WHERE item_id = ?1 ORDER BY position, id",
    )?;
    let rows = stmt
        .query_map([id], |r| {
            Ok((
                Trailer {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    url: r.get(2)?,
                    quality: r.get(3)?,
                    language: r.get(4)?,
                    provider: r.get(5)?,
                    in_nfo: r.get(6)?,
                    date: None,
                    scraped_by: r.get(8)?,
                    // The library does not keep the length; a search knows it.
                    length_secs: 0,
                },
                r.get::<_, Option<String>>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(mut t, date)| {
            t.date = parse_date(date, "trailer date")?;
            Ok(t)
        })
        .collect()
}

fn parse_date(text: Option<String>, what: &str) -> DbResult<Option<CalendarDate>> {
    match text {
        Some(t) => t
            .parse()
            .map(Some)
            .map_err(|e| DbError::InvalidData(format!("{what}: {e}"))),
        None => Ok(None),
    }
}

// ---- episode numbers -------------------------------------------------------------------------

fn save_episode_numbers(conn: &Connection, id: i64, item: &Item) -> DbResult<()> {
    conn.prepare_cached("DELETE FROM episode_number WHERE item_id = ?1")?
        .execute([id])?;
    let ItemDetail::Episode(e) = &item.detail else {
        return Ok(());
    };
    let mut stmt = conn.prepare_cached(
        "INSERT OR REPLACE INTO episode_number (item_id, ordering, group_name, season, episode)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for n in e.numbers.iter() {
        // The aired ordering has no named group, whatever name the value carries.
        let group_name = if n.group.ordering == EpisodeOrdering::Aired {
            ""
        } else {
            n.group.name.as_str()
        };
        stmt.execute(params![
            id,
            enum_text(&n.group.ordering)?,
            group_name,
            n.season,
            n.episode
        ])?;
    }
    Ok(())
}

fn load_episode_numbers(conn: &Connection, id: i64) -> DbResult<Vec<EpisodeNumber>> {
    let mut stmt = conn.prepare_cached(
        "SELECT ordering, group_name, season, episode FROM episode_number
         WHERE item_id = ?1
         ORDER BY CASE ordering WHEN 'aired' THEN 0 WHEN 'dvd' THEN 1 WHEN 'absolute' THEN 2
                                WHEN 'alternate' THEN 3 ELSE 4 END,
                  group_name",
    )?;
    let rows = stmt
        .query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(ordering, name, season, episode)| {
            Ok(EpisodeNumber::new(
                EpisodeGroup::new(parse_enum(&ordering, "episode ordering")?, name),
                to_u32(season),
                to_u32(episode),
            ))
        })
        .collect()
}

// ---- media files and streams -----------------------------------------------------------------

fn save_media_files(conn: &Connection, id: i64, item: &Item) -> DbResult<()> {
    // Streams follow their file (ON DELETE CASCADE).
    conn.prepare_cached("DELETE FROM media_file WHERE item_id = ?1")?
        .execute([id])?;
    let mut insert_file = conn.prepare_cached(
        "INSERT INTO media_file (
             item_id, position, kind, directory, filename, size_bytes, modified_ms,
             video_codec, codec_id, container_format, exact_video_format, video_3d,
             width, height, aspect_ratio, aspect_ratio2, pixel_aspect_ratio,
             video_bitrate_kbps, overall_bitrate_kbps, bit_depth, frame_rate, duration_secs,
             stacking, stacking_marker, title, is_iso, is_animated_graphic,
             hdr_formats, checksums, hdr_details)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                 ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)",
    )?;
    let mut insert_stream = conn.prepare_cached(
        "INSERT INTO stream (media_file_id, kind, position, codec, codec_id, language, title,
                            flags, channels, bitrate_kbps, bit_depth, sample_rate)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;
    for (pos, f) in item.core.media_files.iter().enumerate() {
        insert_file.execute(params![
            id,
            pos as i64,
            enum_text(&f.kind)?,
            f.directory,
            f.filename,
            to_i64(f.size_bytes),
            f.modified_ms,
            f.video_codec,
            f.codec_id,
            f.container_format,
            f.exact_video_format,
            enum_text(&f.video_3d)?,
            f.width,
            f.height,
            f.aspect_ratio,
            f.aspect_ratio2,
            f.pixel_aspect_ratio,
            f.video_bitrate_kbps,
            f.overall_bitrate_kbps,
            f.bit_depth,
            f.frame_rate,
            f.duration_secs,
            f.stacking,
            f.stacking_marker,
            f.title,
            f.is_iso,
            f.is_animated_graphic,
            serde_json::to_string(&f.hdr_formats)?,
            serde_json::to_string(&f.checksums)?,
            serde_json::to_string(&f.hdr_details)?,
        ])?;
        let file_id = conn.last_insert_rowid();
        for (spos, a) in f.audio_streams.iter().enumerate() {
            insert_stream.execute(params![
                file_id,
                "audio",
                spos as i64,
                a.codec,
                a.codec_id,
                a.language,
                a.title,
                flags_json(&a.flags)?,
                a.channels,
                a.bitrate_kbps,
                a.bit_depth,
                a.sample_rate_hz,
            ])?;
        }
        for (spos, s) in f.subtitles.iter().enumerate() {
            insert_stream.execute(params![
                file_id,
                "subtitle",
                spos as i64,
                s.codec,
                s.codec_id,
                s.language,
                s.title,
                flags_json(&s.flags)?,
                0,
                0,
                0,
                0,
            ])?;
        }
    }
    Ok(())
}

fn flags_json(flags: &BTreeSet<StreamFlag>) -> DbResult<String> {
    Ok(serde_json::to_string(flags)?)
}

fn load_media_files(conn: &Connection, id: i64) -> DbResult<Vec<MediaFile>> {
    let mut streams = load_streams(conn, id)?;
    let mut stmt =
        conn.prepare_cached("SELECT * FROM media_file WHERE item_id = ?1 ORDER BY position, id")?;
    let mut rows = stmt.query([id])?;
    let mut files = Vec::new();
    while let Some(row) = rows.next()? {
        let file_id: i64 = row.get("id")?;
        let mut file = read_media_file(row)?;
        if let Some((audio, subs)) = streams.remove(&file_id) {
            file.audio_streams = audio;
            file.subtitles = subs;
        }
        files.push(file);
    }
    Ok(files)
}

fn read_media_file(row: &Row<'_>) -> DbResult<MediaFile> {
    Ok(MediaFile {
        kind: parse_enum(&row.get::<_, String>("kind")?, "media file type")?,
        directory: row.get("directory")?,
        filename: row.get("filename")?,
        size_bytes: to_u64(row.get("size_bytes")?),
        modified_ms: row.get("modified_ms")?,
        video_codec: row.get("video_codec")?,
        codec_id: row.get("codec_id")?,
        container_format: row.get("container_format")?,
        exact_video_format: row.get("exact_video_format")?,
        video_3d: parse_enum(&row.get::<_, String>("video_3d")?, "3D format")?,
        width: to_u32(row.get("width")?),
        height: to_u32(row.get("height")?),
        aspect_ratio: row.get("aspect_ratio")?,
        aspect_ratio2: row.get("aspect_ratio2")?,
        pixel_aspect_ratio: row.get("pixel_aspect_ratio")?,
        video_bitrate_kbps: to_u32(row.get("video_bitrate_kbps")?),
        overall_bitrate_kbps: to_u32(row.get("overall_bitrate_kbps")?),
        bit_depth: to_u32(row.get("bit_depth")?),
        frame_rate: row.get("frame_rate")?,
        duration_secs: to_u32(row.get("duration_secs")?),
        stacking: to_u32(row.get("stacking")?),
        stacking_marker: row.get("stacking_marker")?,
        title: row.get("title")?,
        is_iso: row.get("is_iso")?,
        is_animated_graphic: row.get("is_animated_graphic")?,
        hdr_formats: serde_json::from_str(&row.get::<_, String>("hdr_formats")?)?,
        checksums: serde_json::from_str(&row.get::<_, String>("checksums")?)?,
        hdr_details: serde_json::from_str(&row.get::<_, String>("hdr_details")?)?,
        audio_streams: Vec::new(),
        subtitles: Vec::new(),
    })
}

type FileStreams = (Vec<AudioStream>, Vec<SubtitleStream>);

/// All streams of an item's files, grouped by media file id.
fn load_streams(conn: &Connection, id: i64) -> DbResult<BTreeMap<i64, FileStreams>> {
    let mut stmt = conn.prepare_cached(
        "SELECT s.media_file_id, s.kind, s.codec, s.codec_id, s.language, s.title, s.flags,
                s.channels, s.bitrate_kbps, s.bit_depth, s.sample_rate
         FROM stream s JOIN media_file m ON m.id = s.media_file_id
         WHERE m.item_id = ?1 ORDER BY s.media_file_id, s.kind, s.position, s.id",
    )?;
    let mut rows = stmt.query([id])?;
    let mut out: BTreeMap<i64, FileStreams> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let file_id: i64 = row.get(0)?;
        let kind: String = row.get(1)?;
        let flags: BTreeSet<StreamFlag> = serde_json::from_str(&row.get::<_, String>(6)?)?;
        let slot = out.entry(file_id).or_default();
        match kind.as_str() {
            "audio" => slot.0.push(AudioStream {
                codec: row.get(2)?,
                codec_id: row.get(3)?,
                language: row.get(4)?,
                title: row.get(5)?,
                flags,
                channels: to_u32(row.get(7)?),
                bitrate_kbps: to_u32(row.get(8)?),
                bit_depth: to_u32(row.get(9)?),
                sample_rate_hz: to_u32(row.get(10)?),
            }),
            "subtitle" => slot.1.push(SubtitleStream {
                codec: row.get(2)?,
                codec_id: row.get(3)?,
                language: row.get(4)?,
                title: row.get(5)?,
                flags,
            }),
            other => {
                return Err(DbError::InvalidData(format!(
                    "unknown stream kind `{other}`"
                )));
            }
        }
    }
    Ok(out)
}

// ---- credits and people ----------------------------------------------------------------------

fn save_credits(conn: &Connection, id: i64, item: &Item) -> DbResult<()> {
    conn.prepare_cached("DELETE FROM credit WHERE item_id = ?1")?
        .execute([id])?;
    let mut stmt = conn.prepare_cached(
        "INSERT INTO credit (item_id, person_id, kind, role, position)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (pos, c) in item.core.credits.iter().enumerate() {
        let person_id = upsert_person(conn, &c.person)?;
        stmt.execute(params![
            id,
            person_id,
            enum_text(&c.kind)?,
            c.role,
            pos as i64
        ])?;
    }
    Ok(())
}

/// Adds a provider ID to every person whose TMDB ID is `tmdb_person` (an IMDb `nm` ID read later
/// from the provider), keeping the IDs they have. Returns how many people changed.
pub(crate) fn add_person_id(
    conn: &Connection,
    tmdb_person: &str,
    key: &str,
    value: &str,
) -> DbResult<usize> {
    Ok(conn
        .prepare_cached(
            "UPDATE person SET ids = json_set(ids, '$.' || ?2, ?3)
             WHERE json_extract(ids, '$.tmdb') = ?1
               AND COALESCE(json_extract(ids, '$.' || ?2), '') <> ?3",
        )?
        .execute(params![tmdb_person, key, value])?)
}

/// Finds the person with this name (ignoring case) and provider IDs, or creates them. A person
/// with the same TMDB ID is the same person even when more IDs were stored for them since.
///
/// People without IDs and with the same name share one row. A non-empty thumbnail or profile
/// URL replaces the stored one.
fn upsert_person(conn: &Connection, p: &Person) -> DbResult<i64> {
    let name = p.name.trim();
    let ids = serde_json::to_string(&p.ids)?;
    let found = conn
        .prepare_cached(
            "SELECT id, thumb_url, profile_url FROM person
             WHERE name = ?1 COLLATE NOCASE
               AND (ids = ?2 OR (?3 IS NOT NULL AND json_extract(ids, '$.tmdb') = ?3))
             ORDER BY id LIMIT 1",
        )?
        .query_row(params![name, ids, p.ids.get("tmdb")], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .optional()?;
    match found {
        Some((person_id, thumb, profile)) => {
            let new_thumb = if p.thumb_url.is_empty() {
                &thumb
            } else {
                &p.thumb_url
            };
            let new_profile = if p.profile_url.is_empty() {
                &profile
            } else {
                &p.profile_url
            };
            if *new_thumb != thumb || *new_profile != profile {
                conn.prepare_cached(
                    "UPDATE person SET thumb_url = ?2, profile_url = ?3 WHERE id = ?1",
                )?
                .execute(params![person_id, new_thumb, new_profile])?;
            }
            Ok(person_id)
        }
        None => {
            conn.prepare_cached(
                "INSERT INTO person (name, thumb_url, profile_url, ids) VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![name, p.thumb_url, p.profile_url, ids])?;
            Ok(conn.last_insert_rowid())
        }
    }
}

fn load_credits(conn: &Connection, id: i64) -> DbResult<Vec<Credit>> {
    let mut stmt = conn.prepare_cached(
        "SELECT c.kind, c.role, p.name, p.thumb_url, p.profile_url, p.ids
         FROM credit c JOIN person p ON p.id = c.person_id
         WHERE c.item_id = ?1 ORDER BY c.position, c.id",
    )?;
    let rows = stmt
        .query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(kind, role, name, thumb_url, profile_url, ids)| {
            Ok(Credit {
                kind: parse_enum(&kind, "credit kind")?,
                role,
                person: Person {
                    name,
                    ids: serde_json::from_str(&ids)?,
                    thumb_url,
                    profile_url,
                },
            })
        })
        .collect()
}

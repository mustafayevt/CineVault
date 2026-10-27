//! What each catalogue filter tests, as SQL.
//!
//! A filter is one of five shapes. Every shape except [`Spec::Flag`] reads from a *source*: a
//! `SELECT owner, value` query where `owner` is the id of the library item (the movie, or the
//! show) that the value counts for. A show owns the values of its episodes, so "video codec
//! HEVC" finds a show when any of its episodes is HEVC. A condition compiles to a correlated
//! `EXISTS` over its source, which makes ANY, ALL, negation and the live counts of a facet come
//! out of the same query.
//!
//! The conditions never evaluate to NULL, so wrapping them in `NOT` is always the exact inverse.
//! Item columns are referenced as `item`, the row being tested; sources use their own aliases.

use cv_domain::{CreditKind, id_key};

use super::ids::{FilterId, Scope};
use crate::repo::codec::enum_text;
use crate::repo::query::season_count_sql;

/// The shape of a catalogue filter and the SQL behind it.
#[derive(Debug, Clone)]
pub(crate) enum Spec {
    /// A yes/no condition on `item`.
    Flag(String),
    /// Values compared for equality, ignoring case.
    Multi(String),
    /// Values searched for text.
    Text(String),
    /// Numbers compared with an operator.
    Number(String),
    /// ISO dates compared with an operator.
    Date(String),
}

/// A video file.
const VIDEO: &str = "f.kind = 'video'";

/// "Edition only in the file name": the movie has no edition, and its original video name holds
/// one of the edition spellings the scanner knows (`cv_domain::EDITION_SPELLINGS`) as whole
/// words. The name is lower-cased, its delimiters become spaces and its apostrophes go, the way
/// the scanner splits it. The spellings are constants, never user text.
fn edition_lost() -> String {
    let mut name = "lower(item.original_filename)".to_owned();
    for delimiter in [".", "_", "-", "(", ")", "[", "]", "{", "}", ",", "+"] {
        name = format!("replace({name}, '{delimiter}', ' ')");
    }
    for apostrophe in ["''", "\u{2019}", "`", "\u{b4}"] {
        name = format!("replace({name}, '{apostrophe}', '')");
    }
    for _ in 0..3 {
        name = format!("replace({name}, '  ', ' ')");
    }
    let words: Vec<String> = cv_domain::EDITION_SPELLINGS
        .iter()
        .map(|(_, spelling)| format!("w.name LIKE '% {spelling} %'"))
        .collect();
    format!(
        "(item.edition = '' AND item.original_filename <> '' AND EXISTS \
         (SELECT 1 FROM (SELECT ' ' || {name} || ' ' AS name) w WHERE {}))",
        words.join(" OR ")
    )
}

/// The value of the HDR filter for a video with known media info and no HDR format.
pub const SDR: &str = "SDR";

/// Sources for the library being filtered: movies read their own rows and files, shows read
/// their own rows for show-level facts and their episodes' rows and files for the rest.
struct Sources {
    show: bool,
}

impl Sources {
    /// One row per item, owned by the item itself.
    fn own(&self, cols: &str, cond: &str) -> String {
        format!("SELECT i.id AS owner, {cols} FROM item i WHERE {cond}")
    }

    /// One row per playable item: the movie itself, or each episode of the show.
    fn play(&self, cols: &str, cond: &str) -> String {
        if self.show {
            format!(
                "SELECT i.parent_id AS owner, {cols} FROM item i \
                 WHERE i.kind = 'episode' AND {cond}"
            )
        } else {
            self.own(cols, cond)
        }
    }

    /// Files of the playable items.
    fn files(&self, cols: &str, extra_from: &str, cond: &str) -> String {
        if self.show {
            format!(
                "SELECT e.parent_id AS owner, {cols} FROM media_file f \
                 JOIN item e ON e.id = f.item_id AND e.kind = 'episode'{extra_from} WHERE {cond}"
            )
        } else {
            self.own_files(cols, extra_from, cond)
        }
    }

    /// Files recorded on the item itself (a show's artwork, a movie's files).
    fn own_files(&self, cols: &str, extra_from: &str, cond: &str) -> String {
        format!("SELECT f.item_id AS owner, {cols} FROM media_file f{extra_from} WHERE {cond}")
    }

    /// Audio or subtitle streams of the playable items' files.
    fn streams(&self, kind: &str, cols: &str, extra_from: &str, cond: &str) -> String {
        if self.show {
            format!(
                "SELECT e.parent_id AS owner, {cols} FROM stream s \
                 JOIN media_file f ON f.id = s.media_file_id \
                 JOIN item e ON e.id = f.item_id AND e.kind = 'episode'{extra_from} \
                 WHERE s.kind = '{kind}' AND {cond}"
            )
        } else {
            format!(
                "SELECT f.item_id AS owner, {cols} FROM stream s \
                 JOIN media_file f ON f.id = s.media_file_id{extra_from} \
                 WHERE s.kind = '{kind}' AND {cond}"
            )
        }
    }

    /// Every file that counts as "a file of the item": the playable items' files and, for a
    /// show, its own.
    fn any_files(&self, cols: &str, cond: &str) -> String {
        if self.show {
            union([self.files(cols, "", cond), self.own_files(cols, "", cond)])
        } else {
            self.files(cols, "", cond)
        }
    }

    /// An artwork file of one type, measured by one side.
    fn artwork_size(&self, kind: &str, side: &str) -> Spec {
        Spec::Number(self.own_files(
            &format!("f.{side} AS value"),
            "",
            &format!("f.kind = '{kind}' AND f.{side} > 0"),
        ))
    }

    /// Provider IDs of the item itself (`episode` false) or of its episodes.
    fn ids(&self, provider: &str, episode: bool) -> Spec {
        Spec::Text(if episode {
            format!(
                "SELECT e.parent_id AS owner, x.value AS value FROM ext_id x \
                 JOIN item e ON e.id = x.item_id \
                 WHERE e.kind = 'episode' AND x.provider = '{provider}'"
            )
        } else {
            format!(
                "SELECT x.item_id AS owner, x.value AS value FROM ext_id x \
                 WHERE x.provider = '{provider}'"
            )
        })
    }
}

fn union(parts: impl IntoIterator<Item = String>) -> String {
    parts.into_iter().collect::<Vec<_>>().join(" UNION ALL ")
}

/// `EXISTS` over a source, for flags that ask "does any of the item's rows ...".
fn exists_in(source: &str) -> String {
    format!("EXISTS (SELECT 1 FROM ({source}) v WHERE v.owner = item.id)")
}

/// The resolution label of a video file `f`, with the rules of `cv_media::resolution`: a bucket
/// is reached at its height less 1 %, or, for the 16:9 buckets, at its width less 1 %; the
/// highest bucket reached wins.
fn resolution_case() -> String {
    // (label, height, 16:9 width)
    const BUCKETS: [(&str, u32, Option<u32>); 14] = [
        ("4320p", 4320, Some(7680)),
        ("2160p", 2160, Some(3840)),
        ("1440p", 1440, None),
        ("1080p", 1080, Some(1920)),
        ("720p", 720, Some(1280)),
        ("576p", 576, None),
        ("540p", 540, None),
        ("480p", 480, None),
        ("360p", 360, None),
        ("288p", 288, None),
        ("240p", 240, None),
        ("144p", 144, None),
        ("120p", 120, None),
        ("96p", 96, None),
    ];
    let mut sql = String::from("CASE");
    for (label, height, width) in BUCKETS {
        sql.push_str(&format!(" WHEN f.height * 100 >= {}", height * 99));
        if let Some(width) = width {
            sql.push_str(&format!(" OR f.width * 100 >= {}", width * 99));
        }
        sql.push_str(&format!(" THEN '{label}'"));
    }
    sql.push_str(" END");
    sql
}

/// Artwork types the "missing artwork" filter checks: label and file type.
const ARTWORK: [(&str, &str); 8] = [
    ("Poster", "poster"),
    ("Fan art", "fanart"),
    ("Banner", "banner"),
    ("Clear art", "clearart"),
    ("Clear logo", "clearlogo"),
    ("Disc art", "disc"),
    ("Thumb", "thumb"),
    ("Key art", "keyart"),
];

fn missing_artwork() -> String {
    let types = ARTWORK
        .iter()
        .map(|(label, kind)| format!("SELECT '{label}' AS label, '{kind}' AS kind"))
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    format!(
        "SELECT i.id AS owner, t.label AS value FROM item i, ({types}) t \
         WHERE NOT EXISTS (SELECT 1 FROM media_file f WHERE f.item_id = i.id AND f.kind = t.kind)"
    )
}

fn missing_metadata(s: &Sources) -> String {
    let actor = enum_text(&CreditKind::Actor).unwrap_or_default();
    let director = enum_text(&CreditKind::Director).unwrap_or_default();
    let credit = |kind: &str| {
        format!("NOT EXISTS (SELECT 1 FROM credit c WHERE c.item_id = i.id AND c.kind = '{kind}')")
    };
    union([
        s.own("'Plot' AS value", "i.plot = ''"),
        s.own("'Year' AS value", "i.year = 0"),
        s.own("'Runtime' AS value", "i.runtime_minutes = 0"),
        s.own("'Certification' AS value", "i.certification = ''"),
        s.own("'Release date' AS value", "i.release_date IS NULL"),
        s.own(
            "'Genres' AS value",
            "NOT EXISTS (SELECT 1 FROM item_genre g WHERE g.item_id = i.id)",
        ),
        s.own(
            "'Ratings' AS value",
            "NOT EXISTS (SELECT 1 FROM rating r WHERE r.item_id = i.id)",
        ),
        s.own("'Cast' AS value", &credit(&actor)),
        s.own("'Directors' AS value", &credit(&director)),
        s.own(
            "'IDs' AS value",
            "NOT EXISTS (SELECT 1 FROM ext_id x WHERE x.item_id = i.id)",
        ),
    ])
}

/// "The item `x` has a subtitle", embedded or as a file next to the video.
fn has_subtitle(item: &str) -> String {
    format!(
        "(EXISTS (SELECT 1 FROM media_file m WHERE m.item_id = {item} AND m.kind = 'subtitle') \
         OR EXISTS (SELECT 1 FROM stream s JOIN media_file m ON m.id = s.media_file_id \
                    WHERE m.item_id = {item} AND s.kind = 'subtitle'))"
    )
}

/// The number of subtitles of an item `i`: embedded streams plus subtitle files that have no
/// stream rows of their own.
const SUBTITLE_COUNT: &str = "((SELECT COUNT(*) FROM stream s \
        JOIN media_file m ON m.id = s.media_file_id \
        WHERE m.item_id = i.id AND s.kind = 'subtitle') \
     + (SELECT COUNT(*) FROM media_file m WHERE m.item_id = i.id AND m.kind = 'subtitle' \
        AND NOT EXISTS (SELECT 1 FROM stream s WHERE s.media_file_id = m.id)))";

fn children_of(kind: &str) -> String {
    format!("SELECT 1 FROM item c WHERE c.parent_id = item.id AND c.kind = '{kind}'")
}

/// The SQL behind a filter for a library, or `None` when the library has no such filter.
pub(crate) fn spec(scope: Scope, id: FilterId) -> Option<Spec> {
    if !scope.has(id) {
        return None;
    }
    if scope == Scope::Set {
        return Some(set_spec(id));
    }
    let show = scope == Scope::Show;
    let s = Sources { show };
    let crc32 = "COALESCE(json_extract(f.checksums, '$.crc32'), '') <> ''";

    Some(match id {
        // ---- details ----------------------------------------------------------------------
        FilterId::AllInOne => Spec::Text(union([
            s.own("i.title AS value", "1"),
            s.own("i.original_title AS value", "1"),
            s.own("i.sort_title AS value", "1"),
            s.own("i.english_title AS value", "1"),
            s.own("i.plot AS value", "1"),
            s.own("i.tagline AS value", "1"),
            s.own("i.note AS value", "1"),
            s.own("i.path AS value", "1"),
            "SELECT l.item_id AS owner, n.name AS value FROM item_tag l \
             JOIN tag n ON n.id = l.tag_id"
                .to_owned(),
            "SELECT l.item_id AS owner, n.name AS value FROM item_genre l \
             JOIN genre n ON n.id = l.genre_id"
                .to_owned(),
            "SELECT cr.item_id AS owner, p.name AS value FROM credit cr \
             JOIN person p ON p.id = cr.person_id"
                .to_owned(),
            "SELECT i.id AS owner, j.value AS value FROM item i, \
             json_each(i.production_companies) j"
                .to_owned(),
            s.any_files("f.filename AS value", "1"),
        ])),
        FilterId::DataSource => Spec::Multi(s.own("i.data_source AS value", "i.data_source <> ''")),
        FilterId::Watched => Spec::Flag(if show {
            format!(
                "(EXISTS ({all}) AND NOT EXISTS ({all} AND c.watched = 0))",
                all = children_of("episode")
            )
        } else {
            "item.watched = 1".to_owned()
        }),
        FilterId::Locked => Spec::Flag("item.locked = 1".to_owned()),
        FilterId::DateAdded => Spec::Date(s.own(
            "date(i.date_added_ms / 1000, 'unixepoch') AS value",
            "i.date_added_ms > 0",
        )),
        FilterId::Year => Spec::Number(s.own("i.year AS value", "i.year > 0")),
        FilterId::Decade => {
            Spec::Multi(s.own("((i.year / 10) * 10) || 's' AS value", "i.year > 0"))
        }
        FilterId::Genre => Spec::Multi(
            "SELECT l.item_id AS owner, n.name AS value FROM item_genre l \
             JOIN genre n ON n.id = l.genre_id"
                .to_owned(),
        ),
        FilterId::Certification => {
            Spec::Multi(s.own("i.certification AS value", "i.certification <> ''"))
        }
        FilterId::Cast => Spec::Text(
            "SELECT cr.item_id AS owner, p.name AS value FROM credit cr \
             JOIN person p ON p.id = cr.person_id"
                .to_owned(),
        ),
        FilterId::Country => Spec::Text(if show {
            s.own("i.country AS value", "i.country <> ''")
        } else {
            "SELECT i.id AS owner, j.value AS value FROM item i, \
             json_each(i.ext, '$.countries') j"
                .to_owned()
        }),
        FilterId::Edition => Spec::Multi(s.play("i.edition AS value", "i.edition <> ''")),
        FilterId::Note => Spec::Text(s.own("i.note AS value", "i.note <> ''")),
        FilterId::MediaSource => {
            Spec::Multi(s.play("i.media_source AS value", "i.media_source <> ''"))
        }
        FilterId::Runtime => {
            Spec::Number(s.own("i.runtime_minutes AS value", "i.runtime_minutes > 0"))
        }
        FilterId::ImdbId => s.ids(id_key::IMDB, false),
        FilterId::TmdbId => s.ids(id_key::TMDB, false),
        FilterId::TraktId => s.ids(id_key::TRAKT, false),
        FilterId::TvdbId => s.ids(id_key::TVDB, false),
        FilterId::EpisodeImdbId => s.ids(id_key::IMDB, true),
        FilterId::EpisodeTmdbId => s.ids(id_key::TMDB, true),
        FilterId::EpisodeTvdbId => s.ids(id_key::TVDB, true),

        // ---- video ------------------------------------------------------------------------
        FilterId::VideoFormat => {
            Spec::Multi(s.files(&format!("{} AS value", resolution_case()), "", VIDEO))
        }
        FilterId::VideoCodec => Spec::Multi(s.files(
            "f.video_codec AS value",
            "",
            &format!("{VIDEO} AND f.video_codec <> ''"),
        )),
        FilterId::VideoBitrate => Spec::Number(s.files(
            "f.video_bitrate_kbps AS value",
            "",
            &format!("{VIDEO} AND f.video_bitrate_kbps > 0"),
        )),
        FilterId::VideoBitdepth => Spec::Number(s.files(
            "f.bit_depth AS value",
            "",
            &format!("{VIDEO} AND f.bit_depth > 0"),
        )),
        FilterId::VideoContainer => Spec::Multi(s.files(
            "f.container_format AS value",
            "",
            &format!("{VIDEO} AND f.container_format <> ''"),
        )),
        FilterId::AspectRatio => Spec::Multi(s.files(
            "printf('%.2f', f.aspect_ratio) AS value",
            "",
            &format!("{VIDEO} AND f.aspect_ratio > 0"),
        )),
        FilterId::FrameRate => Spec::Multi(s.files(
            "printf('%g', round(f.frame_rate, 3)) AS value",
            "",
            &format!("{VIDEO} AND f.frame_rate > 0"),
        )),
        // `SDR` is a value too: a video whose size is known (media info has run) and that carries
        // no HDR format. A video without media info is neither SDR nor HDR.
        FilterId::HdrFormat => Spec::Multi(format!(
            "{} UNION ALL {}",
            s.files("h.value AS value", ", json_each(f.hdr_formats) h", VIDEO),
            s.files(
                &format!("'{SDR}' AS value"),
                "",
                &format!(
                    "{VIDEO} AND (f.height > 0 OR f.width > 0) \
                     AND coalesce(json_array_length(f.hdr_formats), 0) = 0"
                ),
            ),
        )),
        FilterId::Video3d => Spec::Multi(s.files(
            "f.video_3d AS value",
            "",
            &format!("{VIDEO} AND f.video_3d <> ''"),
        )),
        FilterId::VideoCrc32 | FilterId::EpisodeCrc32 => Spec::Flag(exists_in(&s.files(
            "1 AS value",
            "",
            &format!("{VIDEO} AND {crc32}"),
        ))),

        // ---- audio ------------------------------------------------------------------------
        FilterId::AudioCodec => {
            Spec::Multi(s.streams("audio", "s.codec AS value", "", "s.codec <> ''"))
        }
        FilterId::AudioChannel => {
            Spec::Multi(s.streams("audio", "s.channels || 'ch' AS value", "", "s.channels > 0"))
        }
        FilterId::AudioLanguage => {
            Spec::Multi(s.streams("audio", "s.language AS value", "", "s.language <> ''"))
        }
        FilterId::AudioTitle => {
            Spec::Multi(s.streams("audio", "s.title AS value", "", "s.title <> ''"))
        }
        FilterId::AudioStreamCount => Spec::Number(s.files(
            "(SELECT COUNT(*) FROM stream s WHERE s.media_file_id = f.id AND s.kind = 'audio') \
             AS value",
            "",
            VIDEO,
        )),

        // ---- artwork ----------------------------------------------------------------------
        FilterId::PosterSize => s.artwork_size("poster", "height"),
        FilterId::KeyArtSize => s.artwork_size("keyart", "height"),
        FilterId::FanartSize => s.artwork_size("fanart", "width"),
        FilterId::BannerSize => s.artwork_size("banner", "width"),
        FilterId::ThumbSize => s.artwork_size("thumb", "width"),
        FilterId::ClearLogoSize => s.artwork_size("clearlogo", "width"),
        FilterId::ClearArtSize => s.artwork_size("clearart", "width"),
        FilterId::DiscArtSize => s.artwork_size("disc", "width"),
        FilterId::MissingArtwork => Spec::Multi(missing_artwork()),

        // ---- files ------------------------------------------------------------------------
        FilterId::MediaFiles => Spec::Multi(s.any_files("f.kind AS value", "1")),
        FilterId::Filename => Spec::Text(s.any_files("f.filename AS value", "1")),
        FilterId::VideoFilename => Spec::Text(s.files("f.filename AS value", "", VIDEO)),
        FilterId::Path => Spec::Text(s.own("i.path AS value", "1")),
        FilterId::EpisodePath => Spec::Text(union([
            s.play("i.path AS value", "i.path <> ''"),
            s.files("f.directory AS value", "", VIDEO),
        ])),
        FilterId::VideoFilesize => {
            Spec::Number(s.files("f.size_bytes / 1048576.0 AS value", "", VIDEO))
        }
        FilterId::VideoExtras => Spec::Flag(exists_in(&s.files(
            "1 AS value",
            "",
            "f.kind = 'video_extra'",
        ))),

        // ---- subtitles --------------------------------------------------------------------
        FilterId::SubtitleCount => Spec::Number(s.play(&format!("{SUBTITLE_COUNT} AS value"), "1")),
        FilterId::SubtitleLanguage => {
            Spec::Multi(s.streams("subtitle", "s.language AS value", "", "s.language <> ''"))
        }
        FilterId::SubtitleFormat => Spec::Multi(union([
            s.streams("subtitle", "s.codec AS value", "", "s.codec <> ''"),
            // A subtitle file without stream rows is described by its extension.
            s.files(
                "lower(replace(f.filename, rtrim(f.filename, replace(f.filename, '.', '')), '')) \
                 AS value",
                "",
                "f.kind = 'subtitle' AND instr(f.filename, '.') > 0 \
                 AND NOT EXISTS (SELECT 1 FROM stream s WHERE s.media_file_id = f.id)",
            ),
        ])),
        FilterId::SubtitleType => Spec::Multi(s.streams(
            "subtitle",
            "t.value AS value",
            ", json_each(s.flags) t",
            "1",
        )),
        FilterId::SubtitleTitle => {
            Spec::Multi(s.streams("subtitle", "s.title AS value", "", "s.title <> ''"))
        }
        FilterId::MissingSubtitles => Spec::Flag(if show {
            format!(
                "EXISTS ({} AND NOT {})",
                children_of("episode"),
                has_subtitle("c.id")
            )
        } else {
            format!("NOT {}", has_subtitle("item.id"))
        }),

        // ---- metadata ---------------------------------------------------------------------
        FilterId::MissingMetadata => Spec::Multi(missing_metadata(&s)),
        FilterId::DifferentRuntime => Spec::Number(s.own(
            "abs(i.runtime_minutes - (SELECT m.duration_secs FROM media_file m \
                 WHERE m.item_id = i.id AND m.kind = 'video' AND m.duration_secs > 0 \
                 ORDER BY m.position, m.id LIMIT 1) / 60) AS value",
            "i.runtime_minutes > 0",
        )),

        // ---- movies only ------------------------------------------------------------------
        FilterId::New => Spec::Flag("item.is_new = 1".to_owned()),
        FilterId::Duplicate => Spec::Flag(
            "EXISTS (SELECT 1 FROM ext_id a \
             JOIN ext_id b ON b.provider = a.provider AND b.value = a.value \
                          AND b.item_id <> a.item_id \
             JOIN item o ON o.id = b.item_id AND o.kind = item.kind \
             WHERE a.item_id = item.id)"
                .to_owned(),
        ),
        FilterId::EditionLost => Spec::Flag(edition_lost()),
        FilterId::InMovieSet => Spec::Flag(
            "EXISTS (SELECT 1 FROM item p WHERE p.id = item.parent_id AND p.kind = 'set')"
                .to_owned(),
        ),
        FilterId::ReleaseDate | FilterId::FirstAired => {
            Spec::Date(s.own("i.release_date AS value", "i.release_date IS NOT NULL"))
        }
        FilterId::Language => Spec::Text(
            "SELECT i.id AS owner, j.value AS value FROM item i, \
             json_each(i.ext, '$.spokenLanguages') j"
                .to_owned(),
        ),
        FilterId::ProductionCompany => Spec::Text(
            "SELECT i.id AS owner, j.value AS value FROM item i, \
             json_each(i.production_companies) j"
                .to_owned(),
        ),
        FilterId::TrailerFormat => Spec::Multi(
            "SELECT t.item_id AS owner, t.quality AS value FROM trailer t WHERE t.quality <> ''"
                .to_owned(),
        ),

        // ---- shows only -------------------------------------------------------------------
        FilterId::NewEpisodes => Spec::Flag(exists_in(&s.play("1 AS value", "i.is_new = 1"))),
        FilterId::DuplicateEpisodes => Spec::Flag(
            "EXISTS (SELECT 1 FROM item e1 \
             JOIN episode_number a ON a.item_id = e1.id AND a.ordering = 'aired' \
             JOIN episode_number b ON b.ordering = 'aired' AND b.group_name = a.group_name \
                                  AND b.season = a.season AND b.episode = a.episode \
                                  AND b.item_id <> a.item_id \
             JOIN item e2 ON e2.id = b.item_id AND e2.parent_id = e1.parent_id \
                         AND e2.kind = 'episode' AND e2.availability = 'online' \
             WHERE e1.parent_id = item.id AND e1.kind = 'episode' \
               AND e1.availability = 'online')"
                .to_owned(),
        ),
        FilterId::Status => Spec::Multi(s.own("i.aired_status AS value", "1")),
        FilterId::Empty => Spec::Flag(format!("NOT EXISTS ({})", children_of("episode"))),
        FilterId::Studio => Spec::Multi(
            "SELECT i.id AS owner, j.value AS value FROM item i, \
             json_each(i.production_companies) j"
                .to_owned(),
        ),
        FilterId::SeasonNote => Spec::Text(
            "SELECT c.parent_id AS owner, c.note AS value FROM item c \
             WHERE c.kind = 'season' AND c.note <> ''"
                .to_owned(),
        ),
        FilterId::EpisodeNote => Spec::Text(s.play("i.note AS value", "i.note <> ''")),
        FilterId::EpisodeCount => Spec::Number(s.own(
            "(SELECT COUNT(*) FROM item c WHERE c.parent_id = i.id AND c.kind = 'episode') \
             AS value",
            "1",
        )),
        FilterId::SeasonCount => {
            Spec::Number(s.own(&format!("{} AS value", season_count_sql("i")), "1"))
        }
        FilterId::UncategorizedEpisodes => Spec::Flag(exists_in(&s.play(
            "1 AS value",
            "NOT EXISTS (SELECT 1 FROM episode_number n WHERE n.item_id = i.id \
             AND n.ordering = 'aired' AND n.season >= 0)",
        ))),
        FilterId::MissingEpisodes => Spec::Flag(
            "COALESCE(json_array_length(item.ext, '$.missingEpisodes'), 0) > 0".to_owned(),
        ),

        // ---- ratings ----------------------------------------------------------------------
        FilterId::Top250Rank => Spec::Number(s.own("i.top250 AS value", "i.top250 > 0")),
        FilterId::RatingImdb => source_rating("imdb"),
        FilterId::RatingTmdb => source_rating("tmdb"),
        FilterId::RatingTrakt => source_rating("trakt"),
        FilterId::RatingTvdb => source_rating("tvdb"),
        FilterId::RatingMetacritic => source_rating("metacritic"),
        FilterId::RatingRtCritics => source_rating("tomatometerallcritics"),
        FilterId::RatingRtAudience => source_rating("tomatometerallaudience"),
        FilterId::RatingLetterboxd => source_rating("letterboxd"),
        FilterId::RatingMyAnimeList => source_rating("myanimelist"),
        FilterId::RatingRogerEbert => source_rating("rogerebert"),
        FilterId::RatingUser => source_rating("user"),

        // Set filters are handled above; none of them is in the movie or show lists.
        FilterId::SetMissingMovies | FilterId::SetNewMovies | FilterId::SetMultipleMovies => {
            return None;
        }
    })
}

/// The rating a source gave, in the units the source reports. The value is rounded so that a
/// stored `4.4` (a single-precision number) equals the typed `4.4`.
fn source_rating(source: &str) -> Spec {
    Spec::Number(format!(
        "SELECT r.item_id AS owner, ROUND(r.value, 3) AS value FROM rating r          WHERE r.source = '{source}'"
    ))
}

fn set_spec(id: FilterId) -> Spec {
    match id {
        FilterId::DataSource => Spec::Multi(
            "SELECT i.id AS owner, i.data_source AS value FROM item i WHERE i.data_source <> ''"
                .to_owned(),
        ),
        FilterId::SetMissingMovies => {
            Spec::Flag("COALESCE(json_array_length(item.ext, '$.missingMovies'), 0) > 0".to_owned())
        }
        FilterId::SetNewMovies => Spec::Flag(format!(
            "EXISTS ({} AND c.is_new = 1)",
            children_of("movie")
        )),
        FilterId::SetMultipleMovies => Spec::Flag(
            "(SELECT COUNT(*) FROM item c WHERE c.parent_id = item.id AND c.kind = 'movie' \
             AND c.trashed_entry IS NULL) > 1"
                .to_owned(),
        ),
        // The catalogue of sets holds only the four ids above.
        _ => Spec::Flag("0".to_owned()),
    }
}

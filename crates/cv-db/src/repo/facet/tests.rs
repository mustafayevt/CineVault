//! One table-driven case per catalogue filter against a fixture library: 64 movie, 71 show and
//! 4 movie-set filters. Every case is also run negated and must return exactly the rest of the
//! library, which proves each condition is a plain yes/no with no NULL holes.

use std::collections::BTreeSet;

use cv_domain::{
    AiredStatus, AudioStream, CalendarDate, Certification, Credit, CreditKind, Edition,
    EpisodeNumber, Genre, Item, ItemDetail, ItemId, ItemKind, MediaFile, MediaFileType,
    MediaSource, MissingEpisode, MissingMovie, Rating, StreamFlag, SubtitleStream, Trailer,
    Video3dFormat, id_key,
};
use rusqlite::Connection;

use super::*;
use crate::repo::{
    Cursor, ItemQuery, SortField, SortKey, count_items, insert_item, list_items, set_new,
};

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

// ---- fixture builders --------------------------------------------------------------------

fn file(kind: MediaFileType, dir: &str, name: &str) -> MediaFile {
    MediaFile::new(kind, dir, name)
}

fn image(kind: MediaFileType, dir: &str, name: &str, width: u32, height: u32) -> MediaFile {
    MediaFile {
        width,
        height,
        ..file(kind, dir, name)
    }
}

struct Video<'a> {
    dir: &'a str,
    name: &'a str,
    size_mib: u64,
    codec: &'a str,
    container: &'a str,
    width: u32,
    height: u32,
    aspect: f32,
    bitrate: u32,
    bit_depth: u32,
    fps: f64,
    minutes: u32,
    hdr: &'a [&'a str],
}

fn video(v: &Video<'_>) -> MediaFile {
    MediaFile {
        size_bytes: v.size_mib * 1_048_576,
        video_codec: v.codec.into(),
        container_format: v.container.into(),
        width: v.width,
        height: v.height,
        aspect_ratio: v.aspect,
        video_bitrate_kbps: v.bitrate,
        bit_depth: v.bit_depth,
        frame_rate: v.fps,
        duration_secs: v.minutes * 60,
        hdr_formats: v.hdr.iter().map(|s| (*s).to_owned()).collect(),
        ..file(MediaFileType::Video, v.dir, v.name)
    }
}

fn audio(codec: &str, language: &str, channels: u32, title: &str) -> AudioStream {
    AudioStream {
        codec: codec.into(),
        language: language.into(),
        channels,
        title: title.into(),
        ..AudioStream::default()
    }
}

fn subtitle(codec: &str, language: &str, title: &str, flag: StreamFlag) -> SubtitleStream {
    SubtitleStream {
        codec: codec.into(),
        language: language.into(),
        title: title.into(),
        flags: [flag].into(),
        ..SubtitleStream::default()
    }
}

/// Adds `(source, value, scale)` ratings to an item.
fn rate(item: &mut Item, ratings: &[(&str, f32, u32)]) {
    for (source, value, max) in ratings {
        item.core
            .ratings
            .insert(Rating::new(*source, *value, 100, *max));
    }
}

fn movie_fields(item: &mut Item) -> &mut cv_domain::MovieFields {
    let ItemDetail::Movie(m) = &mut item.detail else {
        unreachable!()
    };
    m
}

fn show_fields(item: &mut Item) -> &mut cv_domain::ShowFields {
    let ItemDetail::Show(s) = &mut item.detail else {
        unreachable!()
    };
    s
}

fn episode_fields(item: &mut Item) -> &mut cv_domain::EpisodeFields {
    let ItemDetail::Episode(e) = &mut item.detail else {
        unreachable!()
    };
    e
}

fn date(year: i32, month: u8, day: u8) -> Option<CalendarDate> {
    CalendarDate::new(year, month, day)
}

// ---- the movie library ---------------------------------------------------------------------

/// Echo is empty on purpose; Alpha has something for every filter.
fn movie_library(conn: &Connection) {
    let mut duo = Item::set("Duo Set");
    duo.core.data_source = "/m".into();
    if let ItemDetail::Set(s) = &mut duo.detail {
        s.missing_movies = vec![MissingMovie {
            title: "Part Three".into(),
            ..MissingMovie::default()
        }];
    }
    let duo = insert_item(conn, &duo).unwrap();
    let mut solo = Item::set("Solo Set");
    solo.core.data_source = "/n".into();
    let solo = insert_item(conn, &solo).unwrap();

    let dir = "/m/Alpha (1995)";
    let mut alpha = Item::movie("Alpha");
    {
        let c = &mut alpha.core;
        c.original_title = "Alfa Original".into();
        c.year = 1995;
        c.plot = "A heist crew.".into();
        c.note = "Rewatch soon".into();
        c.data_source = "/m".into();
        c.path = dir.into();
        c.date_added_ms = 1_710_028_800_000;
        c.locked = true;
        c.production_companies = vec!["Regency".into()];
        c.tags.add("Heist");
        c.tags.add("Favourite");
        c.ids.set(id_key::IMDB, "tt0113277");
        c.ids.set(id_key::TMDB, "949");
        c.ids.set(id_key::TRAKT, "tr-100");
        c.credits
            .push(Credit::new(CreditKind::Actor, "Al Pacino", "Hanna"));
        c.credits
            .push(Credit::new(CreditKind::Director, "Michael Mann", ""));
        let mut main = video(&Video {
            dir,
            name: "Alpha.2160p.mkv",
            size_mib: 4096,
            codec: "HEVC",
            container: "Matroska",
            width: 3840,
            height: 1608,
            aspect: 2.39,
            bitrate: 20_000,
            bit_depth: 10,
            fps: 23.976,
            minutes: 120,
            hdr: &["Dolby Vision", "HDR10"],
        });
        main.video_3d = Video3dFormat::SideBySide;
        main.checksums.insert("crc32".into(), "DEADBEEF".into());
        main.audio_streams = vec![
            audio("TrueHD Atmos", "eng", 8, "Main"),
            audio("AC3", "fra", 6, "Dub"),
        ];
        main.subtitles = vec![subtitle("PGS", "eng", "English Forced", StreamFlag::Forced)];
        c.media_files = vec![
            main,
            file(MediaFileType::Subtitle, dir, "Alpha.en.srt"),
            file(MediaFileType::VideoExtra, dir, "Alpha - Making of.mkv"),
            file(MediaFileType::Nfo, dir, "movie.nfo"),
            image(MediaFileType::Poster, dir, "poster.jpg", 1000, 1500),
            image(MediaFileType::Fanart, dir, "fanart.jpg", 1920, 1080),
            image(MediaFileType::Clearlogo, dir, "clearlogo.png", 800, 310),
        ];
    }
    {
        let m = movie_fields(&mut alpha);
        m.runtime_minutes = 120;
        m.release_date = date(1995, 12, 15);
        m.certification = Certification("R".into());
        m.media_source = MediaSource::UhdBluray;
        m.edition = Edition::Other("Director's Cut".into());
        m.watched = true;
        m.spoken_languages = vec!["en".into(), "fr".into()];
        m.countries = vec!["US".into()];
        m.genres = vec![Genre::Drama, Genre::Crime];
        m.trailers = vec![Trailer {
            quality: "1080p".into(),
            ..Trailer::default()
        }];
    }
    rate(
        &mut alpha,
        &[
            ("imdb", 8.2, 10),
            ("tmdb", 7.9, 10),
            ("trakt", 8.0, 10),
            ("tvdb", 7.0, 10),
            ("metacritic", 76.0, 100),
            ("tomatometerallcritics", 93.0, 100),
            ("tomatometerallaudience", 88.0, 100),
            ("letterboxd", 4.4, 5),
            ("rogerebert", 3.5, 4),
            ("user", 9.0, 10),
        ],
    );
    movie_fields(&mut alpha).top250 = 42;
    insert_item(conn, &alpha).unwrap();

    let dir = "/m/Bravo (2005)";
    let mut bravo = Item::movie("Bravo");
    bravo.parent_id = Some(duo);
    rate(
        &mut bravo,
        &[
            ("imdb", 6.5, 10),
            ("tmdb", 6.1, 10),
            ("tomatometerallcritics", 55.0, 100),
            ("letterboxd", 3.0, 5),
        ],
    );
    movie_fields(&mut bravo).top250 = 120;
    {
        let c = &mut bravo.core;
        c.year = 2005;
        c.plot = "Plot B".into();
        c.data_source = "/m".into();
        c.path = dir.into();
        c.date_added_ms = 1_685_577_600_000;
        c.production_companies = vec!["Regency".into(), "Paramount".into()];
        c.tags.add("Favourite");
        c.ids.set(id_key::IMDB, "tt0000002");
        c.ids.set(id_key::TMDB, "22");
        c.credits
            .push(Credit::new(CreditKind::Actor, "Bob Hope", "Pat"));
        c.credits
            .push(Credit::new(CreditKind::Director, "Michael Mann", ""));
        let mut main = video(&Video {
            dir,
            name: "Bravo.1080p.avi",
            size_mib: 2048,
            codec: "AVC",
            container: "AVI",
            width: 1920,
            height: 1080,
            aspect: 1.78,
            bitrate: 8000,
            bit_depth: 8,
            fps: 25.0,
            minutes: 90,
            hdr: &["HDR10"],
        });
        main.audio_streams = vec![audio("AC3", "eng", 2, "")];
        c.media_files = vec![
            main,
            image(MediaFileType::Poster, dir, "poster.jpg", 680, 1000),
            image(MediaFileType::Fanart, dir, "fanart.jpg", 1280, 720),
            image(MediaFileType::Banner, dir, "banner.jpg", 758, 140),
            image(MediaFileType::Clearart, dir, "clearart.png", 1000, 562),
        ];
    }
    {
        let m = movie_fields(&mut bravo);
        m.runtime_minutes = 100;
        m.release_date = date(2005, 3, 3);
        m.certification = Certification("PG-13".into());
        m.media_source = MediaSource::WebDl;
        m.spoken_languages = vec!["en".into()];
        m.countries = vec!["GB".into()];
        m.genres = vec![Genre::Drama, Genre::Comedy];
        m.trailers = vec![Trailer {
            quality: "720p".into(),
            ..Trailer::default()
        }];
    }
    insert_item(conn, &bravo).unwrap();

    let mut charlie = Item::movie("Charlie");
    charlie.parent_id = Some(solo);
    rate(&mut charlie, &[("imdb", 7.0, 10), ("user", 5.0, 10)]);
    charlie.core.year = 1995;
    charlie.core.data_source = "/n".into();
    charlie.core.path = "/n/Charlie".into();
    charlie.core.ids.set(id_key::TMDB, "22");
    // A rename took the edition out of the file names; the original name still has it.
    charlie.core.original_filename = "Charlie - Director’s.Cut (1995) [1080p].mkv".into();
    movie_fields(&mut charlie).certification = Certification("R".into());
    insert_item(conn, &charlie).unwrap();

    let dir = "/m/Delta (2015)";
    let mut delta = Item::movie("Delta");
    delta.parent_id = Some(duo);
    rate(
        &mut delta,
        &[
            ("tmdb", 8.0, 10),
            ("trakt", 6.0, 10),
            ("metacritic", 40.0, 100),
            ("myanimelist", 8.4, 10),
        ],
    );
    {
        let c = &mut delta.core;
        c.year = 2015;
        c.plot = "Plot D".into();
        c.data_source = "/m".into();
        c.path = dir.into();
        c.date_added_ms = 1_732_060_800_000;
        c.ids.set(id_key::TMDB, "44");
        let mut main = video(&Video {
            dir,
            name: "Delta.720p.mkv",
            size_mib: 1024,
            codec: "AVC",
            container: "Matroska",
            width: 1280,
            height: 720,
            aspect: 1.78,
            bitrate: 4000,
            bit_depth: 8,
            fps: 24.0,
            minutes: 95,
            hdr: &[],
        });
        main.audio_streams = vec![audio("DTS", "eng", 6, "")];
        main.subtitles = vec![subtitle(
            "SubRip",
            "fra",
            "Francais SDH",
            StreamFlag::HearingImpaired,
        )];
        c.media_files = vec![
            main,
            image(MediaFileType::Keyart, dir, "keyart.jpg", 1000, 1500),
            image(MediaFileType::Thumb, dir, "thumb.jpg", 1280, 720),
            image(MediaFileType::Disc, dir, "disc.png", 1000, 1000),
        ];
    }
    {
        let m = movie_fields(&mut delta);
        m.runtime_minutes = 95;
        m.release_date = date(2015, 7, 1);
        m.certification = Certification("PG-13".into());
        m.media_source = MediaSource::WebDl;
        m.edition = Edition::Other("Extended".into());
        m.watched = true;
        m.genres = vec![Genre::Drama];
    }
    let delta = insert_item(conn, &delta).unwrap();
    set_new(conn, &[delta], true).unwrap();

    let mut echo = Item::movie("Echo");
    // "Cutting" is not "Cut": only whole words count.
    echo.core.original_filename = "Echo - Final Cutting (2001).mkv".into();
    insert_item(conn, &echo).unwrap();
}

// ---- the show library ------------------------------------------------------------------------

fn episode(show: ItemId, title: &str) -> Item {
    Item::episode(show, title)
}

fn show_library(conn: &Connection) {
    let dir = "/t/Sierra";
    let mut sierra = Item::show("Sierra");
    {
        let c = &mut sierra.core;
        c.year = 2010;
        c.plot = "A long story".into();
        c.note = "show note".into();
        c.data_source = "/t".into();
        c.path = dir.into();
        c.date_added_ms = 1_706_832_000_000;
        c.locked = true;
        c.production_companies = vec!["HBO".into()];
        c.tags.add("Binge");
        c.ids.set(id_key::IMDB, "tt9000001");
        c.ids.set(id_key::TMDB, "9001");
        c.ids.set(id_key::TVDB, "5551");
        c.credits
            .push(Credit::new(CreditKind::Actor, "Bryan Cranston", "Walter"));
        c.media_files = vec![
            image(MediaFileType::Poster, dir, "poster.jpg", 1000, 1500),
            image(MediaFileType::Fanart, dir, "fanart.jpg", 1920, 1080),
            image(MediaFileType::Clearart, dir, "clearart.png", 1000, 562),
            image(MediaFileType::Clearlogo, dir, "clearlogo.png", 800, 310),
            image(MediaFileType::Thumb, dir, "thumb.jpg", 1280, 720),
        ];
    }
    {
        let s = show_fields(&mut sierra);
        s.runtime_minutes = 45;
        s.first_aired = date(2010, 9, 1);
        s.status = AiredStatus::Continuing;
        s.country = "US".into();
        s.certification = Certification("TV-MA".into());
        s.genres = vec![Genre::Drama];
        s.missing_episodes = vec![MissingEpisode {
            title: "Gap".into(),
            ..MissingEpisode::default()
        }];
    }
    rate(
        &mut sierra,
        &[
            ("imdb", 9.0, 10),
            ("tmdb", 8.5, 10),
            ("trakt", 8.8, 10),
            ("tvdb", 8.0, 10),
            ("metacritic", 90.0, 100),
            ("tomatometerallcritics", 100.0, 100),
            ("tomatometerallaudience", 95.0, 100),
            ("letterboxd", 4.5, 5),
            ("myanimelist", 8.7, 10),
            ("rogerebert", 4.0, 4),
            ("user", 10.0, 10),
        ],
    );
    show_fields(&mut sierra).top250 = 7;
    let sierra = insert_item(conn, &sierra).unwrap();
    let mut season1 = Item::season(sierra, 1);
    season1.core.note = "season note".into();
    insert_item(conn, &season1).unwrap();
    insert_item(conn, &Item::season(sierra, 2)).unwrap();

    let season_dir = "/t/Sierra/Season 1";
    let mut e1 = episode(sierra, "Pilot");
    {
        let c = &mut e1.core;
        c.path = season_dir.into();
        c.note = "ep note".into();
        c.tags.add("Pilot");
        c.ids.set(id_key::IMDB, "tt91");
        c.ids.set(id_key::TMDB, "991");
        c.ids.set(id_key::TVDB, "7001");
        let mut main = video(&Video {
            dir: season_dir,
            name: "Sierra.S01E01.mkv",
            size_mib: 1024,
            codec: "AVC",
            container: "Matroska",
            width: 1920,
            height: 1080,
            aspect: 1.78,
            bitrate: 5000,
            bit_depth: 8,
            fps: 24.0,
            minutes: 45,
            hdr: &[],
        });
        main.checksums.insert("crc32".into(), "AAAA1111".into());
        main.audio_streams = vec![audio("AC3", "eng", 6, "Main")];
        main.subtitles = vec![subtitle(
            "SubRip",
            "eng",
            "Sierra English",
            StreamFlag::Default,
        )];
        c.media_files = vec![main];
    }
    {
        let e = episode_fields(&mut e1);
        e.numbers.set(EpisodeNumber::aired(1, 1));
        e.watched = true;
        e.edition = Edition::Other("Extended".into());
        e.media_source = MediaSource::WebDl;
    }
    let e1 = insert_item(conn, &e1).unwrap();
    set_new(conn, &[e1], true).unwrap();

    let mut e2 = episode(sierra, "Second");
    {
        let c = &mut e2.core;
        c.path = season_dir.into();
        let mut main = video(&Video {
            dir: season_dir,
            name: "Sierra.S01E02.mkv",
            size_mib: 512,
            codec: "HEVC",
            container: "Matroska",
            width: 1280,
            height: 720,
            aspect: 1.78,
            bitrate: 2500,
            bit_depth: 10,
            fps: 25.0,
            minutes: 45,
            hdr: &["HDR10"],
        });
        main.audio_streams = vec![audio("DTS", "eng", 2, ""), audio("AAC", "eng", 2, "")];
        c.media_files = vec![main];
    }
    episode_fields(&mut e2)
        .numbers
        .set(EpisodeNumber::aired(1, 2));
    insert_item(conn, &e2).unwrap();

    let mut e2b = episode(sierra, "Second again");
    episode_fields(&mut e2b)
        .numbers
        .set(EpisodeNumber::aired(1, 2));
    insert_item(conn, &e2b).unwrap();
    insert_item(conn, &episode(sierra, "Loose")).unwrap();

    insert_item(conn, &Item::show("Tango")).unwrap();

    let dir = "/u/Uniform";
    let mut uniform = Item::show("Uniform");
    {
        let c = &mut uniform.core;
        c.year = 2020;
        c.data_source = "/u".into();
        c.path = dir.into();
        c.date_added_ms = 1_677_801_600_000;
        c.ids.set(id_key::IMDB, "tt9000002");
        c.ids.set(id_key::TMDB, "8002");
        c.media_files = vec![
            image(MediaFileType::Poster, dir, "poster.jpg", 680, 1000),
            image(MediaFileType::Banner, dir, "banner.jpg", 758, 140),
            image(MediaFileType::Disc, dir, "disc.png", 1000, 1000),
            image(MediaFileType::Keyart, dir, "keyart.jpg", 1000, 1500),
        ];
    }
    {
        let s = show_fields(&mut uniform);
        s.first_aired = date(2020, 1, 1);
        s.status = AiredStatus::Ended;
        s.certification = Certification("TV-14".into());
        s.genres = vec![Genre::Comedy, Genre::Drama];
    }
    rate(
        &mut uniform,
        &[
            ("imdb", 7.5, 10),
            ("tmdb", 7.0, 10),
            ("trakt", 7.0, 10),
            ("tvdb", 6.0, 10),
            ("metacritic", 60.0, 100),
            ("tomatometerallcritics", 80.0, 100),
            ("tomatometerallaudience", 70.0, 100),
            ("letterboxd", 3.5, 5),
            ("myanimelist", 6.5, 10),
            ("rogerebert", 2.5, 4),
            ("user", 6.0, 10),
        ],
    );
    let uniform = insert_item(conn, &uniform).unwrap();
    insert_item(conn, &Item::season(uniform, 1)).unwrap();
    let mut solo = episode(uniform, "Solo");
    {
        let c = &mut solo.core;
        let mut main = video(&Video {
            dir: "/u/Uniform/Season 1",
            name: "Uniform.S01E01.mkv",
            size_mib: 8192,
            codec: "HEVC",
            container: "Matroska",
            width: 3840,
            height: 2160,
            aspect: 2.39,
            bitrate: 30_000,
            bit_depth: 10,
            fps: 23.976,
            minutes: 50,
            hdr: &["Dolby Vision", "HDR10"],
        });
        main.audio_streams = vec![audio("TrueHD", "eng", 8, "")];
        main.subtitles = vec![subtitle("PGS", "fra", "", StreamFlag::Forced)];
        c.media_files = vec![
            main,
            file(MediaFileType::Subtitle, "/u/Uniform/Season 1", "Solo.srt"),
        ];
    }
    {
        let e = episode_fields(&mut solo);
        e.numbers.set(EpisodeNumber::aired(1, 1));
        e.watched = true;
        e.media_source = MediaSource::UhdBluray;
    }
    insert_item(conn, &solo).unwrap();
}

// ---- running a case ----------------------------------------------------------------------------

fn titles(conn: &Connection, kind: ItemKind, filter: Filter) -> Vec<String> {
    let query = ItemQuery::new(kind).filter(filter);
    let page = list_items(conn, &query, None, 1000).unwrap();
    assert_eq!(
        count_items(conn, &query).unwrap(),
        page.items.len() as u64,
        "count and listing agree"
    );
    page.items.into_iter().map(|i| i.title).collect()
}

type Case = (FilterId, FacetCondition, &'static [&'static str]);

fn c(id: FilterId) -> FacetCondition {
    FacetCondition::new(id)
}

fn any(id: FilterId, values: &[&str]) -> FacetCondition {
    c(id).with_values(values.iter().copied())
}

fn all(id: FilterId, values: &[&str]) -> FacetCondition {
    any(id, values).with_mode(MatchMode::All)
}

fn run(conn: &Connection, kind: ItemKind, cases: &[Case]) {
    let everyone = titles(conn, kind, Filter::All);
    for (id, condition, expected) in cases {
        let got = titles(conn, kind, Filter::Facet(condition.clone()));
        assert_eq!(&got, expected, "{id:?} {condition:?}");

        // Negation is exactly the rest of the library.
        let negated = titles(conn, kind, Filter::Facet(condition.clone().negate()));
        let rest: Vec<String> = everyone
            .iter()
            .filter(|t| !expected.contains(&t.as_str()))
            .cloned()
            .collect();
        assert_eq!(negated, rest, "{id:?} negated {condition:?}");
    }
}

fn covered(cases: &[Case]) -> BTreeSet<FilterId> {
    cases.iter().map(|(id, ..)| *id).collect()
}

fn catalogue_ids(kind: ItemKind) -> BTreeSet<FilterId> {
    catalogue(kind).unwrap().iter().map(|d| d.id).collect()
}

use FilterId as F;
use NumOp::{Between, Eq, Ge, Gt, Le, Lt};

fn movie_cases() -> Vec<Case> {
    let web = MediaSource::WebDl.name().to_owned();
    let uhd = MediaSource::UhdBluray.name().to_owned();
    // The fixed strings of the cases below that are not literals.
    let web: &'static str = Box::leak(web.into_boxed_str());
    let uhd: &'static str = Box::leak(uhd.into_boxed_str());
    vec![
        (F::New, c(F::New), &["Delta"]),
        (F::DataSource, any(F::DataSource, &["/n"]), &["Charlie"]),
        (F::Watched, c(F::Watched), &["Alpha", "Delta"]),
        (F::Locked, c(F::Locked), &["Alpha"]),
        (
            F::DateAdded,
            c(F::DateAdded).with_date(Ge, "2024-01-01"),
            &["Alpha", "Delta"],
        ),
        (
            F::DateAdded,
            c(F::DateAdded).between_dates("2023-01-01", "2023-12-31"),
            &["Bravo"],
        ),
        (F::Duplicate, c(F::Duplicate), &["Bravo", "Charlie"]),
        (
            F::InMovieSet,
            c(F::InMovieSet),
            &["Bravo", "Charlie", "Delta"],
        ),
        (F::AllInOne, any(F::AllInOne, &["heist"]), &["Alpha"]),
        (
            F::AllInOne,
            any(F::AllInOne, &["heist", "comedy"]),
            &["Alpha", "Bravo"],
        ),
        (
            F::AllInOne,
            all(F::AllInOne, &["heist", "pacino"]),
            &["Alpha"],
        ),
        (
            F::Year,
            c(F::Year).with_number(Ge, 2005.0),
            &["Bravo", "Delta"],
        ),
        (
            F::Year,
            c(F::Year).between(1990.0, 1999.0),
            &["Alpha", "Charlie"],
        ),
        (
            F::ReleaseDate,
            c(F::ReleaseDate).with_date(Ge, "2005-01-01"),
            &["Bravo", "Delta"],
        ),
        (
            F::ReleaseDate,
            c(F::ReleaseDate).between_dates("1995-01-01", "1995-12-31"),
            &["Alpha"],
        ),
        (F::Decade, any(F::Decade, &["1990s"]), &["Alpha", "Charlie"]),
        (F::Decade, all(F::Decade, &["1990s", "2000s"]), &[]),
        (
            F::Genre,
            any(F::Genre, &["crime", "comedy"]),
            &["Alpha", "Bravo"],
        ),
        (F::Genre, all(F::Genre, &["Drama", "Crime"]), &["Alpha"]),
        (
            F::Certification,
            any(F::Certification, &["R"]),
            &["Alpha", "Charlie"],
        ),
        (F::Cast, any(F::Cast, &["pacino"]), &["Alpha"]),
        (F::Cast, any(F::Cast, &["mann"]), &["Alpha", "Bravo"]),
        (F::Cast, all(F::Cast, &["pacino", "hope"]), &[]),
        (F::Country, any(F::Country, &["us"]), &["Alpha"]),
        (F::Country, any(F::Country, &["gb"]), &["Bravo"]),
        (F::Language, any(F::Language, &["fr"]), &["Alpha"]),
        (
            F::ProductionCompany,
            any(F::ProductionCompany, &["regency"]),
            &["Alpha", "Bravo"],
        ),
        (
            F::ProductionCompany,
            any(F::ProductionCompany, &["paramount"]),
            &["Bravo"],
        ),
        (F::Edition, any(F::Edition, &["Director's Cut"]), &["Alpha"]),
        (
            F::Edition,
            any(F::Edition, &["Extended", "Director's Cut"]),
            &["Alpha", "Delta"],
        ),
        (F::Note, any(F::Note, &["rewatch"]), &["Alpha"]),
        (F::ImdbId, any(F::ImdbId, &["tt0113277"]), &["Alpha"]),
        (F::ImdbId, any(F::ImdbId, &["tt000"]), &["Bravo"]),
        (F::TmdbId, any(F::TmdbId, &["22"]), &["Bravo", "Charlie"]),
        (F::TraktId, any(F::TraktId, &["tr-100"]), &["Alpha"]),
        (F::VideoFormat, any(F::VideoFormat, &["2160p"]), &["Alpha"]),
        (F::VideoFormat, any(F::VideoFormat, &["1080p"]), &["Bravo"]),
        (
            F::VideoFormat,
            any(F::VideoFormat, &["720p", "1080p"]),
            &["Bravo", "Delta"],
        ),
        (F::VideoCodec, any(F::VideoCodec, &["hevc"]), &["Alpha"]),
        (
            F::VideoCodec,
            any(F::VideoCodec, &["AVC"]),
            &["Bravo", "Delta"],
        ),
        (
            F::VideoBitrate,
            c(F::VideoBitrate).with_number(Gt, 10_000.0),
            &["Alpha"],
        ),
        (
            F::VideoBitrate,
            c(F::VideoBitrate).between(4000.0, 8000.0),
            &["Bravo", "Delta"],
        ),
        (
            F::VideoBitdepth,
            c(F::VideoBitdepth).with_number(Ge, 10.0),
            &["Alpha"],
        ),
        (
            F::VideoBitdepth,
            c(F::VideoBitdepth).with_number(Eq, 8.0),
            &["Bravo", "Delta"],
        ),
        (
            F::VideoContainer,
            any(F::VideoContainer, &["Matroska"]),
            &["Alpha", "Delta"],
        ),
        (
            F::VideoContainer,
            any(F::VideoContainer, &["avi"]),
            &["Bravo"],
        ),
        (F::AspectRatio, any(F::AspectRatio, &["2.39"]), &["Alpha"]),
        (
            F::AspectRatio,
            any(F::AspectRatio, &["1.78"]),
            &["Bravo", "Delta"],
        ),
        (F::FrameRate, any(F::FrameRate, &["23.976"]), &["Alpha"]),
        (
            F::FrameRate,
            any(F::FrameRate, &["25", "24"]),
            &["Bravo", "Delta"],
        ),
        (F::Video3d, any(F::Video3d, &["3D SBS"]), &["Alpha"]),
        (F::VideoCrc32, c(F::VideoCrc32), &["Alpha"]),
        (
            F::HdrFormat,
            any(F::HdrFormat, &["HDR10"]),
            &["Alpha", "Bravo"],
        ),
        (
            F::HdrFormat,
            any(F::HdrFormat, &["dolby vision"]),
            &["Alpha"],
        ),
        (
            F::HdrFormat,
            all(F::HdrFormat, &["Dolby Vision", "HDR10"]),
            &["Alpha"],
        ),
        (F::HdrFormat, any(F::HdrFormat, &["SDR"]), &["Delta"]),
        (
            F::AudioCodec,
            any(F::AudioCodec, &["TrueHD Atmos"]),
            &["Alpha"],
        ),
        (
            F::AudioCodec,
            any(F::AudioCodec, &["AC3"]),
            &["Alpha", "Bravo"],
        ),
        (F::AudioChannel, any(F::AudioChannel, &["8ch"]), &["Alpha"]),
        (F::AudioChannel, any(F::AudioChannel, &["2ch"]), &["Bravo"]),
        (
            F::AudioChannel,
            any(F::AudioChannel, &["6ch"]),
            &["Alpha", "Delta"],
        ),
        (
            F::AudioStreamCount,
            c(F::AudioStreamCount).with_number(Eq, 2.0),
            &["Alpha"],
        ),
        (
            F::AudioStreamCount,
            c(F::AudioStreamCount).with_number(Eq, 1.0),
            &["Bravo", "Delta"],
        ),
        (
            F::AudioLanguage,
            any(F::AudioLanguage, &["fra"]),
            &["Alpha"],
        ),
        (
            F::AudioLanguage,
            any(F::AudioLanguage, &["eng"]),
            &["Alpha", "Bravo", "Delta"],
        ),
        (F::AudioTitle, any(F::AudioTitle, &["Dub"]), &["Alpha"]),
        (
            F::AudioTitle,
            all(F::AudioTitle, &["Main", "Dub"]),
            &["Alpha"],
        ),
        (
            F::PosterSize,
            c(F::PosterSize).with_number(Ge, 1500.0),
            &["Alpha"],
        ),
        (
            F::PosterSize,
            c(F::PosterSize).with_number(Eq, 1000.0),
            &["Bravo"],
        ),
        (
            F::FanartSize,
            c(F::FanartSize).with_number(Eq, 1920.0),
            &["Alpha"],
        ),
        (
            F::FanartSize,
            c(F::FanartSize).with_number(Lt, 1500.0),
            &["Bravo"],
        ),
        (
            F::BannerSize,
            c(F::BannerSize).with_number(Eq, 758.0),
            &["Bravo"],
        ),
        (
            F::ThumbSize,
            c(F::ThumbSize).with_number(Eq, 1280.0),
            &["Delta"],
        ),
        (
            F::ClearLogoSize,
            c(F::ClearLogoSize).with_number(Gt, 500.0),
            &["Alpha"],
        ),
        (
            F::ClearArtSize,
            c(F::ClearArtSize).with_number(Eq, 1000.0),
            &["Bravo"],
        ),
        (
            F::DiscArtSize,
            c(F::DiscArtSize).between(900.0, 1100.0),
            &["Delta"],
        ),
        (
            F::KeyArtSize,
            c(F::KeyArtSize).with_number(Ge, 1400.0),
            &["Delta"],
        ),
        (
            F::MissingArtwork,
            any(F::MissingArtwork, &["Poster"]),
            &["Charlie", "Delta", "Echo"],
        ),
        (
            F::MissingArtwork,
            all(F::MissingArtwork, &["Poster", "Thumb"]),
            &["Charlie", "Echo"],
        ),
        (
            F::MediaSource,
            any(F::MediaSource, &[web]),
            &["Bravo", "Delta"],
        ),
        (F::MediaSource, any(F::MediaSource, &[uhd]), &["Alpha"]),
        (
            F::MediaFiles,
            any(F::MediaFiles, &["video_extra"]),
            &["Alpha"],
        ),
        (
            F::MediaFiles,
            all(F::MediaFiles, &["nfo", "poster"]),
            &["Alpha"],
        ),
        (F::Filename, any(F::Filename, &["making"]), &["Alpha"]),
        (F::Filename, any(F::Filename, &[".avi"]), &["Bravo"]),
        (
            F::VideoFilename,
            any(F::VideoFilename, &[".mkv"]),
            &["Alpha", "Delta"],
        ),
        (F::VideoFilename, any(F::VideoFilename, &["making"]), &[]),
        (F::Path, any(F::Path, &["/n/"]), &["Charlie"]),
        (F::Path, any(F::Path, &["(1995)"]), &["Alpha"]),
        (
            F::VideoFilesize,
            c(F::VideoFilesize).with_number(Ge, 4096.0),
            &["Alpha"],
        ),
        (
            F::VideoFilesize,
            c(F::VideoFilesize).between(1000.0, 2048.0),
            &["Bravo", "Delta"],
        ),
        (
            F::SubtitleCount,
            c(F::SubtitleCount).with_number(Eq, 2.0),
            &["Alpha"],
        ),
        (
            F::SubtitleCount,
            c(F::SubtitleCount).with_number(Eq, 0.0),
            &["Bravo", "Charlie", "Echo"],
        ),
        (
            F::SubtitleLanguage,
            any(F::SubtitleLanguage, &["eng"]),
            &["Alpha"],
        ),
        (
            F::SubtitleLanguage,
            any(F::SubtitleLanguage, &["fra"]),
            &["Delta"],
        ),
        (
            F::SubtitleFormat,
            any(F::SubtitleFormat, &["pgs"]),
            &["Alpha"],
        ),
        (
            F::SubtitleFormat,
            any(F::SubtitleFormat, &["srt"]),
            &["Alpha"],
        ),
        (
            F::SubtitleFormat,
            any(F::SubtitleFormat, &["SubRip"]),
            &["Delta"],
        ),
        (
            F::SubtitleType,
            any(F::SubtitleType, &["forced"]),
            &["Alpha"],
        ),
        (
            F::SubtitleType,
            any(F::SubtitleType, &["hearing_impaired"]),
            &["Delta"],
        ),
        (
            F::SubtitleTitle,
            any(F::SubtitleTitle, &["English Forced"]),
            &["Alpha"],
        ),
        (F::VideoExtras, c(F::VideoExtras), &["Alpha"]),
        (
            F::TrailerFormat,
            any(F::TrailerFormat, &["1080p"]),
            &["Alpha"],
        ),
        (
            F::TrailerFormat,
            any(F::TrailerFormat, &["720p"]),
            &["Bravo"],
        ),
        (
            F::Runtime,
            c(F::Runtime).with_number(Ge, 100.0),
            &["Alpha", "Bravo"],
        ),
        (F::Runtime, c(F::Runtime).with_number(Lt, 100.0), &["Delta"]),
        (
            F::DifferentRuntime,
            c(F::DifferentRuntime).with_number(Ge, 5.0),
            &["Bravo"],
        ),
        (
            F::DifferentRuntime,
            c(F::DifferentRuntime).with_number(Eq, 0.0),
            &["Alpha", "Delta"],
        ),
        (
            F::MissingMetadata,
            any(F::MissingMetadata, &["Plot"]),
            &["Charlie", "Echo"],
        ),
        (
            F::MissingMetadata,
            all(F::MissingMetadata, &["Plot", "Runtime"]),
            &["Charlie", "Echo"],
        ),
        (
            F::MissingMetadata,
            any(F::MissingMetadata, &["Cast"]),
            &["Charlie", "Delta", "Echo"],
        ),
        (
            F::MissingMetadata,
            any(F::MissingMetadata, &["IDs"]),
            &["Echo"],
        ),
        (
            F::MissingSubtitles,
            c(F::MissingSubtitles),
            &["Bravo", "Charlie", "Echo"],
        ),
        // ---- ratings, in the units each source reports ----------------------------------
        (
            F::Top250Rank,
            c(F::Top250Rank).with_number(Le, 50.0),
            &["Alpha"],
        ),
        (
            F::Top250Rank,
            c(F::Top250Rank).between(100.0, 200.0),
            &["Bravo"],
        ),
        (
            F::Top250Rank,
            c(F::Top250Rank).with_number(Ge, 1.0),
            &["Alpha", "Bravo"],
        ),
        (
            F::RatingImdb,
            c(F::RatingImdb).with_number(Ge, 7.0),
            &["Alpha", "Charlie"],
        ),
        (
            F::RatingImdb,
            c(F::RatingImdb).between(6.0, 7.0),
            &["Bravo", "Charlie"],
        ),
        (
            F::RatingTmdb,
            c(F::RatingTmdb).with_number(Gt, 7.9),
            &["Delta"],
        ),
        (
            F::RatingTmdb,
            c(F::RatingTmdb).with_number(Eq, 7.9),
            &["Alpha"],
        ),
        (
            F::RatingTrakt,
            c(F::RatingTrakt).with_number(Ge, 6.0),
            &["Alpha", "Delta"],
        ),
        (
            F::RatingTvdb,
            c(F::RatingTvdb).with_number(Eq, 7.0),
            &["Alpha"],
        ),
        (
            F::RatingMetacritic,
            c(F::RatingMetacritic).with_number(Lt, 50.0),
            &["Delta"],
        ),
        (
            F::RatingMetacritic,
            c(F::RatingMetacritic).with_number(Ge, 75.0),
            &["Alpha"],
        ),
        (
            F::RatingRtCritics,
            c(F::RatingRtCritics).with_number(Ge, 90.0),
            &["Alpha"],
        ),
        (
            F::RatingRtCritics,
            c(F::RatingRtCritics).with_number(Lt, 90.0),
            &["Bravo"],
        ),
        (
            F::RatingRtAudience,
            c(F::RatingRtAudience).with_number(Ge, 80.0),
            &["Alpha"],
        ),
        (
            F::RatingLetterboxd,
            c(F::RatingLetterboxd).with_number(Ge, 4.0),
            &["Alpha"],
        ),
        (
            F::RatingLetterboxd,
            c(F::RatingLetterboxd).with_number(Eq, 4.4),
            &["Alpha"],
        ),
        (
            F::RatingLetterboxd,
            c(F::RatingLetterboxd).with_number(Le, 3.0),
            &["Bravo"],
        ),
        (
            F::RatingMyAnimeList,
            c(F::RatingMyAnimeList).with_number(Ge, 8.0),
            &["Delta"],
        ),
        (
            F::RatingRogerEbert,
            c(F::RatingRogerEbert).with_number(Ge, 3.0),
            &["Alpha"],
        ),
        (
            F::RatingUser,
            c(F::RatingUser).with_number(Ge, 5.0),
            &["Alpha", "Charlie"],
        ),
        (
            F::RatingUser,
            c(F::RatingUser).with_number(Gt, 5.0),
            &["Alpha"],
        ),
        (F::EditionLost, c(F::EditionLost), &["Charlie"]),
    ]
}

#[test]
fn every_movie_filter_has_a_case_that_matches_the_fixture_and_its_negation_is_the_rest() {
    let conn = memory_db();
    movie_library(&conn);
    let cases = movie_cases();
    run(&conn, ItemKind::Movie, &cases);
    assert_eq!(covered(&cases), catalogue_ids(ItemKind::Movie));
}

fn show_cases() -> Vec<Case> {
    let web = MediaSource::WebDl.name().to_owned();
    let uhd = MediaSource::UhdBluray.name().to_owned();
    let web: &'static str = Box::leak(web.into_boxed_str());
    let uhd: &'static str = Box::leak(uhd.into_boxed_str());
    vec![
        (F::NewEpisodes, c(F::NewEpisodes), &["Sierra"]),
        (F::DuplicateEpisodes, c(F::DuplicateEpisodes), &["Sierra"]),
        (F::Status, any(F::Status, &["continuing"]), &["Sierra"]),
        (F::Status, any(F::Status, &["ended"]), &["Uniform"]),
        (F::Status, any(F::Status, &["unknown"]), &["Tango"]),
        (F::Empty, c(F::Empty), &["Tango"]),
        (
            F::FirstAired,
            c(F::FirstAired).with_date(Ge, "2015-01-01"),
            &["Uniform"],
        ),
        (
            F::FirstAired,
            c(F::FirstAired).between_dates("2010-01-01", "2010-12-31"),
            &["Sierra"],
        ),
        (F::Studio, any(F::Studio, &["hbo"]), &["Sierra"]),
        (
            F::SeasonNote,
            any(F::SeasonNote, &["season note"]),
            &["Sierra"],
        ),
        (
            F::EpisodeNote,
            any(F::EpisodeNote, &["ep note"]),
            &["Sierra"],
        ),
        (F::TvdbId, any(F::TvdbId, &["5551"]), &["Sierra"]),
        (F::TvdbId, any(F::TvdbId, &["7001"]), &[]),
        (
            F::EpisodeImdbId,
            any(F::EpisodeImdbId, &["tt91"]),
            &["Sierra"],
        ),
        (
            F::EpisodeTmdbId,
            any(F::EpisodeTmdbId, &["991"]),
            &["Sierra"],
        ),
        (
            F::EpisodeTvdbId,
            any(F::EpisodeTvdbId, &["7001"]),
            &["Sierra"],
        ),
        (
            F::EpisodeCount,
            c(F::EpisodeCount).with_number(Eq, 4.0),
            &["Sierra"],
        ),
        (
            F::EpisodeCount,
            c(F::EpisodeCount).with_number(Eq, 1.0),
            &["Uniform"],
        ),
        (
            F::EpisodeCount,
            c(F::EpisodeCount).with_number(Eq, 0.0),
            &["Tango"],
        ),
        // Seasons with episodes: Sierra's empty season 2 record does not count.
        (
            F::SeasonCount,
            c(F::SeasonCount).with_number(Eq, 1.0),
            &["Sierra", "Uniform"],
        ),
        (
            F::SeasonCount,
            c(F::SeasonCount).with_number(Eq, 0.0),
            &["Tango"],
        ),
        (F::EpisodeCrc32, c(F::EpisodeCrc32), &["Sierra"]),
        (
            F::EpisodePath,
            any(F::EpisodePath, &["season 1"]),
            &["Sierra", "Uniform"],
        ),
        (
            F::EpisodePath,
            any(F::EpisodePath, &["/t/sierra/"]),
            &["Sierra"],
        ),
        (
            F::SubtitleCount,
            c(F::SubtitleCount).with_number(Eq, 2.0),
            &["Uniform"],
        ),
        (
            F::SubtitleCount,
            c(F::SubtitleCount).with_number(Eq, 0.0),
            &["Sierra"],
        ),
        (
            F::UncategorizedEpisodes,
            c(F::UncategorizedEpisodes),
            &["Sierra"],
        ),
        (F::MissingEpisodes, c(F::MissingEpisodes), &["Sierra"]),
        (F::Edition, any(F::Edition, &["Extended"]), &["Sierra"]),
        (
            F::AudioStreamCount,
            c(F::AudioStreamCount).with_number(Eq, 2.0),
            &["Sierra"],
        ),
        (
            F::AudioStreamCount,
            c(F::AudioStreamCount).with_number(Eq, 1.0),
            &["Sierra", "Uniform"],
        ),
        (F::AllInOne, any(F::AllInOne, &["story"]), &["Sierra"]),
        (
            F::AllInOne,
            any(F::AllInOne, &["cranston", "tango"]),
            &["Sierra", "Tango"],
        ),
        (F::AspectRatio, any(F::AspectRatio, &["1.78"]), &["Sierra"]),
        (F::AspectRatio, any(F::AspectRatio, &["2.39"]), &["Uniform"]),
        (F::AudioChannel, any(F::AudioChannel, &["6ch"]), &["Sierra"]),
        (
            F::AudioChannel,
            any(F::AudioChannel, &["8ch"]),
            &["Uniform"],
        ),
        (F::AudioCodec, any(F::AudioCodec, &["AC3"]), &["Sierra"]),
        (F::AudioCodec, any(F::AudioCodec, &["TrueHD"]), &["Uniform"]),
        (
            F::AudioLanguage,
            any(F::AudioLanguage, &["eng"]),
            &["Sierra", "Uniform"],
        ),
        (F::AudioTitle, any(F::AudioTitle, &["Main"]), &["Sierra"]),
        (
            F::BannerSize,
            c(F::BannerSize).with_number(Eq, 758.0),
            &["Uniform"],
        ),
        (
            F::ClearArtSize,
            c(F::ClearArtSize).with_number(Eq, 1000.0),
            &["Sierra"],
        ),
        (
            F::ClearLogoSize,
            c(F::ClearLogoSize).with_number(Gt, 500.0),
            &["Sierra"],
        ),
        (
            F::DiscArtSize,
            c(F::DiscArtSize).between(900.0, 1100.0),
            &["Uniform"],
        ),
        (
            F::FanartSize,
            c(F::FanartSize).with_number(Ge, 1900.0),
            &["Sierra"],
        ),
        (
            F::KeyArtSize,
            c(F::KeyArtSize).with_number(Ge, 1400.0),
            &["Uniform"],
        ),
        (
            F::PosterSize,
            c(F::PosterSize).with_number(Ge, 1500.0),
            &["Sierra"],
        ),
        (
            F::PosterSize,
            c(F::PosterSize).with_number(Eq, 1000.0),
            &["Uniform"],
        ),
        (
            F::ThumbSize,
            c(F::ThumbSize).with_number(Eq, 1280.0),
            &["Sierra"],
        ),
        (F::Cast, any(F::Cast, &["cranston"]), &["Sierra"]),
        (
            F::Certification,
            any(F::Certification, &["TV-MA"]),
            &["Sierra"],
        ),
        (F::Country, any(F::Country, &["us"]), &["Sierra"]),
        (F::DataSource, any(F::DataSource, &["/u"]), &["Uniform"]),
        (
            F::DateAdded,
            c(F::DateAdded).with_date(Ge, "2024-01-01"),
            &["Sierra"],
        ),
        (
            F::DateAdded,
            c(F::DateAdded).between_dates("2023-01-01", "2023-12-31"),
            &["Uniform"],
        ),
        (F::Decade, any(F::Decade, &["2010s"]), &["Sierra"]),
        (F::Filename, any(F::Filename, &["s01e02"]), &["Sierra"]),
        (F::Filename, any(F::Filename, &["solo.srt"]), &["Uniform"]),
        (
            F::Filename,
            any(F::Filename, &["poster"]),
            &["Sierra", "Uniform"],
        ),
        (F::FrameRate, any(F::FrameRate, &["25"]), &["Sierra"]),
        (F::FrameRate, any(F::FrameRate, &["23.976"]), &["Uniform"]),
        (F::Genre, any(F::Genre, &["comedy"]), &["Uniform"]),
        (F::Genre, all(F::Genre, &["Comedy", "Drama"]), &["Uniform"]),
        (F::Genre, any(F::Genre, &["drama"]), &["Sierra", "Uniform"]),
        (
            F::HdrFormat,
            any(F::HdrFormat, &["Dolby Vision"]),
            &["Uniform"],
        ),
        (
            F::HdrFormat,
            any(F::HdrFormat, &["HDR10"]),
            &["Sierra", "Uniform"],
        ),
        (
            F::HdrFormat,
            all(F::HdrFormat, &["HDR10", "Dolby Vision"]),
            &["Uniform"],
        ),
        (F::ImdbId, any(F::ImdbId, &["tt9000001"]), &["Sierra"]),
        (F::ImdbId, any(F::ImdbId, &["tt91"]), &[]),
        (F::Locked, c(F::Locked), &["Sierra"]),
        (
            F::MediaFiles,
            any(F::MediaFiles, &["subtitle"]),
            &["Uniform"],
        ),
        (
            F::MediaFiles,
            any(F::MediaFiles, &["video"]),
            &["Sierra", "Uniform"],
        ),
        (
            F::MediaFiles,
            all(F::MediaFiles, &["video", "poster"]),
            &["Sierra", "Uniform"],
        ),
        (F::MediaSource, any(F::MediaSource, &[web]), &["Sierra"]),
        (F::MediaSource, any(F::MediaSource, &[uhd]), &["Uniform"]),
        (
            F::MissingArtwork,
            any(F::MissingArtwork, &["Banner"]),
            &["Sierra", "Tango"],
        ),
        (
            F::MissingArtwork,
            all(F::MissingArtwork, &["Banner", "Disc art"]),
            &["Sierra", "Tango"],
        ),
        (
            F::MissingArtwork,
            any(F::MissingArtwork, &["Poster"]),
            &["Tango"],
        ),
        (
            F::MissingMetadata,
            any(F::MissingMetadata, &["Plot"]),
            &["Tango", "Uniform"],
        ),
        (
            F::MissingMetadata,
            any(F::MissingMetadata, &["IDs"]),
            &["Tango"],
        ),
        (F::MissingSubtitles, c(F::MissingSubtitles), &["Sierra"]),
        (F::Note, any(F::Note, &["show note"]), &["Sierra"]),
        (F::Path, any(F::Path, &["/t/"]), &["Sierra"]),
        (F::Runtime, c(F::Runtime).with_number(Ge, 45.0), &["Sierra"]),
        (
            F::SubtitleFormat,
            any(F::SubtitleFormat, &["SubRip"]),
            &["Sierra"],
        ),
        (
            F::SubtitleFormat,
            any(F::SubtitleFormat, &["pgs"]),
            &["Uniform"],
        ),
        (
            F::SubtitleFormat,
            any(F::SubtitleFormat, &["srt"]),
            &["Uniform"],
        ),
        (
            F::SubtitleLanguage,
            any(F::SubtitleLanguage, &["eng"]),
            &["Sierra"],
        ),
        (
            F::SubtitleLanguage,
            any(F::SubtitleLanguage, &["fra"]),
            &["Uniform"],
        ),
        (
            F::SubtitleTitle,
            any(F::SubtitleTitle, &["Sierra English"]),
            &["Sierra"],
        ),
        (
            F::SubtitleType,
            any(F::SubtitleType, &["default"]),
            &["Sierra"],
        ),
        (
            F::SubtitleType,
            any(F::SubtitleType, &["forced"]),
            &["Uniform"],
        ),
        (F::TmdbId, any(F::TmdbId, &["9001"]), &["Sierra"]),
        (
            F::VideoBitdepth,
            c(F::VideoBitdepth).with_number(Ge, 10.0),
            &["Sierra", "Uniform"],
        ),
        (
            F::VideoBitdepth,
            c(F::VideoBitdepth).with_number(Eq, 8.0),
            &["Sierra"],
        ),
        (
            F::VideoBitrate,
            c(F::VideoBitrate).with_number(Gt, 20_000.0),
            &["Uniform"],
        ),
        (
            F::VideoBitrate,
            c(F::VideoBitrate).between(2000.0, 3000.0),
            &["Sierra"],
        ),
        (
            F::VideoCodec,
            any(F::VideoCodec, &["HEVC"]),
            &["Sierra", "Uniform"],
        ),
        (F::VideoCodec, any(F::VideoCodec, &["AVC"]), &["Sierra"]),
        (
            F::VideoContainer,
            any(F::VideoContainer, &["Matroska"]),
            &["Sierra", "Uniform"],
        ),
        (
            F::VideoFilename,
            any(F::VideoFilename, &["s01e01"]),
            &["Sierra", "Uniform"],
        ),
        (F::VideoFilename, any(F::VideoFilename, &["solo.srt"]), &[]),
        (
            F::VideoFilesize,
            c(F::VideoFilesize).with_number(Ge, 8192.0),
            &["Uniform"],
        ),
        (
            F::VideoFilesize,
            c(F::VideoFilesize).between(500.0, 600.0),
            &["Sierra"],
        ),
        (F::VideoFormat, any(F::VideoFormat, &["720p"]), &["Sierra"]),
        (
            F::VideoFormat,
            any(F::VideoFormat, &["2160p"]),
            &["Uniform"],
        ),
        (F::VideoFormat, any(F::VideoFormat, &["1080p"]), &["Sierra"]),
        (F::Watched, c(F::Watched), &["Uniform"]),
        (F::Year, c(F::Year).with_number(Ge, 2020.0), &["Uniform"]),
        (F::Year, c(F::Year).between(2010.0, 2015.0), &["Sierra"]),
        // ---- ratings -----------------------------------------------------------------------
        (
            F::Top250Rank,
            c(F::Top250Rank).with_number(Le, 10.0),
            &["Sierra"],
        ),
        (
            F::RatingImdb,
            c(F::RatingImdb).with_number(Ge, 8.0),
            &["Sierra"],
        ),
        (
            F::RatingImdb,
            c(F::RatingImdb).with_number(Lt, 8.0),
            &["Uniform"],
        ),
        (
            F::RatingTmdb,
            c(F::RatingTmdb).with_number(Ge, 8.0),
            &["Sierra"],
        ),
        (
            F::RatingTrakt,
            c(F::RatingTrakt).with_number(Ge, 8.0),
            &["Sierra"],
        ),
        (
            F::RatingTvdb,
            c(F::RatingTvdb).with_number(Ge, 7.0),
            &["Sierra"],
        ),
        (
            F::RatingMetacritic,
            c(F::RatingMetacritic).with_number(Ge, 80.0),
            &["Sierra"],
        ),
        (
            F::RatingRtCritics,
            c(F::RatingRtCritics).with_number(Ge, 90.0),
            &["Sierra"],
        ),
        (
            F::RatingRtAudience,
            c(F::RatingRtAudience).with_number(Lt, 90.0),
            &["Uniform"],
        ),
        (
            F::RatingLetterboxd,
            c(F::RatingLetterboxd).with_number(Ge, 4.0),
            &["Sierra"],
        ),
        (
            F::RatingMyAnimeList,
            c(F::RatingMyAnimeList).with_number(Ge, 8.0),
            &["Sierra"],
        ),
        (
            F::RatingRogerEbert,
            c(F::RatingRogerEbert).with_number(Ge, 3.0),
            &["Sierra"],
        ),
        (
            F::RatingUser,
            c(F::RatingUser).with_number(Eq, 6.0),
            &["Uniform"],
        ),
    ]
}

#[test]
fn every_show_filter_has_a_case_that_matches_the_fixture_and_its_negation_is_the_rest() {
    let conn = memory_db();
    show_library(&conn);
    let cases = show_cases();
    run(&conn, ItemKind::Show, &cases);
    assert_eq!(covered(&cases), catalogue_ids(ItemKind::Show));
}

fn set_cases() -> Vec<Case> {
    vec![
        // Solo Set holds one movie, so the listing hides it (two or more owned movies list a set).
        (F::DataSource, any(F::DataSource, &["/n"]), &[]),
        (F::DataSource, any(F::DataSource, &["/m"]), &["Duo Set"]),
        (F::SetMissingMovies, c(F::SetMissingMovies), &["Duo Set"]),
        (F::SetNewMovies, c(F::SetNewMovies), &["Duo Set"]),
        (F::SetMultipleMovies, c(F::SetMultipleMovies), &["Duo Set"]),
    ]
}

#[test]
fn every_movie_set_filter_has_a_case_that_matches_the_fixture_and_its_negation_is_the_rest() {
    let conn = memory_db();
    movie_library(&conn);
    let cases = set_cases();
    run(&conn, ItemKind::Set, &cases);
    assert_eq!(covered(&cases), catalogue_ids(ItemKind::Set));
}

// ---- the catalogue itself -------------------------------------------------------------------

#[test]
fn the_catalogue_has_63_movie_69_show_and_4_set_filters_plus_the_rating_filters() {
    let base = |kind| {
        catalogue(kind)
            .unwrap()
            .iter()
            .filter(|def| def.group != FilterGroup::Ratings && def.id != FilterId::EditionLost)
            .count()
    };
    // The full catalogue has 64 and 71; the tag filters (Tag, Episode tag) are gone.
    assert_eq!(base(ItemKind::Movie), 63);
    assert_eq!(base(ItemKind::Show), 69);
    assert_eq!(base(ItemKind::Set), 4);
    // Movies and shows add the rating filters (per source and the Top 250 rank); sets add none.
    // Movies also have CineVault's own "Edition only in the file name".
    assert_eq!(catalogue(ItemKind::Movie).unwrap().len(), 63 + 12 + 1);
    assert_eq!(catalogue(ItemKind::Show).unwrap().len(), 69 + 12);
    assert_eq!(catalogue(ItemKind::Set).unwrap().len(), 4);
    for kind in [ItemKind::Movie, ItemKind::Show, ItemKind::Set] {
        let ids = catalogue_ids(kind);
        assert_eq!(
            ids.len(),
            catalogue(kind).unwrap().len(),
            "{kind:?} has no duplicate ids"
        );
    }
    assert!(catalogue(ItemKind::Episode).is_err());
    assert!(catalogue(ItemKind::Season).is_err());
}

#[test]
fn filter_labels_spell_uncategorised_as_the_show_page_does() {
    assert_eq!(
        FilterId::UncategorizedEpisodes.def().label,
        "Uncategorised episodes"
    );
    for id in FilterId::ALL {
        let label = id.def().label;
        assert!(!label.contains("ategorized"), "{label}");
    }
}

#[test]
fn every_id_belongs_to_a_catalogue_and_has_sql_there() {
    let in_some: BTreeSet<FilterId> = [ItemKind::Movie, ItemKind::Show, ItemKind::Set]
        .into_iter()
        .flat_map(catalogue_ids)
        .collect();
    assert_eq!(in_some.len(), FilterId::ALL.len());
    for kind in [ItemKind::Movie, ItemKind::Show, ItemKind::Set] {
        let scope = ids::Scope::of(kind).unwrap();
        for def in catalogue(kind).unwrap() {
            assert!(spec(scope, def.id).is_some(), "{:?} for {kind:?}", def.id);
        }
    }
}

#[test]
fn the_kind_of_each_filter_matches_the_sql_it_compiles_to() {
    for kind in [ItemKind::Movie, ItemKind::Show, ItemKind::Set] {
        let scope = ids::Scope::of(kind).unwrap();
        for def in catalogue(kind).unwrap() {
            let matches = matches!(
                (spec(scope, def.id).unwrap(), def.kind),
                (Spec::Flag(_), FilterKind::Flag)
                    | (Spec::Multi(_), FilterKind::Multi)
                    | (Spec::Text(_), FilterKind::Text)
                    | (Spec::Number(_), FilterKind::Number)
                    | (Spec::Date(_), FilterKind::Date)
            );
            assert!(matches, "{:?} for {kind:?}", def.id);
        }
    }
}

#[test]
fn a_filter_of_another_library_is_refused() {
    let conn = memory_db();
    movie_library(&conn);
    let filter = Filter::Facet(c(F::Status).with_values(["ended"]));
    let err = list_items(
        &conn,
        &ItemQuery::new(ItemKind::Movie).filter(filter),
        None,
        10,
    )
    .unwrap_err();
    assert!(matches!(err, DbError::InvalidFilter(_)), "{err}");

    let filter = Filter::Facet(c(F::Watched));
    let err = count_items(&conn, &ItemQuery::new(ItemKind::Episode).filter(filter)).unwrap_err();
    assert!(matches!(err, DbError::InvalidFilter(_)), "{err}");
}

// ---- operators, modes and combination ------------------------------------------------------

#[test]
fn all_six_numeric_operators_compare_against_the_runtime() {
    let conn = memory_db();
    movie_library(&conn);
    let ask = |condition: FacetCondition| titles(&conn, ItemKind::Movie, Filter::Facet(condition));
    let runtime = |op, n| c(F::Runtime).with_number(op, n);

    assert_eq!(ask(runtime(Lt, 100.0)), ["Delta"]);
    assert_eq!(ask(runtime(Le, 100.0)), ["Bravo", "Delta"]);
    assert_eq!(ask(runtime(Eq, 100.0)), ["Bravo"]);
    assert_eq!(ask(runtime(Ge, 100.0)), ["Alpha", "Bravo"]);
    assert_eq!(ask(runtime(Gt, 100.0)), ["Alpha"]);
    assert_eq!(ask(c(F::Runtime).between(95.0, 100.0)), ["Bravo", "Delta"]);
    // Bounds given the wrong way round still mean the range between them.
    assert_eq!(ask(c(F::Runtime).between(100.0, 95.0)), ["Bravo", "Delta"]);
}

#[test]
fn date_operators_compare_dates_as_text() {
    let conn = memory_db();
    movie_library(&conn);
    let ask = |condition: FacetCondition| titles(&conn, ItemKind::Movie, Filter::Facet(condition));
    let released = |op, d: &str| c(F::ReleaseDate).with_date(op, d);

    assert_eq!(ask(released(Lt, "2005-03-03")), ["Alpha"]);
    assert_eq!(ask(released(Le, "2005-03-03")), ["Alpha", "Bravo"]);
    assert_eq!(ask(released(Eq, "2005-03-03")), ["Bravo"]);
    assert_eq!(ask(released(Ge, "2005-03-03")), ["Bravo", "Delta"]);
    assert_eq!(ask(released(Gt, "2005-03-03")), ["Delta"]);
}

#[test]
fn any_matches_one_value_and_all_needs_every_value() {
    let conn = memory_db();
    movie_library(&conn);
    let ask = |condition: FacetCondition| titles(&conn, ItemKind::Movie, Filter::Facet(condition));

    assert_eq!(ask(any(F::Genre, &["Crime", "Comedy"])), ["Alpha", "Bravo"]);
    assert_eq!(
        ask(all(F::Genre, &["Crime", "Comedy"])),
        Vec::<String>::new()
    );
    assert_eq!(ask(all(F::Genre, &["Drama", "Comedy"])), ["Bravo"]);
    // Case and repeats do not matter.
    assert_eq!(
        ask(all(F::Genre, &["DRAMA", "drama", " Crime "])),
        ["Alpha"]
    );
    // Text terms: ANY is a union of contains, ALL needs every term somewhere.
    assert_eq!(ask(any(F::Cast, &["pacino", "hope"])), ["Alpha", "Bravo"]);
    assert_eq!(ask(all(F::Cast, &["michael", "mann"])), ["Alpha", "Bravo"]);
}

#[test]
fn text_terms_are_literal_and_wildcards_are_not_special() {
    let conn = memory_db();
    movie_library(&conn);
    let ask = |condition: FacetCondition| titles(&conn, ItemKind::Movie, Filter::Facet(condition));

    assert_eq!(ask(any(F::Path, &["%"])), Vec::<String>::new());
    assert_eq!(ask(any(F::Path, &["_"])), Vec::<String>::new());
    assert_eq!(ask(any(F::Path, &["ALPHA"])), ["Alpha"]);
    // A quote is data, not SQL.
    assert_eq!(
        ask(any(F::Note, &["'; DROP TABLE item; --"])),
        Vec::<String>::new()
    );
    assert_eq!(
        count_items(&conn, &ItemQuery::new(ItemKind::Movie)).unwrap(),
        5
    );
}

#[test]
fn a_condition_without_input_restricts_nothing_even_when_negated() {
    let conn = memory_db();
    movie_library(&conn);
    let everyone = titles(&conn, ItemKind::Movie, Filter::All);
    for condition in [
        c(F::Genre),
        c(F::Genre).with_values(["  ", ""]),
        c(F::Cast).negate(),
        c(F::Year),
        c(F::Year).between(1990.0, f64::NAN),
        c(F::Year).with_number(Ge, f64::INFINITY).negate(),
        c(F::ReleaseDate),
        c(F::ReleaseDate).with_date(Between, "2000-01-01"),
    ] {
        assert!(!condition.has_input(), "{condition:?}");
        assert_eq!(
            titles(&conn, ItemKind::Movie, Filter::Facet(condition.clone())),
            everyone,
            "{condition:?}"
        );
    }
    assert!(c(F::Watched).has_input());
}

#[test]
fn facets_combine_with_other_filters_and_keep_keyset_paging_exact() {
    let conn = memory_db();
    movie_library(&conn);
    let filter = Filter::and([
        Filter::Facet(c(F::Year).with_number(Ge, 1995.0)),
        Filter::not(Filter::Facet(any(F::Genre, &["comedy"]))),
        Filter::or([
            Filter::Facet(c(F::Watched)),
            Filter::Facet(any(F::Certification, &["R"])),
        ]),
    ]);
    assert_eq!(
        titles(&conn, ItemKind::Movie, filter.clone()),
        ["Alpha", "Charlie", "Delta"]
    );

    // Page through the filtered, sorted list two rows at a time.
    let query = ItemQuery::new(ItemKind::Movie)
        .filter(filter)
        .sort_by(SortKey::desc(SortField::Year));
    let mut seen = Vec::new();
    let mut cursor: Option<Cursor> = None;
    loop {
        let page = list_items(&conn, &query, cursor.as_ref(), 2).unwrap();
        seen.extend(page.items.iter().map(|i| i.title.clone()));
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    // Equal years fall back to the id, descending like the last sort column.
    assert_eq!(seen, ["Delta", "Charlie", "Alpha"]);
}

#[test]
fn a_facet_filter_survives_a_json_round_trip() {
    let filter = Filter::and([
        Filter::Facet(
            c(F::Genre)
                .with_values(["Drama"])
                .with_mode(MatchMode::All)
                .negate(),
        ),
        Filter::Facet(c(F::Year).between(1990.0, 1999.0)),
        Filter::Facet(c(F::DateAdded).with_date(Ge, "2024-01-01")),
    ]);
    let json = serde_json::to_string(&filter).unwrap();
    assert_eq!(serde_json::from_str::<Filter>(&json).unwrap(), filter);

    // Missing optional parts take their defaults.
    let minimal: FacetCondition = serde_json::from_str(r#"{"id":"watched"}"#).unwrap();
    assert_eq!(minimal, c(F::Watched));
    assert!(serde_json::from_str::<FacetCondition>(r#"{"id":"no_such_filter"}"#).is_err());
}

// ---- live counts --------------------------------------------------------------------------

#[test]
fn facet_values_count_items_per_value_and_follow_the_other_filters() {
    let conn = memory_db();
    movie_library(&conn);
    let counts = |id, base: &Filter| -> Vec<(String, u64)> {
        facet_values(&conn, ItemKind::Movie, id, base)
            .unwrap()
            .into_iter()
            .map(|v| (v.value, v.count))
            .collect()
    };

    assert_eq!(
        counts(F::HdrFormat, &Filter::All),
        [
            ("HDR10".to_owned(), 2),
            ("Dolby Vision".to_owned(), 1),
            ("SDR".to_owned(), 1)
        ]
    );
    assert_eq!(
        counts(F::VideoFormat, &Filter::All),
        [
            ("1080p".to_owned(), 1),
            ("2160p".to_owned(), 1),
            ("720p".to_owned(), 1)
        ]
    );
    // With another chip active the counts shrink to the items that still match.
    let watched = Filter::Facet(c(F::Watched));
    assert_eq!(
        counts(F::HdrFormat, &watched),
        [
            ("Dolby Vision".to_owned(), 1),
            ("HDR10".to_owned(), 1),
            ("SDR".to_owned(), 1)
        ]
    );
    // Values that differ only in case are one value.
    let genres = counts(F::Genre, &Filter::All);
    assert_eq!(genres.iter().map(|(_, n)| *n).sum::<u64>(), 5);
    assert_eq!(genres[0].1, 3, "{genres:?}");

    // Only list filters have values to count.
    let err = facet_values(&conn, ItemKind::Movie, F::Year, &Filter::All).unwrap_err();
    assert!(matches!(err, DbError::InvalidFilter(_)));
    let err = facet_values(&conn, ItemKind::Movie, F::Status, &Filter::All).unwrap_err();
    assert!(matches!(err, DbError::InvalidFilter(_)));
}

#[test]
fn a_video_without_media_info_is_neither_sdr_nor_hdr() {
    let conn = memory_db();
    for (title, width, height) in [("Known", 1920, 1080), ("Pending", 0, 0)] {
        let dir = format!("/m/{title}");
        let mut item = Item::movie(title);
        item.core.data_source = "/m".into();
        item.core.path.clone_from(&dir);
        item.core.media_files = vec![video(&Video {
            dir: &dir,
            name: "video.mkv",
            size_mib: 1,
            codec: "",
            container: "",
            width,
            height,
            aspect: 0.0,
            bitrate: 0,
            bit_depth: 0,
            fps: 0.0,
            minutes: 0,
            hdr: &[],
        })];
        insert_item(&conn, &item).unwrap();
    }
    let values = facet_values(&conn, ItemKind::Movie, F::HdrFormat, &Filter::All).unwrap();
    assert_eq!(
        values,
        [FacetValue {
            value: "SDR".into(),
            count: 1
        }]
    );
    let sdr = Filter::Facet(any(F::HdrFormat, &["SDR"]));
    let query = ItemQuery::new(ItemKind::Movie).filter(sdr);
    assert_eq!(crate::repo::count_items(&conn, &query).unwrap(), 1);
}

#[test]
fn show_facet_values_count_shows_not_episodes() {
    let conn = memory_db();
    show_library(&conn);
    let values = facet_values(&conn, ItemKind::Show, F::VideoCodec, &Filter::All).unwrap();
    assert_eq!(
        values,
        [
            FacetValue {
                value: "HEVC".into(),
                count: 2
            },
            FacetValue {
                value: "AVC".into(),
                count: 1
            },
        ]
    );
}

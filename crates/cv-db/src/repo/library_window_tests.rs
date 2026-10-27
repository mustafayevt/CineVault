//! Random-access windows over a listing, and the per-card lookups of the poster grid.

use cv_domain::{Item, ItemId, ItemKind, MediaFile, MediaFileType, id_key};
use rusqlite::Connection;

use super::graphics::{
    card_file_names, poster_files_for, set_member_folders, unmatched_among, video_facts_for,
};
use super::*;

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn file(kind: MediaFileType, name: &str) -> MediaFile {
    MediaFile {
        kind,
        directory: "/media/Movies/Heat".into(),
        filename: name.into(),
        ..MediaFile::default()
    }
}

#[test]
fn a_window_equals_the_same_rows_read_page_by_page_for_any_sort() {
    let conn = memory_db();
    for n in 0..57_u16 {
        let mut m = Item::movie(format!("Film {:02}", n % 23));
        m.core.year = 1990 + (n % 7);
        insert_item(&conn, &m).unwrap();
    }
    let queries = [
        ItemQuery::new(ItemKind::Movie),
        ItemQuery::new(ItemKind::Movie)
            .sort_by(SortKey::desc(SortField::Year))
            .sort_by(SortKey::asc(SortField::Title)),
    ];
    for query in &queries {
        let mut by_cursor = Vec::new();
        let mut cursor = None;
        loop {
            let page = list_items(&conn, query, cursor.as_ref(), 10).unwrap();
            by_cursor.extend(page.items.iter().map(|i| i.id));
            match page.next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        assert_eq!(by_cursor.len(), 57);
        for (offset, limit) in [(0, 10), (10, 10), (45, 30), (56, 5), (57, 5), (200, 5)] {
            let window = list_items_at(&conn, query, offset, limit).unwrap();
            let ids: Vec<ItemId> = window.iter().map(|i| i.id).collect();
            let end = (offset + limit).min(by_cursor.len());
            let expected = by_cursor.get(offset..end).unwrap_or(&[]);
            assert_eq!(ids, expected, "offset {offset} limit {limit}");
        }
    }
}

#[test]
fn posters_are_chosen_by_kind_then_position_and_only_for_the_asked_items() {
    let conn = memory_db();
    let mut with_both = Item::movie("Heat");
    with_both.core.media_files = vec![
        file(MediaFileType::Keyart, "keyart.jpg"),
        file(MediaFileType::Fanart, "fanart.jpg"),
        file(MediaFileType::Poster, "poster.jpg"),
    ];
    let mut only_keyart = Item::movie("Ronin");
    only_keyart.core.media_files = vec![file(MediaFileType::Keyart, "keyart.jpg")];
    let mut none = Item::movie("Thief");
    none.core.media_files = vec![file(MediaFileType::Fanart, "fanart.jpg")];
    let a = insert_item(&conn, &with_both).unwrap();
    let b = insert_item(&conn, &only_keyart).unwrap();
    let c = insert_item(&conn, &none).unwrap();

    let found = poster_files_for(&conn, &[a, b, c]).unwrap();
    assert_eq!(found[&a].filename, "poster.jpg");
    assert_eq!(found[&b].filename, "keyart.jpg");
    assert!(!found.contains_key(&c), "fan art is not a poster");
    assert!(poster_files_for(&conn, &[]).unwrap().is_empty());
    assert_eq!(poster_files_for(&conn, &[b]).unwrap().len(), 1);
}

#[test]
fn a_collections_member_folders_come_earliest_release_first() {
    // The `movieset-poster.jpg` next to the movies is where a collection without a
    // poster of its own finds one.
    let conn = memory_db();
    let set = insert_item(&conn, &Item::set("Alien Collection")).unwrap();
    let empty = insert_item(&conn, &Item::set("Empty Collection")).unwrap();
    let video = |dir: &str| MediaFile {
        directory: dir.into(),
        ..file(MediaFileType::Video, "film.mkv")
    };
    let mut aliens = Item::movie("Aliens");
    aliens.core.year = 1986;
    aliens.parent_id = Some(set);
    aliens.core.media_files = vec![video("/media/Movies/Aliens (1986)")];
    let mut alien = Item::movie("Alien");
    alien.core.year = 1979;
    alien.parent_id = Some(set);
    alien.core.media_files = vec![video("/media/Movies/Alien (1979)")];
    let loose = Item::movie("Thief");
    insert_item(&conn, &aliens).unwrap();
    let alien = insert_item(&conn, &alien).unwrap();
    let loose = insert_item(&conn, &loose).unwrap();

    let folders = set_member_folders(&conn, &[set, empty, alien, loose]).unwrap();
    assert_eq!(
        folders[&set],
        ["/media/Movies/Alien (1979)", "/media/Movies/Aliens (1986)"]
    );
    assert!(!folders.contains_key(&empty), "no movie, no folder");
    assert!(!folders.contains_key(&alien), "a movie is not a collection");
    assert!(set_member_folders(&conn, &[]).unwrap().is_empty());
}

#[test]
fn an_item_without_any_provider_id_is_unmatched() {
    let conn = memory_db();
    let mut matched = Item::movie("Heat");
    matched.core.ids.set(id_key::TMDB, "949");
    let loose = Item::movie("heat.1995.1080p");
    let a = insert_item(&conn, &matched).unwrap();
    let b = insert_item(&conn, &loose).unwrap();

    let unmatched = unmatched_among(&conn, &[a, b]).unwrap();
    assert!(!unmatched.contains(&a));
    assert!(unmatched.contains(&b));
    assert!(unmatched_among(&conn, &[]).unwrap().is_empty());
}

#[test]
fn video_facts_come_from_the_first_video_file() {
    let conn = memory_db();
    let mut item = Item::movie("Dune");
    item.core.media_files = vec![
        file(MediaFileType::Poster, "poster.jpg"),
        MediaFile {
            width: 3840,
            height: 2160,
            hdr_formats: vec!["Dolby Vision".into()],
            ..file(MediaFileType::Video, "dune.mkv")
        },
        MediaFile {
            height: 1080,
            ..file(MediaFileType::Video, "dune-1080.mkv")
        },
    ];
    let a = insert_item(&conn, &item).unwrap();
    let b = insert_item(&conn, &Item::movie("Bare")).unwrap();

    let facts = video_facts_for(&conn, &[a, b]).unwrap();

    assert_eq!(facts[&a].height, 2160);
    assert_eq!(facts[&a].width, 3840);
    assert_eq!(facts[&a].hdr_formats, ["Dolby Vision"]);
    assert!(!facts.contains_key(&b));
}

#[test]
fn a_card_is_named_by_its_folder_or_by_its_video_in_a_shared_folder() {
    let conn = memory_db();
    let mut own = Item::movie("Heat");
    own.core.path = r"C:\\Movies\\Heat (1995)\\".into();
    own.core.media_files = vec![file(MediaFileType::Video, "heat.mkv")];
    // Two loose episodes that became movies in one `Season 1` folder.
    let shared = |name: &str| {
        let mut item = Item::movie(name);
        item.core.path = "/media/Mixed/Chernobyl/Season 1".into();
        if let ItemDetail::Movie(movie) = &mut item.detail {
            movie.multi_movie_dir = true;
        }
        item.core.media_files = vec![
            file(MediaFileType::Poster, "poster.jpg"),
            file(MediaFileType::Video, &format!("{name}.mkv")),
        ];
        item
    };
    let a = insert_item(&conn, &own).unwrap();
    let b = insert_item(&conn, &shared("Chernobyl S01E01")).unwrap();
    let c = insert_item(&conn, &shared("Chernobyl S01E02")).unwrap();
    let mut bare = Item::movie("Bare");
    bare.core.path = "/m/Bare".into();
    let d = insert_item(&conn, &bare).unwrap();
    let mut nameless = Item::movie("Nameless");
    nameless.core.path = String::new();
    let e = insert_item(&conn, &nameless).unwrap();

    let names = card_file_names(&conn, &[a, b, c, d, e]).unwrap();

    assert_eq!(names[&a], "Heat (1995)");
    assert_eq!(names[&b], "Chernobyl S01E01.mkv");
    assert_eq!(names[&c], "Chernobyl S01E02.mkv");
    assert_eq!(names[&d], "Bare");
    assert!(!names.contains_key(&e));
    assert!(card_file_names(&conn, &[]).unwrap().is_empty());
}

// ---- the list and table views -----------------------------------------------------------------

use super::table_facts::table_facts_for;
use cv_domain::{AudioStream, ItemDetail, Rating as Score, RatingPreference, SubtitleStream};

fn movie_with_files(title: &str) -> Item {
    let mut item = Item::movie(title);
    item.core.year = 2021;
    item.core.plot = "A plot.".into();
    item.core.ids.set(id_key::TMDB, "1");
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.runtime_minutes = 155;
    }
    item.core.ratings.insert(Score::new("imdb", 8.0, 1000, 10));
    item.core.ratings.insert(Score::new("tmdb", 7.0, 500, 10));
    item.core
        .ratings
        .insert(Score::new("tomatometerallcritics", 90.0, 0, 100));
    item.core.ratings.insert(Score::new("user", 9.5, 0, 10));
    item.core.media_files = vec![
        file(MediaFileType::Poster, "poster.jpg"),
        file(MediaFileType::Clearlogo, "clearlogo.png"),
        file(MediaFileType::Nfo, "movie.nfo"),
        file(MediaFileType::Subtitle, "movie.en.srt"),
        file(MediaFileType::Subtitle, "movie.fr.srt"),
        MediaFile {
            size_bytes: 4_000,
            height: 2160,
            video_codec: "HEVC".into(),
            video_bitrate_kbps: 40_000,
            aspect_ratio: 2.39,
            hdr_formats: vec!["HDR10".into()],
            audio_streams: vec![
                AudioStream {
                    codec: "TrueHD Atmos".into(),
                    channels: 8,
                    ..AudioStream::default()
                },
                AudioStream::default(),
            ],
            subtitles: vec![SubtitleStream::default()],
            ..file(MediaFileType::Video, "movie.mkv")
        },
        MediaFile {
            size_bytes: 1_000,
            ..file(MediaFileType::Video, "movie-b.mkv")
        },
    ];
    item
}

#[test]
fn table_facts_collect_ratings_video_streams_and_artwork() {
    let conn = memory_db();
    let rich = insert_item(&conn, &movie_with_files("Dune")).unwrap();
    let bare = insert_item(&conn, &Item::movie("Bare")).unwrap();

    let facts = table_facts_for(
        &conn,
        &[rich, bare, ItemId(9999)],
        &RatingPreference::default(),
    )
    .unwrap();

    let rich = &facts[&rich];
    // The default preference is TMDB, which this movie has.
    assert_eq!(rich.rating_source.as_deref(), Some("tmdb"));
    assert_eq!(rich.rating, Some(7.0));
    assert_eq!(rich.votes, 500);
    assert_eq!(rich.imdb_rating, Some(8.0));
    assert_eq!(rich.rotten_tomatoes_rating, Some(9.0));
    assert_eq!(rich.user_rating, Some(9.5));
    let video = rich.video.as_ref().unwrap();
    assert_eq!((video.height, video.codec.as_str()), (2160, "HEVC"));
    assert_eq!(video.size_bytes, 4_000);
    assert_eq!(video.filename, "movie.mkv");
    assert_eq!(rich.total_size_bytes, 5_000);
    assert_eq!(rich.audio_streams, 2);
    assert_eq!(rich.audio_tracks.len(), 2);
    assert_eq!(rich.audio_tracks[0].codec, "TrueHD Atmos");
    assert_eq!(
        (rich.audio_codec.as_str(), rich.audio_channels),
        ("TrueHD Atmos", 8)
    );
    assert_eq!(rich.subtitle_streams, 1);
    assert!(rich.has_poster && !rich.has_fanart);
    assert!(rich.has_logo && rich.has_nfo);
    assert_eq!(rich.subtitle_files, 2);
    assert!(rich.metadata_complete);

    let bare = &facts[&bare];
    assert!(bare.video.is_none() && bare.rating.is_none() && !bare.metadata_complete);
    assert!(!bare.has_poster && !bare.has_logo && !bare.has_nfo && bare.subtitle_files == 0);
    assert_eq!(bare.total_size_bytes, 0);
    assert!(!facts.contains_key(&ItemId(9999)));
    assert!(
        table_facts_for(&conn, &[], &RatingPreference::default())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_set_and_its_members_report_each_other() {
    let conn = memory_db();
    let set = insert_item(&conn, &Item::set("Dune Collection")).unwrap();
    let mut member = movie_with_files("Dune");
    member.parent_id = Some(set);
    let member = insert_item(&conn, &member).unwrap();

    let facts = table_facts_for(&conn, &[set, member], &RatingPreference::default()).unwrap();

    assert_eq!(facts[&member].movie_set.as_deref(), Some("Dune Collection"));
    assert_eq!(facts[&set].movies_in_set, 1);
    // A set has no video of its own: it shows its first movie's.
    assert_eq!(facts[&set].video.as_ref().map(|v| v.height), Some(2160));
    assert_eq!(facts[&set].total_size_bytes, 5_000);
}

#[test]
fn a_collection_is_listed_and_counted_only_with_two_or_more_owned_movies() {
    let conn = memory_db();
    let set = insert_item(&conn, &Item::set("Alien Collection")).unwrap();
    let listed = |conn: &Connection| {
        let query = ItemQuery::new(ItemKind::Set);
        (
            count_items(conn, &query).unwrap(),
            list_items(conn, &query, None, 50).unwrap().items.len(),
        )
    };
    assert_eq!(listed(&conn), (0, 0), "an empty collection is hidden");

    let mut alien = Item::movie("Alien");
    alien.parent_id = Some(set);
    insert_item(&conn, &alien).unwrap();
    assert_eq!(listed(&conn), (0, 0), "one owned movie is not enough");
    assert!(get_item(&conn, set).unwrap().is_some(), "but it is kept");

    let mut aliens = Item::movie("Aliens");
    aliens.parent_id = Some(set);
    insert_item(&conn, &aliens).unwrap();
    assert_eq!(listed(&conn), (1, 1), "the second movie brings it back");
}

#[test]
fn a_show_counts_the_seasons_it_has_episodes_in_without_the_specials() {
    use cv_domain::{EpisodeGroup, EpisodeNumber, EpisodeOrdering};

    let conn = memory_db();
    let office = insert_item(&conn, &Item::show("The Office")).unwrap();
    // Season records for the specials, season 1 and an empty season 2.
    for number in [0, 1, 2] {
        insert_item(&conn, &Item::season(office, number)).unwrap();
    }
    let add = |title: &str, season: u32, number: u32| {
        let mut episode = Item::episode(office, title);
        if let ItemDetail::Episode(e) = &mut episode.detail {
            e.numbers.set(EpisodeNumber::aired(season, number));
        }
        insert_item(&conn, &episode).unwrap()
    };
    add("Special", 0, 1);
    add("Pilot", 1, 1);
    add("Diversity Day", 1, 2);
    let dvd = add("Health Care", 1, 3);
    insert_item(&conn, &Item::episode(office, "No number")).unwrap();

    let seasons = |conn: &Connection| {
        table_facts_for(conn, &[office], &RatingPreference::default()).unwrap()[&office].seasons
    };
    assert_eq!(
        seasons(&conn),
        1,
        "as the show page: one season, the specials apart"
    );

    // In the DVD order the third episode is in season 2: the count follows the show's choice.
    let mut episode = get_item(&conn, dvd).unwrap().unwrap();
    if let ItemDetail::Episode(e) = &mut episode.detail {
        e.numbers.set(EpisodeNumber::new(EpisodeGroup::dvd(), 2, 1));
    }
    update_item(&conn, &episode).unwrap();
    let mut show = get_item(&conn, office).unwrap().unwrap();
    if let ItemDetail::Show(s) = &mut show.detail {
        s.episode_group = EpisodeGroup::new(EpisodeOrdering::Dvd, "");
    }
    update_item(&conn, &show).unwrap();
    assert_eq!(seasons(&conn), 2);

    // The sort reads the same number.
    let query = ItemQuery::new(ItemKind::Show).sort_by(SortKey::asc(SortField::Seasons));
    assert_eq!(list_items(&conn, &query, None, 10).unwrap().items.len(), 1);
}

/// Every new sort field must produce a valid, stable order whether read by window or by cursor.
#[test]
fn every_table_sort_field_pages_the_same_as_it_windows() {
    use SortField::*;
    let conn = memory_db();
    let set = insert_item(&conn, &Item::set("Set")).unwrap();
    for n in 0..14_u32 {
        let mut item = movie_with_files(&format!("Film {}", n % 5));
        item.core
            .ratings
            .insert(Score::new("imdb", f32::from((n % 4) as u8), n, 10));
        item.core.media_files[1].size_bytes = u64::from(n % 3) * 100;
        if n % 3 == 0 {
            item.parent_id = Some(set);
        }
        insert_item(&conn, &item).unwrap();
    }
    let fields = [
        OriginalTitle,
        Filename,
        Top250,
        Certification,
        Edition,
        MediaSource,
        Watched,
        IsNew,
        MovieSet,
        Rating,
        Votes,
        UserRating,
        ImdbRating,
        TmdbRating,
        TraktRating,
        RottenTomatoesRating,
        RottenTomatoesAudienceRating,
        MetacriticRating,
        LetterboxdRating,
        MyAnimeListRating,
        RogerEbertRating,
        TvdbRating,
        VideoHeight,
        AspectRatio,
        AspectRatio2,
        VideoCodec,
        VideoBitrate,
        Hdr,
        FileSize,
        TotalFileSize,
        Quality,
        Folder,
        Year,
        Runtime,
        FileModified,
        AudioStreams,
        SubtitleStreams,
        Trailers,
        Images,
        Metadata,
        Seasons,
        Episodes,
        MissingEpisodes,
        MoviesInSet,
        MissingMovies,
        AiredStatus,
    ];
    for field in fields {
        for direction in [Direction::Asc, Direction::Desc] {
            let query = ItemQuery::new(ItemKind::Movie)
                .sort_by(SortKey { field, direction })
                .sort_by(SortKey::asc(SortField::Title));
            let mut by_cursor = Vec::new();
            let mut cursor = None;
            loop {
                let page = list_items(&conn, &query, cursor.as_ref(), 4).unwrap();
                by_cursor.extend(page.items.iter().map(|i| i.id));
                match page.next {
                    Some(c) => cursor = Some(c),
                    None => break,
                }
            }
            assert_eq!(by_cursor.len(), 14, "{field:?} {direction:?}");
            let window: Vec<ItemId> = list_items_at(&conn, &query, 0, 100)
                .unwrap()
                .iter()
                .map(|i| i.id)
                .collect();
            assert_eq!(window, by_cursor, "{field:?} {direction:?}");
        }
    }
}

#[test]
fn sorting_by_rating_then_title_orders_ties_by_title() {
    let conn = memory_db();
    for (title, value) in [
        ("Beta", 8.0_f32),
        ("Alpha", 8.0),
        ("Gamma", 9.0),
        ("Delta", 7.0),
    ] {
        let mut item = Item::movie(title);
        item.core.ratings.insert(Score::new("imdb", value, 10, 10));
        insert_item(&conn, &item).unwrap();
    }
    let query = ItemQuery::new(ItemKind::Movie)
        .sort_by(SortKey::desc(SortField::Rating))
        .sort_by(SortKey::asc(SortField::Title));
    let titles: Vec<String> = list_items_at(&conn, &query, 0, 10)
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect();
    assert_eq!(titles, ["Gamma", "Alpha", "Beta", "Delta"]);
}

/// A movie whose title folder is `path` with a main video of the given height and HDR formats.
fn movie_at(title: &str, path: &str, height: u32, hdr: &[&str], bytes: u64) -> Item {
    let mut item = Item::movie(title);
    item.core.path = path.into();
    if height > 0 || bytes > 0 {
        item.core.media_files = vec![MediaFile {
            size_bytes: bytes,
            height,
            hdr_formats: hdr.iter().map(|f| (*f).to_owned()).collect(),
            ..file(MediaFileType::Video, "movie.mkv")
        }];
    }
    item
}

fn titles_by(conn: &Connection, key: SortKey) -> Vec<String> {
    let query = ItemQuery::new(ItemKind::Movie)
        .sort_by(key)
        .sort_by(SortKey::asc(SortField::Title));
    list_items_at(conn, &query, 0, 100)
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect()
}

#[test]
fn quality_sorts_by_resolution_rank_then_hdr_with_unknown_last() {
    let conn = memory_db();
    for item in [
        // A cropped 1080p release is still 1080p; HDR ranks above the same resolution without it.
        movie_at("Scope 1080", r"D:\M\a", 800, &[], 1),
        movie_at("Full HD", r"D:\M\b", 1080, &[], 1),
        movie_at("4K", r"D:\M\c", 2160, &[], 1),
        movie_at("4K HDR", r"D:\M\d", 2160, &["HDR10"], 1),
        movie_at("720", r"D:\M\e", 720, &[], 1),
        movie_at("No file", r"D:\M\f", 0, &[], 0),
    ] {
        insert_item(&conn, &item).unwrap();
    }
    assert_eq!(
        titles_by(&conn, SortKey::desc(SortField::Quality)),
        ["4K HDR", "4K", "Full HD", "Scope 1080", "720", "No file"]
    );
    assert_eq!(
        titles_by(&conn, SortKey::asc(SortField::Quality)),
        ["720", "Full HD", "Scope 1080", "4K", "4K HDR", "No file"]
    );
}

#[test]
fn folder_sorts_by_the_folder_a_title_sits_in_and_size_by_total_bytes_with_none_last() {
    let conn = memory_db();
    for item in [
        movie_at("In movies", r"D:\Movies\Alpha (2000)", 1080, &[], 300),
        movie_at("In movies 4K", r"D:\Movies 4K\Beta (2001)", 2160, &[], 100),
        movie_at("On other drive", r"C:\Films/Gamma (2002)", 720, &[], 200),
        movie_at("Empty", r"D:\Movies\Delta (2003)", 0, &[], 0),
    ] {
        insert_item(&conn, &item).unwrap();
    }
    // r"C:\Films" < r"D:\Movies" < r"D:\Movies 4K", as the column shows them (the path alone would
    // put r"D:\Movies 4K\..." before r"D:\Movies\...").
    assert_eq!(
        titles_by(&conn, SortKey::asc(SortField::Folder)),
        ["On other drive", "Empty", "In movies", "In movies 4K"]
    );
    assert_eq!(
        titles_by(&conn, SortKey::desc(SortField::TotalFileSize)),
        ["In movies", "On other drive", "In movies 4K", "Empty"]
    );
    assert_eq!(
        titles_by(&conn, SortKey::asc(SortField::TotalFileSize)),
        ["In movies 4K", "On other drive", "In movies", "Empty"]
    );
}

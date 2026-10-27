use cv_domain::{
    AiredStatus, AudioStream, CalendarDate, Certification, Credit, CreditKind, Edition,
    EpisodeGroup, EpisodeNumber, Genre, Item, ItemDetail, ItemId, ItemKind, MediaFile,
    MediaFileType, MediaSource, MissingEpisode, MissingMovie, Rating, RenameHistory, RenameStep,
    SeasonText, StreamFlag, SubtitleStream, Trailer, Video3dFormat, id_key, rating_key,
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

use super::*;
use crate::{Database, DbError};

fn temp_db() -> (TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    (dir, db)
}

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn fully_populated_movie() -> Item {
    let mut item = Item::movie("Blade Runner 2049");
    item.core.locked = true;
    item.core.data_source = "D:\\Movies".into();
    item.core.original_title = "Blade Runner 2049 (orig)".into();
    item.core.original_language = "en".into();
    item.core.year = 2017;
    item.core.plot = "A young blade runner discovers a long-buried secret.".into();
    item.core.outline = "Short outline".into();
    item.core.path = "D:\\Movies\\Blade Runner 2049 (2017)".into();
    item.core.date_added_ms = 1_700_000_000_000;
    item.core.production_companies = vec!["Alcon".into(), "Columbia".into()];
    item.core.note = "Rewatch in 4K".into();
    item.core.original_filename = "br2049.2017.mkv".into();
    item.core.last_scraper_id = "tmdb".into();
    item.core.last_scrape_language = "en".into();
    item.core.rename_history = Some(RenameHistory {
        old_path: "D:\\Movies\\br2049".into(),
        new_path: "D:\\Movies\\Blade Runner 2049 (2017)".into(),
        files: vec![RenameStep {
            old_filename: "br2049.2017.mkv".into(),
            new_filename: "Blade Runner 2049 (2017).mkv".into(),
        }],
    });
    item.core.ids.set(id_key::IMDB, "tt1856101");
    item.core.ids.set(id_key::TMDB, "335984");
    item.core.ids.set("kinopoisk", "12345");
    item.core
        .ratings
        .insert(Rating::new("imdb", 8.0, 600_000, 10).fetched_at(1_700_000_000_000));
    item.core
        .ratings
        .insert(Rating::new("metacritic", 81.0, 50, 100).fetched_at(1_700_000_100_000));
    item.core
        .ratings
        .insert(Rating::new(rating_key::USER, 9.5, 0, 10).fetched_at(1_700_000_200_000));
    item.core.tags.add("Cyberpunk");
    item.core.tags.add("favourite");
    item.core.artwork_urls.insert(
        MediaFileType::Poster,
        "https://example.test/poster.jpg".into(),
    );
    item.core.artwork_urls.insert(
        MediaFileType::Fanart,
        "https://example.test/fanart.jpg".into(),
    );
    item.core
        .credits
        .push(Credit::new(CreditKind::Director, "Denis Villeneuve", ""));
    let mut ryan = Credit::new(CreditKind::Actor, "Ryan Gosling", "K");
    ryan.person.ids.set("tmdb", "30614");
    ryan.person.thumb_url = "https://example.test/ryan.jpg".into();
    item.core.credits.push(ryan);
    item.core
        .credits
        .push(Credit::new(CreditKind::Actor, "Harrison Ford", "Deckard"));

    let mut video = MediaFile::new(
        MediaFileType::Video,
        "D:\\Movies\\Blade Runner 2049 (2017)",
        "Blade Runner 2049 (2017).mkv",
    );
    video.size_bytes = 40_000_000_000;
    video.modified_ms = 1_690_000_000_000;
    video.video_codec = "HEVC".into();
    video.codec_id = "V_MPEGH/ISO/HEVC".into();
    video.container_format = "Matroska".into();
    video.video_3d = Video3dFormat::HalfSideBySide;
    video.width = 3840;
    video.height = 2160;
    video.aspect_ratio = 2.39;
    video.aspect_ratio2 = 1.78;
    video.pixel_aspect_ratio = 1.0;
    video.video_bitrate_kbps = 60_000;
    video.overall_bitrate_kbps = 65_000;
    video.bit_depth = 10;
    video.frame_rate = 23.976;
    video.duration_secs = 9_840;
    video.stacking = 1;
    video.stacking_marker = "cd1".into();
    video.title = "Container title".into();
    video.is_iso = false;
    video.hdr_formats = vec!["Dolby Vision".into(), "HDR10".into()];
    video.checksums.insert("crc32".into(), "DEADBEEF".into());
    video.audio_streams.push(AudioStream {
        codec: "TrueHD Atmos".into(),
        codec_id: "A_TRUEHD".into(),
        language: "eng".into(),
        title: "Main".into(),
        flags: [StreamFlag::Default, StreamFlag::Original].into(),
        channels: 8,
        bitrate_kbps: 4_000,
        bit_depth: 24,
        sample_rate_hz: 48_000,
    });
    video.audio_streams.push(AudioStream {
        codec: "AC3".into(),
        language: "fra".into(),
        flags: [StreamFlag::Dub].into(),
        channels: 6,
        ..AudioStream::default()
    });
    video.subtitles.push(SubtitleStream {
        codec: "PGS".into(),
        language: "eng".into(),
        flags: [StreamFlag::Forced, StreamFlag::HearingImpaired].into(),
        ..SubtitleStream::default()
    });
    item.core.media_files.push(video);
    item.core.media_files.push(MediaFile::new(
        MediaFileType::Nfo,
        "D:\\Movies\\Blade Runner 2049 (2017)",
        "movie.nfo",
    ));

    let ItemDetail::Movie(m) = &mut item.detail else {
        unreachable!()
    };
    m.english_title = "Blade Runner 2049".into();
    m.sort_title = "Blade Runner 02".into();
    m.tagline = "There's still a page left.".into();
    m.runtime_minutes = 164;
    m.watched = true;
    m.play_count = 2;
    m.last_watched_ms = Some(1_710_000_000_000);
    m.spoken_languages = vec!["en".into(), "fi".into()];
    m.countries = vec!["US".into(), "GB".into()];
    m.release_date = CalendarDate::new(2017, 10, 4);
    m.top250 = 150;
    m.media_source = MediaSource::UhdBluray;
    m.certification = Certification("US_R".into());
    m.edition = Edition::Other("Ultimate Archive".into());
    m.stacked = true;
    m.genres = vec![
        Genre::ScienceFiction,
        Genre::Drama,
        Genre::Other("Neo Dystopia".into()),
    ];
    m.extra_thumbs = vec!["https://example.test/t1.jpg".into()];
    m.extra_fanarts = vec!["https://example.test/f1.jpg".into()];
    m.show_links = vec!["Black Mirror".into()];
    m.trailers = vec![Trailer {
        id: "yt1".into(),
        name: "Official Trailer".into(),
        url: "https://example.test/t".into(),
        quality: "1080p".into(),
        language: "en".into(),
        provider: "tmdb".into(),
        in_nfo: true,
        date: CalendarDate::new(2017, 6, 1),
        scraped_by: "tmdb".into(),
        length_secs: 0,
    }];
    item
}

fn with_id(mut item: Item, id: ItemId) -> Item {
    item.id = Some(id);
    item
}

// ---- CRUD ------------------------------------------------------------------------------------

#[test]
fn a_fully_populated_movie_round_trips_through_the_database() {
    let conn = memory_db();
    let movie = fully_populated_movie();

    let id = insert_item(&conn, &movie).unwrap();
    let back = get_item(&conn, id).unwrap().unwrap();

    assert_eq!(back, with_id(movie, id));
}

#[test]
fn the_original_language_belongs_to_a_show_and_its_episodes_read_it() {
    let conn = memory_db();
    let mut show = Item::show("Dark");
    show.core.original_language = "de".into();
    let show_id = insert_item(&conn, &show).unwrap();
    let mut episode = Item::episode(show_id, "Secrets");
    // An episode never owns the value: whatever it carries is not stored.
    episode.core.original_language = "xx".into();
    let episode_id = insert_item(&conn, &episode).unwrap();
    let season_id = insert_item(&conn, &Item::season(show_id, 1)).unwrap();

    let stored = |id: ItemId| -> String {
        conn.query_row(
            "SELECT original_language FROM item WHERE id = ?1",
            [id.0],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(stored(show_id), "de");
    assert_eq!(stored(episode_id), "");

    // Reading gives the episode and the season their show's value.
    for id in [episode_id, season_id] {
        assert_eq!(
            get_item(&conn, id).unwrap().unwrap().core.original_language,
            "de"
        );
        assert_eq!(items::original_language_of(&conn, id).unwrap(), "de");
    }
    assert_eq!(items::original_language_of(&conn, show_id).unwrap(), "de");
    assert_eq!(
        items::original_language_of(&conn, ItemId(9999)).unwrap(),
        ""
    );
}

#[test]
fn a_populated_movie_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let movie = fully_populated_movie();
    let id = {
        let db = Database::open(&path).unwrap();
        let movie = movie.clone();
        db.write(move |tx| insert_item(tx, &movie)).unwrap()
    };

    let db = Database::open(&path).unwrap();
    let back = db.read(|c| get_item(c, id)).unwrap().unwrap();

    assert_eq!(back, with_id(movie, id));
}

#[test]
fn shows_seasons_and_episodes_round_trip_with_every_numbering() {
    let conn = memory_db();
    let mut show = Item::show("Severance");
    {
        let ItemDetail::Show(s) = &mut show.detail else {
            unreachable!()
        };
        s.english_title = "Severance".into();
        s.sort_title = "Severance".into();
        s.runtime_minutes = 50;
        s.year_end = 0;
        s.first_aired = CalendarDate::new(2022, 2, 18);
        s.status = AiredStatus::Continuing;
        s.country = "US".into();
        s.certification = Certification("US_TVMA".into());
        s.episode_group = EpisodeGroup::new(cv_domain::EpisodeOrdering::Dvd, "Production");
        s.episode_groups = vec![
            EpisodeGroup::aired(),
            EpisodeGroup::new(cv_domain::EpisodeOrdering::Dvd, "Production"),
        ];
        s.genres = vec![Genre::Drama, Genre::Mystery];
        s.extra_fanart_urls = vec!["https://example.test/ef.jpg".into()];
        s.season_texts = vec![SeasonText {
            group: EpisodeGroup::aired(),
            season: 1,
            name: "Season One".into(),
            overview: "Overview".into(),
        }];
        s.missing_episodes = vec![MissingEpisode {
            title: "Missing".into(),
            numbers: Default::default(),
            first_aired: CalendarDate::new(2022, 3, 1),
            plot: "Not here".into(),
        }];
    }
    show.core.ids.set(id_key::TVDB, "371980");
    let show_id = insert_item(&conn, &show).unwrap();

    let season = Item::season(show_id, 1);
    let season_id = insert_item(&conn, &season).unwrap();

    let mut episode = Item::episode(show_id, "Good News About Hell");
    episode.parent_id = Some(season_id);
    if let ItemDetail::Episode(e) = &mut episode.detail {
        e.numbers.set(EpisodeNumber::aired(1, 2));
        e.numbers.set(EpisodeNumber::new(EpisodeGroup::dvd(), 1, 3));
        e.numbers.set(EpisodeNumber::new(
            EpisodeGroup::new(cv_domain::EpisodeOrdering::Alternate, "Director's cut"),
            0,
            17,
        ));
        e.first_aired = CalendarDate::new(2022, 2, 18);
        e.runtime_minutes = 55;
        e.multi_episode = true;
        e.watched = true;
        e.media_source = MediaSource::WebDl;
    }
    let episode_id = insert_item(&conn, &episode).unwrap();

    assert_eq!(
        get_item(&conn, show_id).unwrap().unwrap(),
        with_id(show, show_id)
    );
    assert_eq!(
        get_item(&conn, season_id).unwrap().unwrap(),
        with_id(season, season_id)
    );
    let back = get_item(&conn, episode_id).unwrap().unwrap();
    let ItemDetail::Episode(e) = &back.detail else {
        panic!("not an episode")
    };
    assert_eq!(e.numbers.len(), 3);
    assert_eq!(
        e.numbers
            .get(&EpisodeGroup::dvd())
            .map(|n| (n.season, n.episode)),
        Some((1, 3))
    );
    assert_eq!(back, with_id(episode, episode_id));
    assert_eq!(children_of(&conn, show_id).unwrap(), vec![season_id]);
}

#[test]
fn a_set_keeps_its_missing_movies_and_members_can_be_detached() {
    let conn = memory_db();
    let mut set = Item::set("Alien Collection");
    if let ItemDetail::Set(s) = &mut set.detail {
        s.missing_movies = vec![MissingMovie {
            title: "Alien 3".into(),
            year: 1992,
            release_date: CalendarDate::new(1992, 5, 22),
            ..MissingMovie::default()
        }];
    }
    let set_id = insert_item(&conn, &set).unwrap();
    let mut movie = Item::movie("Alien");
    movie.parent_id = Some(set_id);
    let movie_id = insert_item(&conn, &movie).unwrap();

    assert_eq!(
        get_item(&conn, set_id).unwrap().unwrap(),
        with_id(set, set_id)
    );

    delete_item(&conn, set_id).unwrap();

    assert!(get_item(&conn, set_id).unwrap().is_none());
    let movie = get_item(&conn, movie_id).unwrap().unwrap();
    assert_eq!(
        movie.parent_id, None,
        "the member movie stays in the library"
    );
}

#[test]
fn update_replaces_every_related_row() {
    let conn = memory_db();
    let id = insert_item(&conn, &fully_populated_movie()).unwrap();

    let mut edited = get_item(&conn, id).unwrap().unwrap();
    edited.core.title = "Blade Runner 2049 (Director's Cut)".into();
    edited.core.ids.remove("kinopoisk");
    edited.core.ids.set("trakt", "99");
    edited.core.ratings.remove("metacritic");
    edited.core.tags.remove("favourite");
    edited.core.tags.add("neo-noir");
    edited.core.artwork_urls.remove(&MediaFileType::Fanart);
    edited.core.credits.truncate(1);
    edited.core.media_files.remove(1);
    edited.core.media_files[0].audio_streams.truncate(1);
    if let ItemDetail::Movie(m) = &mut edited.detail {
        m.genres = vec![Genre::Drama];
        m.trailers.clear();
        m.watched = false;
    }

    update_item(&conn, &edited).unwrap();

    assert_eq!(get_item(&conn, id).unwrap().unwrap(), edited);
    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(count("SELECT count(*) FROM item"), 1);
    assert_eq!(count("SELECT count(*) FROM media_file"), 1);
    assert_eq!(count("SELECT count(*) FROM stream"), 2);
    assert_eq!(count("SELECT count(*) FROM credit"), 1);
    assert_eq!(count("SELECT count(*) FROM trailer"), 0);
}

#[test]
fn update_keeps_the_runtime_flags_the_domain_does_not_own() {
    let conn = memory_db();
    let id = insert_item(&conn, &Item::movie("Alien")).unwrap();
    conn.execute(
        "UPDATE item SET is_new = 1, review_state = 'needs_review', availability = 'offline'
         WHERE id = ?1",
        [id.0],
    )
    .unwrap();

    let mut item = get_item(&conn, id).unwrap().unwrap();
    item.core.plot = "In space".into();
    update_item(&conn, &item).unwrap();

    let s = get_summary(&conn, id).unwrap().unwrap();
    assert!(s.is_new);
    assert_eq!(s.review_state, "needs_review");
    assert_eq!(s.availability, "offline");
    assert_eq!(get_item(&conn, id).unwrap().unwrap().core.plot, "In space");
}

#[test]
fn an_unchanged_artwork_url_keeps_its_cached_hash_and_a_changed_one_drops_it() {
    let conn = memory_db();
    let mut movie = Item::movie("Alien");
    movie
        .core
        .artwork_urls
        .insert(MediaFileType::Poster, "https://example.test/a.jpg".into());
    movie
        .core
        .artwork_urls
        .insert(MediaFileType::Fanart, "https://example.test/f.jpg".into());
    let id = insert_item(&conn, &movie).unwrap();
    conn.execute(
        "UPDATE artwork_ref SET cache_hash = 'abc', width = 1000, height = 1500 WHERE item_id = ?1",
        [id.0],
    )
    .unwrap();

    let mut edited = get_item(&conn, id).unwrap().unwrap();
    edited
        .core
        .artwork_urls
        .insert(MediaFileType::Fanart, "https://example.test/f2.jpg".into());
    update_item(&conn, &edited).unwrap();

    let hash = |kind: &str| -> (Option<String>, i64) {
        conn.query_row(
            "SELECT cache_hash, width FROM artwork_ref WHERE item_id = ?1 AND kind = ?2",
            params![id.0, kind],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    assert_eq!(hash("poster"), (Some("abc".into()), 1000));
    assert_eq!(hash("fanart"), (None, 0));
}

#[test]
fn update_errors_are_specific() {
    let conn = memory_db();
    assert!(matches!(
        update_item(&conn, &Item::movie("x")),
        Err(DbError::ItemWithoutId)
    ));
    assert!(matches!(
        update_item(&conn, &with_id(Item::movie("x"), ItemId(404))),
        Err(DbError::ItemNotFound(404))
    ));
    let id = insert_item(&conn, &Item::movie("x")).unwrap();
    let as_set = with_id(Item::set("x"), id);
    assert!(matches!(
        update_item(&conn, &as_set),
        Err(DbError::InvalidData(_))
    ));
}

#[test]
fn people_are_shared_between_items_by_name_and_ids() {
    let conn = memory_db();
    for title in ["A", "B"] {
        let mut m = Item::movie(title);
        m.core
            .credits
            .push(Credit::new(CreditKind::Director, "Denis Villeneuve", ""));
        insert_item(&conn, &m).unwrap();
    }
    let mut other = Item::movie("C");
    let mut namesake = Credit::new(CreditKind::Actor, "Denis Villeneuve", "");
    namesake.person.ids.set("tmdb", "1");
    other.core.credits.push(namesake);
    insert_item(&conn, &other).unwrap();

    let people: i64 = conn
        .query_row("SELECT count(*) FROM person", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        people, 2,
        "same name without IDs is one person; IDs split it"
    );
}

#[test]
fn deleting_removes_one_item_and_refuses_parents_with_children() {
    let conn = memory_db();
    let show = insert_item(&conn, &Item::show("Severance")).unwrap();
    let season = insert_item(&conn, &Item::season(show, 1)).unwrap();
    let mut ep = Item::episode(show, "Pilot");
    ep.parent_id = Some(season);
    let ep = insert_item(&conn, &ep).unwrap();

    assert!(matches!(
        delete_item(&conn, show),
        Err(DbError::HasChildren { count: 1, .. })
    ));
    assert!(matches!(
        delete_item(&conn, ItemId(999)),
        Err(DbError::ItemNotFound(999))
    ));
    delete_item(&conn, ep).unwrap();
    assert!(!item_exists(&conn, ep).unwrap());
    assert!(item_exists(&conn, show).unwrap());

    let mut ep2 = Item::episode(show, "Second");
    ep2.parent_id = Some(season);
    insert_item(&conn, &ep2).unwrap();
    assert_eq!(delete_tree(&conn, show).unwrap(), 3);
    let left: i64 = conn
        .query_row("SELECT count(*) FROM item", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn provider_id_lookup_and_unused_lookup_pruning() {
    let conn = memory_db();
    let mut m = Item::movie("Alien");
    m.core.ids.set("imdb", "tt0078748");
    m.core.tags.add("horror");
    m.core
        .credits
        .push(Credit::new(CreditKind::Actor, "Sigourney Weaver", ""));
    if let ItemDetail::Movie(d) = &mut m.detail {
        d.genres = vec![Genre::Horror];
    }
    let id = insert_item(&conn, &m).unwrap();

    assert_eq!(
        items_by_provider_id(&conn, "imdb", "tt0078748").unwrap(),
        vec![id]
    );
    assert!(
        items_by_provider_id(&conn, "imdb", "nope")
            .unwrap()
            .is_empty()
    );

    assert_eq!(prune_unused_lookups(&conn).unwrap(), 0);
    delete_item(&conn, id).unwrap();
    assert_eq!(prune_unused_lookups(&conn).unwrap(), 3);
}

// ---- full-text index -------------------------------------------------------------------------

fn search_ids(conn: &Connection, text: &str) -> Vec<ItemId> {
    fts::search(conn, text, 100)
        .unwrap()
        .into_iter()
        .map(|h| h.id)
        .collect()
}

#[test]
fn full_text_search_covers_titles_plot_people_tags_and_file_names() {
    let conn = memory_db();
    let id = insert_item(&conn, &fully_populated_movie()).unwrap();
    let other = insert_item(&conn, &Item::movie("Arrival")).unwrap();

    for (what, text) in [
        ("title", "blade runner"),
        ("original title", "orig"),
        ("sort title", "02"),
        ("plot", "buried secret"),
        ("person", "villeneuve"),
        ("person prefix", "villen"),
        ("role person", "gosling"),
        ("tag", "cyberpunk"),
        ("file name", "br2049"),
        ("diacritics", "bladé"),
        ("english title", "Blade"),
    ] {
        assert_eq!(search_ids(&conn, text), vec![id], "{what}: {text}");
    }
    assert_eq!(search_ids(&conn, "arriv"), vec![other]);
    assert!(search_ids(&conn, "nonexistentword").is_empty());
}

#[test]
fn the_index_follows_updates_and_deletes() {
    let conn = memory_db();
    let id = insert_item(&conn, &Item::movie("Alien")).unwrap();
    assert_eq!(search_ids(&conn, "alien"), vec![id]);

    let mut item = get_item(&conn, id).unwrap().unwrap();
    item.core.title = "Aliens".into();
    item.core.tags.add("sequel");
    item.core
        .credits
        .push(Credit::new(CreditKind::Director, "James Cameron", ""));
    update_item(&conn, &item).unwrap();
    assert_eq!(search_ids(&conn, "cameron"), vec![id]);
    assert_eq!(search_ids(&conn, "sequel"), vec![id]);

    item.core.credits.clear();
    update_item(&conn, &item).unwrap();
    assert!(search_ids(&conn, "cameron").is_empty());

    delete_item(&conn, id).unwrap();
    assert!(search_ids(&conn, "aliens").is_empty());
}

#[test]
fn rebuild_recreates_the_index_for_every_item() {
    let conn = memory_db();
    insert_item(&conn, &Item::movie("Alien")).unwrap();
    insert_item(&conn, &Item::movie("Aliens")).unwrap();
    conn.execute("DELETE FROM item_fts", []).unwrap();
    assert!(search_ids(&conn, "alien").is_empty());

    assert_eq!(fts::rebuild(&conn).unwrap(), 2);

    assert_eq!(search_ids(&conn, "alien").len(), 2);
}

#[test]
fn match_expressions_neutralise_operators_and_punctuation() {
    assert_eq!(fts::match_expression("  "), None);
    assert_eq!(fts::match_expression("- * \"\""), None);
    assert_eq!(
        fts::match_expression("spider-man AND \"x").as_deref(),
        Some("\"spider-man\"* \"AND\"* \"\"\"x\"*")
    );
    let conn = memory_db();
    let id = insert_item(&conn, &Item::movie("Spider-Man: No Way Home")).unwrap();
    // None of these may raise an FTS syntax error.
    for text in [
        "spider-man",
        "NOT",
        "a OR",
        "\"",
        "(",
        "col:x",
        "no way hom",
        "*",
    ] {
        fts::search(&conn, text, 10).unwrap();
    }
    assert_eq!(search_ids(&conn, "spider-man no way"), vec![id]);
}

#[test]
fn search_ranks_title_matches_above_plot_matches() {
    let conn = memory_db();
    let mut plot_only = Item::movie("Something Else");
    plot_only.core.plot = "A story about a heist".into();
    let plot_id = insert_item(&conn, &plot_only).unwrap();
    let title_id = insert_item(&conn, &Item::movie("The Heist")).unwrap();

    assert_eq!(search_ids(&conn, "heist"), vec![title_id, plot_id]);
}

#[test]
fn title_first_lists_title_matches_before_plot_matches_whatever_the_alphabet() {
    let conn = memory_db();
    // "A Dark Comedy" sorts first by title but matches only by its tag; "Dark Waters" matches by
    // its title and sorts after it.
    let mut by_tag = Item::movie("A Dark Comedy Without Words");
    by_tag.core.plot = "A quiet film with a dark ending".into();
    let tag_id = insert_item(&conn, &by_tag).unwrap();
    let mut plot_only = Item::movie("Aardvark Tales");
    plot_only.core.plot = "Dark nights on the savannah".into();
    let plot_id = insert_item(&conn, &plot_only).unwrap();
    let title_id = insert_item(&conn, &Item::movie("Zodiac of Dark Waters")).unwrap();

    let plain = ItemQuery::new(ItemKind::Movie).filter(Filter::Search {
        text: "dark".into(),
    });
    assert_eq!(walk(&conn, &plain, 10), vec![tag_id, plot_id, title_id]);

    let first = plain.clone().title_first("dark");
    // Title matches ("A Dark Comedy…", "Zodiac of Dark Waters") come before the plot-only one.
    assert_eq!(walk(&conn, &first, 10), vec![tag_id, title_id, plot_id]);
    // Paging by keyset keeps the same order.
    assert_eq!(walk(&conn, &first, 1), vec![tag_id, title_id, plot_id]);
    // An apostrophe or a LIKE wildcard in the words is just text: the statement still runs.
    let odd = ItemQuery::new(ItemKind::Movie).title_first("it's 100% _");
    assert_eq!(list_items(&conn, &odd, None, 10).unwrap().items.len(), 3);
}

// ---- filters, sorting and keyset paging ------------------------------------------------------

fn movie(title: &str, year: u16) -> Item {
    let mut m = Item::movie(title);
    m.core.year = year;
    m
}

fn titles(page: &Page) -> Vec<&str> {
    page.items.iter().map(|i| i.title.as_str()).collect()
}

/// Walks every page and returns the ids in order.
fn walk(conn: &Connection, query: &ItemQuery, page_size: usize) -> Vec<ItemId> {
    let mut all = Vec::new();
    let mut cursor: Option<Cursor> = None;
    loop {
        let page = list_items(conn, query, cursor.as_ref(), page_size).unwrap();
        all.extend(page.items.iter().map(|i| i.id));
        match page.next {
            Some(c) => cursor = Some(c),
            None => return all,
        }
    }
}

fn sample_library(conn: &Connection) -> Vec<ItemId> {
    let rows: &[(&str, u16, Option<&str>, i64, u32)] = &[
        ("alien", 1979, Some("1979-05-25"), 100, 117),
        ("Aliens", 1986, Some("1986-07-18"), 200, 137),
        ("Alien 3", 1992, Some("1992-05-22"), 300, 114),
        ("Blade Runner", 1982, Some("1982-06-25"), 400, 117),
        ("Arrival", 2016, None, 500, 116),
        ("Arrival", 2016, Some("2016-11-11"), 600, 116),
        ("Dune", 2021, Some("2021-10-22"), 700, 155),
        ("Dune", 2021, Some("2021-10-22"), 800, 155),
    ];
    rows.iter()
        .map(|(title, year, release, added, runtime)| {
            let mut m = movie(title, *year);
            m.core.date_added_ms = *added;
            if let ItemDetail::Movie(d) = &mut m.detail {
                d.release_date = release.map(|r| r.parse().unwrap());
                d.runtime_minutes = *runtime;
            }
            insert_item(conn, &m).unwrap()
        })
        .collect()
}

#[test]
fn default_order_is_title_ignoring_case_with_id_as_tiebreaker() {
    let conn = memory_db();
    let ids = sample_library(&conn);

    let page = list_items(&conn, &ItemQuery::new(ItemKind::Movie), None, 100).unwrap();

    assert_eq!(
        titles(&page),
        [
            "alien",
            "Alien 3",
            "Aliens",
            "Arrival",
            "Arrival",
            "Blade Runner",
            "Dune",
            "Dune"
        ]
    );
    // Equal titles come in id order.
    assert_eq!(page.items[3].id, ids[4]);
    assert_eq!(page.items[4].id, ids[5]);
    assert!(page.next.is_none());
}

#[test]
fn keyset_paging_visits_every_row_once_for_any_sort() {
    let conn = memory_db();
    sample_library(&conn);
    let sorts: Vec<Vec<SortKey>> = vec![
        vec![],
        vec![SortKey::desc(SortField::Title)],
        vec![
            SortKey::asc(SortField::Year),
            SortKey::desc(SortField::Title),
        ],
        vec![
            SortKey::desc(SortField::Year),
            SortKey::asc(SortField::DateAdded),
        ],
        vec![SortKey::asc(SortField::ReleaseDate)],
        vec![SortKey::desc(SortField::ReleaseDate)],
        vec![
            SortKey::desc(SortField::ReleaseDate),
            SortKey::desc(SortField::Runtime),
        ],
        vec![SortKey::asc(SortField::SortTitle)],
        vec![
            SortKey::desc(SortField::Runtime),
            SortKey::asc(SortField::Path),
        ],
    ];
    for sort in sorts {
        let mut query = ItemQuery::new(ItemKind::Movie);
        query.sort = sort.clone();
        let everything = walk(&conn, &query, 1000);
        assert_eq!(everything.len(), 8, "{sort:?}");
        for size in [1, 2, 3, 5] {
            assert_eq!(
                walk(&conn, &query, size),
                everything,
                "{sort:?} page {size}"
            );
        }
    }
}

#[test]
fn nulls_sort_last_in_both_directions() {
    let conn = memory_db();
    sample_library(&conn);
    let asc = ItemQuery::new(ItemKind::Movie).sort_by(SortKey::asc(SortField::ReleaseDate));
    let desc = ItemQuery::new(ItemKind::Movie).sort_by(SortKey::desc(SortField::ReleaseDate));

    let first = list_items(&conn, &asc, None, 100).unwrap();
    let last = list_items(&conn, &desc, None, 100).unwrap();

    assert!(first.items.iter().any(|i| i.release_date.is_none()));
    assert_eq!(first.items.last().unwrap().release_date, None);
    assert_eq!(last.items.last().unwrap().release_date, None);
    assert!(first.items[0].release_date.is_some());
    assert!(last.items[0].release_date.is_some());
}

#[test]
fn an_unknown_year_or_runtime_sorts_last_in_both_directions() {
    let conn = memory_db();
    for (title, year, runtime) in [
        ("Known A", 1999, 90),
        ("Unknown", 0, 0),
        ("Known B", 2010, 120),
    ] {
        let mut item = movie(title, year);
        if let ItemDetail::Movie(d) = &mut item.detail {
            d.runtime_minutes = runtime;
        }
        insert_item(&conn, &item).unwrap();
    }
    for field in [SortField::Year, SortField::Runtime] {
        for key in [SortKey::asc(field), SortKey::desc(field)] {
            let query = ItemQuery::new(ItemKind::Movie).sort_by(key);
            for size in [1, 2, 100] {
                let all = walk(&conn, &query, size);
                assert_eq!(all.len(), 3);
                let names = list_items(&conn, &query, None, 100)
                    .unwrap()
                    .items
                    .into_iter()
                    .map(|i| i.title)
                    .collect::<Vec<_>>();
                assert_eq!(names.last().map(String::as_str), Some("Unknown"), "{key:?}");
            }
        }
    }
}

#[test]
fn paging_is_stable_while_rows_are_added_between_pages() {
    let conn = memory_db();
    sample_library(&conn);
    let query = ItemQuery::new(ItemKind::Movie);
    let first = list_items(&conn, &query, None, 3).unwrap();

    insert_item(&conn, &movie("Aaa new", 2000)).unwrap(); // sorts before the cursor
    insert_item(&conn, &movie("Zzz new", 2000)).unwrap(); // sorts after it
    let second = list_items(&conn, &query, first.next.as_ref(), 100).unwrap();

    let second_titles = titles(&second);
    assert_eq!(second_titles.first(), Some(&"Arrival"));
    assert!(second_titles.contains(&"Zzz new"));
    assert!(!second_titles.contains(&"Aaa new"), "already passed");
    assert_eq!(second_titles.len(), 6);
}

#[test]
fn a_cursor_from_another_sort_is_rejected() {
    let conn = memory_db();
    sample_library(&conn);
    let two_keys = ItemQuery::new(ItemKind::Movie)
        .sort_by(SortKey::asc(SortField::Year))
        .sort_by(SortKey::asc(SortField::Title));
    let cursor = list_items(&conn, &two_keys, None, 2).unwrap().next.unwrap();

    let err = list_items(&conn, &ItemQuery::new(ItemKind::Movie), Some(&cursor), 2).unwrap_err();

    assert!(matches!(err, DbError::InvalidCursor));
}

#[test]
fn cursors_and_queries_serialise_for_ipc() {
    let conn = memory_db();
    sample_library(&conn);
    let query = ItemQuery::new(ItemKind::Movie)
        .filter(Filter::and([
            Filter::Year {
                range: IntRange::at_least(1980),
            },
            Filter::not(Filter::Search {
                text: "dune".into(),
            }),
        ]))
        .sort_by(SortKey::desc(SortField::ReleaseDate));
    let query: ItemQuery = serde_json::from_str(&serde_json::to_string(&query).unwrap()).unwrap();
    let page = list_items(&conn, &query, None, 2).unwrap();
    let cursor: Cursor =
        serde_json::from_str(&serde_json::to_string(&page.next.unwrap()).unwrap()).unwrap();

    let rest = list_items(&conn, &query, Some(&cursor), 100).unwrap();

    assert_eq!(page.items.len() + rest.items.len(), 5);
}

fn matching(conn: &Connection, filter: Filter) -> Vec<String> {
    let query = ItemQuery::new(ItemKind::Movie).filter(filter);
    let page = list_items(conn, &query, None, 1000).unwrap();
    assert_eq!(
        count_items(conn, &query).unwrap() as usize,
        page.items.len(),
        "count agrees with the listing"
    );
    page.items.into_iter().map(|i| i.title).collect()
}

fn catalogue(conn: &Connection) {
    let mut a = movie("Alien", 1979);
    a.core.data_source = "D:\\Movies".into();
    a.core.tags.add("horror");
    a.core.tags.add("classic");
    a.core.ids.set("imdb", "tt0078748");
    a.core.ratings.insert(Rating::new("imdb", 8.5, 1, 10));
    a.core
        .credits
        .push(Credit::new(CreditKind::Director, "Ridley Scott", ""));
    a.core
        .credits
        .push(Credit::new(CreditKind::Actor, "Sigourney Weaver", "Ripley"));
    let mut v = MediaFile::new(MediaFileType::Video, "D:\\Movies\\Alien", "alien.mkv");
    v.height = 1080;
    v.video_codec = "H.264".into();
    v.hdr_formats = vec!["HDR10".into()];
    v.audio_streams.push(AudioStream {
        language: "eng".into(),
        ..AudioStream::default()
    });
    v.subtitles.push(SubtitleStream {
        language: "deu".into(),
        ..SubtitleStream::default()
    });
    a.core.media_files.push(v);
    if let ItemDetail::Movie(d) = &mut a.detail {
        d.genres = vec![Genre::Horror, Genre::ScienceFiction];
        d.watched = true;
        d.certification = Certification("US_R".into());
        d.media_source = MediaSource::Bluray;
        d.edition = Edition::DirectorsCut;
        d.top250 = 200;
        d.runtime_minutes = 117;
        d.release_date = CalendarDate::new(1979, 5, 25);
    }
    insert_item(conn, &a).unwrap();

    let mut b = movie("Blade Runner 2049", 2017);
    b.core.data_source = "E:\\Films".into();
    b.core.tags.add("Classic");
    b.core.ids.set("tmdb", "335984");
    b.core.ratings.insert(Rating::new("imdb", 80.0, 1, 100));
    b.core
        .credits
        .push(Credit::new(CreditKind::Director, "Denis Villeneuve", ""));
    b.core
        .credits
        .push(Credit::new(CreditKind::Actor, "Ridley Scott", "cameo"));
    let mut v = MediaFile::new(MediaFileType::Video, "E:\\Films\\BR", "br.mkv");
    v.height = 2160;
    v.video_codec = "HEVC".into();
    v.hdr_formats = vec!["Dolby Vision".into(), "HDR10".into()];
    v.audio_streams.push(AudioStream {
        language: "fra".into(),
        ..AudioStream::default()
    });
    b.core.media_files.push(v);
    if let ItemDetail::Movie(d) = &mut b.detail {
        d.genres = vec![Genre::ScienceFiction];
        d.certification = Certification("US_R".into());
        d.media_source = MediaSource::UhdBluray;
        d.runtime_minutes = 164;
        d.release_date = CalendarDate::new(2017, 10, 4);
    }
    insert_item(conn, &b).unwrap();

    insert_item(conn, &movie("Plain", 1990)).unwrap();
}

#[test]
fn leaf_filters_select_the_expected_rows() {
    let conn = memory_db();
    catalogue(&conn);
    let strings = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let m = |f| matching(&conn, f);

    assert_eq!(m(Filter::All).len(), 3);
    assert_eq!(
        m(Filter::Year {
            range: IntRange::between(1980, 2020)
        }),
        ["Blade Runner 2049", "Plain"]
    );
    assert_eq!(
        m(Filter::RuntimeMinutes {
            range: IntRange::at_least(150)
        }),
        ["Blade Runner 2049"]
    );
    assert_eq!(
        m(Filter::ReleaseDate {
            range: TextRange {
                from: Some("1979".into()),
                to: Some("1979-12".into())
            }
        }),
        ["Alien"]
    );
    assert_eq!(
        m(Filter::Top250 {
            range: IntRange::at_most(250)
        }),
        ["Alien"]
    );
    assert_eq!(
        m(Filter::DataSource {
            values: strings(&["d:\\movies"])
        }),
        ["Alien"]
    );
    assert_eq!(
        m(Filter::MediaSource {
            values: strings(&["BLURAY", "UHD_BLURAY"])
        })
        .len(),
        2
    );
    assert_eq!(
        m(Filter::MediaSource { values: vec![] }),
        Vec::<String>::new()
    );
    assert_eq!(
        m(Filter::Edition {
            values: strings(&["DIRECTORS_CUT"])
        }),
        ["Alien"]
    );
    assert_eq!(
        m(Filter::Flag {
            flag: ItemFlag::Watched,
            value: true
        }),
        ["Alien"]
    );
    assert_eq!(
        m(Filter::Flag {
            flag: ItemFlag::Watched,
            value: false
        })
        .len(),
        2
    );
    assert_eq!(
        m(Filter::Availability {
            values: strings(&["online"])
        })
        .len(),
        3
    );
}

#[test]
fn relation_filters_select_the_expected_rows() {
    let conn = memory_db();
    catalogue(&conn);
    let strings = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let m = |f| matching(&conn, f);

    assert_eq!(
        m(Filter::Genre {
            names: strings(&["science_fiction"]),
            mode: MatchMode::Any
        })
        .len(),
        2
    );
    assert_eq!(
        m(Filter::Genre {
            names: strings(&["HORROR", "science_fiction"]),
            mode: MatchMode::All
        }),
        ["Alien"]
    );
    assert_eq!(
        m(Filter::Genre {
            names: vec![],
            mode: MatchMode::Any
        })
        .len(),
        0
    );
    assert_eq!(
        m(Filter::Person {
            name: "ridley scott".into(),
            kind: None
        })
        .len(),
        2
    );
    assert_eq!(
        m(Filter::Person {
            name: "Ridley Scott".into(),
            kind: Some(CreditKind::Director)
        }),
        ["Alien"]
    );
    assert_eq!(
        m(Filter::HasProviderId {
            provider: "imdb".into(),
            present: false
        })
        .len(),
        2
    );
    assert_eq!(
        m(Filter::ProviderId {
            provider: "tmdb".into(),
            value: "335984".into()
        }),
        ["Blade Runner 2049"]
    );
    // 80/100 and 8.5/10 are both on the 10 scale
    assert_eq!(
        m(Filter::Rating {
            source: "imdb".into(),
            min: Some(8.2),
            max: None
        }),
        ["Alien"]
    );
    assert_eq!(
        m(Filter::Rating {
            source: "imdb".into(),
            min: Some(7.9),
            max: Some(8.1)
        }),
        ["Blade Runner 2049"]
    );
}

#[test]
fn video_and_stream_filters_select_the_expected_rows() {
    let conn = memory_db();
    catalogue(&conn);
    let strings = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let m = |f| matching(&conn, f);

    assert_eq!(
        m(Filter::VideoHeight {
            range: IntRange::at_least(2000)
        }),
        ["Blade Runner 2049"]
    );
    assert_eq!(
        m(Filter::VideoCodec {
            values: strings(&["hevc"])
        }),
        ["Blade Runner 2049"]
    );
    assert_eq!(
        m(Filter::Hdr {
            values: strings(&["dolby vision"]),
            mode: MatchMode::Any
        }),
        ["Blade Runner 2049"]
    );
    assert_eq!(
        m(Filter::Hdr {
            values: strings(&["HDR10", "Dolby Vision"]),
            mode: MatchMode::All
        }),
        ["Blade Runner 2049"]
    );
    assert_eq!(
        m(Filter::Hdr {
            values: strings(&["HDR10"]),
            mode: MatchMode::Any
        })
        .len(),
        2
    );
    assert_eq!(
        m(Filter::AudioLanguage {
            values: strings(&["fra"])
        }),
        ["Blade Runner 2049"]
    );
    assert_eq!(
        m(Filter::SubtitleLanguage {
            values: strings(&["deu"])
        }),
        ["Alien"]
    );
}

#[test]
fn combinators_and_search_compose() {
    let conn = memory_db();
    catalogue(&conn);
    let m = |f| matching(&conn, f);

    assert_eq!(
        m(Filter::and([
            Filter::Search {
                text: "scott".into()
            },
            Filter::Year {
                range: IntRange::at_most(2000)
            },
        ])),
        ["Alien"]
    );
    assert_eq!(
        m(Filter::or([
            Filter::Search {
                text: "villeneuve".into()
            },
            Filter::Year {
                range: IntRange::at_most(1985)
            },
        ]))
        .len(),
        2
    );
    assert_eq!(
        m(Filter::not(Filter::Search {
            text: "scott".into()
        })),
        ["Plain"]
    );
    assert_eq!(m(Filter::and([])).len(), 3, "empty AND matches everything");
    assert!(m(Filter::or([])).is_empty(), "empty OR matches nothing");
    assert_eq!(
        m(Filter::Search { text: "  ".into() }).len(),
        3,
        "a blank search does not restrict"
    );
}

#[test]
fn filters_only_apply_to_the_requested_kind_and_parent() {
    let conn = memory_db();
    let show = insert_item(&conn, &Item::show("Severance")).unwrap();
    insert_item(&conn, &Item::season(show, 2)).unwrap();
    let s1 = insert_item(&conn, &Item::season(show, 1)).unwrap();
    insert_item(&conn, &Item::movie("Severance (film)")).unwrap();

    let seasons = ItemQuery::new(ItemKind::Season)
        .filter(Filter::Parent { parent: Some(show) })
        .sort_by(SortKey::asc(SortField::SeasonNumber));
    let page = list_items(&conn, &seasons, None, 10).unwrap();
    assert_eq!(page.items.first().map(|i| i.id), Some(s1));
    assert_eq!(page.items.len(), 2);

    let top_level = ItemQuery::new(ItemKind::Show).filter(Filter::Parent { parent: None });
    assert_eq!(count_items(&conn, &top_level).unwrap(), 1);
    let movies = ItemQuery::new(ItemKind::Movie);
    assert_eq!(count_items(&conn, &movies).unwrap(), 1);
}

#[test]
fn hostile_text_in_filters_is_bound_not_executed() {
    let conn = memory_db();
    catalogue(&conn);
    let evil = "x') OR 1=1 --".to_owned();

    for f in [
        Filter::Person {
            name: evil.clone(),
            kind: None,
        },
        Filter::DataSource {
            values: vec![evil.clone()],
        },
        Filter::Genre {
            names: vec![evil.clone()],
            mode: MatchMode::Any,
        },
        Filter::Search { text: evil.clone() },
        Filter::ProviderId {
            provider: evil.clone(),
            value: evil.clone(),
        },
    ] {
        assert!(matching(&conn, f).is_empty());
    }
}

#[test]
fn summaries_expose_the_list_columns() {
    let conn = memory_db();
    let id = insert_item(&conn, &fully_populated_movie()).unwrap();

    let s = get_summary(&conn, id).unwrap().unwrap();

    assert_eq!(s.title, "Blade Runner 2049");
    assert_eq!(s.year, 2017);
    assert_eq!(s.release_date.as_deref(), Some("2017-10-04"));
    assert_eq!(s.runtime_minutes, 164);
    assert!(s.watched && s.locked);
    assert_eq!(s.availability, "online");
    assert_eq!(s.kind, ItemKind::Movie);
    assert!(get_summary(&conn, ItemId(12345)).unwrap().is_none());
}

#[test]
fn repositories_work_through_the_database_handle() {
    let (_dir, db) = temp_db();
    let id = db
        .write(|tx| {
            let id = insert_item(tx, &Item::movie("Alien"))?;
            update_item(tx, &with_id(movie("Alien", 1979), id))?;
            Ok(id)
        })
        .unwrap();

    let page = db
        .read(|c| list_items(c, &ItemQuery::new(ItemKind::Movie), None, 10))
        .unwrap();

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, id);
    assert_eq!(page.items[0].year, 1979);

    // a failing write rolls the whole repository change back
    let res: Result<(), _> = db.write(|tx| {
        insert_item(tx, &Item::movie("Ghost"))?;
        Err(DbError::InvalidCursor)
    });
    assert!(res.is_err());
    let n = db
        .read(|c| count_items(c, &ItemQuery::new(ItemKind::Movie)))
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn a_search_finds_the_start_of_an_original_title_in_japanese() {
    let conn = memory_db();
    let mut spirited = movie("Spirited Away", 2001);
    spirited.core.original_title = "千と千尋の神隠し".into();
    insert_item(&conn, &spirited).unwrap();
    insert_item(&conn, &movie("Alien", 1979)).unwrap();
    for text in ["千と", "千と千尋", "神隠し", "spirited"] {
        assert_eq!(
            matching(&conn, Filter::Search { text: text.into() }),
            ["Spirited Away"],
            "{text}"
        );
    }
}

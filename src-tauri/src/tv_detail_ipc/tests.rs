use super::*;
use cv_db::repo::update_item;
use cv_domain::{AudioStream, Credit, EpisodeNumber, Genre, MediaFile, MissingEpisode, StreamFlag};

struct Lab {
    dir: tempfile::TempDir,
    db: Arc<Database>,
    sources: Arc<LibrarySources>,
}

impl Lab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
        Self {
            dir,
            db,
            sources: Arc::new(LibrarySources::new()),
        }
    }

    fn reader(&self) -> TvReader {
        TvReader::new(self.db.clone(), self.sources.clone(), None)
    }

    fn add(&self, item: Item) -> ItemId {
        self.db
            .write(move |conn| cv_db::repo::insert_item(conn, &item))
            .unwrap()
    }

    fn item(&self, id: ItemId) -> Item {
        self.db
            .read(|conn| get_item(conn, id))
            .unwrap()
            .expect("item")
    }
}

fn date(year: i32, month: u8, day: u8) -> Option<CalendarDate> {
    CalendarDate::new(year, month, day)
}

/// A video file with media info.
fn video(dir: &str, name: &str, height: u32) -> MediaFile {
    let mut file = MediaFile::new(MediaFileType::Video, dir, name);
    file.size_bytes = 1_500_000_000;
    file.video_codec = "HEVC".into();
    file.container_format = "Matroska".into();
    file.width = height * 16 / 9;
    file.height = height;
    file.audio_streams = vec![AudioStream {
        codec: "E-AC-3".into(),
        language: "en".into(),
        channels: 6,
        flags: [StreamFlag::Default].into_iter().collect(),
        ..AudioStream::default()
    }];
    file
}

struct Fixture {
    show: ItemId,
    season_one: ItemId,
    episodes: Vec<ItemId>,
    special: ItemId,
    loose: ItemId,
}

/// A show with a season record, five numbered episodes (two of them with the same number), one
/// special, one episode without numbering, guests, a missing episode, an unaired one and one of
/// a season the library does not have.
fn severance(lab: &Lab) -> Fixture {
    let folder = lab.dir.path().join("Severance");
    let season_folder = folder.join("Season 1");
    std::fs::create_dir_all(&season_folder).unwrap();
    std::fs::write(folder.join("fanart.jpg"), b"fanart bytes").unwrap();
    std::fs::write(season_folder.join("S01E01-thumb.jpg"), b"thumb bytes").unwrap();
    let dir = folder.to_string_lossy().into_owned();
    let season_dir = season_folder.to_string_lossy().into_owned();

    let mut show = Item::show("Severance");
    show.core.year = 2022;
    show.core.path = dir.clone();
    show.core.plot = "Mark leads a team whose memories are split.".into();
    show.core.production_companies = vec!["Apple TV+".into()];
    show.core.ids.set("tvdb", "371980");
    show.core.credits.extend([
        Credit::new(CreditKind::Actor, "Adam Scott", "Mark Scout"),
        Credit::new(CreditKind::Director, "Ben Stiller", ""),
    ]);
    show.core.media_files.push(MediaFile::new(
        MediaFileType::Fanart,
        dir.clone(),
        "fanart.jpg",
    ));
    show.core.media_files.push(MediaFile::new(
        MediaFileType::Nfo,
        dir.clone(),
        "tvshow.nfo",
    ));
    show.core.media_files.push(MediaFile::new(
        MediaFileType::Theme,
        dir.clone(),
        "theme.mp3",
    ));
    if let ItemDetail::Show(fields) = &mut show.detail {
        fields.status = AiredStatus::Continuing;
        fields.first_aired = date(2022, 2, 18);
        fields.genres = vec![Genre::ScienceFiction];
        fields.runtime_minutes = 50;
        fields.missing_episodes = vec![
            MissingEpisode {
                title: "Aired but absent".into(),
                numbers: {
                    let mut n = cv_domain::EpisodeNumbers::new();
                    n.set(EpisodeNumber::aired(1, 6));
                    n
                },
                first_aired: date(2022, 4, 1),
                plot: String::new(),
            },
            MissingEpisode {
                title: "Next season opener".into(),
                numbers: {
                    let mut n = cv_domain::EpisodeNumbers::new();
                    n.set(EpisodeNumber::aired(2, 1));
                    n
                },
                first_aired: date(2099, 1, 17),
                plot: String::new(),
            },
        ];
        fields.season_texts = vec![cv_domain::SeasonText {
            group: EpisodeGroup::aired(),
            season: 1,
            name: "The Innies".into(),
            overview: "The first season.".into(),
        }];
    }
    let show_id = lab.add(show);

    let mut season = Item::season(show_id, 1);
    season.core.path = season_dir.clone();
    season.core.note = "Rewatch before season two".into();
    let season_one = lab.add(season);

    let mut ids = Vec::new();
    let numbered = [
        (1_u32, 1_u32, "Good News About Hell", "S01E01.mkv", 1080),
        (1, 2, "Half Loop", "S01E02.mkv", 1080),
        (1, 3, "In Perpetuity", "S01E03.mkv", 2160),
        (1, 3, "In Perpetuity (repack)", "S01E03.REPACK.mkv", 1080),
        (1, 4, "The You You Are", "S01E04.mkv", 1080),
    ];
    for (season, number, title, file, height) in numbered {
        let mut episode = Item::episode(show_id, title);
        episode.core.plot = format!("Plot of {title}.");
        episode
            .core
            .media_files
            .push(video(&season_dir, file, height));
        if number == 1 {
            episode.core.media_files.push(MediaFile::new(
                MediaFileType::Thumb,
                season_dir.clone(),
                "S01E01-thumb.jpg",
            ));
            episode.core.credits.extend([
                Credit::new(CreditKind::Guest, "Michael Chernus", "Ricken"),
                Credit::new(CreditKind::Director, "Ben Stiller", ""),
                Credit::new(CreditKind::Writer, "Dan Erickson", "Teleplay"),
            ]);
        }
        if number == 2 {
            episode
                .core
                .credits
                .push(Credit::new(CreditKind::Guest, "Michael Chernus", ""));
        }
        if let ItemDetail::Episode(e) = &mut episode.detail {
            e.numbers.set(EpisodeNumber::aired(season, number));
            e.first_aired = date(2022, 2, 18);
            e.runtime_minutes = 50;
            e.watched = number == 1;
        }
        ids.push(lab.add(episode));
    }

    let mut special = Item::episode(show_id, "Behind the Scenes");
    special
        .core
        .media_files
        .push(video(&dir, "special.mkv", 720));
    if let ItemDetail::Episode(e) = &mut special.detail {
        e.numbers.set(EpisodeNumber::aired(0, 1));
    }
    let special = lab.add(special);

    let mut loose = Item::episode(show_id, "Some Unnumbered Clip");
    loose.core.media_files.push(video(&dir, "clip.mkv", 720));
    let loose = lab.add(loose);

    Fixture {
        show: show_id,
        season_one,
        episodes: ids,
        special,
        loose,
    }
}

#[test]
fn today_is_computed_from_epoch_milliseconds() {
    let at = |millis| date_of_millis(millis).to_string();
    assert_eq!(at(0), "1970-01-01");
    assert_eq!(at(951_782_400_000), "2000-02-29");
    assert_eq!(at(1_700_000_000_000), "2023-11-14");
    assert_eq!(at(1_700_000_000_000 - 86_400_000 * 14), "2023-10-31");
    assert_eq!(at(-1), "1969-12-31");
    assert!(today_utc() >= CalendarDate::new(2024, 1, 1).unwrap());
}

#[test]
fn the_show_page_gathers_seasons_episodes_and_counts() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let detail = lab.reader().show(fx.show).unwrap();

    assert_eq!(detail.title, "Severance");
    assert_eq!(detail.status, "continuing");
    assert_eq!(detail.ordering, "aired");
    assert_eq!(detail.studios, ["Apple TV+"]);
    assert_eq!(detail.first_aired.as_deref(), Some("2022-02-18"));
    assert_eq!(detail.genres, ["Science Fiction"]);
    assert_eq!(
        detail.credits.iter().filter(|c| c.kind == "actor").count(),
        1
    );
    assert!(detail.backdrop.is_some(), "the fanart file is hashed");
    assert!(detail.logo.is_none());

    let numbers: Vec<u32> = detail.seasons.iter().map(|s| s.number).collect();
    assert_eq!(
        numbers,
        [0, 1, 2],
        "specials, the season record and a missing season"
    );
    let one = &detail.seasons[1];
    assert_eq!(one.id, Some(fx.season_one.0.to_string()));
    assert_eq!(one.name, "The Innies");
    assert_eq!(one.overview, "The first season.");
    assert_eq!(one.note, "Rewatch before season two");
    // Five files, four episodes: the REPACK of S01E03 is a duplicate, not a fifth episode.
    assert_eq!(
        (one.episodes, one.watched, one.missing, one.unaired),
        (4, 1, 1, 0)
    );
    assert_eq!(one.first_aired.as_deref(), Some("2022-02-18"));
    assert_eq!(one.artwork.len(), 4);
    assert!(one.poster.is_none());
    let two = &detail.seasons[2];
    assert_eq!(two.id, None);
    assert_eq!((two.episodes, two.missing, two.unaired), (0, 0, 1));
    assert_eq!(detail.seasons[0].episodes, 1);

    let order: Vec<(u32, u32)> = detail
        .episodes
        .iter()
        .map(|e| (e.season, e.number))
        .collect();
    assert_eq!(order, [(0, 1), (1, 1), (1, 2), (1, 3), (1, 3), (1, 4)]);
    let first = &detail.episodes[1];
    assert!(first.watched && first.thumb.is_some());
    assert_eq!(first.resolution, "1080p");
    assert_eq!(first.file_name, "S01E01.mkv");
    assert_eq!(detail.watched_episodes, 1);
    assert!(!detail.watched);

    // The show's own files, theme music among them.
    let files: Vec<(&str, &str)> = detail
        .files
        .iter()
        .map(|f| (f.kind.as_str(), f.name.as_str()))
        .collect();
    assert!(files.contains(&("theme", "theme.mp3")), "{files:?}");
}

#[test]
fn a_duplicate_or_unnumbered_file_adds_no_new_or_watched_episode() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let mut all: Vec<ItemId> = fx.episodes.clone();
    all.extend([fx.special, fx.loose]);
    lab.db
        .write(move |tx| cv_db::repo::flags::set_new(tx, &all, true))
        .unwrap();
    // Both files of S01E03 watched: still one watched episode for that number.
    for id in [fx.episodes[2], fx.episodes[3]] {
        let mut episode = lab.item(id);
        if let ItemDetail::Episode(e) = &mut episode.detail {
            e.watched = true;
        }
        lab.db.write(move |tx| update_item(tx, &episode)).unwrap();
    }

    let detail = lab.reader().show(fx.show).unwrap();

    // Seven files: four numbered episodes in season 1 (the REPACK is a second file of S01E03),
    // one special and one clip without a number.
    assert_eq!(detail.new_episodes, 5);
    assert_eq!(detail.watched_episodes, 2);
    let seasons: u32 = detail.seasons.iter().map(|s| s.new_episodes).sum();
    assert_eq!(detail.new_episodes, seasons);

    // The TV table's Episodes column counts the same way.
    let facts = lab
        .db
        .read(move |c| {
            cv_db::repo::table_facts::table_facts_for(
                c,
                &[fx.show],
                &cv_domain::RatingPreference::default(),
            )
        })
        .unwrap();
    assert_eq!(facts[&fx.show].episodes, 5);
}

#[test]
fn missing_episodes_are_marked_unaired_by_their_date() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let detail = lab.reader().show(fx.show).unwrap();

    assert_eq!(detail.missing.len(), 2);
    let aired = &detail.missing[0];
    assert_eq!((aired.season, aired.number), (1, 6));
    assert!(!aired.unaired);
    let upcoming = &detail.missing[1];
    assert_eq!((upcoming.season, upcoming.number), (2, 1));
    assert!(upcoming.unaired);
    assert_eq!(upcoming.first_aired.as_deref(), Some("2099-01-17"));
}

#[test]
fn two_files_for_one_episode_are_flagged_and_unnumbered_ones_listed() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let detail = lab.reader().show(fx.show).unwrap();

    let duplicates: Vec<&str> = detail
        .episodes
        .iter()
        .filter(|e| e.duplicate)
        .map(|e| e.file_name.as_str())
        .collect();
    assert_eq!(duplicates, ["S01E03.mkv", "S01E03.REPACK.mkv"]);
    assert_eq!(detail.duplicate_episodes, 1, "one number is claimed twice");

    assert_eq!(detail.uncategorised.len(), 1);
    let loose = &detail.uncategorised[0];
    assert_eq!(loose.id, fx.loose.0.to_string());
    assert_eq!(loose.file_name, "clip.mkv");
    assert!(
        detail.episodes.iter().all(|e| e.id != loose.id),
        "an unnumbered episode is in no season"
    );
}

#[test]
fn an_episode_whose_file_is_gone_makes_no_duplicate() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let repack = fx.episodes[3];
    lab.db
        .write(move |conn| {
            cv_db::repo::set_availability(
                conn,
                repack,
                cv_domain::Availability::Missing,
                "the file is gone",
            )
        })
        .unwrap();

    let detail = lab.reader().show(fx.show).unwrap();
    assert_eq!(detail.duplicate_episodes, 0, "only online files can clash");
    assert!(detail.episodes.iter().all(|e| !e.duplicate));
    assert_eq!(
        detail.episodes.len(),
        6,
        "the missing episode is still listed"
    );

    let reader = lab.reader();
    assert!(!reader.episode(repack).unwrap().duplicate);
    assert!(!reader.episode(fx.episodes[2]).unwrap().duplicate);
}

#[test]
fn guests_are_counted_over_the_episodes_and_the_badges_over_the_files() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let detail = lab.reader().show(fx.show).unwrap();

    assert_eq!(detail.guests.len(), 1);
    let guest = &detail.guests[0];
    assert_eq!(guest.name, "Michael Chernus");
    assert_eq!(guest.role, "Ricken");
    assert_eq!(guest.episodes, 2);

    let find = |facet: &str, label: &str| {
        detail
            .tech
            .iter()
            .find(|b| b.facet == facet && b.label == label)
    };
    // The most common comes first: 1080p (four files), 720p (two), 2160p (one).
    let resolutions: Vec<&str> = detail
        .tech
        .iter()
        .filter(|b| b.facet == "video_format")
        .map(|b| b.label.as_str())
        .collect();
    assert_eq!(resolutions, ["1080p", "720p", "2160p"]);
    assert_eq!(
        find("video_codec", "HEVC").map(|b| b.values.clone()),
        Some(vec!["HEVC".to_owned()])
    );
    let audio = find("audio_codec", "E-AC-3 5.1").expect("audio badge");
    assert_eq!(audio.values, ["E-AC-3"]);
}

#[test]
fn the_hero_audio_of_an_episode_follows_the_original_language_of_its_show() {
    let lab = Lab::new();
    let folder = lab.dir.path().join("Anime");
    std::fs::create_dir_all(&folder).unwrap();
    let dir = folder.to_string_lossy().into_owned();
    let mut show = Item::show("Anime");
    show.core.path = dir.clone();
    show.core.original_language = "ja".into();
    let show_id = lab.add(show);
    let mut episode = Item::episode(show_id, "Pilot");
    episode.core.path = dir.clone();
    let mut file = video(&dir, "S01E01.mkv", 1080);
    file.audio_streams = vec![
        AudioStream {
            codec: "AC3".into(),
            language: "eng".into(),
            channels: 2,
            flags: [StreamFlag::Default, StreamFlag::Dub].into_iter().collect(),
            ..AudioStream::default()
        },
        AudioStream {
            codec: "FLAC".into(),
            language: "jpn".into(),
            channels: 2,
            ..AudioStream::default()
        },
    ];
    episode.core.media_files.push(file);
    let episode_id = lab.add(episode);

    let reader = lab.reader();
    let detail = reader.episode(episode_id).unwrap();
    // The episode has no language of its own: it reads its show's.
    assert_eq!(detail.original_language, "ja");
    let hero = detail.hero_audio.expect("a hero track");
    assert_eq!(
        (hero.index, hero.codec.as_str(), hero.language.as_str()),
        (1, "FLAC", "jpn")
    );
    assert!(
        detail
            .tech
            .iter()
            .any(|b| b.facet == "audio_codec" && b.values == ["FLAC"])
    );

    let show = reader.show(show_id).unwrap();
    assert_eq!(show.original_language, "ja");
    assert!(
        show.tech
            .iter()
            .any(|b| b.facet == "audio_codec" && b.values == ["FLAC"])
    );
}

#[test]
fn the_show_ordering_decides_the_numbers_shown() {
    let lab = Lab::new();
    let fx = severance(&lab);
    // Episode 2 is episode 5 of season 2 in DVD order, and the show follows DVD order.
    let mut show = lab.item(fx.show);
    if let ItemDetail::Show(fields) = &mut show.detail {
        fields.episode_group = EpisodeGroup::dvd();
    }
    lab.db.write(move |tx| update_item(tx, &show)).unwrap();
    let mut second = lab.item(fx.episodes[1]);
    if let ItemDetail::Episode(e) = &mut second.detail {
        e.numbers.set(EpisodeNumber::new(EpisodeGroup::dvd(), 2, 5));
    }
    lab.db.write(move |tx| update_item(tx, &second)).unwrap();

    let detail = lab.reader().show(fx.show).unwrap();
    assert_eq!(detail.ordering, "dvd");
    let moved = detail
        .episodes
        .iter()
        .find(|e| e.id == fx.episodes[1].0.to_string())
        .unwrap();
    assert_eq!((moved.season, moved.number), (2, 5));
    // The others have no DVD number and keep their aired one.
    let other = detail
        .episodes
        .iter()
        .find(|e| e.id == fx.episodes[0].0.to_string())
        .unwrap();
    assert_eq!((other.season, other.number), (1, 1));
}

#[test]
fn the_episode_page_gathers_numbers_credits_files_and_neighbours() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let reader = lab.reader();
    let detail = reader.episode(fx.episodes[1]).unwrap();

    assert_eq!(detail.title, "Half Loop");
    assert_eq!(detail.show_title, "Severance");
    assert_eq!(detail.show_id, fx.show.0.to_string());
    assert_eq!(detail.season_id, Some(fx.season_one.0.to_string()));
    assert_eq!((detail.season, detail.number), (1, 2));
    assert_eq!(detail.numbers.len(), 1);
    assert_eq!(detail.numbers[0].ordering, "aired");
    assert!(!detail.duplicate && !detail.uncategorised);
    assert_eq!(detail.videos.len(), 1);
    assert_eq!(detail.videos[0].name, "S01E02.mkv");
    assert_eq!(detail.guests.len(), 1);
    assert!(detail.show_backdrop.is_some());
    assert_eq!(detail.previous.as_ref().map(|p| p.number), Some(1));
    let next = detail.next.expect("a next episode");
    assert_eq!((next.season, next.number), (1, 3));

    let first = reader.episode(fx.episodes[0]).unwrap();
    assert_eq!(first.directors, ["Ben Stiller"]);
    assert_eq!(first.writers, ["Dan Erickson"]);
    assert_eq!(first.guests[0].role, "Ricken");
    assert!(first.credits.iter().all(|c| c.kind != "guest"));
    assert!(first.thumb.is_some());
    assert_eq!(first.artwork[0].slot, "thumb");
    assert_eq!(
        first.previous.as_ref().map(|p| (p.season, p.number)),
        Some((0, 1))
    );

    let repack = reader.episode(fx.episodes[3]).unwrap();
    assert!(repack.duplicate);
}

#[test]
fn an_unnumbered_episode_says_so_and_has_no_neighbours() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let detail = lab.reader().episode(fx.loose).unwrap();
    assert!(detail.uncategorised);
    assert!(detail.previous.is_none() && detail.next.is_none());
    assert!(detail.season_id.is_none());
}

#[test]
fn unknown_ids_and_the_wrong_kind_are_refused() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let reader = lab.reader();
    assert_eq!(reader.show(ItemId(9999)).unwrap_err().code, "not_found");
    assert_eq!(reader.show(fx.episodes[0]).unwrap_err().code, "not_found");
    assert_eq!(reader.episode(fx.show).unwrap_err().code, "not_found");
    assert_eq!(reader.episode(ItemId(9999)).unwrap_err().code, "not_found");
    let movie = lab.add(Item::movie("Heat"));
    assert_eq!(reader.show(movie).unwrap_err().code, "not_found");
}

#[test]
fn marking_episodes_watched_changes_only_unwatched_episodes_and_can_be_undone() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let reader = lab.reader();
    let nfo = std::sync::Arc::new(cv_app::nfo::NfoService::new(
        lab.db.clone(),
        lab.dir.path().join("undo"),
    ));
    let watch = cv_app::curation::WatchService::new(lab.db.clone(), nfo);
    let settings = cv_app::nfo::NfoSettings::default();
    let ids = vec![fx.episodes[0], fx.episodes[1], fx.special];

    // Episode 1 is watched already.
    assert_eq!(
        watch.set_watched(&ids, true, &settings).unwrap().changed(),
        2
    );
    let after = reader.show(fx.show).unwrap();
    assert_eq!(after.watched_episodes, 3);
    assert_eq!(
        watch.set_watched(&ids, true, &settings).unwrap().changed(),
        0
    );
    let watched = reader.episode(fx.episodes[1]).unwrap();
    assert!(watched.watched);
    assert_eq!(watched.play_count, 1);

    assert_eq!(
        watch.set_watched(&ids, false, &settings).unwrap().changed(),
        3
    );
    assert_eq!(reader.show(fx.show).unwrap().watched_episodes, 0);
    assert_eq!(reader.episode(fx.episodes[1]).unwrap().play_count, 0);

    // A show whose every episode is watched is watched.
    let all: Vec<ItemId> = fx
        .episodes
        .iter()
        .copied()
        .chain([fx.special, fx.loose])
        .collect();
    watch.set_watched(&all, true, &settings).unwrap();
    assert!(reader.show(fx.show).unwrap().watched);
}

#[test]
fn a_running_show_has_last_and_next_air_dates_and_no_end() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let detail = lab.reader().show(fx.show).unwrap();

    assert_eq!(
        detail.last_aired.as_deref(),
        Some("2022-04-01"),
        "a missing episode counts"
    );
    assert_eq!(detail.next_aired.as_deref(), Some("2099-01-17"));
    assert_eq!(detail.end_date, None);
    assert_eq!(detail.year_end, 0);
}

#[test]
fn an_ended_show_reports_its_end_date_and_year() {
    let lab = Lab::new();
    let fx = severance(&lab);
    let mut show = lab.item(fx.show);
    if let ItemDetail::Show(fields) = &mut show.detail {
        fields.status = AiredStatus::Ended;
        fields.missing_episodes.pop();
    }
    lab.db.write(move |conn| update_item(conn, &show)).unwrap();

    let detail = lab.reader().show(fx.show).unwrap();
    assert_eq!(detail.status, "ended");
    assert_eq!(detail.end_date.as_deref(), Some("2022-04-01"));
    assert_eq!(detail.year_end, 2022);
    assert_eq!(detail.next_aired, None);
}

/// Gives the show a poster file, and season one a poster of its own when `own` is set.
fn with_posters(lab: &Lab, fx: &Fixture, own: bool) {
    let dir = lab.dir.path().join("Severance");
    std::fs::write(dir.join("poster.jpg"), b"show poster bytes").unwrap();
    let dir_text = dir.to_string_lossy().into_owned();
    let mut show = lab.item(fx.show);
    show.core.media_files.push(MediaFile::new(
        MediaFileType::Poster,
        dir_text.clone(),
        "poster.jpg",
    ));
    lab.db.write(move |conn| update_item(conn, &show)).unwrap();
    if own {
        std::fs::write(dir.join("season01-poster.jpg"), b"season poster bytes").unwrap();
        let mut season = lab.item(fx.season_one);
        season.core.media_files.push(MediaFile::new(
            MediaFileType::SeasonPoster,
            dir_text,
            "season01-poster.jpg",
        ));
        lab.db
            .write(move |conn| update_item(conn, &season))
            .unwrap();
    }
}

#[test]
fn a_season_without_artwork_shows_the_show_poster_unless_switched_off() {
    let lab = Lab::new();
    let fx = severance(&lab);
    with_posters(&lab, &fx, false);

    let detail = lab.reader().show(fx.show).unwrap();
    let show_poster = detail.poster.clone().expect("the show poster");
    for season in &detail.seasons {
        assert_eq!(
            season.poster.as_ref(),
            Some(&show_poster),
            "season {}",
            season.number
        );
        assert!(season.poster_from_show);
    }
    // The show has fanart; the season has none of its own either.
    assert_eq!(detail.seasons[1].fanart, detail.backdrop);

    let off = lab
        .reader()
        .with_season_art_fallback(false)
        .show(fx.show)
        .unwrap();
    assert!(
        off.seasons
            .iter()
            .all(|s| s.poster.is_none() && !s.poster_from_show)
    );
    assert!(off.seasons.iter().all(|s| s.fanart.is_none()));
}

#[test]
fn a_season_with_its_own_poster_keeps_it() {
    let lab = Lab::new();
    let fx = severance(&lab);
    with_posters(&lab, &fx, true);

    let detail = lab.reader().show(fx.show).unwrap();
    let one = &detail.seasons[1];
    let own = one.poster.clone().expect("the season poster");
    assert!(!one.poster_from_show);
    assert_ne!(Some(&own), detail.poster.as_ref(), "not the show poster");
    // Seasons without one still fall back.
    assert!(detail.seasons[2].poster_from_show);
}

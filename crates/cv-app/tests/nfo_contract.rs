//! The NFO completeness contract.
//!
//! * **Every stored field is classified.** The `*_contract` functions take each library struct
//!   apart without `..`, so a field added to the domain model does not compile until it is given
//!   its NFO element here or named as library-only state.
//! * **Round trip.** A title with every written field filled is written through the Rewrite NFO
//!   path, the file is read back with cv-nfo's reader, and each field is compared with the library
//!   item, including the stream details of the video, its audio and subtitle streams and the
//!   subtitle files next to it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_app::nfo::{NfoScope, NfoService, NfoSettings};
use cv_db::Database;
use cv_db::repo::{get_item, insert_item};
use cv_domain::{
    AiredStatus, AudioStream, CalendarDate, Certification, Credit, CreditKind, Edition,
    EpisodeFields, EpisodeGroup, EpisodeNumber, EpisodeNumbers, EpisodeOrdering, Genre, Item,
    ItemCore, ItemDetail, ItemId, MediaFile, MediaFileType, MediaSource, MovieFields, Person,
    Rating, SeasonFields, SeasonText, SetFields, ShowFields, StreamFlag, SubtitleStream, Trailer,
    Video3dFormat,
};
use cv_nfo::{Element, Node, parse_roots, read_episode_nfo, read_movie_nfo, read_show_nfo};

// ---- the classification ------------------------------------------------------------------------

/// Where a stored field goes. `Nfo` names the element (path below the root); `Library` says why
/// the field is the library's own state and not part of the title's metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Goes {
    Nfo(&'static str),
    Library(&'static str),
}

use Goes::{Library, Nfo};

fn core_contract(core: &ItemCore) -> Vec<(&'static str, Goes)> {
    let ItemCore {
        locked: _,
        data_source: _,
        ids: _,
        title: _,
        original_title: _,
        original_language: _,
        year: _,
        plot: _,
        outline: _,
        path: _,
        date_added_ms: _,
        production_companies: _,
        note: _,
        ratings: _,
        media_files: _,
        tags: _,
        artwork_urls: _,
        original_filename: _,
        last_scraper_id: _,
        last_scrape_language: _,
        rename_history: _,
        credits: _,
    } = core;
    vec![
        (
            "locked",
            Library("the whole title is locked against changes; field locks go to cv_locked"),
        ),
        (
            "data_source",
            Library("the library folder the title was found in"),
        ),
        ("ids", Nfo("uniqueid*, id, tmdbid")),
        ("title", Nfo("title")),
        ("original_title", Nfo("originaltitle")),
        ("original_language", Nfo("original_language")),
        ("year", Nfo("year")),
        ("plot", Nfo("plot")),
        ("outline", Nfo("outline")),
        ("path", Library("the folder on disk; the NFO lives in it")),
        ("date_added_ms", Nfo("dateadded")),
        ("production_companies", Nfo("studio*")),
        ("note", Nfo("user_note")),
        ("ratings", Nfo("ratings/rating*, userrating")),
        ("media_files", Nfo("fileinfo/streamdetails")),
        ("tags", Nfo("tag*")),
        ("artwork_urls", Nfo("thumb[aspect]*, fanart/thumb*")),
        ("original_filename", Nfo("original_filename")),
        (
            "last_scraper_id",
            Library("which provider scraped last; the episode guide uses it"),
        ),
        (
            "last_scrape_language",
            Library("the language of the last scrape"),
        ),
        (
            "rename_history",
            Library("the undo record of the last rename"),
        ),
        (
            "credits",
            Nfo("actor*, director*, credits*, producer*, crew*"),
        ),
    ]
}

fn movie_contract(m: &MovieFields) -> Vec<(&'static str, Goes)> {
    let MovieFields {
        english_title: _,
        sort_title: _,
        tagline: _,
        runtime_minutes: _,
        watched: _,
        play_count: _,
        last_watched_ms: _,
        is_disc: _,
        spoken_languages: _,
        countries: _,
        release_date: _,
        multi_movie_dir: _,
        top250: _,
        media_source: _,
        video_in_3d: _,
        certification: _,
        edition: _,
        stacked: _,
        genres: _,
        extra_thumbs: _,
        extra_fanarts: _,
        trailers: _,
        show_links: _,
    } = m;
    vec![
        ("english_title", Nfo("english_title")),
        ("sort_title", Nfo("sorttitle")),
        ("tagline", Nfo("tagline")),
        ("runtime_minutes", Nfo("runtime")),
        ("watched", Nfo("watched")),
        ("play_count", Nfo("playcount")),
        ("last_watched_ms", Nfo("lastplayed")),
        (
            "is_disc",
            Library("the disc layout of the files; fileinfo reads the main title"),
        ),
        ("spoken_languages", Nfo("languages")),
        ("countries", Nfo("country*")),
        ("release_date", Nfo("premiered")),
        (
            "multi_movie_dir",
            Library("the folder layout; decides the NFO file name"),
        ),
        ("top250", Nfo("top250 (when ranked)")),
        ("media_source", Nfo("source")),
        ("video_in_3d", Nfo("video3d")),
        ("certification", Nfo("mpaa, certification")),
        ("edition", Nfo("edition, videoassettitle")),
        (
            "stacked",
            Library("the parts layout; fileinfo adds the parts up"),
        ),
        ("genres", Nfo("genre*")),
        ("extra_thumbs", Nfo("extrathumbs/thumb*")),
        ("extra_fanarts", Nfo("fanart/thumb* after the first")),
        ("trailers", Nfo("trailer (the one marked for the NFO)")),
        ("show_links", Nfo("showlink*")),
    ]
}

fn set_contract(s: &SetFields) -> Vec<(&'static str, Goes)> {
    let SetFields {
        english_title: _,
        sort_title: _,
        missing_movies: _,
    } = s;
    vec![
        (
            "english_title",
            Nfo("collection NFO english_title; movie NFO set/name"),
        ),
        ("sort_title", Nfo("collection NFO sorttitle")),
        (
            "missing_movies",
            Library("derived from the provider's collection list"),
        ),
    ]
}

fn show_contract(s: &ShowFields) -> Vec<(&'static str, Goes)> {
    let ShowFields {
        english_title: _,
        sort_title: _,
        tagline: _,
        runtime_minutes: _,
        year_end: _,
        first_aired: _,
        status: _,
        country: _,
        certification: _,
        episode_group: _,
        episode_groups: _,
        genres: _,
        top250: _,
        extra_fanart_urls: _,
        trailers: _,
        season_texts: _,
        missing_episodes: _,
        episode_list_fetched: _,
    } = s;
    vec![
        ("english_title", Nfo("english_title")),
        ("sort_title", Nfo("sorttitle")),
        ("tagline", Nfo("tagline")),
        ("runtime_minutes", Nfo("runtime")),
        ("year_end", Nfo("year_end")),
        ("first_aired", Nfo("premiered")),
        ("status", Nfo("status")),
        ("country", Nfo("country*")),
        ("certification", Nfo("mpaa, certification")),
        ("episode_group", Nfo("episode_groups (the active ordering)")),
        ("episode_groups", Nfo("episode_groups/group*")),
        ("genres", Nfo("genre*")),
        ("top250", Nfo("top250 (when ranked)")),
        ("extra_fanart_urls", Nfo("fanart/thumb* after the first")),
        ("trailers", Nfo("trailer (the one marked for the NFO)")),
        (
            "season_texts",
            Nfo("namedseason*, seasonplot*; season NFO title and plot"),
        ),
        (
            "missing_episodes",
            Library("derived from the provider's episode list"),
        ),
        (
            "episode_list_fetched",
            Library("whether that list was fetched"),
        ),
    ]
}

fn season_contract(s: &SeasonFields) -> Vec<(&'static str, Goes)> {
    let SeasonFields { season: _ } = s;
    vec![("season", Nfo("season NFO seasonnumber"))]
}

fn episode_contract(e: &EpisodeFields) -> Vec<(&'static str, Goes)> {
    let EpisodeFields {
        english_title: _,
        numbers: _,
        first_aired: _,
        is_disc: _,
        multi_episode: _,
        watched: _,
        play_count: _,
        last_watched_ms: _,
        media_source: _,
        edition: _,
        stacked: _,
        runtime_minutes: _,
    } = e;
    vec![
        ("english_title", Nfo("english_title")),
        (
            "numbers",
            Nfo("season, episode, displayseason, displayepisode, episode_groups"),
        ),
        ("first_aired", Nfo("aired, premiered")),
        ("is_disc", Library("the disc layout of the files")),
        (
            "multi_episode",
            Library("one file, several episodes: one episodedetails each"),
        ),
        ("watched", Nfo("watched")),
        ("play_count", Nfo("playcount")),
        ("last_watched_ms", Nfo("lastplayed")),
        ("media_source", Nfo("source")),
        ("edition", Nfo("edition, videoassettitle")),
        (
            "stacked",
            Library("the parts layout; fileinfo adds the parts up"),
        ),
        ("runtime_minutes", Nfo("runtime")),
    ]
}

fn media_file_contract(f: &MediaFile) -> Vec<(&'static str, Goes)> {
    let MediaFile {
        kind: _,
        directory: _,
        filename: _,
        size_bytes: _,
        modified_ms: _,
        video_codec: _,
        codec_id: _,
        container_format: _,
        exact_video_format: _,
        video_3d: _,
        width: _,
        height: _,
        aspect_ratio: _,
        aspect_ratio2: _,
        pixel_aspect_ratio: _,
        video_bitrate_kbps: _,
        overall_bitrate_kbps: _,
        bit_depth: _,
        frame_rate: _,
        duration_secs: _,
        stacking: _,
        stacking_marker: _,
        title: _,
        is_iso: _,
        is_animated_graphic: _,
        hdr_formats: _,
        hdr_details: _,
        checksums: _,
        audio_streams: _,
        subtitles: _,
    } = f;
    vec![
        (
            "kind",
            Library("what the file is; video, audio and subtitle files feed fileinfo"),
        ),
        ("directory", Library("where the file is")),
        (
            "filename",
            Library("the file name; the NFO is named after the video"),
        ),
        ("size_bytes", Nfo("fileinfo/streamdetails/video/filesize")),
        ("modified_ms", Library("change detection for rescans")),
        ("video_codec", Nfo("video/codec")),
        (
            "codec_id",
            Library("the raw codec id; video/codec holds the codec"),
        ),
        ("container_format", Nfo("video/container")),
        (
            "exact_video_format",
            Library("the resolution label; video/width and height hold it"),
        ),
        ("video_3d", Nfo("video/stereomode")),
        ("width", Nfo("video/width")),
        ("height", Nfo("video/height")),
        ("aspect_ratio", Nfo("video/aspect")),
        (
            "aspect_ratio2",
            Library("a second ratio of multi-format videos; aspect holds the main"),
        ),
        (
            "pixel_aspect_ratio",
            Library("folded into the display aspect ratio"),
        ),
        ("video_bitrate_kbps", Nfo("video/bitrate")),
        ("overall_bitrate_kbps", Nfo("video/overallbitrate")),
        ("bit_depth", Nfo("video/bitdepth")),
        ("frame_rate", Nfo("video/framerate")),
        ("duration_secs", Nfo("video/durationinseconds")),
        (
            "stacking",
            Library("the part number; fileinfo adds the parts up"),
        ),
        (
            "stacking_marker",
            Library("the part marker in the file name"),
        ),
        ("title", Library("the container's title tag")),
        ("is_iso", Library("the file is a disc image")),
        (
            "is_animated_graphic",
            Library("an artwork file is animated"),
        ),
        ("hdr_formats", Nfo("video/hdrtype, video/hdrformats")),
        (
            "hdr_details",
            Library(
                "Dolby Vision profile, mastering display and light levels for the Technical card; the NFO carries the format names",
            ),
        ),
        ("checksums", Nfo("crc32")),
        ("audio_streams", Nfo("audio*")),
        ("subtitles", Nfo("subtitle*")),
    ]
}

fn audio_contract(a: &AudioStream) -> Vec<(&'static str, Goes)> {
    let AudioStream {
        codec: _,
        codec_id: _,
        language: _,
        title: _,
        flags: _,
        channels: _,
        bitrate_kbps: _,
        bit_depth: _,
        sample_rate_hz: _,
    } = a;
    vec![
        ("codec", Nfo("audio/codec")),
        (
            "codec_id",
            Library("the raw codec id; audio/codec holds the codec"),
        ),
        ("language", Nfo("audio/language")),
        ("title", Nfo("audio/title")),
        ("flags", Nfo("audio/default, forced, commentary, ...")),
        ("channels", Nfo("audio/channels")),
        ("bitrate_kbps", Nfo("audio/bitrate")),
        ("bit_depth", Nfo("audio/bitdepth")),
        ("sample_rate_hz", Nfo("audio/samplerate")),
    ]
}

fn subtitle_contract(s: &SubtitleStream) -> Vec<(&'static str, Goes)> {
    let SubtitleStream {
        codec: _,
        codec_id: _,
        language: _,
        title: _,
        flags: _,
    } = s;
    vec![
        ("codec", Nfo("subtitle/codec")),
        (
            "codec_id",
            Library("the raw codec id; subtitle/codec holds the format"),
        ),
        ("language", Nfo("subtitle/language")),
        ("title", Nfo("subtitle/title")),
        ("flags", Nfo("subtitle/forced, hearingimpaired, ...")),
    ]
}

/// Every classification names a reason or an element: a field cannot be added to the model
/// without a decision about the NFO.
#[test]
fn every_stored_field_has_an_nfo_element_or_a_reason() {
    let item = Item::movie("x");
    let ItemDetail::Movie(m) = &item.detail else {
        unreachable!()
    };
    let tables: Vec<(&str, Vec<(&'static str, Goes)>)> = vec![
        ("ItemCore", core_contract(&item.core)),
        ("MovieFields", movie_contract(m)),
        ("SetFields", set_contract(&SetFields::default())),
        ("ShowFields", show_contract(&ShowFields::default())),
        ("SeasonFields", season_contract(&SeasonFields::default())),
        ("EpisodeFields", episode_contract(&EpisodeFields::default())),
        ("MediaFile", media_file_contract(&MediaFile::default())),
        ("AudioStream", audio_contract(&AudioStream::default())),
        (
            "SubtitleStream",
            subtitle_contract(&SubtitleStream::default()),
        ),
    ];
    for (table, fields) in tables {
        for (field, goes) in fields {
            match goes {
                Nfo(element) | Library(element) => assert!(
                    !element.trim().is_empty(),
                    "{table}.{field} names no element or reason"
                ),
            }
        }
    }
}

// ---- the round trip ----------------------------------------------------------------------------

struct Lab {
    root: tempfile::TempDir,
    db: Arc<Database>,
    service: NfoService,
}

impl Lab {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(root.path().join("library.db")).unwrap());
        let service = NfoService::new(Arc::clone(&db), root.path().join("undo"));
        Self { root, db, service }
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.root.path().join("media").join(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn insert(&self, item: Item) -> ItemId {
        self.db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    fn item(&self, id: ItemId) -> Item {
        self.db.read(|c| get_item(c, id)).unwrap().unwrap()
    }

    fn rewrite(&self, id: ItemId) {
        let report = self
            .service
            .rewrite(&NfoScope::Items(vec![id]), &NfoSettings::default())
            .unwrap();
        assert_eq!(report.totals.failed, 0, "{report:?}");
    }
}

fn person(kind: CreditKind, name: &str, role: &str, tmdb: &str) -> Credit {
    let mut credit = Credit::new(kind, name, role);
    credit.person = Person {
        name: name.to_owned(),
        thumb_url: format!("https://img.test/p/{tmdb}.jpg"),
        profile_url: format!("https://www.themoviedb.org/person/{tmdb}"),
        ..Person::default()
    };
    credit.person.ids.set("tmdb", tmdb);
    credit
}

fn audio(codec: &str, language: &str, channels: u32, title: &str) -> AudioStream {
    AudioStream {
        codec: codec.into(),
        language: language.into(),
        channels,
        title: title.into(),
        bitrate_kbps: 4_500,
        bit_depth: 24,
        sample_rate_hz: 48_000,
        ..AudioStream::default()
    }
}

/// A movie with every field the NFO carries filled, in its own folder.
fn full_movie(lab: &Lab, set: ItemId) -> (ItemId, PathBuf) {
    let folder = lab.dir("Movies/13 Assassins (2010)");
    let video_name = "13 Assassins - Extended Edition (2010).mkv";
    fs::write(folder.join(video_name), b"video").unwrap();
    fs::write(
        folder.join("13 Assassins - Extended Edition (2010).eng.forced.srt"),
        b"1",
    )
    .unwrap();
    let dir = folder.to_string_lossy().into_owned();

    let mut item = Item::movie("13 Assassins");
    item.parent_id = Some(set);
    let c = &mut item.core;
    c.path = dir.clone();
    c.original_title = "十三人の刺客".into();
    c.original_language = "ja".into();
    c.year = 2010;
    c.plot = "A group of assassins comes together for a suicide mission.".into();
    c.outline = "Samurai against a sadistic lord.".into();
    c.date_added_ms = 1_700_000_000_000;
    c.production_companies = vec!["TOHO".into(), "Sedic International".into()];
    c.note = "Watch with the commentary.".into();
    c.original_filename = video_name.into();
    for (key, value) in [
        ("imdb", "tt1436045"),
        ("tmdb", "58857"),
        ("trakt", "41235"),
        ("wikidata", "Q186810"),
        ("tvdb", "1234"),
    ] {
        c.ids.set(key, value);
    }
    for rating in [
        Rating::new("imdb", 7.5, 72_444, 10),
        Rating::new("tmdb", 7.3, 1_332, 10),
        Rating::new("metacritic", 84.0, 33, 100),
        Rating::new("tomatometerallcritics", 95.0, 129, 100),
        Rating::new("letterboxd", 3.9, 63_215, 5),
    ] {
        c.ratings.insert(rating);
    }
    c.ratings.set_user_rating(8.5).unwrap();
    for tag in ["samurai", "jidaigeki"] {
        c.tags.add(tag);
    }
    for (kind, url) in [
        (MediaFileType::Poster, "https://img.test/poster.jpg"),
        (MediaFileType::Fanart, "https://img.test/fanart.jpg"),
        (MediaFileType::Banner, "https://img.test/banner.jpg"),
        (MediaFileType::Clearart, "https://img.test/clearart.png"),
        (MediaFileType::Clearlogo, "https://img.test/clearlogo.png"),
        (MediaFileType::Disc, "https://img.test/disc.png"),
        (MediaFileType::Thumb, "https://img.test/landscape.jpg"),
        (MediaFileType::Keyart, "https://img.test/keyart.jpg"),
        (MediaFileType::Squareart, "https://img.test/square.jpg"),
    ] {
        c.artwork_urls.insert(kind, url.to_owned());
    }
    c.credits = vec![
        person(
            CreditKind::Actor,
            "Koji Yakusho",
            "Shinzaemon Shimada",
            "18056",
        ),
        person(CreditKind::Actor, "Meguru Kato", "", "150773"),
        person(CreditKind::Director, "Takashi Miike", "Director", "17282"),
        person(CreditKind::Writer, "Daisuke Tengan", "Screenplay", "68114"),
        person(
            CreditKind::Producer,
            "Jeremy Thomas",
            "Executive Producer",
            "3056",
        ),
        person(
            CreditKind::Composer,
            "Koji Endo",
            "Original Music Composer",
            "35633",
        ),
        person(CreditKind::Editor, "Kenji Yamashita", "Editor", "2195348"),
        person(
            CreditKind::Camera,
            "Nobuyasu Kita",
            "Director of Photography",
            "989305",
        ),
    ];

    let mut video = MediaFile::new(MediaFileType::Video, &dir, video_name);
    video.size_bytes = 32_000_000_000;
    video.video_codec = "HEVC".into();
    video.container_format = "Matroska".into();
    video.width = 3840;
    video.height = 2160;
    video.aspect_ratio = 2.39;
    video.video_bitrate_kbps = 58_000;
    video.overall_bitrate_kbps = 64_000;
    video.bit_depth = 10;
    video.frame_rate = 23.976;
    video.duration_secs = 8_511;
    video.video_3d = Video3dFormat::None;
    video.hdr_formats = vec!["Dolby Vision".into(), "HDR10".into()];
    video.checksums.insert("crc32".into(), "a1b2c3d4".into());
    let mut japanese = audio("TrueHD Atmos", "ja", 8, "Original");
    japanese.flags.insert(StreamFlag::Default);
    japanese.flags.insert(StreamFlag::Original);
    let mut commentary = audio("AC3", "en", 2, "Commentary");
    commentary.flags.insert(StreamFlag::Comment);
    video.audio_streams = vec![japanese, commentary];
    let mut english = SubtitleStream {
        codec: "PGS".into(),
        language: "en".into(),
        title: "English SDH".into(),
        ..SubtitleStream::default()
    };
    english.flags.insert(StreamFlag::HearingImpaired);
    video.subtitles = vec![english];
    c.media_files.push(video);
    // A sidecar the subtitle extraction wrote: no stream rows, the name says it all.
    c.media_files.push(MediaFile::new(
        MediaFileType::Subtitle,
        &dir,
        "13 Assassins - Extended Edition (2010).eng.forced.srt",
    ));

    if let ItemDetail::Movie(m) = &mut item.detail {
        m.english_title = "13 Assassins".into();
        m.sort_title = "Thirteen Assassins".into();
        m.tagline = "Take up your sword.".into();
        m.runtime_minutes = 141;
        m.watched = true;
        m.play_count = 3;
        m.last_watched_ms = Some(1_710_000_000_000);
        m.spoken_languages = vec!["ja".into()];
        m.countries = vec!["JP".into(), "GB".into()];
        m.release_date = CalendarDate::new(2010, 9, 25);
        m.top250 = 212;
        m.media_source = MediaSource::UhdBluray;
        m.video_in_3d = true;
        m.certification = Certification("US_R".into());
        m.edition = Edition::ExtendedEdition;
        m.genres = vec![Genre::Action, Genre::Drama];
        m.extra_thumbs = vec!["https://img.test/thumb1.jpg".into()];
        m.extra_fanarts = vec!["https://img.test/fanart2.jpg".into()];
        m.trailers = vec![
            Trailer {
                url: "https://www.youtube.com/watch?v=Xpm007vne54".into(),
                in_nfo: true,
                ..Trailer::default()
            },
            Trailer {
                url: "https://www.youtube.com/watch?v=other".into(),
                ..Trailer::default()
            },
        ];
        m.show_links = vec!["Samurai Gourmet".into()];
    }
    (lab.insert(item), folder)
}

fn nfo_text(folder: &Path, name: &str) -> String {
    fs::read_to_string(folder.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The direct child elements of `element` called `name`.
fn children<'a>(element: &'a Element, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
    element.children.iter().filter_map(move |n| match n {
        Node::Element(e) if e.name == name => Some(e),
        _ => None,
    })
}

fn child_text(element: &Element, name: &str) -> String {
    children(element, name)
        .next()
        .map(|e| e.text.clone())
        .unwrap_or_default()
}

#[test]
fn a_movie_nfo_carries_every_field_of_the_library_item() {
    let lab = Lab::new();
    let mut set = Item::set("Assassins Collection");
    set.core.plot = "Films about hired swords.".into();
    set.core.ids.set("tmdb", "777");
    let set_id = lab.insert(set);
    let (id, folder) = full_movie(&lab, set_id);
    lab.rewrite(id);
    let item = lab.item(id);
    let text = nfo_text(&folder, "13 Assassins - Extended Edition (2010).nfo");
    let nfo = read_movie_nfo(&text).expect("a movie NFO");
    let ItemDetail::Movie(m) = &item.detail else {
        unreachable!()
    };
    let c = &item.core;

    assert_eq!(nfo.title, c.title);
    assert_eq!(nfo.original_title, c.original_title);
    assert_eq!(nfo.original_language, c.original_language);
    assert_eq!(nfo.year, u32::from(c.year));
    assert_eq!(nfo.plot, c.plot);
    assert_eq!(nfo.outline, c.outline);
    assert_eq!(nfo.note, c.note);
    assert_eq!(nfo.studios, c.production_companies);
    assert_eq!(nfo.original_filename, c.original_filename);
    assert_eq!(
        nfo.date_added.as_deref(),
        Some(local(c.date_added_ms).as_str())
    );
    for (key, value) in c.ids.iter() {
        assert_eq!(nfo.ids.get(key), Some(value), "id {key}");
    }
    assert_eq!(
        nfo.ids.get("tmdbSet"),
        Some("777"),
        "the collection's TMDB id"
    );
    for rating in c.ratings.iter() {
        let read = nfo
            .ratings
            .get(&rating.id)
            .unwrap_or_else(|| panic!("rating {}", rating.id));
        assert!(
            (read.value - rating.value).abs() < 0.051,
            "rating {}",
            rating.id
        );
        if rating.id != "user" {
            assert_eq!(read.votes, rating.votes, "votes of {}", rating.id);
            assert_eq!(read.max_value, rating.max_value, "max of {}", rating.id);
        }
    }
    let tags: Vec<&str> = c.tags.iter().collect();
    assert_eq!(nfo.tags, tags);
    for (kind, url) in &c.artwork_urls {
        let read = match kind {
            MediaFileType::Poster => &nfo.artwork.poster,
            MediaFileType::Fanart => &nfo.artwork.fanart,
            MediaFileType::Banner => &nfo.artwork.banner,
            MediaFileType::Clearart => &nfo.artwork.clearart,
            MediaFileType::Clearlogo => &nfo.artwork.clearlogo,
            MediaFileType::Disc => &nfo.artwork.disc,
            MediaFileType::Thumb => &nfo.artwork.thumb,
            MediaFileType::Keyart => &nfo.artwork.keyart,
            MediaFileType::Squareart => &nfo.artwork.squareart,
            other => panic!("no NFO element for {other:?}"),
        };
        assert_eq!(read, url, "{kind:?}");
    }
    assert_eq!(nfo.artwork.extra_fanarts, m.extra_fanarts);
    assert_eq!(nfo.artwork.extra_thumbs, m.extra_thumbs);
    for credit in &c.credits {
        let read = nfo
            .credits
            .iter()
            .find(|r| r.kind == credit.kind && r.person.name == credit.person.name)
            .unwrap_or_else(|| panic!("credit {credit:?}"));
        assert_eq!(read.role, credit.role, "role of {}", credit.person.name);
        assert_eq!(read.person.ids.get("tmdb"), credit.person.ids.get("tmdb"));
        assert_eq!(
            read.person.thumb_url, credit.person.thumb_url,
            "{}",
            credit.person.name
        );
        assert_eq!(
            read.person.profile_url, credit.person.profile_url,
            "{}",
            credit.person.name
        );
    }
    assert_eq!(nfo.english_title, m.english_title);
    assert_eq!(nfo.sort_title, m.sort_title);
    assert_eq!(nfo.tagline, m.tagline);
    assert_eq!(nfo.runtime_minutes, m.runtime_minutes);
    assert_eq!(nfo.watched, m.watched);
    assert_eq!(nfo.play_count, m.play_count);
    assert_eq!(nfo.last_played, m.last_watched_ms.map(local));
    assert_eq!(nfo.spoken_languages, ["Japanese"]);
    assert_eq!(nfo.countries, ["Japan", "United Kingdom"]);
    assert_eq!(nfo.release_date, m.release_date);
    assert_eq!(nfo.top250, m.top250);
    assert_eq!(nfo.media_source, m.media_source);
    assert_eq!(nfo.video_in_3d, m.video_in_3d);
    assert_eq!(nfo.certification, Certification("US_R".into()));
    assert_eq!(nfo.edition, m.edition);
    assert_eq!(nfo.genres, m.genres);
    assert_eq!(nfo.trailers.len(), 1);
    assert_eq!(nfo.trailers[0].url, m.trailers[0].url);
    assert_eq!(nfo.show_links, m.show_links);
    let set = nfo.movie_set.expect("the collection");
    assert_eq!(set.title, "Assassins Collection");
    assert_eq!(set.overview, "Films about hired swords.");
    assert_eq!(nfo.crc32.as_deref(), Some("A1B2C3D4"));

    // The stream details.
    let roots = parse_roots(&text);
    let movie = &roots[0];
    let details = children(movie, "fileinfo")
        .next()
        .and_then(|f| children(f, "streamdetails").next())
        .expect("stream details");
    let video = children(details, "video").next().expect("video");
    let want_video = [
        ("codec", "hevc"),
        ("aspect", "2.40"),
        ("width", "3840"),
        ("height", "2160"),
        ("resolution", "4K"),
        ("hdrtype", "dolbyvision"),
        ("durationinseconds", "8511"),
        ("container", "Matroska"),
        ("bitrate", "58000"),
        ("overallbitrate", "64000"),
        ("framerate", "23.976"),
        ("bitdepth", "10"),
        ("hdrformats", "Dolby Vision, HDR10"),
        ("filesize", "32000000000"),
    ];
    for (name, want) in want_video {
        assert_eq!(child_text(video, name), want, "video/{name}");
    }
    let audio: Vec<&Element> = children(details, "audio").collect();
    assert_eq!(audio.len(), 2);
    for (name, want) in [
        ("codec", "truehd_atmos"),
        ("language", "jpn"),
        ("channels", "8"),
        ("title", "Original"),
        ("bitrate", "4500"),
        ("bitdepth", "24"),
        ("samplerate", "48000"),
        ("default", "true"),
        ("original", "true"),
    ] {
        assert_eq!(child_text(audio[0], name), want, "audio/{name}");
    }
    assert_eq!(child_text(audio[1], "commentary"), "true");
    let subtitles: Vec<&Element> = children(details, "subtitle").collect();
    assert_eq!(subtitles.len(), 2, "the embedded stream and the sidecar");
    for (name, want) in [
        ("language", "eng"),
        ("codec", "PGS"),
        ("title", "English SDH"),
        ("hearingimpaired", "true"),
    ] {
        assert_eq!(child_text(subtitles[0], name), want, "subtitle/{name}");
    }
    for (name, want) in [
        ("language", "eng"),
        ("forced", "true"),
        ("external", "true"),
    ] {
        assert_eq!(child_text(subtitles[1], name), want, "sidecar/{name}");
    }
}

/// `yyyy-MM-dd HH:mm:ss` in local time, as the NFO writes time stamps.
fn local(ms: i64) -> String {
    cv_app::nfo::time::local_from_millis(ms).expect("a time")
}

#[test]
fn a_show_and_its_episode_carry_every_field_of_the_library() {
    let lab = Lab::new();
    let folder = lab.dir("Shows/Kingdom");
    let dir = folder.to_string_lossy().into_owned();
    let mut show = Item::show("Kingdom");
    show.core.path = dir.clone();
    show.core.original_title = "킹덤".into();
    show.core.original_language = "ko".into();
    show.core.year = 2019;
    show.core.plot = "A crown prince faces a plague.".into();
    show.core.ids.set("tvdb", "355678");
    show.core.ids.set("tmdb", "70593");
    show.core.ids.set("imdb", "tt6611916");
    show.core.production_companies = vec!["AStory".into()];
    show.core.note = "Season 3 announced.".into();
    show.core.last_scraper_id = "tmdb".into();
    show.core
        .ratings
        .insert(Rating::new("tmdb", 8.3, 2_000, 10));
    if let ItemDetail::Show(s) = &mut show.detail {
        s.english_title = "Kingdom".into();
        s.sort_title = "Kingdom".into();
        s.tagline = "The dead walk.".into();
        s.runtime_minutes = 50;
        s.year_end = 2020;
        s.first_aired = CalendarDate::new(2019, 1, 25);
        s.status = AiredStatus::Ended;
        s.country = "KR".into();
        s.certification = Certification("US_TV_MA".into());
        s.genres = vec![Genre::Drama, Genre::Horror];
        s.top250 = 120;
        s.season_texts = vec![SeasonText {
            group: EpisodeGroup::aired(),
            season: 1,
            name: "Season One".into(),
            overview: "The plague begins.".into(),
        }];
    }
    let show_id = lab.insert(show);

    let season_dir = folder.join("Season 1");
    fs::create_dir_all(&season_dir).unwrap();
    let mut season = Item::season(show_id, 1);
    season.core.title = "Season One".into();
    season.core.path = season_dir.to_string_lossy().into_owned();
    lab.insert(season);

    let file = "Kingdom - S01E01.mkv";
    fs::write(season_dir.join(file), b"video").unwrap();
    let mut episode = Item::episode(show_id, "Episode 1");
    episode.core.path = season_dir.to_string_lossy().into_owned();
    episode.core.original_title = "1화".into();
    episode.core.plot = "The king is ill.".into();
    episode.core.ids.set("tvdb", "6900001");
    episode.core.original_filename = file.into();
    episode
        .core
        .ratings
        .insert(Rating::new("tmdb", 8.0, 50, 10));
    episode.core.credits = vec![
        person(CreditKind::Actor, "Ju Ji-hoon", "Lee Chang", "1"),
        person(CreditKind::Director, "Kim Seong-hun", "Director", "2"),
    ];
    let mut video = MediaFile::new(MediaFileType::Video, &episode.core.path, file);
    video.video_codec = "AVC".into();
    video.width = 1920;
    video.height = 1080;
    video.frame_rate = 25.0;
    video.duration_secs = 3_000;
    video.audio_streams = vec![audio("E-AC-3 Atmos", "ko", 6, "")];
    episode.core.media_files.push(video);
    if let ItemDetail::Episode(e) = &mut episode.detail {
        e.english_title = "Episode 1".into();
        let mut numbers = EpisodeNumbers::default();
        numbers.set(EpisodeNumber::new(EpisodeGroup::aired(), 1, 1));
        numbers.set(EpisodeNumber::new(
            EpisodeGroup::new(EpisodeOrdering::Absolute, ""),
            1,
            1,
        ));
        e.numbers = numbers;
        e.first_aired = CalendarDate::new(2019, 1, 25);
        e.watched = true;
        e.play_count = 2;
        e.media_source = MediaSource::WebDl;
        e.edition = Edition::Uncut;
        e.runtime_minutes = 50;
    }
    let episode_id = lab.insert(episode);

    let report = lab
        .service
        .rewrite(&NfoScope::Items(vec![show_id]), &NfoSettings::default())
        .unwrap();
    assert_eq!(report.totals.failed, 0, "{report:?}");

    let show = lab.item(show_id);
    let text = nfo_text(&folder, "tvshow.nfo");
    let nfo = read_show_nfo(&text).expect("a show NFO");
    let ItemDetail::Show(s) = &show.detail else {
        unreachable!()
    };
    assert_eq!(nfo.title, show.core.title);
    assert_eq!(nfo.original_title, show.core.original_title);
    assert_eq!(nfo.original_language, show.core.original_language);
    assert_eq!(nfo.year, u32::from(show.core.year));
    assert_eq!(nfo.plot, show.core.plot);
    assert_eq!(nfo.note, show.core.note);
    assert_eq!(nfo.studios, show.core.production_companies);
    for (key, value) in show.core.ids.iter() {
        assert_eq!(nfo.ids.get(key), Some(value), "id {key}");
    }
    assert_eq!(nfo.english_title, s.english_title);
    assert_eq!(nfo.sort_title, s.sort_title);
    assert_eq!(nfo.tagline, s.tagline);
    assert_eq!(nfo.runtime_minutes, s.runtime_minutes);
    assert_eq!(nfo.year_end, u32::from(s.year_end));
    assert_eq!(nfo.first_aired, s.first_aired);
    assert_eq!(nfo.status, s.status);
    assert_eq!(nfo.countries, ["South Korea"]);
    // `US_TV_MA` (the providers' spelling) and `US_TVMA` name the same rating.
    assert_eq!(
        nfo.certification.0.replace('_', ""),
        s.certification.0.replace('_', "")
    );
    assert_eq!(nfo.genres, s.genres);
    assert_eq!(nfo.top250, s.top250);
    assert!(
        text.contains("<namedseason number=\"1\">Season One</namedseason>"),
        "{text}"
    );

    let episode = lab.item(episode_id);
    let text = nfo_text(&season_dir, "Kingdom - S01E01.nfo");
    let read = read_episode_nfo(&text);
    assert_eq!(read.len(), 1);
    let nfo = &read[0].episode;
    let ItemDetail::Episode(e) = &episode.detail else {
        unreachable!()
    };
    assert_eq!(nfo.title, episode.core.title);
    assert_eq!(nfo.original_title, episode.core.original_title);
    assert_eq!(nfo.plot, episode.core.plot);
    assert_eq!(nfo.original_filename, episode.core.original_filename);
    assert_eq!(nfo.ids.get("tvdb"), Some("6900001"));
    assert_eq!(nfo.english_title, e.english_title);
    assert_eq!((nfo.season, nfo.episode), (1, 1));
    assert_eq!(nfo.first_aired, e.first_aired);
    assert_eq!(nfo.watched, e.watched);
    assert_eq!(nfo.play_count, e.play_count);
    assert_eq!(nfo.media_source, e.media_source);
    assert_eq!(nfo.edition, e.edition);
    assert_eq!(nfo.runtime_minutes, e.runtime_minutes);
    assert!(text.contains("<framerate>25</framerate>"), "{text}");
    assert!(text.contains("<codec>eac3_ddp_atmos</codec>"), "{text}");
}

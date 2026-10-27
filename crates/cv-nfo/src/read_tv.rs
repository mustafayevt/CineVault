//! The tolerant TV show, season and episode NFO readers.
//!
//! * [`read_show_nfo`] reads a `tvshow.nfo` (`<tvshow>`): titles, year, premiere date, plot,
//!   runtime, ratings, certification, genres, studios, countries, tags, people, trailer, status
//!   (`Continuing`, `Ended`, `Canceled` and the common alternatives), artwork, named seasons
//!   (`<namedseason number>`) and season artwork (`<thumb type="season" season>`). IDs come from
//!   `uniqueid`, `id` (a numeric `<id>` is the TVDB ID), `imdbid`, `tmdbid`, `tvdbid` and the
//!   `<episodeguide>` (JSON or URL); missing ones are searched in the raw text.
//! * [`read_season_nfo`] reads a `<season>`: its number (`seasonnumber`, else `season`), title,
//!   plot, premiere, artwork and IDs.
//! * [`read_episode_nfo`] reads every `<episodedetails>` of a file (several blocks, a
//!   `<xbmcmultiepisode>` wrapper, or blocks nested by a missing closing tag), and a NextPVR
//!   `<recording>`.
//!
//! Like the movie reader, they match names without regard to case, skip values that do not parse
//! and keep everything else of the file. See [`crate::read`] for the shared rules.

use std::collections::BTreeMap;

use cv_domain::{AiredStatus, CalendarDate, Edition, MediaSource, ProviderIds};

use crate::certification::parse_certification;
use crate::read::{
    detect_show_ids, each, fanart_urls, find_root, kids, merge_detected, parse_bool, parse_count,
    parse_date, parse_minutes, parse_timestamp, parse_year, prepare, read_credits, read_genres,
    read_ids, read_list, read_ratings, read_trailer, split_list, text, text_of_any, texts, thumbs,
};
use crate::tree::parse_roots;
use crate::tv::{EpisodeNfo, SeasonArtwork, SeasonNfo, ShowArtwork, ShowNfo, split_episode_list};
use crate::xml::Element;

impl ShowNfo {
    /// True when the NFO has a title.
    pub fn is_valid(&self) -> bool {
        !self.title.trim().is_empty()
    }
}

/// A season NFO as read. [`SeasonRead::season`] has the number `0` when the file gives none, so
/// check [`SeasonRead::number`] or [`SeasonRead::is_valid`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeasonRead {
    pub season: SeasonNfo,
    /// The season number from the file; `None` when it has none (or a negative one).
    pub number: Option<u32>,
    pub show_title: String,
    pub sort_title: String,
    pub year: u32,
    pub ids: ProviderIds,
}

impl SeasonRead {
    /// A season NFO is valid when it holds a season number (`0` is the specials season).
    pub fn is_valid(&self) -> bool {
        self.number.is_some()
    }
}

/// One `<episodedetails>` as read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EpisodeRead {
    pub episode: EpisodeNfo,
    /// The season number from the file; `None` when it has none (or `-1`).
    pub season_number: Option<u32>,
    /// The episode number from the file; `None` when it has none (or `-1`).
    pub episode_number: Option<u32>,
}

impl EpisodeRead {
    /// An episode block is valid when it has a title or an episode number.
    pub fn is_valid(&self) -> bool {
        !self.episode.title.trim().is_empty() || self.episode_number.is_some()
    }
}

/// A non-negative whole number from text; `None` for blank, negative or non-numeric text.
fn number(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    text.parse::<i64>()
        .ok()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| {
            let digits: String = text.chars().filter(char::is_ascii_digit).collect();
            (!text.starts_with('-'))
                .then(|| digits.parse().ok())
                .flatten()
        })
}

fn status_of(text: &str) -> AiredStatus {
    let lower = text.trim().to_ascii_lowercase();
    if lower.contains("cancel") {
        AiredStatus::Canceled
    } else if lower.contains("end") {
        AiredStatus::Ended
    } else if [
        "continu",
        "return",
        "production",
        "airing",
        "running",
        "ongoing",
    ]
    .iter()
    .any(|word| lower.contains(word))
    {
        AiredStatus::Continuing
    } else {
        AiredStatus::Unknown
    }
}

// ---- show ---------------------------------------------------------------------------------

/// Reads a `tvshow.nfo`. Returns `None` only when the text holds no `<tvshow>` element and no ID.
pub fn read_show_nfo(text_in: &str) -> Option<ShowNfo> {
    let roots = parse_roots(text_in);
    let mut recognised = true;
    let mut show = match find_root(&roots, &["tvshow"]) {
        Some(root) => show_of(&root),
        None => {
            recognised = false;
            ShowNfo::default()
        }
    };
    merge_detected(&mut show.ids, &detect_show_ids(text_in));
    (recognised || !show.ids.is_empty()).then_some(show)
}

fn show_of(root: &Element) -> ShowNfo {
    let (studios, _) = read_list(root, "studio");
    let (countries, _) = read_list(root, "country");
    let mut show = ShowNfo {
        title: text_of_any(root, &["title", "showtitle"]).to_owned(),
        original_title: text(root, "originaltitle").to_owned(),
        sort_title: text(root, "sorttitle").to_owned(),
        english_title: text(root, "english_title").to_owned(),
        plot: text(root, "plot").to_owned(),
        outline: text(root, "outline").to_owned(),
        tagline: text(root, "tagline").to_owned(),
        runtime_minutes: parse_minutes(text(root, "runtime")),
        top250: parse_count(text(root, "top250")),
        ids: read_ids(root, true),
        ratings: read_ratings(root),
        certification: cv_domain::Certification(parse_certification(text_of_any(
            root,
            &["certification", "mpaa"],
        ))),
        genres: read_genres(root),
        studios,
        countries,
        tags: texts(root, "tag"),
        credits: read_credits(root),
        first_aired: ["premiered", "releasedate", "aired"]
            .into_iter()
            .find_map(|name| parse_date(text(root, name))),
        status: status_of(text(root, "status")),
        date_added: parse_timestamp(text(root, "dateadded")),
        note: text(root, "user_note").to_owned(),
        locked_fields: split_list(text(root, "cv_locked")),
        original_language: text(root, "original_language").to_owned(),
        year_end: parse_year(text(root, "year_end")),
        ..ShowNfo::default()
    };
    show.year = match parse_year(text(root, "year")) {
        0 => year_of(show.first_aired),
        year => year,
    };
    show.watched = parse_bool(text(root, "watched")) || parse_count(text(root, "playcount")) > 0;
    show.trailers.extend(read_trailer(text(root, "trailer")));

    let mut seasons: BTreeMap<u32, SeasonNfo> = BTreeMap::new();
    for named in kids(root, "namedseason") {
        if let Some(n) = named.attribute("number").and_then(number) {
            let title = named.text.trim();
            if !title.is_empty() {
                season_entry(&mut seasons, n).title = title.to_owned();
            }
        }
    }
    artwork(root, &mut show.artwork, &mut seasons);
    show.seasons = seasons.into_values().collect();

    let guide = episodeguide_text(root);
    merge_detected(&mut show.ids, &detect_show_ids(&guide));
    show
}

fn year_of(date: Option<CalendarDate>) -> u32 {
    date.map_or(0, |d| u32::try_from(d.year()).unwrap_or(0))
}

fn season_entry(seasons: &mut BTreeMap<u32, SeasonNfo>, number: u32) -> &mut SeasonNfo {
    seasons.entry(number).or_insert_with(|| SeasonNfo {
        number,
        ..SeasonNfo::default()
    })
}

/// The text of `<episodeguide>`: its own text (the JSON style) and its `<url>` children.
fn episodeguide_text(root: &Element) -> String {
    let mut out = String::new();
    for guide in kids(root, "episodeguide") {
        out.push_str(&guide.text);
        out.push(' ');
        for url in kids(guide, "url") {
            out.push_str(&url.text);
            out.push(' ');
        }
    }
    out
}

fn artwork(root: &Element, art: &mut ShowArtwork, seasons: &mut BTreeMap<u32, SeasonNfo>) {
    for (aspect, thumb) in thumbs(root) {
        let url = thumb.text.trim().to_owned();
        if thumb.attribute("type") == Some("season") {
            let Some(n) = thumb.attribute("season").and_then(number) else {
                continue;
            };
            let season = &mut season_entry(seasons, n).artwork;
            let slot = match aspect.as_str() {
                "poster" => &mut season.poster,
                "banner" => &mut season.banner,
                "landscape" | "thumb" => &mut season.thumb,
                _ => continue,
            };
            if slot.is_empty() {
                *slot = url;
            }
            continue;
        }
        let slot = match aspect.as_str() {
            "poster" => &mut art.poster,
            "banner" => &mut art.banner,
            "clearart" => &mut art.clearart,
            "clearlogo" => &mut art.clearlogo,
            "landscape" | "thumb" => &mut art.thumb,
            "keyart" => &mut art.keyart,
            "squareart" => &mut art.squareart,
            "logo" => &mut art.logo,
            "characterart" => &mut art.characterart,
            "discart" | "disc" => &mut art.disc,
            _ => continue,
        };
        if slot.is_empty() {
            *slot = url;
        }
    }
    let mut fanart = fanart_urls(root).into_iter();
    art.fanart = fanart.next().unwrap_or_default();
    art.extra_fanarts = fanart.collect();
}

// ---- season -------------------------------------------------------------------------------

/// Reads a season NFO (`<season>`). Returns `None` when the text has no `<season>` element.
pub fn read_season_nfo(text_in: &str) -> Option<SeasonRead> {
    let roots = parse_roots(text_in);
    let root = find_root(&roots, &["season"])?;
    let number = number(text_of_any(&root, &["seasonnumber", "season"]));
    let first_aired = parse_date(text(&root, "premiered"));
    let year = match parse_year(text(&root, "year")) {
        0 => year_of(first_aired),
        year => year,
    };
    let artwork = SeasonArtwork {
        poster: kids(&root, "thumb")
            .map(|t| t.text.trim())
            .find(|u| crate::read::is_http(u))
            .unwrap_or_default()
            .to_owned(),
        fanart: fanart_urls(&root).into_iter().next().unwrap_or_default(),
        ..SeasonArtwork::default()
    };
    Some(SeasonRead {
        season: SeasonNfo {
            number: number.unwrap_or(0),
            title: text(&root, "title").to_owned(),
            plot: text(&root, "plot").to_owned(),
            note: text(&root, "user_note").to_owned(),
            first_aired,
            artwork,
            unknown_elements: Vec::new(),
        },
        number,
        show_title: text(&root, "showtitle").to_owned(),
        sort_title: text(&root, "sorttitle").to_owned(),
        year,
        ids: read_ids(&root, true),
    })
}

// ---- episodes -----------------------------------------------------------------------------

fn collect_blocks(element: &Element, blocks: &mut Vec<Element>) {
    let name = element.name.trim();
    if name.eq_ignore_ascii_case("episodedetails") || name.eq_ignore_ascii_case("recording") {
        blocks.push(element.clone());
    }
    for node in &element.children {
        if let crate::xml::Node::Element(child) = node {
            collect_blocks(child, blocks);
        }
    }
}

/// Reads all episodes of an NFO file: each `<episodedetails>` (also inside a wrapper element or
/// nested by a missing closing tag), or a NextPVR `<recording>`. The result is empty when the file
/// holds none; blocks without a title or a number are returned too, see
/// [`EpisodeRead::is_valid`].
pub fn read_episode_nfo(text_in: &str) -> Vec<EpisodeRead> {
    let mut blocks = Vec::new();
    for root in parse_roots(text_in) {
        collect_blocks(&root, &mut blocks);
    }
    blocks
        .iter()
        .map(prepare)
        .map(|block| {
            if block.name == "recording" {
                recording_episode(&block)
            } else {
                episode_of(&block)
            }
        })
        .collect()
}

fn recording_episode(block: &Element) -> EpisodeRead {
    let subtitle = text(block, "subtitle");
    let (title, show_title) = if subtitle.is_empty() {
        (text(block, "title"), "")
    } else {
        (subtitle, text(block, "title"))
    };
    let season_number = number(text(block, "season"));
    let episode_number = number(text(block, "episode"));
    EpisodeRead {
        episode: EpisodeNfo {
            title: title.to_owned(),
            show_title: show_title.to_owned(),
            season: season_number.unwrap_or(0),
            episode: episode_number.unwrap_or(0),
            plot: text_of_any(block, &["description", "plot"]).to_owned(),
            certification: cv_domain::Certification(parse_certification(text(block, "rating"))),
            first_aired: parse_date(text_of_any(block, &["aired", "premiered", "startTime"])),
            ..EpisodeNfo::default()
        },
        season_number,
        episode_number,
    }
}

fn episode_of(block: &Element) -> EpisodeRead {
    let season_number = number(text(block, "season"));
    let episode_number = number(text(block, "episode"));
    let display = number(text(block, "displayseason")).zip(number(text(block, "displayepisode")));
    let play_count = parse_count(text(block, "playcount"));
    let episode = EpisodeNfo {
        title: text(block, "title").to_owned(),
        original_title: text(block, "originaltitle").to_owned(),
        english_title: text(block, "english_title").to_owned(),
        show_title: text(block, "showtitle").to_owned(),
        season: season_number.unwrap_or(0),
        episode: episode_number.unwrap_or(0),
        display,
        ids: read_ids(block, true),
        ratings: read_ratings(block),
        plot: text(block, "plot").to_owned(),
        outline: text(block, "outline").to_owned(),
        runtime_minutes: parse_minutes(text(block, "runtime")),
        thumb: kids(block, "thumb")
            .map(|t| t.text.trim())
            .find(|u| crate::read::is_http(u))
            .unwrap_or_default()
            .to_owned(),
        certification: cv_domain::Certification(parse_certification(text_of_any(
            block,
            &["certification", "mpaa"],
        ))),
        first_aired: ["aired", "premiered", "releasedate"]
            .into_iter()
            .find_map(|name| parse_date(text(block, name))),
        watched: parse_bool(text(block, "watched")) || play_count > 0,
        play_count,
        last_played: parse_timestamp(text(block, "lastplayed")),
        studios: each(block, "studio")
            .into_iter()
            .flat_map(|e| split_episode_list(&e.text))
            .collect(),
        tags: texts(block, "tag"),
        credits: read_credits(block),
        date_added: parse_timestamp(text(block, "dateadded")),
        media_source: MediaSource::parse(text(block, "source")).unwrap_or_default(),
        edition: Edition::parse(text(block, "edition")).unwrap_or_default(),
        original_filename: text(block, "original_filename").to_owned(),
        note: text(block, "user_note").to_owned(),
        crc32: Some(text(block, "crc32").to_owned()).filter(|c| !c.is_empty()),
        ..EpisodeNfo::default()
    };
    EpisodeRead {
        episode,
        season_number,
        episode_number,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_domain::CreditKind;

    const SHOW: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<tvshow>\r\n  <title>Harbor Town</title>\r\n  <originaltitle>Hafenstadt</originaltitle>\r\n  <year>2010</year>\r\n  <plot>Life at the docks.</plot>\r\n  <runtime>45</runtime>\r\n  <thumb aspect=\"poster\">https://example.test/p.jpg</thumb>\r\n  <thumb aspect=\"banner\">https://example.test/b.jpg</thumb>\r\n  <thumb aspect=\"poster\" type=\"season\" season=\"1\">https://example.test/s1.jpg</thumb>\r\n  <thumb aspect=\"poster\" type=\"season\" season=\"0\">https://example.test/s0.jpg</thumb>\r\n  <thumb aspect=\"poster\" type=\"season\" season=\"-1\">https://example.test/all.jpg</thumb>\r\n  <namedseason number=\"1\">The Beginning</namedseason>\r\n  <fanart><thumb>https://example.test/f.jpg</thumb></fanart>\r\n  <mpaa>TV-14</mpaa>\r\n  <episodeguide>{\"tvdb\":\"81189\"}</episodeguide>\r\n  <id>81189</id>\r\n  <imdbid>tt0903747</imdbid>\r\n  <tmdbid>1396</tmdbid>\r\n  <premiered>2010-01-20</premiered>\r\n  <status>Returning Series</status>\r\n  <genre>Drama</genre>\r\n  <studio>Dock Network</studio>\r\n  <country>US</country>\r\n  <actor><name>Lead One</name><role>Captain</role></actor>\r\n  <director>Boss One</director>\r\n</tvshow>\r\n";

    #[test]
    fn a_show_is_read_completely() {
        let show = read_show_nfo(SHOW).unwrap();
        assert!(show.is_valid());
        assert_eq!(show.title, "Harbor Town");
        assert_eq!(show.year, 2010);
        assert_eq!(show.runtime_minutes, 45);
        assert_eq!(show.certification.0, "US_TV14");
        assert_eq!(show.status, AiredStatus::Continuing);
        assert_eq!(show.first_aired.unwrap().to_string(), "2010-01-20");
        assert_eq!(show.ids.get("tvdb"), Some("81189"));
        assert_eq!(show.ids.get("imdb"), Some("tt0903747"));
        assert_eq!(show.ids.get("tmdb"), Some("1396"));
        assert_eq!(show.artwork.poster, "https://example.test/p.jpg");
        assert_eq!(show.artwork.banner, "https://example.test/b.jpg");
        assert_eq!(show.artwork.fanart, "https://example.test/f.jpg");
        let numbers: Vec<_> = show.seasons.iter().map(|s| s.number).collect();
        assert_eq!(numbers, [0, 1]);
        assert_eq!(show.seasons[1].title, "The Beginning");
        assert_eq!(
            show.seasons[1].artwork.poster,
            "https://example.test/s1.jpg"
        );
        assert_eq!(
            show.seasons[0].artwork.poster,
            "https://example.test/s0.jpg"
        );
        let kinds: Vec<_> = show.credits.iter().map(|c| c.kind).collect();
        assert_eq!(kinds, [CreditKind::Actor, CreditKind::Director]);
        assert_eq!(show.studios, ["Dock Network"]);
    }

    #[test]
    fn show_ids_come_from_the_episodeguide_and_the_raw_text() {
        let show = read_show_nfo(
            "<tvshow><title>X</title><episodeguide><url cache=\"auth.json\">http://thetvdb.com/?tab=series&amp;id=81189</url></episodeguide></tvshow>",
        )
        .unwrap();
        assert_eq!(show.ids.get("tvdb"), Some("81189"));
        let raw = read_show_nfo(
            "<tvshow><title>X</title><plot>https://www.themoviedb.org/tv/1396</plot></tvshow>",
        )
        .unwrap();
        assert_eq!(raw.ids.get("tmdb"), Some("1396"));
        let only = read_show_nfo("https://thetvdb.com/series/81189").unwrap();
        assert_eq!(only.ids.get("tvdb"), Some("81189"));
        assert!(!only.is_valid());
        assert!(read_show_nfo("plain words").is_none());
        let tmdb_only = read_show_nfo("https://www.themoviedb.org/tv/1396").unwrap();
        assert_eq!(tmdb_only.ids.get("tmdb"), Some("1396"));
    }

    #[test]
    fn show_status_words() {
        for (word, status) in [
            ("Ended", AiredStatus::Ended),
            ("Canceled", AiredStatus::Canceled),
            ("Cancelled", AiredStatus::Canceled),
            ("Continuing", AiredStatus::Continuing),
            ("In Production", AiredStatus::Continuing),
            ("whatever", AiredStatus::Unknown),
            ("", AiredStatus::Unknown),
        ] {
            assert_eq!(status_of(word), status, "{word}");
        }
    }

    #[test]
    fn a_broken_show_keeps_what_was_read() {
        let show =
            read_show_nfo("<tvshow><title>Half</title><year>2001</year><plot>Cut o").unwrap();
        assert_eq!((show.title.as_str(), show.year), ("Half", 2001));
    }

    #[test]
    fn a_season_is_read() {
        let season = read_season_nfo(
            "<season><seasonnumber>2</seasonnumber><title>Second</title><showtitle>Harbor Town</showtitle><year>2011</year><plot>More.</plot><thumb>https://example.test/s2.jpg</thumb><fanart>https://example.test/f2.jpg</fanart><tvdbid>5</tvdbid><premiered>2011-02-03</premiered></season>",
        )
        .unwrap();
        assert!(season.is_valid());
        assert_eq!(season.season.number, 2);
        assert_eq!(season.season.title, "Second");
        assert_eq!(season.show_title, "Harbor Town");
        assert_eq!(season.year, 2011);
        assert_eq!(season.season.first_aired.unwrap().to_string(), "2011-02-03");
        assert_eq!(season.season.artwork.poster, "https://example.test/s2.jpg");
        assert_eq!(season.season.artwork.fanart, "https://example.test/f2.jpg");
        assert_eq!(season.ids.get("tvdb"), Some("5"));
    }

    #[test]
    fn season_numbers_and_validity() {
        let specials = read_season_nfo("<season><season>0</season></season>").unwrap();
        assert_eq!(specials.number, Some(0));
        assert!(specials.is_valid());
        for bad in [
            "<season><title>x</title></season>",
            "<season><seasonnumber>-1</seasonnumber></season>",
        ] {
            assert!(!read_season_nfo(bad).unwrap().is_valid(), "{bad}");
        }
        assert!(read_season_nfo("<tvshow/>").is_none());
    }

    const MULTI: &str = "<?xml version=\"1.0\"?>\r\n<episodedetails><title>One</title><showtitle>Harbor Town</showtitle><season>2</season><episode>5</episode><aired>2011-03-04</aired><plot>First.</plot><runtime>44</runtime><id>111</id><thumb>https://example.test/e5.jpg</thumb><watched>true</watched><playcount>1</playcount><lastplayed>2025-01-02 03:04:05</lastplayed><credits>Writer One</credits><director>Director One</director><actor><name>Guest One</name><type>GuestStar</type></actor><studio>A;B</studio></episodedetails>\r\n<episodedetails><title>Two</title><season>2</season><episode>6</episode></episodedetails>";

    #[test]
    fn multi_episode_files_give_one_episode_per_block() {
        let episodes = read_episode_nfo(MULTI);
        assert_eq!(episodes.len(), 2);
        let first = &episodes[0];
        assert!(first.is_valid());
        assert_eq!(
            (first.season_number, first.episode_number),
            (Some(2), Some(5))
        );
        let e = &first.episode;
        assert_eq!(e.title, "One");
        assert_eq!(e.show_title, "Harbor Town");
        assert_eq!(e.first_aired.unwrap().to_string(), "2011-03-04");
        assert_eq!(e.runtime_minutes, 44);
        assert_eq!(e.ids.get("tvdb"), Some("111"));
        assert_eq!(e.thumb, "https://example.test/e5.jpg");
        assert!(e.watched);
        assert_eq!(e.last_played.as_deref(), Some("2025-01-02 03:04:05"));
        assert_eq!(e.studios, ["A", "B"]);
        let kinds: Vec<_> = e.credits.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            [CreditKind::Guest, CreditKind::Director, CreditKind::Writer]
        );
        assert_eq!(episodes[1].episode.title, "Two");
    }

    #[test]
    fn wrapped_and_nested_blocks_are_found() {
        let wrapped = read_episode_nfo(
            "<xbmcmultiepisode><episodedetails><title>A</title><episode>1</episode></episodedetails><episodedetails><title>B</title><episode>2</episode></episodedetails></xbmcmultiepisode>",
        );
        assert_eq!(wrapped.len(), 2);
        let nested = read_episode_nfo(
            "<episodedetails><title>A</title><episode>1</episode><episodedetails><title>B</title><episode>2</episode></episodedetails>",
        );
        let titles: Vec<_> = nested.iter().map(|e| e.episode.title.as_str()).collect();
        assert_eq!(titles, ["A", "B"]);
        assert_eq!(nested[1].episode_number, Some(2));
    }

    #[test]
    fn episode_validity_and_missing_numbers() {
        let episodes = read_episode_nfo(
            "<episodedetails><title>Only title</title><season>-1</season><episode>-1</episode></episodedetails><episodedetails><episode>3</episode></episodedetails><episodedetails><plot>x</plot></episodedetails>",
        );
        assert_eq!(episodes.len(), 3);
        assert!(episodes[0].is_valid());
        assert_eq!(episodes[0].episode_number, None);
        assert!(episodes[1].is_valid());
        assert!(!episodes[2].is_valid());
        assert!(read_episode_nfo("not xml").is_empty());
        assert!(read_episode_nfo("<movie><title>x</title></movie>").is_empty());
    }

    #[test]
    fn a_recording_is_an_episode_with_the_subtitle_as_title() {
        let episodes = read_episode_nfo(
            "<recording><title>Night Show</title><subtitle>Pilot</subtitle><description>Live.</description><season>1</season><episode>1</episode><rating>TV-PG</rating></recording>",
        );
        assert_eq!(episodes.len(), 1);
        let e = &episodes[0].episode;
        assert_eq!(
            (e.title.as_str(), e.show_title.as_str()),
            ("Pilot", "Night Show")
        );
        assert_eq!(e.plot, "Live.");
        assert_eq!(e.certification.0, "US_TVPG");
        assert_eq!(episodes[0].episode_number, Some(1));
        let no_subtitle = read_episode_nfo("<recording><title>Just this</title></recording>");
        assert_eq!(no_subtitle[0].episode.title, "Just this");
        assert!(no_subtitle[0].is_valid());
    }
}

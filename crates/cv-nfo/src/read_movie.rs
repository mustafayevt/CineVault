//! The tolerant movie and movie-set NFO readers.
//!
//! [`read_movie_nfo`] reads a `<movie>` (Kodi, XBMC, Jellyfin, Emby, MediaPortal and other
//! tools) or a NextPVR `<recording>` into a [`MovieNfo`]. What it maps:
//!
//! | Model field | Elements read |
//! |---|---|
//! | title, original and sort title | `title`, `originaltitle`, `sorttitle`, `english_title` |
//! | year | `year`, else the year of the release date |
//! | release date | `premiered`, `releasedate`, `aired` (several date layouts) |
//! | plot, outline, tagline | `plot`, `outline` (`plotoutline`), `tagline` |
//! | runtime | `runtime` as minutes, `1h 58m` or `1:58:00` |
//! | ids | see [`crate::read`]: `uniqueid`, `id` (IMDb only), `imdbid`, `tmdbid`, `ids/entry` |
//! | ratings | `ratings/rating` or the old `rating` + `votes`, `userrating`, `criticrating` |
//! | certification | `certification`, else `mpaa`, as a token such as `US_PG13` |
//! | genres | `genre` (repeated or inside `genres`), each split at `; , / \|` |
//! | studios, countries | repeated tags, or one tag with values joined by `,` or `/` |
//! | sets | `set` as text or `<set><name/><overview/></set>`, `sets`, `tmdbcolid`, `collectionnumber` |
//! | people | `actor`, `director`, `credits` (writers), `producer`, `crew` |
//! | artwork | `thumb` with an `aspect` (http and https URLs only), `fanart` |
//! | trailer | `trailer`; the Kodi YouTube and HD-Trailers plug-in URLs become web links |
//! | watched state | `watched`, `playcount`, `lastplayed`, `dateadded` |
//! | CineVault's own tags | `source`, `edition`, `original_filename`, `user_note`, `crc32`, `cv_locked`, `original_language`, `video3d`, `extrathumbs/thumb` |
//!
//! A NextPVR `<recording>` gives the title, `description` as the plot, the certification
//! (written in `<rating>`) and the genres. NFOs that are only a URL or an ID give a movie that
//! holds just the ID (see [`crate::read`]).

use cv_domain::{CalendarDate, Edition, MediaSource, id_key};

use crate::certification::parse_certification;
use crate::movie::{MovieArtwork, MovieNfo, MovieSetRef};
use crate::movie_set::MovieSetNfo;
use crate::read::{
    detect_collection_id, detect_movie_ids, each, fanart_urls, find_root, is_http, kids,
    merge_detected, parse_bool, parse_count, parse_date, parse_minutes, parse_timestamp,
    parse_year, read_credits, read_genres, read_ids, read_list, read_ratings, read_trailer,
    split_list, text, text_of_any, texts, thumbs,
};
use crate::tree::parse_roots;
use crate::xml::Element;

impl MovieNfo {
    /// True when the NFO has a title. CineVault manages only such files: an NFO without a title
    /// (a release note, a URL file) is foreign and is never rewritten or deleted.
    pub fn is_valid(&self) -> bool {
        !self.title.trim().is_empty()
    }
}

impl MovieSetNfo {
    /// True when the NFO has a title.
    pub fn is_valid(&self) -> bool {
        !self.title.trim().is_empty()
    }
}

/// Reads a movie NFO. Returns `None` only when the text holds neither a `<movie>` or
/// `<recording>` element nor any ID, so a file that is just a URL or an ID still gives a movie
/// (without a title, so [`MovieNfo::is_valid`] is false).
pub fn read_movie_nfo(text: &str) -> Option<MovieNfo> {
    let roots = parse_roots(text);
    let mut recognised = true;
    let mut movie = if let Some(root) = find_root(&roots, &["movie"]) {
        movie_of(&root)
    } else if let Some(root) = find_root(&roots, &["recording"]) {
        recording_of(&root)
    } else {
        recognised = false;
        MovieNfo::default()
    };
    merge_detected(&mut movie.ids, &detect_movie_ids(text));
    (recognised || !movie.ids.is_empty()).then_some(movie)
}

fn recording_of(root: &Element) -> MovieNfo {
    MovieNfo {
        title: text(root, "title").to_owned(),
        plot: text_of_any(root, &["description", "plot"]).to_owned(),
        certification: cv_domain::Certification(parse_certification(text(root, "rating"))),
        genres: read_genres(root),
        year: parse_year(text(root, "year")),
        ..MovieNfo::default()
    }
}

fn movie_of(root: &Element) -> MovieNfo {
    let mut movie = MovieNfo {
        title: text_of_any(root, &["title", "name"]).to_owned(),
        original_title: text(root, "originaltitle").to_owned(),
        sort_title: text(root, "sorttitle").to_owned(),
        english_title: text(root, "english_title").to_owned(),
        plot: text(root, "plot").to_owned(),
        outline: text(root, "outline").to_owned(),
        tagline: text(root, "tagline").to_owned(),
        runtime_minutes: parse_minutes(text(root, "runtime")),
        top250: parse_count(text(root, "top250")),
        ids: read_ids(root, false),
        ratings: read_ratings(root),
        certification: cv_domain::Certification(parse_certification(text_of_any(
            root,
            &["certification", "mpaa"],
        ))),
        genres: read_genres(root),
        spoken_languages: texts(root, "languages")
            .iter()
            .chain(texts(root, "language").iter())
            .flat_map(|list| split_list(list))
            .collect(),
        tags: texts(root, "tag"),
        credits: read_credits(root),
        show_links: texts(root, "showlink"),
        original_filename: text(root, "original_filename").to_owned(),
        note: text(root, "user_note").to_owned(),
        crc32: Some(text(root, "crc32").to_owned()).filter(|c| !c.is_empty()),
        artwork: artwork(root),
        ..MovieNfo::default()
    };

    let (studios, studio_text) = read_list(root, "studio");
    let (countries, country_text) = read_list(root, "country");
    movie.studios = studios;
    movie.studio_text = studio_text;
    movie.countries = countries;
    movie.country_text = country_text;

    movie.release_date = ["premiered", "releasedate", "aired"]
        .into_iter()
        .find_map(|name| parse_date(text(root, name)));
    movie.year = match parse_year(text(root, "year")) {
        0 => release_year(movie.release_date),
        year => year,
    };

    if let Some(trailer) = read_trailer(text(root, "trailer")) {
        movie.trailers.push(trailer);
    }

    let mut sets = sets(root);
    if let Some(first) = sets.first_mut() {
        if first.tmdb_id.is_none() {
            first.tmdb_id = movie.ids.get_u64(id_key::TMDB_SET);
        }
        if let Some(id) = first.tmdb_id {
            movie.ids.set(id_key::TMDB_SET, id.to_string());
        }
    }
    if !sets.is_empty() {
        movie.movie_set = Some(sets.remove(0));
        movie.other_sets = sets;
    }

    movie.play_count = parse_count(text(root, "playcount"));
    movie.watched = parse_bool(text(root, "watched")) || movie.play_count > 0;
    movie.last_played = parse_timestamp(text(root, "lastplayed"));
    movie.date_added = parse_timestamp(text(root, "dateadded"));

    movie.media_source = MediaSource::parse(text(root, "source")).unwrap_or_default();
    movie.edition = edition(root);
    movie.locked_fields = split_list(text(root, "cv_locked"));
    movie.original_language = text(root, "original_language").to_owned();
    movie.video_in_3d = parse_bool(text(root, "video3d"));
    movie.artwork.extra_thumbs = kids(root, "extrathumbs")
        .flat_map(|list| kids(list, "thumb"))
        .map(|thumb| thumb.text.trim().to_owned())
        .filter(|url| is_http(url))
        .collect();
    movie
}

fn release_year(date: Option<CalendarDate>) -> u32 {
    date.map_or(0, |d| u32::try_from(d.year()).unwrap_or(0))
}

fn edition(root: &Element) -> Edition {
    let own = text(root, "edition");
    if !own.is_empty() {
        return Edition::parse(own).unwrap_or_default();
    }
    // The title Kodi shows: `Director's Cut` reads as the `DIRECTORS_CUT` edition.
    let title = text(root, "videoassettitle").replace(['\'', '\u{2019}'], "");
    Edition::parse(&title).unwrap_or_default()
}

/// Every `<set>`: text or object form, also inside `<sets>`.
fn sets(root: &Element) -> Vec<MovieSetRef> {
    each(root, "set")
        .into_iter()
        .filter_map(|set| {
            let (title, overview) = if kids(set, "name").next().is_some() {
                (text(set, "name"), text(set, "overview"))
            } else {
                (set.text.trim(), "")
            };
            let id = set
                .attribute("tmdbcolid")
                .map(str::to_owned)
                .filter(|v| !v.trim().is_empty())
                .or_else(|| Some(text(set, "tmdbcolid").to_owned()))
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|id| *id > 0);
            (!title.is_empty()).then(|| MovieSetRef {
                title: title.to_owned(),
                overview: overview.to_owned(),
                tmdb_id: id,
            })
        })
        .collect()
}

/// The `<thumb aspect>` and `<fanart>` URLs. The first thumb of an aspect wins.
fn artwork(root: &Element) -> MovieArtwork {
    let mut art = MovieArtwork::default();
    for (aspect, thumb) in thumbs(root) {
        let url = thumb.text.trim().to_owned();
        let slot = match aspect.as_str() {
            "poster" => &mut art.poster,
            "banner" => &mut art.banner,
            "clearart" => &mut art.clearart,
            "clearlogo" => &mut art.clearlogo,
            "discart" | "disc" => &mut art.disc,
            "landscape" | "thumb" => &mut art.thumb,
            "keyart" => &mut art.keyart,
            "squareart" => &mut art.squareart,
            "logo" => &mut art.logo,
            _ => continue,
        };
        if slot.is_empty() {
            *slot = url;
        }
    }
    let mut fanart = fanart_urls(root).into_iter();
    art.fanart = fanart.next().unwrap_or_default();
    art.extra_fanarts = fanart.collect();
    art
}

/// Reads a movie-set NFO (`<collection>`): title (`title` or `name`), `sorttitle`, `overview`
/// (`plot`), `thumb` and `fanart` URLs, IDs (`uniqueid`, `tmdbid`), tags, `dateadded` and
/// CineVault's `user_note` and `english_title`. Returns `None` when there is no `<collection>`
/// element and no ID in the text.
pub fn read_movie_set_nfo(text_in: &str) -> Option<MovieSetNfo> {
    let roots = parse_roots(text_in);
    let Some(root) = find_root(&roots, &["collection"]) else {
        let mut set = MovieSetNfo::default();
        if let Some(id) = detect_collection_id(text_in) {
            set.ids.set(id_key::TMDB, id.to_string());
        }
        return (!set.ids.is_empty()).then_some(set);
    };
    let mut set = MovieSetNfo {
        title: text_of_any(&root, &["title", "name"]).to_owned(),
        sort_title: text(&root, "sorttitle").to_owned(),
        overview: text_of_any(&root, &["overview", "plot"]).to_owned(),
        ids: read_ids(&root, false),
        tags: texts(&root, "tag"),
        date_added: parse_timestamp(text(&root, "dateadded")),
        note: text(&root, "user_note").to_owned(),
        english_title: text(&root, "english_title").to_owned(),
        ..MovieSetNfo::default()
    };
    set.poster = kids(&root, "thumb")
        .map(|t| t.text.trim())
        .find(|u| is_http(u))
        .unwrap_or_default()
        .to_owned();
    set.fanart = fanart_urls(&root).into_iter().next().unwrap_or_default();
    if !set.ids.contains(id_key::TMDB)
        && let Some(id) = detect_collection_id(text_in)
    {
        set.ids.set(id_key::TMDB, id.to_string());
    }
    Some(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_domain::{CreditKind, Genre};

    const KODI: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<movie>\r\n  <title>Harbor Lights</title>\r\n  <originaltitle>Les Lumieres</originaltitle>\r\n  <year>1992</year>\r\n  <ratings><rating name=\"imdb\" max=\"10\" default=\"true\"><value>7.5</value><votes>1200</votes></rating></ratings>\r\n  <userrating>8</userrating>\r\n  <set tmdbcolid=\"77\"><name>Harbor Saga</name><overview>All parts.</overview></set>\r\n  <plot>A sailor finds a lamp.</plot>\r\n  <runtime>96</runtime>\r\n  <thumb aspect=\"poster\">https://example.test/p.jpg</thumb>\r\n  <thumb aspect=\"landscape\">https://example.test/l.jpg</thumb>\r\n  <thumb aspect=\"poster\">/local/p.jpg</thumb>\r\n  <fanart><thumb>https://example.test/f1.jpg</thumb><thumb>https://example.test/f2.jpg</thumb></fanart>\r\n  <mpaa>Rated PG-13</mpaa>\r\n  <uniqueid type=\"imdb\" default=\"true\">tt0103639</uniqueid>\r\n  <uniqueid type=\"tmdb\">949</uniqueid>\r\n  <premiered>1992-05-01</premiered>\r\n  <watched>true</watched>\r\n  <playcount>2</playcount>\r\n  <lastplayed>2025-01-02 03:04:05</lastplayed>\r\n  <genre>Drama</genre><genre>Sci-Fi / Thriller</genre>\r\n  <studio>First Co</studio><studio>Second Co</studio>\r\n  <country>US</country>\r\n  <credits>Writer One</credits>\r\n  <director>Director One</director>\r\n  <tag>harbor</tag>\r\n  <actor><name>Actor One</name><role>Lead</role><order>0</order></actor>\r\n  <trailer>plugin://plugin.video.youtube/play/?video_id=abc123</trailer>\r\n  <languages>English, German</languages>\r\n  <dateadded>2024-01-01 00:00:00</dateadded>\r\n  <videoassettitle>Director's Cut</videoassettitle>\r\n  <customfield>kept elsewhere</customfield>\r\n</movie>\r\n";

    #[test]
    fn a_kodi_movie_is_read_completely() {
        let movie = read_movie_nfo(KODI).unwrap();
        assert!(movie.is_valid());
        assert_eq!(movie.title, "Harbor Lights");
        assert_eq!(movie.original_title, "Les Lumieres");
        assert_eq!(movie.year, 1992);
        assert_eq!(movie.runtime_minutes, 96);
        assert_eq!(movie.ids.get("imdb"), Some("tt0103639"));
        assert_eq!(movie.ids.get("tmdb"), Some("949"));
        assert_eq!(movie.ids.get("tmdbSet"), Some("77"));
        assert_eq!(movie.ratings.get("imdb").unwrap().votes, 1200);
        assert_eq!(movie.ratings.get("user").unwrap().value, 8.0);
        assert_eq!(movie.certification.0, "US_PG13");
        assert_eq!(
            movie.genres,
            [Genre::Drama, Genre::ScienceFiction, Genre::Thriller]
        );
        assert_eq!(movie.studios, ["First Co", "Second Co"]);
        assert_eq!(movie.countries, ["US"]);
        assert_eq!(movie.spoken_languages, ["English", "German"]);
        assert_eq!(movie.release_date.unwrap().to_string(), "1992-05-01");
        assert!(movie.watched);
        assert_eq!(movie.play_count, 2);
        assert_eq!(movie.last_played.as_deref(), Some("2025-01-02 03:04:05"));
        assert_eq!(movie.date_added.as_deref(), Some("2024-01-01 00:00:00"));
        assert_eq!(movie.edition, Edition::DirectorsCut);
        let set = movie.movie_set.unwrap();
        assert_eq!(
            (set.title.as_str(), set.overview.as_str(), set.tmdb_id),
            ("Harbor Saga", "All parts.", Some(77))
        );
        assert_eq!(movie.artwork.poster, "https://example.test/p.jpg");
        assert_eq!(movie.artwork.thumb, "https://example.test/l.jpg");
        assert_eq!(movie.artwork.fanart, "https://example.test/f1.jpg");
        assert_eq!(movie.artwork.extra_fanarts, ["https://example.test/f2.jpg"]);
        assert_eq!(
            movie.trailers[0].url,
            "https://www.youtube.com/watch?v=abc123"
        );
        let kinds: Vec<_> = movie.credits.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            [CreditKind::Actor, CreditKind::Director, CreditKind::Writer]
        );
        assert_eq!(movie.tags, ["harbor"]);
    }

    #[test]
    fn the_unclosed_tag_after_title_and_year_keeps_both() {
        let movie =
            read_movie_nfo("<movie><title>Heat</title><year>1995</year><plot>Cut o").unwrap();
        assert_eq!((movie.title.as_str(), movie.year), ("Heat", 1995));
        assert!(movie.is_valid());
        let swallowed = read_movie_nfo(
            "<movie><title>Heat</title><genre>Crime<runtime>170</runtime><year>1995</year></movie>",
        )
        .unwrap();
        assert_eq!(swallowed.runtime_minutes, 170);
        assert_eq!(swallowed.year, 1995);
        assert_eq!(swallowed.genres, [Genre::Crime]);
    }

    #[test]
    fn a_url_or_id_alone_gives_the_id() {
        let movie = read_movie_nfo("https://www.themoviedb.org/movie/949\r\n").unwrap();
        assert_eq!(movie.ids.get("tmdb"), Some("949"));
        assert!(!movie.is_valid());
        let imdb = read_movie_nfo("tt0113277").unwrap();
        assert_eq!(imdb.ids.get("imdb"), Some("tt0113277"));
        assert!(read_movie_nfo("").is_none());
        assert!(read_movie_nfo("Release notes only").is_none());
    }

    #[test]
    fn missing_ids_come_from_the_raw_text() {
        let movie = read_movie_nfo(
            "<movie><title>Heat</title><plot>See https://www.themoviedb.org/movie/949 and tt0113277.</plot></movie>",
        )
        .unwrap();
        assert_eq!(movie.ids.get("tmdb"), Some("949"));
        assert_eq!(movie.ids.get("imdb"), Some("tt0113277"));
        // IDs of the elements win over the text.
        let own = read_movie_nfo(
            "<movie><title>Heat</title><imdbid>tt0000001</imdbid><!-- tt0113277 --></movie>",
        )
        .unwrap();
        assert_eq!(own.ids.get("imdb"), Some("tt0000001"));
    }

    #[test]
    fn alternative_spellings_and_old_layouts() {
        let movie = read_movie_nfo(
            "<MOVIE><Title> Old Layout </Title><Original_Title>Alt</Original_Title><Sort_Title>Layout, Old</Sort_Title><rating>6,8</rating><votes>1.234</votes><tmdbId>12</tmdbId><imdb_id>tt0113277</imdb_id><releasedate>01.03.2001</releasedate><certification>FSK 12</certification><studio>A, B</studio><sets><set>Plain Set</set><set>Second</set></sets><fanart>https://example.test/f.jpg</fanart><thumb>https://example.test/old.jpg</thumb></MOVIE>",
        )
        .unwrap();
        assert_eq!(movie.title, "Old Layout");
        assert_eq!(movie.original_title, "Alt");
        assert_eq!(movie.sort_title, "Layout, Old");
        let rating = movie.ratings.get("NFO").unwrap();
        assert_eq!((rating.value, rating.votes), (6.8, 1234));
        assert_eq!(movie.ids.get("tmdb"), Some("12"));
        assert_eq!(movie.ids.get("imdb"), Some("tt0113277"));
        assert_eq!(movie.year, 2001, "the year follows the release date");
        assert_eq!(movie.certification.0, "DE_FSK12");
        assert_eq!(movie.studios, ["A", "B"]);
        assert_eq!(movie.studio_text, "A, B");
        assert_eq!(movie.movie_set.as_ref().unwrap().title, "Plain Set");
        assert_eq!(movie.other_sets[0].title, "Second");
        assert_eq!(movie.artwork.fanart, "https://example.test/f.jpg");
        assert_eq!(movie.artwork.poster, "https://example.test/old.jpg");
    }

    #[test]
    fn jaxb_id_maps_and_collection_numbers() {
        let movie = read_movie_nfo(
            "<movie><title>X</title><ids><entry><key>imdb</key><value>tt0113277</value></entry><entry><key>tmdb</key><value>5</value></entry></ids><collectionnumber>321</collectionnumber><set><name>S</name></set></movie>",
        )
        .unwrap();
        assert_eq!(movie.ids.get("imdb"), Some("tt0113277"));
        assert_eq!(movie.ids.get("tmdb"), Some("5"));
        assert_eq!(movie.movie_set.unwrap().tmdb_id, Some(321));
    }

    #[test]
    fn a_bad_element_does_not_fail_the_file() {
        let movie = read_movie_nfo(
            "<movie><title>Fine</title><year>soon</year><runtime>long</runtime><rating>n/a</rating><premiered>someday</premiered><playcount>many</playcount><lastplayed>never</lastplayed><top250>-</top250></movie>",
        )
        .unwrap();
        assert_eq!(movie.title, "Fine");
        assert_eq!(movie.year, 0);
        assert!(movie.ratings.is_empty());
        assert!(!movie.watched);
        assert!(movie.last_played.is_none());
    }

    #[test]
    fn a_recording_gives_title_plot_certification_and_genres() {
        let movie = read_movie_nfo(
            "<recording><title>Night Show</title><description>Live from the harbor.</description><rating>PG</rating><genre>News</genre><genre>Talk Show</genre></recording>",
        )
        .unwrap();
        assert_eq!(movie.title, "Night Show");
        assert_eq!(movie.plot, "Live from the harbor.");
        assert_eq!(movie.certification.0, "US_PG");
        assert_eq!(movie.genres, [Genre::News, Genre::TalkShow]);
        assert!(movie.ratings.is_empty(), "the rating is a certification");
    }

    #[test]
    fn foreign_xml_without_a_title_is_not_valid() {
        let movie = read_movie_nfo("<movie><year>1999</year></movie>").unwrap();
        assert!(!movie.is_valid());
        assert!(read_movie_nfo("<html><body>x</body></html>").is_none());
    }

    #[test]
    fn a_movie_set_is_read_in_both_layouts() {
        let kodi = read_movie_set_nfo(
            "<collection><title>Harbor Saga</title><sorttitle>Saga, Harbor</sorttitle><overview>All parts.</overview><thumb>https://example.test/p.jpg</thumb><fanart>https://example.test/f.jpg</fanart><uniqueid type=\"tmdb\" default=\"true\">77</uniqueid><tag>t</tag><dateadded>2024-01-01 00:00:00</dateadded><user_note>n</user_note></collection>",
        )
        .unwrap();
        assert!(kodi.is_valid());
        assert_eq!(kodi.overview, "All parts.");
        assert_eq!(kodi.ids.get("tmdb"), Some("77"));
        assert_eq!(kodi.poster, "https://example.test/p.jpg");
        assert_eq!(kodi.fanart, "https://example.test/f.jpg");
        let emby = read_movie_set_nfo(
            "<collection><name>Harbor Saga</name><plot>Plot.</plot><tmdbid>77</tmdbid>",
        )
        .unwrap();
        assert_eq!(emby.title, "Harbor Saga");
        assert_eq!(emby.overview, "Plot.");
        assert_eq!(emby.ids.get("tmdb"), Some("77"));
        assert!(read_movie_set_nfo("nothing").is_none());
        assert_eq!(
            read_movie_set_nfo("https://www.themoviedb.org/collection/77-harbor")
                .unwrap()
                .ids
                .get("tmdb"),
            Some("77")
        );
    }
}

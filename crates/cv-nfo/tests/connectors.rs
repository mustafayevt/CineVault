//! Behaviour of the XBMC, Emby, Jellyfin and MediaPortal movie writers, checked on the rendered
//! text. The byte-exact fixtures run in `cv-compat` (`cargo test -p cv-compat`); these tests pin
//! the rules that differ between the formats, including cases no fixture covers.
#![allow(clippy::field_reassign_with_default)]

use cv_domain::{
    AudioStream, Credit, CreditKind, Edition, Genre, Rating, SubtitleStream, Trailer, id_key,
};
use cv_nfo::{
    MediaPortalStyle, MovieConnector, MovieNfo, MovieNfoOptions, MovieSetRef, Node, VideoInfo,
    movie_document_for, render_movie_nfo_for,
};

const MP_LEGACY: MovieConnector = MovieConnector::MediaPortal(MediaPortalStyle::Legacy);
const MP_MOVING: MovieConnector = MovieConnector::MediaPortal(MediaPortalStyle::MovingPictures);
const MP_MYVIDEO: MovieConnector = MovieConnector::MediaPortal(MediaPortalStyle::MyVideo);

fn movie() -> MovieNfo {
    let mut movie = MovieNfo {
        title: "Blade Runner".to_owned(),
        year: 1982,
        plot: "A blade runner must pursue replicants.".to_owned(),
        top250: 12,
        studios: vec!["Ladd Company".to_owned(), "Shaw Brothers".to_owned()],
        countries: vec!["US".to_owned(), "HK".to_owned()],
        genres: vec![Genre::ScienceFiction, Genre::Thriller],
        tags: vec!["noir".to_owned()],
        spoken_languages: vec!["en".to_owned(), "de".to_owned()],
        edition: Edition::DirectorsCut,
        movie_set: Some(MovieSetRef {
            title: "Blade Runner Collection".to_owned(),
            overview: "Replicants.".to_owned(),
            tmdb_id: Some(422837),
        }),
        credits: vec![
            Credit::new(CreditKind::Writer, "Hampton Fancher", "Writer"),
            Credit::new(CreditKind::Writer, "David Peoples", "Writer"),
            Credit::new(CreditKind::Director, "Ridley Scott", ""),
            Credit::new(CreditKind::Actor, "Harrison Ford", "Deckard"),
            Credit::new(CreditKind::Producer, "Michael Deeley", "Producer"),
        ],
        trailers: vec![Trailer {
            url: "https://www.youtube.com/watch?v=abc123XYZ_-".to_owned(),
            in_nfo: true,
            ..Trailer::default()
        }],
        ..MovieNfo::default()
    };
    movie.ids.set("imdb", "tt0083658");
    movie.ids.set("tmdb", "78");
    movie.ratings.insert(Rating::new("imdb", 8.1, 900, 10));
    movie.artwork.poster = "https://img.example.test/p.jpg".to_owned();
    movie.artwork.fanart = "https://img.example.test/f.jpg".to_owned();
    movie.artwork.extra_fanarts = vec!["https://img.example.test/f2.jpg".to_owned()];
    movie.video = VideoInfo {
        codec: "h264".to_owned(),
        width: 1920,
        height: 1080,
        aspect_ratio: 2.39,
        duration_secs: 7000,
        audio: vec![AudioStream {
            codec: "DTS".to_owned(),
            language: "en".to_owned(),
            channels: 6,
            ..AudioStream::default()
        }],
        subtitles: vec![SubtitleStream {
            language: "de".to_owned(),
            ..SubtitleStream::default()
        }],
        ..VideoInfo::default()
    };
    movie.date_added = Some("2024-03-04 05:06:07".to_owned());
    movie
}

fn names(connector: MovieConnector, movie: &MovieNfo, options: &MovieNfoOptions) -> Vec<String> {
    movie_document_for(connector, movie, options).roots[0]
        .children
        .iter()
        .filter_map(|n| match n {
            Node::Element(e) => Some(e.name.clone()),
            Node::Comment(_) => None,
        })
        .collect()
}

fn position(names: &[String], name: &str) -> usize {
    names
        .iter()
        .position(|n| n == name)
        .unwrap_or_else(|| panic!("no <{name}> in {names:?}"))
}

fn render(connector: MovieConnector, movie: &MovieNfo) -> String {
    render_movie_nfo_for(connector, movie, &MovieNfoOptions::default())
}

// ---- Emby ------------------------------------------------------------------------------------

#[test]
fn emby_writes_the_collection_id_and_puts_credits_after_the_actors() {
    let text = render(MovieConnector::Emby, &movie());
    assert!(text.contains(r#"<set tmdbcolid="422837">"#), "{text}");
    let n = names(MovieConnector::Emby, &movie(), &MovieNfoOptions::default());
    let actor = position(&n, "actor");
    assert!(position(&n, "credits") > actor);
    assert!(position(&n, "director") > actor);
    assert!(position(&n, "producer") > position(&n, "director"));
    assert!(position(&n, "tag") < actor);
    // Everything else follows the Kodi layout, edition elements included.
    assert!(n.contains(&"videoassettitle".to_owned()));
    assert!(n.contains(&"epbookmark".to_owned()));
    assert!(n.contains(&"status".to_owned()));
}

#[test]
fn emby_set_without_an_id_has_no_attribute() {
    let mut m = movie();
    m.movie_set.as_mut().unwrap().tmdb_id = None;
    let text = render(MovieConnector::Emby, &m);
    assert!(text.contains("<set>\r\n"), "{text}");
    assert!(!text.contains("tmdbcolid"));
}

/// Old `<set>` entries are each written back as a set with its TMDB collection ID.
#[test]
fn emby_lists_the_other_sets_of_the_old_nfo_with_their_ids() {
    let mut m = movie();
    m.other_sets = vec![
        MovieSetRef {
            title: "Neo-Noir Classics".to_owned(),
            overview: "Rain and neon.".to_owned(),
            tmdb_id: Some(555),
        },
        MovieSetRef {
            title: "No Id Set".to_owned(),
            overview: String::new(),
            tmdb_id: None,
        },
        // The movie's own set and unnamed entries are not repeated.
        MovieSetRef {
            title: "blade runner collection".to_owned(),
            overview: String::new(),
            tmdb_id: Some(422837),
        },
        MovieSetRef::default(),
    ];
    let text = render(MovieConnector::Emby, &m);
    let sets = text.matches("<set").count();
    assert_eq!(sets, 3, "{text}");
    assert!(text.contains(
        "  <set tmdbcolid=\"555\">\r\n    <name>Neo-Noir Classics</name>\r\n    <overview>Rain and neon.</overview>\r\n  </set>\r\n"
    ));
    assert!(text.contains("  <set>\r\n    <name>No Id Set</name>"));
    assert!(!text.contains("MovieSetRef") && !text.contains("title:"));
    // They follow the movie's own set directly.
    let n = names(MovieConnector::Emby, &m, &MovieNfoOptions::default());
    let first = position(&n, "set");
    assert_eq!(n[first..first + 4], ["set", "set", "set", "plot"]);
}

#[test]
fn only_emby_lists_other_sets() {
    let mut m = movie();
    m.other_sets = vec![MovieSetRef {
        title: "Other".to_owned(),
        overview: String::new(),
        tmdb_id: Some(1),
    }];
    for connector in [
        MovieConnector::Kodi,
        MovieConnector::Jellyfin,
        MovieConnector::Xbmc,
    ] {
        let text = render(connector, &m);
        assert!(!text.contains("Other"), "{connector:?}");
    }
}

// ---- Jellyfin --------------------------------------------------------------------------------

#[test]
fn jellyfin_leaves_out_bookmark_top250_status_and_code() {
    let n = names(
        MovieConnector::Jellyfin,
        &movie(),
        &MovieNfoOptions::default(),
    );
    for gone in ["epbookmark", "top250", "status", "code"] {
        assert!(!n.contains(&gone.to_owned()), "{gone} in {n:?}");
    }
    let kodi = names(MovieConnector::Kodi, &movie(), &MovieNfoOptions::default());
    let expected: Vec<&String> = kodi
        .iter()
        .filter(|n| !["epbookmark", "top250", "status", "code"].contains(&n.as_str()))
        .collect();
    assert_eq!(n.iter().collect::<Vec<_>>(), expected);
}

#[test]
fn jellyfin_writes_the_collection_number_after_the_edition_elements() {
    let mut m = movie();
    assert!(!render(MovieConnector::Jellyfin, &m).contains("collectionnumber"));
    m.ids.set(id_key::TMDB_SET, "422837");
    let text = render(MovieConnector::Jellyfin, &m);
    assert!(
        text.contains("<collectionnumber>422837</collectionnumber>"),
        "{text}"
    );
    let n = names(MovieConnector::Jellyfin, &m, &MovieNfoOptions::default());
    assert_eq!(
        n[position(&n, "hasvideoversions") + 1],
        "collectionnumber",
        "{n:?}"
    );
    // The other formats do not write it.
    for connector in [
        MovieConnector::Kodi,
        MovieConnector::Emby,
        MovieConnector::Xbmc,
        MP_LEGACY,
    ] {
        assert!(!render(connector, &m).contains("collectionnumber"));
    }
}

#[test]
fn the_header_comment_names_the_format() {
    let comment = |connector| {
        movie_document_for(connector, &movie(), &MovieNfoOptions::default())
            .comment
            .unwrap()
    };
    assert!(comment(MovieConnector::Kodi).ends_with("for Kodi"));
    assert!(comment(MovieConnector::Xbmc).ends_with("for XBMC"));
    assert!(comment(MovieConnector::Emby).ends_with("for Emby"));
    assert!(comment(MovieConnector::Jellyfin).ends_with("for Jellyfin"));
    assert!(comment(MP_MYVIDEO).ends_with("for MediaPortal"));
}

// ---- XBMC ------------------------------------------------------------------------------------

#[test]
fn xbmc_writes_one_rating_with_its_votes_and_a_plain_set() {
    let text = render(MovieConnector::Xbmc, &movie());
    for want in [
        "<rating>8.1</rating>",
        "<votes>900</votes>",
        "<top250>12</top250>",
        "<set>Blade Runner Collection</set>",
        "<thumb>https://img.example.test/p.jpg</thumb>",
        "<fanart>https://img.example.test/f.jpg</fanart>",
    ] {
        assert!(text.contains(want), "missing {want} in\n{text}");
    }
    assert!(!text.contains("<ratings>") && !text.contains("overview"));
    assert!(!text.contains("f2.jpg"), "only the main fanart is written");
    assert!(!text.contains("videoassettitle"));
}

#[test]
fn xbmc_without_a_rating_or_set_writes_zero_and_empty_elements() {
    let mut m = movie();
    m.ratings = Default::default();
    m.movie_set = None;
    m.artwork = Default::default();
    let text = render(MovieConnector::Xbmc, &m);
    assert!(text.contains("<rating>0.0</rating>"));
    assert!(text.contains("<votes>0</votes>"));
    assert!(text.contains("<set/>"));
    assert!(text.contains("<thumb/>") && text.contains("<fanart/>"));
}

#[test]
fn xbmc_uses_the_older_trailer_url_and_keeps_other_urls() {
    let text = render(MovieConnector::Xbmc, &movie());
    assert!(text.contains(
        "<trailer>plugin://plugin.video.youtube/?action=play_video&amp;videoid=abc123XYZ_-</trailer>"
    ));
    let mut m = movie();
    m.trailers[0].url = "https://example.test/t.mp4".to_owned();
    assert!(
        render(MovieConnector::Xbmc, &m).contains("<trailer>https://example.test/t.mp4</trailer>")
    );
}

#[test]
fn xbmc_fileinfo_is_plain() {
    let text = render(MovieConnector::Xbmc, &movie());
    for want in [
        "<codec>h264</codec>",
        "<aspect>2.4</aspect>",
        "<durationinseconds>7000</durationinseconds>",
        "<stereomode/>",
        "<codec>DTS</codec>",
        "<language>en</language>",
        "<language>de</language>",
    ] {
        assert!(text.contains(want), "missing {want} in\n{text}");
    }
    assert!(!text.contains("<resolution>") && !text.contains("hdrtype"));
}

#[test]
fn xbmc_fileinfo_reports_a_disc_without_duration() {
    let mut m = movie();
    m.is_disc = true;
    assert!(render(MovieConnector::Xbmc, &m).contains("<durationinseconds>0</durationinseconds>"));
}

#[test]
fn xbmc_order_matches_the_fixtures() {
    let n = names(MovieConnector::Xbmc, &movie(), &MovieNfoOptions::default());
    let wanted = [
        "title",
        "originaltitle",
        "sorttitle",
        "epbookmark",
        "year",
        "rating",
        "userrating",
        "votes",
        "top250",
        "set",
        "plot",
        "tagline",
        "runtime",
        "thumb",
        "fanart",
        "mpaa",
        "certification",
        "id",
        "tmdbid",
        "uniqueid",
    ];
    let mut last = 0;
    for name in wanted {
        let at = position(&n, name);
        assert!(at >= last, "{name} out of order in {n:?}");
        last = at;
    }
    assert!(position(&n, "status") < position(&n, "premiered"));
    assert!(position(&n, "trailer") < position(&n, "languages"));
    assert!(position(&n, "fileinfo") < position(&n, "source"));
}

// ---- MediaPortal -----------------------------------------------------------------------------

#[test]
fn media_portal_joins_names_into_single_elements() {
    for connector in [MP_LEGACY, MP_MOVING, MP_MYVIDEO] {
        let text = render(connector, &movie());
        assert!(
            text.contains("<credits>Hampton Fancher, David Peoples</credits>"),
            "{connector:?}"
        );
        assert!(text.contains("<director>Ridley Scott</director>"));
        assert!(!text.contains("<top250>") && !text.contains("<epbookmark"));
        assert!(!text.contains("<status") && !text.contains("<code/>"));
        assert!(!text.contains("videoassettitle"));
        assert!(text.contains("<trailer>https://www.youtube.com/watch?v=abc123XYZ_-</trailer>"));
    }
}

#[test]
fn media_portal_styles_differ_in_certification_genres_and_languages() {
    let mut m = movie();
    m.certification = cv_domain::Certification("US_R".to_owned());
    let legacy = render(MP_LEGACY, &m);
    assert!(legacy.contains("<certification>R</certification>") && !legacy.contains("<mpaa>"));
    assert!(legacy.contains("<genres>\r\n    <genre>Science Fiction</genre>"));
    assert!(legacy.contains("<languages>English|German</languages>"));
    assert!(legacy.contains("<fanart>\r\n    <thumb>https://img.example.test/f.jpg</thumb>"));

    let moving = render(MP_MOVING, &m);
    assert!(moving.contains("<mpaa>R</mpaa>") && !moving.contains("<certification>"));
    assert!(moving.contains("  <genre>Science Fiction</genre>"));
    assert!(moving.contains("<language>English, German</language>"));
    assert!(moving.contains("<fanart>https://img.example.test/f.jpg</fanart>"));

    let myvideo = render(MP_MYVIDEO, &m);
    assert!(
        myvideo.contains("<mpaa>R</mpaa>") && myvideo.contains("<certification>R</certification>")
    );
    assert!(myvideo.contains("<imdb>tt0083658</imdb>"));
    assert!(myvideo.contains("<language>English|German</language>"));
    assert!(!legacy.contains("<imdb>") && !moving.contains("<imdb>"));
    let n = names(MP_MYVIDEO, &m, &MovieNfoOptions::default());
    assert_eq!(position(&n, "imdb") + 1, position(&n, "id"));
}

#[test]
fn media_portal_countries_and_studios_use_the_style_separator() {
    let m = movie();
    assert!(render(MP_MOVING, &m).contains("<studio>Ladd Company, Shaw Brothers</studio>"));
    assert!(render(MP_MOVING, &m).contains("<country>US, HK</country>"));
    assert!(render(MP_MYVIDEO, &m).contains("<studio>Ladd Company / Shaw Brothers</studio>"));
    assert!(render(MP_MYVIDEO, &m).contains("<country>US / HK</country>"));
    // Legacy joins the list too, until a stored text is given.
    assert!(render(MP_LEGACY, &m).contains("<country>US, HK</country>"));
}

#[test]
fn legacy_media_portal_writes_the_stored_country_and_studio_text() {
    let mut m = movie();
    m.country_text = "US; HK / UK".to_owned();
    m.studio_text = "Ladd Company; Shaw Brothers, Warner".to_owned();
    let legacy = render(MP_LEGACY, &m);
    assert!(legacy.contains("<country>US; HK / UK</country>"));
    assert!(legacy.contains("<studio>Ladd Company; Shaw Brothers, Warner</studio>"));
    // The other styles keep joining the list.
    assert!(render(MP_MOVING, &m).contains("<country>US, HK</country>"));
    // "Only the first studio" wins over the stored text.
    let mut options = MovieNfoOptions::default();
    options.single_studio = true;
    let text = render_movie_nfo_for(MP_LEGACY, &m, &options);
    assert!(text.contains("<studio>Ladd Company</studio>"), "{text}");
}

#[test]
fn media_portal_uses_the_kodi_fileinfo() {
    let text = render(MP_LEGACY, &movie());
    assert!(text.contains("<resolution>1080</resolution>"));
    assert!(text.contains("<aspect>2.40</aspect>"));
    assert!(text.contains("<codec>dca</codec>") && text.contains("<language>eng</language>"));
}

// ---- options, all formats --------------------------------------------------------------------

#[test]
fn options_switch_the_optional_elements_off_in_every_format() {
    let all = [
        MovieConnector::Kodi,
        MovieConnector::Xbmc,
        MovieConnector::Emby,
        MovieConnector::Jellyfin,
        MP_LEGACY,
        MP_MOVING,
        MP_MYVIDEO,
    ];
    let mut options = MovieNfoOptions::default();
    options.write_date_added = false;
    options.trailer = false;
    options.fileinfo = false;
    options.artwork_urls = false;
    for connector in all {
        let n = names(connector, &movie(), &options);
        for gone in ["dateadded", "trailer", "fileinfo", "thumb", "fanart"] {
            assert!(
                !n.contains(&gone.to_owned()),
                "{gone} in {connector:?}: {n:?}"
            );
        }
    }
    options.lockdata = true;
    for connector in all {
        assert!(names(connector, &movie(), &options).contains(&"lockdata".to_owned()));
    }
}

#[test]
fn every_format_ends_with_the_cinevault_tags_and_uses_crlf() {
    for connector in [
        MovieConnector::Kodi,
        MovieConnector::Xbmc,
        MovieConnector::Emby,
        MovieConnector::Jellyfin,
        MP_LEGACY,
        MP_MOVING,
        MP_MYVIDEO,
    ] {
        let text = render(connector, &movie());
        assert!(
            text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n")
        );
        assert!(text.contains("<!--CineVault meta data-->"), "{connector:?}");
        assert!(
            text.ends_with("<cv_locked/>\r\n</movie>\r\n"),
            "{connector:?}"
        );
        assert!(!text.replace("\r\n", "").contains('\n'));
        let n = names(connector, &movie(), &MovieNfoOptions::default());
        assert!(position(&n, "source") > position(&n, "fileinfo"));
    }
}

#[test]
fn unmanaged_elements_come_before_the_private_tags() {
    let mut m = movie();
    m.unknown_elements = vec![cv_nfo::Element::text("customfield", "abc")];
    for connector in [MovieConnector::Xbmc, MP_MOVING, MovieConnector::Emby] {
        let n = names(connector, &m, &MovieNfoOptions::default());
        assert_eq!(position(&n, "customfield") + 1, position(&n, "source"));
    }
}

#[test]
fn trailer_site_links_use_the_hdtrailers_plugin_url_in_every_kodi_family_format() {
    let mut m = movie();
    m.trailers[0].url = "https://www.hd-trailers.net/movie/heat/".to_owned();
    let want = "<trailer>plugin://plugin.video.hdtrailers_net/video/hd-trailers.net/https%3A%2F%2Fwww.hd-trailers.net%2Fmovie%2Fheat%2F</trailer>";
    for connector in [
        MovieConnector::Xbmc,
        MovieConnector::Emby,
        MovieConnector::Jellyfin,
    ] {
        assert!(render(connector, &m).contains(want), "{connector:?}");
    }
    // MediaPortal keeps the stored URL
    for connector in [MP_LEGACY, MP_MOVING, MP_MYVIDEO] {
        assert!(
            render(connector, &m)
                .contains("<trailer>https://www.hd-trailers.net/movie/heat/</trailer>"),
            "{connector:?}"
        );
    }
}

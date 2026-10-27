//! Behaviour of the Kodi movie writer, checked on the rendered text. The byte-exact fixtures run
//! in `cv-compat` (`cargo test -p cv-compat`); these tests pin the rules in the NFO spec.
#![allow(clippy::field_reassign_with_default)]

use cv_domain::{
    AudioStream, CalendarDate, Certification, Credit, CreditKind, Edition, Genre, Rating,
    SubtitleStream, Trailer,
};
use cv_nfo::{
    CertificationStyle, DateAddedField, MovieNfo, MovieNfoOptions, MovieSetRef, VideoInfo,
    movie_document, render_movie_nfo, write_nfo_if_changed,
};

fn movie() -> MovieNfo {
    let mut movie = MovieNfo {
        title: "Blade Runner".to_owned(),
        year: 1982,
        plot: "A blade runner must pursue replicants. He finds more than he expected.".to_owned(),
        ..MovieNfo::default()
    };
    movie.ids.set("imdb", "tt0083658");
    movie.ids.set("tmdb", "78");
    movie.video = VideoInfo {
        codec: "HEVC".to_owned(),
        width: 3840,
        height: 2160,
        aspect_ratio: 1.78,
        duration_secs: 7080,
        hdr_formats: vec!["Dolby Vision".to_owned(), "HDR10".to_owned()],
        audio: vec![AudioStream {
            codec: "DTS-HD MA".to_owned(),
            language: "en".to_owned(),
            channels: 8,
            ..AudioStream::default()
        }],
        subtitles: vec![SubtitleStream {
            language: "de".to_owned(),
            ..SubtitleStream::default()
        }],
        created: Some("2023-01-02 03:04:05".to_owned()),
        modified: Some("2023-02-03 04:05:06".to_owned()),
        ..VideoInfo::default()
    };
    movie.date_added = Some("2024-03-04 05:06:07".to_owned());
    movie
}

fn render(movie: &MovieNfo) -> String {
    render_movie_nfo(movie, &MovieNfoOptions::default())
}

/// Names of the direct children of the root, comments excluded.
fn child_names(movie: &MovieNfo, options: &MovieNfoOptions) -> Vec<String> {
    let doc = movie_document(movie, options);
    doc.roots[0]
        .children
        .iter()
        .filter_map(|n| match n {
            cv_nfo::Node::Element(e) => Some(e.name.clone()),
            cv_nfo::Node::Comment(_) => None,
        })
        .collect()
}

#[test]
fn stream_details_map_codecs_resolution_and_hdr() {
    let text = render(&movie());
    for want in [
        "<codec>hevc</codec>",
        "<resolution>4K</resolution>",
        "<hdrtype>dolbyvision</hdrtype>",
        "<codec>dtshd_ma</codec>",
        "<language>eng</language>",
        "<language>deu</language>",
        "<channels>8</channels>",
        "<durationinseconds>7080</durationinseconds>",
    ] {
        assert!(text.contains(want), "missing {want} in\n{text}");
    }
}

#[test]
fn element_order_follows_the_fixtures() {
    let mut m = movie();
    m.original_title = "Blade Runner".to_owned();
    m.top250 = 12;
    m.movie_set = Some(MovieSetRef {
        title: "Set".to_owned(),
        overview: "About".to_owned(),
        tmdb_id: Some(1),
    });
    m.ratings.insert(Rating::new("imdb", 8.1, 900, 10));
    m.certification = Certification("US_R".to_owned());
    m.genres = vec![Genre::ScienceFiction];
    m.studios = vec!["Ladd Company".to_owned()];
    m.countries = vec!["US".to_owned()];
    m.tags = vec!["noir".to_owned()];
    m.credits = vec![
        Credit::new(CreditKind::Writer, "Hampton Fancher", "Writer"),
        Credit::new(CreditKind::Director, "Ridley Scott", ""),
        Credit::new(CreditKind::Actor, "Harrison Ford", "Deckard"),
        Credit::new(CreditKind::Producer, "Michael Deeley", "Producer"),
    ];
    m.trailers = vec![Trailer {
        url: "https://example.test/t.mp4".to_owned(),
        in_nfo: true,
        ..Trailer::default()
    }];
    m.spoken_languages = vec!["en".to_owned()];
    m.show_links = vec!["A Show".to_owned()];
    m.edition = Edition::DirectorsCut;
    m.watched = true;
    m.play_count = 2;
    m.last_played = Some("2024-01-01 10:00:00".to_owned());
    m.release_date = CalendarDate::new(1982, 6, 25);
    m.outline = "Short outline".to_owned();
    m.artwork.poster = "https://example.test/p.jpg".to_owned();
    m.artwork.fanart = "https://example.test/f.jpg".to_owned();
    let mut options = MovieNfoOptions::default();
    options.lockdata = true;

    let names = child_names(&m, &options);
    let want = [
        "title",
        "originaltitle",
        "sorttitle",
        "epbookmark",
        "year",
        "ratings",
        "userrating",
        "top250",
        "set",
        "plot",
        "outline",
        "tagline",
        "runtime",
        "thumb",
        "fanart",
        "mpaa",
        "certification",
        "id",
        "tmdbid",
        "uniqueid",
        "uniqueid",
        "country",
        "status",
        "code",
        "premiered",
        "watched",
        "playcount",
        "lastplayed",
        "genre",
        "studio",
        "credits",
        "director",
        "tag",
        "actor",
        "producer",
        "trailer",
        "languages",
        "showlink",
        "dateadded",
        "lockdata",
        "fileinfo",
        "videoassettitle",
        "hasvideoversions",
        "source",
        "edition",
        "original_filename",
        "user_note",
        "english_title",
        "crew",
        "crew",
        "crew",
        "cv_locked",
    ];
    assert_eq!(names, want);
}

#[test]
fn a_bare_movie_writes_empty_elements_and_skips_optional_ones() {
    let mut m = MovieNfo {
        title: "Bare".to_owned(),
        ..MovieNfo::default()
    };
    m.date_added = Some("2024-01-01 00:00:00".to_owned());
    let text = render(&m);
    for want in [
        "  <year/>\r\n",
        "  <ratings/>\r\n",
        "  <userrating>0</userrating>\r\n",
        "  <set/>\r\n",
        "  <epbookmark/>\r\n",
        "  <status/>\r\n",
        "  <trailer/>\r\n",
        "  <languages/>\r\n",
        "  <cv_locked/>\r\n",
    ] {
        assert!(text.contains(want), "missing {want:?}");
    }
    for absent in [
        "<top250",
        "<outline",
        "<thumb",
        "<fanart",
        "<lockdata",
        "<lastplayed",
    ] {
        assert!(!text.contains(absent), "unexpected {absent}");
    }
}

#[test]
fn layout_is_utf8_crlf_with_two_space_indent() {
    let text = render(&movie());
    assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n"));
    assert!(text.ends_with("</movie>\r\n"));
    assert!(!text.replace("\r\n", "").contains('\r'));
    // Every line break outside text values is CRLF; the plot here has no line break of its own.
    assert!(!text.replace("\r\n", "").contains('\n'));
    for line in text.split("\r\n").filter(|l| !l.is_empty()) {
        let indent = line.len() - line.trim_start().len();
        assert_eq!(indent % 2, 0, "odd indent: {line:?}");
        assert!(
            line.trim_start().starts_with('<'),
            "text outside an element: {line:?}"
        );
    }
    assert!(text.contains("\r\n  <title>Blade Runner</title>\r\n"));
    assert!(text.contains("\r\n      <video>\r\n        <codec>hevc</codec>\r\n"));
}

#[test]
fn line_breaks_in_the_plot_stay_inside_the_element() {
    let mut m = movie();
    m.plot = "One.\nTwo.".to_owned();
    let text = render(&m);
    assert!(text.contains("<plot>One.\r\nTwo.</plot>"));
}

#[test]
fn text_is_escaped_and_forbidden_characters_dropped() {
    let mut m = movie();
    m.title = "Tom & Jerry <1> \"quoted\" \u{0}".to_owned();
    let text = render(&m);
    assert!(text.contains("<title>Tom &amp; Jerry &lt;1&gt; \"quoted\" </title>"));
}

#[test]
fn the_header_comment_names_cinevault_and_never_another_product() {
    let mut options = MovieNfoOptions::default();
    options.created_on = "2024-05-06 07:08:09".to_owned();
    let text = render_movie_nfo(&movie(), &options);
    assert!(text.contains("<!--created on 2024-05-06 07:08:09 by CineVault "));
    assert!(text.contains("<!--CineVault meta data-->"));
}

#[test]
fn ratings_start_with_the_preferred_source_and_skip_the_user_rating() {
    let mut m = movie();
    m.ratings.insert(Rating::new("imdb", 7.4, 5987, 10));
    m.ratings.insert(Rating::new("themoviedb", 7.7, 56987, 10));
    m.ratings.insert(Rating::new("metacritic", 73.0, 40, 100));
    m.ratings.insert(Rating::new("user", 8.0, 1, 10));
    let text = render(&m);
    let first = text.find("name=\"imdb\"").unwrap();
    assert!(text[..first].ends_with("<rating default=\"true\" max=\"10\" "));
    assert!(text.contains("<rating default=\"false\" max=\"100\" name=\"metacritic\">"));
    assert!(text.contains("<value>73.0</value>"));
    assert!(text.contains("<userrating>8</userrating>"));
    assert!(!text.contains("name=\"user\""));

    let mut options = MovieNfoOptions::default();
    options.rating_sources = vec!["themoviedb".to_owned()];
    let text = render_movie_nfo(&m, &options);
    assert!(text.contains("<rating default=\"true\" max=\"10\" name=\"themoviedb\">"));
    assert!(text.contains("<rating default=\"false\" max=\"10\" name=\"imdb\">"));

    options.rating_sources = vec!["rottentomatoes".to_owned()];
    let text = render_movie_nfo(&m, &options);
    assert!(!text.contains("<rating default=\"true\""));
}

#[test]
fn rating_values_round_half_up_on_their_short_form() {
    let mut m = movie();
    m.ratings.insert(Rating::new("imdb", 6.85, 1200, 10));
    assert!(render(&m).contains("<value>6.9</value>"));
}

#[test]
fn unique_ids_default_to_imdb_then_tvdb_then_tmdb() {
    let mut m = movie();
    let text = render(&m);
    assert!(text.contains("<uniqueid default=\"true\" type=\"imdb\">tt0083658</uniqueid>"));
    assert!(text.contains("<uniqueid default=\"false\" type=\"tmdb\">78</uniqueid>"));
    m.ids.remove("imdb");
    m.ids.set("tvdb", "9");
    let text = render(&m);
    assert!(text.contains("<uniqueid default=\"true\" type=\"tvdb\">9</uniqueid>"));
    m.ids.remove("tvdb");
    assert!(render(&m).contains("<uniqueid default=\"true\" type=\"tmdb\">78</uniqueid>"));
}

#[test]
fn options_switch_elements_off() {
    let mut m = movie();
    m.studios = vec!["A".to_owned(), "B".to_owned()];
    m.artwork.poster = "https://example.test/p.jpg".to_owned();
    m.trailers = vec![Trailer {
        url: "https://example.test/t.mp4".to_owned(),
        in_nfo: true,
        ..Trailer::default()
    }];
    let mut options = MovieNfoOptions::default();
    options.single_studio = true;
    options.artwork_urls = false;
    options.trailer = false;
    options.fileinfo = false;
    options.write_date_added = false;
    let text = render_movie_nfo(&m, &options);
    assert!(text.contains("<studio>A</studio>"));
    assert!(!text.contains("<studio>B</studio>"));
    for absent in ["<thumb", "<trailer", "<fileinfo", "<dateadded"] {
        assert!(!text.contains(absent), "unexpected {absent}");
    }
}

#[test]
fn date_added_can_come_from_the_file_or_the_release_date() {
    let mut m = movie();
    m.release_date = CalendarDate::new(1982, 6, 25);
    let mut options = MovieNfoOptions::default();
    for (field, want) in [
        (DateAddedField::DateAdded, "2024-03-04 05:06:07"),
        (DateAddedField::FileCreationDate, "2023-01-02 03:04:05"),
        (DateAddedField::FileLastModifiedDate, "2023-02-03 04:05:06"),
        (DateAddedField::ReleaseDate, "1982-06-25 00:00:00"),
    ] {
        options.date_added_field = field;
        let text = render_movie_nfo(&m, &options);
        assert!(
            text.contains(&format!("<dateadded>{want}</dateadded>")),
            "{field:?}"
        );
    }
}

#[test]
fn the_outline_is_created_from_the_plot_only_when_asked() {
    let mut m = movie();
    assert!(!render(&m).contains("<outline"));
    let mut options = MovieNfoOptions::default();
    options.create_outline = true;
    let text = render_movie_nfo(&m, &options);
    assert!(text.contains("<outline>A blade runner must pursue replicants</outline>"));
    options.outline_first_sentence = false;
    assert!(render_movie_nfo(&m, &options).contains(
        "<outline>A blade runner must pursue replicants. He finds more than he expected.</outline>"
    ));
    m.outline = "Own outline".to_owned();
    assert!(render_movie_nfo(&m, &options).contains("<outline>Own outline</outline>"));
}

#[test]
fn certification_follows_the_style() {
    let mut m = movie();
    m.certification = Certification("DE_FSK16".to_owned());
    let mut options = MovieNfoOptions::default();
    options.certification_style = CertificationStyle::MediumFull;
    let text = render_movie_nfo(&m, &options);
    assert!(text.contains("<mpaa>Germany: FSK 16</mpaa>"));
    assert!(text.contains("<certification>Germany: FSK 16</certification>"));
}

#[test]
fn genres_and_languages_follow_the_nfo_language() {
    let mut m = movie();
    m.genres = vec![Genre::Comedy, Genre::Crime];
    m.spoken_languages = vec!["de".to_owned(), "fr".to_owned()];
    let mut options = MovieNfoOptions::default();
    options.language = "de".to_owned();
    let text = render_movie_nfo(&m, &options);
    assert!(text.contains("<genre>Komödie</genre>"));
    assert!(text.contains("<genre>Krimi</genre>"));
    assert!(text.contains("<languages>Deutsch, Französisch</languages>"));
}

#[test]
fn crew_carries_ids_and_roles_and_pictures_follow_the_artwork_option() {
    let mut m = movie();
    let mut director = Credit::new(CreditKind::Director, "Ridley Scott", "");
    director.person.ids.set("tmdb", "578");
    director.person.thumb_url = "https://example.test/rs.jpg".to_owned();
    m.credits = vec![director];
    let text = render(&m);
    assert!(text.contains("<director tmdbid=\"578\">Ridley Scott</director>"));
    assert!(text.contains(
        "<crew>\r\n    <name>Ridley Scott</name>\r\n    <role>DIRECTOR</role>\r\n    <thumb>https://example.test/rs.jpg</thumb>\r\n    <tmdbid>578</tmdbid>\r\n  </crew>"
    ));
    let mut options = MovieNfoOptions::default();
    options.artwork_urls = false;
    assert!(!render_movie_nfo(&m, &options).contains("rs.jpg"));
}

#[test]
fn locked_fields_fill_the_lock_element() {
    let mut m = movie();
    m.locked_fields = vec!["title".to_owned(), "plot".to_owned()];
    assert!(render(&m).contains("<cv_locked>title,plot</cv_locked>"));
}

#[test]
fn rewriting_an_unchanged_movie_leaves_the_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Blade Runner (1982).nfo");
    let mut options = MovieNfoOptions::default();
    options.created_on = "2024-01-01 00:00:00".to_owned();
    let first = render_movie_nfo(&movie(), &options);
    assert!(write_nfo_if_changed(&path, &first).unwrap());
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(30));
    options.created_on = "2025-06-07 08:09:10".to_owned();
    let again = render_movie_nfo(&movie(), &options);
    assert_ne!(first, again, "only the header comment differs");
    assert!(!write_nfo_if_changed(&path, &again).unwrap());
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );

    let mut changed = movie();
    changed.tagline = "More human than human".to_owned();
    assert!(write_nfo_if_changed(&path, &render_movie_nfo(&changed, &options)).unwrap());
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("More human than human")
    );
}

#[test]
fn a_person_without_a_role_gets_no_role_element() {
    let mut movie = movie();
    movie.credits = vec![
        Credit::new(CreditKind::Actor, "Meguru Kato", ""),
        Credit::new(CreditKind::Actor, "Koji Yakusho", "Shinzaemon Shimada"),
        Credit::new(CreditKind::Producer, "Jeremy Thomas", "  "),
    ];
    let text = render(&movie);
    assert!(!text.contains("<role/>"), "{text}");
    assert!(text.contains("  <actor>\r\n    <name>Meguru Kato</name>\r\n  </actor>\r\n"));
    assert!(text.contains("<role>Shinzaemon Shimada</role>"));
    assert!(text.contains("  <producer>\r\n    <name>Jeremy Thomas</name>\r\n  </producer>\r\n"));
}

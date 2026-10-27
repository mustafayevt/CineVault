//! Round trips through the old-NFO reader: custom elements and carried-over values survive a
//! rewrite in every format, a clean NFO drops them, and rewriting the result changes nothing.
//! The byte-exact fixtures run in `cv-compat`; these tests pin the behaviour of the spec
//! requirement "Preserve unknown and foreign content".
#![allow(clippy::field_reassign_with_default)]

use cv_nfo::{
    EpisodeNfo, MovieConnector, MovieNfo, MovieNfoOptions, MovieSetRef, SeasonNfo, ShowNfo,
    TvConnector, TvNfoOptions, apply_old_episode_nfo, apply_old_movie_nfo, apply_old_season_nfo,
    apply_old_show_nfo, content_differs, parse_roots, read_existing_nfo, render_episode_nfo,
    render_movie_nfo_for, render_season_nfo, render_show_nfo, write_nfo_if_changed,
};

const CUSTOM: &str = "  <customfield>abc</customfield>\r\n  <nested>\r\n    <inner a=\"1\" b=\"x&amp;y\">value &lt;1&gt;</inner>\r\n    <empty/>\r\n  </nested>\r\n";

fn old_movie_nfo() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<!--created on 2020-01-01 by someone-->\r\n<movie>\r\n  <title>Old Title</title>\r\n  <epbookmark>1234.5</epbookmark>\r\n  <outline>Old outline.</outline>\r\n  <playcount>7</playcount>\r\n  <lastplayed>2025-01-02 03:04:05</lastplayed>\r\n  <status>Released</status>\r\n  <code>ABC-123</code>\r\n{CUSTOM}</movie>\r\n"
    )
}

fn fresh_movie() -> MovieNfo {
    let mut movie = MovieNfo {
        title: "Harbor Lights".to_owned(),
        year: 1992,
        plot: "A sailor finds a lamp.".to_owned(),
        watched: true,
        play_count: 3,
        last_played: Some("2024-05-06 07:08:09".to_owned()),
        date_added: Some("2024-01-01 00:00:00".to_owned()),
        ..MovieNfo::default()
    };
    movie.ids.set("imdb", "tt0103639");
    movie
}

/// Writes the NFO to disk, reads it back the way a rewrite does, and renders again.
fn rewrite(connector: MovieConnector, old: &str, options: &MovieNfoOptions) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Harbor Lights (1992).nfo");
    write_nfo_if_changed(&path, old).unwrap();
    let text = read_existing_nfo(&[path]).unwrap();
    let mut movie = fresh_movie();
    apply_old_movie_nfo(&mut movie, &text, options);
    render_movie_nfo_for(connector, &movie, options)
}

const ALL_MOVIE_CONNECTORS: [MovieConnector; 5] = [
    MovieConnector::Kodi,
    MovieConnector::Xbmc,
    MovieConnector::Emby,
    MovieConnector::Jellyfin,
    MovieConnector::MediaPortal(cv_nfo::MediaPortalStyle::Legacy),
];

fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

#[test]
fn a_custom_element_survives_a_rescrape_in_every_format() {
    for connector in ALL_MOVIE_CONNECTORS {
        let text = rewrite(connector, &old_movie_nfo(), &MovieNfoOptions::default());
        assert!(
            text.contains("<customfield>abc</customfield>\r\n"),
            "{connector:?}: {text}"
        );
        assert!(
            text.contains(
                "  <nested>\r\n    <inner a=\"1\" b=\"x&amp;y\">value &lt;1&gt;</inner>\r\n    <empty/>\r\n  </nested>\r\n"
            ),
            "{connector:?}: {text}"
        );
        // The scraped title wins; the old one is not written as an unmanaged element.
        assert_eq!(count(&text, "<title>"), 1, "{connector:?}");
        assert!(text.contains("<title>Harbor Lights</title>"));
        // Unmanaged elements come after the managed ones and before the private tags.
        let custom = text.find("<customfield>").unwrap();
        assert!(text.find("<title>").unwrap() < custom);
        assert!(custom < text.find("meta data").unwrap(), "{connector:?}");
    }
}

#[test]
fn carried_over_values_are_kept_unless_the_nfo_is_clean() {
    let kept = rewrite(
        MovieConnector::Kodi,
        &old_movie_nfo(),
        &MovieNfoOptions::default(),
    );
    assert!(kept.contains("<epbookmark>1234.5</epbookmark>"));
    assert!(kept.contains("<outline>Old outline.</outline>"));
    assert!(kept.contains("<status>Released</status>"));
    assert!(kept.contains("<code>ABC-123</code>"));
    // The higher play count and the later last-played time win.
    assert!(kept.contains("<playcount>7</playcount>"));
    assert!(kept.contains("<lastplayed>2025-01-02 03:04:05</lastplayed>"));

    let clean = rewrite(
        MovieConnector::Kodi,
        &old_movie_nfo(),
        &MovieNfoOptions {
            clean_nfo: true,
            ..MovieNfoOptions::default()
        },
    );
    assert!(!clean.contains("customfield"));
    assert!(!clean.contains("nested"));
    assert!(!clean.contains("1234.5"));
    assert!(!clean.contains("Old outline."));
    assert!(!clean.contains("ABC-123"));
    assert!(clean.contains("<epbookmark/>"));
    assert!(clean.contains("<playcount>3</playcount>"));
    assert!(clean.contains("<lastplayed>2024-05-06 07:08:09</lastplayed>"));
}

#[test]
fn the_stored_values_win_when_they_are_higher_or_later() {
    let old = "<movie><title>x</title><playcount>1</playcount><lastplayed>2020-01-01 00:00:00</lastplayed></movie>";
    let mut movie = fresh_movie();
    apply_old_movie_nfo(&mut movie, old, &MovieNfoOptions::default());
    let text = render_movie_nfo_for(MovieConnector::Kodi, &movie, &MovieNfoOptions::default());
    assert!(text.contains("<playcount>3</playcount>"));
    assert!(text.contains("<lastplayed>2024-05-06 07:08:09</lastplayed>"));
}

#[test]
fn an_unwatched_movie_does_not_take_the_old_play_state() {
    let mut movie = fresh_movie();
    movie.watched = false;
    movie.play_count = 0;
    movie.last_played = None;
    apply_old_movie_nfo(&mut movie, &old_movie_nfo(), &MovieNfoOptions::default());
    let text = render_movie_nfo_for(MovieConnector::Kodi, &movie, &MovieNfoOptions::default());
    assert!(text.contains("<watched>false</watched>"));
    assert!(text.contains("<playcount>0</playcount>"));
    assert!(!text.contains("<lastplayed>"));
}

#[test]
fn rewriting_a_rewritten_nfo_changes_nothing() {
    for connector in ALL_MOVIE_CONNECTORS {
        let options = MovieNfoOptions::default();
        let first = rewrite(connector, &old_movie_nfo(), &options);
        let second = rewrite(connector, &first, &options);
        assert!(
            !content_differs(&first, &second),
            "{connector:?} changed on the second rewrite:\n{first}\n---\n{second}"
        );
    }
}

#[test]
fn rewriting_an_unchanged_nfo_leaves_the_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Harbor Lights (1992).nfo");
    let options = MovieNfoOptions::default();
    let render = |old: Option<&str>| {
        let mut movie = fresh_movie();
        if let Some(old) = old {
            apply_old_movie_nfo(&mut movie, old, &options);
        }
        render_movie_nfo_for(MovieConnector::Kodi, &movie, &options)
    };
    assert!(write_nfo_if_changed(&path, &render(Some(&old_movie_nfo()))).unwrap());
    let written = read_existing_nfo(std::slice::from_ref(&path)).unwrap();
    assert!(!write_nfo_if_changed(&path, &render(Some(&written))).unwrap());
}

#[test]
fn a_file_without_a_movie_root_or_with_broken_xml_is_handled() {
    let options = MovieNfoOptions::default();
    let mut movie = fresh_movie();
    assert!(!apply_old_movie_nfo(
        &mut movie,
        "<tvshow><title>x</title><customfield>no</customfield></tvshow>",
        &options
    ));
    assert!(movie.unknown_elements.is_empty());
    // A truncated file keeps what was read before the break.
    let mut movie = fresh_movie();
    assert!(apply_old_movie_nfo(
        &mut movie,
        "<movie><title>x</title><customfield>abc</customfield><plot>cut o",
        &options
    ));
    assert_eq!(movie.unknown_elements.len(), 1);
    assert_eq!(movie.unknown_elements[0].text, "abc");
}

#[test]
fn the_emby_writer_lists_the_other_sets_of_the_old_nfo() {
    let old = "<movie><title>x</title><set tmdbcolid=\"2344\"><name>Own</name><overview>o</overview></set><set tmdbcolid=\"77\"><name>Other</name></set><set>Text only</set></movie>";
    let mut movie = fresh_movie();
    movie.movie_set = Some(MovieSetRef {
        title: "Own".to_owned(),
        overview: "o".to_owned(),
        tmdb_id: Some(2344),
    });
    let options = MovieNfoOptions::default();
    apply_old_movie_nfo(&mut movie, old, &options);
    let text = render_movie_nfo_for(MovieConnector::Emby, &movie, &options);
    assert_eq!(count(&text, "<set "), 2, "{text}");
    assert!(text.contains("<set tmdbcolid=\"77\">"));
    assert!(text.contains("<name>Other</name>"));
    assert!(text.contains("<set tmdbcolid=\"2344\">"));
}

// ---- TV ---------------------------------------------------------------------------------------

fn tv_options(clean: bool) -> TvNfoOptions {
    let mut options = TvNfoOptions::default();
    options.common.clean_nfo = clean;
    options.new_episodeguide_style = false;
    options
}

fn fresh_show() -> ShowNfo {
    ShowNfo {
        title: "Lighthouse Keepers".to_owned(),
        watched: true,
        ..ShowNfo::default()
    }
}

#[test]
fn a_show_keeps_its_custom_elements_and_old_episodeguide() {
    let old = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<tvshow>\r\n  <title>Old Show</title>\r\n  <episodeguide><url>https://example.test/guide</url></episodeguide>\r\n  <playcount>4</playcount>\r\n  <customfield>kept</customfield>\r\n  <extra x=\"1\"><deep>y</deep></extra>\r\n</tvshow>\r\n";
    let options = tv_options(false);
    let mut show = fresh_show();
    assert!(apply_old_show_nfo(&mut show, old, &options.common));
    let text = render_show_nfo(TvConnector::Kodi, &show, &options);
    assert!(text.contains("<customfield>kept</customfield>"));
    assert!(text.contains("<extra x=\"1\">\r\n    <deep>y</deep>\r\n  </extra>"));
    assert!(text.contains("https://example.test/guide"));
    assert!(text.contains("<playcount>4</playcount>"));
    assert_eq!(count(&text, "<title>"), 1);

    let again = {
        let mut show = fresh_show();
        apply_old_show_nfo(&mut show, &text, &options.common);
        render_show_nfo(TvConnector::Kodi, &show, &options)
    };
    assert!(!content_differs(&text, &again));

    let clean = tv_options(true);
    let mut show = fresh_show();
    assert!(!apply_old_show_nfo(&mut show, old, &clean.common));
    let text = render_show_nfo(TvConnector::Kodi, &show, &clean);
    assert!(!text.contains("customfield"));
    assert!(!text.contains("example.test"));
}

#[test]
fn a_season_keeps_its_custom_elements() {
    let old = "<season><seasonnumber>2</seasonnumber><title>Two</title><customfield>kept</customfield></season>";
    let options = tv_options(false);
    let show = fresh_show();
    let mut season = SeasonNfo {
        number: 2,
        ..SeasonNfo::default()
    };
    assert!(apply_old_season_nfo(&mut season, old, &options.common));
    let text = render_season_nfo(&show, &season, &options);
    assert!(text.contains("<customfield>kept</customfield>"));
    assert_eq!(count(&text, "<title>"), 1);
}

#[test]
fn a_multi_episode_file_keeps_each_blocks_custom_elements_and_values() {
    let old = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<episodedetails>\r\n  <title>One</title>\r\n  <season>1</season>\r\n  <episode>1</episode>\r\n  <epbookmark>10</epbookmark>\r\n  <code>C1</code>\r\n  <playcount>2</playcount>\r\n  <lastplayed>2025-02-02 00:00:00</lastplayed>\r\n  <custom>first</custom>\r\n</episodedetails>\r\n<episodedetails>\r\n  <title>Two</title>\r\n  <season>1</season>\r\n  <episode>2</episode>\r\n  <custom>second</custom>\r\n</episodedetails>\r\n";
    let options = tv_options(false);
    let mut episodes = vec![
        EpisodeNfo {
            title: "New One".to_owned(),
            season: 1,
            episode: 1,
            ..EpisodeNfo::default()
        },
        EpisodeNfo {
            title: "New Two".to_owned(),
            season: 1,
            episode: 2,
            ..EpisodeNfo::default()
        },
    ];
    assert_eq!(
        apply_old_episode_nfo(&mut episodes, old, &options.common),
        2
    );
    let text = render_episode_nfo(TvConnector::Kodi, &episodes, &options);
    assert_eq!(count(&text, "<?xml"), 1);
    assert!(text.contains("<custom>first</custom>"));
    assert!(text.contains("<custom>second</custom>"));
    assert!(text.find("<custom>first").unwrap() < text.find("<custom>second").unwrap());
    assert!(text.contains("<epbookmark>10</epbookmark>"));
    assert!(text.contains("<code>C1</code>"));
    let roots = parse_roots(&text);
    assert_eq!(roots.len(), 2);
    assert_eq!(roots[0].find("title").unwrap().text, "New One");
    assert_eq!(roots[1].find("custom").unwrap().text, "second");
}

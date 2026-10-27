//! Behaviour of the TV show, season and episode writers, checked on the rendered text. The
//! byte-exact fixtures run in `cv-compat` (`cargo test -p cv-compat`); these tests pin the rules in
//! the NFO and TV specs, including cases no fixture covers.
#![allow(clippy::field_reassign_with_default)]

use cv_domain::{
    AiredStatus, AudioStream, CalendarDate, Certification, Credit, CreditKind, Edition,
    EpisodeGroup, EpisodeNumber, EpisodeOrdering, Genre, Rating, Trailer,
};
use cv_nfo::{
    AirDate, EpisodeNfo, EpisodeguideKeys, Node, SeasonNfo, SeasonNfoNaming, ShowNfo, TvConnector,
    TvNfoOptions, VideoInfo, content_differs, episode_document, render_episode_nfo,
    render_season_nfo, render_show_nfo, season_nfo_paths, show_document, split_episode_list,
    split_show_list,
};

fn date(text: &str) -> CalendarDate {
    text.parse().expect("valid date")
}

fn show() -> ShowNfo {
    let mut show = ShowNfo {
        title: "Lighthouse Keepers".to_owned(),
        original_title: "The Lighthouse Keepers".to_owned(),
        year: 2015,
        plot: "Two keepers, one rock. Endless weather.".to_owned(),
        runtime_minutes: 45,
        top250: 17,
        certification: Certification("US_TVPG".to_owned()),
        genres: vec![Genre::Drama],
        countries: split_show_list("US, CA"),
        studios: split_show_list("Beacon Network, Tide TV"),
        first_aired: Some(date("2015-03-10")),
        status: AiredStatus::Continuing,
        date_added: Some("2024-02-03 04:05:06".to_owned()),
        last_scraper_id: "tvdb".to_owned(),
        ..ShowNfo::default()
    };
    show.ids.set("tvdb", "81189");
    show.ids.set("tmdb", "1396");
    show.ids.set("imdb", "tt0903747");
    show.ratings.insert(Rating::new("tvdb", 8.8, 100, 10));
    show.ratings.insert(Rating::new("imdb", 9.5, 2000, 10));
    show
}

fn options() -> TvNfoOptions {
    TvNfoOptions::default()
}

fn episode(season: u32, number: u32) -> EpisodeNfo {
    EpisodeNfo {
        title: format!("Episode {number}"),
        show_title: "Lighthouse Keepers".to_owned(),
        season,
        episode: number,
        first_aired: Some(date("2015-03-10")),
        runtime_minutes: 44,
        date_added: Some("2024-01-01 00:00:00".to_owned()),
        original_filename: "show.mkv".to_owned(),
        video: VideoInfo {
            codec: "AVC".to_owned(),
            width: 1920,
            height: 1080,
            aspect_ratio: 1.78,
            duration_secs: 2700,
            audio: vec![AudioStream {
                codec: "AC3".to_owned(),
                language: "en".to_owned(),
                channels: 6,
                ..AudioStream::default()
            }],
            ..VideoInfo::default()
        },
        ..EpisodeNfo::default()
    }
}

fn lines(text: &str) -> Vec<&str> {
    text.split("\r\n").collect()
}

fn has(text: &str, line: &str) -> bool {
    lines(text).iter().any(|l| l.trim() == line)
}

fn season(number: u32, title: &str, plot: &str) -> SeasonNfo {
    SeasonNfo {
        number,
        title: title.to_owned(),
        plot: plot.to_owned(),
        ..SeasonNfo::default()
    }
}

// ---- layout, formats --------------------------------------------------------------------------

#[test]
fn show_nfo_is_utf8_crlf_with_two_space_indent() {
    let text = render_show_nfo(TvConnector::Kodi, &show(), &options());
    assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n"));
    assert!(text.ends_with("</tvshow>\r\n"));
    assert!(!text.replace("\r\n", "").contains('\n'));
    assert!(text.contains("\r\n  <title>Lighthouse Keepers</title>\r\n"));
}

#[test]
fn header_comment_names_the_generator_and_the_format() {
    let mut options = options();
    options.common.created_on = "2024-05-06 07:08:09".to_owned();
    let text = render_show_nfo(TvConnector::Emby, &show(), &options);
    let comment = lines(&text)[1].to_owned();
    assert!(comment.starts_with("<!--created on 2024-05-06 07:08:09 by CineVault "));
    assert!(comment.ends_with(" for Emby-->"));
}

#[test]
fn plex_writes_the_kodi_format_and_media_portal_the_xbmc_show() {
    let show = show();
    let kodi = render_show_nfo(TvConnector::Kodi, &show, &options());
    let plex = render_show_nfo(TvConnector::parse("PLEX").unwrap(), &show, &options());
    assert_eq!(content_free(&kodi), content_free(&plex));

    let portal = render_show_nfo(TvConnector::MediaPortal, &show, &options());
    let xbmc = render_show_nfo(TvConnector::Xbmc, &show, &options());
    assert_eq!(content_free(&portal), content_free(&xbmc));
    assert!(has(&portal, "<rating>8.8</rating>"));
    assert!(has(&portal, "<votes>100</votes>"));
    assert!(!portal.contains("<ratings>"));
    assert!(!portal.contains("<top250>"));
    assert!(has(&kodi, "<top250>17</top250>"));
    assert!(kodi.contains("<ratings>"));
}

/// The text without comment lines, so two renders compare by content.
fn content_free(text: &str) -> String {
    cv_nfo::xml::strip_comments(text)
}

#[test]
fn media_portal_episodes_use_the_kodi_layout() {
    let mut ep = episode(1, 1);
    ep.ratings.insert(Rating::new("tvdb", 8.2, 10, 10));
    let portal = render_episode_nfo(TvConnector::MediaPortal, &[ep.clone()], &options());
    let kodi = render_episode_nfo(TvConnector::Kodi, &[ep.clone()], &options());
    let xbmc = render_episode_nfo(TvConnector::Xbmc, &[ep], &options());
    assert_eq!(content_free(&portal), content_free(&kodi));
    assert!(xbmc.contains("<rating>8.2</rating>") && !xbmc.contains("<ratings>"));
}

#[test]
fn connector_names_parse() {
    assert_eq!(TvConnector::parse("kodi"), Some(TvConnector::Kodi));
    assert_eq!(TvConnector::parse("JELLYFIN"), Some(TvConnector::Jellyfin));
    assert_eq!(TvConnector::parse("PLEX"), Some(TvConnector::Kodi));
    assert_eq!(
        TvConnector::parse("MEDIAPORTAL"),
        Some(TvConnector::MediaPortal)
    );
    assert_eq!(TvConnector::parse("nope"), None);
}

// ---- episodeguide ----------------------------------------------------------------------------

#[test]
fn new_style_episodeguide_lists_the_ids_as_json() {
    let text = render_show_nfo(TvConnector::Kodi, &show(), &options());
    assert!(has(
        &text,
        r#"<episodeguide>{"tmdb":"1396","imdb":"tt0903747","tvdb":"81189"}</episodeguide>"#
    ));
    let mut empty = ShowNfo::default();
    empty.title = "x".to_owned();
    assert!(has(
        &render_show_nfo(TvConnector::Kodi, &empty, &options()),
        "<episodeguide>{}</episodeguide>"
    ));
}

#[test]
fn episodeguide_can_be_left_out() {
    let mut options = options();
    options.write_episodeguide = false;
    let text = render_show_nfo(TvConnector::Kodi, &show(), &options);
    assert!(!text.contains("episodeguide"));
}

fn old_style() -> TvNfoOptions {
    let mut options = options();
    options.new_episodeguide_style = false;
    options.episodeguide_keys = EpisodeguideKeys {
        tvdb: "KEY-A".to_owned(),
        tmdb: "KEY-B".to_owned(),
    };
    options
}

#[test]
fn old_style_episodeguide_uses_the_provider_of_the_last_scraper() {
    let mut show = show();
    let tvdb = render_show_nfo(TvConnector::Kodi, &show, &old_style());
    assert!(has(
        &tvdb,
        r#"<url cache="auth.json" post="yes">https://api.thetvdb.com/login?{"apikey":"KEY-A","id":81189}|Content-Type=application/json</url>"#
    ));

    show.last_scraper_id = "tmdb".to_owned();
    let tmdb = render_show_nfo(TvConnector::Kodi, &show, &old_style());
    assert!(has(
        &tmdb,
        "<url>http://api.themoviedb.org/3/tv/1396?api_key=KEY-B&amp;language=en</url>"
    ));
}

#[test]
fn old_style_episodeguide_falls_back_to_the_available_ids() {
    let mut show = show();
    show.last_scraper_id = String::new();
    // TVDB wins over TMDB when the last scraper decides nothing
    let text = render_show_nfo(TvConnector::Kodi, &show, &old_style());
    assert!(text.contains("api.thetvdb.com"));

    show.ids.remove("tvdb");
    let text = render_show_nfo(TvConnector::Kodi, &show, &old_style());
    assert!(text.contains("api.themoviedb.org/3/tv/1396"));

    // a last scraper without an ID of its own is not used
    show.last_scraper_id = "tvdb".to_owned();
    let text = render_show_nfo(TvConnector::Kodi, &show, &old_style());
    assert!(text.contains("api.themoviedb.org/3/tv/1396"));
}

#[test]
fn old_style_episodeguide_without_ids_keeps_the_carried_element_or_nothing() {
    let mut show = show();
    show.ids.remove("tvdb");
    show.ids.remove("tmdb");
    let none = render_show_nfo(TvConnector::Kodi, &show, &old_style());
    assert!(!none.contains("episodeguide"));

    show.carried.episodeguide = Some(
        cv_nfo::Element::new("episodeguide")
            .child(cv_nfo::Element::text("url", "https://example.test/guide")),
    );
    let kept = render_show_nfo(TvConnector::Kodi, &show, &old_style());
    assert!(kept.contains("<url>https://example.test/guide</url>"));
}

// ---- namedseason, seasonplot, season thumbs ------------------------------------------------

#[test]
fn show_nfo_lists_season_names_plots_and_pictures() {
    let mut show = show();
    let mut one = season(1, "The First Winter", "Season one plot");
    one.artwork.poster = "https://img.test/s1.jpg".to_owned();
    one.artwork.banner = "https://img.test/s1b.jpg".to_owned();
    let mut two = season(2, "", "");
    two.artwork.poster = "https://img.test/s2.jpg".to_owned();
    show.seasons = vec![one, two];
    let text = render_show_nfo(TvConnector::Kodi, &show, &options());
    assert!(has(
        &text,
        r#"<namedseason number="1">The First Winter</namedseason>"#
    ));
    assert!(has(
        &text,
        r#"<seasonplot number="1">Season one plot</seasonplot>"#
    ));
    assert!(!text.contains(r#"<namedseason number="2""#));
    assert!(!text.contains(r#"<seasonplot number="2""#));

    // posters of all seasons come before the banners
    let poster_1 = text
        .find(r#"season="1" type="season">https://img.test/s1.jpg"#)
        .unwrap();
    let poster_2 = text
        .find(r#"season="2" type="season">https://img.test/s2.jpg"#)
        .unwrap();
    let banner_1 = text
        .find(r#"season="1" type="season">https://img.test/s1b.jpg"#)
        .unwrap();
    assert!(poster_1 < poster_2 && poster_2 < banner_1);
    // names and plots stay with the artwork option off; the pictures go
    let mut off = options();
    off.common.artwork_urls = false;
    let text = render_show_nfo(TvConnector::Kodi, &show, &off);
    assert!(text.contains("namedseason") && text.contains("seasonplot"));
    assert!(!text.contains("type=\"season\""));
}

#[test]
fn season_nfo_has_a_default_title_and_the_ids_of_the_show() {
    let show = show();
    let mut s = season(3, "", "Plot");
    s.first_aired = Some(date("2017-01-02"));
    s.artwork.thumb = "https://img.test/t.jpg".to_owned();
    s.artwork.fanart = "https://img.test/f.jpg".to_owned();
    let text = render_season_nfo(&show, &s, &options());
    for line in [
        "<seasonnumber>3</seasonnumber>",
        "<title>Season 3</title>",
        "<showtitle>Lighthouse Keepers</showtitle>",
        "<sorttitle>Season 03</sorttitle>",
        "<year/>",
        "<plot>Plot</plot>",
        r#"<thumb aspect="landscape">https://img.test/t.jpg</thumb>"#,
        "<tvdbid>81189</tvdbid>",
        "<imdbid>tt0903747</imdbid>",
        "<tmdbid>1396</tmdbid>",
        r#"<uniqueid type="tvdb">81189</uniqueid>"#,
        "<premiered>2017-01-02</premiered>",
        "<user_note/>",
    ] {
        assert!(has(&text, line), "missing {line}\n{text}");
    }
    assert!(!text.contains("default="));
    assert!(text.contains("<fanart>\r\n    <thumb>https://img.test/f.jpg</thumb>\r\n  </fanart>"));

    let named = render_season_nfo(&show, &season(0, "Specials", ""), &options());
    assert!(has(&named, "<title>Specials</title>"));
    assert!(has(&named, "<sorttitle>Season 00</sorttitle>"));
}

#[test]
fn season_nfo_paths_follow_the_naming_choices() {
    let both = [SeasonNfoNaming::Season, SeasonNfoNaming::SeasonFolder];
    assert_eq!(
        season_nfo_paths(&both, 0, Some("Specials"), true),
        ["season-specials.nfo", "Specials/season.nfo"]
    );
}

// ---- status, enddate -------------------------------------------------------------------------

fn ended_show() -> ShowNfo {
    let mut show = show();
    show.status = AiredStatus::Ended;
    show.episode_air_dates = vec![
        AirDate {
            season: 1,
            date: date("2016-05-01"),
        },
        AirDate {
            season: 2,
            date: date("2017-06-02"),
        },
        AirDate {
            season: 0,
            date: date("2018-12-24"),
        },
    ];
    show
}

#[test]
fn ended_show_writes_the_end_date_for_emby_and_jellyfin() {
    let emby = render_show_nfo(TvConnector::Emby, &ended_show(), &options());
    assert!(has(&emby, "<enddate>2018-12-24</enddate>"));
    assert!(has(&emby, "<status>Ended</status>"));

    // Jellyfin ignores the specials
    let jellyfin = render_show_nfo(TvConnector::Jellyfin, &ended_show(), &options());
    assert!(has(&jellyfin, "<enddate>2017-06-02</enddate>"));
}

#[test]
fn end_date_is_left_out_when_it_does_not_apply() {
    let ended = ended_show();
    for connector in [
        TvConnector::Kodi,
        TvConnector::Xbmc,
        TvConnector::MediaPortal,
    ] {
        assert!(!render_show_nfo(connector, &ended, &options()).contains("enddate"));
    }
    let mut off = options();
    off.write_date_ended = false;
    assert!(!render_show_nfo(TvConnector::Emby, &ended, &off).contains("enddate"));

    let mut running = ended_show();
    running.status = AiredStatus::Continuing;
    assert!(!render_show_nfo(TvConnector::Emby, &running, &options()).contains("enddate"));

    let mut no_dates = ended_show();
    no_dates.episode_air_dates.clear();
    assert!(!render_show_nfo(TvConnector::Emby, &no_dates, &options()).contains("enddate"));
}

#[test]
fn end_date_comes_after_the_lock_element() {
    let mut options = options();
    options.common.lockdata = true;
    let text = render_show_nfo(TvConnector::Emby, &ended_show(), &options);
    let lock = text.find("<lockdata>true</lockdata>").unwrap();
    let end = text.find("<enddate>").unwrap();
    assert!(lock < end);
}

#[test]
fn status_and_watch_state_are_written() {
    let mut show = show();
    for (status, name) in [
        (AiredStatus::Continuing, "Continuing"),
        (AiredStatus::Ended, "Ended"),
        (AiredStatus::Canceled, "Canceled"),
        (AiredStatus::Unknown, "Unknown"),
    ] {
        show.status = status;
        let text = render_show_nfo(TvConnector::Kodi, &show, &options());
        assert!(has(&text, &format!("<status>{name}</status>")));
    }
    assert!(has(&text_of(&show), "<watched>false</watched>"));
    assert!(has(&text_of(&show), "<playcount/>"));
    show.watched = true;
    assert!(has(&text_of(&show), "<watched>true</watched>"));
    assert!(has(&text_of(&show), "<playcount>1</playcount>"));
    show.carried.play_count = 4;
    assert!(has(&text_of(&show), "<playcount>4</playcount>"));
}

fn text_of(show: &ShowNfo) -> String {
    render_show_nfo(TvConnector::Kodi, show, &options())
}

// ---- show details ----------------------------------------------------------------------------

#[test]
fn show_ids_default_to_tvdb_then_imdb_then_tmdb() {
    let mut show = show();
    let text = text_of(&show);
    assert!(text.contains(r#"<uniqueid default="true" type="tvdb">81189</uniqueid>"#));
    show.ids.remove("tvdb");
    assert!(text_of(&show).contains(r#"<uniqueid default="true" type="imdb">"#));
    show.ids.remove("imdb");
    assert!(text_of(&show).contains(r#"<uniqueid default="true" type="tmdb">"#));
    assert!(has(&text_of(&show), "<id/>"));
}

#[test]
fn show_ratings_follow_the_source_order_with_a_fallback() {
    let mut options = options();
    options.common.rating_sources = vec!["imdb".to_owned(), "tvdb".to_owned()];
    let text = render_show_nfo(TvConnector::Kodi, &show(), &options);
    let imdb = text
        .find(r#"<rating default="true" max="10" name="imdb">"#)
        .unwrap();
    let tvdb = text
        .find(r#"<rating default="false" max="10" name="tvdb">"#)
        .unwrap();
    assert!(imdb < tvdb);

    // no source matches: TVDB, TMDB, IMDb in that order decides
    options.common.rating_sources = vec!["metacritic".to_owned()];
    let text = render_show_nfo(TvConnector::Kodi, &show(), &options);
    assert!(text.contains(r#"<rating default="true" max="10" name="tvdb">"#));
}

#[test]
fn studios_and_countries_are_written_once_each_and_empty_when_missing() {
    let text = text_of(&show());
    assert_eq!(text.matches("<studio>").count(), 2);
    let mut single = options();
    single.common.single_studio = true;
    let text = render_show_nfo(TvConnector::Kodi, &show(), &single);
    assert_eq!(text.matches("<studio>").count(), 1);
    assert!(has(&text, "<studio>Beacon Network</studio>"));

    let bare = ShowNfo::default();
    let text = text_of(&bare);
    assert!(has(&text, "<studio/>") && has(&text, "<country/>"));
}

#[test]
fn all_actors_adds_each_guest_once_in_the_kodi_family_only() {
    let cast = Credit::new(CreditKind::Actor, "Mara Voss", "Head Keeper");
    let guest = Credit::new(CreditKind::Guest, "Guest One", "Visitor");
    let mut show = show();
    show.credits = vec![cast.clone()];
    show.guest_actors = vec![guest.clone(), guest, cast];

    let plain = text_of(&show);
    assert_eq!(plain.matches("<actor>").count(), 1);

    let mut all = options();
    all.write_all_actors = true;
    let text = render_show_nfo(TvConnector::Kodi, &show, &all);
    assert_eq!(text.matches("<actor>").count(), 2);
    assert!(has(&text, "<type>GuestStar</type>"));
    let xbmc = render_show_nfo(TvConnector::Xbmc, &show, &all);
    assert_eq!(xbmc.matches("<actor>").count(), 1);
}

#[test]
fn trailers_use_the_plug_in_url_of_the_format() {
    let mut show = show();
    show.trailers = vec![Trailer {
        url: "https://www.youtube.com/watch?v=abc123".to_owned(),
        in_nfo: true,
        ..Trailer::default()
    }];
    assert!(has(
        &text_of(&show),
        "<trailer>plugin://plugin.video.youtube/play/?video_id=abc123</trailer>"
    ));
    let xbmc = render_show_nfo(TvConnector::Xbmc, &show, &options());
    assert!(xbmc.contains(
        "<trailer>plugin://plugin.video.youtube/?action=play_video&amp;videoid=abc123</trailer>"
    ));
    let mut off = options();
    off.common.trailer = false;
    assert!(!render_show_nfo(TvConnector::Kodi, &show, &off).contains("<trailer"));
}

#[test]
fn outline_is_created_from_the_plot_on_request() {
    let mut options = options();
    assert!(!text_of(&show()).contains("<outline>"));
    options.common.create_outline = true;
    let text = render_show_nfo(TvConnector::Kodi, &show(), &options);
    assert!(has(&text, "<outline>Two keepers, one rock</outline>"));
    options.common.outline_first_sentence = false;
    let text = render_show_nfo(TvConnector::Kodi, &show(), &options);
    assert!(has(
        &text,
        "<outline>Two keepers, one rock. Endless weather.</outline>"
    ));
    // the show's own outline wins, the old NFO's is the last resort
    let mut own = show();
    own.outline = "Own".to_owned();
    assert!(has(
        &render_show_nfo(TvConnector::Kodi, &own, &options),
        "<outline>Own</outline>"
    ));
    let mut carried = show();
    carried.carried.outline = "Old".to_owned();
    assert!(has(&text_of(&carried), "<outline>Old</outline>"));
}

#[test]
fn date_added_follows_the_chosen_source() {
    let mut show = show();
    show.file_created = Some("2020-01-02 03:04:05".to_owned());
    show.file_modified = Some("2021-01-02 03:04:05".to_owned());
    let render = |field| {
        let mut options = options();
        options.common.date_added_field = field;
        render_show_nfo(TvConnector::Kodi, &show, &options)
    };
    use cv_nfo::DateAddedField::*;
    assert!(has(
        &render(DateAdded),
        "<dateadded>2024-02-03 04:05:06</dateadded>"
    ));
    assert!(has(
        &render(FileCreationDate),
        "<dateadded>2020-01-02 03:04:05</dateadded>"
    ));
    assert!(has(
        &render(FileLastModifiedDate),
        "<dateadded>2021-01-02 03:04:05</dateadded>"
    ));
    assert!(has(
        &render(ReleaseDate),
        "<dateadded>2015-03-10 00:00:00</dateadded>"
    ));
    let mut off = options();
    off.common.write_date_added = false;
    assert!(!render_show_nfo(TvConnector::Kodi, &show, &off).contains("dateadded"));
}

#[test]
fn episode_groups_list_only_named_orderings_and_mark_the_active_one() {
    let mut show = show();
    show.episode_groups = vec![
        EpisodeGroup::new(EpisodeOrdering::Aired, "Aired Order"),
        EpisodeGroup::new(EpisodeOrdering::Dvd, "DVD Order"),
        EpisodeGroup::new(EpisodeOrdering::Dvd, ""),
        EpisodeGroup::new(EpisodeOrdering::Absolute, ""),
    ];
    show.active_group = EpisodeGroup::aired();
    let text = text_of(&show);
    assert!(has(
        &text,
        r#"<group active="true" id="AIRED" name="Aired Order"/>"#
    ));
    assert!(has(&text, r#"<group id="DVD" name="DVD Order"/>"#));
    assert!(!text.contains(r#"name="""#));

    show.episode_groups = vec![EpisodeGroup::aired()];
    assert!(!text_of(&show).contains("episode_groups"));
}

#[test]
fn crew_and_lock_close_the_show_nfo() {
    let mut show = show();
    show.credits = vec![
        Credit::new(CreditKind::Actor, "A", "r"),
        Credit::new(CreditKind::Director, "D", ""),
        Credit::new(CreditKind::Writer, "W", "Creator"),
    ];
    show.locked_fields = vec!["title".to_owned(), "plot".to_owned()];
    let text = text_of(&show);
    assert!(
        text.contains("<crew>\r\n    <name>D</name>\r\n    <role>DIRECTOR</role>\r\n  </crew>")
    );
    assert!(text.contains(r#"<role subrole="Creator">WRITER</role>"#));
    assert!(!text.contains("<name>A</name>\r\n    <role>ACTOR"));
    assert!(text.ends_with("  <cv_locked>title,plot</cv_locked>\r\n</tvshow>\r\n"));
    // the comment before CineVault's own tags
    assert!(text.contains("<!--CineVault meta data-->"));
}

#[test]
fn show_nfo_has_unmanaged_elements_before_the_private_tags() {
    let mut show = show();
    show.unknown_elements = vec![cv_nfo::Element::text("customfield", "kept")];
    let text = text_of(&show);
    let custom = text.find("<customfield>kept</customfield>").unwrap();
    let meta = text.find("<!--CineVault meta data-->").unwrap();
    let dated = text.find("<dateadded>").unwrap();
    assert!(dated < custom && custom < meta);
}

// ---- episodes --------------------------------------------------------------------------------

#[test]
fn multi_episode_file_has_one_block_per_episode_and_one_declaration() {
    let episodes: Vec<EpisodeNfo> = (5..=7).map(|n| episode(2, n)).collect();
    let text = render_episode_nfo(TvConnector::Kodi, &episodes, &options());
    assert_eq!(text.matches("<?xml").count(), 1);
    assert_eq!(text.matches("<episodedetails>").count(), 3);
    assert_eq!(
        text.matches("</episodedetails>\r\n<episodedetails>")
            .count(),
        2
    );
    assert_eq!(text.matches("<!--created").count(), 1);
    for n in 5..=7 {
        assert!(has(&text, &format!("<episode>{n}</episode>")));
    }
    // the file's runtime is shared by its episodes
    assert_eq!(text.matches("<runtime>15</runtime>").count(), 3);
    assert_eq!(
        text.matches("<durationinseconds>900</durationinseconds>")
            .count(),
        3
    );
    assert!(!text.contains("episodenumberend"));

    let document = episode_document(TvConnector::Kodi, &episodes, &options());
    assert_eq!(document.roots.len(), 3);
}

#[test]
fn emby_marks_the_last_episode_of_a_multi_episode_file_in_every_block() {
    let episodes: Vec<EpisodeNfo> = (5..=7).map(|n| episode(2, n)).collect();
    let text = render_episode_nfo(TvConnector::Emby, &episodes, &options());
    assert_eq!(
        text.matches("<episodenumberend>7</episodenumberend>")
            .count(),
        3
    );
    let single = render_episode_nfo(TvConnector::Emby, &[episode(2, 5)], &options());
    assert!(!single.contains("episodenumberend"));
    assert!(
        !render_episode_nfo(TvConnector::Jellyfin, &episodes, &options())
            .contains("episodenumberend")
    );
}

#[test]
fn emby_lists_writers_and_directors_after_the_actors() {
    let mut ep = episode(1, 1);
    ep.credits = vec![
        Credit::new(CreditKind::Guest, "Guest", "Visitor"),
        Credit::new(CreditKind::Director, "Dana", ""),
        Credit::new(CreditKind::Writer, "Eli", ""),
    ];
    let kodi = render_episode_nfo(TvConnector::Kodi, &[ep.clone()], &options());
    let emby = render_episode_nfo(TvConnector::Emby, &[ep], &options());
    assert!(kodi.find("<credits>").unwrap() < kodi.find("<actor>").unwrap());
    assert!(emby.find("<actor>").unwrap() < emby.find("<credits>").unwrap());
    assert!(has(&kodi, "<type>GuestStar</type>"));
}

#[test]
fn episode_runtime_comes_from_the_video_file_else_the_episode() {
    let text = render_episode_nfo(TvConnector::Kodi, &[episode(1, 1)], &options());
    assert!(has(&text, "<runtime>45</runtime>"));
    let mut ep = episode(1, 1);
    ep.video.duration_secs = 0;
    let text = render_episode_nfo(TvConnector::Kodi, &[ep], &options());
    assert!(has(&text, "<runtime>44</runtime>"));
}

#[test]
fn display_numbers_are_written_together() {
    let mut ep = episode(2, 1);
    ep.display = Some((1, 99));
    let text = render_episode_nfo(TvConnector::Kodi, &[ep], &options());
    assert!(has(&text, "<displayseason>1</displayseason>"));
    assert!(has(&text, "<displayepisode>99</displayepisode>"));
    let text = render_episode_nfo(TvConnector::Kodi, &[episode(2, 1)], &options());
    assert!(!text.contains("displayseason"));
}

#[test]
fn episode_ids_ratings_and_user_rating() {
    let mut ep = episode(1, 1);
    ep.ids.set("tvdb", "55501");
    ep.ids.set("imdb", "tt7000001");
    ep.ratings.insert(Rating::new("tvdb", 8.2, 321, 10));
    ep.ratings.insert(Rating::new("user", 9.0, 1, 10));
    let text = render_episode_nfo(TvConnector::Kodi, &[ep], &options());
    assert!(has(&text, "<id>55501</id>"));
    assert!(text.contains(r#"<uniqueid default="true" type="imdb">tt7000001</uniqueid>"#));
    assert!(text.contains(r#"<uniqueid default="false" type="tvdb">55501</uniqueid>"#));
    assert!(text.contains(r#"<rating default="true" max="10" name="tvdb">"#));
    assert!(!text.contains(r#"name="user""#));
    assert!(has(&text, "<userrating>9</userrating>"));
}

#[test]
fn episode_watch_state_keeps_the_higher_play_count_and_later_time() {
    let mut ep = episode(1, 1);
    ep.watched = true;
    ep.play_count = 2;
    ep.last_played = Some("2020-01-02 03:04:05".to_owned());
    ep.carried.play_count = 5;
    ep.carried.last_played = Some("2021-01-01 00:00:00".to_owned());
    let text = render_episode_nfo(TvConnector::Kodi, &[ep.clone()], &options());
    assert!(has(&text, "<playcount>5</playcount>"));
    assert!(has(&text, "<lastplayed>2021-01-01 00:00:00</lastplayed>"));

    ep.watched = false;
    let text = render_episode_nfo(TvConnector::Kodi, &[ep], &options());
    assert!(has(&text, "<watched>false</watched>"));
    assert!(has(&text, "<playcount>0</playcount>"));
    assert!(!text.contains("lastplayed"));
}

#[test]
fn episode_edition_elements_are_for_the_kodi_family_only() {
    let mut ep = episode(1, 1);
    ep.edition = Edition::DirectorsCut;
    let kodi = render_episode_nfo(TvConnector::Kodi, &[ep.clone()], &options());
    assert!(has(
        &kodi,
        "<videoassettitle>Director's Cut</videoassettitle>"
    ));
    assert!(has(&kodi, "<hasvideoversions>true</hasvideoversions>"));
    assert!(has(&kodi, "<edition>DIRECTORS_CUT</edition>"));
    let xbmc = render_episode_nfo(TvConnector::Xbmc, &[ep], &options());
    assert!(!xbmc.contains("videoassettitle"));
    assert!(has(&xbmc, "<edition>DIRECTORS_CUT</edition>"));
}

#[test]
fn episode_fileinfo_follows_the_option_and_the_format() {
    let kodi = render_episode_nfo(TvConnector::Kodi, &[episode(1, 1)], &options());
    assert!(has(&kodi, "<codec>h264</codec>"));
    assert!(has(&kodi, "<resolution>1080</resolution>"));
    assert!(has(&kodi, "<language>eng</language>"));
    let xbmc = render_episode_nfo(TvConnector::Xbmc, &[episode(1, 1)], &options());
    assert!(has(&xbmc, "<codec>AVC</codec>"));
    assert!(has(&xbmc, "<stereomode/>"));
    assert!(!xbmc.contains("<resolution>"));
    let mut off = options();
    off.common.fileinfo = false;
    assert!(!render_episode_nfo(TvConnector::Kodi, &[episode(1, 1)], &off).contains("fileinfo"));
}

#[test]
fn episode_trailer_element_follows_the_all_actors_option() {
    let plain = render_episode_nfo(TvConnector::Kodi, &[episode(1, 1)], &options());
    assert!(!plain.contains("<trailer"));
    let mut all = options();
    all.write_all_actors = true;
    let mut ep = episode(1, 1);
    let text = render_episode_nfo(TvConnector::Kodi, &[ep.clone()], &all);
    assert!(has(&text, "<trailer/>"));
    ep.carried.trailer = "plugin://old".to_owned();
    let text = render_episode_nfo(TvConnector::Kodi, &[ep], &all);
    assert!(has(&text, "<trailer>plugin://old</trailer>"));
}

#[test]
fn episode_studios_and_single_studio() {
    let mut ep = episode(1, 1);
    ep.studios = split_episode_list("Beacon Network; Tide TV | Third / Fourth, Fifth");
    assert_eq!(ep.studios.len(), 5);
    let text = render_episode_nfo(TvConnector::Kodi, &[ep.clone()], &options());
    assert_eq!(text.matches("<studio>").count(), 5);
    let mut single = options();
    single.common.single_studio = true;
    let text = render_episode_nfo(TvConnector::Kodi, &[ep], &single);
    assert_eq!(text.matches("<studio>").count(), 1);
    assert!(
        !render_episode_nfo(TvConnector::Kodi, &[episode(1, 1)], &options()).contains("<studio")
    );
}

#[test]
fn episode_picture_and_artwork_option() {
    let mut ep = episode(1, 1);
    ep.thumb = "https://img.test/e.jpg".to_owned();
    assert!(has(
        &render_episode_nfo(TvConnector::Kodi, &[ep.clone()], &options()),
        "<thumb>https://img.test/e.jpg</thumb>"
    ));
    let mut off = options();
    off.common.artwork_urls = false;
    assert!(!render_episode_nfo(TvConnector::Kodi, &[ep], &off).contains("<thumb>"));
}

#[test]
fn episode_groups_of_an_episode_need_a_name() {
    let mut ep = episode(1, 1);
    ep.numbers = vec![
        EpisodeNumber::new(EpisodeGroup::aired(), 1, 1),
        EpisodeNumber::new(EpisodeGroup::dvd(), 1, 3),
    ];
    let text = render_episode_nfo(TvConnector::Kodi, &[ep.clone()], &options());
    assert!(!text.contains("episode_groups"));
    ep.numbers.push(EpisodeNumber::new(
        EpisodeGroup::new(EpisodeOrdering::Alternate, "Production"),
        2,
        4,
    ));
    let text = render_episode_nfo(TvConnector::Kodi, &[ep], &options());
    assert!(has(
        &text,
        r#"<group episode="4" id="ALTERNATE" name="Production" season="2"/>"#
    ));
}

#[test]
fn episode_private_tags_close_each_block() {
    let mut ep = episode(1, 1);
    ep.english_title = "Arrival".to_owned();
    ep.crc32 = Some("ABCD1234".to_owned());
    ep.note = "n".to_owned();
    let text = render_episode_nfo(TvConnector::Kodi, &[ep], &options());
    let at = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("missing {needle}"))
    };
    assert!(at("<epbookmark/>") < at("<code/>"));
    assert!(at("<code/>") < at("<!--CineVault meta data-->"));
    assert!(at("<source>UNKNOWN</source>") < at("<edition>NONE</edition>"));
    assert!(at("<original_filename>") < at("<user_note>n</user_note>"));
    assert!(at("<user_note>") < at("<english_title>Arrival</english_title>"));
    assert!(at("<english_title>") < at("<crc32>ABCD1234</crc32>"));
}

// ---- rewriting ----------------------------------------------------------------------------

#[test]
fn a_rewrite_of_the_same_data_is_not_a_change() {
    let mut first = options();
    first.common.created_on = "2024-01-01 00:00:00".to_owned();
    let mut later = options();
    later.common.created_on = "2025-06-06 06:06:06".to_owned();
    let old = render_show_nfo(TvConnector::Kodi, &show(), &first);
    let new = render_show_nfo(TvConnector::Kodi, &show(), &later);
    assert_ne!(old, new);
    assert!(!content_differs(&old, &new));

    let mut changed = show();
    changed.plot = "Different".to_owned();
    assert!(content_differs(
        &old,
        &render_show_nfo(TvConnector::Kodi, &changed, &first)
    ));
}

#[test]
fn document_roots_carry_the_header_comment_only_once() {
    let document = show_document(TvConnector::Kodi, &show(), &options());
    assert!(document.comment.is_some());
    assert_eq!(document.roots.len(), 1);
    let root = &document.roots[0];
    assert!(root.children.iter().any(|n| matches!(n, Node::Comment(_))));
}

#[test]
fn list_splitting_helpers() {
    assert_eq!(split_show_list("US, CA / MX;DE"), ["US", "CA", "MX;DE"]);
    assert_eq!(split_show_list(""), Vec::<String>::new());
    assert_eq!(split_episode_list("A; B | C"), ["A", "B", "C"]);
}

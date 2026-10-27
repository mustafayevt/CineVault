//! The `tvshow.nfo` writer for the Kodi, Emby, Jellyfin and XBMC layouts (MediaPortal and Plex
//! use the XBMC and Kodi layouts).
//!
//! Element order, as in `tests/compat/expected/nfo/tv-*`:
//!
//! ```text
//! title originaltitle showtitle sorttitle year [top250] ratings userrating [outline] plot
//! tagline runtime [thumb*] namedseason* seasonplot* [season thumbs] [fanart] mpaa certification
//! [episodeguide] id imdbid tmdbid uniqueid* premiered status watched playcount genre* studio+
//! country+ tag* credits* director* actor* [trailer] [dateadded] [lockdata] [enddate]
//! <elements CineVault does not manage>  <!-- CineVault meta data -->
//! user_note [episode_groups] english_title crew* cv_locked
//! ```
//!
//! The XBMC layout has a flat `<rating>` and `<votes>` instead of `<ratings>`, no `top250`, and
//! the older trailer URL. `enddate` is written by the Emby and Jellyfin layouts only. Elements
//! without a value are written empty, except those marked optional.

use cv_domain::{AiredStatus, CalendarDate, CreditKind, EpisodeGroup};

use crate::kodi_movie::{
    finish, named_person, non_blank, number_or_empty, product_name, trailer, xbmc_trailer_url,
};
use crate::locale;
use crate::movie::{DateAddedField, MovieNfoOptions};
use crate::tv::{ShowNfo, TvConnector, TvFlavor, TvNfoOptions};
use crate::tv_common::{
    crew_of, flat_rating, ids_json, of_kind, ordering_id, outline, ratings_element,
    show_main_rating, tv_actor, unique_ids, user_rating,
};
use crate::xml::{Document, Element};

/// Builds the `tvshow.nfo` document of a show.
pub fn show_document(connector: TvConnector, show: &ShowNfo, options: &TvNfoOptions) -> Document {
    let flavor = connector.show_flavor();
    let common = &options.common;
    let text = |name: &str, value: &str| Element::text(name, value);
    let xbmc = flavor == TvFlavor::Xbmc;

    let mut root = Element::new("tvshow");
    root.push(text("title", &show.title));
    root.push(text("originaltitle", &show.original_title));
    root.push(text("showtitle", &show.title));
    root.push(text("sorttitle", &show.sort_title));
    root.push(text("year", &number_or_empty(show.year)));
    if xbmc {
        let (rating, votes) = flat_rating(&show.ratings, common);
        root.push(text("rating", &rating));
        root.push(text("userrating", &user_rating(&show.ratings)));
        root.push(text("votes", &votes));
    } else {
        if show.top250 > 0 {
            root.push(text("top250", &show.top250.to_string()));
        }
        let main = show_main_rating(&show.ratings, common);
        root.push(ratings_element(main, &show.ratings));
        root.push(text("userrating", &user_rating(&show.ratings)));
    }
    if let Some(outline) = outline(&show.outline, &show.plot, &show.carried.outline, common) {
        root.push(text("outline", &outline));
    }
    root.push(text("plot", &show.plot));
    root.push(text("tagline", &show.tagline));
    root.push(text("runtime", &show.runtime_minutes.to_string()));
    if common.artwork_urls {
        show_thumbs(show, &mut root);
    }
    named_seasons(show, &mut root);
    if common.artwork_urls {
        season_thumbs(show, &mut root);
        fanart(show, &mut root);
    }

    let certification = crate::certification::format_certification(
        &show.certification.0,
        common.certification_style,
    );
    root.push(text("mpaa", &certification));
    root.push(text("certification", &certification));
    if options.write_episodeguide
        && let Some(guide) = episodeguide(show, options)
    {
        root.push(guide);
    }
    root.push(text("id", show.ids.get("tvdb").unwrap_or_default()));
    root.push(text("imdbid", show.ids.get("imdb").unwrap_or_default()));
    root.push(text("tmdbid", show.ids.get("tmdb").unwrap_or_default()));
    for id in unique_ids(&show.ids, Some(&["tvdb", "imdb", "tmdb"])) {
        root.push(id);
    }
    root.push(text(
        "premiered",
        &show.first_aired.map(|d| d.to_string()).unwrap_or_default(),
    ));
    root.push(text("status", status_name(show.status)));
    root.push(text("watched", if show.watched { "true" } else { "false" }));
    root.push(text("playcount", &play_count(show)));
    for genre in &show.genres {
        root.push(text("genre", &locale::genre_name(genre, &common.language)));
    }
    let mut studios: Vec<&str> = non_blank(&show.studios).collect();
    if common.single_studio {
        studios.truncate(1);
    }
    for studio in or_one_empty(studios) {
        root.push(text("studio", studio));
    }
    for country in or_one_empty(non_blank(&show.countries).collect()) {
        root.push(text("country", country));
    }
    for tag in non_blank(&show.tags) {
        root.push(text("tag", tag));
    }
    for credit in of_kind(&show.credits, CreditKind::Writer) {
        root.push(named_person("credits", credit));
    }
    for credit in of_kind(&show.credits, CreditKind::Director) {
        root.push(named_person("director", credit));
    }
    for credit in actors(show, options, flavor) {
        root.push(tv_actor(credit, common));
    }
    if common.trailer {
        root.push(text("trailer", &trailer_text(show, flavor)));
    }
    if common.write_date_added {
        root.push(text(
            "dateadded",
            &date_added(show, common).unwrap_or_default(),
        ));
    }
    if common.lockdata {
        root.push(text("lockdata", "true"));
    }
    if matches!(flavor, TvFlavor::Emby | TvFlavor::Jellyfin)
        && let Some(date) = end_date(show, options, flavor)
    {
        root.push(text("enddate", &date.to_string()));
    }
    for unknown in &show.unknown_elements {
        root.push(unknown.clone());
    }

    private_tags(show, common, &mut root);
    finish(root, common, connector.label())
}

/// Renders the `tvshow.nfo` of a show: UTF-8 text, two-space indentation, CRLF line ends.
pub fn render_show_nfo(connector: TvConnector, show: &ShowNfo, options: &TvNfoOptions) -> String {
    show_document(connector, show, options).render()
}

/// The status as the NFO spells it.
pub(crate) fn status_name(status: AiredStatus) -> &'static str {
    match status {
        AiredStatus::Unknown => "Unknown",
        AiredStatus::Continuing => "Continuing",
        AiredStatus::Ended => "Ended",
        AiredStatus::Canceled => "Canceled",
    }
}

/// The values, or one empty value when there are none (the element is written empty).
fn or_one_empty(values: Vec<&str>) -> Vec<&str> {
    if values.is_empty() { vec![""] } else { values }
}

fn play_count(show: &ShowNfo) -> String {
    if !show.watched {
        String::new()
    } else if show.carried.play_count > 0 {
        show.carried.play_count.to_string()
    } else {
        "1".to_owned()
    }
}

fn show_thumbs(show: &ShowNfo, root: &mut Element) {
    let art = &show.artwork;
    for (aspect, url) in [
        ("poster", &art.poster),
        ("banner", &art.banner),
        ("clearart", &art.clearart),
        ("clearlogo", &art.clearlogo),
        ("landscape", &art.thumb),
        ("keyart", &art.keyart),
        ("squareart", &art.squareart),
        ("logo", &art.logo),
        ("characterart", &art.characterart),
        ("discart", &art.disc),
    ] {
        if !url.trim().is_empty() {
            root.push(Element::text("thumb", url.trim()).attr("aspect", aspect));
        }
    }
}

/// `<namedseason>` and `<seasonplot>` of the seasons that have a name or a plot.
fn named_seasons(show: &ShowNfo, root: &mut Element) {
    for season in show.seasons.iter().filter(|s| !s.title.trim().is_empty()) {
        root.push(
            Element::text("namedseason", &season.title).attr("number", season.number.to_string()),
        );
    }
    for season in show.seasons.iter().filter(|s| !s.plot.trim().is_empty()) {
        root.push(
            Element::text("seasonplot", &season.plot).attr("number", season.number.to_string()),
        );
    }
}

/// The season posters, then banners, then thumbs: each kind lists all seasons before the next.
fn season_thumbs(show: &ShowNfo, root: &mut Element) {
    for aspect in ["poster", "banner", "thumb"] {
        for season in &show.seasons {
            let art = &season.artwork;
            let url = match aspect {
                "poster" => &art.poster,
                "banner" => &art.banner,
                _ => &art.thumb,
            }
            .trim();
            if !url.is_empty() {
                root.push(
                    Element::text("thumb", url)
                        .attr("aspect", aspect)
                        .attr("season", season.number.to_string())
                        .attr("type", "season"),
                );
            }
        }
    }
}

fn fanart(show: &ShowNfo, root: &mut Element) {
    let mut urls: Vec<&str> = Vec::new();
    for url in std::iter::once(show.artwork.fanart.as_str())
        .chain(show.artwork.extra_fanarts.iter().map(String::as_str))
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    if !urls.is_empty() {
        let mut element = Element::new("fanart");
        for url in urls {
            element.push(Element::text("thumb", url));
        }
        root.push(element);
    }
}

// ---- episodeguide ----------------------------------------------------------------------------

/// `<episodeguide>`: the JSON of all IDs, or the old URL of the last scraper's provider.
fn episodeguide(show: &ShowNfo, options: &TvNfoOptions) -> Option<Element> {
    if options.new_episodeguide_style {
        return Some(Element::text("episodeguide", ids_json(&show.ids)));
    }
    let tvdb = show.ids.get("tvdb");
    let tmdb = show.ids.get("tmdb");
    let last = show.last_scraper_id.trim();
    let url = if let (true, Some(id)) = (last == "tvdb", tvdb) {
        tvdb_url(id, options)
    } else if let (true, Some(id)) = (last == "tmdb", tmdb) {
        tmdb_url(id, options)
    } else if let Some(id) = tvdb {
        tvdb_url(id, options)
    } else if let Some(id) = tmdb {
        tmdb_url(id, options)
    } else {
        return show.carried.episodeguide.clone();
    };
    Some(Element::new("episodeguide").child(url))
}

fn tvdb_url(id: &str, options: &TvNfoOptions) -> Element {
    Element::text(
        "url",
        format!(
            "https://api.thetvdb.com/login?{{\"apikey\":\"{}\",\"id\":{id}}}|Content-Type=application/json",
            options.episodeguide_keys.tvdb
        ),
    )
    .attr("cache", "auth.json")
    .attr("post", "yes")
}

fn tmdb_url(id: &str, options: &TvNfoOptions) -> Element {
    Element::text(
        "url",
        format!(
            "http://api.themoviedb.org/3/tv/{id}?api_key={}&language={}",
            options.episodeguide_keys.tmdb, options.scraper_language
        ),
    )
}

// ---- people, trailer, dates --------------------------------------------------------------

/// The cast; with "write all actors" the guest actors of the episodes follow, each name once
/// (Kodi, Emby and Jellyfin layouts).
fn actors<'a>(
    show: &'a ShowNfo,
    options: &TvNfoOptions,
    flavor: TvFlavor,
) -> Vec<&'a cv_domain::Credit> {
    let cast = of_kind(&show.credits, CreditKind::Actor);
    if !options.write_all_actors || flavor == TvFlavor::Xbmc {
        return cast.collect();
    }
    let mut all: Vec<&cv_domain::Credit> = Vec::new();
    for credit in cast.chain(show.guest_actors.iter()) {
        let name = credit.person.name.trim();
        if !name.is_empty() && !all.iter().any(|c| c.person.name.trim() == name) {
            all.push(credit);
        }
    }
    all
}

fn trailer_text(show: &ShowNfo, flavor: TvFlavor) -> String {
    if flavor != TvFlavor::Xbmc {
        return trailer(&show.trailers);
    }
    let url = show
        .trailers
        .iter()
        .find(|t| t.in_nfo && !t.url.trim().is_empty())
        .map(|t| t.url.trim())
        .unwrap_or_default();
    xbmc_trailer_url(url)
}

fn date_added(show: &ShowNfo, options: &MovieNfoOptions) -> Option<String> {
    let fallback = || show.date_added.clone();
    match options.date_added_field {
        DateAddedField::DateAdded => fallback(),
        DateAddedField::FileCreationDate => show.file_created.clone().or_else(fallback),
        DateAddedField::FileLastModifiedDate => show.file_modified.clone().or_else(fallback),
        DateAddedField::ReleaseDate => show
            .first_aired
            .map(|d| format!("{d} 00:00:00"))
            .or_else(fallback),
    }
}

/// The date of the last episode of an ended show. Jellyfin ignores the specials.
fn end_date(show: &ShowNfo, options: &TvNfoOptions, flavor: TvFlavor) -> Option<CalendarDate> {
    if !options.write_date_ended || show.status != AiredStatus::Ended {
        return None;
    }
    show.episode_air_dates
        .iter()
        .filter(|air| flavor != TvFlavor::Jellyfin || air.season > 0)
        .map(|air| air.date)
        .max()
}

// ---- CineVault tags --------------------------------------------------------------------------

/// CineVault's own tags that close the show NFO.
fn private_tags(show: &ShowNfo, options: &MovieNfoOptions, root: &mut Element) {
    let text = |name: &str, value: &str| Element::text(name, value);
    root.push_comment(format!("{} meta data", product_name(&options.generator)));
    root.push(text("user_note", &show.note));
    if let Some(groups) = episode_groups(show) {
        root.push(groups);
    }
    root.push(text("english_title", &show.english_title));
    if options.cv_extensions {
        if !show.original_language.trim().is_empty() {
            root.push(text("original_language", show.original_language.trim()));
        }
        if show.year_end > 0 {
            root.push(text("year_end", &show.year_end.to_string()));
        }
    }
    for credit in crew_of(&show.credits) {
        root.push(crate::kodi_movie::crew(credit, options));
    }
    root.push(text("cv_locked", &show.locked_fields.join(",")));
}

/// `<episode_groups>`: the orderings that have a name. Left out when there is none.
fn episode_groups(show: &ShowNfo) -> Option<Element> {
    let groups: Vec<&EpisodeGroup> = show
        .episode_groups
        .iter()
        .filter(|g| !g.name.trim().is_empty())
        .collect();
    if groups.is_empty() {
        return None;
    }
    let mut element = Element::new("episode_groups");
    for group in groups {
        let mut e = Element::new("group")
            .attr("id", ordering_id(group.ordering))
            .attr("name", &group.name);
        if *group == show.active_group {
            e = e.attr("active", "true");
        }
        element.push(e);
    }
    Some(element)
}

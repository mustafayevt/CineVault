//! Applying a read NFO to a library item ("Read NFO").
//!
//! The rule is **the file wins where it speaks**: a value the NFO holds replaces the library
//! value, a value the NFO leaves out (an empty element, a missing one) leaves the library value
//! alone, so a sparse file never blanks a record. Lists (genres, tags, cast, studios) are replaced
//! as a whole when the NFO has any entry; IDs and ratings are merged one by one; trailers are
//! added when their URL is new. These stay as they are: the date the item was added, its folder
//! and files, locks and the review state. A movie in no collection joins the one its `<set>`
//! names (see `readback`); a movie already in a collection stays there. Watch history is only ever added
//! to: an NFO that says "watched" marks the item watched (the higher play count and the file's
//! last played time win); an NFO that says "unwatched" is the same as one that says nothing.
//!
//! The functions change the item in place and return nothing; the caller compares the item with
//! a copy taken before to learn whether anything changed.

use cv_domain::{EpisodeGroup, Item, ItemDetail, MediaFileType, MediaSource, SeasonText, Tags};
use cv_nfo::{EpisodeRead, MovieNfo, MovieSetNfo, SeasonRead, ShowNfo};

use super::time::millis_from_local;

fn set_text(target: &mut String, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        value.clone_into(target);
    }
}

fn set_list<T: Clone>(target: &mut Vec<T>, value: &[T]) {
    if !value.is_empty() {
        value.clone_into(target);
    }
}

/// Countries read from a file as the library stores them: a known name (`Japan`) becomes its
/// code (`JP`), anything else stays as written.
fn stored_countries(countries: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for country in countries.iter().map(|c| cv_domain::country_stored(c)) {
        if !country.is_empty() && !out.contains(&country) {
            out.push(country);
        }
    }
    out
}

fn set_number(target: &mut u32, value: u32) {
    if value > 0 {
        *target = value;
    }
}

fn set_url(item: &mut Item, kind: MediaFileType, url: &str) {
    let url = url.trim();
    if !url.is_empty() {
        item.core.artwork_urls.insert(kind, url.to_owned());
    }
}

fn merge_ids(item: &mut Item, ids: &cv_domain::ProviderIds) {
    for (key, value) in ids.iter() {
        item.core.ids.set(key, value);
    }
}

fn merge_ratings(item: &mut Item, ratings: &cv_domain::Ratings) {
    for rating in ratings.iter() {
        item.core.ratings.insert(rating.clone());
    }
}

fn merge_common(item: &mut Item, nfo: &CommonFields<'_>) {
    let c = &mut item.core;
    set_text(&mut c.title, nfo.title);
    set_text(&mut c.original_title, nfo.original_title);
    if let Ok(year) = u16::try_from(nfo.year)
        && year > 0
    {
        c.year = year;
    }
    set_text(&mut c.plot, nfo.plot);
    set_text(&mut c.outline, nfo.outline);
    set_text(&mut c.note, nfo.note);
    set_list(&mut c.production_companies, nfo.studios);
    set_list(&mut c.credits, nfo.credits);
    if !nfo.tags.is_empty() {
        c.tags = Tags::from(nfo.tags.to_vec());
    }
    merge_ids(item, nfo.ids);
    merge_ratings(item, nfo.ratings);
}

/// The fields every kind shares, borrowed from the model that was read.
struct CommonFields<'a> {
    title: &'a str,
    original_title: &'a str,
    year: u32,
    plot: &'a str,
    outline: &'a str,
    note: &'a str,
    studios: &'a [String],
    credits: &'a [cv_domain::Credit],
    tags: &'a [String],
    ids: &'a cv_domain::ProviderIds,
    ratings: &'a cv_domain::Ratings,
}

/// Applies a movie NFO to a movie item.
pub fn merge_movie(item: &mut Item, nfo: &MovieNfo) {
    if !matches!(item.detail, ItemDetail::Movie(_)) {
        return;
    }
    merge_common(
        item,
        &CommonFields {
            title: &nfo.title,
            original_title: &nfo.original_title,
            year: nfo.year,
            plot: &nfo.plot,
            outline: &nfo.outline,
            note: &nfo.note,
            studios: &nfo.studios,
            credits: &nfo.credits,
            tags: &nfo.tags,
            ids: &nfo.ids,
            ratings: &nfo.ratings,
        },
    );
    set_text(&mut item.core.original_language, &nfo.original_language);
    let art = &nfo.artwork;
    for (kind, url) in [
        (MediaFileType::Poster, &art.poster),
        (MediaFileType::Banner, &art.banner),
        (MediaFileType::Clearart, &art.clearart),
        (MediaFileType::Clearlogo, &art.clearlogo),
        (MediaFileType::Disc, &art.disc),
        (MediaFileType::Thumb, &art.thumb),
        (MediaFileType::Keyart, &art.keyart),
        (MediaFileType::Squareart, &art.squareart),
        (MediaFileType::Fanart, &art.fanart),
    ] {
        set_url(item, kind, url);
    }
    let ItemDetail::Movie(m) = &mut item.detail else {
        return;
    };
    set_text(&mut m.sort_title, &nfo.sort_title);
    set_text(&mut m.english_title, &nfo.english_title);
    set_text(&mut m.tagline, &nfo.tagline);
    set_number(&mut m.runtime_minutes, nfo.runtime_minutes);
    set_number(&mut m.top250, nfo.top250);
    if !nfo.certification.0.trim().is_empty() {
        m.certification = nfo.certification.clone();
    }
    set_list(&mut m.genres, &nfo.genres);
    set_list(&mut m.countries, &stored_countries(&nfo.countries));
    set_list(&mut m.spoken_languages, &nfo.spoken_languages);
    set_list(&mut m.show_links, &nfo.show_links);
    set_list(&mut m.extra_fanarts, &art.extra_fanarts);
    set_list(&mut m.extra_thumbs, &art.extra_thumbs);
    if nfo.video_in_3d {
        m.video_in_3d = true;
    }
    if nfo.release_date.is_some() {
        m.release_date = nfo.release_date;
    }
    if nfo.media_source != MediaSource::Unknown {
        m.media_source = nfo.media_source.clone();
    }
    if nfo.edition != cv_domain::Edition::None {
        m.edition = nfo.edition.clone();
    }
    if nfo.watched || nfo.play_count > 0 {
        m.watched = true;
        m.play_count = m.play_count.max(nfo.play_count);
        if let Some(ms) = nfo.last_played.as_deref().and_then(millis_from_local) {
            m.last_watched_ms = Some(ms);
        }
    }
    for trailer in &nfo.trailers {
        if !trailer.url.trim().is_empty() && !m.trailers.iter().any(|t| t.url == trailer.url) {
            let mut added = trailer.clone();
            added.in_nfo = true;
            m.trailers.push(added);
        }
    }
}

/// Applies a movie-set NFO to a set item.
pub fn merge_set(item: &mut Item, nfo: &MovieSetNfo) {
    if !matches!(item.detail, ItemDetail::Set(_)) {
        return;
    }
    set_text(&mut item.core.title, &nfo.title);
    set_text(&mut item.core.plot, &nfo.overview);
    set_text(&mut item.core.note, &nfo.note);
    if !nfo.tags.is_empty() {
        item.core.tags = Tags::from(nfo.tags.clone());
    }
    merge_ids(item, &nfo.ids);
    set_url(item, MediaFileType::Poster, &nfo.poster);
    set_url(item, MediaFileType::Fanart, &nfo.fanart);
    if let ItemDetail::Set(s) = &mut item.detail {
        set_text(&mut s.sort_title, &nfo.sort_title);
        set_text(&mut s.english_title, &nfo.english_title);
    }
}

/// Applies a show NFO to a show item, including the season names and plots it lists.
pub fn merge_show(item: &mut Item, nfo: &ShowNfo) {
    if !matches!(item.detail, ItemDetail::Show(_)) {
        return;
    }
    merge_common(
        item,
        &CommonFields {
            title: &nfo.title,
            original_title: &nfo.original_title,
            year: nfo.year,
            plot: &nfo.plot,
            outline: &nfo.outline,
            note: &nfo.note,
            studios: &nfo.studios,
            credits: &nfo.credits,
            tags: &nfo.tags,
            ids: &nfo.ids,
            ratings: &nfo.ratings,
        },
    );
    set_text(&mut item.core.original_language, &nfo.original_language);
    let art = &nfo.artwork;
    for (kind, url) in [
        (MediaFileType::Poster, &art.poster),
        (MediaFileType::Banner, &art.banner),
        (MediaFileType::Clearart, &art.clearart),
        (MediaFileType::Clearlogo, &art.clearlogo),
        (MediaFileType::Thumb, &art.thumb),
        (MediaFileType::Keyart, &art.keyart),
        (MediaFileType::Squareart, &art.squareart),
        (MediaFileType::Characterart, &art.characterart),
        (MediaFileType::Disc, &art.disc),
        (MediaFileType::Fanart, &art.fanart),
    ] {
        set_url(item, kind, url);
    }
    let ItemDetail::Show(s) = &mut item.detail else {
        return;
    };
    set_text(&mut s.sort_title, &nfo.sort_title);
    set_text(&mut s.english_title, &nfo.english_title);
    if let Ok(year) = u16::try_from(nfo.year_end)
        && year > 0
    {
        s.year_end = year;
    }
    set_text(&mut s.tagline, &nfo.tagline);
    set_number(&mut s.runtime_minutes, nfo.runtime_minutes);
    set_number(&mut s.top250, nfo.top250);
    if !nfo.certification.0.trim().is_empty() {
        s.certification = nfo.certification.clone();
    }
    set_list(&mut s.genres, &nfo.genres);
    if !nfo.countries.is_empty() {
        s.country = stored_countries(&nfo.countries).join(", ");
    }
    set_list(&mut s.extra_fanart_urls, &art.extra_fanarts);
    if nfo.first_aired.is_some() {
        s.first_aired = nfo.first_aired;
    }
    if nfo.status != cv_domain::AiredStatus::Unknown {
        s.status = nfo.status;
    }
    for trailer in &nfo.trailers {
        if !trailer.url.trim().is_empty() && !s.trailers.iter().any(|t| t.url == trailer.url) {
            let mut added = trailer.clone();
            added.in_nfo = true;
            s.trailers.push(added);
        }
    }
    let group = s.episode_group.clone();
    for season in &nfo.seasons {
        merge_season_text(item, season.number, &season.title, &season.plot, &group);
    }
}

/// Sets the name and overview of one season of a show, in the show's current ordering.
pub fn merge_season_text(
    show: &mut Item,
    number: u32,
    title: &str,
    plot: &str,
    group: &EpisodeGroup,
) {
    let ItemDetail::Show(s) = &mut show.detail else {
        return;
    };
    let (title, plot) = (title.trim(), plot.trim());
    if title.is_empty() && plot.is_empty() {
        return;
    }
    let existing = s
        .season_texts
        .iter_mut()
        .find(|t| t.season == number && &t.group == group);
    match existing {
        Some(text) => {
            set_text(&mut text.name, title);
            set_text(&mut text.overview, plot);
        }
        None => s.season_texts.push(SeasonText {
            group: group.clone(),
            season: number,
            name: title.to_owned(),
            overview: plot.to_owned(),
        }),
    }
}

/// Applies a season NFO to the season item (artwork) and to its show (name and overview).
pub fn merge_season(season: &mut Item, show: &mut Item, read: &SeasonRead) {
    let ItemDetail::Season(_) = &season.detail else {
        return;
    };
    let Some(number) = read.number else {
        return;
    };
    let art = &read.season.artwork;
    for (kind, url) in [
        (MediaFileType::SeasonPoster, &art.poster),
        (MediaFileType::SeasonBanner, &art.banner),
        (MediaFileType::SeasonThumb, &art.thumb),
        (MediaFileType::SeasonFanart, &art.fanart),
    ] {
        set_url(season, kind, url);
    }
    set_text(&mut season.core.note, &read.season.note);
    let group = match &show.detail {
        ItemDetail::Show(s) => s.episode_group.clone(),
        _ => EpisodeGroup::aired(),
    };
    merge_season_text(show, number, &read.season.title, &read.season.plot, &group);
}

/// Applies one episode block to an episode item.
pub fn merge_episode(item: &mut Item, read: &EpisodeRead) {
    if !matches!(item.detail, ItemDetail::Episode(_)) {
        return;
    }
    let nfo = &read.episode;
    set_text(&mut item.core.title, &nfo.title);
    set_text(&mut item.core.original_title, &nfo.original_title);
    set_text(&mut item.core.plot, &nfo.plot);
    set_text(&mut item.core.outline, &nfo.outline);
    set_text(&mut item.core.note, &nfo.note);
    set_list(&mut item.core.production_companies, &nfo.studios);
    set_list(&mut item.core.credits, &nfo.credits);
    if !nfo.tags.is_empty() {
        item.core.tags = Tags::from(nfo.tags.clone());
    }
    merge_ids(item, &nfo.ids);
    merge_ratings(item, &nfo.ratings);
    set_url(item, MediaFileType::Thumb, &nfo.thumb);
    let ItemDetail::Episode(e) = &mut item.detail else {
        return;
    };
    set_text(&mut e.english_title, &nfo.english_title);
    set_number(&mut e.runtime_minutes, nfo.runtime_minutes);
    if nfo.first_aired.is_some() {
        e.first_aired = nfo.first_aired;
    }
    if nfo.media_source != MediaSource::Unknown {
        e.media_source = nfo.media_source.clone();
    }
    if nfo.edition != cv_domain::Edition::None {
        e.edition = nfo.edition.clone();
    }
    if nfo.watched || nfo.play_count > 0 {
        e.watched = true;
        e.play_count = e.play_count.max(nfo.play_count);
        if let Some(ms) = nfo.last_played.as_deref().and_then(millis_from_local) {
            e.last_watched_ms = Some(ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_domain::{Credit, CreditKind, Genre, Rating};

    fn nfo() -> MovieNfo {
        let mut nfo = MovieNfo {
            title: "Heat".into(),
            year: 1995,
            plot: "A crew, a cop.".into(),
            genres: vec![Genre::Drama],
            credits: vec![Credit::new(CreditKind::Actor, "Al Pacino", "Hanna")],
            play_count: 2,
            last_played: Some("2024-03-04 05:06:07".into()),
            ..MovieNfo::default()
        };
        nfo.ids.set("imdb", "tt0113277");
        nfo.ratings.insert(Rating::new("imdb", 8.3, 700_000, 10));
        nfo.artwork.poster = "https://img.example.test/p.jpg".into();
        nfo
    }

    #[test]
    fn the_file_wins_where_it_speaks_and_the_library_keeps_the_rest() {
        let mut item = Item::movie("Heat (old)");
        item.core.plot = "Library plot".into();
        item.core.note = "my note".into();
        item.core.date_added_ms = 1_700_000_000_000;
        item.core.ids.set("tmdb", "949");
        let mut sparse = MovieNfo {
            title: "Heat".into(),
            ..MovieNfo::default()
        };
        sparse.ids.set("imdb", "tt0113277");
        merge_movie(&mut item, &sparse);
        assert_eq!(item.core.title, "Heat");
        assert_eq!(
            item.core.plot, "Library plot",
            "an empty element keeps the value"
        );
        assert_eq!(item.core.note, "my note");
        assert_eq!(item.core.ids.get("tmdb"), Some("949"));
        assert_eq!(item.core.ids.get("imdb"), Some("tt0113277"));
        assert_eq!(item.core.date_added_ms, 1_700_000_000_000);
    }

    #[test]
    fn country_names_read_from_a_file_are_stored_as_codes() {
        let mut item = Item::movie("x");
        let mut file = nfo();
        file.countries = vec![
            "Japan".into(),
            "United Kingdom".into(),
            "JP".into(),
            "Atlantis".into(),
        ];
        merge_movie(&mut item, &file);
        let ItemDetail::Movie(m) = &item.detail else {
            panic!("a movie")
        };
        assert_eq!(m.countries, ["JP", "GB", "Atlantis"]);
    }

    #[test]
    fn a_full_movie_nfo_fills_the_item() {
        let mut item = Item::movie("x");
        merge_movie(&mut item, &nfo());
        assert_eq!(item.core.year, 1995);
        assert_eq!(item.core.plot, "A crew, a cop.");
        assert_eq!(item.core.credits.len(), 1);
        assert_eq!(
            item.core.ratings.get("imdb").map(|r| r.votes),
            Some(700_000)
        );
        assert_eq!(
            item.core
                .artwork_urls
                .get(&MediaFileType::Poster)
                .map(String::as_str),
            Some("https://img.example.test/p.jpg")
        );
        let ItemDetail::Movie(m) = &item.detail else {
            panic!("not a movie")
        };
        assert_eq!(m.genres, [Genre::Drama]);
        assert!(m.watched);
        assert_eq!(m.play_count, 2);
        assert!(m.last_watched_ms.is_some());
    }

    #[test]
    fn merging_twice_changes_nothing_more() {
        let mut item = Item::movie("x");
        merge_movie(&mut item, &nfo());
        let once = item.clone();
        merge_movie(&mut item, &nfo());
        assert_eq!(item, once);
    }

    #[test]
    fn season_names_land_on_the_show() {
        let mut show = Item::show("Severance");
        let mut season = Item::season(cv_domain::ItemId(1), 2);
        let read = SeasonRead {
            number: Some(2),
            season: cv_nfo::SeasonNfo {
                number: 2,
                title: "Hello, Ms. Cobel".into(),
                plot: "Mark returns.".into(),
                ..cv_nfo::SeasonNfo::default()
            },
            ..SeasonRead::default()
        };
        merge_season(&mut season, &mut show, &read);
        let ItemDetail::Show(s) = &show.detail else {
            panic!("not a show")
        };
        assert_eq!(s.season_texts.len(), 1);
        assert_eq!(s.season_texts[0].name, "Hello, Ms. Cobel");
        merge_season(&mut season, &mut show, &read);
        let ItemDetail::Show(s) = &show.detail else {
            panic!("not a show")
        };
        assert_eq!(s.season_texts.len(), 1);
    }

    #[test]
    fn other_kinds_are_left_alone() {
        let mut show = Item::show("S");
        let before = show.clone();
        merge_movie(&mut show, &nfo());
        assert_eq!(show, before);
    }
}

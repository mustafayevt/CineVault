//! The TV model: what `tvShow.*`, `season.*` and `episode.*` read.

use std::collections::BTreeMap;
use std::sync::Arc;

use cv_domain::{
    CalendarDate, EpisodeFields, EpisodeGroup, EpisodeNumbers, Item, ItemDetail, ShowFields, id_key,
};

use super::ModelOptions;
use super::labels::{genre_label, media_source_label, status_label};
use super::media::media_fields;
use super::movie::{
    basename, date_value, labeled_list, number_id, parent_folder, rating_value, sortable,
};
use crate::value::Value;

/// The number of an episode that does not have one in an ordering.
const MISSING_NUMBER: i64 = -1;

/// The model root for an episode: `tvShow`, `season` and `episode`.
///
/// `show` is the show the episode belongs to. `season` is read from the show's season texts
/// (`${seasonName}`) in the show's episode ordering, falling back to the aired ordering. An
/// item that is not a show or an episode reads as an empty one.
pub fn tv_model(show: &Item, episode: &Item, options: &ModelOptions) -> Value {
    let default_show = ShowFields::default();
    let show_fields = match &show.detail {
        ItemDetail::Show(fields) => fields,
        _ => &default_show,
    };
    let default_episode = EpisodeFields::default();
    let episode_fields = match &episode.detail {
        ItemDetail::Episode(fields) => fields,
        _ => &default_episode,
    };
    let display = display_number(&episode_fields.numbers, &show_fields.episode_group);

    Value::map([
        ("tvShow", show_model(show, show_fields, options)),
        (
            "season",
            Value::map([(
                "title",
                Value::text(season_name(show_fields, display.map(|(season, _)| season))),
            )]),
        ),
        (
            "episode",
            episode_model(episode, episode_fields, show_fields, options),
        ),
    ])
}

fn show_model(show: &Item, fields: &ShowFields, options: &ModelOptions) -> Value {
    let core = &show.core;
    let genres: Vec<String> = fields.genres.iter().map(genre_label).collect();
    let tags: Vec<String> = core.tags.iter().map(str::to_owned).collect();
    Value::map([
        ("title", Value::text(&core.title)),
        ("originalTitle", Value::text(&core.original_title)),
        ("englishTitle", Value::text(&fields.english_title)),
        ("titleSortable", Value::text(sortable(&core.title, options))),
        ("tagline", Value::text(&fields.tagline)),
        ("year", Value::Int(i64::from(core.year))),
        ("yearEnd", Value::Int(i64::from(fields.year_end))),
        ("parent", Value::text(parent_folder(core))),
        ("note", Value::text(&core.note)),
        ("status", Value::text(status_label(fields.status))),
        ("rating", rating_value(&core.ratings, options)),
        ("top250", Value::Int(i64::from(fields.top250))),
        (
            "imdbId",
            Value::text(core.ids.get(id_key::IMDB).unwrap_or_default()),
        ),
        ("tmdbId", number_id(&core.ids, id_key::TMDB)),
        ("tvdbId", number_id(&core.ids, id_key::TVDB)),
        ("certification", Value::text(&fields.certification.0)),
        ("tags", Value::list(tags.iter().map(Value::text))),
        ("tagsAsString", Value::text(tags.join(", "))),
        ("genresAsString", Value::text(genres.join(", "))),
        ("genres", labeled_list(genres)),
        (
            "productionCompany",
            Value::text(core.production_companies.join("; ")),
        ),
        (
            "productionCompanyAsArray",
            Value::list(core.production_companies.iter().map(Value::text)),
        ),
    ])
}

fn episode_model(
    episode: &Item,
    fields: &EpisodeFields,
    show: &ShowFields,
    options: &ModelOptions,
) -> Value {
    let core = &episode.core;
    let numbers = &fields.numbers;
    let display = display_number(numbers, &show.episode_group);
    let number = |group: &EpisodeGroup, season: bool| {
        let found = numbers.get(group);
        Value::Int(found.map_or(MISSING_NUMBER, |n| {
            i64::from(if season { n.season } else { n.episode })
        }))
    };
    let tags: Vec<String> = core.tags.iter().map(str::to_owned).collect();
    let folder = core
        .path
        .replace('\\', "/")
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned();

    let mut map: BTreeMap<String, Value> = BTreeMap::new();
    let mut put = |key: &str, value: Value| {
        map.insert(key.to_owned(), value);
    };
    put("title", Value::text(&core.title));
    put("originalTitle", Value::text(&core.original_title));
    put("englishTitle", Value::text(&fields.english_title));
    put("originalFilename", Value::text(&core.original_filename));
    put(
        "originalBasename",
        Value::text(basename(&core.original_filename)),
    );
    put("folderName", Value::text(folder));
    put("titleSortable", Value::text(sortable(&core.title, options)));
    put("year", Value::Int(year_of(fields.first_aired)));
    put("firstAired", date_value(fields.first_aired));
    put("rating", rating_value(&core.ratings, options));
    put(
        "imdbId",
        Value::text(core.ids.get(id_key::IMDB).unwrap_or_default()),
    );
    put("tmdbId", number_id(&core.ids, id_key::TMDB));
    put("tvdbId", number_id(&core.ids, id_key::TVDB));
    put("tags", Value::list(tags.iter().map(Value::text)));
    put("tagsAsString", Value::text(tags.join(", ")));
    put("note", Value::text(&core.note));
    put(
        "mediaSource",
        Value::text(media_source_label(&fields.media_source)),
    );
    put(
        "season",
        Value::Int(display.map_or(MISSING_NUMBER, |(season, _)| i64::from(season))),
    );
    put(
        "episode",
        Value::Int(display.map_or(MISSING_NUMBER, |(_, episode)| i64::from(episode))),
    );
    put("airedSeason", number(&EpisodeGroup::aired(), true));
    put("airedEpisode", number(&EpisodeGroup::aired(), false));
    put("dvdSeason", number(&EpisodeGroup::dvd(), true));
    put("dvdEpisode", number(&EpisodeGroup::dvd(), false));
    put(
        "absoluteNumber",
        Value::Int(
            numbers
                .get(&EpisodeGroup::absolute())
                .map_or(MISSING_NUMBER, |n| i64::from(n.episode)),
        ),
    );
    for (key, value) in media_fields(&core.media_files) {
        // Episodes have no second 3D spelling.
        if key != "video3DFormat2" {
            put(key, value);
        }
    }
    Value::Map(Arc::new(map))
}

/// The season and episode number shown for the show's ordering, falling back to aired.
fn display_number(numbers: &EpisodeNumbers, group: &EpisodeGroup) -> Option<(u32, u32)> {
    numbers.display(group).map(|n| (n.season, n.episode))
}

fn season_name(show: &ShowFields, season: Option<u32>) -> String {
    let Some(season) = season else {
        return String::new();
    };
    let find = |group: &EpisodeGroup| {
        show.season_texts
            .iter()
            .find(|text| &text.group == group && text.season == season && !text.name.is_empty())
    };
    find(&show.episode_group)
        .or_else(|| find(&EpisodeGroup::aired()))
        .map_or_else(String::new, |text| text.name.clone())
}

fn year_of(date: Option<CalendarDate>) -> i64 {
    date.map_or(0, |d| i64::from(d.year()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::{RenamerOptions, render_name, tv_aliases};
    use cv_domain::{AiredStatus, EpisodeNumber, ItemId, MediaFile, MediaFileType, SeasonText};

    fn show() -> Item {
        let mut show = Item::show("Lighthouse Keepers: The Series");
        show.core.year = 2015;
        show.core.ids.set(id_key::TVDB, "81189");
        if let ItemDetail::Show(s) = &mut show.detail {
            s.status = AiredStatus::Continuing;
            s.genres = vec![cv_domain::Genre::Drama];
            s.season_texts = vec![SeasonText {
                group: EpisodeGroup::aired(),
                season: 2,
                name: "The Second Watch".into(),
                overview: String::new(),
            }];
        }
        show
    }

    fn episode(season: u32, number: u32) -> Item {
        let mut item = Item::episode(ItemId(1), "Arrival");
        if let ItemDetail::Episode(e) = &mut item.detail {
            e.numbers.set(EpisodeNumber::aired(season, number));
            e.first_aired = CalendarDate::new(2015, 3, 10);
        }
        let mut file = MediaFile::new(MediaFileType::Video, "/tv", "a.mkv");
        file.width = 1920;
        file.height = 1080;
        file.audio_streams = vec![cv_domain::AudioStream {
            codec: "AC3".into(),
            channels: 6,
            ..cv_domain::AudioStream::default()
        }];
        item.core.media_files.push(file);
        item
    }

    fn render(template: &str, show: &Item, episode: &Item) -> String {
        let model = tv_model(show, episode, &ModelOptions::default());
        render_name(template, &tv_aliases(), &model, &RenamerOptions::default()).unwrap()
    }

    #[test]
    fn episode_file_name() {
        let (show, ep) = (show(), episode(1, 1));
        assert_eq!(
            render(
                "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}",
                &show,
                &ep
            ),
            "Lighthouse Keepers - The Series - S01E01 - Arrival"
        );
        assert_eq!(
            render("${seasonNr}x${episodeNr2}", &show, &episode(10, 11)),
            "10x11"
        );
        assert_eq!(
            render("${airedDate} ${year}", &show, &ep),
            "2015-03-10 2015"
        );
        assert_eq!(
            render("${videoFormat} ${audioCodec}", &show, &ep),
            "1080p AC3"
        );
    }

    #[test]
    fn season_zero_prints_with_the_number_renderer_only() {
        let (show, ep) = (show(), episode(0, 1));
        assert_eq!(render("${seasonNr}", &show, &ep), "0");
        assert_eq!(render("${seasonNr2}", &show, &ep), "00");
        assert_eq!(render("${episodeNr}", &show, &ep), "1");
    }

    #[test]
    fn missing_numbers_are_minus_one() {
        let (show, ep) = (show(), episode(1, 1));
        assert_eq!(render("${absoluteNr}", &show, &ep), "-1");
        assert_eq!(
            render("S${seasonNrDvd2}E${episodeNrDvd2}", &show, &ep),
            "S-1E-1"
        );
        assert_eq!(render("${seasonNrAired2}", &show, &ep), "01");
    }

    #[test]
    fn show_and_season_tokens() {
        let (show, ep) = (show(), episode(2, 1));
        assert_eq!(
            render("${showTitle} (${showYear})", &show, &ep),
            "Lighthouse Keepers - The Series (2015)"
        );
        assert_eq!(
            render("${showTvdb} ${showStatus}", &show, &ep),
            "81189 Continuing"
        );
        assert_eq!(render("${showGenresAsString}", &show, &ep), "Drama");
        assert_eq!(render("${seasonName}", &show, &ep), "The Second Watch");
        assert_eq!(render("${seasonName}", &show, &episode(1, 1)), "");
        assert_eq!(render("${showYearEnd}", &show, &ep), "");
    }
}

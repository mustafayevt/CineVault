//! The movie model: what `movie.*` and `movieSet.*` read.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use cv_domain::{
    CalendarDate, Credit, CreditKind, Item, ItemCore, ItemDetail, MovieFields, Ratings, id_key,
};

use super::ModelOptions;
use super::labels::{
    decade_long, decade_short, edition_label, genre_label, media_source_label, sortable_title,
};
use super::media::media_fields;
use crate::datetime::DateTime;
use crate::value::{Object, Value};

/// The collection a movie belongs to, for `${movieSet.title}` and the set index tokens.
#[derive(Debug, Clone, Default)]
pub struct MovieSetContext {
    pub title: String,
    pub sort_title: String,
    /// The ids of the movies in the collection that are in the library, in play order. A movie's
    /// `id` must be one of these for `${movieSetIndex}` to find it.
    pub movies: Vec<i64>,
    /// The same, with an entry (and so a position) for every movie the collection is missing.
    /// Use a negative id for a missing movie.
    pub movies_with_dummy: Vec<i64>,
}

/// The model root for a movie: `movie`, and `movieSet` when `set` is given.
///
/// `movie.id` identifies the movie inside `set`; it is the item id, or `0` for an item that is
/// not stored yet.
pub fn movie_model(item: &Item, set: Option<&MovieSetContext>, options: &ModelOptions) -> Value {
    let default_fields = MovieFields::default();
    let fields = match &item.detail {
        ItemDetail::Movie(fields) => fields,
        _ => &default_fields,
    };
    let core = &item.core;
    let mut movie: BTreeMap<String, Value> = BTreeMap::new();
    let mut put = |key: &str, value: Value| {
        movie.insert(key.to_owned(), value);
    };

    put("id", Value::Int(item.id.map_or(0, |id| id.0)));
    put("title", Value::text(&core.title));
    put("originalTitle", Value::text(&core.original_title));
    put("englishTitle", Value::text(&fields.english_title));
    put("originalFilename", Value::text(&core.original_filename));
    put(
        "originalBasename",
        Value::text(basename(&core.original_filename)),
    );
    put("sortTitle", Value::text(&fields.sort_title));
    put("titleSortable", Value::text(sortable(&core.title, options)));
    put("year", Value::Int(i64::from(core.year)));
    put("releaseDate", date_value(fields.release_date));
    put("rating", rating_value(&core.ratings, options));
    put("top250", Value::Int(i64::from(fields.top250)));
    put(
        "imdbId",
        Value::text(core.ids.get(id_key::IMDB).unwrap_or_default()),
    );
    put("tmdbId", number_id(&core.ids, id_key::TMDB));
    put("certification", Value::text(&fields.certification.0));
    put(
        "spokenLanguages",
        Value::text(fields.spoken_languages.join(", ")),
    );
    let genres: Vec<String> = fields.genres.iter().map(genre_label).collect();
    put("genresAsString", Value::text(genres.join(", ")));
    put("genres", labeled_list(genres));
    let tags: Vec<String> = core.tags.iter().map(str::to_owned).collect();
    put("tagsAsString", Value::text(tags.join(", ")));
    put("tags", Value::list(tags.iter().map(Value::text)));
    for (key, kind) in [
        ("actors", CreditKind::Actor),
        ("directors", CreditKind::Director),
        ("writers", CreditKind::Writer),
        ("producers", CreditKind::Producer),
    ] {
        put(key, people(&core.credits, kind));
    }
    put(
        "productionCompany",
        Value::text(core.production_companies.join("; ")),
    );
    put(
        "productionCompanyAsArray",
        Value::list(core.production_companies.iter().map(Value::text)),
    );
    put(
        "mediaSource",
        Value::text(media_source_label(&fields.media_source)),
    );
    put("edition", Value::text(edition_label(&fields.edition)));
    put("parent", Value::text(parent_folder(core)));
    put("decadeLong", Value::text(decade_long(core.year)));
    put("decadeShort", Value::text(decade_short(core.year)));
    put("note", Value::text(&core.note));
    for (key, value) in media_fields(&core.media_files) {
        put(key, value);
    }

    let set_value = set.map(|set| set_model(set, options));
    if let Some(value) = &set_value {
        put("movieSet", value.clone());
    }
    let movie = Value::Map(Arc::new(movie));
    let mut root = vec![("movie", movie)];
    if let Some(value) = set_value {
        root.push(("movieSet", value));
    }
    Value::map(root)
}

fn set_model(set: &MovieSetContext, options: &ModelOptions) -> Value {
    let ids = |ids: &[i64]| Value::list(ids.iter().map(|id| Value::map([("id", Value::Int(*id))])));
    Value::map([
        ("title", Value::text(&set.title)),
        ("sortTitle", Value::text(&set.sort_title)),
        ("titleSortable", Value::text(sortable(&set.title, options))),
        ("movies", ids(&set.movies)),
        ("moviesWithDummy", ids(&set.movies_with_dummy)),
    ])
}

// ---- helpers shared with the TV model -------------------------------------------------------

/// A value that prints as its name and also answers `.name`.
#[derive(Debug)]
struct Labeled {
    name: String,
    role: String,
}

impl Object for Labeled {
    fn get(&self, key: &str) -> Option<Value> {
        match key {
            "name" => Some(Value::text(&self.name)),
            "role" => Some(Value::text(&self.role)),
            _ => None,
        }
    }

    fn display(&self) -> String {
        self.name.clone()
    }
}

/// A list of values that print as their text and answer `.name`, like `${genres[0].name}`.
pub(super) fn labeled_list(names: Vec<String>) -> Value {
    Value::list(names.into_iter().map(|name| {
        Value::object(Labeled {
            name,
            role: String::new(),
        })
    }))
}

/// The people of one credit kind, with `.name` and `.role`.
pub(super) fn people(credits: &[Credit], kind: CreditKind) -> Value {
    Value::list(credits.iter().filter(|c| c.kind == kind).map(|c| {
        Value::object(Labeled {
            name: c.person.name.clone(),
            role: c.role.clone(),
        })
    }))
}

pub(super) fn sortable(title: &str, options: &ModelOptions) -> String {
    sortable_title(title, &options.title_prefixes)
}

/// A date as a template date value (midnight UTC), or `Null`.
pub(super) fn date_value(date: Option<CalendarDate>) -> Value {
    date.and_then(|d| {
        DateTime::from_local(
            i64::from(d.year()),
            u32::from(d.month()),
            u32::from(d.day()),
            (0, 0, 0),
            0,
        )
    })
    .map_or(Value::Null, Value::DateTime)
}

/// `{rating: n}` with the item's rating on its own scale; `0.0` when there is none. With a rating
/// source, the rating is picked as the library shows it: that source, else the next one in the
/// library's fallback order.
pub(super) fn rating_value(ratings: &Ratings, options: &ModelOptions) -> Value {
    let chosen = match options.rating_source.as_deref() {
        Some(source) => ratings
            .pick(&cv_domain::RatingPreference::new(source))
            .map(|picked| picked.rating),
        None => ratings.iter().find(|r| r.id != "user"),
    };
    let value = chosen.map_or(0.0, |r| f64::from(r.value));
    Value::map([("rating", Value::Float(value))])
}

/// A numeric provider id; `0` when the item has none, which prints as nothing.
pub(super) fn number_id(ids: &cv_domain::ProviderIds, key: &str) -> Value {
    Value::Int(
        ids.get_u64(key)
            .and_then(|n| i64::try_from(n).ok())
            .unwrap_or(0),
    )
}

/// The file name without its extension.
pub(super) fn basename(filename: &str) -> String {
    Path::new(filename)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The folders between the data source and the item's own folder, `/`-separated:
/// `Action` for `D:\Movies\Action\Dune` in data source `D:\Movies`.
pub(super) fn parent_folder(core: &ItemCore) -> String {
    let normalise = |text: &str| text.replace('\\', "/").trim_end_matches('/').to_owned();
    let (root, path) = (normalise(&core.data_source), normalise(&core.path));
    let Some(relative) = path.strip_prefix(&root) else {
        return String::new();
    };
    let relative = relative.trim_matches('/');
    relative
        .rsplit_once('/')
        .map_or_else(String::new, |(parent, _)| parent.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::{RenamerOptions, movie_aliases};
    use cv_domain::{Edition, Genre, MediaSource, Rating};

    fn render(template: &str, item: &Item, set: Option<&MovieSetContext>) -> String {
        let model = movie_model(item, set, &ModelOptions::default());
        crate::tokens::render_name(
            template,
            &movie_aliases(),
            &model,
            &RenamerOptions::default(),
        )
        .unwrap()
    }

    fn dune() -> Item {
        let mut item = Item::movie("Dune: Part Two");
        item.core.year = 2024;
        item.core.data_source = "D:\\Movies".into();
        item.core.path = "D:\\Movies\\Action\\Dune Part Two".into();
        item.core.ids.set(id_key::TMDB, "693134");
        item.core.ratings.insert(Rating::new("imdb", 8.6, 1, 10));
        item.core.production_companies = vec!["Legend".into(), "Quay".into()];
        item.core.credits = vec![
            Credit::new(CreditKind::Actor, "Alex Moreau", "Hero"),
            Credit::new(CreditKind::Director, "Jo Clarke", ""),
            Credit::new(CreditKind::Actor, "Sam Rivera", "Friend"),
        ];
        item.core.tags.add("desert");
        if let ItemDetail::Movie(m) = &mut item.detail {
            m.genres = vec![Genre::ScienceFiction, Genre::Drama];
            m.edition = Edition::ExtendedEdition;
            m.media_source = MediaSource::WebDl;
        }
        item
    }

    #[test]
    fn a_rating_source_picks_the_rating_as_the_library_shows_it() {
        let mut item = dune();
        item.core.ratings.insert(Rating::new("tmdb", 7.9, 1, 10));
        let rating = |source: Option<&str>| {
            let options = ModelOptions {
                rating_source: source.map(str::to_owned),
                ..ModelOptions::default()
            };
            rating_value(&item.core.ratings, &options)
                .get("rating")
                .map(|v| v.to_text(false))
                .unwrap_or_default()
        };
        assert!(
            rating(Some("tmdb")).starts_with("7.9"),
            "{}",
            rating(Some("tmdb"))
        );
        assert!(rating(Some("imdb")).starts_with("8.6"));
        // A source the title has no rating from falls back like the library tables do.
        let fallback = rating(Some("trakt"));
        let picked = item
            .core
            .ratings
            .pick(&cv_domain::RatingPreference::new("trakt"))
            .map(|p| p.rating.value.to_string())
            .unwrap_or_default();
        assert!(fallback.starts_with(&picked[..3]), "{fallback} vs {picked}");
    }

    #[test]
    fn plain_tokens() {
        let item = dune();
        assert_eq!(render("${title}", &item, None), "Dune - Part Two");
        assert_eq!(render("${title;upper}", &item, None), "DUNE - PART TWO");
        assert_eq!(render("${year}", &item, None), "2024");
        assert_eq!(render("${tmdb}", &item, None), "693134");
        assert_eq!(render("${rating;number(%.1f)}", &item, None), "8.6");
        assert_eq!(
            render("${genresAsString}", &item, None),
            "Science Fiction, Drama"
        );
        assert_eq!(render("${genres[0].name}", &item, None), "Science Fiction");
        assert_eq!(render("${actors[1].role}", &item, None), "Friend");
        assert_eq!(render("${directors[0].name}", &item, None), "Jo Clarke");
        assert_eq!(render("${productionCompany}", &item, None), "Legend; Quay");
        assert_eq!(
            render("${parent}/${title}", &item, None),
            "Action/Dune - Part Two"
        );
        assert_eq!(
            render("${decadeShort}/${decadeLong}", &item, None),
            "2020s/2020-2029"
        );
        assert_eq!(
            render("${mediaSource} ${edition}", &item, None),
            "Web-DL Extended Edition"
        );
        assert_eq!(render("${tags[0]}", &item, None), "desert");
    }

    #[test]
    fn parent_folder_is_relative_to_the_data_source() {
        let mut core = ItemCore {
            data_source: "D:\\Movies\\".into(),
            path: "D:\\Movies\\Dune".into(),
            ..ItemCore::default()
        };
        assert_eq!(parent_folder(&core), "");
        core.path = "D:\\Movies\\A\\B\\Dune".into();
        assert_eq!(parent_folder(&core), "A/B");
        core.path = "E:\\Elsewhere\\Dune".into();
        assert_eq!(parent_folder(&core), "");
    }

    #[test]
    fn movie_set_tokens() {
        let mut item = dune();
        item.id = Some(cv_domain::ItemId(7));
        let set = MovieSetContext {
            title: "Dune Collection".into(),
            sort_title: String::new(),
            movies: vec![3, 7],
            movies_with_dummy: vec![3, 7, -1],
        };
        assert_eq!(
            render("${movieSet.title}", &item, Some(&set)),
            "Dune Collection"
        );
        assert_eq!(render("${movieSetIndex}", &item, Some(&set)), "1");
        assert_eq!(render("${movieSetIndex2}", &item, Some(&set)), "1");
        // Without a set the tokens print nothing.
        assert_eq!(render("${movieSet.title}", &item, None), "");
        assert_eq!(render("${movieSetIndex}", &item, None), "");
    }
}

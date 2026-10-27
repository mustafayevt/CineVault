//! Tests of in-place editing over a real database and real folders.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::{apply_scrape, get_item, insert_item};
use cv_domain::{
    Credit, CreditKind, EpisodeNumber, Genre, Item, ItemDetail, ItemId, MediaFile, MediaFileType,
    MetaField, ScrapeOptions,
};

use super::fields::{
    FieldValue, NumberValue, Pair, PersonValue, RatingValue, TrailerValue, apply_change, read_field,
};
use super::{EditError, EditService, FieldEntry};
use crate::nfo::{NfoService, NfoSettings};

struct Lab {
    root: tempfile::TempDir,
    db: Arc<Database>,
    service: EditService,
}

impl Lab {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(root.path().join("library.db")).unwrap());
        let nfo = Arc::new(NfoService::new(Arc::clone(&db), root.path().join("undo")));
        let service = EditService::new(Arc::clone(&db), nfo);
        Self { root, db, service }
    }

    fn insert(&self, item: Item) -> ItemId {
        self.db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    fn item(&self, id: ItemId) -> Item {
        self.db.read(|c| get_item(c, id)).unwrap().unwrap()
    }

    /// A movie "Heat (1995)" in its own folder with a video file.
    fn movie(&self) -> (ItemId, PathBuf) {
        let folder = self.root.path().join("Movies").join("Heat (1995)");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("Heat (1995).mkv"), b"video").unwrap();
        let mut item = Item::movie("Heat");
        item.core.year = 1995;
        item.core.plot = "A crew of thieves and the cop who hunts them.".into();
        item.core.path = folder.to_string_lossy().into_owned();
        item.core.ids.set("imdb", "tt0113277");
        let mut pacino = Credit::new(CreditKind::Actor, "Al Pacino", "Hanna");
        pacino.person.ids.set("tmdb", "1158");
        item.core.credits.push(pacino);
        item.core
            .credits
            .push(Credit::new(CreditKind::Actor, "Robert De Niro", "McCauley"));
        item.core
            .credits
            .push(Credit::new(CreditKind::Director, "Michael Mann", ""));
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            folder.to_string_lossy(),
            "Heat (1995).mkv",
        ));
        if let ItemDetail::Movie(m) = &mut item.detail {
            m.runtime_minutes = 170;
            m.genres = vec![Genre::Drama];
        }
        (self.insert(item), folder)
    }

    fn save(
        &self,
        id: ItemId,
        changes: Vec<(&str, FieldValue)>,
    ) -> Result<super::SaveReport, EditError> {
        self.service
            .save(id, entries(changes), &NfoSettings::default())
    }
}

fn entries(changes: Vec<(&str, FieldValue)>) -> Vec<FieldEntry> {
    changes
        .into_iter()
        .map(|(key, value)| FieldEntry {
            key: key.to_owned(),
            value,
            lock: None,
        })
        .collect()
}

fn text(value: &str) -> FieldValue {
    FieldValue::Text(value.to_owned())
}

fn nfo_text(folder: &std::path::Path) -> String {
    fs::read_to_string(folder.join("Heat (1995).nfo")).unwrap()
}

#[test]
fn saving_a_sort_title_stores_it_and_rewrites_the_nfo() {
    let lab = Lab::new();
    let (id, folder) = lab.movie();

    let report = lab
        .save(
            id,
            vec![("sort_title", text("Heat, The")), ("title", text("Heat"))],
        )
        .unwrap();

    assert_eq!(report.changed, vec!["sort_title".to_owned()]);
    assert!(report.nfo_problem.is_none(), "{:?}", report.nfo_problem);
    assert!(report.nfo_files_written >= 1);
    let ItemDetail::Movie(movie) = lab.item(id).detail else {
        panic!("not a movie");
    };
    assert_eq!(movie.sort_title, "Heat, The");
    assert!(nfo_text(&folder).contains("<sorttitle>Heat, The</sorttitle>"));
}

#[test]
fn a_save_without_changes_touches_nothing() {
    let lab = Lab::new();
    let (id, folder) = lab.movie();

    let report = lab
        .save(
            id,
            vec![
                ("title", text("Heat")),
                ("year", FieldValue::Number(Some(1995.0))),
            ],
        )
        .unwrap();

    assert!(report.changed.is_empty());
    assert_eq!(report.nfo_files_written, 0);
    assert!(!folder.join("Heat (1995).nfo").exists());
}

#[test]
fn a_refused_value_saves_nothing() {
    let lab = Lab::new();
    let (id, _) = lab.movie();

    let result = lab.save(
        id,
        vec![
            ("tagline", text("Hunt or be hunted")),
            ("release_date", FieldValue::Date(Some("1995-13-40".into()))),
        ],
    );

    let Err(EditError::Invalid(error)) = result else {
        panic!("expected a refusal, got {result:?}");
    };
    assert_eq!(error.key, "release_date");
    let ItemDetail::Movie(movie) = lab.item(id).detail else {
        panic!("not a movie");
    };
    assert_eq!(
        movie.tagline, "",
        "the tagline must not be saved on its own"
    );
}

#[test]
fn numbers_dates_and_titles_are_checked() {
    let lab = Lab::new();
    let (id, _) = lab.movie();
    let mut item = lab.item(id);

    for (key, value) in [
        ("year", FieldValue::Number(Some(1000.0))),
        ("year", FieldValue::Number(Some(1995.5))),
        ("runtime", FieldValue::Number(Some(-1.0))),
        ("title", text("   ")),
        ("user_rating", FieldValue::Number(Some(11.0))),
        ("artwork.poster", text("not a url")),
        ("top250", FieldValue::Number(Some(300.0))),
        ("sort_title", FieldValue::Flag(true)),
        ("status", text("ended")),
    ] {
        assert!(
            apply_change(&mut item, key, &value).is_err(),
            "{key} should be refused"
        );
    }
    assert_eq!(
        item,
        lab.item(id),
        "a refused value must not change the item"
    );
}

#[test]
fn previous_values_undo_a_save() {
    let lab = Lab::new();
    let (id, _) = lab.movie();

    let report = lab
        .save(
            id,
            vec![
                ("plot", text("Changed plot.")),
                (
                    "genres",
                    FieldValue::List(vec!["Crime".into(), "Drama".into()]),
                ),
            ],
        )
        .unwrap();
    assert_eq!(report.changed.len(), 2);

    lab.service
        .save(id, report.previous, &NfoSettings::default())
        .unwrap();
    let item = lab.item(id);
    assert_eq!(
        item.core.plot,
        "A crew of thieves and the cop who hunts them."
    );
    assert_eq!(
        read_field(&item, "genres"),
        Some(FieldValue::List(vec!["DRAMA".into()]))
    );
}

#[test]
fn a_locked_genre_field_survives_a_rescrape() {
    let lab = Lab::new();
    let (id, _) = lab.movie();
    lab.save(
        id,
        vec![("genres", FieldValue::List(vec!["Western".into()]))],
    )
    .unwrap();

    lab.service.set_field_lock(id, "genres", true).unwrap();
    let snapshot = lab.service.load(id).unwrap();
    assert!(snapshot.locked_fields.contains(&"genres".to_owned()));
    let genres = snapshot.fields.iter().find(|f| f.key == "genres").unwrap();
    assert_eq!(genres.lock.as_deref(), Some("genres"));

    let mut scraped = Item::movie("Heat");
    if let ItemDetail::Movie(m) = &mut scraped.detail {
        m.genres = vec![Genre::Action];
    }
    scraped.core.plot = "From the provider.".into();
    let outcome = lab
        .db
        .write(move |tx| {
            apply_scrape(
                tx,
                id,
                &scraped,
                &ScrapeOptions::all(cv_domain::ItemKind::Movie),
            )
        })
        .unwrap();
    assert!(outcome.locked.contains(&MetaField::Genres));
    let item = lab.item(id);
    assert_eq!(
        read_field(&item, "genres"),
        Some(FieldValue::List(vec!["WESTERN".into()]))
    );
    assert_eq!(item.core.plot, "From the provider.");

    // The owner can still edit a locked field.
    lab.save(id, vec![("genres", FieldValue::List(vec!["Crime".into()]))])
        .unwrap();
    assert_eq!(
        read_field(&lab.item(id), "genres"),
        Some(FieldValue::List(vec!["CRIME".into()]))
    );

    lab.service.set_field_lock(id, "genres", false).unwrap();
    assert!(lab.service.load(id).unwrap().locked_fields.is_empty());
}

#[test]
fn a_refused_value_is_explained_in_a_sentence_with_the_range_the_form_uses() {
    let lab = Lab::new();
    let (id, _) = lab.movie();
    for year in [-5.0, 1750.0, 99_999.0, 1995.5] {
        let Err(EditError::Invalid(error)) =
            lab.save(id, vec![("year", FieldValue::Number(Some(year)))])
        else {
            panic!("{year} was accepted");
        };
        assert_eq!(error.key, "year");
        assert_eq!(
            error.message,
            "Enter a year from 1800 to 2200, or leave it empty"
        );
    }
    let Err(EditError::Invalid(error)) =
        lab.save(id, vec![("runtime", FieldValue::Number(Some(-10.0)))])
    else {
        panic!("a negative runtime was accepted");
    };
    assert!(
        error.message.starts_with("The runtime must be"),
        "{}",
        error.message
    );
    lab.save(id, vec![("year", FieldValue::Number(None))])
        .unwrap();
    lab.save(id, vec![("year", FieldValue::Number(Some(1800.0)))])
        .unwrap();
}

#[test]
fn the_play_count_and_last_watched_day_can_be_edited() {
    let lab = Lab::new();
    let (id, _) = lab.movie();
    let value = |key: &str| {
        lab.service
            .load(id)
            .unwrap()
            .fields
            .into_iter()
            .find(|f| f.key == key)
            .map(|f| f.value)
    };
    assert_eq!(value("play_count"), Some(FieldValue::Number(None)));
    assert_eq!(value("last_watched"), Some(FieldValue::Date(None)));

    lab.save(
        id,
        vec![
            ("play_count", FieldValue::Number(Some(3.0))),
            ("last_watched", FieldValue::Date(Some("2024-03-01".into()))),
        ],
    )
    .unwrap();
    let ItemDetail::Movie(m) = &lab.item(id).detail else {
        panic!("a movie");
    };
    assert_eq!(m.play_count, 3);
    let stored = m.last_watched_ms.expect("a last-watched time");
    assert_eq!(
        value("last_watched"),
        Some(FieldValue::Date(Some("2024-03-01".into())))
    );

    // Saving the same day again keeps the stored time of day.
    lab.save(
        id,
        vec![("last_watched", FieldValue::Date(Some("2024-03-01".into())))],
    )
    .unwrap();
    let ItemDetail::Movie(m) = &lab.item(id).detail else {
        panic!("a movie");
    };
    assert_eq!(m.last_watched_ms, Some(stored));

    assert!(matches!(
        lab.save(id, vec![("play_count", FieldValue::Number(Some(-1.0)))]),
        Err(EditError::Invalid(_))
    ));
}

#[test]
fn a_certification_is_edited_as_readable_text_and_stored_as_its_token() {
    let lab = Lab::new();
    let (id, _) = lab.movie();
    let mut item = lab.item(id);
    if let ItemDetail::Movie(m) = &mut item.detail {
        m.certification = cv_domain::Certification("US_R".into());
    }
    lab.db
        .write(move |tx| cv_db::repo::update_item(tx, &item))
        .unwrap();

    let snapshot = lab.service.load(id).unwrap();
    let field = snapshot
        .fields
        .iter()
        .find(|f| f.key == "certification")
        .unwrap();
    assert_eq!(field.value, FieldValue::Text("US: R".into()));

    for typed in ["R", "US: R", "US_R"] {
        lab.save(id, vec![("certification", FieldValue::Text(typed.into()))])
            .unwrap();
        let ItemDetail::Movie(m) = &lab.item(id).detail else {
            panic!("a movie");
        };
        assert_eq!(m.certification.0, "US_R", "{typed}");
    }
}

#[test]
fn only_fields_of_the_kind_can_be_locked() {
    let lab = Lab::new();
    let (id, _) = lab.movie();
    assert!(matches!(
        lab.service.set_field_lock(id, "status", true),
        Err(EditError::NotLockable(_))
    ));
    assert!(matches!(
        lab.service.set_field_lock(id, "nonsense", true),
        Err(EditError::NotLockable(_))
    ));
    assert!(matches!(
        lab.service.set_field_lock(ItemId(9999), "plot", true),
        Err(EditError::NotFound)
    ));
}

#[test]
fn the_item_lock_toggles() {
    let lab = Lab::new();
    let (id, _) = lab.movie();
    lab.service.set_item_locked(id, true).unwrap();
    assert!(lab.service.load(id).unwrap().item_locked);
    lab.service.set_item_locked(id, false).unwrap();
    assert!(!lab.service.load(id).unwrap().item_locked);
}

#[test]
fn cast_edits_keep_known_people_and_the_order_given() {
    let lab = Lab::new();
    let (id, _) = lab.movie();
    let person = |kind: &str, name: &str, role: &str| PersonValue {
        kind: kind.into(),
        name: name.into(),
        role: role.into(),
    };

    lab.save(
        id,
        vec![(
            "cast",
            FieldValue::People(vec![
                person("actor", "Robert De Niro", "Neil McCauley"),
                person("actor", "Al Pacino", "Vincent Hanna"),
                person("director", "Val Kilmer", "Chris Shiherlis"),
                person("actor", "  ", "ignored"),
            ]),
        )],
    )
    .unwrap();

    let item = lab.item(id);
    let cast: Vec<_> = item
        .core
        .credits
        .iter()
        .filter(|c| matches!(c.kind, CreditKind::Actor))
        .map(|c| (c.person.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(
        cast,
        vec![
            ("Robert De Niro", "Neil McCauley"),
            ("Al Pacino", "Vincent Hanna"),
            ("Val Kilmer", "Chris Shiherlis"),
        ],
        "a crew kind in the cast list becomes an actor"
    );
    let pacino = item
        .core
        .credits
        .iter()
        .find(|c| c.person.name == "Al Pacino")
        .unwrap();
    assert_eq!(pacino.person.ids.get("tmdb"), Some("1158"));
    assert!(
        item.core
            .credits
            .iter()
            .any(|c| c.kind == CreditKind::Director && c.person.name == "Michael Mann"),
        "the crew is not touched by a cast edit"
    );
}

#[test]
fn ids_ratings_trailers_and_artwork_save() {
    let lab = Lab::new();
    let (id, _) = lab.movie();

    lab.save(
        id,
        vec![
            (
                "ids",
                FieldValue::Pairs(vec![
                    Pair {
                        key: "tmdb".into(),
                        value: "949".into(),
                    },
                    Pair {
                        key: "imdb".into(),
                        value: "tt0113277".into(),
                    },
                ]),
            ),
            ("user_rating", FieldValue::Number(Some(8.5))),
            (
                "trailers",
                FieldValue::Trailers(vec![TrailerValue {
                    name: "Official".into(),
                    url: "https://example.test/watch".into(),
                    quality: "1080p".into(),
                    language: "en".into(),
                    in_nfo: true,
                }]),
            ),
            ("artwork.poster", text("https://example.test/poster.jpg")),
            ("edition", text("Director's Cut")),
        ],
    )
    .unwrap();

    let item = lab.item(id);
    assert_eq!(item.core.ids.get("tmdb"), Some("949"));
    assert_eq!(item.core.ratings.user_rating(), Some(8.5));
    assert_eq!(
        item.core
            .artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("https://example.test/poster.jpg")
    );
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("not a movie");
    };
    assert_eq!(movie.trailers.len(), 1);
    assert_eq!(movie.trailers[0].url, "https://example.test/watch");
    assert_eq!(movie.edition.name(), "Director's Cut");

    // Clearing works too.
    lab.save(
        id,
        vec![
            ("user_rating", FieldValue::Number(None)),
            ("trailers", FieldValue::Trailers(vec![])),
            ("artwork.poster", text("")),
        ],
    )
    .unwrap();
    let item = lab.item(id);
    assert_eq!(item.core.ratings.user_rating(), None);
    assert!(item.core.artwork_urls.is_empty());
}

#[test]
fn show_and_season_fields_save() {
    let lab = Lab::new();
    let show = Item::show("Severance");
    let show_id = lab.insert(show);

    lab.save(
        show_id,
        vec![
            ("status", text("continuing")),
            ("first_aired", FieldValue::Date(Some("2022-02-18".into()))),
            ("season_name.1", text("The Goat Season")),
            ("season_overview.1", text("Mark leads a team.")),
        ],
    )
    .unwrap();

    let snapshot = lab.service.load(show_id).unwrap();
    let get = |key: &str| {
        snapshot
            .fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.value.clone())
    };
    assert_eq!(get("status"), Some(text("continuing")));
    assert_eq!(get("season_name.1"), Some(text("The Goat Season")));
    assert_eq!(get("season_overview.1"), Some(text("Mark leads a team.")));
    assert_eq!(
        snapshot
            .fields
            .iter()
            .find(|f| f.key == "status")
            .unwrap()
            .lock
            .as_deref(),
        Some("status")
    );
}

#[test]
fn episode_numbers_per_ordering_save() {
    let lab = Lab::new();
    let show_id = lab.insert(Item::show("Severance"));
    let mut episode = Item::episode(show_id, "Good News About Hell");
    if let ItemDetail::Episode(e) = &mut episode.detail {
        e.numbers.set(EpisodeNumber::aired(1, 1));
    }
    let id = lab.insert(episode);

    lab.save(
        id,
        vec![(
            "numbers",
            FieldValue::Numbers(vec![
                NumberValue {
                    ordering: "aired".into(),
                    name: String::new(),
                    season: 1,
                    episode: 2,
                },
                NumberValue {
                    ordering: "dvd".into(),
                    name: String::new(),
                    season: 1,
                    episode: 3,
                },
            ]),
        )],
    )
    .unwrap();

    let ItemDetail::Episode(e) = lab.item(id).detail else {
        panic!("not an episode");
    };
    assert_eq!(e.numbers.len(), 2);
    assert_eq!(
        e.numbers
            .get(&cv_domain::EpisodeGroup::aired())
            .unwrap()
            .episode,
        2
    );

    let duplicate = lab.save(
        id,
        vec![(
            "numbers",
            FieldValue::Numbers(vec![
                NumberValue {
                    ordering: "dvd".into(),
                    name: String::new(),
                    season: 1,
                    episode: 1,
                },
                NumberValue {
                    ordering: "dvd".into(),
                    name: String::new(),
                    season: 1,
                    episode: 2,
                },
            ]),
        )],
    );
    assert!(matches!(duplicate, Err(EditError::Invalid(_))));
}

#[test]
fn every_kind_lists_its_fields_with_lock_keys() {
    let lab = Lab::new();
    let (movie, _) = lab.movie();
    let snapshot = lab.service.load(movie).unwrap();
    let keys: Vec<&str> = snapshot.fields.iter().map(|f| f.key.as_str()).collect();
    for expected in [
        "title",
        "sort_title",
        "tagline",
        "plot",
        "year",
        "release_date",
        "runtime",
        "certification",
        "genres",
        "studios",
        "countries",
        "spoken_languages",
        "edition",
        "media_source",
        "tags",
        "note",
        "user_rating",
        "ratings",
        "collection",
        "ids",
        "cast",
        "crew",
        "trailers",
        "artwork.poster",
        "artwork.fanart",
    ] {
        assert!(keys.contains(&expected), "movie lacks {expected}");
    }
    // Fields no scrape writes have no lock; scraped fields do.
    let lock_of = |key: &str| {
        snapshot
            .fields
            .iter()
            .find(|f| f.key == key)
            .unwrap()
            .lock
            .clone()
    };
    assert_eq!(lock_of("sort_title"), None);
    assert_eq!(lock_of("studios").as_deref(), Some("studio"));
    assert_eq!(lock_of("cast").as_deref(), Some("actors"));
    assert_eq!(lock_of("artwork.fanart").as_deref(), Some("fanart"));
    assert_eq!(lock_of("ratings").as_deref(), Some("rating"));
    assert_eq!(lock_of("collection").as_deref(), Some("collection"));

    let set = lab.insert(Item::set("Heat Collection"));
    let set_keys: Vec<String> = lab
        .service
        .load(set)
        .unwrap()
        .fields
        .into_iter()
        .map(|f| f.key)
        .collect();
    assert!(set_keys.contains(&"title".to_owned()) && set_keys.contains(&"plot".to_owned()));
}

fn rating(source: &str, value: f64, max: u32, votes: u32) -> RatingValue {
    RatingValue {
        source: source.to_owned(),
        value,
        max,
        votes,
    }
}

#[test]
fn provider_ratings_are_edited_in_place_and_the_users_own_rating_stays() {
    let lab = Lab::new();
    let (id, folder) = lab.movie();
    lab.save(id, vec![("user_rating", FieldValue::Number(Some(9.0)))])
        .unwrap();

    let report = lab
        .save(
            id,
            vec![(
                "ratings",
                FieldValue::Ratings(vec![
                    rating("imdb", 8.3, 10, 700_000),
                    rating("metacritic", 76.0, 100, 22),
                ]),
            )],
        )
        .unwrap();
    assert_eq!(report.changed, ["ratings"]);
    assert_eq!(report.previous[0].value, FieldValue::Ratings(vec![]));
    let item = lab.item(id);
    let imdb = item.core.ratings.get("imdb").unwrap().clone();
    assert!((imdb.value - 8.3).abs() < 1e-6 && imdb.votes == 700_000);
    assert_eq!(item.core.ratings.get("metacritic").unwrap().max_value, 100);
    assert_eq!(item.core.ratings.user_rating(), Some(9.0));
    // It reads back as typed: 8.3, not the f32 8.300000190734863.
    assert_eq!(
        read_field(&item, "ratings"),
        Some(FieldValue::Ratings(vec![
            rating("imdb", 8.3, 10, 700_000),
            rating("metacritic", 76.0, 100, 22),
        ]))
    );
    assert!(nfo_text(&folder).contains("imdb"));

    // An unchanged rating keeps its fetch time; a removed one is gone.
    lab.save(
        id,
        vec![(
            "ratings",
            FieldValue::Ratings(vec![rating("imdb", 8.3, 10, 700_000)]),
        )],
    )
    .unwrap();
    let item = lab.item(id);
    assert_eq!(
        item.core.ratings.get("imdb").unwrap().fetched_at_ms,
        imdb.fetched_at_ms
    );
    assert!(item.core.ratings.get("metacritic").is_none());
    assert_eq!(item.core.ratings.user_rating(), Some(9.0));

    // Out of range, a repeated source and the user's own key are refused, saving nothing.
    for bad in [
        vec![rating("imdb", 11.0, 10, 0)],
        vec![rating("imdb", 7.0, 10, 0), rating("IMDb", 6.0, 10, 0)],
        vec![rating("user", 7.0, 10, 0)],
        vec![rating(" ", 7.0, 10, 0)],
        vec![rating("tmdb", 7.0, 0, 0)],
    ] {
        let error = lab.save(id, vec![("ratings", FieldValue::Ratings(bad))]);
        assert!(matches!(error, Err(EditError::Invalid(_))), "{error:?}");
    }
    assert!(lab.item(id).core.ratings.get("imdb").is_some());
}

#[test]
fn a_movie_joins_and_leaves_a_collection_in_place_with_undo() {
    let lab = Lab::new();
    let (id, _) = lab.movie();
    let set = lab.insert(Item::set("Heat Collection"));
    lab.insert(Item::set("Another Collection"));

    let snapshot = lab.service.load(id).unwrap();
    assert_eq!(
        snapshot.collections,
        ["Another Collection", "Heat Collection"]
    );
    let field = snapshot
        .fields
        .iter()
        .find(|f| f.key == "collection")
        .unwrap();
    assert_eq!(field.value, text(""));

    let report = lab
        .save(id, vec![("collection", text("heat collection"))])
        .unwrap();
    assert_eq!(report.changed, ["collection"]);
    assert_eq!(report.previous[0].value, text(""));
    assert_eq!(lab.item(id).parent_id, Some(set));
    let fields = lab.service.load(id).unwrap().fields;
    let field = fields.iter().find(|f| f.key == "collection").unwrap();
    assert_eq!(field.value, text("Heat Collection"));

    // The same collection again is no change.
    let same = lab
        .save(id, vec![("collection", text("Heat Collection"))])
        .unwrap();
    assert!(same.changed.is_empty());

    // A collection that does not exist is refused; nothing changes.
    let error = lab.save(id, vec![("collection", text("Nope"))]);
    assert!(matches!(error, Err(EditError::Invalid(_))), "{error:?}");
    assert_eq!(lab.item(id).parent_id, Some(set));

    // Undo is a save of the previous value: out of any collection.
    lab.save(id, vec![("collection", text(""))]).unwrap();
    assert_eq!(lab.item(id).parent_id, None);

    // Only a movie has the field.
    let other = lab.service.load(set).unwrap();
    assert!(other.fields.iter().all(|f| f.key != "collection"));
    assert!(other.collections.is_empty());
    let refused = lab.save(set, vec![("collection", text("Heat Collection"))]);
    assert!(matches!(refused, Err(EditError::Invalid(_))), "{refused:?}");
}

//! Tests of collections over a real database and a scripted provider with no network. They follow
//! the scenarios of the `movie-collections` spec.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use cv_db::Database;
use cv_db::repo::{get_item, insert_item, set_field_lock, set_item_locked, update_item};
use cv_domain::{
    CalendarDate, Item, ItemDetail, ItemId, ItemKind, MediaFile, MediaFileType, MetaField,
    MissingMovie, ProviderIds, id_key,
};
use cv_providers::model::{
    Capabilities, MediaKind, ProviderCollection, RequestOptions, SearchHit, SearchQuery,
};
use cv_providers::{MetadataProvider, Provider, ProviderError};

use super::store;
use super::{CollectionError, CollectionService};
use crate::matching::{MatchSettings, accept_candidate, candidate_from_hit};
use crate::nfo::{NfoService, NfoSettings};

fn date(y: i32, m: u8, d: u8) -> CalendarDate {
    CalendarDate::new(y, m, d).unwrap()
}

/// One part of a scripted collection.
struct Part {
    tmdb: &'static str,
    title: &'static str,
    released: Option<CalendarDate>,
}

struct ScriptedSet {
    id: &'static str,
    name: &'static str,
    overview: &'static str,
    parts: Vec<Part>,
}

/// A TMDB stand-in: movies by TMDB ID that know their collection, and the collections.
struct Tmdb {
    sets: Vec<ScriptedSet>,
    collection_calls: AtomicU32,
    detail_calls: AtomicU32,
}

impl Tmdb {
    fn new() -> Self {
        let dune = ScriptedSet {
            id: "726871",
            name: "Dune Collection",
            overview: "The saga of Arrakis.",
            parts: vec![
                Part {
                    tmdb: "438631",
                    title: "Dune",
                    released: Some(date(2021, 10, 22)),
                },
                Part {
                    tmdb: "693134",
                    title: "Dune: Part Two",
                    released: Some(date(2024, 2, 27)),
                },
                Part {
                    tmdb: "1000001",
                    title: "Dune: Part Three",
                    released: Some(date(2099, 12, 18)),
                },
                Part {
                    tmdb: "1000002",
                    title: "Dune: Untitled",
                    released: None,
                },
            ],
        };
        let alien = ScriptedSet {
            id: "8091",
            name: "Alien Collection",
            overview: "",
            parts: vec![
                Part {
                    tmdb: "348",
                    title: "Alien",
                    released: Some(date(1979, 5, 25)),
                },
                Part {
                    tmdb: "679",
                    title: "Aliens",
                    released: Some(date(1986, 7, 18)),
                },
                Part {
                    tmdb: "8077",
                    title: "Alien 3",
                    released: Some(date(1992, 5, 22)),
                },
            ],
        };
        Self {
            sets: vec![dune, alien],
            collection_calls: AtomicU32::new(0),
            detail_calls: AtomicU32::new(0),
        }
    }

    fn set_of_movie(&self, tmdb: &str) -> Option<&ScriptedSet> {
        self.sets
            .iter()
            .find(|s| s.parts.iter().any(|p| p.tmdb == tmdb))
    }

    fn title_of(&self, tmdb: &str) -> Option<(&'static str, Option<CalendarDate>)> {
        for set in &self.sets {
            if let Some(p) = set.parts.iter().find(|p| p.tmdb == tmdb) {
                return Some((p.title, p.released));
            }
        }
        match tmdb {
            "550" => Some(("Fight Club", Some(date(1999, 10, 15)))),
            // Says it is in a collection the provider cannot produce.
            "777" => Some(("Ghost Film", Some(date(2001, 1, 1)))),
            _ => None,
        }
    }
}

#[async_trait]
impl Provider for Tmdb {
    fn id(&self) -> &'static str {
        "tmdb"
    }
    fn name(&self) -> &'static str {
        "Scripted TMDB"
    }
}

#[async_trait]
impl MetadataProvider for Tmdb {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: false,
            orderings: Vec::new(),
            lookup_ids: vec!["tmdb"],
            best_effort: false,
        }
    }

    async fn search(
        &self,
        _query: &SearchQuery,
        _options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        Ok(Vec::new())
    }

    async fn lookup(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        Ok(None)
    }

    async fn details(
        &self,
        _kind: MediaKind,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        self.detail_calls.fetch_add(1, Ordering::Relaxed);
        let tmdb = ids.get("tmdb").unwrap_or_default();
        let Some((title, released)) = self.title_of(tmdb) else {
            return Err(ProviderError::NotFound("tmdb".to_owned()));
        };
        let mut item = Item::movie(title);
        item.core.ids.set("tmdb", tmdb);
        if let Some(set) = self.set_of_movie(tmdb) {
            item.core.ids.set(id_key::TMDB_SET, set.id);
        }
        if tmdb == "777" {
            item.core.ids.set(id_key::TMDB_SET, "999");
        }
        if let (Some(date), ItemDetail::Movie(fields)) = (released, &mut item.detail) {
            fields.release_date = Some(date);
            item.core.year = u16::try_from(date.year()).unwrap_or(0);
        }
        Ok(item)
    }

    fn offers_collections(&self) -> bool {
        true
    }

    async fn search_collections(
        &self,
        title: &str,
        _options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        let needle = title.to_lowercase();
        Ok(self
            .sets
            .iter()
            .filter(|s| s.name.to_lowercase().contains(&needle))
            .map(|s| {
                let mut ids = ProviderIds::new();
                ids.set(id_key::TMDB_SET, s.id);
                SearchHit {
                    provider: "tmdb".to_owned(),
                    ids,
                    title: s.name.to_owned(),
                    overview: s.overview.to_owned(),
                    ..SearchHit::default()
                }
            })
            .collect())
    }

    async fn collection(
        &self,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<ProviderCollection, ProviderError> {
        self.collection_calls.fetch_add(1, Ordering::Relaxed);
        let wanted = ids.get(id_key::TMDB_SET).unwrap_or_default();
        let Some(set) = self.sets.iter().find(|s| s.id == wanted) else {
            return Err(ProviderError::NotFound("tmdb".to_owned()));
        };
        let mut item = Item::set(set.name);
        item.core.plot = set.overview.to_owned();
        item.core.ids.set(id_key::TMDB_SET, set.id);
        let movies = set
            .parts
            .iter()
            .map(|p| {
                let mut ids = ProviderIds::new();
                ids.set("tmdb", p.tmdb);
                MissingMovie {
                    title: p.title.to_owned(),
                    year: p
                        .released
                        .map_or(0, |d| u16::try_from(d.year()).unwrap_or(0)),
                    release_date: p.released,
                    poster_url: format!("https://img.invalid/{}.jpg", p.tmdb),
                    ids,
                    ..MissingMovie::default()
                }
            })
            .collect();
        Ok(ProviderCollection { item, movies })
    }
}

/// A provider that knows no collections (every other metadata provider).
struct Plain;

#[async_trait]
impl Provider for Plain {
    fn id(&self) -> &'static str {
        "plain"
    }
    fn name(&self) -> &'static str {
        "Plain"
    }
}

#[async_trait]
impl MetadataProvider for Plain {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: true,
            orderings: Vec::new(),
            lookup_ids: Vec::new(),
            best_effort: false,
        }
    }
    async fn search(
        &self,
        _query: &SearchQuery,
        _options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        Ok(Vec::new())
    }
    async fn lookup(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        Ok(None)
    }
    async fn details(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        Err(ProviderError::NotFound("plain".to_owned()))
    }
}

struct Lab {
    root: tempfile::TempDir,
    db: Arc<Database>,
    nfo: Arc<NfoService>,
    tmdb: Tmdb,
}

impl Lab {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(root.path().join("library.db")).unwrap());
        let nfo = Arc::new(NfoService::new(Arc::clone(&db), root.path().join("undo")));
        Self {
            root,
            db,
            nfo,
            tmdb: Tmdb::new(),
        }
    }

    fn service(&self) -> CollectionService {
        CollectionService::new(Arc::clone(&self.db), Arc::clone(&self.nfo))
    }

    fn providers(&self) -> Vec<&dyn MetadataProvider> {
        vec![&Plain, &self.tmdb]
    }

    fn insert(&self, item: Item) -> ItemId {
        self.db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    fn item(&self, id: ItemId) -> Item {
        self.db.read(|c| get_item(c, id)).unwrap().unwrap()
    }

    /// A collection the library already has (a scrape or an NFO made it) with its movies in it.
    fn collection_of(&self, title: &str, plot: &str, movies: &[ItemId]) -> ItemId {
        let mut set = Item::set(title);
        set.core.plot = plot.to_owned();
        let set = self.insert(set);
        let movies = movies.to_vec();
        store::write(&self.db, move |tx| {
            store::assign(tx, &movies, Some(set), false)
        })
        .unwrap();
        set
    }

    /// A movie with its release date and TMDB ID, in a folder with a video file.
    fn movie(&self, title: &str, tmdb: &str, released: Option<CalendarDate>) -> (ItemId, PathBuf) {
        let folder = self.root.path().join("Movies").join(title.replace(':', ""));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("movie.mkv"), b"video").unwrap();
        let mut item = Item::movie(title);
        item.core.path = folder.to_string_lossy().into_owned();
        item.core.ids.set("tmdb", tmdb);
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            folder.to_string_lossy(),
            "movie.mkv",
        ));
        if let (Some(date), ItemDetail::Movie(fields)) = (released, &mut item.detail) {
            fields.release_date = Some(date);
            item.core.year = u16::try_from(date.year()).unwrap_or(0);
        }
        (self.insert(item), folder)
    }

    fn dune(&self) -> ItemId {
        self.movie("Dune", "438631", Some(date(2021, 10, 22))).0
    }

    fn part_two(&self) -> ItemId {
        self.movie("Dune: Part Two", "693134", Some(date(2024, 2, 27)))
            .0
    }

    fn parent_of(&self, movie: ItemId) -> Option<ItemId> {
        self.item(movie).parent_id
    }

    /// Every collection the library keeps, listed or not.
    fn sets(&self) -> Vec<(ItemId, String)> {
        self.db
            .read(|c| {
                let mut stmt =
                    c.prepare("SELECT id, title FROM item WHERE kind = 'set' ORDER BY id")?;
                let rows = stmt
                    .query_map([], |r| Ok((ItemId(r.get(0)?), r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap()
    }
}

fn nfo_settings() -> NfoSettings {
    NfoSettings::default()
}

fn request() -> RequestOptions {
    RequestOptions::default()
}

fn set_by_name(lab: &Lab, name: &str) -> ItemId {
    lab.sets()
        .into_iter()
        .find(|(_, title)| title == name)
        .unwrap_or_else(|| panic!("no collection called {name}"))
        .0
}

// ---- collection page ------------------------------------------------------------------------

#[test]
fn a_collection_page_lists_its_owned_movies_in_release_order_with_the_plot() {
    let lab = Lab::new();
    let part_two = lab.part_two();
    let dune = lab.dune();
    // Inserted newest first: the page still orders by release date.
    let service = lab.service();
    let set = lab.collection_of("Dune Collection", "The saga of Arrakis.", &[part_two, dune]);

    let page = service.page(set).unwrap();
    assert_eq!(page.title, "Dune Collection");
    assert_eq!(page.plot, "The saga of Arrakis.");
    let titles: Vec<_> = page.entries.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["Dune", "Dune: Part Two"]);
    assert!(page.entries.iter().all(|e| e.is_owned()));
    assert_eq!((page.owned, page.total), (2, 2));
}

#[tokio::test]
async fn missing_movies_are_ghosted_with_counts() {
    let lab = Lab::new();
    // The library has Alien and Aliens; Alien 3 is the third film.
    let alien = lab.movie("Alien", "348", Some(date(1979, 5, 25))).0;
    let aliens = lab.movie("Aliens", "679", Some(date(1986, 7, 18))).0;
    let service = lab.service();
    service
        .find(
            &lab.providers(),
            &[alien, aliens],
            &request(),
            &nfo_settings(),
        )
        .await
        .unwrap();
    let set = set_by_name(&lab, "Alien Collection");

    let page = service.page(set).unwrap();
    assert_eq!((page.owned, page.total), (2, 3), "2 of 3 owned");
    let ghost: Vec<_> = page.entries.iter().filter(|e| !e.is_owned()).collect();
    assert_eq!(ghost.len(), 1);
    assert_eq!(ghost[0].title, "Alien 3");
    assert_eq!(ghost[0].year, 1992);
    assert_eq!(ghost[0].poster_url, "https://img.invalid/8077.jpg");
    // Release order puts the ghost after the films that came out earlier.
    assert_eq!(page.entries.last().unwrap().title, "Alien 3");
}

#[tokio::test]
async fn unreleased_and_undated_films_never_count_as_missing() {
    let lab = Lab::new();
    let dune = lab.dune();
    let service = lab.service();
    service
        .find(&lab.providers(), &[dune], &request(), &nfo_settings())
        .await
        .unwrap();
    let set = set_by_name(&lab, "Dune Collection");

    let page = service.page(set).unwrap();
    // Part Two is released and missing; Part Three (2099) and the undated one are left out.
    assert_eq!((page.owned, page.total), (1, 2));
    let titles: Vec<_> = page.entries.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["Dune", "Dune: Part Two"]);
    assert!(
        service.list().unwrap().is_empty(),
        "one owned movie is not enough to list the collection"
    );
}

#[tokio::test]
async fn a_movie_found_later_leaves_the_missing_list_at_once() {
    let lab = Lab::new();
    let dune = lab.dune();
    let service = lab.service();
    service
        .find(&lab.providers(), &[dune], &request(), &nfo_settings())
        .await
        .unwrap();
    let set = set_by_name(&lab, "Dune Collection");
    assert_eq!(service.page(set).unwrap().total, 2);

    let part_two = lab.part_two();
    service
        .find(&lab.providers(), &[part_two], &request(), &nfo_settings())
        .await
        .unwrap();
    let page = service.page(set).unwrap();
    assert_eq!((page.owned, page.total), (2, 2));
    assert!(page.entries.iter().all(|e| e.is_owned()));
}
#[tokio::test]
async fn a_collection_is_listed_only_while_two_owned_movies_are_in_it() {
    let lab = Lab::new();
    let dune = lab.dune();
    let service = lab.service();
    service
        .find(&lab.providers(), &[dune], &request(), &nfo_settings())
        .await
        .unwrap();

    // One owned movie: the collection is kept, so its page still opens, but it is not listed.
    let set = set_by_name(&lab, "Dune Collection");
    assert!(service.list().unwrap().is_empty());
    assert_eq!(service.page(set).unwrap().owned, 1);

    // The second film arrives and the collection returns.
    let part_two = lab.part_two();
    service
        .find(&lab.providers(), &[part_two], &request(), &nfo_settings())
        .await
        .unwrap();
    let listed = service.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].id, listed[0].owned), (set, 2));

    // One film leaves again: hidden, not deleted.
    lab.db
        .write(move |tx| {
            let mut movie = get_item(tx, part_two)?.unwrap();
            movie.parent_id = None;
            update_item(tx, &movie)
        })
        .unwrap();
    assert!(service.list().unwrap().is_empty());
    assert_eq!(lab.sets().len(), 1);
}

// ---- automatic assignment -------------------------------------------------------------------

fn hit_of(tmdb: &str, title: &str) -> SearchHit {
    let mut ids = ProviderIds::new();
    ids.set("tmdb", tmdb);
    SearchHit {
        provider: "tmdb".to_owned(),
        ids,
        title: title.to_owned(),
        ..SearchHit::default()
    }
}

async fn scrape(lab: &Lab, movie: ItemId, tmdb: &str, title: &str, settings: &MatchSettings) {
    let ranked = cv_match::rank_candidates(
        &cv_match::normalize_query(title),
        vec![candidate_from_hit(&hit_of(tmdb, title))],
        &cv_match::LocalEvidence::default(),
    );
    accept_candidate(&lab.db, movie, &ranked[0], &lab.providers(), settings)
        .await
        .unwrap();
}

#[tokio::test]
async fn scraping_with_the_collection_field_assigns_and_creates_the_collection() {
    let lab = Lab::new();
    let (part_two, _) = lab.movie("Dune: Part Two", "693134", None);
    let settings = MatchSettings::default();

    scrape(&lab, part_two, "693134", "Dune: Part Two", &settings).await;

    let sets = lab.sets();
    assert_eq!(sets.len(), 1, "created from the TMDB data");
    assert_eq!(sets[0].1, "Dune Collection");
    let set = lab.item(sets[0].0);
    assert_eq!(set.core.plot, "The saga of Arrakis.");
    assert_eq!(set.core.ids.get("tmdb"), Some("726871"));
    assert_eq!(lab.parent_of(part_two), Some(sets[0].0));
    assert_eq!(
        lab.item(part_two).core.ids.get(id_key::TMDB_SET),
        Some("726871")
    );

    // The next film of the collection joins the same collection; it is fetched again but not
    // created twice.
    let (dune, _) = lab.movie("Dune", "438631", None);
    scrape(&lab, dune, "438631", "Dune", &settings).await;
    assert_eq!(lab.sets().len(), 1);
    assert_eq!(lab.parent_of(dune), Some(sets[0].0));
    let page = lab.service().page(sets[0].0).unwrap();
    assert_eq!((page.owned, page.total), (2, 2));
}

#[tokio::test]
async fn a_collection_named_after_another_one_takes_the_provider_name() {
    let lab = Lab::new();
    // The NFO named the collection wrongly, but it carries the Dune Collection's ID.
    let mut wrong = Item::set("Psycho Collection");
    wrong.core.plot = "Bates Motel.".into();
    wrong.core.ids.set("tmdb", "726871");
    let set = lab.insert(wrong);
    let part_two = lab.part_two();

    scrape(
        &lab,
        part_two,
        "693134",
        "Dune: Part Two",
        &MatchSettings::default(),
    )
    .await;

    assert_eq!(lab.parent_of(part_two), Some(set));
    let fixed = lab.item(set);
    assert_eq!(fixed.core.title, "Dune Collection");
    assert_eq!(fixed.core.plot, "The saga of Arrakis.");
}
#[tokio::test]
async fn a_movie_the_provider_puts_in_no_collection_leaves_a_provider_collection_only() {
    let lab = Lab::new();
    let mut provider_set = Item::set("Alien Collection");
    provider_set.core.ids.set("tmdb", "8091");
    let provider_set = lab.insert(provider_set);
    let own_set = lab.insert(Item::set("Favourites"));
    let (fight_club, _) = lab.movie("Fight Club", "550", None);
    let (in_own, _) = lab.movie("Fight Club copy", "550", None);
    lab.db
        .write(move |tx| {
            for (movie, set) in [(fight_club, provider_set), (in_own, own_set)] {
                let mut item = get_item(tx, movie)?.unwrap();
                item.parent_id = Some(set);
                update_item(tx, &item)?;
            }
            Ok(())
        })
        .unwrap();

    scrape(
        &lab,
        fight_club,
        "550",
        "Fight Club",
        &MatchSettings::default(),
    )
    .await;
    scrape(&lab, in_own, "550", "Fight Club", &MatchSettings::default()).await;

    assert_eq!(lab.parent_of(fight_club), None);
    assert_eq!(
        lab.parent_of(in_own),
        Some(own_set),
        "a collection made by hand stays"
    );
}

#[tokio::test]
async fn a_scrape_without_the_collection_field_leaves_collections_alone() {
    let lab = Lab::new();
    let (part_two, _) = lab.movie("Dune: Part Two", "693134", None);
    let mut settings = MatchSettings::default();
    settings.scrape.movie = settings
        .scrape
        .movie
        .iter()
        .filter(|f| *f != MetaField::Collection)
        .collect();

    scrape(&lab, part_two, "693134", "Dune: Part Two", &settings).await;

    assert!(lab.sets().is_empty());
    assert_eq!(lab.parent_of(part_two), None);
    assert_eq!(lab.tmdb.collection_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn a_scrape_leaves_collections_alone_when_automatic_grouping_is_off() {
    let lab = Lab::new();
    let (part_two, _) = lab.movie("Dune: Part Two", "693134", None);
    let settings = MatchSettings {
        scrape: crate::scrape_settings::ScrapeSettings::from_values(
            &[(
                crate::scrape_settings::AUTO_GROUP_KEY.to_owned(),
                serde_json::json!(false),
            )]
            .into_iter()
            .collect(),
        ),
        ..MatchSettings::default()
    };

    scrape(&lab, part_two, "693134", "Dune: Part Two", &settings).await;

    assert!(lab.sets().is_empty());
    assert_eq!(lab.parent_of(part_two), None);
    assert_eq!(lab.tmdb.collection_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn a_scrape_does_not_move_a_movie_with_a_locked_collection_field() {
    let lab = Lab::new();
    let (part_two, _) = lab.movie("Dune: Part Two", "693134", None);
    lab.db
        .write(move |tx| set_field_lock(tx, part_two, MetaField::Collection, true))
        .unwrap();
    // The lock also keeps the scrape from writing the collection ID.
    scrape(
        &lab,
        part_two,
        "693134",
        "Dune: Part Two",
        &MatchSettings::default(),
    )
    .await;
    assert!(lab.sets().is_empty());
    assert_eq!(lab.parent_of(part_two), None);
}

#[tokio::test]
async fn a_failing_collection_lookup_never_fails_the_scrape() {
    let lab = Lab::new();
    let (movie, _) = lab.movie("Ghost Film", "777", None);

    // The scrape writes the collection ID, then the collection cannot be fetched.
    scrape(&lab, movie, "777", "Ghost Film", &MatchSettings::default()).await;

    assert_eq!(lab.item(movie).core.title, "Ghost Film");
    assert_eq!(lab.item(movie).core.ids.get(id_key::TMDB_SET), Some("999"));
    assert!(lab.sets().is_empty());
    assert_eq!(lab.parent_of(movie), None);
}

// ---- find collections -----------------------------------------------------------------------

#[tokio::test]
async fn find_collections_assigns_a_selection_and_fetches_each_collection_once() {
    let lab = Lab::new();
    let dune = lab.dune();
    let part_two = lab.part_two();
    let alien = lab.movie("Alien", "348", Some(date(1979, 5, 25))).0;
    let loner = lab.movie("Fight Club", "550", Some(date(1999, 10, 15))).0;
    let unmatched = lab.insert(Item::movie("No Ids"));

    let service = lab.service();
    let outcome = service
        .find(
            &lab.providers(),
            &[dune, part_two, alien, loner, unmatched],
            &request(),
            &nfo_settings(),
        )
        .await
        .unwrap();

    let report = &outcome.report;
    assert_eq!(report.assigned.len(), 3);
    assert_eq!(report.created.len(), 2);
    assert_eq!(report.no_collection, 1, "Fight Club has no collection");
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].movie, unmatched);
    assert_eq!(
        lab.tmdb.collection_calls.load(Ordering::Relaxed),
        2,
        "one fetch per collection"
    );
    let dune_set = set_by_name(&lab, "Dune Collection");
    assert_eq!(lab.parent_of(dune), Some(dune_set));
    assert_eq!(lab.parent_of(part_two), Some(dune_set));
    assert_eq!(lab.parent_of(loner), None);

    // A second run changes nothing.
    let again = service
        .find(
            &lab.providers(),
            &[dune, part_two, alien],
            &request(),
            &nfo_settings(),
        )
        .await
        .unwrap();
    assert!(again.report.assigned.is_empty());
    assert_eq!(again.report.unchanged, 3);
    assert!(again.undo.is_none());
}

#[tokio::test]
async fn undoing_find_collections_puts_movies_back_and_removes_the_collections_it_made() {
    let lab = Lab::new();
    let dune = lab.dune();
    let existing = lab.collection_of("My Sci-Fi", "", &[dune]);
    assert_eq!(lab.parent_of(dune), Some(existing));

    let service = lab.service();
    let outcome = service
        .find(&lab.providers(), &[dune], &request(), &nfo_settings())
        .await
        .unwrap();
    let dune_set = set_by_name(&lab, "Dune Collection");
    assert_eq!(
        lab.parent_of(dune),
        Some(dune_set),
        "a movie is in one collection"
    );

    service
        .undo(outcome.undo.unwrap(), &nfo_settings())
        .unwrap();
    assert_eq!(lab.parent_of(dune), Some(existing));
    assert_eq!(lab.sets().len(), 1, "the collection the run made is gone");
    assert!(matches!(
        service.undo(outcome.undo.unwrap(), &nfo_settings()),
        Err(CollectionError::NothingToUndo)
    ));
}

#[tokio::test]
async fn find_collections_skips_locked_movies_and_needs_a_provider_with_collections() {
    let lab = Lab::new();
    let dune = lab.dune();
    let part_two = lab.part_two();
    lab.db
        .write(move |tx| set_item_locked(tx, &[part_two], true))
        .unwrap();
    let service = lab.service();
    let outcome = service
        .find(
            &lab.providers(),
            &[dune, part_two],
            &request(),
            &nfo_settings(),
        )
        .await
        .unwrap();
    assert_eq!(outcome.report.locked, 1);
    assert_eq!(lab.parent_of(part_two), None);

    let none: Vec<&dyn MetadataProvider> = vec![&Plain];
    let error = service
        .find(&none, &[dune], &request(), &nfo_settings())
        .await
        .unwrap_err();
    assert!(matches!(error, CollectionError::NoProvider));
}

// ---- deleting, finding ----------------------------------------------------------------------

#[test]
fn deleting_a_collection_keeps_its_movies_and_undo_restores_the_collection() {
    let lab = Lab::new();
    let service = lab.service();
    let movies: Vec<ItemId> = (1..=4)
        .map(|n| lab.movie(&format!("Part {n}"), &format!("90{n}"), None).0)
        .collect();
    let set = lab.collection_of("Quartet", "Four films.", &movies);

    let report = service.delete(set, &nfo_settings()).unwrap();
    assert_eq!(report.movies_kept, 4);
    assert!(lab.sets().is_empty());
    for movie in &movies {
        let item = lab.item(*movie);
        assert_eq!(
            item.kind(),
            ItemKind::Movie,
            "the movie is still in the library"
        );
        assert_eq!(item.parent_id, None, "without a collection");
    }
    assert!(matches!(
        service.delete(set, &nfo_settings()),
        Err(CollectionError::NotFound)
    ));

    let restored = service.undo(report.undo.unwrap(), &nfo_settings()).unwrap();
    let back = restored.restored_set.unwrap();
    assert_eq!(restored.movies, 4);
    assert_eq!(lab.item(back).core.title, "Quartet");
    assert_eq!(lab.item(back).core.plot, "Four films.");
    for movie in &movies {
        assert_eq!(lab.parent_of(*movie), Some(back));
    }
}

#[tokio::test]
async fn find_collections_adopts_a_collection_with_the_same_name() {
    let lab = Lab::new();
    let service = lab.service();
    let mine = lab.insert(Item::set("dune collection"));
    let dune = lab.dune();

    let outcome = service
        .find(&lab.providers(), &[dune], &request(), &nfo_settings())
        .await
        .unwrap();

    assert!(outcome.report.created.is_empty());
    assert_eq!(lab.parent_of(dune), Some(mine));
    assert_eq!(lab.sets().len(), 1);
    assert_eq!(lab.item(mine).core.ids.get("tmdb"), Some("726871"));
}

#[test]
fn ungrouped_movies_are_the_ones_with_an_id_and_no_collection() {
    let lab = Lab::new();
    let service = lab.service();
    let dune = lab.dune();
    let alien = lab.movie("Alien", "348", Some(date(1979, 5, 25))).0;
    let _no_ids = lab.insert(Item::movie("No Ids"));
    lab.collection_of("Epics", "", &[alien]);

    assert_eq!(service.ungrouped_movies().unwrap(), vec![dune]);
}

#[test]
fn today_decides_what_has_come_out() {
    let today = date(2025, 6, 1);
    let film = |release: Option<CalendarDate>, year: u16| MissingMovie {
        release_date: release,
        year,
        ..MissingMovie::default()
    };
    assert!(super::store::is_released(
        &film(Some(date(2025, 6, 1)), 2025),
        today
    ));
    assert!(!super::store::is_released(
        &film(Some(date(2025, 6, 2)), 2025),
        today
    ));
    assert!(super::store::is_released(&film(None, 2024), today));
    assert!(!super::store::is_released(&film(None, 2026), today));
    assert!(
        !super::store::is_released(&film(None, 0), today),
        "not announced"
    );
}

//! Episode scraping with selectable fields, and title-based episode search.
//!
//! A made-up show is listed by a fixture provider (`fixtures/episode_scraping/harbour_lights.json`)
//! in the shape a real provider answers: the aired list has a special and two seasons, the DVD
//! list renumbers the regular episodes. The tests scrape a library through the same write path
//! every scrape uses, so the field selection, the do-not-overwrite mode and the item and field
//! locks are the ones the spec describes.

use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cv_app::episode_order::EpisodeOrderer;
use cv_app::episode_scrape::{
    EpisodePick, EpisodeScrapeError, EpisodeScraper, scrape_after_show, search_titles,
};
use cv_app::matching::MatchSettings;
use cv_app::nfo::{NfoScope, NfoService, NfoSettings};
use cv_db::Database;
use cv_db::repo::{get_item, insert_item, set_field_lock, set_item_locked};
use cv_domain::{
    CalendarDate, Credit, CreditKind, EpisodeFields, EpisodeGroup, EpisodeNumber, EpisodeOrdering,
    Item, ItemDetail, ItemId, MediaFile, MediaFileType, MetaField, ProviderIds, Rating,
    ScrapeBlock, ScrapeOptions,
};
use cv_providers::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use cv_providers::{MetadataProvider, Provider, ProviderError};
use serde::Deserialize;

// ---- the fixture ------------------------------------------------------------------------------

#[derive(Deserialize)]
struct Fixture {
    show: String,
    tvdb: String,
    episodes: Vec<Listed>,
}

#[derive(Deserialize)]
struct Listed {
    id: String,
    aired: (u32, u32),
    #[serde(default)]
    dvd: Option<(u32, u32)>,
    title: String,
    plot: String,
    date: String,
    rating: f32,
    votes: u32,
    runtime: u32,
    guests: Vec<(String, String)>,
    directors: Vec<String>,
    writers: Vec<String>,
    thumb: String,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "fixtures/episode_scraping/harbour_lights.json"
    ))
    .expect("the fixture is valid")
}

fn date_of(text: &str) -> CalendarDate {
    let mut parts = text.split('-').map(|p| p.parse::<u32>().unwrap());
    let (y, m, d) = (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap(),
    );
    CalendarDate::new(
        i32::try_from(y).unwrap(),
        u8::try_from(m).unwrap(),
        u8::try_from(d).unwrap(),
    )
    .unwrap()
}

impl Listed {
    /// The episode as the provider returns it.
    fn item(&self, group: &EpisodeGroup) -> Item {
        let mut fields = EpisodeFields::default();
        fields
            .numbers
            .set(EpisodeNumber::aired(self.aired.0, self.aired.1));
        if let (Some(dvd), EpisodeOrdering::Dvd) = (self.dvd, group.ordering) {
            fields
                .numbers
                .set(EpisodeNumber::new(EpisodeGroup::dvd(), dvd.0, dvd.1));
        }
        fields.first_aired = Some(date_of(&self.date));
        fields.runtime_minutes = self.runtime;
        let mut item = Item::new(ItemDetail::Episode(fields));
        item.core.title = self.title.clone();
        item.core.plot = self.plot.clone();
        item.core.ids.set("tvdb", &self.id);
        item.core
            .ratings
            .insert(Rating::new("tvdb", self.rating, self.votes, 10));
        for (name, role) in &self.guests {
            item.core
                .credits
                .push(Credit::new(CreditKind::Guest, name, role));
        }
        for name in &self.directors {
            item.core
                .credits
                .push(Credit::new(CreditKind::Director, name, ""));
        }
        for name in &self.writers {
            item.core
                .credits
                .push(Credit::new(CreditKind::Writer, name, ""));
        }
        item.core
            .artwork_urls
            .insert(MediaFileType::Thumb, self.thumb.clone());
        item
    }
}

/// A provider that lists the fixture's episodes and remembers which orderings it was asked for.
struct Catalogue {
    fixture: Fixture,
    asked: Mutex<Vec<EpisodeGroup>>,
    /// The list is gone from the provider (a stale search).
    drop_ids: Mutex<Vec<String>>,
}

impl Catalogue {
    fn new() -> Self {
        Self {
            fixture: fixture(),
            asked: Mutex::new(Vec::new()),
            drop_ids: Mutex::new(Vec::new()),
        }
    }

    fn asked(&self) -> Vec<EpisodeGroup> {
        self.asked.lock().unwrap().clone()
    }
}

#[async_trait]
impl Provider for Catalogue {
    fn id(&self) -> &'static str {
        "tvdb"
    }
    fn name(&self) -> &'static str {
        "Fixture TVDB"
    }
}

#[async_trait]
impl MetadataProvider for Catalogue {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: false,
            shows: true,
            orderings: vec![EpisodeOrdering::Aired, EpisodeOrdering::Dvd],
            lookup_ids: vec!["tvdb"],
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
        kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        if kind != MediaKind::Show {
            return Err(ProviderError::NotFound("Fixture TVDB".to_owned()));
        }
        let mut show = Item::show(&self.fixture.show);
        show.core.ids.set("tvdb", &self.fixture.tvdb);
        show.core.plot = "A lighthouse crew and its odd harbour.".to_owned();
        Ok(show)
    }

    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        _options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        if show.get("tvdb") != Some(self.fixture.tvdb.as_str()) {
            return Err(ProviderError::NotFound("Fixture TVDB".to_owned()));
        }
        self.asked.lock().unwrap().push(ordering.clone());
        let dropped = self.drop_ids.lock().unwrap().clone();
        let list: Vec<Item> = self
            .fixture
            .episodes
            .iter()
            .filter(|e| !dropped.contains(&e.id))
            .filter(|e| ordering.ordering != EpisodeOrdering::Dvd || e.dvd.is_some())
            .map(|e| e.item(ordering))
            .collect();
        Ok(list)
    }
}

/// A provider with no episode lists at all.
struct MovieOnly;

#[async_trait]
impl Provider for MovieOnly {
    fn id(&self) -> &'static str {
        "movie-only"
    }
    fn name(&self) -> &'static str {
        "Movie only"
    }
}

#[async_trait]
impl MetadataProvider for MovieOnly {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: false,
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
        Err(ProviderError::NotFound("Movie only".to_owned()))
    }
}

// ---- the library ------------------------------------------------------------------------------

struct Library {
    dir: tempfile::TempDir,
    db: Arc<Database>,
    show: ItemId,
}

impl Library {
    fn new(fixture: &Fixture) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
        let mut show = Item::show(&fixture.show);
        show.core.ids.set("tvdb", &fixture.tvdb);
        let show = db.write(move |tx| insert_item(tx, &show)).unwrap();
        Self { dir, db, show }
    }

    /// An episode the scan found: a title from the file name, a plot of the user's own and the
    /// aired number the name gave (none when `aired` is `None`).
    fn add(&self, title: &str, aired: Option<(u32, u32)>) -> ItemId {
        let mut episode = Item::episode(self.show, title);
        episode.core.plot = "My own plot".to_owned();
        if let ItemDetail::Episode(fields) = &mut episode.detail {
            if let Some((s, e)) = aired {
                fields.numbers.set(EpisodeNumber::aired(s, e));
            }
            fields.first_aired = CalendarDate::new(1999, 1, 1);
        }
        self.db.write(move |tx| insert_item(tx, &episode)).unwrap()
    }

    fn item(&self, id: ItemId) -> Item {
        self.db.read(|c| get_item(c, id)).unwrap().unwrap()
    }

    fn fields(&self, id: ItemId) -> EpisodeFields {
        match self.item(id).detail {
            ItemDetail::Episode(fields) => fields,
            other => panic!("not an episode: {other:?}"),
        }
    }

    /// Whether the show's episode list was fetched, so its missing list means something.
    fn list_fetched(&self) -> bool {
        match self.item(self.show).detail {
            ItemDetail::Show(fields) => fields.episode_list_fetched,
            other => panic!("not a show: {other:?}"),
        }
    }

    fn missing_titles(&self) -> Vec<String> {
        match self.item(self.show).detail {
            ItemDetail::Show(fields) => fields
                .missing_episodes
                .into_iter()
                .map(|m| m.title)
                .collect(),
            other => panic!("not a show: {other:?}"),
        }
    }

    fn lock_field(&self, id: ItemId, field: MetaField) {
        self.db
            .write(move |tx| set_field_lock(tx, id, field, true))
            .unwrap();
    }

    fn lock_item(&self, id: ItemId) {
        self.db
            .write(move |tx| set_item_locked(tx, &[id], true))
            .unwrap();
    }

    fn scraper(&self) -> EpisodeScraper {
        EpisodeScraper::new(Arc::clone(&self.db))
    }
}

fn request() -> RequestOptions {
    RequestOptions::default()
}

fn all() -> ScrapeOptions {
    ScrapeOptions::all(cv_domain::ItemKind::Episode)
}

fn names(item: &Item, kinds: &[CreditKind]) -> Vec<String> {
    item.core
        .credits
        .iter()
        .filter(|c| kinds.contains(&c.kind))
        .map(|c| c.person.name.clone())
        .collect()
}

// ---- scraping by number -----------------------------------------------------------------------

/// The episode fields the spec lists: title, plot, aired date, rating, guests, directors, writers
/// and thumb, written by number from the aired list.
#[tokio::test]
async fn every_selected_field_of_the_numbered_episodes_is_scraped() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let first = library.add("harbour.lights.s01e01.mkv", Some((1, 1)));
    let second = library.add("harbour.lights.s01e02.mkv", Some((1, 2)));
    assert!(!library.list_fetched(), "an ID alone is no episode list");

    let report = library
        .scraper()
        .scrape_show(library.show, &catalogue, &request(), &all())
        .await
        .unwrap();

    assert!(library.list_fetched());
    assert_eq!(report.group, EpisodeGroup::aired());
    assert_eq!((report.matched, report.changed), (2, 2));
    assert!(report.unmatched.is_empty());
    assert_eq!(catalogue.asked(), vec![EpisodeGroup::aired()]);

    let item = library.item(first);
    assert_eq!(item.core.title, "The Night Shift");
    assert_eq!(
        item.core.plot,
        "Mara takes the night watch and finds the lamp already lit."
    );
    assert_eq!(
        library.fields(first).first_aired,
        CalendarDate::new(2021, 3, 1)
    );
    assert_eq!(library.fields(first).runtime_minutes, 45);
    let rating = item.core.ratings.get("tvdb").expect("rating stored");
    assert!((rating.value - 8.1).abs() < 1e-3 && rating.votes == 120);
    assert_eq!(
        names(&item, &[CreditKind::Guest]),
        ["Odile Brandt", "Tomas Veld"]
    );
    assert_eq!(names(&item, &[CreditKind::Director]), ["Ines Marlow"]);
    assert_eq!(
        names(&item, &[CreditKind::Writer]),
        ["Pavel Orr", "Sunniva Dahl"]
    );
    assert_eq!(
        item.core
            .artwork_urls
            .get(&MediaFileType::Thumb)
            .map(String::as_str),
        Some("https://img.example.test/harbour/e101.jpg")
    );
    assert_eq!(item.core.ids.get("tvdb"), Some("e101"), "the link is kept");
    assert_eq!(library.item(second).core.title, "Salt and Rope");

    // The list's other entries are the show's missing episodes.
    assert_eq!(report.missing, Some(4));
    assert_eq!(
        library.missing_titles(),
        [
            "Behind the Lamps",
            "The Quiet Tide",
            "Second Light",
            "Fog Signals"
        ]
    );
}

/// reg2-4: an episode with a second file (the original and its REPACK) is one episode in the
/// report, as on the show page, while both files get the scraped values.
#[tokio::test]
async fn two_files_of_one_episode_count_as_one_scraped_episode() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let original = library.add("harbour.lights.s01e01.mkv", Some((1, 1)));
    let repack = library.add("harbour.lights.s01e01.repack.mkv", Some((1, 1)));
    library.add("harbour.lights.s01e02.mkv", Some((1, 2)));

    let report = library
        .scraper()
        .scrape_show(library.show, &catalogue, &request(), &all())
        .await
        .unwrap();

    assert_eq!((report.matched, report.changed), (2, 2), "{report:?}");
    assert_eq!(report.episodes.len(), 3, "every file is written");
    assert_eq!(library.item(original).core.title, "The Night Shift");
    assert_eq!(library.item(repack).core.title, "The Night Shift");
}

/// Spec scenario "Only refresh ratings and artwork": the other fields stay as they were.
#[tokio::test]
async fn only_the_chosen_fields_change() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let id = library.add("harbour.lights.s01e01.mkv", Some((1, 1)));

    let options =
        ScrapeOptions::only([MetaField::Rating, MetaField::Artwork(MediaFileType::Thumb)]);
    let report = library
        .scraper()
        .scrape_show(library.show, &catalogue, &request(), &options)
        .await
        .unwrap();

    let item = library.item(id);
    assert_eq!(report.episodes[0].outcome.written.len(), 2);
    assert!(item.core.ratings.get("tvdb").is_some());
    assert!(item.core.artwork_urls.contains_key(&MediaFileType::Thumb));
    assert_eq!(item.core.title, "harbour.lights.s01e01.mkv");
    assert_eq!(item.core.plot, "My own plot");
    assert_eq!(
        library.fields(id).first_aired,
        CalendarDate::new(1999, 1, 1)
    );
    assert!(names(&item, &[CreditKind::Guest, CreditKind::Director]).is_empty());
}

/// An empty selection still links the episode (ID and numbers) but writes no field.
#[tokio::test]
async fn an_empty_selection_writes_no_field() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let id = library.add("harbour.lights.s01e01.mkv", Some((1, 1)));

    let report = library
        .scraper()
        .scrape_show(
            library.show,
            &catalogue,
            &request(),
            &ScrapeOptions::only([]),
        )
        .await
        .unwrap();

    assert_eq!(report.matched, 1);
    assert!(report.episodes[0].outcome.written.is_empty());
    let item = library.item(id);
    assert_eq!(item.core.title, "harbour.lights.s01e01.mkv");
    assert_eq!(item.core.ids.get("tvdb"), Some("e101"));
}

#[tokio::test]
async fn do_not_overwrite_fills_only_the_empty_fields() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let id = library.add("harbour.lights.s01e01.mkv", Some((1, 1)));

    let mut options = all();
    options.do_not_overwrite = true;
    let report = library
        .scraper()
        .scrape_show(library.show, &catalogue, &request(), &options)
        .await
        .unwrap();

    let item = library.item(id);
    assert_eq!(item.core.plot, "My own plot", "a filled field is kept");
    assert_eq!(
        library.fields(id).first_aired,
        CalendarDate::new(1999, 1, 1)
    );
    assert!(report.episodes[0].outcome.kept.contains(&MetaField::Plot));
    // Empty fields are filled.
    assert_eq!(names(&item, &[CreditKind::Director]), ["Ines Marlow"]);
    assert!(item.core.ratings.get("tvdb").is_some());
}

/// Spec scenario "Locked plot": every selected field updates except the plot. A locked episode
/// is not modified at all, numbers included.
#[tokio::test]
async fn locked_fields_and_locked_episodes_are_protected() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let plot_locked = library.add("harbour.lights.s01e01.mkv", Some((1, 1)));
    let item_locked = library.add("harbour.lights.s01e02.mkv", Some((1, 2)));
    library.lock_field(plot_locked, MetaField::Plot);
    library.lock_item(item_locked);

    let report = library
        .scraper()
        .scrape_show(library.show, &catalogue, &request(), &all())
        .await
        .unwrap();

    assert_eq!((report.matched, report.changed, report.locked), (2, 1, 1));
    assert_eq!(report.field_locked, 1);
    let first = library.item(plot_locked);
    assert_eq!(first.core.plot, "My own plot", "the locked plot stays");
    assert_eq!(first.core.title, "The Night Shift", "the rest updates");
    assert_eq!(
        report.episodes[0].outcome.locked,
        vec![MetaField::Plot],
        "the report says which field a lock kept"
    );

    let second = library.item(item_locked);
    assert_eq!(second.core.title, "harbour.lights.s01e02.mkv");
    assert!(second.core.ids.get("tvdb").is_none());
    assert_eq!(
        report.episodes[1].outcome.blocked,
        Some(ScrapeBlock::ItemLocked)
    );
}

/// Scraping reads the list in the show's chosen ordering, so the DVD numbers decide the match.
#[tokio::test]
async fn scraping_follows_the_shows_ordering() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    // "Fog Signals" is S02E02 aired and S01E04 on the discs; the file follows the discs.
    let id = library.add("fog.signals.mkv", Some((1, 4)));
    let orderer = EpisodeOrderer::new(Arc::clone(&library.db));
    orderer
        .apply(library.show, &EpisodeGroup::dvd(), &catalogue, &request())
        .await
        .unwrap();

    let report = library
        .scraper()
        .scrape_show(library.show, &catalogue, &request(), &all())
        .await
        .unwrap();

    assert_eq!(report.group, EpisodeGroup::dvd());
    assert_eq!(
        catalogue.asked(),
        vec![EpisodeGroup::dvd(), EpisodeGroup::dvd()],
        "the ordering switch and the scrape both ask for the DVD list"
    );
    assert_eq!(library.item(id).core.title, "Fog Signals");
    assert_eq!(
        library.missing_titles().len(),
        4,
        "the specials are not on the discs"
    );
}

// ---- one episode ------------------------------------------------------------------------------

#[tokio::test]
async fn one_episode_is_scraped_and_the_missing_list_is_left_alone() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let id = library.add("harbour.lights.s02e01.mkv", Some((2, 1)));
    let other = library.add("harbour.lights.s01e01.mkv", Some((1, 1)));

    let outcome = library
        .scraper()
        .scrape_episode(id, &catalogue, &request(), &all())
        .await
        .unwrap();

    assert_eq!(outcome.id, id);
    assert!(outcome.outcome.changed());
    assert_eq!(library.item(id).core.title, "Second Light");
    assert_eq!(
        library.item(other).core.title,
        "harbour.lights.s01e01.mkv",
        "the other episode is not touched"
    );
    assert!(library.missing_titles().is_empty());
}

#[tokio::test]
async fn an_unnumbered_episode_has_no_counterpart_to_scrape() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let id = library.add("quiet tide.mkv", None);

    let error = library
        .scraper()
        .scrape_episode(id, &catalogue, &request(), &all())
        .await
        .unwrap_err();

    assert!(
        matches!(error, EpisodeScrapeError::NoCounterpart),
        "{error}"
    );
    assert_eq!(error.code(), "no_match");
    assert_eq!(library.item(id).core.title, "quiet tide.mkv");
}

// ---- search by title --------------------------------------------------------------------------

/// Spec scenario "Unnumbered episode search": the user searches the show's episode list by title
/// and assigns the match.
#[tokio::test]
async fn an_unnumbered_episode_is_found_by_title_and_assigned() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let scraper = library.scraper();
    let numbered = library.add("harbour.lights.s01e01.mkv", Some((1, 1)));
    let id = library.add("quiet tide.mkv", None);
    scraper
        .scrape_show(library.show, &catalogue, &request(), &all())
        .await
        .unwrap();
    assert!(
        library
            .missing_titles()
            .contains(&"The Quiet Tide".to_owned())
    );

    // Part of a title, in any case, finds it.
    let hits = scraper
        .search(library.show, "quiet TIDE", &catalogue, &request())
        .await
        .unwrap();
    assert_eq!(hits[0].title, "The Quiet Tide");
    assert_eq!(hits[0].number, Some((1, 3)));
    assert_eq!(hits[0].first_aired, CalendarDate::new(2021, 3, 15));
    assert!(hits[0].plot.starts_with("Nothing moves"));
    assert!(
        scraper
            .search(library.show, "zeppelin", &catalogue, &request())
            .await
            .unwrap()
            .is_empty()
    );

    let report = scraper
        .assign(id, &hits[0].pick(), &catalogue, &request(), &all())
        .await
        .unwrap();

    assert_eq!(report.number, Some((1, 3)));
    assert!(report.outcome.changed());
    let item = library.item(id);
    assert_eq!(item.core.title, "The Quiet Tide");
    assert_eq!(item.core.ids.get("tvdb"), Some("e103"));
    assert_eq!(
        library
            .fields(id)
            .numbers
            .get(&EpisodeGroup::aired())
            .map(|n| (n.season, n.episode)),
        Some((1, 3))
    );
    assert_eq!(names(&item, &[CreditKind::Director]), ["Rhea Kimura"]);
    assert!(
        !library
            .missing_titles()
            .contains(&"The Quiet Tide".to_owned()),
        "the assigned episode is no longer missing"
    );
    assert_eq!(
        library.item(numbered).core.title,
        "The Night Shift",
        "other episodes are untouched"
    );
}

#[tokio::test]
async fn assigning_respects_the_field_selection_and_the_locks() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let scraper = library.scraper();
    let selective = library.add("a.mkv", None);
    let locked = library.add("b.mkv", None);
    library.lock_item(locked);
    let pick = search_titles(
        &catalogue
            .fixture
            .episodes
            .iter()
            .map(|e| e.item(&EpisodeGroup::aired()))
            .collect::<Vec<_>>(),
        &EpisodeGroup::aired(),
        "Salt and Rope",
    )[0]
    .pick();

    let only_rating = ScrapeOptions::only([MetaField::Rating]);
    let report = scraper
        .assign(selective, &pick, &catalogue, &request(), &only_rating)
        .await
        .unwrap();
    assert_eq!(report.number, Some((1, 2)));
    let item = library.item(selective);
    assert_eq!(item.core.title, "a.mkv", "the title was not selected");
    assert!(item.core.ratings.get("tvdb").is_some());

    let report = scraper
        .assign(locked, &pick, &catalogue, &request(), &all())
        .await
        .unwrap();
    assert_eq!(report.outcome.blocked, Some(ScrapeBlock::ItemLocked));
    assert_eq!(report.number, None);
    assert!(
        library
            .fields(locked)
            .numbers
            .get(&EpisodeGroup::aired())
            .is_none()
    );
    assert_eq!(library.item(locked).core.title, "b.mkv");
}

/// A pick whose entry has left the provider's list is refused and nothing is written.
#[tokio::test]
async fn a_pick_that_left_the_list_is_refused() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let id = library.add("quiet tide.mkv", None);
    let hits = library
        .scraper()
        .search(library.show, "quiet tide", &catalogue, &request())
        .await
        .unwrap();
    catalogue.drop_ids.lock().unwrap().push("e103".to_owned());

    let error = library
        .scraper()
        .assign(id, &hits[0].pick(), &catalogue, &request(), &all())
        .await
        .unwrap_err();

    assert!(
        matches!(error, EpisodeScrapeError::UnknownEpisode),
        "{error}"
    );
    assert_eq!(library.item(id).core.title, "quiet tide.mkv");
}

#[test]
fn a_pick_is_found_by_number_when_the_ids_are_gone() {
    let catalogue = Catalogue::new();
    let group = EpisodeGroup::aired();
    let list: Vec<Item> = catalogue
        .fixture
        .episodes
        .iter()
        .map(|e| e.item(&group))
        .collect();
    let hit = &search_titles(&list, &group, "second light")[0];
    let by_number = EpisodePick {
        ids: ProviderIds::default(),
        number: hit.number,
    };
    let index = cv_app::episode_scrape::title_search::find_pick(&list, &group, &by_number);
    assert_eq!(
        index.map(|i| list[i].core.title.as_str()),
        Some("Second Light")
    );
}

// ---- after a show scrape ----------------------------------------------------------------------

/// The spec: scraping a show also scrapes its episodes, with the user's episode field selection.
#[tokio::test]
async fn a_show_scrape_scrapes_its_episodes_with_the_episode_selection() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let id = library.add("harbour.lights.s01e01.mkv", Some((1, 1)));

    let mut settings = MatchSettings::default();
    settings.scrape.episode = [MetaField::Title, MetaField::Plot].into_iter().collect();
    let report = scrape_after_show(&library.db, library.show, &catalogue, &settings)
        .await
        .expect("the episodes were scraped");

    assert_eq!(report.matched, 1);
    let item = library.item(id);
    assert_eq!(item.core.title, "The Night Shift");
    assert!(
        item.core.ratings.get("tvdb").is_none(),
        "rating was not selected"
    );

    // A provider without episode lists leaves the show and its episodes alone.
    let before = library.item(id);
    assert!(
        scrape_after_show(&library.db, library.show, &MovieOnly, &settings)
            .await
            .is_none()
    );
    assert_eq!(library.item(id), before);
}

/// `tv.episode-order`: a show scraped for the first time starts in the chosen ordering.
#[tokio::test]
async fn a_new_show_starts_in_the_default_episode_order() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    // "Fog Signals" is S02E02 aired and S01E04 on the discs; the file follows the discs.
    let id = library.add("fog.signals.mkv", Some((1, 4)));
    let mut settings = MatchSettings::default();
    settings.scrape.episode_order = EpisodeOrdering::Dvd;

    scrape_after_show(&library.db, library.show, &catalogue, &settings)
        .await
        .expect("the episodes were scraped");

    let ItemDetail::Show(fields) = library.item(library.show).detail else {
        panic!("not a show");
    };
    assert_eq!(fields.episode_group, EpisodeGroup::dvd());
    assert_eq!(library.item(id).core.title, "Fog Signals");
}

/// A show that was scraped before keeps its ordering whatever the default says.
#[tokio::test]
async fn the_default_episode_order_does_not_move_a_show_that_was_scraped_before() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    library.add("harbour.lights.s01e01.mkv", Some((1, 1)));
    scrape_after_show(
        &library.db,
        library.show,
        &catalogue,
        &MatchSettings::default(),
    )
    .await
    .expect("the first scrape");

    let mut settings = MatchSettings::default();
    settings.scrape.episode_order = EpisodeOrdering::Dvd;
    scrape_after_show(&library.db, library.show, &catalogue, &settings)
        .await
        .expect("the second scrape");

    let ItemDetail::Show(fields) = library.item(library.show).detail else {
        panic!("not a show");
    };
    assert_eq!(fields.episode_group, EpisodeGroup::aired());
}

/// A provider without episode lists leaves the show in the aired order.
#[tokio::test]
async fn an_unavailable_default_episode_order_leaves_the_show_aired() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    library.add("harbour.lights.s01e01.mkv", Some((1, 1)));
    let mut settings = MatchSettings::default();
    settings.scrape.episode_order = EpisodeOrdering::Dvd;

    assert!(
        scrape_after_show(&library.db, library.show, &MovieOnly, &settings)
            .await
            .is_none()
    );

    let ItemDetail::Show(fields) = library.item(library.show).detail else {
        panic!("not a show");
    };
    assert_eq!(fields.episode_group, EpisodeGroup::aired());
}

/// Accepting a show candidate (an automatic match, a picked candidate, force best match) scrapes
/// the show and then its episodes in one go.
#[tokio::test]
async fn accepting_a_show_candidate_scrapes_its_episodes_too() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let id = library.add("harbour.lights.s01e02.mkv", Some((1, 2)));
    let query = cv_match::normalize_query("Harbour Lights");
    let candidate = cv_match::Candidate {
        provider: "tvdb".to_owned(),
        title: "Harbour Lights".to_owned(),
        ids: library.item(library.show).core.ids,
        ..cv_match::Candidate::default()
    };
    let ranked =
        cv_match::rank_candidates(&query, vec![candidate], &cv_match::LocalEvidence::default());

    let accepted = cv_app::matching::accept_candidate(
        &library.db,
        library.show,
        &ranked[0],
        &[&catalogue],
        &MatchSettings::default(),
    )
    .await
    .unwrap();

    assert!(accepted.scrape.changed());
    assert_eq!(
        library.item(library.show).core.plot,
        "A lighthouse crew and its odd harbour."
    );
    assert_eq!(library.item(id).core.title, "Salt and Rope");
    assert_eq!(library.missing_titles().len(), 5);
}

// ---- the NFO ----------------------------------------------------------------------------------

/// The scraped values reach the episode NFO the Rewrite NFO action writes.
#[tokio::test]
async fn scraped_fields_are_written_to_the_episode_nfo() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);

    let folder = library.dir.path().join("media/Harbour Lights");
    let season = folder.join("Season 1");
    std::fs::create_dir_all(&season).unwrap();
    let file = "Harbour Lights - S01E01.mkv";
    std::fs::write(season.join(file), b"video").unwrap();
    let show_id = library.show;
    let path = folder.to_string_lossy().into_owned();
    library
        .db
        .write(move |tx| {
            let mut show = get_item(tx, show_id)?.unwrap();
            show.core.path = path;
            cv_db::repo::update_item(tx, &show)
        })
        .unwrap();
    let id = library.add("Harbour Lights - S01E01", Some((1, 1)));
    let season_dir = season.to_string_lossy().into_owned();
    library
        .db
        .write(move |tx| {
            let mut episode = get_item(tx, id)?.unwrap();
            episode
                .core
                .media_files
                .push(MediaFile::new(MediaFileType::Video, season_dir, file));
            cv_db::repo::update_item(tx, &episode)
        })
        .unwrap();

    library
        .scraper()
        .scrape_show(library.show, &catalogue, &request(), &all())
        .await
        .unwrap();
    let service = NfoService::new(Arc::clone(&library.db), library.dir.path().join("undo"));
    let report = service
        .rewrite(&NfoScope::Items(vec![id]), &NfoSettings::default())
        .unwrap();
    assert_eq!(report.totals.failed, 0, "{:?}", report.units);

    let nfo = std::fs::read_to_string(season.join("Harbour Lights - S01E01.nfo")).unwrap();
    for expected in [
        "<title>The Night Shift</title>",
        "<plot>Mara takes the night watch and finds the lamp already lit.</plot>",
        "<aired>2021-03-01</aired>",
        "<runtime>45</runtime>",
        "<name>Odile Brandt</name>",
        "<role>Harbour master</role>",
        "<director>Ines Marlow</director>",
        "<credits>Pavel Orr</credits>",
        "<credits>Sunniva Dahl</credits>",
        "https://img.example.test/harbour/e101.jpg",
    ] {
        assert!(nfo.contains(expected), "{expected} missing from\n{nfo}");
    }
    assert!(Path::new(&season).exists());
}

#[tokio::test]
async fn scraping_the_same_list_again_changes_nothing() {
    let catalogue = Catalogue::new();
    let library = Library::new(&catalogue.fixture);
    let first = library.add("harbour.lights.s01e01.mkv", Some((1, 1)));
    library.add("harbour.lights.s01e02.mkv", Some((1, 2)));
    let scraper = library.scraper();
    let once = scraper
        .scrape_show(library.show, &catalogue, &request(), &all())
        .await
        .unwrap();
    assert_eq!(once.changed, 2);
    let after_once = library.item(first);

    let twice = scraper
        .scrape_show(library.show, &catalogue, &request(), &all())
        .await
        .unwrap();

    let written: Vec<_> = twice
        .episodes
        .iter()
        .map(|e| (e.outcome.written.clone(), e.outcome.ids_added))
        .collect();
    assert_eq!(twice.changed, 0, "{written:?}");
    // Nothing but the ratings' fetch date moved on.
    let undated = |mut item: Item| {
        let ratings: Vec<cv_domain::Rating> = item
            .core
            .ratings
            .iter()
            .map(|r| r.clone().fetched_at(0))
            .collect();
        item.core.ratings = ratings.into_iter().collect();
        item
    };
    assert_eq!(undated(library.item(first)), undated(after_once));
}

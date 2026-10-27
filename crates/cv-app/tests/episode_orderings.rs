//! Episode orderings. A show is switched between aired, DVD and absolute order; the
//! local episodes are matched in the chosen ordering, the numbers of every ordering are kept, and
//! the season list follows the choice. The anime fixture is a long-running show whose files are
//! named by absolute number (`Show - 137.mkv`).

use std::sync::Arc;

use async_trait::async_trait;
use cv_app::episode_order::{EpisodeOrderError, EpisodeOrderer, seasons_in};
use cv_db::Database;
use cv_db::repo::{children_of, get_item, insert_item, set_item_locked};
use cv_domain::{
    EpisodeFields, EpisodeGroup, EpisodeNumber, EpisodeOrdering, Item, ItemDetail, ItemId,
    ProviderIds,
};
use cv_providers::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use cv_providers::{MetadataProvider, Provider, ProviderError};
use cv_scan::episode::parse_episode;
use serde::Deserialize;

// ---- the fixture ------------------------------------------------------------------------------

#[derive(Deserialize)]
struct Fixture {
    show: String,
    tvdb: String,
    seasons: Vec<SeasonSize>,
    specials: u32,
    #[serde(rename = "dvdBlock")]
    dvd_block: u32,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct SeasonSize {
    season: u32,
    episodes: u32,
}

#[derive(Deserialize)]
struct Case {
    file: String,
    absolute: u32,
    aired: (u32, u32),
    dvd: (u32, u32),
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "fixtures/episode_orderings/anime_absolute.json"
    ))
    .expect("the fixture is valid")
}

/// TheTVDB's shape: the aired list has every episode; the DVD and absolute lists have the regular
/// ones, each also carrying its aired number. TheTVDB numbers the absolute list as season 1.
struct Catalogue {
    tvdb: String,
    /// `(aired season, aired episode, absolute number)` of the regular episodes.
    regular: Vec<(u32, u32, u32)>,
    specials: u32,
    dvd_block: u32,
}

impl Catalogue {
    fn of(fixture: &Fixture) -> Self {
        let mut regular = Vec::new();
        let mut absolute = 0;
        for season in &fixture.seasons {
            for episode in 1..=season.episodes {
                absolute += 1;
                regular.push((season.season, episode, absolute));
            }
        }
        Self {
            tvdb: fixture.tvdb.clone(),
            regular,
            specials: fixture.specials,
            dvd_block: fixture.dvd_block,
        }
    }

    fn episode(id: String, numbers: &[EpisodeNumber]) -> Item {
        let mut fields = EpisodeFields::default();
        for n in numbers {
            fields.numbers.set(n.clone());
        }
        let mut item = Item::new(ItemDetail::Episode(fields));
        item.core.title = format!("Episode {id}");
        item.core.ids.set("tvdb", id);
        item
    }

    fn list(&self, group: &EpisodeGroup) -> Option<Vec<Item>> {
        let mut items = Vec::new();
        match group.ordering {
            EpisodeOrdering::Aired => {
                for n in 1..=self.specials {
                    items.push(Self::episode(
                        format!("sp{n}"),
                        &[EpisodeNumber::aired(0, n)],
                    ));
                }
                for &(s, e, a) in &self.regular {
                    items.push(Self::episode(
                        format!("e{a}"),
                        &[EpisodeNumber::aired(s, e)],
                    ));
                }
            }
            EpisodeOrdering::Dvd => {
                for &(s, e, a) in &self.regular {
                    let dvd = ((a - 1) / self.dvd_block + 1, (a - 1) % self.dvd_block + 1);
                    items.push(Self::episode(
                        format!("e{a}"),
                        &[
                            EpisodeNumber::aired(s, e),
                            EpisodeNumber::new(EpisodeGroup::dvd(), dvd.0, dvd.1),
                        ],
                    ));
                }
            }
            EpisodeOrdering::Absolute => {
                for &(s, e, a) in &self.regular {
                    items.push(Self::episode(
                        format!("e{a}"),
                        &[
                            EpisodeNumber::aired(s, e),
                            EpisodeNumber::new(EpisodeGroup::absolute(), 1, a),
                        ],
                    ));
                }
            }
            _ => return None,
        }
        Some(items)
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
            orderings: vec![
                EpisodeOrdering::Aired,
                EpisodeOrdering::Dvd,
                EpisodeOrdering::Absolute,
            ],
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
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        Err(ProviderError::NotFound("Fixture TVDB".to_owned()))
    }

    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        _options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        if show.get("tvdb") != Some(self.tvdb.as_str()) {
            return Err(ProviderError::NotFound("Fixture TVDB".to_owned()));
        }
        self.list(ordering)
            .ok_or_else(|| ProviderError::NotFound("Fixture TVDB".to_owned()))
    }
}

// ---- the library ------------------------------------------------------------------------------

struct Library {
    _dir: tempfile::TempDir,
    db: Arc<Database>,
    show: ItemId,
    show_name: String,
}

impl Library {
    fn new(fixture: &Fixture) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
        let mut show = Item::show(&fixture.show);
        show.core.ids.set("tvdb", &fixture.tvdb);
        let show = db.write(move |tx| insert_item(tx, &show)).unwrap();
        Self {
            _dir: dir,
            db,
            show,
            show_name: fixture.show.clone(),
        }
    }

    /// Adds an episode the way a scan does: the file name read by the regular parser gives the
    /// aired number.
    fn add_file(&self, file: &str) -> ItemId {
        let parsed = parse_episode::<&str>(file, &self.show_name, &[]);
        let mut episode = Item::episode(self.show, file);
        episode.core.original_filename = file.to_owned();
        if let ItemDetail::Episode(fields) = &mut episode.detail {
            fields.numbers.set(EpisodeNumber::aired(
                parsed.season.unwrap_or(1),
                parsed.episodes.first().copied().unwrap_or(0),
            ));
        }
        self.db.write(move |tx| insert_item(tx, &episode)).unwrap()
    }

    fn item(&self, id: ItemId) -> Item {
        self.db.read(|c| get_item(c, id)).unwrap().unwrap()
    }

    fn numbers(&self, id: ItemId) -> cv_domain::EpisodeNumbers {
        match self.item(id).detail {
            ItemDetail::Episode(fields) => fields.numbers,
            other => panic!("not an episode: {other:?}"),
        }
    }

    fn number(&self, id: ItemId, group: &EpisodeGroup) -> Option<(u32, u32)> {
        self.numbers(id).get(group).map(|n| (n.season, n.episode))
    }

    fn show_fields(&self) -> cv_domain::ShowFields {
        match self.item(self.show).detail {
            ItemDetail::Show(fields) => fields,
            other => panic!("not a show: {other:?}"),
        }
    }

    fn seasons(&self) -> Vec<u32> {
        let fields = self.show_fields();
        let episodes: Vec<EpisodeFields> = self
            .db
            .read(|c| children_of(c, self.show))
            .unwrap()
            .into_iter()
            .filter_map(|id| match self.item(id).detail {
                ItemDetail::Episode(fields) => Some(fields),
                _ => None,
            })
            .collect();
        seasons_in(&fields, &episodes)
    }

    fn orderer(&self) -> EpisodeOrderer {
        EpisodeOrderer::new(Arc::clone(&self.db))
    }
}

fn request() -> RequestOptions {
    RequestOptions::default()
}

// ---- tests ------------------------------------------------------------------------------------

/// Spec scenario "Anime absolute order": files named `Show - 137.mkv` in an absolute-order show
/// are matched to the right season and episode of the aired list.
#[tokio::test]
async fn anime_absolute_files_are_matched_to_the_aired_list() {
    let fixture = fixture();
    let catalogue = Catalogue::of(&fixture);
    let library = Library::new(&fixture);
    let ids: Vec<(ItemId, &Case)> = fixture
        .cases
        .iter()
        .map(|case| (library.add_file(&case.file), case))
        .collect();
    let ova = library.add_file("Skyward Blades - OVA 1.mkv");

    // Before the switch the scan's reading of the names is wrong: 137 reads as season 1,
    // episode 37.
    let (id_137, _) = ids.iter().find(|(_, c)| c.absolute == 137).unwrap();
    assert_eq!(
        library.number(*id_137, &EpisodeGroup::aired()),
        Some((1, 37))
    );

    let report = library
        .orderer()
        .apply(
            library.show,
            &EpisodeGroup::absolute(),
            &catalogue,
            &request(),
        )
        .await
        .unwrap();

    assert_eq!(report.group, EpisodeGroup::absolute());
    assert_eq!(report.matched, fixture.cases.len());
    assert_eq!(report.unmatched, vec![ova], "an OVA has no absolute number");
    let regular_total = catalogue.regular.len();
    assert_eq!(
        report.missing,
        regular_total - fixture.cases.len(),
        "the rest of the absolute list is missing"
    );
    for (id, case) in &ids {
        assert_eq!(
            library.number(*id, &EpisodeGroup::aired()),
            Some(case.aired),
            "{} is aired S{:02}E{:02}",
            case.file,
            case.aired.0,
            case.aired.1
        );
        assert_eq!(
            library.number(*id, &EpisodeGroup::absolute()),
            Some((1, case.absolute))
        );
    }

    // The choice is stored on the show, and the show now lists the absolute order.
    let fields = library.show_fields();
    assert_eq!(fields.episode_group, EpisodeGroup::absolute());
    assert_eq!(fields.missing_episodes.len(), report.missing);
    assert_eq!(library.seasons(), vec![0, 1]);
}

/// Spec scenario "Switch to DVD order": the season list follows DVD order and the numbers of the
/// other orderings are kept.
#[tokio::test]
async fn switching_orderings_keeps_every_number_and_the_season_list_follows() {
    let fixture = fixture();
    let catalogue = Catalogue::of(&fixture);
    let library = Library::new(&fixture);
    let ids: Vec<(ItemId, &Case)> = fixture
        .cases
        .iter()
        .map(|case| (library.add_file(&case.file), case))
        .collect();
    let orderer = library.orderer();

    orderer
        .apply(
            library.show,
            &EpisodeGroup::absolute(),
            &catalogue,
            &request(),
        )
        .await
        .unwrap();
    assert_eq!(library.seasons(), vec![1]);

    // Aired order: matched by the IDs the first scrape stored, not by the file names.
    let aired = orderer
        .apply(library.show, &EpisodeGroup::aired(), &catalogue, &request())
        .await
        .unwrap();
    assert_eq!(aired.matched, fixture.cases.len());
    assert!(aired.unmatched.is_empty());
    assert_eq!(library.show_fields().episode_group, EpisodeGroup::aired());
    // Every aired season, and the specials, have episodes: local or still missing.
    assert_eq!(library.seasons(), (0..=6).collect::<Vec<_>>());

    let dvd = orderer
        .apply(library.show, &EpisodeGroup::dvd(), &catalogue, &request())
        .await
        .unwrap();
    assert_eq!(dvd.matched, fixture.cases.len());
    // 152 episodes in blocks of 13 make twelve DVD seasons; the specials are not on the discs.
    assert_eq!(library.seasons(), (1..=12).collect::<Vec<_>>());

    for (id, case) in &ids {
        assert_eq!(library.number(*id, &EpisodeGroup::dvd()), Some(case.dvd));
        assert_eq!(
            library.number(*id, &EpisodeGroup::aired()),
            Some(case.aired),
            "the aired number is kept"
        );
        assert_eq!(
            library.number(*id, &EpisodeGroup::absolute()),
            Some((1, case.absolute)),
            "the absolute number is kept"
        );
        assert_eq!(library.numbers(*id).len(), 3);
    }

    // Going round again changes nothing.
    let again = orderer
        .apply(
            library.show,
            &EpisodeGroup::absolute(),
            &catalogue,
            &request(),
        )
        .await
        .unwrap();
    assert_eq!(again.matched, fixture.cases.len());
    for (id, case) in &ids {
        assert_eq!(library.number(*id, &EpisodeGroup::dvd()), Some(case.dvd));
    }
}

/// Spec scenario "Switch to DVD order": local files are matched by DVD season and episode.
#[tokio::test]
async fn dvd_order_matches_files_by_dvd_numbers() {
    let fixture = fixture();
    let catalogue = Catalogue::of(&fixture);
    let library = Library::new(&fixture);
    // DVD season 2, episode 3 is absolute episode 16, which aired as season 1, episode 16.
    let file = library.add_file("Skyward Blades S02E03.mkv");
    // DVD season 3, episode 1 is absolute episode 27: aired season 2, episode 1.
    let next = library.add_file("Skyward Blades S03E01.mkv");
    // No such DVD episode.
    let stray = library.add_file("Skyward Blades S40E01.mkv");

    let report = library
        .orderer()
        .apply(library.show, &EpisodeGroup::dvd(), &catalogue, &request())
        .await
        .unwrap();

    assert_eq!(report.matched, 2);
    assert_eq!(report.unmatched, vec![stray]);
    assert_eq!(library.number(file, &EpisodeGroup::dvd()), Some((2, 3)));
    assert_eq!(library.number(file, &EpisodeGroup::aired()), Some((1, 16)));
    assert_eq!(library.number(next, &EpisodeGroup::dvd()), Some((3, 1)));
    assert_eq!(library.number(next, &EpisodeGroup::aired()), Some((2, 1)));
    let mut want: Vec<u32> = (1..=12).collect();
    want.push(40);
    assert_eq!(library.seasons(), want);
}

#[tokio::test]
async fn an_ordering_the_show_lacks_is_reported_and_changes_nothing() {
    let fixture = fixture();
    let catalogue = Catalogue::of(&fixture);
    let library = Library::new(&fixture);
    let file = library.add_file(&fixture.cases[0].file);
    let before = library.numbers(file);

    let alternate = EpisodeGroup::new(EpisodeOrdering::Alternate, "Production order");
    let error = library
        .orderer()
        .apply(library.show, &alternate, &catalogue, &request())
        .await
        .unwrap_err();

    assert!(matches!(&error, EpisodeOrderError::NotAvailable(g) if *g == alternate));
    assert_eq!(error.code(), "ordering_not_available");
    assert_eq!(
        error.to_string(),
        "Production order is not available for this show."
    );
    assert_eq!(library.show_fields().episode_group, EpisodeGroup::aired());
    assert_eq!(library.numbers(file), before);
}

#[tokio::test]
async fn a_locked_episode_keeps_its_numbers() {
    let fixture = fixture();
    let catalogue = Catalogue::of(&fixture);
    let library = Library::new(&fixture);
    let locked = library.add_file(&fixture.cases[8].file);
    let free = library.add_file(&fixture.cases[2].file);
    library
        .db
        .write(move |tx| set_item_locked(tx, &[locked], true))
        .unwrap();
    let before = library.numbers(locked);

    let report = library
        .orderer()
        .apply(
            library.show,
            &EpisodeGroup::absolute(),
            &catalogue,
            &request(),
        )
        .await
        .unwrap();

    assert_eq!(report.matched, 2);
    assert_eq!(report.locked, 1);
    assert_eq!(library.numbers(locked), before);
    assert_eq!(library.number(free, &EpisodeGroup::aired()), Some((2, 1)));
}

#[tokio::test]
async fn only_shows_have_an_ordering() {
    let fixture = fixture();
    let catalogue = Catalogue::of(&fixture);
    let library = Library::new(&fixture);
    let file = library.add_file(&fixture.cases[0].file);

    let error = library
        .orderer()
        .apply(file, &EpisodeGroup::dvd(), &catalogue, &request())
        .await
        .unwrap_err();
    assert!(matches!(error, EpisodeOrderError::NotAShow));

    let error = library
        .orderer()
        .apply(ItemId(9999), &EpisodeGroup::dvd(), &catalogue, &request())
        .await
        .unwrap_err();
    assert!(matches!(error, EpisodeOrderError::ShowNotFound));
}

#[tokio::test]
async fn the_picker_lists_what_the_provider_offers() {
    let fixture = fixture();
    let library = Library::new(&fixture);
    let (current, options) = library
        .orderer()
        .options(
            library.show,
            &[
                EpisodeOrdering::Aired,
                EpisodeOrdering::Dvd,
                EpisodeOrdering::Absolute,
            ],
        )
        .unwrap();
    assert_eq!(current, EpisodeGroup::aired());
    assert_eq!(
        options,
        vec![
            EpisodeGroup::aired(),
            EpisodeGroup::dvd(),
            EpisodeGroup::absolute()
        ]
    );
}

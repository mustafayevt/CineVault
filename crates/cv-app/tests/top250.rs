//! The IMDb Top 250 is fetched on request and stored on the matching library items.
//! The chart source is an in-memory fake, so nothing here touches the network.

use async_trait::async_trait;
use cv_app::top250::{Top250Error, refresh_top250};
use cv_db::Database;
use cv_db::repo::{
    Direction, Filter, IntRange, ItemQuery, SortField, SortKey, get_item, insert_item,
    list_items_at, set_item_locked,
};
use cv_domain::{Item, ItemDetail, ItemId, ItemKind, id_key};
use cv_providers::model::RequestOptions;
use cv_providers::{ProviderError, Top250Entry, Top250List, Top250Source};

struct Chart(Result<Vec<Top250Entry>, ()>);

#[async_trait]
impl Top250Source for Chart {
    async fn top250(
        &self,
        _list: Top250List,
        _options: &RequestOptions,
    ) -> Result<Vec<Top250Entry>, ProviderError> {
        self.0
            .clone()
            .map_err(|()| ProviderError::FormatChanged("IMDb".into()))
    }
}

fn chart(ids: &[&str]) -> Chart {
    Chart(Ok(ids
        .iter()
        .enumerate()
        .map(|(i, id)| Top250Entry {
            rank: u32::try_from(i + 1).unwrap(),
            imdb_id: (*id).to_owned(),
            title: format!("Chart {}", i + 1),
        })
        .collect()))
}

fn movie(db: &Database, title: &str, imdb: &str, rank: u32) -> ItemId {
    let mut item = Item::movie(title);
    item.core.ids.set(id_key::IMDB, imdb);
    if let ItemDetail::Movie(m) = &mut item.detail {
        m.top250 = rank;
    }
    db.write(move |tx| insert_item(tx, &item)).unwrap()
}

fn rank(db: &Database, id: ItemId) -> u32 {
    match db.read(|c| get_item(c, id)).unwrap().unwrap().detail {
        ItemDetail::Movie(m) => m.top250,
        _ => panic!("not a movie"),
    }
}

fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    (dir, db)
}

#[tokio::test]
async fn the_chart_positions_are_stored_and_the_top_50_filter_finds_them() {
    let (_dir, db) = open();
    let heat = movie(&db, "Heat", "tt0000001", 0);
    let drive = movie(&db, "Drive", "tt0000002", 0);
    let unranked = movie(&db, "Obscure", "tt0000099", 0);
    let fell_off = movie(&db, "Fell off", "tt0000050", 12);

    let report = refresh_top250(
        &db,
        &chart(&["tt0000001", "tt0000002", "tt0000777"]),
        Top250List::Movies,
        &RequestOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(rank(&db, heat), 1);
    assert_eq!(rank(&db, drive), 2);
    assert_eq!(rank(&db, unranked), 0);
    assert_eq!(rank(&db, fell_off), 0);
    assert_eq!(
        (report.ranked, report.cleared, report.not_in_library),
        (2, 1, 1)
    );

    // "Top 250 rank <= 50" shows only the ranked movies, best first.
    let query = ItemQuery::new(ItemKind::Movie)
        .filter(Filter::Top250 {
            range: IntRange::at_most(50),
        })
        .sort_by(SortKey {
            field: SortField::Top250,
            direction: Direction::Asc,
        });
    let titles: Vec<String> = db
        .read(|c| list_items_at(c, &query, 0, 10))
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect();
    assert_eq!(titles, ["Heat", "Drive"]);
}

#[tokio::test]
async fn a_chart_that_cannot_be_read_changes_nothing() {
    let (_dir, db) = open();
    let kept = movie(&db, "Kept", "tt0000001", 7);

    let error = refresh_top250(
        &db,
        &Chart(Err(())),
        Top250List::Movies,
        &RequestOptions::default(),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, Top250Error::Source(ProviderError::FormatChanged(_))),
        "{error:?}"
    );
    assert_eq!(rank(&db, kept), 7);
}

#[tokio::test]
async fn a_locked_movie_keeps_its_position() {
    let (_dir, db) = open();
    let locked = movie(&db, "Locked", "tt0000001", 99);
    db.write(move |tx| set_item_locked(tx, &[locked], true))
        .unwrap();

    let report = refresh_top250(
        &db,
        &chart(&["tt0000001"]),
        Top250List::Movies,
        &RequestOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(rank(&db, locked), 99);
    assert_eq!(report.locked, 1);
}

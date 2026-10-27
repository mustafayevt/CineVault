//! A universal scrape is written through the normal scrape write path, so field locks,
//! the entity lock and *do not overwrite* hold for it as they do for a single provider.

#[path = "../../cv-providers/tests/support/fake.rs"]
mod fake;

use std::collections::BTreeMap;
use std::sync::Arc;

use cv_app::scrape_settings::ScrapeSettings;
use cv_db::Database;
use cv_db::repo::{apply_scrape, get_item, insert_item, set_field_lock, set_item_locked};
use cv_domain::{Item, ItemKind, MetaField, ScrapeBlock};
use cv_providers::model::{MediaKind, RequestOptions};
use cv_providers::{MetadataProvider, UniversalConfig, UniversalProvider};
use fake::{Fake, ids};
use serde_json::{Value, json};

/// TMDB supplies the title, IMDb the plot.
async fn mixed_scrape() -> Item {
    let mut from_tmdb = Item::movie("The Matrix");
    from_tmdb.core.year = 1999;
    from_tmdb.core.plot = "TMDB plot".into();
    from_tmdb.core.ids = ids(&[("tmdb", "603"), ("imdb", "tt0133093")]);
    let mut from_imdb = Item::movie("Matrix, The");
    from_imdb.core.plot = "IMDb plot".into();
    from_imdb.core.ids = ids(&[("imdb", "tt0133093")]);

    let tmdb = Fake::new("tmdb", &["tmdb", "imdb"], vec![from_tmdb]).shared();
    let imdb = Fake::new("imdb", &["imdb"], vec![from_imdb]).shared();
    let universal = UniversalProvider::new(
        UniversalConfig {
            search: "tmdb".into(),
            fields: BTreeMap::from([
                (MetaField::Title, "tmdb".to_owned()),
                (MetaField::Plot, "imdb".to_owned()),
            ]),
            ..UniversalConfig::default()
        },
        [
            tmdb as Arc<dyn MetadataProvider>,
            imdb as Arc<dyn MetadataProvider>,
        ],
    );
    universal
        .details(
            MediaKind::Movie,
            &ids(&[("tmdb", "603")]),
            &RequestOptions::default(),
        )
        .await
        .unwrap()
}

fn library() -> (tempfile::TempDir, Database, cv_domain::ItemId) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    let mut item = Item::movie("Local title");
    item.core.plot = "My own plot".into();
    let id = db.write(move |tx| insert_item(tx, &item)).unwrap();
    (dir, db, id)
}

fn settings(pairs: &[(&str, Value)]) -> ScrapeSettings {
    ScrapeSettings::from_values(
        &pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    )
}

#[tokio::test]
async fn a_locked_field_keeps_its_value_and_the_other_fields_update() {
    let scraped = mixed_scrape().await;
    let (_dir, db, id) = library();
    db.write(move |tx| set_field_lock(tx, id, MetaField::Plot, true).map(|_| ()))
        .unwrap();

    let options = ScrapeSettings::default().options_for(ItemKind::Movie);
    let outcome = db
        .write(move |tx| apply_scrape(tx, id, &scraped, &options))
        .unwrap();
    assert!(outcome.written.contains(&MetaField::Title));
    assert_eq!(outcome.locked, [MetaField::Plot]);

    let stored = db.read(|c| get_item(c, id)).unwrap().unwrap();
    assert_eq!(stored.core.title, "The Matrix");
    assert_eq!(stored.core.plot, "My own plot");
    assert_eq!(stored.core.ids.get("tmdb"), Some("603"));
}

#[tokio::test]
async fn do_not_overwrite_fills_only_empty_fields() {
    let scraped = mixed_scrape().await;
    let (_dir, db, id) = library();
    let options =
        settings(&[("metadata.scrape-do-not-overwrite", json!(true))]).options_for(ItemKind::Movie);
    let outcome = db
        .write(move |tx| apply_scrape(tx, id, &scraped, &options))
        .unwrap();
    assert!(outcome.kept.contains(&MetaField::Title));
    assert!(outcome.kept.contains(&MetaField::Plot));
    assert!(outcome.written.contains(&MetaField::Year), "year was empty");

    let stored = db.read(|c| get_item(c, id)).unwrap().unwrap();
    assert_eq!(stored.core.title, "Local title");
    assert_eq!(stored.core.plot, "My own plot");
    assert_eq!(stored.core.year, 1999);
}

#[tokio::test]
async fn a_locked_item_is_not_modified_and_unselected_fields_are_left_alone() {
    let scraped = mixed_scrape().await;
    let (_dir, db, id) = library();

    let only_plot =
        settings(&[("metadata.scrape-fields-movie", json!("plot"))]).options_for(ItemKind::Movie);
    let scraped_again = scraped.clone();
    db.write(move |tx| apply_scrape(tx, id, &scraped_again, &only_plot))
        .unwrap();
    let stored = db.read(|c| get_item(c, id)).unwrap().unwrap();
    assert_eq!(stored.core.title, "Local title");
    assert_eq!(stored.core.plot, "IMDb plot");

    db.write(move |tx| set_item_locked(tx, &[id], true).map(|_| ()))
        .unwrap();
    let options = ScrapeSettings::default().options_for(ItemKind::Movie);
    let outcome = db
        .write(move |tx| apply_scrape(tx, id, &scraped, &options))
        .unwrap();
    assert_eq!(outcome.blocked, Some(ScrapeBlock::ItemLocked));
    let stored = db.read(|c| get_item(c, id)).unwrap().unwrap();
    assert_eq!(stored.core.title, "Local title");
}

//! The NEW flag, review state, last scraper and rename history are
//! persisted, so they survive a restart (closing and reopening the database file).

use cv_db::Database;
use cv_db::repo::{self, Filter, ItemFlag, ItemFlags, ItemQuery, ItemSummary};
use cv_domain::{Item, ItemId, ItemKind, RenameHistory, RenameStep, ReviewState};

fn list_movies(db: &Database, filter: Filter) -> Vec<ItemSummary> {
    db.read(|c| {
        Ok(repo::list_items(
            c,
            &ItemQuery::new(ItemKind::Movie).filter(filter),
            None,
            100,
        )?
        .items)
    })
    .unwrap()
}

fn new_filter(value: bool) -> Filter {
    Filter::Flag {
        flag: ItemFlag::New,
        value,
    }
}

fn titles(items: &[ItemSummary]) -> Vec<&str> {
    items.iter().map(|i| i.title.as_str()).collect()
}

fn add_movies(db: &Database, titles: &[&str]) -> Vec<ItemId> {
    let items: Vec<Item> = titles.iter().map(|t| Item::movie(*t)).collect();
    db.write(move |tx| {
        items
            .iter()
            .map(|i| repo::insert_item(tx, i))
            .collect::<Result<Vec<_>, _>>()
    })
    .unwrap()
}

#[test]
fn new_items_stay_flagged_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");

    let ids = {
        let db = Database::open(&path).unwrap();
        let ids = add_movies(&db, &["Alpha", "Bravo", "Charlie", "Delta"]);
        // A scan adds the first three and flags them; the fourth was opened before.
        let flagged = ids[..3].to_vec();
        let n = db
            .write(move |tx| repo::set_new(tx, &flagged, true))
            .unwrap();
        assert_eq!(n, 3);
        ids
    }; // the database is dropped here: the app quits

    let db = Database::open(&path).unwrap();
    let new_items = list_movies(&db, new_filter(true));
    assert_eq!(titles(&new_items), ["Alpha", "Bravo", "Charlie"]);
    assert!(new_items.iter().all(|i| i.is_new));
    let rest = list_movies(&db, new_filter(false));
    assert_eq!(titles(&rest), ["Delta"]);
    db.read(|c| {
        assert_eq!(repo::count_new(c, Some(ItemKind::Movie))?, 3);
        assert!(repo::get_flags(c, ids[0])?.is_new);
        assert!(!repo::get_flags(c, ids[3])?.is_new);
        Ok(())
    })
    .unwrap();
}

#[test]
fn clearing_new_persists_and_metadata_edits_keep_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let total = {
        let db = Database::open(&path).unwrap();
        let ids = add_movies(&db, &["One", "Two", "Three", "Four", "Five"]);
        let all = ids.clone();
        db.write(move |tx| repo::set_new(tx, &all, true)).unwrap();

        // Saving edited metadata must not clear the flag.
        let id = ids[0];
        db.write(move |tx| {
            let mut item = repo::get_item(tx, id)?.unwrap();
            item.core.plot = "Edited".into();
            repo::update_item(tx, &item)
        })
        .unwrap();
        db.read(|c| {
            assert!(repo::get_flags(c, id)?.is_new);
            Ok(())
        })
        .unwrap();

        // "Clear NEW" on a selection of two.
        let picked = vec![ids[0], ids[1]];
        let n = db
            .write(move |tx| repo::set_new(tx, &picked, false))
            .unwrap();
        assert_eq!(n, 2);
        ids.len()
    };

    let db = Database::open(&path).unwrap();
    assert_eq!(
        titles(&list_movies(&db, new_filter(true))),
        ["Five", "Four", "Three"]
    );
    let cleared = db.write(|tx| repo::clear_all_new(tx, None)).unwrap();
    assert_eq!(cleared, 3);
    drop(db);

    let db = Database::open(&path).unwrap();
    assert!(list_movies(&db, new_filter(true)).is_empty());
    assert_eq!(list_movies(&db, Filter::All).len(), total);
}

#[test]
fn new_episodes_are_counted_per_show() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    let (show, eps) = db
        .write(|tx| {
            let show = repo::insert_item(tx, &Item::show("Some Show"))?;
            let a = repo::insert_item(tx, &Item::episode(show, "Pilot"))?;
            let b = repo::insert_item(tx, &Item::episode(show, "Second"))?;
            repo::set_new(tx, &[a], true)?;
            Ok((show, vec![a, b]))
        })
        .unwrap();
    db.read(|c| {
        assert_eq!(repo::count_new_children(c, show)?, 1);
        assert_eq!(repo::count_new(c, Some(ItemKind::Episode))?, 1);
        Ok(())
    })
    .unwrap();
    let n = db
        .write(move |tx| repo::clear_new_children(tx, show))
        .unwrap();
    assert_eq!(n, 1);
    db.read(|c| {
        assert_eq!(repo::count_new_children(c, show)?, 0);
        assert!(!repo::get_flags(c, eps[0])?.is_new);
        Ok(())
    })
    .unwrap();
}

#[test]
fn review_state_last_scraper_and_rename_history_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let history = RenameHistory {
        old_path: "D:\\Movies\\Old Name".into(),
        new_path: "D:\\Movies\\New Name (2020)".into(),
        files: vec![RenameStep {
            old_filename: "old.mkv".into(),
            new_filename: "New Name (2020).mkv".into(),
        }],
    };

    let (id, plain) = {
        let db = Database::open(&path).unwrap();
        let mut item = Item::movie("Scraped");
        item.core.last_scraper_id = "tmdb".into();
        item.core.last_scrape_language = "de-DE".into();
        item.core.rename_history = Some(history.clone());
        let ids = db
            .write(move |tx| {
                let a = repo::insert_item(tx, &item)?;
                let b = repo::insert_item(tx, &Item::movie("Plain"))?;
                repo::set_review_state(tx, a, ReviewState::NeedsReview)?;
                repo::set_new(tx, &[a], true)?;
                Ok((a, b))
            })
            .unwrap();

        // A later edit through update_item keeps the flags and the history.
        let id = ids.0;
        db.write(move |tx| {
            let mut item = repo::get_item(tx, id)?.unwrap();
            item.core.note = "kept".into();
            repo::update_item(tx, &item)
        })
        .unwrap();
        ids
    };

    let db = Database::open(&path).unwrap();
    db.read(|c| {
        let item = repo::get_item(c, id)?.unwrap();
        assert_eq!(item.core.last_scraper_id, "tmdb");
        assert_eq!(item.core.last_scrape_language, "de-DE");
        assert_eq!(item.core.rename_history, Some(history.clone()));
        assert_eq!(
            repo::get_flags(c, id)?,
            ItemFlags {
                is_new: true,
                review_state: ReviewState::NeedsReview
            }
        );
        assert_eq!(repo::get_flags(c, plain)?, ItemFlags::default());
        Ok(())
    })
    .unwrap();

    let review = Filter::ReviewState {
        values: vec![ReviewState::NeedsReview.to_string()],
    };
    assert_eq!(titles(&list_movies(&db, review)), ["Scraped"]);

    // Resolving is persisted too.
    db.write(move |tx| repo::set_review_state(tx, id, ReviewState::Resolved))
        .unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    db.read(|c| {
        assert_eq!(repo::get_flags(c, id)?.review_state, ReviewState::Resolved);
        Ok(())
    })
    .unwrap();
}

#[test]
fn unknown_ids_are_reported_or_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    let ghost = ItemId(999);
    db.read(|c| {
        assert!(repo::get_flags(c, ghost).is_err());
        Ok(())
    })
    .unwrap();
    assert!(
        db.write(move |tx| repo::set_review_state(tx, ghost, ReviewState::Resolved))
            .is_err()
    );
    let n = db
        .write(move |tx| repo::set_new(tx, &[ghost], true))
        .unwrap();
    assert_eq!(n, 0);
    let n = db
        .write(move |tx| repo::set_review_states(tx, &[ghost], ReviewState::None))
        .unwrap();
    assert_eq!(n, 0);
}

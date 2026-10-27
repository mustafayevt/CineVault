//! A person's IMDb ID, read from the provider later, is stored with the person and survives a
//! rescrape that brings the person back with their TMDB ID alone.

use cv_db::Database;
use cv_db::repo;
use cv_domain::{Credit, CreditKind, Item, Person};

fn movie_with(person: Person) -> Item {
    let mut item = Item::movie("Alien");
    item.core.credits.push(Credit {
        kind: CreditKind::Actor,
        role: "Ripley".to_owned(),
        person,
    });
    item
}

fn tmdb_only(name: &str, id: &str) -> Person {
    let mut person = Person::new(name);
    person.ids.set("tmdb", id);
    person
}

fn people(db: &Database) -> i64 {
    db.read(|c| Ok(c.query_row("SELECT COUNT(*) FROM person", [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn an_imdb_id_added_later_is_kept_and_a_rescrape_does_not_duplicate_the_person() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    let item = movie_with(tmdb_only("Sigourney Weaver", "10205"));
    let id = db.write(move |tx| repo::insert_item(tx, &item)).unwrap();

    let changed = db
        .write(|tx| repo::add_person_id(tx, "10205", "imdb", "nm0000244"))
        .unwrap();
    assert_eq!(changed, 1);
    // The same value again changes nothing.
    let again = db
        .write(|tx| repo::add_person_id(tx, "10205", "imdb", "nm0000244"))
        .unwrap();
    assert_eq!(again, 0);

    let stored = db.read(|c| repo::get_item(c, id)).unwrap().unwrap();
    let ids = &stored.core.credits[0].person.ids;
    assert_eq!(ids.get("imdb"), Some("nm0000244"));
    assert_eq!(ids.get("tmdb"), Some("10205"));

    // A rescrape brings the person back with the TMDB ID alone: still one row, IMDb ID kept.
    let mut rescraped = stored.clone();
    rescraped.core.credits = movie_with(tmdb_only("Sigourney Weaver", "10205"))
        .core
        .credits;
    db.write(move |tx| repo::update_item(tx, &rescraped))
        .unwrap();
    assert_eq!(people(&db), 1);
    let after = db.read(|c| repo::get_item(c, id)).unwrap().unwrap();
    assert_eq!(
        after.core.credits[0].person.ids.get("imdb"),
        Some("nm0000244")
    );
}

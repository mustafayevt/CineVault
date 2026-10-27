//! The stage filter: New, Scraped and Renamed as a library filter
//! that agrees with the lifecycle's own rule.

use cv_domain::{Item, ItemId, ItemKind, id_key};
use rusqlite::Connection;

use super::organize_state::{OrganizeStateEntry, OrganizeStateKind, set_organize_states};
use super::{Filter, ItemQuery, SortField, SortKey, insert_item, list_items};

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn add(conn: &Connection, title: &str, scraped: bool) -> ItemId {
    let mut item = Item::movie(title);
    if scraped {
        item.core.ids.set(id_key::TMDB, "1");
        item.core.last_scraper_id = "tmdb".into();
    }
    insert_item(conn, &item).unwrap()
}

fn tidy(conn: &Connection, id: ItemId, state: OrganizeStateKind, key: &str) {
    let entry = OrganizeStateEntry {
        item: id,
        state,
        reason: String::new(),
    };
    set_organize_states(conn, &[entry], key, 1).unwrap();
}

fn stage(stages: &[&str], key: &str) -> ItemQuery {
    ItemQuery::new(ItemKind::Movie)
        .filter(Filter::Stage {
            stages: stages.iter().map(|s| (*s).to_owned()).collect(),
            profile_key: key.to_owned(),
        })
        .sort_by(SortKey::asc(SortField::Title))
}

fn titles(conn: &Connection, query: &ItemQuery) -> Vec<String> {
    list_items(conn, query, None, 50)
        .unwrap()
        .items
        .into_iter()
        .map(|i| i.title)
        .collect()
}

/// Two titles nobody scraped, one scraped with names to change, two scraped and tidy (one of them
/// computed for another naming profile).
fn library() -> Connection {
    let conn = memory_db();
    add(&conn, "Alien", false);
    add(&conn, "Brazil", false);
    let heat = add(&conn, "Heat", true);
    let ronin = add(&conn, "Ronin", true);
    let zodiac = add(&conn, "Zodiac", true);
    tidy(&conn, heat, OrganizeStateKind::Pending, "p1");
    tidy(&conn, ronin, OrganizeStateKind::Tidy, "p1");
    tidy(&conn, zodiac, OrganizeStateKind::Tidy, "old-profile");
    conn
}

#[test]
fn each_stage_lists_exactly_its_titles() {
    let conn = library();
    assert_eq!(titles(&conn, &stage(&["new"], "p1")), ["Alien", "Brazil"]);
    assert_eq!(
        titles(&conn, &stage(&["scraped"], "p1")),
        ["Heat", "Zodiac"]
    );
    assert_eq!(titles(&conn, &stage(&["renamed"], "p1")), ["Ronin"]);
}

#[test]
fn several_stages_match_any_of_them_and_none_matches_nothing() {
    let conn = library();
    assert_eq!(
        titles(&conn, &stage(&["New", "RENAMED"], "p1")),
        ["Alien", "Brazil", "Ronin"]
    );
    assert!(titles(&conn, &stage(&[], "p1")).is_empty());
}

#[test]
fn a_tidy_state_of_another_profile_is_not_renamed() {
    let conn = library();
    // Under the profile Zodiac was computed for, it is the one that is tidy.
    assert_eq!(
        titles(&conn, &stage(&["renamed"], "old-profile")),
        ["Zodiac"]
    );
}

#[test]
fn the_stage_counts_agree_with_the_pipeline_groups() {
    use super::lifecycle::lifecycle_groups;
    let conn = library();
    let groups = lifecycle_groups(&conn, ItemKind::Movie, "p1").unwrap();
    let new: u32 = groups.iter().filter(|g| !g.scraped).map(|g| g.count).sum();
    let renamed: u32 = groups
        .iter()
        .filter(|g| g.scraped && g.organize == Some(OrganizeStateKind::Tidy))
        .map(|g| g.count)
        .sum();
    assert_eq!(new as usize, titles(&conn, &stage(&["new"], "p1")).len());
    assert_eq!(
        renamed as usize,
        titles(&conn, &stage(&["renamed"], "p1")).len()
    );
}

#[test]
fn an_unknown_profile_means_the_newest_computation() {
    let conn = library();
    let renamed = ItemQuery::new(ItemKind::Movie)
        .filter(Filter::Stage {
            stages: vec!["renamed".into()],
            profile_key: String::new(),
        })
        .sort_by(SortKey::asc(SortField::Title));
    // Ronin and Zodiac were computed at the same instant for different profiles: say Zodiac's
    // profile was computed last.
    let zodiac = list_items(&conn, &stage(&["scraped"], "p1"), None, 50)
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.title == "Zodiac")
        .unwrap();
    let entry = OrganizeStateEntry {
        item: zodiac.id,
        state: OrganizeStateKind::Tidy,
        reason: String::new(),
    };
    set_organize_states(&conn, &[entry], "old-profile", 9).unwrap();
    assert_eq!(titles(&conn, &renamed), ["Zodiac"]);
}

#[test]
fn sorting_by_stage_puts_new_first_then_scraped_then_renamed() {
    let conn = library();
    let ascending = ItemQuery::new(ItemKind::Movie)
        .profile_key("p1")
        .sort_by(SortKey::asc(SortField::Stage))
        .sort_by(SortKey::asc(SortField::Title));
    assert_eq!(
        titles(&conn, &ascending),
        ["Alien", "Brazil", "Heat", "Zodiac", "Ronin"]
    );
    let descending = ItemQuery::new(ItemKind::Movie)
        .profile_key("p1")
        .sort_by(SortKey::desc(SortField::Stage))
        .sort_by(SortKey::asc(SortField::Title));
    assert_eq!(
        titles(&conn, &descending),
        ["Ronin", "Heat", "Zodiac", "Alien", "Brazil"]
    );
    // A cursor through the stage sort agrees with the full order.
    let mut seen = Vec::new();
    let mut after = None;
    loop {
        let page = list_items(&conn, &ascending, after.as_ref(), 2).unwrap();
        seen.extend(page.items.into_iter().map(|i| i.title));
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(seen, titles(&conn, &ascending));
}

//! Unit tests of the title search.

use cv_domain::{EpisodeFields, EpisodeGroup, EpisodeNumber, Item, ItemDetail};

use super::title_search::{EpisodePick, MAX_HITS, find_pick, search_titles};

fn episode(id: &str, season: u32, number: u32, title: &str) -> Item {
    let mut fields = EpisodeFields::default();
    fields.numbers.set(EpisodeNumber::aired(season, number));
    let mut item = Item::new(ItemDetail::Episode(fields));
    item.core.title = title.to_owned();
    item.core.ids.set("tvdb", id);
    item
}

fn list() -> Vec<Item> {
    vec![
        episode("a", 1, 1, "The Night Shift"),
        episode("b", 1, 2, "Salt and Rope"),
        episode("c", 1, 3, "The Quiet Tide"),
        episode("d", 2, 1, "Tide Tables"),
        episode("e", 2, 2, "Fog Signals (Part 1)"),
    ]
}

fn titles(query: &str) -> Vec<String> {
    search_titles(&list(), &EpisodeGroup::aired(), query)
        .into_iter()
        .map(|h| h.title)
        .collect()
}

#[test]
fn the_whole_title_ranks_first_whatever_its_case_and_punctuation() {
    assert_eq!(titles("the quiet tide")[0], "The Quiet Tide");
    assert_eq!(titles("  THE   QUIET-TIDE! ")[0], "The Quiet Tide");
    assert_eq!(titles("fog signals part 1")[0], "Fog Signals (Part 1)");
}

#[test]
fn part_of_a_title_finds_it() {
    assert_eq!(titles("night"), ["The Night Shift"]);
    assert_eq!(titles("salt")[0], "Salt and Rope");
    // A half-typed word still fits.
    assert_eq!(titles("quiet ti")[0], "The Quiet Tide");
}

#[test]
fn equal_scores_are_in_number_order() {
    let hits = titles("tide");
    assert_eq!(hits, ["The Quiet Tide", "Tide Tables"]);
}

#[test]
fn a_typo_still_finds_the_title() {
    assert_eq!(titles("The Quite Tide")[0], "The Quiet Tide");
}

#[test]
fn nothing_or_unrelated_text_finds_nothing() {
    assert!(titles("").is_empty());
    assert!(titles("   ...  ").is_empty());
    assert!(titles("zeppelin").is_empty());
}

#[test]
fn the_original_title_is_searched_too() {
    let mut list = list();
    list[1].core.original_title = "Sel et corde".to_owned();
    let hits = search_titles(&list, &EpisodeGroup::aired(), "sel et corde");
    assert_eq!(hits[0].title, "Salt and Rope");
}

#[test]
fn a_hit_carries_what_tells_it_apart() {
    let mut list = list();
    list[2].core.plot = "x".repeat(1000);
    let hit = &search_titles(&list, &EpisodeGroup::aired(), "quiet tide")[0];
    assert_eq!(hit.number, Some((1, 3)));
    assert_eq!(hit.plot.chars().count(), 240, "the plot is cut");
    assert_eq!(hit.ids.get("tvdb"), Some("c"));
    assert_eq!(hit.pick().number, Some((1, 3)));
}

#[test]
fn a_search_returns_a_bounded_number_of_hits() {
    let many: Vec<Item> = (1..=40)
        .map(|n| episode(&format!("x{n}"), 1, n, "Same Title"))
        .collect();
    let hits = search_titles(&many, &EpisodeGroup::aired(), "same title");
    assert_eq!(hits.len(), MAX_HITS);
    assert_eq!(hits[0].number, Some((1, 1)));
}

#[test]
fn a_pick_is_found_by_id_then_by_number() {
    let list = list();
    let group = EpisodeGroup::aired();
    let by_id = EpisodePick {
        ids: list[3].core.ids.clone(),
        number: Some((1, 1)),
    };
    assert_eq!(find_pick(&list, &group, &by_id), Some(3), "the ID wins");
    let by_number = EpisodePick {
        ids: Default::default(),
        number: Some((1, 2)),
    };
    assert_eq!(find_pick(&list, &group, &by_number), Some(1));
    assert_eq!(find_pick(&list, &group, &EpisodePick::default()), None);
}

#[test]
fn a_run_can_override_the_settings_selection_and_mode() {
    use cv_domain::{FieldSet, ItemKind, MetaField, ScrapeOptions};

    use super::ScrapeOverride;

    let settings = ScrapeOptions {
        fields: FieldSet::all_for(ItemKind::Episode),
        do_not_overwrite: true,
    };
    // Nothing chosen: the settings stand.
    assert_eq!(ScrapeOverride::default().apply(settings.clone()), settings);

    let chosen = ScrapeOverride {
        fields: Some(FieldSet::from_keys("rating,thumb,tagline,status")),
        do_not_overwrite: Some(false),
    }
    .apply(settings);
    assert_eq!(
        chosen.fields.to_keys(),
        "rating,thumb",
        "fields an episode does not have are dropped"
    );
    assert!(chosen.fields.contains(MetaField::Rating));
    assert!(!chosen.do_not_overwrite);
}

//! The lifecycle rules (spec `title-lifecycle`): one test per scenario, then the same rules over a
//! real library file for the pipeline counts.

use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::organize_state::{OrganizeStateEntry, OrganizeStateKind, set_organize_states};
use cv_db::repo::review::{NewReview, enqueue};
use cv_db::repo::step_outcome::{
    STEP_ARTWORK, STEP_RATINGS, STEP_SCRAPE, StepOutcome, StepStatus, record_step_outcome,
    source_step,
};
use cv_db::repo::{insert_item, set_item_locked};
use cv_domain::{Availability, Item, ItemId, ItemKind, MediaFile, MediaFileType};
use serde_json::json;

use super::*;

fn scraped_movie() -> Facts {
    let mut facts = Facts::new(ItemKind::Movie);
    facts.title = "Heat".to_owned();
    facts.year = 1995;
    facts.added_ms = 1_700_000_000_000;
    facts.has_provider_id = true;
    facts.scraped_by_provider = true;
    facts.provider = Some(Provider {
        label: "TMDB".to_owned(),
        id: "949".to_owned(),
    });
    facts.organize = Organize::Pending("1 change".to_owned());
    facts
}

fn outcome(step: &str, status: StepStatus) -> StepOutcome {
    StepOutcome {
        item: ItemId(1),
        step: step.to_owned(),
        status,
        reason: String::new(),
        at_ms: 1_700_000_100_000,
    }
}

fn kinds(lifecycle: &Lifecycle) -> Vec<FlagKind> {
    lifecycle.flags.iter().map(|f| f.kind).collect()
}

fn texts(line: &Line) -> Vec<&str> {
    line.parts.iter().map(|p| p.text.as_str()).collect()
}

// ---- stages -------------------------------------------------------------------------------

#[test]
fn a_file_the_scan_just_found_is_new() {
    let mut facts = Facts::new(ItemKind::Movie);
    facts.title = "heat 1995".to_owned();
    let lifecycle = derive(&facts);
    assert_eq!(lifecycle.stage, Stage::New);
    // No scrape was tried, so it is just New: not "Not matched", and no decision
    // waits, so Needs review does not count it either.
    assert!(lifecycle.flags.is_empty(), "{:?}", lifecycle.flags);
    assert!(!lifecycle.in_review);
    assert_eq!(lifecycle.lines[1].label, NOT_SCRAPED_YET);
    assert_eq!(lifecycle.lines[1].status, LineStatus::Waiting);
}

fn scrape_miss(status: StepStatus, reason: &str) -> StepOutcome {
    StepOutcome {
        reason: reason.to_owned(),
        ..outcome(STEP_SCRAPE, status)
    }
}

#[test]
fn a_scrape_that_found_no_confident_match_and_waits_for_nothing_is_not_matched() {
    // The review was set aside: the stored scrape outcome still says what the scrape found.
    let mut facts = Facts::new(ItemKind::Movie);
    facts.outcomes = vec![scrape_miss(StepStatus::NotFound, "2 strong candidates")];
    let lifecycle = derive(&facts);
    assert_eq!(lifecycle.stage, Stage::New);
    assert_eq!(kinds(&lifecycle), [FlagKind::NotMatched]);
    assert_eq!(lifecycle.flags[0].reason, "2 strong candidates");
    assert!(!lifecycle.in_review, "nothing waits in the review queue");
    let line = &lifecycle.lines[1];
    assert_eq!(line.label, "Not matched");
    assert_eq!(line.status, LineStatus::Missing);
    assert_eq!(line.at_ms, Some(1_700_000_100_000));
    assert_eq!(texts(line), ["2 strong candidates"]);
}

#[test]
fn a_failed_or_keyless_scrape_is_not_matched_with_why() {
    let mut facts = Facts::new(ItemKind::Movie);
    facts.outcomes = vec![scrape_miss(
        StepStatus::Failed,
        "The scrape failed: provider unreachable",
    )];
    let lifecycle = derive(&facts);
    assert_eq!(
        lifecycle.flags[0],
        Flag::new(
            FlagKind::NotMatched,
            "The scrape failed: provider unreachable"
        )
    );

    facts.outcomes = vec![scrape_miss(StepStatus::NeedsKey, "")];
    let lifecycle = derive(&facts);
    assert_eq!(lifecycle.flags[0].reason, "The provider needs a key");
    assert_eq!(lifecycle.lines[1].status, LineStatus::NeedsKey);
}

#[test]
fn a_successful_scrape_or_a_match_clears_not_matched() {
    // The last scrape matched.
    let mut facts = Facts::new(ItemKind::Movie);
    facts.outcomes = vec![outcome(STEP_SCRAPE, StepStatus::Done)];
    assert!(derive(&facts).flags.is_empty());

    // An old miss next to a provider match (an NFO with an ID turned up later).
    let mut facts = scraped_movie();
    facts.organize = Organize::Tidy;
    facts.outcomes = vec![scrape_miss(StepStatus::NotFound, "No candidates found")];
    let lifecycle = derive(&facts);
    assert!(lifecycle.flags.is_empty(), "{:?}", lifecycle.flags);
    assert_eq!(lifecycle.lines[1].label, "Scraped");
}

#[test]
fn a_title_with_an_id_but_no_details_is_still_new() {
    // An ID read from a folder tag is not a scrape.
    let mut facts = Facts::new(ItemKind::Movie);
    facts.has_provider_id = true;
    let lifecycle = derive(&facts);
    assert_eq!(lifecycle.stage, Stage::New);
    assert!(
        lifecycle.flags.is_empty(),
        "it has a match, only no details"
    );
}

#[test]
fn a_title_is_scraped_once_matched_even_if_a_rating_source_found_nothing() {
    let mut facts = scraped_movie();
    facts.outcomes = vec![
        outcome(STEP_ARTWORK, StepStatus::Done),
        outcome(STEP_RATINGS, StepStatus::Done),
        outcome(&source_step("omdb"), StepStatus::NotFound),
        outcome(&source_step("imdb-dataset"), StepStatus::Done),
    ];
    assert_eq!(derive(&facts).stage, Stage::Scraped);
}

#[test]
fn a_title_read_from_an_nfo_with_a_provider_id_is_scraped() {
    let mut facts = Facts::new(ItemKind::Movie);
    facts.has_provider_id = true;
    facts.has_nfo = true;
    assert_eq!(derive(&facts).stage, Stage::Scraped);
}

#[test]
fn a_scraped_title_whose_names_match_the_style_is_renamed() {
    let mut facts = scraped_movie();
    facts.organize = Organize::Tidy;
    let lifecycle = derive(&facts);
    assert_eq!(lifecycle.stage, Stage::Renamed);
    assert!(lifecycle.flags.is_empty());
    assert_eq!(lifecycle.lines[2].label, "Renamed");
    assert_eq!(lifecycle.lines[2].status, LineStatus::Done);
}

#[test]
fn a_title_that_is_not_scraped_is_never_renamed_even_when_its_names_look_tidy() {
    let mut facts = Facts::new(ItemKind::Movie);
    facts.organize = Organize::Tidy;
    assert_eq!(derive(&facts).stage, Stage::New);
}

// ---- flags --------------------------------------------------------------------------------

#[test]
fn an_uncertain_match_stays_new_and_needs_review_with_its_reason() {
    let mut facts = Facts::new(ItemKind::Movie);
    facts.review = Some(Review {
        reason: "2 strong candidates".to_owned(),
        no_candidates: false,
    });
    let lifecycle = derive(&facts);
    assert_eq!(lifecycle.stage, Stage::New);
    let flag = lifecycle.main_flag().unwrap();
    assert_eq!(flag.kind, FlagKind::NeedsReview);
    assert_eq!(flag.reason, "2 strong candidates");
    assert_eq!(
        kinds(&lifecycle),
        [FlagKind::NeedsReview],
        "not also Not matched"
    );
    assert!(lifecycle.in_review);
}

#[test]
fn a_title_the_search_found_nothing_for_is_not_matched_with_the_reason() {
    let mut facts = Facts::new(ItemKind::Movie);
    facts.review = Some(Review {
        reason: "No candidates found".to_owned(),
        no_candidates: true,
    });
    let lifecycle = derive(&facts);
    assert_eq!(kinds(&lifecycle), [FlagKind::NotMatched]);
    assert_eq!(lifecycle.flags[0].reason, "No candidates found");
    assert!(
        lifecycle.in_review,
        "a search that found nothing waits in Needs review"
    );
    assert_eq!(lifecycle.lines[1].label, "Not matched");
}

#[test]
fn a_locked_title_is_locked_and_not_rename_pending() {
    let mut facts = scraped_movie();
    facts.locked = true;
    facts.organize = Organize::Locked;
    let lifecycle = derive(&facts);
    assert_eq!(kinds(&lifecycle), [FlagKind::Locked]);
    // Even a stale cache that still says pending cannot make a locked title pending.
    facts.organize = Organize::Pending("2 changes".to_owned());
    assert_eq!(kinds(&derive(&facts)), [FlagKind::Locked]);
}

#[test]
fn a_pending_rename_is_a_flag_in_manual_mode_and_nothing_moves() {
    let facts = scraped_movie();
    let lifecycle = derive(&facts);
    assert_eq!(lifecycle.stage, Stage::Scraped);
    assert_eq!(kinds(&lifecycle), [FlagKind::RenamePending]);
    assert_eq!(lifecycle.flags[0].reason, "1 change");
}

#[test]
fn a_rename_conflict_is_pending_in_organize_and_never_needs_review() {
    let mut facts = scraped_movie();
    facts.organize = Organize::Conflict("It would get the same name as Heat (1995).".to_owned());
    let lifecycle = derive(&facts);
    assert_eq!(kinds(&lifecycle), [FlagKind::RenamePending]);
    assert!(!lifecycle.in_review);
    assert_eq!(lifecycle.lines[2].link, Some(LineLink::OrganizeResolve));
}

#[test]
fn missing_files_and_an_offline_folder_are_the_missing_flag() {
    let mut facts = scraped_movie();
    facts.availability = Availability::Missing;
    assert!(derive(&facts).has(FlagKind::Missing));
    facts.availability = Availability::Offline;
    assert!(derive(&facts).has(FlagKind::Missing));
    facts.availability = Availability::Invalid;
    assert!(!derive(&facts).has(FlagKind::Missing));
}

#[test]
fn the_main_flag_is_work_in_progress_then_what_needs_the_user_then_what_is_pending() {
    let mut facts = scraped_movie();
    facts.locked = true;
    facts.availability = Availability::Missing;
    facts.review = Some(Review {
        reason: "Best match 62% is below 75%".to_owned(),
        no_candidates: false,
    });
    assert_eq!(
        kinds(&derive(&facts)),
        [FlagKind::NeedsReview, FlagKind::Missing, FlagKind::Locked]
    );
    facts.working = true;
    assert_eq!(derive(&facts).main_flag().unwrap().kind, FlagKind::Working);
}

// ---- explanation lines --------------------------------------------------------------------

#[test]
fn the_scan_line_names_the_library_and_the_title() {
    let line = &derive(&scraped_movie()).lines[0];
    assert_eq!(line.step, LineStep::Scan);
    assert_eq!(line.label, "Scanned");
    assert_eq!(line.at_ms, Some(1_700_000_000_000));
    assert_eq!(texts(line), ["Movies \u{203a} Heat (1995)"]);
}

#[test]
fn the_scrape_line_lists_the_provider_and_how_each_extra_went() {
    let mut facts = scraped_movie();
    facts.outcomes = vec![
        outcome(STEP_ARTWORK, StepStatus::Done),
        outcome(STEP_RATINGS, StepStatus::Done),
        outcome(&source_step("imdb-dataset"), StepStatus::Done),
        outcome(&source_step("omdb"), StepStatus::NeedsKey),
        outcome(&source_step("mdblist"), StepStatus::NeedsKey),
    ];
    let line = &derive(&facts).lines[1];
    assert_eq!(line.label, "Scraped");
    assert_eq!(line.at_ms, Some(1_700_000_100_000));
    assert_eq!(texts(line), ["TMDB 949", "artwork saved", "ratings 1 of 3"]);
    let status = |at: usize| line.parts[at].status;
    assert_eq!(status(1), Some(LineStatus::Done));
    assert_eq!(status(2), Some(LineStatus::NeedsKey));
}

#[test]
fn an_extra_that_is_switched_off_reads_off_and_one_that_has_not_run_reads_waiting() {
    let mut facts = scraped_movie();
    let line = &derive(&facts).lines[1];
    assert!(
        texts(line).contains(&"artwork waiting"),
        "{:?}",
        texts(line)
    );
    facts.extras.ratings = false;
    let line = &derive(&facts).lines[1];
    assert!(texts(line).contains(&"ratings off"), "{:?}", texts(line));
}

#[test]
fn the_rename_line_of_a_pending_rename_says_how_many_changes_wait_in_organize() {
    let facts = scraped_movie();
    let line = &derive(&facts).lines[2];
    assert_eq!(line.label, "Renamed: no");
    assert_eq!(texts(line)[0], "1 change pending in Organize");
    assert_eq!(texts(line)[1], "nothing moves until you apply it");
    assert_eq!(line.link, Some(LineLink::OrganizePreview));
    assert_eq!(line.status, LineStatus::Waiting);
}

#[test]
fn the_rename_line_of_a_stale_cache_says_checking() {
    let mut facts = scraped_movie();
    facts.organize = Organize::Checking;
    let line = &derive(&facts).lines[2];
    assert_eq!(line.status, LineStatus::InProgress);
    assert!(line.label.starts_with("Renamed: checking"));
}

// ---- reading and counting over a real library ----------------------------------------------

struct Lab {
    _dir: tempfile::TempDir,
    db: Arc<Database>,
}

impl Lab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
        Self { _dir: dir, db }
    }

    fn movie(&self, title: &str, tmdb: Option<&str>, scraper: &str) -> ItemId {
        let mut item = Item::movie(title);
        item.core.year = 2000;
        if let Some(tmdb) = tmdb {
            item.core.ids.set("tmdb", tmdb);
        }
        item.core.last_scraper_id = scraper.to_owned();
        self.db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    fn review(&self, item: ItemId, reason: &str, candidates: usize) {
        let review = NewReview {
            item_id: item,
            reason: reason.to_owned(),
            best_score: (candidates > 0).then_some(0.6),
            candidates: json!(vec![json!({}); candidates]),
            score_breakdown: None,
        };
        self.db.write(move |tx| enqueue(tx, &review)).unwrap();
    }

    /// Stores how the item's last scrape ended, as the matcher does.
    fn scrape(&self, item: ItemId, status: StepStatus, reason: &str) {
        let reason = reason.to_owned();
        self.db
            .write(move |tx| record_step_outcome(tx, item, STEP_SCRAPE, status, &reason, 5))
            .unwrap();
    }

    fn organize(&self, item: ItemId, state: OrganizeStateKind, reason: &str) {
        let entry = OrganizeStateEntry {
            item,
            state,
            reason: reason.to_owned(),
        };
        self.db
            .write(move |tx| set_organize_states(tx, &[entry], "p", 1))
            .unwrap();
    }

    fn context(&self) -> LifecycleContext {
        LifecycleContext {
            profile_key: "p".to_owned(),
            ..LifecycleContext::default()
        }
    }

    fn counts(&self) -> LifecycleCounts {
        lifecycle_counts(&self.db, ItemKind::Movie, "p", 0).unwrap()
    }
}

#[test]
fn needs_review_is_uncertain_plus_not_matched() {
    let lab = Lab::new();
    // 4 uncertain matches and 2 titles the search found nothing for (the spec's example).
    for n in 0..4 {
        let id = lab.movie(&format!("Uncertain {n}"), None, "");
        lab.review(id, "2 strong candidates", 2);
    }
    for n in 0..2 {
        let id = lab.movie(&format!("Nothing {n}"), None, "");
        lab.review(id, "No candidates found", 0);
    }
    // 3 scraped titles: one renamed, one with a rename conflict, one with a pending rename.
    let tidy = lab.movie("Tidy", Some("1"), "tmdb");
    lab.organize(tidy, OrganizeStateKind::Tidy, "");
    let conflict = lab.movie("Conflict", Some("2"), "tmdb");
    lab.organize(conflict, OrganizeStateKind::Conflict, "same folder");
    let pending = lab.movie("Pending", Some("3"), "tmdb");
    lab.organize(pending, OrganizeStateKind::Pending, "1 change");
    // A new title nobody has tried to scrape yet: just New, not Not matched.
    lab.movie("Fresh", None, "");

    let manual = lab.counts();
    assert_eq!(manual.total, 10);
    assert_eq!((manual.new, manual.scraped, manual.renamed), (7, 2, 1));
    assert_eq!(
        (manual.uncertain, manual.could_not_match, manual.not_matched),
        (4, 2, 2)
    );
    assert_eq!(manual.not_scraped, 1);
    assert_eq!(manual.needs_review, 6, "a rename conflict is not a review");
    // The conflict waits in Organize, next to the pending rename.
    assert_eq!(manual.rename_pending, 2);
    assert_eq!(
        manual.not_matched_top_reason,
        Some(("No candidates found".to_owned(), 2))
    );
}

#[test]
fn locked_missing_and_checking_titles_are_counted_and_a_locked_one_is_never_pending() {
    let lab = Lab::new();
    let locked = lab.movie("Locked", Some("1"), "tmdb");
    lab.organize(locked, OrganizeStateKind::Locked, "");
    lab.db
        .write(move |tx| set_item_locked(tx, &[locked], true))
        .unwrap();
    let gone = lab.movie("Gone", Some("2"), "tmdb");
    lab.organize(gone, OrganizeStateKind::Pending, "1 change");
    lab.db
        .write(move |tx| {
            tx.execute(
                "UPDATE item SET availability = 'offline' WHERE id = ?1",
                [gone.0],
            )?;
            Ok(())
        })
        .unwrap();
    // No organize state yet.
    lab.movie("Waiting", Some("3"), "tmdb");

    let counts = lab.counts();
    assert_eq!((counts.locked, counts.missing), (1, 1));
    assert_eq!(counts.checking, 1);
    assert_eq!(
        counts.rename_pending, 1,
        "only the offline title is pending"
    );
}

#[test]
fn the_counts_follow_the_derived_lifecycle_of_every_title() {
    let lab = Lab::new();
    let a = lab.movie("A", Some("1"), "tmdb");
    lab.organize(a, OrganizeStateKind::Tidy, "");
    let b = lab.movie("B", None, "");
    lab.review(b, "Best match 62% is below 75%", 1);
    let c = lab.movie("C", Some("3"), "tmdb");
    lab.organize(c, OrganizeStateKind::Pending, "2 changes");

    let ids = [a, b, c];
    let read = read(&lab.db, &lab.context(), &ids).unwrap();
    assert_eq!(read[&a].stage, Stage::Renamed);
    assert_eq!(read[&b].stage, Stage::New);
    assert_eq!(read[&b].main_flag().unwrap().kind, FlagKind::NeedsReview);
    assert_eq!(
        read[&c].lines[2].parts[0].text,
        "2 changes pending in Organize"
    );

    let counts = lab.counts();
    for (stage, expected) in [
        (Stage::Renamed, counts.renamed),
        (Stage::New, counts.new),
        (Stage::Scraped, counts.scraped),
    ] {
        let derived = read.values().filter(|l| l.stage == stage).count();
        assert_eq!(derived as u32, expected, "{stage:?}");
    }
    let flagged = |kind| read.values().filter(|l| l.has(kind)).count() as u32;
    assert_eq!(flagged(FlagKind::NeedsReview), counts.needs_review);
    assert_eq!(flagged(FlagKind::RenamePending), counts.rename_pending);
    let in_review = read.values().filter(|l| l.in_review).count() as u32;
    assert_eq!(in_review, counts.needs_review);
}

/// Every pipeline count opens the library filtered to exactly the titles it counted: the
/// lifecycle filter of `cv-db` follows the same rules as [`lifecycle_counts`] and [`derive`].
#[test]
fn each_lifecycle_filter_lists_exactly_the_titles_its_count_counted() {
    use cv_db::repo::{Filter, ItemQuery, count_items};

    let lab = Lab::new();
    for n in 0..3 {
        let id = lab.movie(&format!("Uncertain {n}"), None, "");
        lab.review(id, "2 strong candidates", 2);
    }
    let nothing = lab.movie("Nothing", None, "");
    lab.review(nothing, "No candidates found", 0);
    // Never tried: just New.
    lab.movie("Fresh", None, "");
    lab.movie("Fresh too", None, "");
    // Tried, no confident match, the review set aside: Not matched, not in Needs review.
    let set_aside = lab.movie("Set aside", None, "");
    lab.scrape(set_aside, StepStatus::NotFound, "2 strong candidates");
    // Tried and failed: Not matched too.
    let failed = lab.movie("Failed", None, "");
    lab.scrape(
        failed,
        StepStatus::Failed,
        "The scrape failed: provider unreachable",
    );
    // An old miss, then a provider ID read from an NFO: matched.
    let later = lab.movie("Matched later", Some("8"), "");
    lab.scrape(later, StepStatus::NotFound, "No candidates found");
    // An ID read from a folder tag: no details yet, but matched.
    lab.movie("Tagged", Some("9"), "");
    let tidy = lab.movie("Tidy", Some("1"), "tmdb");
    lab.organize(tidy, OrganizeStateKind::Tidy, "");
    let conflict = lab.movie("Conflict", Some("2"), "tmdb");
    lab.organize(conflict, OrganizeStateKind::Conflict, "same folder");
    let pending = lab.movie("Pending", Some("3"), "tmdb");
    lab.organize(pending, OrganizeStateKind::Pending, "1 change");
    let locked = lab.movie("Locked", Some("4"), "tmdb");
    lab.organize(locked, OrganizeStateKind::Pending, "1 change");
    lab.db
        .write(move |tx| set_item_locked(tx, &[locked], true))
        .unwrap();
    let gone = lab.movie("Gone", Some("5"), "tmdb");
    lab.organize(gone, OrganizeStateKind::Pending, "1 change");
    lab.db
        .write(move |tx| {
            tx.execute(
                "UPDATE item SET availability = 'missing' WHERE id = ?1",
                [gone.0],
            )?;
            Ok(())
        })
        .unwrap();

    let counts = lab.counts();
    let listed = |flag: &str| {
        let query = ItemQuery::new(ItemKind::Movie).filter(Filter::Lifecycle {
            flags: vec![flag.to_owned()],
            profile_key: "p".to_owned(),
        });
        let n = lab.db.read(|c| count_items(c, &query)).unwrap();
        u32::try_from(n).unwrap()
    };
    let expected = [
        ("needs-review", counts.needs_review),
        ("not-matched", counts.not_matched),
        ("rename-pending", counts.rename_pending),
        ("missing", counts.missing),
        ("locked", counts.locked),
    ];
    for (flag, count) in expected {
        assert_eq!(listed(flag), count, "{flag}");
    }
    assert_eq!(listed("no-such-flag"), 0);
    let manual = lab.counts();
    assert_eq!(
        (
            manual.needs_review,
            manual.not_matched,
            manual.rename_pending
        ),
        (4, 3, 3)
    );
    // Fresh, Fresh too, Tagged and Matched later wait for their first scrape.
    assert_eq!(manual.not_scraped, 4);
    assert_eq!(
        manual.not_matched_top_reason,
        Some(("2 strong candidates".to_owned(), 1))
    );

    // The same sets as the derived lifecycle of each title.
    let ids: Vec<ItemId> = lab
        .db
        .read(|c| {
            cv_db::repo::list_items(c, &ItemQuery::new(ItemKind::Movie), None, 100)
                .map(|page| page.items.into_iter().map(|i| i.id).collect())
        })
        .unwrap();
    let read = read(&lab.db, &lab.context(), &ids).unwrap();
    let flagged = |kind| read.values().filter(|l| l.has(kind)).count() as u32;
    assert_eq!(flagged(FlagKind::NotMatched), manual.not_matched);
    assert_eq!(flagged(FlagKind::Missing), manual.missing);
    let waiting = read
        .values()
        .filter(|l| l.stage == Stage::New && l.flags.is_empty() && !l.in_review)
        .count() as u32;
    assert_eq!(waiting, manual.not_scraped);
    assert_eq!(
        read.values().filter(|l| l.in_review).count() as u32,
        manual.needs_review
    );
}

#[test]
fn a_season_and_an_episode_take_the_lifecycle_of_their_show() {
    let lab = Lab::new();
    let mut show = Item::show("Dark");
    show.core.ids.set("tvdb", "334");
    show.core.last_scraper_id = "tvdb".to_owned();
    let show_id = lab.db.write(move |tx| insert_item(tx, &show)).unwrap();
    let season = lab
        .db
        .write(move |tx| insert_item(tx, &Item::season(show_id, 1)))
        .unwrap();
    let episode = lab
        .db
        .write(move |tx| insert_item(tx, &Item::episode(show_id, "Secrets")))
        .unwrap();
    lab.organize(show_id, OrganizeStateKind::Tidy, "");

    let read = read(&lab.db, &lab.context(), &[show_id, season, episode]).unwrap();
    for id in [show_id, season, episode] {
        assert_eq!(read[&id].stage, Stage::Renamed, "{id:?}");
    }
    assert_eq!(
        read[&episode].lines[0].parts[0].text,
        "TV shows \u{203a} Dark"
    );
}

#[test]
fn a_title_with_an_nfo_file_and_a_provider_id_reads_as_scraped() {
    let lab = Lab::new();
    let mut item = Item::movie("Heat");
    item.core.ids.set("imdb", "tt0113277");
    item.core.media_files.push(MediaFile::new(
        MediaFileType::Nfo,
        "/m/Heat".to_owned(),
        "heat.nfo".to_owned(),
    ));
    let id = lab.db.write(move |tx| insert_item(tx, &item)).unwrap();
    let read = read(&lab.db, &lab.context(), &[id]).unwrap();
    assert_eq!(read[&id].stage, Stage::Scraped);
    assert_eq!(counts_new(&lab), 0);

    // The step outcomes the scrape left behind feed the line.
    lab.db
        .write(move |tx| record_step_outcome(tx, id, STEP_ARTWORK, StepStatus::Done, "", 5))
        .unwrap();
    let read = super::read(&lab.db, &lab.context(), &[id]).unwrap();
    assert!(
        read[&id].lines[1]
            .parts
            .iter()
            .any(|p| p.text == "artwork saved")
    );
}

/// The Scan line names where the title is now, by its library folder's
/// label, as the Location line does: after a move to Archive it says Archive.
#[test]
fn the_scan_line_names_the_library_folder_the_title_is_in_now() {
    let lab = Lab::new();
    for (path, label) in [("D:\\Movies", "Movies"), ("E:\\Archive", "Archive")] {
        lab.db
            .write(move |tx| {
                let made = cv_db::repo::data_sources::insert_source(
                    tx,
                    cv_domain::SourceKind::Movie,
                    path,
                )?;
                tx.execute(
                    "UPDATE data_source SET label = ?1 WHERE id = ?2",
                    rusqlite::params![label, made.id],
                )?;
                Ok(())
            })
            .unwrap();
    }
    let mut item = Item::movie("Heat");
    item.core.year = 1995;
    item.core.data_source = "E:\\Archive".to_owned();
    item.core.path = "E:\\Archive\\Heat (1995)".to_owned();
    let id = lab.db.write(move |tx| insert_item(tx, &item)).unwrap();
    let read = read(&lab.db, &lab.context(), &[id]).unwrap();
    assert_eq!(texts(&read[&id].lines[0]), ["Archive \u{203a} Heat (1995)"]);
}

fn counts_new(lab: &Lab) -> u32 {
    lab.counts().new
}

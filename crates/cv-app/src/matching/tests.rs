//! Tests of the automatic matching decisions over a real database and a fake provider.

use async_trait::async_trait;
use cv_db::Database;
use cv_db::repo::review::{self, ReviewQueueState};
use cv_db::repo::{get_flags, get_item, insert_item, set_item_locked};
use cv_domain::{Item, ItemId, ProviderIds, ReviewState, ScrapeBlock};
use cv_match::{AcceptRules, LocalEvidence, ReviewReason, normalize_query, rank_candidates};
use cv_providers::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use cv_providers::{MetadataProvider, Provider, ProviderError};

use super::*;

/// A provider that knows two titles by TMDB ID and fills the IMDb ID in its details.
struct Fake;

#[async_trait]
impl Provider for Fake {
    fn id(&self) -> &'static str {
        "tmdb"
    }
    fn name(&self) -> &'static str {
        "Fake"
    }
}

#[async_trait]
impl MetadataProvider for Fake {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: true,
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
        kind: MediaKind,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let (title, imdb, year) = match ids.get("tmdb") {
            Some("593") => ("Solaris", "tt0069293", 1972),
            Some("2103") => ("Solaris", "tt0307479", 2002),
            _ => return Err(ProviderError::NotFound("tmdb".to_owned())),
        };
        let mut item = match kind {
            MediaKind::Movie => Item::movie(title),
            MediaKind::Show => Item::show(title),
        };
        item.core.year = year;
        item.core.plot = format!("Plot of {title} ({year})");
        item.core.ids = ids.clone();
        item.core.ids.set("imdb", imdb);
        Ok(item)
    }
}

struct Lab {
    _dir: tempfile::TempDir,
    db: Database,
    settings: MatchSettings,
}

impl Lab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        Self {
            _dir: dir,
            db,
            settings: MatchSettings::default(),
        }
    }

    fn movie(&self, title: &str) -> ItemId {
        let item = Item::movie(title);
        self.db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    fn show(&self, title: &str) -> ItemId {
        let item = Item::show(title);
        self.db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    fn item(&self, id: ItemId) -> Item {
        self.db.read(|c| get_item(c, id)).unwrap().unwrap()
    }

    fn review_state(&self, id: ItemId) -> ReviewState {
        self.db.read(|c| get_flags(c, id)).unwrap().review_state
    }

    fn pending(&self) -> u64 {
        self.db.read(review::count_pending).unwrap()
    }

    async fn resolve(&self, id: ItemId, ranked: &[ScoredCandidate]) -> MatchResolution {
        resolve_match(&self.db, id, ranked, &[&Fake], &self.settings)
            .await
            .unwrap()
    }
}

fn hit(title: &str, year: u16, tmdb: &str) -> SearchHit {
    let mut ids = ProviderIds::new();
    ids.set("tmdb", tmdb);
    SearchHit {
        provider: "tmdb".to_owned(),
        ids,
        title: title.to_owned(),
        year,
        poster_url: Some("https://img/p.jpg".to_owned()),
        ..SearchHit::default()
    }
}

fn ranked(query: &str, hits: &[SearchHit]) -> Vec<ScoredCandidate> {
    rank_candidates(
        &normalize_query(query),
        hits.iter().map(candidate_from_hit).collect(),
        &LocalEvidence::default(),
    )
}

#[tokio::test]
async fn a_confident_match_is_scraped_and_needs_no_review() {
    let lab = Lab::new();
    let id = lab.movie("Solaris");
    let list = ranked(
        "Solaris (1972)",
        &[hit("Solaris", 1972, "593"), hit("Solaris", 2002, "2103")],
    );

    let MatchResolution::Accepted(done) = lab.resolve(id, &list).await else {
        panic!("expected an accepted match");
    };
    assert_eq!(done.candidate.candidate.year, 1972);
    assert!(done.scrape.blocked.is_none());
    assert!(done.scrape.changed());

    let item = lab.item(id);
    assert_eq!(item.core.plot, "Plot of Solaris (1972)");
    assert_eq!(item.core.year, 1972);
    assert_eq!(item.core.ids.get("tmdb"), Some("593"));
    assert_eq!(item.core.ids.get("imdb"), Some("tt0069293"));
    assert_eq!(item.core.last_scraper_id, "tmdb");
    assert_eq!(item.core.last_scrape_language, "en");
    assert_eq!(lab.review_state(id), ReviewState::None);
    assert_eq!(lab.pending(), 0);
}

#[tokio::test]
async fn a_tie_is_queued_with_the_two_strong_candidates_reason() {
    let lab = Lab::new();
    let id = lab.movie("Solaris");
    // No year and no evidence: both titles score the same.
    let list = ranked(
        "Solaris",
        &[hit("Solaris", 1972, "593"), hit("Solaris", 2002, "2103")],
    );

    let MatchResolution::Queued {
        review_id,
        reason,
        refreshed,
    } = lab.resolve(id, &list).await
    else {
        panic!("expected a queued item");
    };
    assert_eq!(reason, ReviewReason::Tie { count: 2 });
    assert!(!refreshed);

    let row = lab
        .db
        .read(|c| review::get_review(c, review_id))
        .unwrap()
        .unwrap();
    assert_eq!(row.reason, "2 strong candidates");
    assert_eq!(row.state, ReviewQueueState::Pending);
    assert_eq!(row.best_score, Some(list[0].breakdown.score));
    let stored = candidates_of(&row).unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].candidate.ids.get("tmdb"), Some("593"));
    assert_eq!(stored[0].breakdown, list[0].breakdown);
    assert_eq!(
        row.score_breakdown.unwrap(),
        serde_json::to_value(&list[0].breakdown).unwrap()
    );
    // Not matched: nothing was scraped.
    assert_eq!(lab.item(id).core.plot, "");
    assert_eq!(lab.review_state(id), ReviewState::NeedsReview);
}

#[tokio::test]
async fn a_best_match_under_the_threshold_is_queued_with_its_score() {
    let lab = Lab::new();
    let id = lab.movie("Mirror");
    let list = ranked("Mirror", &[hit("Solaris", 1972, "593")]);
    assert!(list[0].breakdown.score < 0.75);

    let MatchResolution::Queued { review_id, .. } = lab.resolve(id, &list).await else {
        panic!("expected a queued item");
    };
    let row = lab
        .db
        .read(|c| review::get_review(c, review_id))
        .unwrap()
        .unwrap();
    assert!(row.reason.starts_with("Best match "), "{}", row.reason);
    assert!(row.reason.ends_with("is below 75%"), "{}", row.reason);
    assert_eq!(lab.item(id).core.plot, "");
}

#[tokio::test]
async fn the_threshold_is_configurable() {
    let mut lab = Lab::new();
    let id = lab.movie("Mirror");
    let list = ranked("Mirror", &[hit("Solaris", 1972, "593")]);
    lab.settings.rules = AcceptRules::new(list[0].breakdown.score - 0.01);
    assert!(matches!(
        lab.resolve(id, &list).await,
        MatchResolution::Accepted(_)
    ));
    assert_eq!(lab.item(id).core.plot, "Plot of Solaris (1972)");
}

#[tokio::test]
async fn a_show_with_a_single_weak_result_still_goes_to_review() {
    let lab = Lab::new();
    let id = lab.show("Mirror");
    let list = ranked("Mirror", &[hit("Solaris", 1972, "593")]);
    assert_eq!(list.len(), 1);
    assert!(matches!(
        lab.resolve(id, &list).await,
        MatchResolution::Queued { .. }
    ));
    assert_eq!(lab.review_state(id), ReviewState::NeedsReview);

    // A single strong result is accepted.
    let strong = lab.show("Solaris");
    let list = ranked("Solaris (1972)", &[hit("Solaris", 1972, "593")]);
    assert!(matches!(
        lab.resolve(strong, &list).await,
        MatchResolution::Accepted(_)
    ));
}

#[tokio::test]
async fn no_candidates_are_queued_with_a_reason_and_no_score() {
    let lab = Lab::new();
    let id = lab.movie("Unknown Home Movie");
    let MatchResolution::Queued { review_id, .. } = lab.resolve(id, &[]).await else {
        panic!("expected a queued item");
    };
    let row = lab
        .db
        .read(|c| review::get_review(c, review_id))
        .unwrap()
        .unwrap();
    assert_eq!(row.reason, "No candidates found");
    assert_eq!(row.best_score, None);
    assert_eq!(row.score_breakdown, None);
}

#[tokio::test]
async fn running_matching_again_refreshes_the_one_pending_row() {
    let lab = Lab::new();
    let id = lab.movie("Solaris");
    let tie = ranked(
        "Solaris",
        &[hit("Solaris", 1972, "593"), hit("Solaris", 2002, "2103")],
    );
    lab.resolve(id, &tie).await;
    let MatchResolution::Queued { refreshed, .. } = lab.resolve(id, &tie).await else {
        panic!("expected a queued item");
    };
    assert!(refreshed);
    assert_eq!(lab.pending(), 1);
}

#[tokio::test]
async fn accepting_a_queued_item_closes_its_row_and_resolves_it() {
    let lab = Lab::new();
    let id = lab.movie("Solaris");
    let tie = ranked(
        "Solaris",
        &[hit("Solaris", 1972, "593"), hit("Solaris", 2002, "2103")],
    );
    lab.resolve(id, &tie).await;
    assert_eq!(lab.pending(), 1);

    // The user picks the second candidate.
    let picked = tie
        .iter()
        .find(|c| c.candidate.year == 2002)
        .unwrap()
        .clone();
    let done = accept_candidate(&lab.db, id, &picked, &[&Fake], &lab.settings)
        .await
        .unwrap();
    assert!(done.scrape.changed());
    assert_eq!(lab.item(id).core.year, 2002);
    assert_eq!(lab.pending(), 0);
    assert_eq!(lab.review_state(id), ReviewState::Resolved);
    let history = lab.db.read(|c| review::list_pending(c, 10, 0)).unwrap();
    assert!(history.is_empty());
}

#[tokio::test]
async fn do_not_overwrite_and_field_selection_are_honoured() {
    let mut lab = Lab::new();
    let mut item = Item::movie("Solaris");
    item.core.plot = "My own plot".to_owned();
    let id = lab.db.write(move |tx| insert_item(tx, &item)).unwrap();
    lab.settings.scrape = ScrapeSettings::from_values(
        &[
            (
                "metadata.scrape-do-not-overwrite".to_owned(),
                serde_json::json!(true),
            ),
            (
                "metadata.scrape-fields-movie".to_owned(),
                serde_json::json!("plot,year"),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let list = ranked("Solaris (1972)", &[hit("Solaris", 1972, "593")]);
    let MatchResolution::Accepted(done) = lab.resolve(id, &list).await else {
        panic!("expected an accepted match");
    };
    assert!(!done.scrape.kept.is_empty());
    let item = lab.item(id);
    assert_eq!(item.core.plot, "My own plot");
    assert_eq!(item.core.year, 1972);
}

#[tokio::test]
async fn a_locked_item_is_not_written_and_its_review_row_stays_open() {
    let lab = Lab::new();
    let id = lab.movie("Solaris");
    let tie = ranked(
        "Solaris",
        &[hit("Solaris", 1972, "593"), hit("Solaris", 2002, "2103")],
    );
    lab.resolve(id, &tie).await;
    lab.db
        .write(move |tx| set_item_locked(tx, &[id], true))
        .unwrap();

    let done = accept_candidate(&lab.db, id, &tie[0], &[&Fake], &lab.settings)
        .await
        .unwrap();
    assert_eq!(done.scrape.blocked, Some(ScrapeBlock::ItemLocked));
    assert!(done.ids_added.is_empty());
    assert_eq!(lab.item(id).core.plot, "");
    assert_eq!(lab.pending(), 1);
    assert_eq!(lab.review_state(id), ReviewState::NeedsReview);
}

#[tokio::test]
async fn an_ignored_item_stays_out_of_matching_and_the_inbox() {
    let lab = Lab::new();
    let id = lab.movie("Birthday 2019");
    let tie = ranked(
        "Birthday",
        &[hit("Solaris", 1972, "593"), hit("Solaris", 2002, "2103")],
    );
    lab.resolve(id, &tie).await;
    assert_eq!(lab.pending(), 1);

    let row_id = ignore_item(&lab.db, id).unwrap();
    assert_eq!(lab.pending(), 0);
    assert_eq!(lab.review_state(id), ReviewState::Resolved);

    // A later run neither scrapes nor queues it, even with a perfect candidate.
    let perfect = ranked("Solaris (1972)", &[hit("Solaris", 1972, "593")]);
    assert_eq!(lab.resolve(id, &perfect).await, MatchResolution::Ignored);
    assert_eq!(lab.resolve(id, &tie).await, MatchResolution::Ignored);
    assert_eq!(lab.pending(), 0);
    assert_eq!(lab.item(id).core.plot, "");

    // Undo brings it back.
    lab.db.write(move |tx| review::reopen(tx, row_id)).unwrap();
    assert_eq!(lab.pending(), 1);
    assert!(matches!(
        lab.resolve(id, &perfect).await,
        MatchResolution::Accepted(_)
    ));
}

#[tokio::test]
async fn the_queue_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let (id, list) = {
        let db = Database::open(&path).unwrap();
        let item = Item::movie("Solaris");
        let id = db.write(move |tx| insert_item(tx, &item)).unwrap();
        let list = ranked(
            "Solaris",
            &[hit("Solaris", 1972, "593"), hit("Solaris", 2002, "2103")],
        );
        let settings = MatchSettings::default();
        let out = resolve_match(&db, id, &list, &[&Fake], &settings)
            .await
            .unwrap();
        assert!(matches!(out, MatchResolution::Queued { .. }));
        (id, list)
    };

    let db = Database::open(&path).unwrap();
    let row = db
        .read(|c| review::pending_for_item(c, id))
        .unwrap()
        .unwrap();
    assert_eq!(row.reason, "2 strong candidates");
    assert_eq!(candidates_of(&row).unwrap().len(), 2);
    assert_eq!(row.best_score, Some(list[0].breakdown.score));
    assert_eq!(
        db.read(|c| get_flags(c, id)).unwrap().review_state,
        ReviewState::NeedsReview
    );
}

#[tokio::test]
async fn a_candidate_from_an_unavailable_provider_is_an_error() {
    let lab = Lab::new();
    let id = lab.movie("Solaris");
    let mut list = ranked("Solaris (1972)", &[hit("Solaris", 1972, "593")]);
    list[0].candidate.provider = "tvdb".to_owned();
    let err = resolve_match(&lab.db, id, &list, &[&Fake], &lab.settings)
        .await
        .unwrap_err();
    assert!(matches!(err, MatchError::NoProvider(p) if p == "tvdb"));
    // Nothing was queued or written.
    assert_eq!(lab.pending(), 0);
}

#[tokio::test]
async fn a_provider_failure_does_not_queue_the_item() {
    let lab = Lab::new();
    let id = lab.movie("Solaris");
    let list = ranked("Solaris (1972)", &[hit("Solaris", 1972, "999")]);
    let err = resolve_match(&lab.db, id, &list, &[&Fake], &lab.settings)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        MatchError::Provider(ProviderError::NotFound(_))
    ));
    assert_eq!(lab.pending(), 0);
    assert_eq!(lab.review_state(id), ReviewState::None);
}

#[test]
fn a_hit_becomes_a_candidate() {
    let mut h = hit("Solaris", 1972, "593");
    h.original_title = "Солярис".to_owned();
    h.exact_id = true;
    let c = candidate_from_hit(&h);
    assert_eq!(c.provider, "tmdb");
    assert!(c.has_poster && c.exact_id);
    assert_eq!(c.poster_url.as_deref(), Some("https://img/p.jpg"));
    assert_eq!(c.runtime_minutes, None);
    h.poster_url = Some(String::new());
    assert!(!candidate_from_hit(&h).has_poster);

    let mut details = Item::movie("Solaris");
    if let cv_domain::ItemDetail::Movie(m) = &mut details.detail {
        m.runtime_minutes = 167;
    }
    details.core.credits.push(cv_domain::Credit::new(
        cv_domain::CreditKind::Director,
        "Andrei Tarkovsky",
        "Director",
    ));
    let filled = with_details(c, &details);
    assert_eq!(filled.runtime_minutes, Some(167));
    // The director stays with the candidate, for the review inbox.
    assert_eq!(filled.director.as_deref(), Some("Andrei Tarkovsky"));
    let stored = serde_json::to_value(&filled).unwrap();
    let back: cv_match::Candidate = serde_json::from_value(stored).unwrap();
    assert_eq!(back.director.as_deref(), Some("Andrei Tarkovsky"));
}

// ---- the stored scrape attempt ------------------------------------------------

fn scrape_outcome(
    lab: &Lab,
    id: ItemId,
) -> Option<(cv_db::repo::step_outcome::StepStatus, String)> {
    lab.db
        .read(move |c| {
            cv_db::repo::step_outcome::step_outcome(c, id, cv_db::repo::step_outcome::STEP_SCRAPE)
        })
        .unwrap()
        .map(|o| (o.status, o.reason))
}

#[tokio::test]
async fn a_title_no_scrape_was_tried_for_has_no_scrape_outcome() {
    let lab = Lab::new();
    let id = lab.movie("Solaris");
    assert_eq!(scrape_outcome(&lab, id), None);
}

#[tokio::test]
async fn a_scrape_that_finds_no_confident_match_is_stored_with_its_reason() {
    use cv_db::repo::step_outcome::StepStatus;

    let lab = Lab::new();
    let nothing = lab.movie("Unknown Home Movie");
    lab.resolve(nothing, &[]).await;
    assert_eq!(
        scrape_outcome(&lab, nothing),
        Some((StepStatus::NotFound, "No candidates found".to_owned()))
    );

    let tie = lab.movie("Solaris");
    let list = ranked(
        "Solaris",
        &[hit("Solaris", 1972, "593"), hit("Solaris", 2002, "2103")],
    );
    lab.resolve(tie, &list).await;
    assert_eq!(
        scrape_outcome(&lab, tie),
        Some((StepStatus::NotFound, "2 strong candidates".to_owned()))
    );

    let keyless = lab.movie("Heat");
    queue_without_key(&lab.db, keyless, "TMDB").unwrap();
    assert_eq!(
        scrape_outcome(&lab, keyless),
        Some((StepStatus::NeedsKey, "No TMDB key".to_owned()))
    );
}

#[tokio::test]
async fn a_successful_scrape_replaces_the_miss_with_done() {
    use cv_db::repo::step_outcome::StepStatus;

    let lab = Lab::new();
    let id = lab.movie("Solaris");
    let tie = ranked(
        "Solaris",
        &[hit("Solaris", 1972, "593"), hit("Solaris", 2002, "2103")],
    );
    lab.resolve(id, &tie).await;
    accept_candidate(&lab.db, id, &tie[0], &[&Fake], &lab.settings)
        .await
        .unwrap();
    assert_eq!(
        scrape_outcome(&lab, id),
        Some((StepStatus::Done, String::new()))
    );

    // A confident automatic match stores done too.
    let auto = lab.movie("Solaris");
    let list = ranked("Solaris (1972)", &[hit("Solaris", 1972, "593")]);
    lab.resolve(auto, &list).await;
    assert_eq!(
        scrape_outcome(&lab, auto).map(|o| o.0),
        Some(StepStatus::Done)
    );
}

#[tokio::test]
async fn an_ignored_title_gets_no_scrape_outcome_from_a_later_run() {
    let lab = Lab::new();
    let id = lab.movie("Home Video");
    ignore_item(&lab.db, id).unwrap();
    assert!(matches!(
        lab.resolve(id, &[]).await,
        MatchResolution::Ignored
    ));
    assert_eq!(scrape_outcome(&lab, id), None);
}

//! The matching service: batches, the Needs review inbox and the search-and-scrape
//! panel, over a real library and a scripted provider. No network.

mod support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cv_app::app_settings::SettingsService;
use cv_app::match_service::{
    Choice, MatchService, MatchServiceError, ScrapeApply, TickKind, TickVerdict,
};
use cv_app::matching::batch::BatchMode;
use cv_app::notices::NoticeHub;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_db::repo::review::{self, ReviewQueueState};
use cv_db::repo::{get_flags, get_item, set_item_locked, update_item};
use cv_domain::{Item, ItemId, ItemKind, MediaFile, MediaFileType, ReviewState};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, ProviderRegistry, TmdbProvider,
};
use serde_json::json;
use support::{Entry, Lab, Scripted, arc};

fn catalogue() -> Scripted {
    Scripted::new(
        "scripted",
        vec![
            Entry::new("593", "Solaris", 1972).runtime(167),
            Entry::new("2103", "Solaris", 2002).runtime(99),
            Entry::new("693134", "Dune: Part Two", 2024),
            Entry::new("1", "Alien", 1979),
        ],
    )
}

struct Rig {
    lab: Lab,
    service: Arc<MatchService>,
    settings: Arc<SettingsService>,
    notices: Arc<NoticeHub>,
}

fn rig_with(registry: ProviderRegistry) -> Rig {
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    rig_with_resolver(registry, resolver)
}

fn rig_with_resolver(registry: ProviderRegistry, resolver: CredentialResolver) -> Rig {
    let lab = Lab::new();
    let providers = Arc::new(ProviderCredentialsService::with_resolver(
        resolver, registry,
    ));
    let settings = Arc::new(SettingsService::in_memory(""));
    let notices = Arc::new(NoticeHub::new());
    let service = MatchService::new(
        Arc::clone(&lab.db),
        lab.manager.clone(),
        providers,
        Arc::clone(&settings),
        Arc::clone(&notices),
    );
    Rig {
        lab,
        service,
        settings,
        notices,
    }
}

fn rig() -> Rig {
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(arc(catalogue()));
    rig_with(registry)
}

impl Rig {
    /// Matches `items` and waits for the batch.
    async fn match_and_wait(&self, items: &[ItemId]) {
        let batch = self
            .service
            .submit(BatchMode::MatchSelected, Some(items))
            .expect("batch");
        batch.wait().await;
    }

    fn pending_review_of(&self, item: ItemId) -> i64 {
        self.lab
            .db
            .read(|c| review::pending_for_item(c, item))
            .unwrap()
            .expect("a pending review row")
            .id
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sure_title_is_matched_and_an_unsure_one_lands_in_needs_review() {
    let rig = rig();
    let dune = rig.lab.movie("Dune: Part Two", 2024);
    let solaris = rig.lab.movie("Solaris", 0);
    rig.match_and_wait(&[dune, solaris]).await;

    assert_eq!(rig.lab.item(dune).core.last_scraper_id, "scripted");
    assert_eq!(rig.lab.item(dune).core.ids.get("tmdb"), Some("693134"));
    assert_eq!(rig.lab.item(solaris).core.last_scraper_id, "");

    assert_eq!(rig.service.pending_count().unwrap(), 1);
    let entries = rig.service.pending(100).unwrap();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.item, solaris);
    assert_eq!(entry.kind, ItemKind::Movie);
    // No year in the name: that is what would tell the two films apart.
    assert_eq!(
        entry.reason,
        "No year in the file name (2 strong candidates)"
    );
    assert!(entry.attention);
    assert!(entry.note.is_some(), "a tie explains itself");
    assert_eq!(entry.local.title, "Solaris");
    assert_eq!(entry.local.year, None);
    assert_eq!(entry.candidates.len(), 2);
    let ids: Vec<_> = entry
        .candidates
        .iter()
        .map(|c| (c.provider.as_str(), c.provider_id.as_str()))
        .collect();
    assert!(ids.contains(&("scripted", "593")) && ids.contains(&("scripted", "2103")));
    for candidate in &entry.candidates {
        assert!(candidate.confidence > 0.9);
        // The remote poster address travels with the candidate, so the app layer can cache it.
        assert_eq!(
            candidate.poster_url.as_deref(),
            Some(format!("https://img.invalid/{}.jpg", candidate.provider_id).as_str())
        );
        let year = candidate
            .evidence
            .iter()
            .find(|t| t.kind == TickKind::Year)
            .expect("a year tick");
        assert_eq!(year.verdict, TickVerdict::Unknown, "{}", year.text);
        assert!(candidate.evidence.iter().any(|t| t.kind == TickKind::Title));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_runtime_of_the_video_settles_a_tie() {
    let rig = rig();
    // A 167 minute video: the 1972 film, so it is matched without asking.
    let long = rig.lab.movie_with_video("Solaris", 0, 167, "ru");
    rig.match_and_wait(&[long]).await;
    assert_eq!(rig.lab.item(long).core.ids.get("tmdb"), Some("593"));
    assert_eq!(rig.service.pending_count().unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepting_a_candidate_scrapes_the_item_and_undo_puts_it_back() {
    let rig = rig();
    let solaris = rig.lab.movie("Solaris", 0);
    rig.match_and_wait(&[solaris]).await;
    let row = rig.pending_review_of(solaris);

    let accepted = rig
        .service
        .accept(
            row,
            &Choice {
                provider: "scripted".to_owned(),
                provider_id: "2103".to_owned(),
            },
        )
        .await
        .expect("accept");
    assert!(accepted.scrape.blocked.is_none());
    let item = rig.lab.item(solaris);
    assert_eq!(item.core.ids.get("tmdb"), Some("2103"));
    assert_eq!(item.core.year, 2002);
    assert_eq!(item.core.last_scraper_id, "scripted");
    assert_eq!(rig.service.pending_count().unwrap(), 0);
    let flags = rig.lab.db.read(|c| get_flags(c, solaris)).unwrap();
    assert_ne!(flags.review_state, ReviewState::NeedsReview);

    // The row is closed; accepting it again is refused.
    let again = rig
        .service
        .accept(
            row,
            &Choice {
                provider: "scripted".to_owned(),
                provider_id: "2103".to_owned(),
            },
        )
        .await
        .expect_err("closed");
    assert_eq!(again.code(), "not_pending");

    // Undo: the row is back, and so is the item as it was before the scrape.
    rig.service.reopen(&[row]).expect("reopen");
    assert_eq!(rig.service.pending_count().unwrap(), 1);
    let undone = rig.lab.item(solaris);
    assert_eq!(undone.core.ids.get("tmdb"), None, "the accepted ID is gone");
    assert_eq!(undone.core.year, 0, "the scraped year is gone");
    assert_eq!(undone.core.last_scraper_id, "");
    assert_eq!(undone.core.title, "Solaris");
    let flags = rig.lab.db.read(|c| get_flags(c, solaris)).unwrap();
    assert_eq!(flags.review_state, ReviewState::NeedsReview);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undoing_an_accept_keeps_the_files_the_item_gained_meanwhile() {
    let rig = rig();
    let solaris = rig.lab.movie("Solaris", 0);
    rig.match_and_wait(&[solaris]).await;
    let row = rig.pending_review_of(solaris);
    rig.service
        .accept(
            row,
            &Choice {
                provider: "scripted".to_owned(),
                provider_id: "2103".to_owned(),
            },
        )
        .await
        .expect("accept");
    // A file the after-match steps wrote after the scrape.
    let mut item = rig.lab.item(solaris);
    item.core.media_files.push(cv_domain::MediaFile::new(
        cv_domain::MediaFileType::Poster,
        "C:/Movies/Solaris",
        "poster.jpg",
    ));
    rig.lab
        .db
        .write(move |tx| cv_db::repo::update_item(tx, &item))
        .unwrap();

    rig.service.reopen(&[row]).expect("reopen");

    let undone = rig.lab.item(solaris);
    assert_eq!(undone.core.ids.get("tmdb"), None);
    assert!(
        undone
            .core
            .media_files
            .iter()
            .any(|f| f.kind == cv_domain::MediaFileType::Poster),
        "the poster file is still listed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_search_result_that_was_not_queued_can_be_accepted() {
    let rig = rig();
    let odd = rig.lab.movie("Solaris", 0);
    rig.match_and_wait(&[odd]).await;
    let row = rig.pending_review_of(odd);

    // By title, and by an ID shortcut.
    let by_title = rig.service.search_review(row, "alien").await.unwrap();
    assert_eq!(by_title.len(), 1);
    assert_eq!(by_title[0].title, "Alien");
    assert_eq!(by_title[0].provider_id, "1");
    let by_id = rig.service.search_review(row, "tt0693134").await.unwrap();
    assert_eq!(by_id.len(), 1);
    assert_eq!(by_id[0].title, "Dune: Part Two");
    assert!(by_id[0].confidence > 0.99, "an ID lookup is an exact match");
    let nothing = rig.service.search_review(row, "   ").await.unwrap_err();
    assert_eq!(nothing.code(), "invalid");

    rig.service
        .accept(
            row,
            &Choice {
                provider: by_id[0].provider.clone(),
                provider_id: by_id[0].provider_id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(rig.lab.item(odd).core.title, "Dune: Part Two");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bulk_accept_takes_the_top_candidate_of_every_row() {
    let rig = rig();
    let one = rig.lab.movie("Solaris", 0);
    let two = rig.lab.movie("Solaris", 0);
    let three = rig.lab.movie("Solaris", 0);
    rig.match_and_wait(&[one, two, three]).await;
    let rows: Vec<i64> = [one, two, three]
        .iter()
        .map(|item| rig.pending_review_of(*item))
        .collect();
    // Without a year the two Solaris films tie. For two of the titles the second one scores
    // lower, as a runtime would make it.
    for row in &rows[..2] {
        let row = *row;
        rig.lab
            .db
            .write(move |tx| {
                tx.execute(
                    "UPDATE review_queue SET reason = 'Best match 80% is below 85%', \
                     candidates = json_set(candidates, '$[1].breakdown.rawScore', 0.5) \
                     WHERE id = ?1",
                    [row],
                )?;
                Ok(())
            })
            .unwrap();
    }
    let tied = rig
        .service
        .pending(10)
        .unwrap()
        .into_iter()
        .find(|entry| entry.review_id == rows[2])
        .unwrap();
    assert!(tied.tied, "{tied:?}");

    // Accept all takes the clear best candidates and leaves the tie for a person.
    let mut accepted = rig.service.accept_top(&rows).await.unwrap();
    accepted.sort_unstable();
    let mut clear = rows[..2].to_vec();
    clear.sort_unstable();
    assert_eq!(accepted, clear);
    for item in [one, two] {
        assert_eq!(rig.lab.item(item).core.last_scraper_id, "scripted");
    }
    assert_eq!(rig.lab.item(three).core.last_scraper_id, "");
    assert_eq!(rig.service.pending_count().unwrap(), 1);
    // Undo of both: the rows and the items.
    rig.service.reopen(&accepted).unwrap();
    assert_eq!(rig.service.pending_count().unwrap(), 3);
    for item in [one, two] {
        assert_eq!(rig.lab.item(item).core.last_scraper_id, "");
        assert_eq!(rig.lab.item(item).core.ids.get("tmdb"), None);
    }
}

/// The Undo of a Change match puts the step outcomes back too, so the Progress lines
/// do not keep "collection not found" from the undone scrape.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undoing_a_scrape_puts_the_step_outcomes_back() {
    use cv_db::repo::step_outcome::{StepStatus, record_step_outcome, step_outcomes};
    let rig = rig();
    let heat = rig.lab.movie("Heat", 1995);
    rig.lab
        .db
        .write(move |tx| record_step_outcome(tx, heat, "collections", StepStatus::Done, "", 1))
        .unwrap();
    let token = rig.service.keep_scrape_snapshot(heat).expect("a snapshot");
    // The scrape of another title records what it found.
    rig.lab
        .db
        .write(move |tx| {
            record_step_outcome(
                tx,
                heat,
                "collections",
                StepStatus::NotFound,
                "collection not found",
                2,
            )?;
            record_step_outcome(tx, heat, "trailers", StepStatus::Failed, "offline", 2)
        })
        .unwrap();

    rig.service.undo_scrape(token).unwrap();
    let outcomes = rig.lab.db.read(move |c| step_outcomes(c, heat)).unwrap();
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert_eq!(outcomes[0].step, "collections");
    assert_eq!(outcomes[0].status, StepStatus::Done);
}

/// A batch scrape stores that it was tried and found no confident match, an accept
/// replaces that with done, and the Undo of the accept puts the miss back with the review row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_scrape_attempt_is_stored_cleared_by_an_accept_and_restored_by_its_undo() {
    use cv_db::repo::step_outcome::{STEP_SCRAPE, StepStatus, step_outcome};
    let rig = rig();
    let fresh = rig.lab.movie("Solaris", 0);
    let scrape_of = |item: ItemId| {
        rig.lab
            .db
            .read(move |c| step_outcome(c, item, STEP_SCRAPE))
            .unwrap()
            .map(|o| (o.status, o.reason))
    };
    // A scan alone tries no scrape.
    assert_eq!(scrape_of(fresh), None);

    // Without a year the two Solaris films tie: tried, no confident match.
    rig.match_and_wait(&[fresh]).await;
    let row = rig.pending_review_of(fresh);
    let missed = scrape_of(fresh).expect("the scrape was stored");
    assert_eq!(missed.0, StepStatus::NotFound);
    assert!(!missed.1.is_empty(), "the review reason is kept");

    rig.service
        .accept(
            row,
            &Choice {
                provider: "scripted".to_owned(),
                provider_id: "2103".to_owned(),
            },
        )
        .await
        .expect("accept");
    assert_eq!(scrape_of(fresh).map(|o| o.0), Some(StepStatus::Done));

    rig.service.reopen(&[row]).unwrap();
    assert_eq!(scrape_of(fresh), Some(missed));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ignoring_an_item_keeps_it_out_of_later_scans_until_undone() {
    let rig = rig();
    let home = rig.lab.movie("Solaris", 0);
    rig.match_and_wait(&[home]).await;
    let row = rig.pending_review_of(home);

    rig.service.ignore(row).unwrap();
    assert_eq!(rig.service.pending_count().unwrap(), 0);
    // A new batch leaves it alone.
    rig.match_and_wait(&[home]).await;
    assert_eq!(rig.service.pending_count().unwrap(), 0);
    let state = rig
        .lab
        .db
        .read(|c| review::get_review(c, row))
        .unwrap()
        .unwrap()
        .state;
    assert_eq!(state, ReviewQueueState::Dismissed);

    rig.service.reopen(&[row]).unwrap();
    assert_eq!(rig.service.pending_count().unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_locked_item_is_not_changed_and_stays_in_the_queue() {
    let rig = rig();
    let item = rig.lab.movie("Solaris", 0);
    rig.match_and_wait(&[item]).await;
    let row = rig.pending_review_of(item);
    rig.lab
        .db
        .write(move |tx| set_item_locked(tx, &[item], true))
        .unwrap();

    let error = rig
        .service
        .accept(
            row,
            &Choice {
                provider: "scripted".to_owned(),
                provider_id: "593".to_owned(),
            },
        )
        .await
        .expect_err("locked");
    assert!(matches!(error, MatchServiceError::Locked));
    assert_eq!(rig.lab.item(item).core.last_scraper_id, "");
    assert_eq!(rig.service.pending_count().unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_match_selection_action_queues_the_titles_and_counts_them() {
    let rig = rig();
    let ids: Vec<ItemId> = (0..3).map(|_| rig.lab.movie("Alien", 1979)).collect();
    let queued = rig.service.match_items(&ids).unwrap();
    assert_eq!(queued, 3);
    wait_until(|| {
        ids.iter()
            .all(|id| rig.lab.item(*id).core.last_scraper_id == "scripted")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_threshold_setting_decides_what_is_accepted() {
    let rig = rig();
    // "Alien" with a year 20 off scores about 97 %; at a threshold of 99 % it must wait.
    rig.settings
        .set("matching.auto-accept-threshold", json!(99))
        .unwrap();
    let item = rig.lab.movie("Alien", 1999);
    rig.match_and_wait(&[item]).await;
    assert_eq!(rig.lab.item(item).core.last_scraper_id, "");
    let entry = &rig.service.pending(10).unwrap()[0];
    assert!(entry.reason.contains("below 99%"), "{}", entry.reason);

    rig.settings
        .set("matching.auto-accept-threshold", json!(50))
        .unwrap();
    rig.match_and_wait(&[item]).await;
    assert_eq!(rig.lab.item(item).core.last_scraper_id, "scripted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_tmdb_key_online_identification_is_paused() {
    let http = HttpClient::builder().build().unwrap();
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(Arc::new(TmdbProvider::new(http, resolver)));
    let rig = rig_with(registry);

    assert!(!rig.service.has_providers());
    assert!(rig.service.identification_paused());
    // The title is in the library (its local data loads) but matching does not start.
    let item = rig.lab.movie("Alien", 1979);
    let error = rig.service.match_items(&[item]).unwrap_err();
    assert_eq!(error.code(), "identification_paused");
    assert!(error.to_string().contains("TMDB key"), "{error}");
    assert_eq!(rig.lab.item(item).core.title, "Alien");
    assert_eq!(rig.lab.item(item).core.last_scraper_id, "");
    assert!(rig.service.resume().unwrap().is_empty());
    assert!(rig.service.scrape_providers(ItemKind::Movie).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_ready_provider_does_not_lift_the_pause_but_a_tmdb_key_does() {
    let http = HttpClient::builder().build().unwrap();
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(Arc::new(TmdbProvider::new(http, resolver.clone())));
    registry.register_metadata(arc(Scripted::new(
        "scripted",
        vec![Entry::new("1", "Alien", 1979)],
    )));
    let rig = rig_with_resolver(registry, resolver.clone());
    let item = rig.lab.movie("Alien", 1979);

    assert!(
        rig.service.has_providers(),
        "the scripted provider is ready"
    );
    assert_eq!(
        rig.service.match_items(&[item]).unwrap_err().code(),
        "identification_paused"
    );

    resolver.set_user("tmdb", "api_key", "a-user-key").unwrap();
    assert!(!rig.service.identification_paused());
    assert!(rig.service.match_items(&[item]).is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saving_the_tmdb_key_lifts_the_pause_but_scrapes_nothing_by_itself() {
    let http = HttpClient::builder().build().unwrap();
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(Arc::new(TmdbProvider::new(http, resolver.clone())));
    registry.register_metadata(arc(Scripted::new(
        "scripted",
        vec![Entry::new("1", "Alien", 1979)],
    )));
    let rig = rig_with_resolver(registry, resolver.clone());
    rig.lab.movie("Alien", 1979);
    rig.lab.movie("Solaris", 1972);

    let heard = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&heard);
    rig.notices.subscribe(move |notice| {
        sink.lock().unwrap().push(notice.title.clone());
        true
    });

    // Still paused after a save that did not add the TMDB key.
    let was_paused = rig.service.identification_paused();
    assert!(was_paused);
    assert!(!rig.service.key_saved(was_paused));

    resolver.set_user("tmdb", "api_key", "a-user-key").unwrap();
    // A save that did not lift a pause changes nothing either.
    assert!(!rig.service.key_saved(false));
    assert!(rig.service.key_saved(was_paused));

    // Scrape is manual: no batch was queued for the waiting titles and nothing was announced.
    let jobs = rig.lab.manager.list();
    assert!(jobs.is_empty(), "a scrape was queued by itself: {jobs:?}");
    assert!(
        heard.lock().unwrap().is_empty(),
        "{:?}",
        heard.lock().unwrap()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_scrape_panel_searches_previews_and_writes_only_the_chosen_fields() {
    let rig = rig();
    let linked = rig.lab.movie("Solaris", 2002);
    rig.match_and_wait(&[linked]).await; // not sure: stays in the queue
    let target = rig.lab.movie("Solaris", 1972);
    let mut stored = rig.lab.item(target);
    stored.core.plot = "My own plot".to_owned();
    let stored_id = stored.id.unwrap();
    rig.lab
        .db
        .write(move |tx| cv_db::repo::update_item(tx, &stored))
        .unwrap();

    let providers = rig.service.scrape_providers(ItemKind::Movie);
    // The universal provider is offered after the real ones.
    let keys: Vec<_> = providers.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["scripted", "universal"]);

    // Another item is already linked to the 2002 film.
    let mut other = rig.lab.item(linked);
    other.core.ids.set("tmdb", "2103");
    rig.lab
        .db
        .write(move |tx| cv_db::repo::update_item(tx, &other))
        .unwrap();

    let hits = rig
        .service
        .scrape_search(stored_id, "scripted", "en", "solaris")
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|h| h.candidate.poster_url.is_some()));
    let marked: Vec<_> = hits
        .iter()
        .map(|h| (h.candidate.year, h.in_library.as_ref().map(|l| l.item)))
        .collect();
    assert!(marked.contains(&(Some(2002), Some(linked))), "{marked:?}");
    assert!(marked.contains(&(Some(1972), None)), "{marked:?}");

    let preview = rig
        .service
        .scrape_preview(ItemKind::Movie, "scripted", "593", "en")
        .await
        .unwrap();
    assert_eq!(preview.title, "Solaris");
    assert_eq!(preview.year, Some(1972));
    assert_eq!(preview.runtime_minutes, Some(167));
    assert!(preview.ids.contains(&"tmdb:593".to_owned()));
    assert_eq!(
        rig.lab.item(target).core.plot,
        "My own plot",
        "nothing written yet"
    );

    // Plot and runtime selected, and a plot that is already there is kept by do-not-overwrite.
    rig.service
        .scrape_apply(
            stored_id,
            &ScrapeApply {
                provider: "scripted".to_owned(),
                provider_id: "593".to_owned(),
                language: "en".to_owned(),
                fields: vec!["plot".to_owned(), "runtime".to_owned()],
                do_not_overwrite: true,
                second_version: false,
            },
        )
        .await
        .unwrap();
    let after = rig.lab.item(target);
    assert_eq!(after.core.plot, "My own plot");
    assert_eq!(
        after.core.ids.get("tmdb"),
        Some("593"),
        "linked to the title"
    );
    assert_eq!(after.core.last_scraper_id, "scripted");

    // Overwriting with the plot selected, as a second version of the 2002 film.
    rig.service
        .scrape_apply(
            stored_id,
            &ScrapeApply {
                provider: "scripted".to_owned(),
                provider_id: "2103".to_owned(),
                language: "en".to_owned(),
                fields: vec!["plot".to_owned()],
                do_not_overwrite: false,
                second_version: true,
            },
        )
        .await
        .unwrap();
    let relinked = rig.lab.item(target);
    assert_eq!(relinked.core.plot, "Plot of Solaris (2002)");
    assert_eq!(
        relinked.core.ids.get("tmdb"),
        Some("2103"),
        "a different title replaces the old link"
    );
    assert_eq!(relinked.core.ids.get("imdb"), Some("tt0002103"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_change_of_match_leaves_nothing_of_the_old_title_behind() {
    let rig = rig();
    let item = rig.lab.movie("Solaris", 1972);
    let apply = |provider_id: &str| ScrapeApply {
        provider: "scripted".to_owned(),
        provider_id: provider_id.to_owned(),
        language: "en".to_owned(),
        fields: vec![
            "plot".to_owned(),
            "rating".to_owned(),
            "collection".to_owned(),
            "clearlogo".to_owned(),
        ],
        do_not_overwrite: false,
        second_version: false,
    };
    rig.service.scrape_apply(item, &apply("593")).await.unwrap();
    // What the 1972 film got from elsewhere: ratings, its logo, its collection, the user's rating.
    rig.lab
        .db
        .write(move |tx| {
            let mut stored = get_item(tx, item)?.unwrap();
            stored.core.ratings.insert(cv_domain::Rating::new(
                "tomatometerallcritics",
                84.0,
                0,
                100,
            ));
            stored.core.artwork_urls.insert(
                cv_domain::MediaFileType::Clearlogo,
                "https://img.test/solaris-logo.png".to_owned(),
            );
            stored.core.ids.set("tmdbSet", "1048282");
            update_item(tx, &stored)?;
            cv_db::repo::set_user_rating(tx, item, Some(7.0))
        })
        .unwrap();

    // Change match to the 2002 film.
    rig.service
        .scrape_apply(item, &apply("2103"))
        .await
        .unwrap();

    let after = rig.lab.item(item);
    assert_eq!(after.core.ids.get("tmdb"), Some("2103"));
    assert_eq!(after.core.plot, "Plot of Solaris (2002)");
    assert_eq!(after.core.ratings.get("tomatometerallcritics"), None);
    assert_eq!(
        after.core.ratings.user_rating(),
        Some(7.0),
        "the user's own rating stays"
    );
    assert_eq!(
        after
            .core
            .artwork_urls
            .get(&cv_domain::MediaFileType::Clearlogo),
        None
    );
    assert_eq!(after.core.ids.get("tmdbSet"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_locked_item_is_refused_by_the_scrape_panel() {
    let rig = rig();
    let item = rig.lab.movie("Alien", 1979);
    rig.lab
        .db
        .write(move |tx| set_item_locked(tx, &[item], true))
        .unwrap();
    let error = rig
        .service
        .scrape_apply(
            item,
            &ScrapeApply {
                provider: "scripted".to_owned(),
                provider_id: "1".to_owned(),
                language: String::new(),
                fields: vec!["plot".to_owned()],
                do_not_overwrite: false,
                second_version: false,
            },
        )
        .await
        .expect_err("locked");
    assert_eq!(error.code(), "locked");
    assert_eq!(rig.lab.item(item).core.plot, "");
}

/// Review 14.7: a change of match that is refused (a locked title) changes nothing. The provider
/// IDs and the old title's fields were emptied in transactions of their own before the write
/// was refused, and the Undo copy is dropped on an error, so a locked title lost its IDs for
/// good.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_change_of_match_of_a_locked_title_changes_nothing() {
    let rig = rig();
    let item = rig.lab.movie("Solaris", 1972);
    let apply = |provider_id: &str| ScrapeApply {
        provider: "scripted".to_owned(),
        provider_id: provider_id.to_owned(),
        language: "en".to_owned(),
        fields: vec!["plot".to_owned(), "rating".to_owned()],
        do_not_overwrite: false,
        second_version: false,
    };
    rig.service.scrape_apply(item, &apply("593")).await.unwrap();
    rig.lab
        .db
        .write(move |tx| set_item_locked(tx, &[item], true))
        .unwrap();
    let before = rig.lab.item(item);
    assert_eq!(before.core.ids.get("tmdb"), Some("593"));

    let error = rig
        .service
        .scrape_apply(item, &apply("2103"))
        .await
        .expect_err("locked");

    assert_eq!(error.code(), "locked");
    let after = rig.lab.item(item);
    assert_eq!(after.core.ids, before.core.ids, "the IDs stay");
    assert_eq!(after.core.plot, before.core.plot);
    assert_eq!(after.core.ratings, before.core.ratings);
}

/// Polls until `done`, for work that runs in the background.
async fn wait_until(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(
            Instant::now() < deadline,
            "the background work did not finish"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ---- the Scrape page: search only, current match, Accept all confident --------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_search_only_pass_scores_a_new_title_and_writes_nothing() {
    let rig = rig();
    let dune = rig.lab.movie("Dune: Part Two", 2024);
    let before = rig.lab.item(dune);

    let found = rig.service.search_only(dune, "", "").await.expect("search");
    assert!(found.confident, "{found:?}");
    assert_eq!(found.candidates[0].provider_id, "693134");
    assert!(found.candidates[0].confidence >= 0.9);

    // Nothing was written: not the title, not its IDs, not a review row, not a scrape record.
    let after = rig.lab.item(dune);
    assert_eq!(after, before);
    assert_eq!(after.core.ids.get("tmdb"), None);
    assert_eq!(rig.service.pending_count().unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_equal_candidates_are_not_confident() {
    let rig = rig();
    let solaris = rig.lab.movie("Solaris", 0);
    let found = rig
        .service
        .search_only(solaris, "", "")
        .await
        .expect("search");
    assert_eq!(found.candidates.len(), 2);
    assert!(!found.confident, "a tie needs a person: {found:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_all_confident_takes_only_the_confident_titles_and_can_be_undone() {
    let rig = rig();
    let dune = rig.lab.movie("Dune: Part Two", 2024);
    let solaris = rig.lab.movie("Solaris", 0);
    // Nothing is scraped until the user asks, even after the search-only pass.
    rig.service.search_only(dune, "", "").await.unwrap();
    rig.service.search_only(solaris, "", "").await.unwrap();
    assert_eq!(rig.lab.item(dune).core.last_scraper_id, "");

    let done = rig
        .service
        .accept_confident(&[dune, solaris])
        .await
        .expect("accept");
    assert_eq!(done.accepted.len(), 1);
    assert_eq!(done.accepted[0].item, dune);
    assert_eq!(done.skipped, vec![solaris]);
    assert!(done.failed.is_empty());
    assert_eq!(rig.lab.item(dune).core.ids.get("tmdb"), Some("693134"));
    assert_eq!(rig.lab.item(solaris).core.last_scraper_id, "");

    let token = done.accepted[0].undo.expect("an undo token");
    rig.service.undo_scrape(token).expect("undo");
    assert_eq!(rig.lab.item(dune).core.ids.get("tmdb"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_all_confident_searches_a_title_that_was_not_searched_yet() {
    let rig = rig();
    let dune = rig.lab.movie("Dune: Part Two", 2024);
    let done = rig.service.accept_confident(&[dune]).await.expect("accept");
    assert_eq!(done.accepted.len(), 1);
    assert_eq!(rig.lab.item(dune).core.last_scraper_id, "scripted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_current_match_comes_back_scored_like_the_other_candidates() {
    let rig = rig();
    let dune = rig.lab.movie("Dune: Part Two", 2024);
    assert!(
        rig.service.current_match(dune, "").await.unwrap().is_none(),
        "a title never scraped has no current match"
    );
    rig.service.accept_confident(&[dune]).await.unwrap();
    let current = rig
        .service
        .current_match(dune, "")
        .await
        .unwrap()
        .expect("the linked title");
    assert_eq!(current.provider_id, "693134");
    assert!(current.confidence > 0.9);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_all_confident_on_a_review_row_is_undone_by_reopening_it() {
    let rig = rig();
    let solaris = rig.lab.movie("Solaris", 0);
    rig.match_and_wait(&[solaris]).await;
    let row = rig.pending_review_of(solaris);
    // The two Solaris films tie, so the title is not confident and stays in the queue.
    let done = rig.service.accept_confident(&[solaris]).await.unwrap();
    assert_eq!(done.skipped, vec![solaris]);
    assert_eq!(rig.service.pending_count().unwrap(), 1);

    // The runtime of the video settles the tie: now the top candidate is clear.
    rig.lab
        .db
        .write(move |tx| {
            tx.execute(
                "UPDATE review_queue SET candidates = json_set(candidates, '$[1].breakdown.rawScore', 0.5, \
                 '$[1].breakdown.score', 0.5, '$[0].breakdown.rawScore', 0.97, '$[0].breakdown.score', 0.97) \
                 WHERE id = ?1",
                [row],
            )?;
            Ok(())
        })
        .unwrap();
    let done = rig.service.accept_confident(&[solaris]).await.unwrap();
    assert_eq!(done.accepted.len(), 1, "{done:?}");
    assert_eq!(done.accepted[0].review_id, Some(row));
    assert_eq!(rig.service.pending_count().unwrap(), 0);
    rig.service.reopen(&[row]).unwrap();
    assert_eq!(rig.service.pending_count().unwrap(), 1);
    assert_eq!(rig.lab.item(solaris).core.last_scraper_id, "");
}

/// A title in a folder with its NFO and the eight kinds of artwork, each file saying what it is.
fn solaris_with_files(
    rig: &Rig,
) -> (
    ItemId,
    std::path::PathBuf,
    Vec<(&'static str, MediaFileType)>,
) {
    let source = rig.lab.dir.path().join("Movies");
    let folder = source.join("Solaris (1972)");
    std::fs::create_dir_all(&folder).unwrap();
    let files = vec![
        ("solaris.nfo", MediaFileType::Nfo),
        ("poster.jpg", MediaFileType::Poster),
        ("fanart.jpg", MediaFileType::Fanart),
        ("banner.jpg", MediaFileType::Banner),
        ("clearart.png", MediaFileType::Clearart),
        ("clearlogo.png", MediaFileType::Clearlogo),
        ("disc.png", MediaFileType::Disc),
        ("thumb.jpg", MediaFileType::Thumb),
        ("keyart.jpg", MediaFileType::Keyart),
    ];
    let mut item = Item::movie("Solaris");
    item.core.year = 1972;
    item.core.path = folder.to_string_lossy().into_owned();
    item.core.data_source = source.to_string_lossy().into_owned();
    item.core.ids.set("tmdb", "593");
    for (name, kind) in &files {
        std::fs::write(folder.join(name), format!("old {name}")).unwrap();
        item.core
            .media_files
            .push(MediaFile::new(*kind, folder.to_string_lossy(), *name));
    }
    (rig.lab.add(item), folder, files)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_change_of_match_is_a_history_entry_whose_undo_brings_back_fields_nfo_and_artwork() {
    use cv_app::history::{HistoryFilter, HistoryService, UndoState};

    let rig = rig();
    let (item, folder, files) = solaris_with_files(&rig);
    let apply = |provider_id: &str| ScrapeApply {
        provider: "scripted".to_owned(),
        provider_id: provider_id.to_owned(),
        language: "en".to_owned(),
        fields: vec!["plot".to_owned()],
        do_not_overwrite: false,
        second_version: false,
    };

    let (_, entry) = rig
        .service
        .scrape_apply_recorded(item, &apply("2103"))
        .await
        .unwrap();
    let entry = entry.expect("a different match is a History entry");
    assert_eq!(rig.lab.item(item).core.ids.get("tmdb"), Some("2103"));
    for (name, _) in &files {
        assert!(
            !folder.join(name).exists(),
            "{name} left the folder before the new match was written"
        );
    }
    // What the follow-up of the new match writes at the same names.
    std::fs::write(folder.join("poster.jpg"), "new poster").unwrap();
    std::fs::write(folder.join("solaris.nfo"), "new nfo").unwrap();

    let history = HistoryService::new(Arc::clone(&rig.lab.db));
    let listed = history
        .list(HistoryFilter::ArtworkAndMatches, 0, 10)
        .unwrap();
    assert_eq!(listed.entries.len(), 1);
    assert_eq!(listed.entries[0].id, entry);
    assert!(listed.entries[0].summary.starts_with("Changed match"));

    let report = rig.service.undo_entry(entry, None).await.unwrap();
    assert_eq!(report.undone, vec![item], "{:?}", report.left);
    assert_eq!(report.state, UndoState::Undone);

    let back = rig.lab.item(item);
    assert_eq!(back.core.ids.get("tmdb"), Some("593"));
    for (name, kind) in &files {
        assert_eq!(
            std::fs::read_to_string(folder.join(name)).unwrap(),
            format!("old {name}"),
            "{name} is the earlier file again"
        );
        assert!(
            back.core
                .media_files
                .iter()
                .any(|f| f.kind == *kind && f.filename == *name),
            "{name} is listed again"
        );
    }
    // The files of the new match were not deleted: they wait in the trash folder.
    let trash = rig
        .lab
        .dir
        .path()
        .join("Movies")
        .join(".deletedByCineVault");
    let mut kept = Vec::new();
    let mut stack = vec![trash];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(text) = std::fs::read_to_string(&path) {
                kept.push(text);
            }
        }
    }
    assert!(kept.contains(&"new poster".to_owned()), "{kept:?}");
    assert!(kept.contains(&"new nfo".to_owned()), "{kept:?}");
    // Undone once.
    assert!(rig.service.undo_entry(entry, None).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undo_of_a_change_of_match_bins_the_artwork_the_new_match_added_at_new_names() {
    let rig = rig();
    let (item, folder, files) = solaris_with_files(&rig);
    let apply = ScrapeApply {
        provider: "scripted".to_owned(),
        provider_id: "2103".to_owned(),
        language: "en".to_owned(),
        fields: vec!["plot".to_owned()],
        do_not_overwrite: false,
        second_version: false,
    };
    let (_, entry) = rig
        .service
        .scrape_apply_recorded(item, &apply)
        .await
        .unwrap();
    let entry = entry.expect("a History entry");
    // The follow-up of the new match fills an artwork type the earlier match never had.
    std::fs::write(folder.join("characterart.png"), "new character art").unwrap();
    let dir = folder.to_string_lossy().into_owned();
    rig.lab
        .db
        .write(move |tx| {
            let mut now = cv_db::repo::get_item(tx, item)?.unwrap();
            now.core.media_files.push(MediaFile::new(
                MediaFileType::Characterart,
                dir,
                "characterart.png",
            ));
            cv_db::repo::update_item(tx, &now)
        })
        .unwrap();

    let report = rig.service.undo_entry(entry, None).await.unwrap();
    assert_eq!(report.undone, vec![item], "{:?}", report.left);
    assert!(
        !folder.join("characterart.png").exists(),
        "the artwork of the undone match left the folder"
    );
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut expected: Vec<String> = files.iter().map(|(n, _)| (*n).to_owned()).collect();
    expected.sort();
    assert_eq!(names, expected, "the folder is as it was before the match");
    assert!(
        !rig.lab
            .item(item)
            .core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::Characterart)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scraping_a_title_never_scraped_or_with_the_same_match_is_not_a_history_entry() {
    let rig = rig();
    let (item, folder, _) = solaris_with_files(&rig);
    let apply = |provider_id: &str| ScrapeApply {
        provider: "scripted".to_owned(),
        provider_id: provider_id.to_owned(),
        language: "en".to_owned(),
        fields: vec!["plot".to_owned()],
        do_not_overwrite: false,
        second_version: false,
    };
    let (_, same) = rig
        .service
        .scrape_apply_recorded(item, &apply("593"))
        .await
        .unwrap();
    assert!(same.is_none());
    assert!(
        folder.join("poster.jpg").exists(),
        "the files stay where they are"
    );
    let fresh = rig.lab.movie("Alien", 1979);
    let (_, none) = rig
        .service
        .scrape_apply_recorded(fresh, &apply("1"))
        .await
        .unwrap();
    assert!(none.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_locked_title_keeps_its_files_when_a_change_of_match_is_refused() {
    let rig = rig();
    let (item, folder, _) = solaris_with_files(&rig);
    rig.lab
        .db
        .write(move |tx| set_item_locked(tx, &[item], true))
        .unwrap();
    let refused = rig
        .service
        .scrape_apply_recorded(
            item,
            &ScrapeApply {
                provider: "scripted".to_owned(),
                provider_id: "2103".to_owned(),
                language: "en".to_owned(),
                fields: vec!["plot".to_owned()],
                do_not_overwrite: false,
                second_version: false,
            },
        )
        .await;
    assert!(matches!(refused, Err(MatchServiceError::Locked)));
    assert_eq!(
        std::fs::read_to_string(folder.join("poster.jpg")).unwrap(),
        "old poster.jpg"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_all_confident_is_one_history_entry_that_undoes_every_title() {
    use cv_app::history::UndoState;

    let rig = rig();
    let dune = rig.lab.movie("Dune: Part Two", 2024);
    let alien = rig.lab.movie("Alien", 1979);
    let done = rig
        .service
        .accept_confident(&[dune, alien])
        .await
        .expect("accept");
    assert_eq!(done.accepted.len(), 2, "{done:?}");
    let entry = done.history_entry.expect("one entry for the whole run");
    assert_eq!(rig.lab.item(dune).core.ids.get("tmdb"), Some("693134"));

    let report = rig.service.undo_entry(entry, None).await.unwrap();
    assert_eq!(report.undone.len(), 2, "{:?}", report.left);
    assert_eq!(report.state, UndoState::Undone);
    assert_eq!(rig.lab.item(dune).core.ids.get("tmdb"), None);
    assert_eq!(rig.lab.item(alien).core.ids.get("tmdb"), None);
}

//! What follows a scrape from any metadata provider: the NFO and the artwork.
//!
//! A scrape from TMDB, OFDb, TVmaze or any other provider ends in the same steps, in this order:
//!
//! 1. the provider IDs are completed (done by the scrape itself, before this runs);
//! 2. the NFO is written by the NFO settings (`metadata.nfo-format`, ...),
//!    next to the video, so the title is readable by Kodi, Jellyfin and Emby at once;
//! 3. the empty artwork types are filled from the enabled artwork providers by the IDs the title
//!    has now (any ID an artwork provider understands, such as an IMDb ID, is enough). When no
//!    artwork provider knows the title, the artwork step outcome says "No artwork provider knows
//!    this title." and Activity shows it as a notice naming the title, as it does a failed
//!    artwork step and an NFO that could not be written.
//!
//! The steps run in the background and never delay or fail the scrape. The returned task ends
//! when both are over, so the rename step, which moves the folder, can wait for it. A locked
//! title is never scraped, so it never gets here.
//!
//! # The artwork job
//!
//! Once the app has given it its job manager ([`ScrapeFollowUp::set_jobs`]), the artwork step of
//! the titles a scrape writes within a short window is one background job, named in Activity as
//! "Downloading artwork for 8 titles" (spec `job-system`), with its progress, the title being
//! fetched, pause and cancel. Each title's task waits for its own item of that job, so the pin
//! and the order of the steps are as before: the NFO first, then the artwork, then (when renaming
//! is automatic) the rename. A cancelled job leaves the titles it had not started without new
//! artwork and lets their tasks go on; nothing waits forever on a job that is over. A title the
//! job had started finishes its run (in a task of its own that holds the title), so no rename
//! or move of it starts while one of its images is still being written.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use cv_db::repo::get_item;
use cv_db::repo::step_outcome::step_outcome;
use cv_domain::ItemId;
use cv_jobs::{ItemControl, ItemError, ItemWork, JobManager, JobSpec, LaneId};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

use crate::app_settings::SettingsService;
use crate::artwork::ArtworkService;
use crate::changes::{ChangeHub, ChangePart, Changes};
use crate::nfo::settings::NfoSettings;
use crate::nfo::{NfoScope, NfoService};
use crate::notices::{AppNotice, NoticeHub, NoticeLevel};
use crate::step_outcomes::{STEP_ARTWORK, StepStatus, now_ms};

/// A follow-up in progress, from [`ScrapeFollowUp::hold`]; it ends when this is dropped.
#[must_use = "the follow-up ends when the hold is dropped"]
pub struct FollowUpHold(Arc<ScrapeFollowUp>);

impl Drop for FollowUpHold {
    fn drop(&mut self) {
        self.0
            .running
            .send_modify(|count| *count = count.saturating_sub(1));
    }
}

/// How long the artwork step waits for more titles of the same scrape before it starts their
/// job, so a batch scrape is one "Downloading artwork for N titles" and not one job per title.
const ARTWORK_BATCH_WINDOW: Duration = Duration::from_millis(400);

/// The lane the artwork jobs run on: a lane of their own, so they never wait behind a scrape.
const ARTWORK_LANE: &str = "artwork";

/// Titles the artwork job fetches at once (the artwork service bounds the downloads further).
const ARTWORK_PARALLELISM: usize = 4;

/// The titles whose artwork step waits for the next job, and who waits for each.
#[derive(Default)]
struct ArtworkBatch {
    items: Vec<ItemId>,
    waiters: HashMap<ItemId, Vec<oneshot::Sender<()>>>,
    /// A job for the batch is about to be submitted.
    scheduled: bool,
}

/// The steps after a scrape, over the app's NFO and artwork services.
pub struct ScrapeFollowUp {
    nfo: Arc<NfoService>,
    artwork: Arc<ArtworkService>,
    settings: Arc<SettingsService>,
    changes: Changes,
    /// Where a step that found nothing or failed is told to Activity, once the app has set it.
    notices: Mutex<Option<Arc<NoticeHub>>>,
    /// How many titles are in their steps now.
    running: watch::Sender<usize>,
    /// The job manager the artwork step runs its jobs on; without one it runs in each title's
    /// own task.
    jobs: OnceLock<JobManager>,
    batch: Mutex<ArtworkBatch>,
}

impl ScrapeFollowUp {
    #[must_use]
    pub fn new(
        nfo: Arc<NfoService>,
        artwork: Arc<ArtworkService>,
        settings: Arc<SettingsService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            nfo,
            artwork,
            settings,
            changes: Changes::default(),
            notices: Mutex::new(None),
            running: watch::channel(0).0,
            jobs: OnceLock::new(),
            batch: Mutex::new(ArtworkBatch::default()),
        })
    }

    /// Runs the artwork step of the scraped titles as named background jobs on `jobs`, so
    /// Activity shows "Downloading artwork for 8 titles". Set once, by the app.
    pub fn set_jobs(&self, jobs: JobManager) {
        let _ = self.jobs.set(jobs);
    }

    /// Connects the follow-up to the hub that announces what changed to open pages.
    pub fn set_changes(&self, hub: Arc<ChangeHub>) {
        self.changes.set(hub);
    }

    /// Connects the follow-up to Activity: a step that found nothing or failed is a notice.
    pub fn set_notices(&self, hub: Arc<NoticeHub>) {
        *self.notices.lock().unwrap_or_else(PoisonError::into_inner) = Some(hub);
    }

    fn notify(&self, notice: AppNotice) {
        let hub = self
            .notices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(hub) = hub {
            hub.publish(notice);
        }
    }

    /// The name Activity uses for the title: "Heat (1995)".
    async fn name_of(&self, item: ItemId) -> String {
        let db = Arc::clone(&self.nfo.db);
        let found = tokio::task::spawn_blocking(move || db.read(|conn| get_item(conn, item))).await;
        match found {
            Ok(Ok(Some(found))) if found.core.year > 0 => {
                format!("{} ({})", found.core.title, found.core.year)
            }
            Ok(Ok(Some(found))) => found.core.title,
            _ => format!("title {}", item.0),
        }
    }

    /// Tells Activity when this run's artwork step found nothing or failed.
    async fn note_artwork(&self, item: ItemId, since_ms: i64) {
        let db = Arc::clone(&self.nfo.db);
        let read = tokio::task::spawn_blocking(move || {
            db.read(|conn| step_outcome(conn, item, STEP_ARTWORK))
        })
        .await;
        let Ok(Ok(Some(outcome))) = read else {
            return;
        };
        // An outcome of an earlier run (the step is off now) says nothing about this one.
        if outcome.at_ms < since_ms {
            return;
        }
        let title = match outcome.status {
            StepStatus::NotFound => "No artwork found after the scrape",
            StepStatus::Failed => "The artwork download after the scrape failed",
            _ => return,
        };
        let name = self.name_of(item).await;
        self.notify(
            AppNotice::new(NoticeLevel::Warning, format!("{title}: {name}"))
                .detail(outcome.reason)
                .source("Artwork"),
        );
    }

    /// Starts the steps for a title a scrape was written into and returns their task. Must be
    /// called inside a tokio runtime.
    pub fn after_match_task(self: &Arc<Self>, item: ItemId) -> JoinHandle<()> {
        let follow_up = Arc::clone(self);
        // Counted before the task starts, so `idle` cannot look over while this title is queued.
        follow_up.running.send_modify(|count| *count += 1);
        // Pinned before the task starts too: no rename or move of the title starts while it is
        // queued or in its steps, since a write would land in the old place or an artwork
        // download would make the old folder again (see `title_guard`). The steps' own writes
        // still hold the title one at a time; the pin only keeps moves and renames out, so the
        // steps never wait on it. A rename or move asking meanwhile is told the title is busy.
        let pin = crate::title_guard::pin(&self.nfo.db, [item]);
        cv_jobs::spawn(async move {
            let show = follow_up.pin_show_of(item).await;
            follow_up.run(item).await;
            drop(show);
            drop(pin);
            follow_up
                .running
                .send_modify(|count| *count = count.saturating_sub(1));
        })
    }

    /// The pin of the show, when `item` is a season or an episode: a title is held by its show.
    async fn pin_show_of(&self, item: ItemId) -> Option<crate::title_guard::TitleGuard> {
        let db = Arc::clone(&self.nfo.db);
        let title =
            tokio::task::spawn_blocking(move || crate::title_guard::title_of(&db, item)).await;
        match title {
            Ok(Ok(title)) if title != item => Some(crate::title_guard::pin(&self.nfo.db, [title])),
            _ => None,
        }
    }

    /// After an undone scrape put the title back in the library: writes its NFO again from the
    /// library, once the steps of the undone scrape (which write the NFO of the other title) are
    /// over, so the file ends up describing the title the library holds. Returns at once; works
    /// from any thread.
    pub fn after_undo(self: &Arc<Self>, item: ItemId) {
        let follow_up = Arc::clone(self);
        cv_jobs::spawn(async move {
            // The steps of the scrape being undone may still be writing; they go first.
            for _ in 0..600 {
                if !follow_up.is_writing(item) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let pin = crate::title_guard::pin(&follow_up.nfo.db, [item]);
            follow_up.write_nfo(item).await;
            drop(pin);
        });
    }

    /// Whether the steps of `item` are queued or run now (its title is pinned): its folder is
    /// not renamed or moved until they are over.
    #[must_use]
    pub fn is_writing(&self, item: ItemId) -> bool {
        crate::title_guard::is_pinned(&self.nfo.db, item)
    }

    /// Counts as one title in its steps until the hold is dropped: a batch waits for it like for
    /// any follow-up, and a rename does not start meanwhile. For other work that writes NFO or
    /// artwork after a scrape, and for tests that need a follow-up that is still writing.
    pub fn hold(self: &Arc<Self>) -> FollowUpHold {
        self.running.send_modify(|count| *count += 1);
        FollowUpHold(Arc::clone(self))
    }

    /// Whether no title is in its NFO or artwork step now.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        *self.running.borrow() == 0
    }

    /// Waits until no title is in its NFO or artwork step. A batch waits for this after its
    /// scrape, before steps that move folders and before it ends.
    pub async fn idle(&self) {
        let mut running = self.running.subscribe();
        let _ = running.wait_for(|count| *count == 0).await;
    }

    /// Runs the steps for one title and waits for them.
    pub async fn run(self: &Arc<Self>, item: ItemId) {
        self.write_nfo(item).await;
        let since = now_ms();
        self.fill_artwork(item).await;
        self.note_artwork(item, since).await;
    }

    /// Step 3: fills the title's empty artwork types, as an item of the artwork job when the app
    /// gave one, and waits for it. Returns at once when the after-scrape artwork is off (tests only).
    async fn fill_artwork(self: &Arc<Self>, item: ItemId) {
        if !self.artwork.after_scrape_enabled() {
            return;
        }
        let Some(jobs) = self.jobs.get().cloned() else {
            let _ = self.artwork.fill_after_scrape(item).await;
            return;
        };
        let (done, wait) = oneshot::channel();
        let start = {
            let mut batch = self.batch.lock().unwrap_or_else(PoisonError::into_inner);
            if !batch.items.contains(&item) {
                batch.items.push(item);
            }
            batch.waiters.entry(item).or_default().push(done);
            !std::mem::replace(&mut batch.scheduled, true)
        };
        if start {
            let follow_up = Arc::clone(self);
            cv_jobs::spawn(async move { follow_up.run_artwork_job(jobs).await });
        }
        // Resolves when the job has fetched this title, or when the job is over without it
        // (cancelled): either way the title's other steps go on.
        let _ = wait.await;
    }

    /// Collects the titles of the window, then fetches their artwork as one job and lets every
    /// waiting title go once the job is over.
    async fn run_artwork_job(self: Arc<Self>, jobs: JobManager) {
        tokio::time::sleep(ARTWORK_BATCH_WINDOW).await;
        let (items, waiters) = {
            let mut batch = self.batch.lock().unwrap_or_else(PoisonError::into_inner);
            batch.scheduled = false;
            (
                std::mem::take(&mut batch.items),
                std::mem::take(&mut batch.waiters),
            )
        };
        if items.is_empty() {
            return;
        }
        let db = Arc::clone(&self.nfo.db);
        let ids = items.clone();
        let names = tokio::task::spawn_blocking(move || {
            db.read(|conn| cv_db::repo::lifecycle::lifecycle_rows(conn, &ids))
        })
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
        let titles: Vec<ArtworkTitle> = items
            .iter()
            .map(|id| ArtworkTitle {
                id: *id,
                name: names.get(id).map_or_else(String::new, |row| {
                    if row.year > 0 {
                        format!("{} ({})", row.title, row.year)
                    } else {
                        row.title.clone()
                    }
                }),
            })
            .collect();
        let work = ArtworkJob {
            artwork: Arc::clone(&self.artwork),
            waiters: Arc::new(Mutex::new(waiters)),
            saved: Arc::new(AtomicUsize::new(0)),
        };
        let spec = JobSpec::new(
            crate::wording::job::downloading_artwork(titles.len()),
            LaneId::provider(ARTWORK_LANE),
        )
        .concurrency(ARTWORK_PARALLELISM);
        let waiting = Arc::clone(&work.waiters);
        let job = jobs.submit(spec, work, titles);
        job.wait().await;
        // Whatever the job did not reach (it was cancelled) is let go: its senders are dropped.
        waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// Step 2: writes the title's NFO when the NFO settings want it. A problem is logged; it
    /// never stops the artwork step.
    async fn write_nfo(&self, item: ItemId) {
        let settings = NfoSettings::from_values(&self.settings.snapshot().values);
        let nfo = Arc::clone(&self.nfo);
        let written = tokio::task::spawn_blocking(move || {
            nfo.rewrite(&NfoScope::Items(vec![item]), &settings)
        })
        .await;
        match written {
            Ok(Ok(report)) => {
                if report.totals.changed > 0 {
                    self.changes.publish_one(item, &[ChangePart::Files]);
                }
            }
            Ok(Err(error)) => {
                tracing::warn!(item = item.0, %error, "the NFO was not written after a scrape");
                let name = self.name_of(item).await;
                self.notify(
                    AppNotice::new(
                        NoticeLevel::Warning,
                        format!("The NFO was not written after the scrape: {name}"),
                    )
                    .detail(error.to_string())
                    .source("NFO"),
                );
            }
            Err(error) => tracing::warn!(item = item.0, %error, "the NFO task stopped"),
        }
    }
}

/// One title of an artwork job, shown by its name as the job's current item.
struct ArtworkTitle {
    id: ItemId,
    name: String,
}

impl std::fmt::Display for ArtworkTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.name.is_empty() {
            write!(f, "title {}", self.id.0)
        } else {
            f.write_str(&self.name)
        }
    }
}

/// The work of an artwork job: the after-scrape artwork step of each title, then the title's
/// task is told it may go on.
struct ArtworkJob {
    artwork: Arc<ArtworkService>,
    waiters: Arc<Mutex<HashMap<ItemId, Vec<oneshot::Sender<()>>>>>,
    /// Titles that got at least one image.
    saved: Arc<AtomicUsize>,
}

impl ItemWork for ArtworkJob {
    type Item = ArtworkTitle;

    /// `artwork for 190`: the titles that got images, as the job's step in the run history.
    fn result(&self) -> Option<String> {
        let saved = self.saved.load(Ordering::Relaxed);
        Some(if saved == 0 {
            "no new artwork".to_owned()
        } else {
            format!("artwork for {saved}")
        })
    }

    async fn process(&self, item: &ArtworkTitle, _control: &ItemControl) -> Result<(), ItemError> {
        // The title's run goes on in a task of its own, which holds the title and lets the
        // title's steps go on only once it is over: a cancelled job drops this future after its
        // grace period, and dropping the run itself would let go of the title while an image
        // write it started on a blocking thread still goes on (making missing folders), so a
        // rename could move the folder under it. A started run therefore always ends; the
        // titles the job did not start get no new artwork.
        let waiting = self
            .waiters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&item.id);
        let artwork = Arc::clone(&self.artwork);
        let saved = Arc::clone(&self.saved);
        let id = item.id;
        let run = cv_jobs::spawn(async move {
            // A problem is logged and left as the title's step outcome; it never fails the job.
            if artwork.fill_after_scrape(id).await {
                saved.fetch_add(1, Ordering::Relaxed);
            }
            for done in waiting.into_iter().flatten() {
                let _ = done.send(());
            }
        });
        let _ = run.await;
        Ok(())
    }
}

impl std::fmt::Debug for ScrapeFollowUp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScrapeFollowUp")
    }
}

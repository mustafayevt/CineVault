//! The after-scrape steps: what runs for a title once a scrape was written into it.
//!
//! The settings pick the steps (see [`super::AfterScrape`]); they run one after the other, in the
//! order ratings, checksums. The artwork download has a hook of its own
//! (`ScrapeFollowUp`). Nothing here renames: renaming is only ever done from Organize.
//!
//! The steps run in the background and never delay or fail the scrape. A step that cannot run
//! (a tool is missing, a key is not set) leaves one notice per session, not one per title.

use std::sync::{Arc, PoisonError};

use cv_domain::{ItemId, ItemKind};

use super::{ActionRun, ActionsService, AfterScrape, Blocker, OutcomeKind};
use crate::notices::{AppNotice, NoticeLevel};

impl ActionsService {
    /// Starts the after-scrape steps for a title a scrape was written into. Returns at once.
    /// Works from any thread, inside a tokio runtime or not.
    pub fn after_match(self: &Arc<Self>, item: ItemId) {
        self.after_match_following(item, None);
    }

    /// [`Self::after_match`] for a title whose artwork is being downloaded at the same time: the
    /// title stays "working" in its lifecycle until the download has finished too.
    pub fn after_match_following(
        self: &Arc<Self>,
        item: ItemId,
        download: Option<tokio::task::JoinHandle<()>>,
    ) {
        let after = AfterScrape::from_values(&self.values());
        // The title is working from now until its steps, and the artwork download, are over.
        let working = self.hold_working(item);
        let service = Arc::clone(self);
        cv_jobs::spawn(async move {
            if after.any_for_movie() || after.any_for_show() {
                service.run_after_scrape(item, &after).await;
            }
            if let Some(download) = download {
                let _ = download.await;
            }
            drop(working);
        });
    }

    /// Runs the switched-on steps for one title and waits for them.
    pub async fn after_scrape(&self, id: ItemId, after: &AfterScrape) {
        self.run_after_scrape(id, after).await;
    }

    async fn run_after_scrape(&self, id: ItemId, after: &AfterScrape) {
        let Ok(_permit) = self.gate.acquire().await else {
            return;
        };
        let Ok(item) = self.load(id).await else {
            return;
        };
        let ids = [id];
        let (show, movie) = (
            item.kind() == ItemKind::Show,
            item.kind() == ItemKind::Movie,
        );
        if !show && !movie {
            return;
        }
        if (movie && after.ratings_movie) || (show && after.ratings_show) {
            let run = self.fetch_ratings(&ids).await;
            self.note("ratings", "Ratings", &run);
        }
        if let Some(kind) = after.checksums {
            let run = self.checksums(&ids, kind).await;
            self.note("checksums", "Checksums", &run);
        }
    }

    /// Logs what a step did and tells the user once about a step that cannot run at all.
    pub(super) fn note(&self, step: &'static str, name: &str, run: &ActionRun) {
        for item in &run.items {
            if item.kind == OutcomeKind::Failed {
                tracing::debug!(step, title = %item.title, message = %item.message, "after-scrape step failed");
            }
        }
        let Some(blocker) = &run.blocker else {
            return;
        };
        let first_time = self
            .announced
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(step);
        if !first_time {
            return;
        }
        let title = match blocker {
            Blocker::Tool(_) => format!("{name} needs a tool"),
            Blocker::Setup(_) => format!("{name} is not set up"),
        };
        self.notices.publish(
            AppNotice::new(NoticeLevel::Warning, title)
                .detail(blocker.message())
                .source(name.to_owned()),
        );
    }
}

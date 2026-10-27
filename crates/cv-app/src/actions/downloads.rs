//! The download actions: artwork.

use std::sync::Arc;

use cv_domain::{Item, ItemId, ItemKind};
use cv_providers::model::MediaKind;

use super::{ActionRun, ActionsService, Blocker, ItemOutcome, OutcomeKind};
use crate::artwork::ArtworkError;
use crate::wording::counted;

const PROVIDERS_HELP: &str = "Add a key or sign in under Settings, Scraping and providers.";

impl ActionsService {
    // ---- artwork -----------------------------------------------------------------------

    /// Fills the empty artwork types of the titles, asking only the providers in `only` (every
    /// ready provider when the list is empty). Images already there are never replaced.
    pub async fn download_artwork(&self, ids: &[ItemId], only: &[String]) -> ActionRun {
        if !self.artwork.has_ready_provider(MediaKind::Movie)
            && !self.artwork.has_ready_provider(MediaKind::Show)
        {
            return ActionRun {
                items: Vec::new(),
                blocker: Some(Blocker::Setup(format!(
                    "No artwork provider is ready. {PROVIDERS_HELP}"
                ))),
            };
        }
        let summary = self.artwork.download_missing_from(ids, false, only).await;
        self.artwork_run(ids, &summary, "It has its artwork already.")
            .await
    }

    /// Fetches the artwork that is gone from disk again, from the sources the library remembers
    /// for each image. Images that are on disk are left as they are.
    pub async fn redownload_artwork(&self, ids: &[ItemId]) -> ActionRun {
        if !self.artwork.has_ready_provider(MediaKind::Movie)
            && !self.artwork.has_ready_provider(MediaKind::Show)
        {
            return ActionRun {
                items: Vec::new(),
                blocker: Some(Blocker::Setup(format!(
                    "No artwork provider is ready. {PROVIDERS_HELP}"
                ))),
            };
        }
        let summary = self.artwork.redownload(ids, false).await;
        self.artwork_run(ids, &summary, "Every image is on disk already.")
            .await
    }

    /// One outcome per title from what an artwork run did.
    async fn artwork_run(
        &self,
        ids: &[ItemId],
        summary: &crate::artwork::ArtworkSummary,
        nothing_to_do: &str,
    ) -> ActionRun {
        let mut run = ActionRun::default();
        for &id in ids {
            let item = match self.load(id).await {
                Ok(item) => item,
                Err(outcome) => {
                    run.items.push(outcome);
                    continue;
                }
            };
            let outcome = if let Some((_, why)) = summary.skipped.iter().find(|(s, _)| *s == id) {
                ItemOutcome::of(&item, OutcomeKind::Skipped, capitalised(why))
            } else if let Some(done) = summary.outcomes.iter().find(|o| o.item == id) {
                if done.images + done.portraits > 0 {
                    ItemOutcome::of(
                        &item,
                        OutcomeKind::Done,
                        format!(
                            "Saved {} and {}.",
                            counted(done.images, "image", "images"),
                            counted(done.portraits, "portrait", "portraits")
                        ),
                    )
                } else if let Some(problem) = done.problems.first() {
                    ItemOutcome::of(&item, OutcomeKind::Failed, problem.clone())
                } else {
                    ItemOutcome::of(&item, OutcomeKind::Unchanged, nothing_to_do)
                }
            } else {
                ItemOutcome::of(&item, OutcomeKind::Skipped, "Nothing was done.")
            };
            run.items.push(outcome);
        }
        run
    }

    /// Moves artwork the naming settings no longer use, where the image under the new name
    /// exists, to the trash folder of the data source. A movie set is tidied with its members:
    /// its own files in the collection artwork locations and the artwork of each of its movies.
    /// With `dry_run` nothing moves and the files that would are listed.
    pub async fn cleanup_artwork(&self, ids: &[ItemId], dry_run: bool) -> ActionRun {
        let mut run = ActionRun::default();
        let mut work: Vec<(ItemId, Item)> = Vec::new();
        for &id in ids {
            match self.load(id).await {
                Ok(item) => work.push((id, item)),
                Err(outcome) => run.items.push(outcome),
            }
        }
        for (id, item) in work {
            let is_set = item.kind() == ItemKind::Set;
            let mut targets = vec![id];
            if is_set {
                let db = Arc::clone(&self.db);
                let members = tokio::task::spawn_blocking(move || {
                    db.read(|conn| cv_db::repo::children_of(conn, id))
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
                targets.extend(members);
            }
            let mut moved = 0_usize;
            let mut folderless = 0_usize;
            let mut failed: Option<String> = None;
            for result in self.artwork.cleanup(&targets, dry_run).await {
                match result.outcome {
                    Ok(files) => moved += files.len(),
                    Err(ArtworkError::NoFolder | ArtworkError::Unsupported) => folderless += 1,
                    Err(error) => failed = failed.or(Some(error.to_string())),
                }
            }
            run.items.push(if let Some(message) = failed {
                ItemOutcome::of(&item, OutcomeKind::Failed, message)
            } else if is_set && targets.len() == 1 && moved == 0 {
                ItemOutcome::of(
                    &item,
                    OutcomeKind::Skipped,
                    "The set has no movie in the library, so there is no artwork to tidy.",
                )
            } else if moved > 0 {
                ItemOutcome::of(
                    &item,
                    OutcomeKind::Done,
                    if dry_run {
                        format!(
                            "{} would move to the trash folder.",
                            counted(moved, "file", "files")
                        )
                    } else {
                        format!(
                            "Moved {} to the trash folder.",
                            counted(moved, "file", "files")
                        )
                    },
                )
            } else if folderless == targets.len() {
                ItemOutcome::of(
                    &item,
                    OutcomeKind::Skipped,
                    "The title has no artwork folder to tidy.",
                )
            } else {
                ItemOutcome::of(
                    &item,
                    OutcomeKind::Unchanged,
                    "The artwork is tidy already.",
                )
            });
        }
        run
    }
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str() + "."
    })
}

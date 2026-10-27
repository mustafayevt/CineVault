//! The media actions: aspect-ratio detection and checksums. Both read the video files with the
//! FFmpeg of the tools manager (the CRC-32 needs none) and store the result on the file.

use std::path::Path;
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::{get_item, update_item};
use cv_domain::{ItemId, MediaFile, MediaFileType};
use cv_media::aspect::{ArdSettings, AspectError, AspectRequest, detect_aspect_ratio};
use cv_media::checksum::{ChecksumError, ChecksumKind, calculate_checksums};
use cv_media::tools::{Cancel, ToolsManager};

use super::{ActionRun, ActionsService, Blocker, ItemOutcome, OutcomeKind};

const FFMPEG_HELP: &str = "Download FFmpeg or set its path under Settings, External tools.";

impl ActionsService {
    /// Measures the visible picture of the titles with FFmpeg and stores the ratio on their
    /// video files. A title whose media info was not read yet, a disc folder and a disc image are
    /// skipped with the reason.
    pub async fn detect_aspect(&self, ids: &[ItemId]) -> ActionRun {
        self.per_title(ids, aspect_one).await
    }

    /// Calculates the checksums of the titles' video files. A checksum a file has already is
    /// kept.
    pub async fn checksums(&self, ids: &[ItemId], kind: ChecksumKind) -> ActionRun {
        self.per_title(ids, move |db, tools, id| checksum_one(db, tools, id, kind))
            .await
    }

    /// Runs `work` for each title on a blocking thread. A [`Blocker`] stops the run.
    async fn per_title<F>(&self, ids: &[ItemId], work: F) -> ActionRun
    where
        F: Fn(&Database, &ToolsManager, ItemId) -> Result<ItemOutcome, Blocker>
            + Send
            + Sync
            + 'static,
    {
        let work = Arc::new(work);
        let mut run = ActionRun::default();
        for &id in ids {
            let (db, tools, work) = (
                Arc::clone(&self.db),
                Arc::clone(&self.tools),
                Arc::clone(&work),
            );
            let result = tokio::task::spawn_blocking(move || work(&db, &tools, id)).await;
            match result {
                Ok(Ok(outcome)) => run.items.push(outcome),
                Ok(Err(blocker)) => {
                    run.blocker = Some(blocker);
                    break;
                }
                Err(_) => run.items.push(ItemOutcome::unreadable(
                    id,
                    "The work stopped unexpectedly.",
                )),
            }
        }
        run
    }
}

/// The video files of a title that are plain files; `Err` is the reason there are none.
fn plain_videos(files: &[MediaFile]) -> Result<Vec<&MediaFile>, &'static str> {
    let videos: Vec<&MediaFile> = files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .collect();
    if videos.is_empty() {
        return Err("The title has no video file.");
    }
    if videos
        .iter()
        .any(|f| Path::new(&f.directory).join(&f.filename).is_dir())
    {
        return Err("A disc folder has no single video file to read.");
    }
    Ok(videos)
}

fn aspect_one(db: &Database, tools: &ToolsManager, id: ItemId) -> Result<ItemOutcome, Blocker> {
    let item = match db.read(|conn| get_item(conn, id)) {
        Ok(Some(item)) => item,
        Ok(None) => {
            return Ok(ItemOutcome::unreadable(
                id,
                "The title is no longer in the library.",
            ));
        }
        Err(error) => {
            return Ok(ItemOutcome::unreadable(
                id,
                format!("The library could not be read: {error}"),
            ));
        }
    };
    let skip = |message: &str| Ok(ItemOutcome::of(&item, OutcomeKind::Skipped, message));
    let videos = match plain_videos(&item.core.media_files) {
        Ok(videos) => videos,
        Err(reason) => return skip(reason),
    };
    if videos.iter().any(|f| !f.has_media_info()) {
        return skip("The media info was not read yet. Run the media info step first.");
    }
    let request = AspectRequest::from_media_files(&videos);
    let detection =
        match detect_aspect_ratio(tools, &request, &ArdSettings::default(), &Cancel::new()) {
            Ok(detection) => detection,
            Err(error @ AspectError::Tool(_)) => {
                return Err(Blocker::Tool(format!("{error} {FFMPEG_HELP}")));
            }
            Err(error @ (AspectError::IsoNotSupported(_) | AspectError::UnknownDuration)) => {
                return skip(&error.to_string());
            }
            Err(error) => {
                return Ok(ItemOutcome::of(
                    &item,
                    OutcomeKind::Failed,
                    error.to_string(),
                ));
            }
        };

    let keys: Vec<(String, String)> = videos
        .iter()
        .map(|f| (f.directory.clone(), f.filename.clone()))
        .collect();
    let to_store = detection.clone();
    let stored = db.write(move |tx| {
        let detection = to_store;
        let Some(mut fresh) = get_item(tx, id)? else {
            return Ok(false);
        };
        let mut applied = false;
        for file in &mut fresh.core.media_files {
            if file.kind == MediaFileType::Video
                && keys
                    .iter()
                    .any(|(d, n)| *d == file.directory && *n == file.filename)
            {
                let before = (file.aspect_ratio, file.aspect_ratio2);
                if detection.apply_to(file) && before != (file.aspect_ratio, file.aspect_ratio2) {
                    applied = true;
                }
            }
        }
        if applied {
            update_item(tx, &fresh)?;
        }
        Ok(applied)
    });
    Ok(match stored {
        Ok(true) => ItemOutcome::of(
            &item,
            OutcomeKind::Done,
            format!("The picture is {:.2}:1.", detection.primary),
        ),
        Ok(false) => ItemOutcome::of(&item, OutcomeKind::Unchanged, "The ratio did not change."),
        Err(error) => ItemOutcome::of(
            &item,
            OutcomeKind::Failed,
            format!("The ratio could not be stored: {error}"),
        ),
    })
}

fn checksum_one(
    db: &Database,
    tools: &ToolsManager,
    id: ItemId,
    kind: ChecksumKind,
) -> Result<ItemOutcome, Blocker> {
    let item = match db.read(|conn| get_item(conn, id)) {
        Ok(Some(item)) => item,
        Ok(None) => {
            return Ok(ItemOutcome::unreadable(
                id,
                "The title is no longer in the library.",
            ));
        }
        Err(error) => {
            return Ok(ItemOutcome::unreadable(
                id,
                format!("The library could not be read: {error}"),
            ));
        }
    };
    let videos = match plain_videos(&item.core.media_files) {
        Ok(videos) => videos,
        Err(reason) => return Ok(ItemOutcome::of(&item, OutcomeKind::Skipped, reason)),
    };
    let cancel = Cancel::new();
    let mut calculated: Vec<MediaFile> = Vec::new();
    let mut names: Vec<&'static str> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for file in videos {
        let mut copy = file.clone();
        match calculate_checksums(tools, &mut copy, kind, false, &cancel, &mut |_, _| {}) {
            Ok(done) => {
                if !done.is_empty() {
                    names.extend(done);
                    calculated.push(copy);
                }
            }
            Err(ChecksumError::Tool(error)) => {
                // The CRC-32 of this file may have been calculated before the hash needed FFmpeg.
                if !copy.checksums.is_empty() && copy.checksums != file.checksums {
                    calculated.push(copy);
                }
                let _ = store_checksums(db, id, &calculated);
                return Err(Blocker::Tool(format!("{error} {FFMPEG_HELP}")));
            }
            Err(error) => {
                problems.push(error.to_string());
                if copy.checksums != file.checksums {
                    calculated.push(copy);
                }
            }
        }
    }
    if let Err(error) = store_checksums(db, id, &calculated) {
        return Ok(ItemOutcome::of(
            &item,
            OutcomeKind::Failed,
            format!("The checksums could not be stored: {error}"),
        ));
    }
    names.sort_unstable();
    names.dedup();
    Ok(if !names.is_empty() {
        let mut message = format!("Calculated {}.", names.join(" and "));
        if let Some(first) = problems.first() {
            message.push(' ');
            message.push_str(first);
        }
        ItemOutcome::of(&item, OutcomeKind::Done, message)
    } else if let Some(first) = problems.first() {
        ItemOutcome::of(&item, OutcomeKind::Failed, first.clone())
    } else {
        ItemOutcome::of(
            &item,
            OutcomeKind::Unchanged,
            "It has these checksums already.",
        )
    })
}

/// Writes the checksums of `files` onto the same files of the stored title.
fn store_checksums(db: &Database, id: ItemId, files: &[MediaFile]) -> cv_db::DbResult<()> {
    if files.is_empty() {
        return Ok(());
    }
    let files = files.to_vec();
    db.write(move |tx| {
        let Some(mut fresh) = get_item(tx, id)? else {
            return Ok(());
        };
        for done in &files {
            if let Some(file) = fresh.core.media_files.iter_mut().find(|f| {
                f.kind == MediaFileType::Video
                    && f.directory == done.directory
                    && f.filename == done.filename
            }) {
                file.checksums.clone_from(&done.checksums);
            }
        }
        update_item(tx, &fresh)
    })
}

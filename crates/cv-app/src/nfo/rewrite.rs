//! Rewrite NFO: library items to NFO files.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use cv_domain::{FieldSet, Item, ItemDetail, ItemId};
use cv_nfo::{
    EpisodeNfo, NfoOwnership, WriteOutcome, apply_old_episode_nfo, apply_old_movie_nfo,
    apply_old_season_nfo, apply_old_show_nfo, classify_nfo_text, content_differs, decode_nfo_bytes,
    render_episode_nfo, render_movie_nfo_for, render_movie_set_nfo, render_season_nfo,
    render_show_nfo, stale_nfos, written_by_cinevault,
};
use cv_rename::exec::TRASH_DIR;

use super::model::{
    ShowTree, aired_number, episode_model, main_video, movie_model, set_model, show_model,
    shown_number,
};
use super::paths::{
    NfoTargets, episode_targets, movie_targets, relative_folder, season_targets, set_targets,
    show_targets,
};
use super::report::{FileOutcome, FileReport, SkipReason, UnitReport, UnitStatus};
use super::service::{NfoService, Run, ShowScope, old_text, parent_exists};

/// The outcome of writing one NFO to all of its names.
pub(crate) struct Written {
    pub files: Vec<FileReport>,
    pub status: UnitStatus,
}

/// Where the original of `path` is kept: `<data source>/.deletedByCineVault/<token>/<path below
/// the data source>`, or a trash folder beside the file when it is not below the data source.
fn kept_path(token: &str, data_source: Option<&Path>, path: &Path) -> PathBuf {
    let name = path.file_name().map(PathBuf::from).unwrap_or_default();
    let (root, below) = match data_source.filter(|root| !root.as_os_str().is_empty()) {
        Some(root) if path.starts_with(root) => (
            root.to_path_buf(),
            path.strip_prefix(root).map_or(name, Path::to_path_buf),
        ),
        _ => (
            path.parent().map(Path::to_path_buf).unwrap_or_default(),
            name,
        ),
    };
    let first = root.join(TRASH_DIR).join(token).join(&below);
    let mut candidate = first.clone();
    let mut n = 1;
    while candidate.exists() {
        let stem = first.file_stem().unwrap_or_default().to_string_lossy();
        let ext = first
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        candidate = first.with_file_name(format!("{stem}-{n}{ext}"));
        n += 1;
    }
    candidate
}

/// Copies the NFO at `path` to the trash folder of the data source when CineVault did not write
/// it and it is about to be replaced by `new` (or moved away, when `new` is `None`): a file a
/// person or another tool wrote is never lost, even when nobody uses the undo record. A missing,
/// empty or foreign file (which is never written over), a file CineVault wrote and one that
/// would not change are not copied.
///
/// # Errors
/// The file cannot be read or the copy cannot be made; the caller then leaves the file alone.
fn keep_original(
    run: &Run,
    data_source: Option<&Path>,
    path: &Path,
    new: Option<&str>,
) -> io::Result<()> {
    let bytes = match cv_fs::retry("reading an NFO", path, || std::fs::read(path)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let text = decode_nfo_bytes(&bytes);
    if matches!(
        classify_nfo_text(&text),
        NfoOwnership::Foreign | NfoOwnership::Empty | NfoOwnership::Missing
    ) || written_by_cinevault(&text)
        || new.is_some_and(|new| !content_differs(&text, new))
    {
        return Ok(());
    }
    let kept = kept_path(&run.token, data_source, path);
    if let Some(parent) = kept.parent() {
        std::fs::create_dir_all(parent)?;
    }
    cv_fs::copy_synced(path, &kept)?;
    tracing::info!(
        nfo = %path.display(),
        kept = %kept.display(),
        "an NFO CineVault did not write is replaced; the original is kept in the trash folder"
    );
    Ok(())
}

/// Writes `content` to every write path of `targets`, then moves the managed NFOs the naming set
/// dropped to the trash. `create_dirs` lets the writer create missing folders (set NFOs); item
/// NFOs never create their folder, so an unplugged drive is reported instead of recreated. An NFO
/// CineVault did not write is first kept in the trash folder of `data_source` (see
/// [`keep_original`]).
pub(crate) fn write_targets(
    run: &mut Run,
    targets: &NfoTargets,
    content: &str,
    create_dirs: bool,
    data_source: Option<&Path>,
) -> Written {
    let mut files: Vec<FileReport> = Vec::new();
    let reachable: Vec<&PathBuf> = targets
        .write
        .iter()
        .filter(|p| create_dirs || parent_exists(p))
        .collect();
    if reachable.is_empty() {
        return Written {
            files,
            status: UnitStatus::Skipped {
                reason: SkipReason::FolderMissing,
            },
        };
    }
    let mut ours = false;
    let mut failed: Option<String> = None;
    for path in &targets.write {
        let outcome = if !create_dirs && !parent_exists(path) {
            FileOutcome::Failed {
                message: "the folder is not reachable".to_owned(),
            }
        } else if let Err(e) = keep_original(run, data_source, path, Some(content)) {
            FileOutcome::Failed {
                message: format!("the original NFO could not be kept, so it was left alone: {e}"),
            }
        } else {
            match run.journal.write(path, content) {
                Ok(WriteOutcome::Created) => FileOutcome::Created,
                Ok(WriteOutcome::Replaced) => FileOutcome::Updated,
                Ok(WriteOutcome::Unchanged) => FileOutcome::Unchanged,
                Ok(WriteOutcome::SkippedForeign) => FileOutcome::KeptForeign,
                Err(e) => FileOutcome::Failed {
                    message: e.to_string(),
                },
            }
        };
        match &outcome {
            FileOutcome::Created | FileOutcome::Updated | FileOutcome::Unchanged => ours = true,
            FileOutcome::Failed { message } if failed.is_none() => failed = Some(message.clone()),
            _ => {}
        }
        files.push(FileReport {
            path: path.clone(),
            outcome,
        });
    }

    // Cleanup only once the item has its NFO under the new names, and never touch a file that is
    // not a managed candidate.
    if ours && failed.is_none() {
        for stale in stale_nfos(&targets.candidates, &targets.write) {
            if let Err(e) = keep_original(run, data_source, &stale, None) {
                if failed.is_none() {
                    failed = Some(e.to_string());
                }
                files.push(FileReport {
                    path: stale,
                    outcome: FileOutcome::Failed {
                        message: format!(
                            "the original NFO could not be kept, so it was left alone: {e}"
                        ),
                    },
                });
                continue;
            }
            let outcome = match run.journal.trash(&stale) {
                Ok(true) => FileOutcome::MovedToTrash,
                Ok(false) => continue,
                Err(e) => FileOutcome::Failed {
                    message: e.to_string(),
                },
            };
            if let FileOutcome::Failed { message } = &outcome
                && failed.is_none()
            {
                failed = Some(message.clone());
            }
            files.push(FileReport {
                path: stale,
                outcome,
            });
        }
    }

    let status = if let Some(message) = failed {
        UnitStatus::Failed { message }
    } else if files.iter().any(|f| {
        matches!(
            f.outcome,
            FileOutcome::Created | FileOutcome::Updated | FileOutcome::MovedToTrash
        )
    }) {
        UnitStatus::Changed
    } else if ours {
        UnitStatus::Unchanged
    } else {
        UnitStatus::KeptForeign
    };
    Written { files, status }
}

fn unit(item: ItemId, label: impl Into<String>, written: Written) -> UnitReport {
    UnitReport {
        item,
        label: label.into(),
        status: written.status,
        files: written.files,
    }
}

/// The data source an item's files are below, when it is recorded.
fn data_source_of(item: &Item) -> Option<PathBuf> {
    let source = item.core.data_source.trim();
    (!source.is_empty()).then(|| PathBuf::from(source))
}

fn first_id(item: &Item) -> ItemId {
    item.id.unwrap_or(ItemId(0))
}

impl NfoService {
    /// Writes the NFO of a movie while its title is held, and lists what it wrote among the
    /// movie's files before it lets go (see [`NfoService::record_nfo_files`]).
    pub(crate) fn rewrite_movie(&self, run: &mut Run, id: ItemId) {
        let Some(_title) = self.hold_title(run, id) else {
            return;
        };
        let start = run.units.len();
        self.write_movie(run, id);
        self.record_nfo_files(&run.units[start..]);
    }

    fn write_movie(&self, run: &mut Run, id: ItemId) {
        let loaded = match self.load_movie(id) {
            Ok(Some(loaded)) => loaded,
            Ok(None) => {
                run.units.push(UnitReport::skipped(
                    id,
                    id.0.to_string(),
                    SkipReason::NotFound,
                ));
                return;
            }
            Err(e) => {
                run.units
                    .push(UnitReport::failed(id, id.0.to_string(), e.to_string()));
                return;
            }
        };
        let (item, set) = loaded;
        let label = item.core.title.clone();
        let options = run.settings.movie.clone();
        let (Some(targets), Some(mut movie)) = (
            movie_targets(&item, &run.settings),
            movie_model(&item, set.as_ref(), &options),
        ) else {
            let reason = if matches!(item.detail, ItemDetail::Movie(_)) {
                SkipReason::NoFiles
            } else {
                SkipReason::Unsupported
            };
            run.units.push(UnitReport::skipped(id, label, reason));
            return;
        };
        match self.locks_of(&[id]) {
            Ok(locks) => {
                movie.locked_fields = locks.get(&id).map(FieldSet::keys).unwrap_or_default();
            }
            Err(e) => {
                run.units.push(UnitReport::failed(id, label, e.to_string()));
                return;
            }
        }
        if let Some(old) = old_text(&targets) {
            apply_old_movie_nfo(&mut movie, &old, &options);
        }
        let content = render_movie_nfo_for(run.settings.movie_connector, &movie, &options);
        let source = data_source_of(&item);
        let written = write_targets(run, &targets, &content, false, source.as_deref());
        run.units.push(unit(id, label, written));
    }

    pub(crate) fn rewrite_set(&self, run: &mut Run, id: ItemId) {
        let item = match self.load_item(id) {
            Ok(Some(item)) => item,
            Ok(None) => {
                run.units.push(UnitReport::skipped(
                    id,
                    id.0.to_string(),
                    SkipReason::NotFound,
                ));
                return;
            }
            Err(e) => {
                run.units
                    .push(UnitReport::failed(id, id.0.to_string(), e.to_string()));
                return;
            }
        };
        let label = item.core.title.clone();
        let Some(set) = set_model(&item) else {
            run.units
                .push(UnitReport::skipped(id, label, SkipReason::Unsupported));
            return;
        };
        let Some(targets) = set_targets(&item, &run.settings) else {
            run.units.push(UnitReport::skipped(
                id,
                label,
                SkipReason::NoCollectionFolder,
            ));
            return;
        };
        let content = render_movie_set_nfo(run.settings.set_connector, &set, &run.settings.movie);
        let written = write_targets(run, &targets, &content, true, None);
        run.units.push(unit(id, label, written));
    }

    /// Holds the title while its NFOs are written, so no move or rename of it runs meanwhile
    /// (see [`crate::title_guard`]). Taken before the title is loaded, so the folder written to
    /// is the current one. `None`, with the unit reported failed, when the title stays busy.
    fn hold_title(&self, run: &mut Run, id: ItemId) -> Option<crate::title_guard::TitleGuard> {
        use crate::title_guard::{WRITE_WAIT, Work, hold_items};
        match hold_items(&self.db, &[id], Work::Write, WRITE_WAIT) {
            Ok(guard) => Some(guard),
            Err(error) => {
                run.units
                    .push(UnitReport::failed(id, id.0.to_string(), error.to_string()));
                None
            }
        }
    }

    /// Writes the NFOs of a show (and its seasons and episodes) while the show is held, and lists
    /// what it wrote among the episodes' files before it lets go.
    pub(crate) fn rewrite_show(&self, run: &mut Run, id: ItemId, scope: &ShowScope) {
        let Some(_title) = self.hold_title(run, id) else {
            return;
        };
        let start = run.units.len();
        self.write_show(run, id, scope);
        self.record_nfo_files(&run.units[start..]);
    }

    fn write_show(&self, run: &mut Run, id: ItemId, scope: &ShowScope) {
        let data = match self.load_show(id) {
            Ok(Some(data)) => data,
            Ok(None) => {
                run.units.push(UnitReport::skipped(
                    id,
                    id.0.to_string(),
                    SkipReason::NotFound,
                ));
                return;
            }
            Err(e) => {
                run.units
                    .push(UnitReport::failed(id, id.0.to_string(), e.to_string()));
                return;
            }
        };
        let tv = run.settings.tv.clone();
        let tree = ShowTree {
            show: &data.show,
            seasons: &data.seasons,
            episodes: &data.episodes,
        };
        let Some(mut show) = show_model(&tree, &tv.common) else {
            run.units.push(UnitReport::skipped(
                id,
                data.show.core.title.clone(),
                SkipReason::Unsupported,
            ));
            return;
        };
        let show_title = data.show.core.title.clone();
        let show_folder = PathBuf::from(data.show.core.path.trim());
        let source = data_source_of(&data.show);
        match self.locks_of(&[id]) {
            Ok(locks) => {
                show.locked_fields = locks.get(&id).map(FieldSet::keys).unwrap_or_default();
            }
            Err(e) => {
                run.units
                    .push(UnitReport::failed(id, show_title, e.to_string()));
                return;
            }
        }

        // The show NFO. Its carried-over values come from the old file.
        let show_paths = show_targets(&data.show, &run.settings);
        if let Some(targets) = &show_paths
            && let Some(old) = old_text(targets)
        {
            apply_old_show_nfo(&mut show, &old, &tv.common);
        }
        if scope.show_nfo() {
            match &show_paths {
                Some(targets) => {
                    let content = render_show_nfo(run.settings.tv_connector, &show, &tv);
                    let written = write_targets(run, targets, &content, false, source.as_deref());
                    run.units.push(unit(id, show_title.clone(), written));
                }
                None => run.units.push(UnitReport::skipped(
                    id,
                    show_title.clone(),
                    SkipReason::NoFiles,
                )),
            }
        }

        // Season NFOs.
        for season in show.seasons.clone() {
            if !scope.wants_season(season.number) {
                continue;
            }
            let record = data
                .seasons
                .iter()
                .find(|s| matches!(&s.detail, ItemDetail::Season(f) if f.season == season.number));
            let season_id = record.map_or(id, first_id);
            let label = format!("{show_title} - Season {}", season.number);
            if show_folder.as_os_str().is_empty() {
                run.units
                    .push(UnitReport::skipped(season_id, label, SkipReason::NoFiles));
                continue;
            }
            let folder = season_folder(&show_folder, record, &data.episodes, season.number);
            let targets = season_targets(
                &show_folder,
                season.number,
                folder.as_deref(),
                &run.settings,
            );
            let mut season_nfo = season;
            if let Some(old) = old_text(&targets) {
                apply_old_season_nfo(&mut season_nfo, &old, &tv.common);
            }
            let content = render_season_nfo(&show, &season_nfo, &tv);
            let written = write_targets(run, &targets, &content, false, source.as_deref());
            run.units.push(unit(season_id, label, written));
        }

        // Episode NFOs, one per video file.
        let mut files: BTreeMap<String, Vec<&Item>> = BTreeMap::new();
        for episode in &data.episodes {
            let Some(video) = main_video(episode) else {
                continue;
            };
            files
                .entry(video.path().to_string_lossy().to_lowercase())
                .or_default()
                .push(episode);
        }
        for episodes in files.into_values() {
            let wanted = episodes
                .iter()
                .any(|e| scope.wants_episode(e.id, aired_number(e).map_or(0, |n| n.0)));
            if !wanted {
                continue;
            }
            self.rewrite_episode_file(run, &data.show, &episodes);
        }
    }

    fn rewrite_episode_file(&self, run: &mut Run, show: &Item, episodes: &[&Item]) {
        let tv = run.settings.tv.clone();
        let ItemDetail::Show(show_fields) = &show.detail else {
            return;
        };
        let mut ordered: Vec<&Item> = episodes.to_vec();
        ordered.sort_by_key(|e| shown_number(e, &show_fields.episode_group));
        let first = ordered[0];
        let id = first_id(first);
        let (season, number) = shown_number(first, &show_fields.episode_group);
        let label = format!("{} S{season:02}E{number:02}", show.core.title);
        let Some(targets) = episode_targets(first) else {
            run.units
                .push(UnitReport::skipped(id, label, SkipReason::NoFiles));
            return;
        };
        let mut models: Vec<EpisodeNfo> = ordered
            .iter()
            .filter_map(|e| episode_model(show, e, &tv.common))
            .collect();
        if let Some(old) = old_text(&targets) {
            apply_old_episode_nfo(&mut models, &old, &tv.common);
        }
        let content = render_episode_nfo(run.settings.tv_connector, &models, &tv);
        let source = data_source_of(show);
        let written = write_targets(run, &targets, &content, false, source.as_deref());
        run.units.push(unit(id, label, written));
    }
}

/// The folder holding a season's episodes, relative to the show folder.
fn season_folder(
    show_folder: &Path,
    record: Option<&Item>,
    episodes: &[Item],
    season: u32,
) -> Option<String> {
    if let Some(record) = record {
        let path = record.core.path.trim();
        if !path.is_empty() {
            return relative_folder(show_folder, Path::new(path));
        }
    }
    episodes
        .iter()
        .filter(|e| aired_number(e).map_or(0, |n| n.0) == season)
        .filter_map(|e| main_video(e))
        .find_map(|v| relative_folder(show_folder, Path::new(&v.directory)))
}

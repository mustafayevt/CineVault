//! Read NFO: NFO files into library items.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cv_db::DbError;
use cv_domain::{Item, ItemDetail, ItemId, MetaField};
use cv_nfo::{
    EpisodeRead, MovieSetRef, read_episode_nfo, read_movie_nfo, read_movie_set_nfo,
    read_season_nfo, read_show_nfo,
};

use crate::collections::{CollectionError, ensure_named};

use super::merge::{merge_episode, merge_movie, merge_season, merge_set, merge_show};
use super::model::{aired_number, main_video, shown_number};
use super::paths::{
    NfoTargets, episode_targets, movie_targets, relative_folder, season_targets, set_targets,
    show_targets,
};
use super::report::{SkipReason, UnitReport, UnitStatus};
use super::service::{Found, NfoService, Run, ShowScope, find_nfo, protect};

/// Marks `item` as scraped by CineVault when its NFO was written by CineVault (the header
/// comment) and names a provider ID, and no scraper is recorded yet: the details came from a
/// scrape of an earlier CineVault, whose library was set aside on the fresh start. The NFO then
/// stays CineVault's to keep current, as it was before. The provider recorded is the one the
/// NFO's IDs name, in the order the NFO writers prefer them, so the files written later are the
/// same as with no scraper recorded.
fn adopt_cinevault_scrape(item: &mut Item, text: &str) {
    if item.core.locked
        || !item.core.last_scraper_id.trim().is_empty()
        || !cv_nfo::written_by_cinevault(text)
    {
        return;
    }
    let order: &[&str] = match item.kind() {
        cv_domain::ItemKind::Show => &["tvdb", "tmdb", "imdb"],
        _ => &["tmdb", "imdb", "tvdb"],
    };
    if let Some(provider) = order.iter().find(|p| item.core.ids.get(p).is_some()) {
        item.core.last_scraper_id = (*provider).to_owned();
    }
}

fn status(changed: bool) -> UnitStatus {
    if changed {
        UnitStatus::Changed
    } else {
        UnitStatus::Unchanged
    }
}

fn unit(item: ItemId, label: String, status: UnitStatus) -> UnitReport {
    UnitReport {
        item,
        label,
        status,
        files: Vec::new(),
    }
}

/// The text of the NFO of `targets`, or the report that explains why there is none.
fn text_or_skip(targets: &NfoTargets, id: ItemId, label: &str) -> Result<String, Box<UnitReport>> {
    match find_nfo(targets) {
        Found::Managed(text) => Ok(text),
        Found::NotManaged => Err(Box::new(UnitReport::skipped(
            id,
            label,
            SkipReason::NotManaged,
        ))),
        Found::None => Err(Box::new(UnitReport::skipped(id, label, SkipReason::NoNfo))),
    }
}

impl NfoService {
    pub(crate) fn read_movie(&self, run: &mut Run, id: ItemId) {
        let loaded = match self.load_movie(id) {
            Ok(Some((item, _))) => item,
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
        let label = loaded.core.title.clone();
        let Some(targets) = movie_targets(&loaded, &run.settings) else {
            run.units
                .push(UnitReport::skipped(id, label, SkipReason::NoFiles));
            return;
        };
        let text = match text_or_skip(&targets, id, &label) {
            Ok(text) => text,
            Err(report) => {
                run.units.push(*report);
                return;
            }
        };
        let Some(nfo) = read_movie_nfo(&text).filter(cv_nfo::MovieNfo::is_valid) else {
            run.units
                .push(UnitReport::skipped(id, label, SkipReason::NotManaged));
            return;
        };
        let locks = match self.locks_of(&[id]) {
            Ok(locks) => locks,
            Err(e) => {
                run.units.push(UnitReport::failed(id, label, e.to_string()));
                return;
            }
        };
        let mut updated = loaded.clone();
        merge_movie(&mut updated, &nfo);
        protect(&mut updated, &loaded, &locks);
        if run.adopt_scrape {
            adopt_cinevault_scrape(&mut updated, &text);
        }
        let collection_locked = loaded.core.locked
            || locks
                .get(&id)
                .is_some_and(|fields| fields.contains(MetaField::Collection));
        if updated.parent_id.is_none()
            && !collection_locked
            && let Some(set) = &nfo.movie_set
        {
            match self.collection_of(set) {
                Ok(set_id) => updated.parent_id = Some(set_id),
                Err(e) => {
                    run.units.push(UnitReport::failed(id, label, e.to_string()));
                    return;
                }
            }
        }
        self.store(run, id, label, vec![(loaded, updated)]);
    }

    /// The collection a movie NFO's `<set>` names, created when the library has none. A movie
    /// that is in no collection joins it on Read NFO and on the first import, so the collection
    /// shows up and the next NFO write keeps the `<set>`.
    fn collection_of(&self, set: &MovieSetRef) -> Result<ItemId, DbError> {
        let (title, overview, tmdb) = (set.title.clone(), set.overview.clone(), set.tmdb_id);
        self.db.write(move |tx| {
            ensure_named(tx, &title, &overview, tmdb).map_err(|e| match e {
                CollectionError::Db(e) => e,
                other => DbError::InvalidData(other.to_string()),
            })
        })
    }

    pub(crate) fn read_set(&self, run: &mut Run, id: ItemId) {
        let loaded = match self.load_item(id) {
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
        let label = loaded.core.title.clone();
        if !matches!(loaded.detail, ItemDetail::Set(_)) {
            run.units
                .push(UnitReport::skipped(id, label, SkipReason::Unsupported));
            return;
        }
        let Some(targets) = set_targets(&loaded, &run.settings) else {
            run.units.push(UnitReport::skipped(
                id,
                label,
                SkipReason::NoCollectionFolder,
            ));
            return;
        };
        let text = match text_or_skip(&targets, id, &label) {
            Ok(text) => text,
            Err(report) => {
                run.units.push(*report);
                return;
            }
        };
        let Some(nfo) = read_movie_set_nfo(&text).filter(cv_nfo::MovieSetNfo::is_valid) else {
            run.units
                .push(UnitReport::skipped(id, label, SkipReason::NotManaged));
            return;
        };
        let locks = match self.locks_of(&[id]) {
            Ok(locks) => locks,
            Err(e) => {
                run.units.push(UnitReport::failed(id, label, e.to_string()));
                return;
            }
        };
        let mut updated = loaded.clone();
        merge_set(&mut updated, &nfo);
        protect(&mut updated, &loaded, &locks);
        self.store(run, id, label, vec![(loaded, updated)]);
    }

    pub(crate) fn read_show(&self, run: &mut Run, id: ItemId, scope: &ShowScope) {
        let mut data = match self.load_show(id) {
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
        let original_show = data.show.clone();
        let original_seasons = data.seasons.clone();
        let original_episodes = data.episodes.clone();
        let tree_ids: Vec<ItemId> = std::iter::once(&data.show)
            .chain(&data.seasons)
            .chain(&data.episodes)
            .filter_map(|i| i.id)
            .collect();
        let locks = match self.locks_of(&tree_ids) {
            Ok(locks) => locks,
            Err(e) => {
                run.units.push(UnitReport::failed(
                    id,
                    data.show.core.title.clone(),
                    e.to_string(),
                ));
                return;
            }
        };
        let show_title = data.show.core.title.clone();
        let show_folder = PathBuf::from(data.show.core.path.trim());

        let mut units: Vec<UnitReport> = Vec::new();

        // The show NFO.
        if scope.show_nfo() {
            match show_targets(&data.show, &run.settings) {
                None => units.push(UnitReport::skipped(
                    id,
                    show_title.clone(),
                    SkipReason::NoFiles,
                )),
                Some(targets) => match text_or_skip(&targets, id, &show_title) {
                    Err(report) => units.push(*report),
                    Ok(text) => match read_show_nfo(&text).filter(cv_nfo::ShowNfo::is_valid) {
                        None => units.push(UnitReport::skipped(
                            id,
                            show_title.clone(),
                            SkipReason::NotManaged,
                        )),
                        Some(nfo) => {
                            let before = data.show.clone();
                            merge_show(&mut data.show, &nfo);
                            protect(&mut data.show, &before, &locks);
                            if run.adopt_scrape {
                                adopt_cinevault_scrape(&mut data.show, &text);
                            }
                            units.push(unit(id, show_title.clone(), status(before != data.show)));
                        }
                    },
                },
            }
        }

        // Season NFOs.
        let season_numbers: Vec<u32> = data
            .seasons
            .iter()
            .filter_map(|s| match &s.detail {
                ItemDetail::Season(f) => Some(f.season),
                _ => None,
            })
            .filter(|n| scope.wants_season(*n))
            .collect();
        for number in season_numbers {
            let Some(index) = data
                .seasons
                .iter()
                .position(|s| matches!(&s.detail, ItemDetail::Season(f) if f.season == number))
            else {
                continue;
            };
            let season_id = data.seasons[index].id.unwrap_or(id);
            let label = format!("{show_title} - Season {number}");
            if show_folder.as_os_str().is_empty() {
                units.push(UnitReport::skipped(season_id, label, SkipReason::NoFiles));
                continue;
            }
            let folder = season_folder(&show_folder, &data.seasons[index], &data.episodes, number);
            let targets = season_targets(&show_folder, number, folder.as_deref(), &run.settings);
            let text = match text_or_skip(&targets, season_id, &label) {
                Ok(text) => text,
                Err(report) => {
                    units.push(*report);
                    continue;
                }
            };
            let Some(read) = read_season_nfo(&text).filter(cv_nfo::SeasonRead::is_valid) else {
                units.push(UnitReport::skipped(
                    season_id,
                    label,
                    SkipReason::NotManaged,
                ));
                continue;
            };
            let before = (data.seasons[index].clone(), data.show.clone());
            let mut season = data.seasons[index].clone();
            merge_season(&mut season, &mut data.show, &read);
            protect(&mut season, &before.0, &locks);
            protect(&mut data.show, &before.1, &locks);
            let changed = before != (season.clone(), data.show.clone());
            data.seasons[index] = season;
            units.push(unit(season_id, label, status(changed)));
        }

        // Episode NFOs, one per video file.
        let ItemDetail::Show(show_fields) = &data.show.detail else {
            return;
        };
        let active = show_fields.episode_group.clone();
        let mut files: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, episode) in data.episodes.iter().enumerate() {
            if let Some(video) = main_video(episode) {
                files
                    .entry(cv_fs::paths::key(&video.path()))
                    .or_default()
                    .push(index);
            }
        }
        for mut indexes in files.into_values() {
            if !indexes.iter().any(|i| {
                let e = &data.episodes[*i];
                scope.wants_episode(e.id, aired_number(e).map_or(0, |n| n.0))
            }) {
                continue;
            }
            indexes.sort_by_key(|i| shown_number(&data.episodes[*i], &active));
            let first = &data.episodes[indexes[0]];
            let first_id = first.id.unwrap_or(id);
            let (season, number) = shown_number(first, &active);
            let label = format!("{show_title} S{season:02}E{number:02}");
            let Some(targets) = episode_targets(first) else {
                units.push(UnitReport::skipped(first_id, label, SkipReason::NoFiles));
                continue;
            };
            let text = match text_or_skip(&targets, first_id, &label) {
                Ok(text) => text,
                Err(report) => {
                    units.push(*report);
                    continue;
                }
            };
            let reads: Vec<EpisodeRead> = read_episode_nfo(&text)
                .into_iter()
                .filter(EpisodeRead::is_valid)
                .collect();
            if reads.is_empty() {
                units.push(UnitReport::skipped(first_id, label, SkipReason::NotManaged));
                continue;
            }
            let numbers: Vec<(u32, u32)> = indexes
                .iter()
                .map(|i| shown_number(&data.episodes[*i], &active))
                .collect();
            let mut changed = false;
            for (position, read_index) in match_blocks(&numbers, &reads) {
                let index = indexes[position];
                let before = data.episodes[index].clone();
                merge_episode(&mut data.episodes[index], &reads[read_index]);
                protect(&mut data.episodes[index], &before, &locks);
                changed |= before != data.episodes[index];
            }
            units.push(unit(first_id, label, status(changed)));
        }

        // Store everything that changed, one transaction for the show.
        let mut changes: Vec<(Item, Item)> = Vec::new();
        if original_show != data.show {
            changes.push((original_show, data.show));
        }
        for (before, after) in original_seasons.into_iter().zip(data.seasons) {
            if before != after {
                changes.push((before, after));
            }
        }
        for (before, after) in original_episodes.into_iter().zip(data.episodes) {
            if before != after {
                changes.push((before, after));
            }
        }
        if let Err(e) = self.commit(run, changes) {
            for unit in &mut units {
                if unit.status == UnitStatus::Changed {
                    unit.status = UnitStatus::Failed {
                        message: e.to_string(),
                    };
                }
            }
        }
        run.units.extend(units);
    }

    /// Stores the changed items of one unit and reports it.
    fn store(&self, run: &mut Run, id: ItemId, label: String, pairs: Vec<(Item, Item)>) {
        let changed: Vec<(Item, Item)> = pairs.into_iter().filter(|(b, a)| b != a).collect();
        let was_changed = !changed.is_empty();
        match self.commit(run, changed) {
            Ok(()) => run.units.push(unit(id, label, status(was_changed))),
            Err(e) => run.units.push(UnitReport::failed(id, label, e.to_string())),
        }
    }
}

fn season_folder(
    show_folder: &Path,
    record: &Item,
    episodes: &[Item],
    season: u32,
) -> Option<String> {
    let own = record.core.path.trim();
    if !own.is_empty() {
        return relative_folder(show_folder, Path::new(own));
    }
    episodes
        .iter()
        .filter(|e| aired_number(e).map_or(0, |n| n.0) == season)
        .filter_map(|e| main_video(e))
        .find_map(|v| relative_folder(show_folder, Path::new(&v.directory)))
}

/// Pairs the episodes of a file (by position) with the blocks read from its NFO: by season and
/// episode number first, then in order when as many are left over on both sides.
pub(crate) fn match_blocks(numbers: &[(u32, u32)], reads: &[EpisodeRead]) -> Vec<(usize, usize)> {
    let mut taken = vec![false; reads.len()];
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (position, number) in numbers.iter().enumerate() {
        let found = reads.iter().enumerate().position(|(i, r)| {
            !taken[i] && (r.season_number, r.episode_number) == (Some(number.0), Some(number.1))
        });
        if let Some(i) = found {
            taken[i] = true;
            pairs.push((position, i));
        }
    }
    let free_positions: Vec<usize> = (0..numbers.len())
        .filter(|p| !pairs.iter().any(|(q, _)| q == p))
        .collect();
    let free_reads: Vec<usize> = (0..reads.len()).filter(|i| !taken[*i]).collect();
    if free_positions.len() == free_reads.len() {
        pairs.extend(free_positions.into_iter().zip(free_reads));
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(season: u32, episode: u32) -> EpisodeRead {
        EpisodeRead {
            season_number: Some(season),
            episode_number: Some(episode),
            ..EpisodeRead::default()
        }
    }

    #[test]
    fn blocks_match_by_number_then_in_order() {
        let reads = [read(2, 6), read(2, 5), read(2, 7)];
        let numbers = [(2, 5), (2, 6), (2, 7)];
        let mut pairs = match_blocks(&numbers, &reads);
        pairs.sort_unstable();
        assert_eq!(pairs, [(0, 1), (1, 0), (2, 2)]);

        // A renumbered episode keeps its block when exactly one is left on each side.
        let reads = [read(2, 5), read(2, 9)];
        let numbers = [(2, 5), (2, 6)];
        let mut pairs = match_blocks(&numbers, &reads);
        pairs.sort_unstable();
        assert_eq!(pairs, [(0, 0), (1, 1)]);

        // Unclear leftovers are not paired.
        let reads = [read(2, 5)];
        let numbers = [(2, 6), (2, 7)];
        assert!(match_blocks(&numbers, &reads).is_empty());
    }
}

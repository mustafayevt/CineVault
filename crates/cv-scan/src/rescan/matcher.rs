//! Pairs the units found on disk with the units already in the library.
//!
//! A *unit* is whatever owns a set of files and keeps its metadata when the files change: a
//! movie, an episode (all the episode items that share one file count as one unit) or a show.
//! The matcher never looks at titles to decide that something is new; it only decides which
//! stored unit a scanned unit continues.
//!
//! Passes, in order. Each unit is paired at most once.
//!
//! 1. **Same video.** A scanned unit that holds a video path the stored unit already has. This
//!    is every unchanged or edited-in-place unit.
//! 2. **Same content, new name.** Within one folder (a *group*), a video with the same size and
//!    modification time as a stored one: the file was renamed in place.
//! 3. **Same title.** Within one group, the same title key (a movie's title and year, an
//!    episode's season and number).
//! 4. **The only two left.** Within one group, when exactly one scanned and one stored unit are
//!    unpaired and the caller allows it: a single-movie folder whose video was replaced by one
//!    with a different name, size and time.

use std::collections::HashMap;

/// What the matcher needs to know about one unit, scanned or stored.
#[derive(Debug, Clone, Default)]
pub(crate) struct Probe {
    /// Normalised paths of the unit's video files.
    pub videos: Vec<String>,
    /// Size and modification time of the unit's video files.
    pub content: Vec<(u64, i64)>,
    /// The folder or show the unit lives in; pass 2 to 4 only pair units of the same group.
    pub group: String,
    /// Identifies the unit within its group, for pass 3.
    pub title_key: Option<String>,
    /// The unit is alone in its folder, so pass 4 may pair it with the only unit left there.
    pub solo: bool,
}

/// For every scanned unit, the index of the stored unit it continues. Pass 4 runs only when
/// `pair_last_two` is set.
pub(crate) fn match_units(
    scanned: &[Probe],
    stored: &[Probe],
    pair_last_two: bool,
) -> Vec<Option<usize>> {
    let mut paired: Vec<Option<usize>> = vec![None; scanned.len()];
    let mut taken = vec![false; stored.len()];

    // Pass 1: the same video path.
    let mut by_video: HashMap<&str, usize> = HashMap::new();
    for (j, unit) in stored.iter().enumerate() {
        for video in &unit.videos {
            by_video.entry(video.as_str()).or_insert(j);
        }
    }
    for (i, unit) in scanned.iter().enumerate() {
        let hit = unit
            .videos
            .iter()
            .filter_map(|v| by_video.get(v.as_str()).copied())
            .find(|&j| !taken[j]);
        if let Some(j) = hit {
            paired[i] = Some(j);
            taken[j] = true;
        }
    }

    // Passes 2 to 4 work inside one group at a time.
    let mut groups: HashMap<&str, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (i, unit) in scanned.iter().enumerate() {
        if paired[i].is_none() {
            groups.entry(unit.group.as_str()).or_default().0.push(i);
        }
    }
    for (j, unit) in stored.iter().enumerate() {
        if !taken[j]
            && let Some(entry) = groups.get_mut(unit.group.as_str())
        {
            entry.1.push(j);
        }
    }
    let mut keys: Vec<&str> = groups.keys().copied().collect();
    keys.sort_unstable();
    for key in keys {
        let (scanned_ids, stored_ids) = &groups[key];
        if stored_ids.is_empty() {
            continue;
        }
        // Pass 2: renamed in place, same size and time.
        for &i in scanned_ids {
            if paired[i].is_some() {
                continue;
            }
            let hit = stored_ids.iter().copied().find(|&j| {
                !taken[j]
                    && scanned[i]
                        .content
                        .iter()
                        .any(|c| c.0 > 0 && stored[j].content.contains(c))
            });
            if let Some(j) = hit {
                paired[i] = Some(j);
                taken[j] = true;
            }
        }
        // Pass 3: the same title key.
        for &i in scanned_ids {
            if paired[i].is_some() {
                continue;
            }
            let Some(title) = &scanned[i].title_key else {
                continue;
            };
            let hit = stored_ids
                .iter()
                .copied()
                .find(|&j| !taken[j] && stored[j].title_key.as_ref() == Some(title));
            if let Some(j) = hit {
                paired[i] = Some(j);
                taken[j] = true;
            }
        }
        // Pass 4: the only two left in the group.
        if pair_last_two {
            let open_scanned: Vec<usize> = scanned_ids
                .iter()
                .copied()
                .filter(|&i| paired[i].is_none())
                .collect();
            let open_stored: Vec<usize> =
                stored_ids.iter().copied().filter(|&j| !taken[j]).collect();
            if let ([i], [j]) = (open_scanned.as_slice(), open_stored.as_slice())
                && scanned[*i].solo
                && stored[*j].solo
            {
                paired[*i] = Some(*j);
                taken[*j] = true;
            }
        }
    }
    paired
}

/// What [`match_moved`] decided about the units that no pass of [`match_units`] paired.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Moved {
    /// `(scanned, stored)` pairs where exactly one stored unit fits the scanned one and the
    /// other way round: the title moved to a new folder or got a new file name.
    pub relinks: Vec<(usize, usize)>,
    /// `(scanned, stored)` pairs that fit but are not unique on one side. Nothing is relinked;
    /// the review lists the pair as a possible duplicate.
    pub ambiguous: Vec<(usize, usize)>,
}

fn stem_of(video_key: &str) -> &str {
    let name = video_key.rsplit('/').next().unwrap_or(video_key);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn sorted_sizes(probe: &Probe) -> Vec<u64> {
    let mut sizes: Vec<u64> = probe.content.iter().map(|c| c.0).collect();
    sizes.sort_unstable();
    sizes
}

/// Whether a scanned unit looks like a stored unit that was moved or renamed outside the app:
/// the same video sizes (every one non-zero) and either a shared file name stem or the same
/// title key with a year.
fn looks_moved(scanned: &Probe, stored: &Probe) -> bool {
    let sizes = sorted_sizes(scanned);
    if sizes.is_empty() || sizes.contains(&0) || sizes != sorted_sizes(stored) {
        return false;
    }
    let same_stem = scanned.videos.iter().any(|a| {
        stored
            .videos
            .iter()
            .any(|b| stem_of(a).eq_ignore_ascii_case(stem_of(b)))
    });
    let same_title = match (&scanned.title_key, &stored.title_key) {
        (Some(a), Some(b)) => a == b && !a.ends_with("|0"),
        _ => false,
    };
    same_stem || same_title
}

/// Finds the stored units that a scan did not pair but that moved to another folder: units in
/// `open_scanned` and `open_stored` (indexes into the slices) that [`looks_moved`] ties together.
/// A pair is relinked only when the scanned unit fits exactly one stored unit and that stored
/// unit fits exactly one scanned unit.
pub(crate) fn match_moved(
    scanned: &[Probe],
    stored: &[Probe],
    open_scanned: &[usize],
    open_stored: &[usize],
) -> Moved {
    let mut fits: Vec<(usize, usize)> = Vec::new();
    for &i in open_scanned {
        for &j in open_stored {
            if looks_moved(&scanned[i], &stored[j]) {
                fits.push((i, j));
            }
        }
    }
    let mut out = Moved::default();
    for &(i, j) in &fits {
        let per_scanned = fits.iter().filter(|f| f.0 == i).count();
        let per_stored = fits.iter().filter(|f| f.1 == j).count();
        if per_scanned == 1 && per_stored == 1 {
            out.relinks.push((i, j));
        } else {
            out.ambiguous.push((i, j));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(videos: &[&str], group: &str, title: Option<&str>, content: &[(u64, i64)]) -> Probe {
        Probe {
            videos: videos.iter().map(|v| (*v).to_owned()).collect(),
            content: content.to_vec(),
            group: group.to_owned(),
            title_key: title.map(str::to_owned),
            solo: true,
        }
    }

    #[test]
    fn the_same_video_path_pairs_first() {
        let scanned = [probe(&["a/x.mkv"], "a", None, &[(10, 1)])];
        let stored = [
            probe(&["a/y.mkv"], "a", None, &[(10, 1)]),
            probe(&["a/x.mkv"], "a", None, &[(99, 9)]),
        ];
        assert_eq!(match_units(&scanned, &stored, false), vec![Some(1)]);
    }

    #[test]
    fn a_renamed_file_pairs_by_size_and_time_inside_its_group_only() {
        let scanned = [
            probe(&["a/new.mkv"], "a", None, &[(10, 1)]),
            probe(&["b/new.mkv"], "b", None, &[(10, 1)]),
        ];
        let stored = [probe(&["a/old.mkv"], "a", None, &[(10, 1)])];
        assert_eq!(match_units(&scanned, &stored, false), vec![Some(0), None]);
    }

    #[test]
    fn the_title_key_pairs_when_the_content_differs() {
        let scanned = [probe(&["m/new.mkv"], "m", Some("heat 1995"), &[(5, 5)])];
        let stored = [probe(&["m/old.mkv"], "m", Some("heat 1995"), &[(9, 9)])];
        assert_eq!(match_units(&scanned, &stored, false), vec![Some(0)]);
    }

    #[test]
    fn the_last_two_in_a_group_pair_only_when_allowed() {
        let scanned = [probe(&["m/new.mkv"], "m", Some("a"), &[(5, 5)])];
        let stored = [probe(&["m/old.mkv"], "m", Some("b"), &[(9, 9)])];
        assert_eq!(match_units(&scanned, &stored, false), vec![None]);
        assert_eq!(match_units(&scanned, &stored, true), vec![Some(0)]);
    }

    #[test]
    fn two_candidates_left_stay_unpaired() {
        let scanned = [
            probe(&["m/n1.mkv"], "m", Some("a"), &[(1, 1)]),
            probe(&["m/n2.mkv"], "m", Some("b"), &[(2, 2)]),
        ];
        let stored = [probe(&["m/o1.mkv"], "m", Some("c"), &[(3, 3)])];
        assert_eq!(match_units(&scanned, &stored, true), vec![None, None]);
    }

    #[test]
    fn a_stored_unit_is_paired_once() {
        let scanned = [
            probe(&["a/x.mkv"], "a", None, &[]),
            probe(&["a/x.mkv"], "a", None, &[]),
        ];
        let stored = [probe(&["a/x.mkv"], "a", None, &[])];
        assert_eq!(match_units(&scanned, &stored, false), vec![Some(0), None]);
    }

    #[test]
    fn a_moved_folder_pairs_by_size_and_name_or_title() {
        let scanned = [probe(&["b/heat.mkv"], "b", Some("heat|1995"), &[(10, 1)])];
        let stored = [probe(&["a/heat.mkv"], "a", Some("x|1995"), &[(10, 7)])];
        let moved = match_moved(&scanned, &stored, &[0], &[0]);
        assert_eq!(moved.relinks, vec![(0, 0)]);
        // A renamed file keeps the title key.
        let scanned = [probe(
            &["b/Heat (1995).mkv"],
            "b",
            Some("heat|1995"),
            &[(10, 1)],
        )];
        let stored = [probe(
            &["a/Heat.1995.dvdrip.mkv"],
            "a",
            Some("heat|1995"),
            &[(10, 7)],
        )];
        assert_eq!(
            match_moved(&scanned, &stored, &[0], &[0]).relinks,
            vec![(0, 0)]
        );
    }

    #[test]
    fn a_different_size_or_nothing_in_common_never_relinks() {
        let scanned = [probe(&["b/heat.mkv"], "b", Some("heat|1995"), &[(11, 1)])];
        let stored = [probe(&["a/heat.mkv"], "a", Some("heat|1995"), &[(10, 1)])];
        assert_eq!(match_moved(&scanned, &stored, &[0], &[0]), Moved::default());
        let scanned = [probe(&["b/one.mkv"], "b", Some("a|2000"), &[(10, 1)])];
        let stored = [probe(&["a/two.mkv"], "a", Some("b|2001"), &[(10, 1)])];
        assert_eq!(match_moved(&scanned, &stored, &[0], &[0]), Moved::default());
    }

    #[test]
    fn two_fits_are_ambiguous_and_relink_nothing() {
        let scanned = [probe(&["b/heat.mkv"], "b", Some("heat|1995"), &[(10, 1)])];
        let stored = [
            probe(&["a/heat.mkv"], "a", Some("heat|1995"), &[(10, 1)]),
            probe(&["c/heat.mkv"], "c", Some("heat|1995"), &[(10, 1)]),
        ];
        let moved = match_moved(&scanned, &stored, &[0], &[0, 1]);
        assert!(moved.relinks.is_empty());
        assert_eq!(moved.ambiguous, vec![(0, 0), (0, 1)]);
    }
}

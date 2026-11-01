//! Copy labels: copies of one movie that would end at the same name are told apart.
//!
//! Two copies of a film (the same movie in the library twice) render the same names. Before
//! [`detect`](super::detect) runs, [`label_copies`] finds the copies of one group whose new
//! folder or video path would be the same and adds ` - <label>` to their folder and their video
//! base name, so the NFO, the artwork named after the video and the subtitles follow. Each copy
//! keeps a folder of its own; copies that already share one folder clash only on their video
//! names, so only those get the label.
//!
//! The label is the copy's resolution when that tells the clashing copies apart; otherwise,
//! among the copies of one resolution, the resolution and the first of HDR, source and lead
//! audio that differs (`2160p DV`). Copies with the same label are numbered by library id
//! (`2160p copy 2`); when the copies do not differ at all, the oldest keeps the plain name and
//! the others become `copy 2`, `copy 3`. The pass is a pure function of the plans and the
//! copies' facts, so the same library always gets the same names, and a copy whose clash is gone
//! simply renders its plain name again.

use std::collections::{BTreeMap, BTreeSet};

use cv_domain::MediaFileType;

use super::resolve::{change_base, change_folder};
use super::{BatchItem, ConflictOptions};
use crate::clean::{Place, create_destination};
use crate::plan::{Fate, FolderChange};

/// What a copy's label is chosen from, as short names (`2160p`, `DV`, `WEB-DL`, `TrueHD 7.1`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyAttributes {
    /// `2160p`, `1080p`, `720p` or `SD`.
    pub resolution: String,
    /// `DV`, `HDR10+`, `HDR10`, `HLG` or `SDR`.
    pub hdr: String,
    /// `BluRay`, `Remux`, `WEB-DL`, `WEBRip`, `HDTV`, `DVD`, another name, or empty.
    pub source: String,
    /// The lead audio track: `Atmos`, `DTS-HD MA`, `TrueHD 7.1`, `AAC 5.1`, ...
    pub audio: String,
}

impl CopyAttributes {
    /// The attributes after the resolution, in the order they are tried.
    fn details(&self) -> [&str; 3] {
        [&self.hdr, &self.source, &self.audio]
    }
}

/// A movie of the batch that belongs to a group of copies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyMember {
    /// The group: the same for every copy of one film.
    pub group: i64,
    /// The library id, which orders identical copies.
    pub library_id: i64,
    pub attributes: CopyAttributes,
}

/// Labels the clashing copies of `items`. `members` holds the copies among them, by
/// [`BatchItem::id`]; items without an entry are left alone. Returns the label given to each
/// labelled item, by id.
pub fn label_copies(
    items: &mut [BatchItem],
    members: &BTreeMap<String, CopyMember>,
    opts: &ConflictOptions,
) -> BTreeMap<String, String> {
    let mut groups: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(member) = members.get(&item.id) {
            groups.entry(member.group).or_default().push(index);
        }
    }
    let mut labels = BTreeMap::new();
    for indices in groups.values().filter(|g| g.len() > 1) {
        for mut set in clashing_sets(items, indices, opts) {
            set.sort_by_key(|(index, _)| members[&items[*index].id].library_id);
            let attributes: Vec<&CopyAttributes> = set
                .iter()
                .map(|(index, _)| &members[&items[*index].id].attributes)
                .collect();
            for ((index, folder_too), label) in set.iter().zip(choose(&attributes)) {
                let Some(label) = label else { continue };
                let label =
                    create_destination(&label, Place::File, &opts.name_options, &opts.space);
                if label.is_empty() {
                    continue;
                }
                apply(&mut items[*index], &label, *folder_too);
                if *folder_too && holds_a_copy(items, *index, indices, opts) {
                    // Renaming the folder would carry the other copy along: move only this
                    // copy's own files into its labelled folder.
                    items[*index].folder_change = FolderChange::MoveFiles;
                }
                labels.insert(items[*index].id.clone(), label);
            }
        }
    }
    labels
}

/// Whether another copy of the group lives inside the old folder of `items[index]` (a copy kept
/// in a subfolder of its twin's folder).
fn holds_a_copy(
    items: &[BatchItem],
    index: usize,
    group: &[usize],
    opts: &ConflictOptions,
) -> bool {
    let item = &items[index];
    if item.folder_change != FolderChange::Rename || item.old_folder.is_empty() {
        return false;
    }
    let folder = format!("{}/", opts.key(&item.root, &item.old_folder));
    group.iter().filter(|&&other| other != index).any(|&other| {
        let other = &items[other];
        format!("{}/", opts.key(&other.root, &other.old_folder)).starts_with(&folder)
    })
}

/// The sets of copies of one group that clash, each member with whether its folder clashes too
/// (else only its video name does).
fn clashing_sets(
    items: &[BatchItem],
    indices: &[usize],
    opts: &ConflictOptions,
) -> Vec<Vec<(usize, bool)>> {
    let folder_key = |item: &BatchItem| opts.key(&item.root, &item.new_folder);
    let old_folder_key = |item: &BatchItem| opts.key(&item.root, &item.old_folder);
    let videos = |item: &BatchItem| -> BTreeSet<String> {
        item.files
            .iter()
            .filter(|f| f.kind == MediaFileType::Video)
            .filter_map(|f| match &f.fate {
                Fate::Carried { to } => to.first().cloned(),
                Fate::LeftBehind => Some(f.from.clone()),
                Fate::Trash => None,
            })
            .map(|path| opts.key(&item.root, &path))
            .collect()
    };

    let mut root: Vec<usize> = (0..indices.len()).collect();
    fn find(root: &mut [usize], mut i: usize) -> usize {
        while root[i] != i {
            root[i] = root[root[i]];
            i = root[i];
        }
        i
    }
    let mut folder_clash = vec![false; indices.len()];
    for a in 0..indices.len() {
        for b in a + 1..indices.len() {
            let (x, y) = (&items[indices[a]], &items[indices[b]]);
            // Copies that already share one folder stay in it: only their video names clash.
            let folders = !x.new_folder.is_empty()
                && !x.shares_folder
                && !y.shares_folder
                && folder_key(x) == folder_key(y)
                && old_folder_key(x) != old_folder_key(y);
            let video = !videos(x).is_disjoint(&videos(y));
            if folders || video {
                let (ra, rb) = (find(&mut root, a), find(&mut root, b));
                root[rb] = ra;
            }
            if folders {
                folder_clash[a] = true;
                folder_clash[b] = true;
            }
        }
    }
    let mut sets: BTreeMap<usize, Vec<(usize, bool)>> = BTreeMap::new();
    for position in 0..indices.len() {
        let set = find(&mut root, position);
        sets.entry(set)
            .or_default()
            .push((indices[position], folder_clash[position]));
    }
    sets.into_values().filter(|set| set.len() > 1).collect()
}

/// The label of each copy of a clashing set (in library id order); `None` keeps the plain name.
fn choose(copies: &[&CopyAttributes]) -> Vec<Option<String>> {
    if copies.windows(2).all(|pair| pair[0] == pair[1]) {
        return (0..copies.len())
            .map(|n| (n > 0).then(|| format!("copy {}", n + 1)))
            .collect();
    }
    let mut labels: Vec<String> = copies.iter().map(|c| c.resolution.clone()).collect();
    for (position, copy) in copies.iter().enumerate() {
        let same_resolution: Vec<&CopyAttributes> = copies
            .iter()
            .copied()
            .filter(|other| other.resolution == copy.resolution)
            .collect();
        if same_resolution.len() < 2 {
            continue;
        }
        let differing = (0..3).find(|&field| {
            same_resolution
                .iter()
                .any(|other| other.details()[field] != copy.details()[field])
        });
        if let Some(field) = differing {
            labels[position] = join(&copy.resolution, copy.details()[field]);
        }
    }
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    labels
        .into_iter()
        .map(|label| {
            let count = seen.entry(label.clone()).or_default();
            *count += 1;
            Some(if *count == 1 {
                label
            } else {
                join(&label, &format!("copy {count}"))
            })
        })
        .collect()
}

fn join(first: &str, second: &str) -> String {
    [first.trim(), second.trim()]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Adds ` - <label>` to the item's video base name, and to its folder when that clashes too.
fn apply(item: &mut BatchItem, label: &str, folder_too: bool) {
    if folder_too {
        let folder = format!("{} - {label}", item.new_folder);
        change_folder(item, &folder);
    }
    if let Some(base) = item.base_name.clone().filter(|b| !b.is_empty()) {
        change_base(item, &format!("{base} - {label}"));
    }
}

#[cfg(test)]
mod tests {
    use cv_domain::Item;

    use super::super::{
        ConflictKind, MemoryDisk, NameOptions, PathLimits, SpaceSubstitution, detect,
    };
    use super::*;
    use crate::movie::{MovieInput, MovieRenamer};
    use crate::options::MovieSettings;
    use crate::plan::PlanFile;

    const ROOT: &str = "D:/Movies";

    fn opts() -> ConflictOptions {
        ConflictOptions {
            case_insensitive: true,
            limits: PathLimits::windows_long_paths(),
            allow_merge: false,
            name_options: NameOptions::default(),
            space: SpaceSubstitution::default(),
        }
    }

    /// Example Movie (2020) planned by the real planner from `folder/<video>`.
    fn planned(id: &str, root: &str, folder: &str, video: &str) -> BatchItem {
        planned_by(MovieSettings::default(), id, root, folder, video)
    }

    fn planned_by(
        settings: MovieSettings,
        id: &str,
        root: &str,
        folder: &str,
        video: &str,
    ) -> BatchItem {
        let mut movie = Item::movie("Example Movie");
        movie.core.year = 2020;
        let stem = video.trim_end_matches(".mkv");
        let files = vec![
            PlanFile::new(format!("{folder}/{video}"), MediaFileType::Video),
            PlanFile::new(format!("{folder}/{stem}.nfo"), MediaFileType::Nfo),
            PlanFile::new(format!("{folder}/{stem}.srt"), MediaFileType::Subtitle),
        ];
        let plan = MovieRenamer::new(settings)
            .plan(&MovieInput {
                item: &movie,
                set: None,
                folder,
                files: &files,
            })
            .expect("plans");
        BatchItem::from_movie(id, root, &plan)
    }

    fn copy(id: &str, folder: &str) -> BatchItem {
        planned(id, ROOT, folder, "movie.mkv")
    }

    fn facts(resolution: &str, hdr: &str, source: &str, audio: &str) -> CopyAttributes {
        CopyAttributes {
            resolution: resolution.into(),
            hdr: hdr.into(),
            source: source.into(),
            audio: audio.into(),
        }
    }

    fn members(copies: &[(&str, CopyAttributes)]) -> BTreeMap<String, CopyMember> {
        copies
            .iter()
            .map(|(id, attributes)| {
                (
                    (*id).to_owned(),
                    CopyMember {
                        group: 1,
                        library_id: id.parse().unwrap(),
                        attributes: attributes.clone(),
                    },
                )
            })
            .collect()
    }

    fn targets(item: &BatchItem) -> Vec<String> {
        item.files
            .iter()
            .flat_map(|f| match &f.fate {
                Fate::Carried { to } => to.clone(),
                _ => Vec::new(),
            })
            .collect()
    }

    fn labelled(
        items: &mut [BatchItem],
        copies: &[(&str, CopyAttributes)],
    ) -> BTreeMap<String, String> {
        label_copies(items, &members(copies), &opts())
    }

    #[test]
    fn resolution_alone_tells_two_copies_apart() {
        let mut items = vec![copy("1", "dl-1"), copy("2", "dl-2")];
        let labels = labelled(
            &mut items,
            &[
                ("1", facts("2160p", "SDR", "BluRay", "AAC 5.1")),
                ("2", facts("1080p", "SDR", "WEB-DL", "AAC 5.1")),
            ],
        );

        assert_eq!(labels["1"], "2160p");
        assert_eq!(labels["2"], "1080p");
        assert_eq!(
            targets(&items[0]),
            vec![
                "Example Movie (2020) - 2160p/Example Movie (2020) - 2160p.mkv",
                "Example Movie (2020) - 2160p/Example Movie (2020) - 2160p.nfo",
                "Example Movie (2020) - 2160p/Example Movie (2020) - 2160p.srt",
            ]
        );
        assert_eq!(items[1].new_folder, "Example Movie (2020) - 1080p");
        assert!(detect(&items, &MemoryDisk::new(true), &opts()).is_empty());
    }

    #[test]
    fn hdr_then_source_then_audio_tell_one_resolution_apart() {
        let mut items = vec![copy("1", "a"), copy("2", "b")];
        let labels = labelled(
            &mut items,
            &[
                ("1", facts("2160p", "DV", "BluRay", "Atmos")),
                ("2", facts("2160p", "HDR10", "BluRay", "AAC 5.1")),
            ],
        );
        assert_eq!(
            (labels["1"].as_str(), labels["2"].as_str()),
            ("2160p DV", "2160p HDR10")
        );

        let mut items = vec![copy("1", "a"), copy("2", "b")];
        let labels = labelled(
            &mut items,
            &[
                ("1", facts("2160p", "HDR10", "BluRay", "Atmos")),
                ("2", facts("2160p", "HDR10", "WEB-DL", "Atmos")),
            ],
        );
        assert_eq!(
            (labels["1"].as_str(), labels["2"].as_str()),
            ("2160p BluRay", "2160p WEB-DL")
        );

        let mut items = vec![copy("1", "a"), copy("2", "b")];
        let labels = labelled(
            &mut items,
            &[
                ("1", facts("1080p", "SDR", "WEB-DL", "AAC 2.0")),
                ("2", facts("1080p", "SDR", "WEB-DL", "AAC 5.1")),
            ],
        );
        assert_eq!(
            (labels["1"].as_str(), labels["2"].as_str()),
            ("1080p AAC 2.0", "1080p AAC 5.1")
        );
    }

    #[test]
    fn identical_copies_are_numbered_and_the_oldest_keeps_the_plain_name() {
        let same = facts("1080p", "SDR", "WEB-DL", "AAC 5.1");
        let mut items = vec![copy("9", "b"), copy("4", "a")];
        let labels = labelled(&mut items, &[("9", same.clone()), ("4", same)]);

        assert_eq!(labels.len(), 1);
        assert_eq!(labels["9"], "copy 2");
        assert_eq!(items[1].new_folder, "Example Movie (2020)");
        assert_eq!(items[0].new_folder, "Example Movie (2020) - copy 2");
        assert!(detect(&items, &MemoryDisk::new(true), &opts()).is_empty());
    }

    #[test]
    fn a_repeated_label_among_three_copies_is_numbered() {
        let mut items = vec![copy("1", "a"), copy("2", "b"), copy("3", "c")];
        let labels = labelled(
            &mut items,
            &[
                ("1", facts("2160p", "DV", "BluRay", "Atmos")),
                ("2", facts("2160p", "DV", "BluRay", "Atmos")),
                ("3", facts("1080p", "SDR", "WEB-DL", "AAC 5.1")),
            ],
        );

        assert_eq!(labels["1"], "2160p");
        assert_eq!(labels["2"], "2160p copy 2");
        assert_eq!(labels["3"], "1080p");
        assert!(detect(&items, &MemoryDisk::new(true), &opts()).is_empty());
    }

    #[test]
    fn copies_in_different_library_folders_keep_the_plain_name() {
        let mut items = vec![
            planned("1", "D:/Movies", "dl", "movie.mkv"),
            planned("2", "E:/Films", "dl", "movie.mkv"),
        ];
        let labels = labelled(
            &mut items,
            &[
                ("1", facts("2160p", "SDR", "", "")),
                ("2", facts("1080p", "SDR", "", "")),
            ],
        );

        assert!(labels.is_empty());
        assert_eq!(items[0].new_folder, "Example Movie (2020)");
        assert_eq!(items[1].new_folder, "Example Movie (2020)");
    }

    #[test]
    fn copies_sharing_one_folder_are_labelled_on_the_video_only() {
        let mut items = vec![
            planned("1", ROOT, "Mixed", "Example.1080p.mkv"),
            planned("2", ROOT, "Mixed", "Example.2160p.mkv"),
        ];
        for item in &mut items {
            // Folder renaming is off: both stay in Mixed.
            let folder = item.old_folder.clone();
            change_folder(item, &folder);
        }
        let labels = labelled(
            &mut items,
            &[
                ("1", facts("1080p", "SDR", "", "")),
                ("2", facts("2160p", "SDR", "", "")),
            ],
        );

        assert_eq!(labels.len(), 2);
        assert_eq!(items[0].new_folder, "Mixed");
        assert_eq!(items[0].folder_change, FolderChange::Unchanged);
        assert_eq!(
            targets(&items[1])[0],
            "Mixed/Example Movie (2020) - 2160p.mkv"
        );
        assert!(detect(&items, &MemoryDisk::new(true), &opts()).is_empty());
    }

    #[test]
    fn different_movies_still_raise_the_duplicate_conflict() {
        let mut items = vec![copy("1", "a"), copy("2", "b")];
        // Neither is a copy of the other: no member entries.
        let labels = labelled(&mut items, &[]);

        assert!(labels.is_empty());
        let report = detect(&items, &MemoryDisk::new(true), &opts());
        assert!(
            report
                .conflicts
                .iter()
                .any(|c| c.kind == ConflictKind::DuplicateDestination)
        );
    }

    #[test]
    fn labels_are_stable_and_a_labelled_copy_plans_as_tidy() {
        let copies = [
            ("1", facts("2160p", "SDR", "", "")),
            ("2", facts("1080p", "SDR", "", "")),
        ];
        let mut first = vec![copy("1", "a"), copy("2", "b")];
        let mut again = vec![copy("1", "a"), copy("2", "b")];
        assert_eq!(labelled(&mut first, &copies), labelled(&mut again, &copies));
        assert_eq!(first, again);

        // The copies as the first rename left them: planned again, nothing changes.
        let mut renamed = vec![
            planned(
                "1",
                ROOT,
                "Example Movie (2020) - 2160p",
                "Example Movie (2020) - 2160p.mkv",
            ),
            planned(
                "2",
                ROOT,
                "Example Movie (2020) - 1080p",
                "Example Movie (2020) - 1080p.mkv",
            ),
        ];
        labelled(&mut renamed, &copies);
        for item in &renamed {
            assert_eq!(item.folder_change, FolderChange::Unchanged, "{item:?}");
            assert!(item.files.iter().all(|f| f.is_unchanged()), "{item:?}");
        }
    }

    #[test]
    fn a_copy_without_its_twin_renders_the_plain_name() {
        let mut items = vec![planned(
            "1",
            ROOT,
            "Example Movie (2020) - 2160p",
            "Example Movie (2020) - 2160p.mkv",
        )];
        let labels = labelled(&mut items, &[("1", facts("2160p", "SDR", "", ""))]);

        assert!(labels.is_empty());
        assert_eq!(items[0].new_folder, "Example Movie (2020)");
    }

    #[test]
    fn a_copy_whose_folder_holds_its_twin_moves_its_own_files_only() {
        // Movie folders straight under the library folder, wherever they were.
        let flat = |id: &str, folder: &str| {
            let settings = MovieSettings {
                keep_subfolders: false,
                ..MovieSettings::default()
            };
            planned_by(settings, id, ROOT, folder, "movie.mkv")
        };
        let mut items = vec![flat("1", "dl"), flat("2", "dl/other copy")];
        // Alone, the outer copy would rename its folder, and the twin inside would go with it.
        assert_eq!(items[0].folder_change, FolderChange::Rename);
        labelled(
            &mut items,
            &[
                ("1", facts("2160p", "SDR", "BluRay", "AAC 5.1")),
                ("2", facts("1080p", "SDR", "WEB-DL", "AAC 5.1")),
            ],
        );

        assert_eq!(items[0].new_folder, "Example Movie (2020) - 2160p");
        assert_eq!(items[0].folder_change, FolderChange::MoveFiles);
        assert_eq!(items[1].new_folder, "Example Movie (2020) - 1080p");
        assert_ne!(items[1].folder_change, FolderChange::MoveFiles);
        assert!(detect(&items, &MemoryDisk::new(true), &opts()).is_empty());
    }
}

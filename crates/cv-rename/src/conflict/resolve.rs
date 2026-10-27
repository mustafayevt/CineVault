//! Resolutions: what the user can choose for a conflict, and what it does to the plans.

use std::collections::{BTreeMap, BTreeSet};

use super::{BatchItem, ConflictKind, ConflictOptions, Disk, detect};
use crate::clean::{Place, create_destination};
use crate::error::RenameError;
use crate::plan::{Fate, FolderChange, PlannedFile};

/// What the user chose for an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Put the items that collide side by side in one folder: the names built on the video name
    /// get ` - <label>` added (the item's [`BatchItem::version_label`], or `Version <n>`). Files
    /// with fixed names (`poster.jpg`) cannot exist twice in the folder; of the twins, the first
    /// item keeps its file and the others' copies go to the trash folder.
    KeepAsVersions,
    /// Leave the item out of the batch.
    Skip,
    /// Rename the destination.
    Edit(NameEdit),
}

impl Resolution {
    pub fn kind(&self) -> ResolutionKind {
        match self {
            Resolution::KeepAsVersions => ResolutionKind::KeepAsVersions,
            Resolution::Skip => ResolutionKind::Skip,
            Resolution::Edit(_) => ResolutionKind::Edit,
        }
    }
}

/// The kinds of resolution, for offering them in the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionKind {
    KeepAsVersions,
    Skip,
    Edit,
}

/// A new name for the destination. A name is cleaned like a rendered one; a folder may hold `/`
/// for a sub-folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameEdit {
    /// The new destination folder, relative to the data source root.
    pub folder: Option<String>,
    /// The new base name of the video files and the files named after them (movies only).
    pub base: Option<String>,
}

/// The batch after the resolutions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The items that stay in the batch, with their plans rewritten. Run [`detect`] on them
    /// again.
    pub items: Vec<BatchItem>,
    /// The ids of the items left out.
    pub skipped: Vec<String>,
}

/// Applies the user's choices, keyed by item id. Items without a choice stay as they are.
pub fn resolve(
    items: &[BatchItem],
    choices: &BTreeMap<String, Resolution>,
    disk: &dyn Disk,
    opts: &ConflictOptions,
) -> Result<Resolved, RenameError> {
    let mut skipped = Vec::new();
    let mut kept: Vec<BatchItem> = Vec::new();
    for item in items {
        match choices.get(&item.id) {
            Some(Resolution::Skip) => skipped.push(item.id.clone()),
            Some(Resolution::Edit(edit)) => {
                let mut item = item.clone();
                edit_item(&mut item, edit, opts)?;
                kept.push(item);
            }
            _ => kept.push(item.clone()),
        }
    }
    keep_as_versions(&mut kept, choices, disk, opts)?;
    Ok(Resolved {
        items: kept,
        skipped,
    })
}

fn edit_item(
    item: &mut BatchItem,
    edit: &NameEdit,
    opts: &ConflictOptions,
) -> Result<(), RenameError> {
    let clean = |text: &str, place: Place| {
        let cleaned = create_destination(text, place, &opts.name_options, &opts.space);
        let cleaned = cleaned.trim_matches('/').to_owned();
        if cleaned.is_empty() {
            Err(RenameError::EmptyEditedName {
                item: item.id.clone(),
            })
        } else {
            Ok(cleaned)
        }
    };
    let folder = edit
        .folder
        .as_deref()
        .map(|f| clean(f, Place::Folder))
        .transpose()?;
    let base = edit
        .base
        .as_deref()
        .map(|b| clean(b, Place::File))
        .transpose()?;
    if base.is_some() && item.base_name.as_deref().is_none_or(str::is_empty) {
        return Err(RenameError::ResolutionNotApplicable {
            item: item.id.clone(),
        });
    }
    if let Some(folder) = folder {
        change_folder(item, &folder);
    }
    if let Some(base) = base {
        change_base(item, &base);
    }
    Ok(())
}

/// Moves everything that ends in the item's new folder into `folder`.
fn change_folder(item: &mut BatchItem, folder: &str) {
    let old = item.new_folder.clone();
    if old == folder {
        return;
    }
    let map = |target: &str| -> String {
        if old.is_empty() {
            format!("{folder}/{target}")
        } else if target == old {
            folder.to_owned()
        } else if let Some(rest) = target.strip_prefix(&format!("{old}/")) {
            format!("{folder}/{rest}")
        } else {
            target.to_owned()
        }
    };
    for_each_target(&mut item.files, |target| map(target));
    item.new_folder = folder.to_owned();
    item.folder_change = if folder == item.old_folder {
        FolderChange::Unchanged
    } else if item.folder_change == FolderChange::Unchanged {
        FolderChange::MoveFiles
    } else {
        item.folder_change
    };
}

/// Replaces the base name at the start of the file names the item writes.
fn change_base(item: &mut BatchItem, base: &str) {
    let Some(old) = item.base_name.clone().filter(|b| !b.is_empty()) else {
        return;
    };
    for_each_target(&mut item.files, |target| match target.rsplit_once('/') {
        Some((dir, name)) => match name.strip_prefix(old.as_str()) {
            Some(rest) => format!("{dir}/{base}{rest}"),
            None => target.to_owned(),
        },
        None => match target.strip_prefix(old.as_str()) {
            Some(rest) => format!("{base}{rest}"),
            None => target.to_owned(),
        },
    });
    item.base_name = Some(base.to_owned());
}

fn for_each_target(files: &mut [PlannedFile], mut map: impl FnMut(&str) -> String) {
    for file in files {
        if let Fate::Carried { to } = &mut file.fate {
            for target in to {
                *target = map(target);
            }
        }
    }
}

/// Keeps the items that chose it side by side, one label each.
fn keep_as_versions(
    items: &mut [BatchItem],
    choices: &BTreeMap<String, Resolution>,
    disk: &dyn Disk,
    opts: &ConflictOptions,
) -> Result<(), RenameError> {
    if !choices
        .values()
        .any(|choice| *choice == Resolution::KeepAsVersions)
    {
        return Ok(());
    }
    let report = detect(items, disk, opts);
    let index_of: BTreeMap<&str, usize> = items
        .iter()
        .enumerate()
        .map(|(i, item)| (item.id.as_str(), i))
        .collect();

    // Group the items whose destinations collide.
    let mut parent: Vec<usize> = (0..items.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut foreign_members: BTreeSet<usize> = BTreeSet::new();
    let mut colliding: BTreeSet<usize> = BTreeSet::new();
    for conflict in &report.conflicts {
        let is_foreign = matches!(conflict.kind, ConflictKind::ForeignDestination { .. });
        if !is_foreign && conflict.kind != ConflictKind::DuplicateDestination {
            continue;
        }
        let members: Vec<usize> = conflict
            .items
            .iter()
            .filter_map(|id| index_of.get(id.as_str()).copied())
            .collect();
        for &m in &members {
            colliding.insert(m);
            if is_foreign {
                foreign_members.insert(m);
            }
        }
        if let Some((&first, rest)) = members.split_first() {
            for &other in rest {
                let (a, b) = (find(&mut parent, first), find(&mut parent, other));
                parent[b] = a;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &m in &colliding {
        let root = find(&mut parent, m);
        groups.entry(root).or_default().push(m);
    }

    for members in groups.values() {
        if !members
            .iter()
            .any(|&m| choices.get(&items[m].id) == Some(&Resolution::KeepAsVersions))
        {
            continue;
        }
        // An existing foreign item takes the first place.
        let offset = usize::from(members.iter().any(|m| foreign_members.contains(m)));
        let mut used: BTreeSet<String> = BTreeSet::new();
        let mut claimed: BTreeSet<String> = BTreeSet::new();
        for (position, &m) in members.iter().enumerate() {
            let chosen = choices.get(&items[m].id) == Some(&Resolution::KeepAsVersions);
            if chosen {
                let number = position + 1 + offset;
                let wanted = items[m]
                    .version_label
                    .as_deref()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map_or_else(|| format!("Version {number}"), str::to_owned);
                let label =
                    create_destination(&wanted, Place::File, &opts.name_options, &opts.space);
                let label = if label.is_empty() {
                    format!("Version {number}")
                } else {
                    label
                };
                let label = if used.contains(&opts.fold(&label)) {
                    format!("{label} ({number})")
                } else {
                    label
                };
                used.insert(opts.fold(&label));
                let Some(old) = items[m].base_name.clone().filter(|b| !b.is_empty()) else {
                    return Err(RenameError::ResolutionNotApplicable {
                        item: items[m].id.clone(),
                    });
                };
                change_base(&mut items[m], &format!("{old} - {label}"));
                items[m].shares_folder = true;
                drop_claimed(&mut items[m], &claimed, opts);
            }
            for file in &items[m].files {
                match &file.fate {
                    Fate::Carried { to } => {
                        claimed.extend(to.iter().map(|t| opts.key(&items[m].root, t)));
                    }
                    Fate::LeftBehind => {
                        claimed.insert(opts.key(&items[m].root, &file.from));
                    }
                    Fate::Trash => {}
                }
            }
        }
    }
    Ok(())
}

/// Files with a fixed name that an earlier twin already writes are not needed twice: they go
/// to the trash folder. Videos are never dropped; they stay a conflict if they still collide.
fn drop_claimed(item: &mut BatchItem, claimed: &BTreeSet<String>, opts: &ConflictOptions) {
    let root = item.root.clone();
    for file in &mut item.files {
        if file.kind == cv_domain::MediaFileType::Video {
            continue;
        }
        let Fate::Carried { to } = &mut file.fate else {
            continue;
        };
        to.retain(|t| !claimed.contains(&opts.key(&root, t)));
        if to.is_empty() {
            file.fate = Fate::Trash;
        }
    }
}

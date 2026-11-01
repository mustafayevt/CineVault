//! Copy labels in a rename batch: the organize side of [`cv_rename::conflict::copies`].
//!
//! The batch's movies that belong to a group of copies are labelled before conflict detection.
//! The other copies of their groups take part as fixed participants: their plans are rendered
//! (never run) so a copy renamed alone still sees its twin. A group whose copies would clash
//! while one of them has no media info yet is held back, like any title whose name waits for its
//! video details, so a label is never made from missing data.

use std::collections::{BTreeMap, BTreeSet};

use cv_db::repo::copies::copy_groups;
use cv_db::{Database, DbError};
use cv_domain::ItemId;
use cv_rename::conflict::copies::{CopyAttributes, CopyMember, label_copies};
use cv_rename::conflict::{BatchItem, ConflictOptions};
use cv_rename::{MovieInput, MovieRenamer};

use super::load::{Loaded, load};
use crate::copies::label_attributes;

/// The copy group of every movie that has copies: the group's oldest member.
pub(super) fn group_of_movies(db: &Database) -> Result<BTreeMap<ItemId, ItemId>, DbError> {
    let groups = db.read(copy_groups)?;
    Ok(groups
        .into_iter()
        .flat_map(|group| {
            let id = group.id();
            group.members.into_iter().map(move |member| (member, id))
        })
        .collect())
}

/// Labels the clashing copies among `items`. `attributes` holds the label facts of the batch's
/// copies by item id (`None` while unread). Returns the ids of the batch items held back because
/// a copy they clash with has no media info yet; they are removed from `items`.
pub(super) fn separate_copies(
    db: &Database,
    items: &mut Vec<BatchItem>,
    group_of: &BTreeMap<ItemId, ItemId>,
    attributes: &BTreeMap<String, Option<CopyAttributes>>,
    renamer: &MovieRenamer,
    options: &ConflictOptions,
) -> Result<BTreeSet<String>, DbError> {
    let id_of = |key: &str| key.parse::<i64>().ok().map(ItemId);
    let groups: BTreeSet<ItemId> = items
        .iter()
        .filter_map(|item| id_of(&item.id).and_then(|id| group_of.get(&id)).copied())
        .collect();
    if groups.is_empty() {
        return Ok(BTreeSet::new());
    }

    // The other copies of those groups, planned but never run.
    let in_batch: BTreeSet<&str> = items.iter().map(|item| item.id.as_str()).collect();
    let others: Vec<ItemId> = group_of
        .iter()
        .filter(|(id, group)| {
            groups.contains(group) && !in_batch.contains(id.0.to_string().as_str())
        })
        .map(|(id, _)| *id)
        .collect();
    let mut facts = attributes.clone();
    let mut fixed: Vec<BatchItem> = Vec::new();
    for loaded in load(db, &others)? {
        let Loaded::Movie(movie) = loaded else {
            continue;
        };
        let input = MovieInput {
            item: &movie.item,
            set: movie.set.as_ref(),
            folder: &movie.folder,
            files: &movie.files,
        };
        let Ok(plan) = renamer.plan(&input) else {
            continue;
        };
        let key = movie.id.0.to_string();
        facts.insert(key.clone(), label_attributes(&movie.item));
        fixed.push(BatchItem::from_movie(key, &movie.root, &plan));
    }
    let fixed_ids: BTreeSet<String> = fixed.iter().map(|item| item.id.clone()).collect();
    let mut all: Vec<BatchItem> = std::mem::take(items);
    all.extend(fixed);

    let member = |key: &str, attributes: CopyAttributes| -> Option<(String, CopyMember)> {
        let id = id_of(key)?;
        Some((
            key.to_owned(),
            CopyMember {
                group: group_of.get(&id)?.0,
                library_id: id.0,
                attributes,
            },
        ))
    };
    // A group with an unread copy is labelled only to see whether it clashes; if it does, its
    // batch items wait for the video details.
    let unread_groups: BTreeSet<i64> = facts
        .iter()
        .filter(|(_, facts)| facts.is_none())
        .filter_map(|(key, _)| member(key, CopyAttributes::default()).map(|(_, m)| m.group))
        .collect();
    let mut held = BTreeSet::new();
    if !unread_groups.is_empty() {
        let probe: BTreeMap<String, CopyMember> = facts
            .keys()
            .filter_map(|key| member(key, CopyAttributes::default()))
            .filter(|(_, m)| unread_groups.contains(&m.group))
            .collect();
        let clashing: BTreeSet<i64> = label_copies(&mut all.clone(), &probe, options)
            .keys()
            .map(|key| probe[key].group)
            .collect();
        held = probe
            .iter()
            .filter(|(key, m)| clashing.contains(&m.group) && !fixed_ids.contains(*key))
            .map(|(key, _)| key.clone())
            .collect();
    }

    let members: BTreeMap<String, CopyMember> = facts
        .iter()
        .filter_map(|(key, facts)| member(key, facts.clone()?))
        .filter(|(_, m)| !unread_groups.contains(&m.group))
        .collect();
    let labels = label_copies(&mut all, &members, options);
    if !labels.is_empty() {
        tracing::debug!(labelled = labels.len(), "copies told apart by labels");
    }
    all.retain(|item| !fixed_ids.contains(&item.id) && !held.contains(&item.id));
    *items = all;
    Ok(held)
}

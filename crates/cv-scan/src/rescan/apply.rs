//! Writes a [`RescanPlan`] in one transaction.
//!
//! Stored items are read again inside the transaction and changed from that fresh copy, so edits
//! the user made while the walk was running are kept. Items deleted in the meantime are skipped.
//! Nothing here deletes a library row; stale fingerprints are the only rows removed.

use std::collections::HashMap;

use cv_db::DbResult;
use cv_db::repo::fingerprint::{delete_fingerprints, upsert_fingerprints};
use cv_db::repo::rename_journal::repoint_folder;
use cv_db::repo::{get_item, insert_item, set_new, update_item};
use cv_domain::{Item, ItemDetail, ItemId, MediaFile, MediaFileType};
use rusqlite::Connection;

use super::build::{
    directory_of, file_key, media_file_of, name_of, refresh_media_file, scanned_key,
};
use super::plan::{Patch, RescanPlan, ShowRef, UpdateUnit, Write};

/// What applying a plan did.
#[derive(Debug, Clone, Default)]
pub(crate) struct Applied {
    pub new_items: Vec<ItemId>,
    /// Of those, the movies and shows: the titles a scan reports as new.
    pub new_titles: usize,
    /// Of those, the episodes.
    pub new_episodes: usize,
    pub updated: Vec<ItemId>,
    pub needs_media_info: Vec<ItemId>,
}

fn lacks_media_info(item: &Item) -> bool {
    item.core
        .media_files
        .iter()
        .any(|f| f.kind == MediaFileType::Video && !f.has_media_info())
}

fn insert_new(conn: &Connection, item: &Item, out: &mut Applied, flag: bool) -> DbResult<ItemId> {
    let id = insert_item(conn, item)?;
    if flag {
        set_new(conn, &[id], true)?;
        out.new_items.push(id);
        match item.kind() {
            cv_domain::ItemKind::Movie | cv_domain::ItemKind::Show => out.new_titles += 1,
            cv_domain::ItemKind::Episode => out.new_episodes += 1,
            _ => {}
        }
    }
    if lacks_media_info(item) {
        out.needs_media_info.push(id);
    }
    Ok(id)
}

fn apply_update(conn: &Connection, unit: &UpdateUnit, out: &mut Applied) -> DbResult<()> {
    for id in &unit.ids {
        let Some(mut item) = get_item(conn, *id)? else {
            continue;
        };
        let mut old: HashMap<String, MediaFile> = item
            .core
            .media_files
            .drain(..)
            .map(|f| (file_key(&f.directory, &f.filename), f))
            .collect();
        item.core.media_files = unit
            .files
            .iter()
            .map(|planned| {
                let key = scanned_key(&planned.file);
                let key = unit.moved_from.get(&key).cloned().unwrap_or(key);
                let kept = planned.keep.then(|| old.remove(&key)).flatten();
                match kept {
                    Some(mut row) => {
                        refresh_media_file(&mut row, &planned.file);
                        if unit.path.is_some() {
                            // The file moved with its title: the row follows it.
                            row.directory = directory_of(&planned.file);
                            row.filename = name_of(&planned.file);
                        }
                        row
                    }
                    None => media_file_of(&planned.file),
                }
            })
            .collect();
        if let Some(path) = &unit.path {
            item.core.path.clone_from(path);
        }
        match (&mut item.detail, &unit.patch) {
            (
                ItemDetail::Movie(m),
                Patch::Movie {
                    stacked,
                    is_disc,
                    multi_movie_dir,
                },
            ) => {
                m.stacked = *stacked;
                m.is_disc = *is_disc;
                m.multi_movie_dir = *multi_movie_dir;
            }
            (ItemDetail::Episode(e), Patch::Episode { stacked, is_disc }) => {
                e.stacked = *stacked;
                e.is_disc = *is_disc;
            }
            _ => {}
        }
        update_item(conn, &item)?;
        if unit.flag_updated {
            set_new(conn, &[*id], true)?;
            out.updated.push(*id);
        }
        if lacks_media_info(&item) {
            out.needs_media_info.push(*id);
        }
    }
    Ok(())
}

/// Writes every change of `plan`: new items, changed file lists and the fingerprints.
pub(crate) fn apply_plan(conn: &Connection, plan: &RescanPlan) -> DbResult<Applied> {
    let mut out = Applied::default();
    for id in &plan.adopted {
        conn.execute(
            "UPDATE item SET data_source = ?1 WHERE id = ?2",
            rusqlite::params![plan.source.path, id.0],
        )?;
    }
    for write in &plan.writes {
        match write {
            Write::NewMovie(item) => {
                insert_new(conn, item, &mut out, true)?;
            }
            Write::Tree {
                show,
                seasons,
                episodes,
            } => {
                let show_id = match show {
                    ShowRef::New(item) => insert_new(conn, item, &mut out, true)?,
                    ShowRef::Existing(id) => *id,
                };
                for season in seasons {
                    let mut item = season.clone();
                    item.parent_id = Some(show_id);
                    insert_new(conn, &item, &mut out, false)?;
                }
                for episode in episodes {
                    let mut item = episode.clone();
                    item.parent_id = Some(show_id);
                    insert_new(conn, &item, &mut out, true)?;
                }
            }
            Write::Update(unit) => apply_update(conn, unit, &mut out)?,
            Write::MoveFolder { from, to } => {
                repoint_folder(conn, from, to)?;
            }
        }
    }
    upsert_fingerprints(conn, &plan.source.path, &plan.fp_upserts, plan.now_ms)?;
    delete_fingerprints(conn, &plan.fp_deletes)?;
    Ok(out)
}

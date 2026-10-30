//! The files named after a title that a delete takes with it (spec `title-deletion` "Delete
//! takes the files named after the title").
//!
//! A title with its own folder goes as that folder. A title in a folder other titles share, in
//! a library folder's root, or an episode goes file by file, and the library only knows the
//! files of the last Scan: a subtitle or poster dropped next to the video since then would be
//! left behind. So, at delete time, each folder that holds one of the title's videos is listed
//! (that folder only, no sub-folder) and a file is taken when the name rules of a Scan
//! ([`cv_scan::companion`]) give it to one of the title's videos:
//!
//! - the bases that compete for a name are the title's videos, every video the library records
//!   in the folder for any other item, and every other video on disk there;
//! - the longest base name the file's name continues wins; a tie with another owner leaves it;
//! - a video, a folder, a link and a file the library records for another item are never taken.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use cv_db::DbResult;
use cv_db::repo::library_ops::{OwnedFile, files_in_folder, lies_under};
use cv_domain::ItemId;
use cv_fs::paths::fold_name;
use cv_scan::companion::{NameOwner, is_plain_video, owner_by_name, video_bases};
use rusqlite::Connection;

/// A file on disk that goes with a title by its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Companion {
    pub(super) path: PathBuf,
    pub(super) size: u64,
}

/// What the library says about one folder that holds a video of the title.
#[derive(Debug, Clone, Default)]
pub(super) struct FolderFacts {
    folder: String,
    /// The title's own files there, folded by the platform's case rule.
    ours: HashSet<String>,
    /// The names of the title's videos there.
    our_videos: Vec<String>,
    /// Files other items record there, folded.
    others: HashSet<String>,
    /// The names of the videos other items record there.
    other_videos: Vec<String>,
}

/// Reads, for each folder that holds one of the title's videos, what the library records there.
/// `tree` is the title and everything below it, `files` its recorded files. With `within`
/// (the title's own folder), only folders inside it count; every folder must be inside
/// `source`, the title's library folder, or be its root.
pub(super) fn folder_facts(
    conn: &Connection,
    tree: &[ItemId],
    files: &[OwnedFile],
    within: Option<&str>,
    source: &str,
) -> DbResult<Vec<FolderFacts>> {
    let mut out: Vec<FolderFacts> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for file in files.iter().filter(|f| is_plain_video(&f.filename)) {
        let folder = file.directory.trim_end_matches(['\\', '/']);
        if folder.is_empty()
            || !lies_under(folder, source)
            || within.is_some_and(|own| !lies_under(folder, own))
            || !seen.insert(fold_name(&folder.replace('\\', "/")))
        {
            continue;
        }
        let mut facts = FolderFacts {
            folder: folder.to_owned(),
            ..FolderFacts::default()
        };
        for recorded in files_in_folder(conn, folder)? {
            let name = recorded.filename;
            let video = is_plain_video(&name);
            if tree.contains(&recorded.item_id) {
                facts.ours.insert(fold_name(&name));
                if video {
                    facts.our_videos.push(name);
                }
            } else {
                facts.others.insert(fold_name(&name));
                if video {
                    facts.other_videos.push(name);
                }
            }
        }
        out.push(facts);
    }
    Ok(out)
}

/// Who a base name belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Claim {
    Title,
    Other,
}

/// Lists each folder and returns the files that go with the title by name, in name order per
/// folder. A folder that cannot be listed gives nothing (the delete goes on with what the library
/// knows).
pub(super) fn find(facts: &[FolderFacts]) -> Vec<Companion> {
    let mut out = Vec::new();
    for folder in facts {
        out.extend(find_in(folder));
    }
    out
}

fn find_in(facts: &FolderFacts) -> Vec<Companion> {
    let folder = Path::new(&facts.folder);
    let Ok(listing) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    // Regular files only: a folder or a link is never taken by name.
    let mut names: Vec<(String, u64)> = listing
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let meta = entry.path().symlink_metadata().ok()?;
            let name = entry.file_name().into_string().ok()?;
            meta.is_file().then_some((name, meta.len()))
        })
        .collect();
    names.sort();

    let mut bases: Vec<(String, Claim)> = Vec::new();
    for video in &facts.our_videos {
        bases.extend(video_bases(video).into_iter().map(|b| (b, Claim::Title)));
    }
    for video in &facts.other_videos {
        bases.extend(video_bases(video).into_iter().map(|b| (b, Claim::Other)));
    }
    for (name, _) in &names {
        if is_plain_video(name) && !facts.ours.contains(&fold_name(name)) {
            bases.extend(video_bases(name).into_iter().map(|b| (b, Claim::Other)));
        }
    }

    names
        .into_iter()
        .filter(|(name, _)| {
            let folded = fold_name(name);
            !facts.ours.contains(&folded)
                && !facts.others.contains(&folded)
                && !is_plain_video(name)
                && owner_by_name(name, &bases) == NameOwner::One(Claim::Title)
        })
        .map(|(name, size)| Companion {
            path: folder.join(name),
            size,
        })
        .collect()
}

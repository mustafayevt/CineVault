//! Shows whose folder was renamed or moved outside the app.
//!
//! A show is matched to its stored row by folder, so a renamed show folder used to look like a new
//! show next to a stored one whose files are gone. This pairs the two when the fit is certain:
//! every episode video of the stored show is gone from disk, the scanned show holds videos of
//! exactly the same sizes (none empty), and either a video file name is shared or the title and
//! year are the same. A pair counts only when the scanned show fits exactly one gone show and that
//! show fits exactly one scanned show. The stored rows are then seen at the new folder (their
//! paths rebased), so the ordinary show planner can compare them with what the walk found.

use std::collections::HashMap;
use std::path::Path;

use cv_db::repo::fingerprint::{StoredFile, StoredItem};
use cv_domain::{ItemId, ItemKind, MediaFileType};

use super::build::{file_key, path_key};
use crate::walk::ScannedShow;

/// A stored show seen at the folder it moved to.
#[derive(Debug, Clone)]
pub(crate) struct MovedShow {
    /// The show row with its path and files rebased to the new folder.
    pub show: StoredItem,
    /// Its seasons and episodes, rebased the same way.
    pub kids: Vec<StoredItem>,
    /// The folder before and after, as stored and as scanned.
    pub from: String,
    pub to: String,
}

/// The video files of a show's stored episodes, each file once (one file may hold several
/// episodes).
fn stored_videos<'a>(kids: &[&'a StoredItem]) -> Vec<&'a StoredFile> {
    let mut seen: HashMap<String, &StoredFile> = HashMap::new();
    for kid in kids.iter().filter(|k| k.kind == ItemKind::Episode) {
        for file in kid.files.iter().filter(|f| f.kind == MediaFileType::Video) {
            seen.entry(file_key(&file.directory, &file.filename))
                .or_insert(file);
        }
    }
    seen.into_values().collect()
}

fn scanned_videos(show: &ScannedShow) -> Vec<(String, u64)> {
    let mut seen: HashMap<String, u64> = HashMap::new();
    for file in show
        .episodes
        .iter()
        .flat_map(|e| e.files.iter())
        .filter(|f| f.kind == MediaFileType::Video)
    {
        seen.entry(path_key(&file.path.to_string_lossy()))
            .or_insert(file.size_bytes);
    }
    seen.into_iter().collect()
}

fn name_of(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

fn simple(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether every stored episode video is gone from disk (and there is at least one).
fn videos_are_gone(videos: &[&StoredFile]) -> bool {
    !videos.is_empty()
        && videos
            .iter()
            .all(|f| std::fs::symlink_metadata(Path::new(&f.directory).join(&f.filename)).is_err())
}

/// Whether the scanned show looks like the stored one moved: the same video sizes and a shared
/// file name or the same title and year.
fn looks_moved(show: &ScannedShow, stored: &StoredItem, videos: &[&StoredFile]) -> bool {
    let scanned = scanned_videos(show);
    let mut want: Vec<u64> = videos.iter().map(|f| f.size_bytes).collect();
    let mut have: Vec<u64> = scanned.iter().map(|(_, size)| *size).collect();
    want.sort_unstable();
    have.sort_unstable();
    if want.is_empty() || want.contains(&0) || want != have {
        return false;
    }
    let same_name = scanned.iter().any(|(key, _)| {
        videos
            .iter()
            .any(|f| cv_fs::paths::same_name(name_of(key), &f.filename))
    });
    let same_title = show.year.is_some_and(|y| y > 0 && y == stored.year)
        && !simple(&show.title).is_empty()
        && simple(&show.title) == simple(&stored.title);
    same_name || same_title
}

/// `path` with the folder `from` replaced by `to`, when `path` is `from` or lies below it.
fn rebase(path: &str, from: &str, to: &str) -> Option<String> {
    let key = path_key(path);
    let from_key = path_key(from);
    let rest = key.strip_prefix(from_key.as_str())?;
    if !(rest.is_empty() || rest.starts_with('/')) {
        return None;
    }
    // Lower-casing keeps ASCII lengths; for the rest, count characters of the original.
    let tail: String = path
        .chars()
        .skip(from.trim_end_matches(['\\', '/']).chars().count())
        .collect();
    Some(format!("{}{tail}", to.trim_end_matches(['\\', '/'])))
}

fn rebased(item: &StoredItem, from: &str, to: &str) -> StoredItem {
    let mut out = item.clone();
    if let Some(path) = rebase(&item.path, from, to) {
        out.path = path;
    }
    for file in &mut out.files {
        if let Some(dir) = rebase(&file.directory, from, to) {
            file.directory = dir;
        }
    }
    out
}

/// The scanned shows (by index) that moved here from a stored show, with that show rebased.
/// `stored_shows` are the stored shows no scanned folder holds; `children` are the seasons and
/// episodes by show. Ambiguous fits are logged and left alone: the new folder becomes its own
/// show and the missing/duplicate review lists the stored one.
pub(crate) fn match_moved_shows(
    scanned: &[(usize, &ScannedShow)],
    stored_shows: &[&StoredItem],
    children: &HashMap<ItemId, Vec<&StoredItem>>,
) -> HashMap<usize, MovedShow> {
    struct Gone<'a> {
        stored: &'a StoredItem,
        kids: &'a [&'a StoredItem],
        videos: Vec<&'a StoredFile>,
    }
    let gone: Vec<Gone<'_>> = stored_shows
        .iter()
        .filter_map(|stored| {
            let kids = children.get(&stored.id).map_or(&[][..], Vec::as_slice);
            let videos = stored_videos(kids);
            videos_are_gone(&videos).then_some(Gone {
                stored,
                kids,
                videos,
            })
        })
        .collect();
    let mut fits: Vec<(usize, usize)> = Vec::new();
    for (n, (_, show)) in scanned.iter().enumerate() {
        for (j, g) in gone.iter().enumerate() {
            if looks_moved(show, g.stored, &g.videos) {
                fits.push((n, j));
            }
        }
    }
    let mut out = HashMap::new();
    for &(n, j) in &fits {
        let (index, show) = scanned[n];
        let g = &gone[j];
        let per_scanned = fits.iter().filter(|f| f.0 == n).count();
        let per_stored = fits.iter().filter(|f| f.1 == j).count();
        if per_scanned != 1 || per_stored != 1 {
            tracing::info!(
                scanned = %show.path.display(),
                stored = %g.stored.path,
                "a new show folder fits more than one missing show; both are kept"
            );
            continue;
        }
        let from = g.stored.path.clone();
        let to = show.path.to_string_lossy().into_owned();
        out.insert(
            index,
            MovedShow {
                show: rebased(g.stored, &from, &to),
                kids: g.kids.iter().map(|k| rebased(k, &from, &to)).collect(),
                from,
                to,
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_below_the_old_folder_follows_it_and_a_sibling_does_not() {
        assert_eq!(
            rebase(
                "D:\\TV\\Dark (2017)\\Season 01",
                "D:\\TV\\Dark (2017)",
                "D:\\TV\\Dark"
            ),
            Some("D:\\TV\\Dark\\Season 01".to_owned())
        );
        // The same folder spelled in another case follows it where file names ignore case
        // (Windows, macOS), and is another folder on Linux.
        let other_case = rebase("d:/tv/dark (2017)", "D:\\TV\\Dark (2017)", "D:\\TV\\Dark");
        if cv_fs::paths::CASE_SENSITIVE {
            assert_eq!(other_case, None);
        } else {
            assert_eq!(other_case, Some("D:\\TV\\Dark".to_owned()));
        }
        assert_eq!(
            rebase(
                "D:\\TV\\Dark (2017) 2\\x.mkv",
                "D:\\TV\\Dark (2017)",
                "D:\\TV\\Dark"
            ),
            None
        );
    }
}

//! Writing the outcome of a download into the library: the new image files, the remembered source
//! URLs and the content hashes.
//!
//! The library never loses a row here. Files that a run moved to the trash stop being listed as
//! the item's files; everything else about the item stays as it was.

use std::path::Path;
use std::time::UNIX_EPOCH;

use cv_db::repo::artwork_refs::{ArtworkRef, replace_extra_refs, set_artwork_ref};
use cv_db::repo::{get_item, update_item};
use cv_db::{Database, DbError, DbResult};
use cv_domain::{ItemId, MediaFile, MediaFileType};

use super::engine::{DownloadReport, WrittenImage};

/// Whether two paths are the same file: separators are ignored, and so is case except on Linux.
fn same_path(a: &Path, b: &Path) -> bool {
    cv_fs::paths::same_path(a, b)
}

fn file_entry(kind: MediaFileType, path: &Path) -> MediaFile {
    let directory = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut file = MediaFile::new(kind, directory, filename);
    if let Ok(meta) = std::fs::metadata(path) {
        file.size_bytes = meta.len();
        file.modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(0);
    }
    file
}

fn reference(image: &WrittenImage) -> ArtworkRef {
    ArtworkRef::fetched(
        image.kind,
        image.position,
        image.url.clone(),
        image.key.clone(),
        image.width,
        image.height,
    )
}

/// Stores what `report` says was written for the item: its image files, the source URL of each
/// image (the URL of a main image also goes to the item's artwork URLs, which the NFO may carry),
/// and the content hash and size.
///
/// # Errors
/// The item no longer exists or the database fails.
pub fn record(db: &Database, item_id: ItemId, report: &DownloadReport) -> DbResult<()> {
    if report.images.is_empty() && report.trashed.is_empty() {
        return Ok(());
    }
    let report = report.clone();
    db.write(move |tx| {
        let Some(mut item) = get_item(tx, item_id)? else {
            return Err(DbError::ItemNotFound(item_id.0));
        };

        // Files that were moved away stop being the item's files.
        let touched: Vec<MediaFileType> = report
            .images
            .iter()
            .map(|i| i.kind)
            .chain(report.replaced_extras.iter().copied())
            .collect();
        item.core
            .media_files
            .retain(|f| !(touched.contains(&f.kind) && !f.path().exists()));

        for image in &report.images {
            for path in &image.files {
                item.core
                    .media_files
                    .retain(|f| !same_path(&f.path(), path));
                item.core.media_files.push(file_entry(image.kind, path));
            }
            if image.position == 0 {
                item.core.artwork_urls.insert(image.kind, image.url.clone());
            }
        }
        update_item(tx, &item)?;

        for image in report.images.iter().filter(|i| i.position == 0) {
            set_artwork_ref(tx, item_id, &reference(image))?;
        }
        for kind in &report.replaced_extras {
            let set: Vec<ArtworkRef> = report
                .images
                .iter()
                .filter(|i| i.kind == *kind && i.position >= 1)
                .map(reference)
                .collect();
            replace_extra_refs(tx, item_id, *kind, &set)?;
        }
        for image in report
            .images
            .iter()
            .filter(|i| i.position >= 1 && !report.replaced_extras.contains(&i.kind))
        {
            set_artwork_ref(tx, item_id, &reference(image))?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_differing_only_in_case_are_one_file_only_where_the_platform_folds_case() {
        let (a, b) = (
            Path::new("/m/Heat/Poster.jpg"),
            Path::new(r"\m\Heat\poster.jpg"),
        );
        assert_eq!(same_path(a, b), !cv_fs::paths::CASE_SENSITIVE);
        assert!(same_path(a, Path::new(r"\m\Heat\Poster.jpg")));
    }
}

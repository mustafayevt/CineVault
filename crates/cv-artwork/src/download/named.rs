//! Writing one image to the files of a name list: season artwork and episode thumbnails.
//!
//! Spec: `artwork` (Download missing artwork), `tv-library` (season and episode artwork). The
//! caller says which relative names the image gets (`season01-poster.jpg`, `Season 1/folder.jpg`,
//! `S01E02-thumb.jpg`...) for a given extension, because the extension is only known once the
//! image is fetched. The data-safety rules are those of the other downloads:
//!
//! * an image that is there, under any extension, is kept unless the caller asks for
//!   replacement, and a replaced file moves to the trash folder, never into the void;
//! * every file is written next to its name and renamed into place;
//! * bytes that are not a picture are never written;
//! * nothing is written below a folder that is not there, so a moved or offline show cannot
//!   sprout a stray folder.
//!
//! The result is a [`DownloadReport`], so [`super::record::record`] stores the files and the
//! source URL on the season or episode exactly as it does for a movie.

use std::path::{Path, PathBuf};

use cv_domain::MediaFileType;

use super::engine::{DownloadReport, Failure, WrittenImage, load_image};
use super::fetch::ImageFetcher;
use super::files::{Stored, Trash, any_variant_exists, store, trash_other_variants};
use crate::thumb::ThumbKey;

/// The relative names of an image for an extension without the dot.
pub type NameList<'a> = &'a (dyn Fn(&str) -> Vec<String> + Sync);

/// One image to fetch and write.
pub struct NamedImage<'a> {
    /// What it is, for a failure message: `season 2 poster`, `S01E03 thumb`.
    pub what: &'a str,
    /// The artwork type the image is recorded under.
    pub kind: MediaFileType,
    pub url: &'a str,
    /// The folder the names are relative to: the show folder for a season image, the folder of
    /// the video for an episode thumbnail.
    pub base: &'a Path,
    pub names: NameList<'a>,
    /// The data source the show lives in; its trash folder takes what is replaced.
    pub data_source: Option<&'a Path>,
    /// Replace files that are there (the old ones go to the trash).
    pub replace: bool,
}

/// Fetches the image and writes it to every name. Nothing is fetched when every name is already
/// taken and `replace` is off.
pub async fn download_named_image(
    fetcher: &dyn ImageFetcher,
    request: &NamedImage<'_>,
) -> DownloadReport {
    let mut report = DownloadReport::default();
    if request.url.trim().is_empty() || !request.base.is_dir() {
        return report;
    }
    // Existing files are matched without the extension, so a complete set costs no download.
    let probe = absolute(request, "jpg");
    if probe.is_empty() {
        return report;
    }
    if !request.replace && probe.iter().all(|p| any_variant_exists(p)) {
        report.kept.extend(probe);
        return report;
    }
    let picture = match load_image(fetcher, request.url.trim()).await {
        Ok(picture) => picture,
        Err(reason) => {
            report.failures.push(Failure::new(request.what, reason));
            return report;
        }
    };
    let targets = absolute(request, picture.info.ext);
    let key = ThumbKey::of_bytes(&picture.bytes).as_str().to_owned();
    let trash = Trash::new(request.data_source, request.base);
    let shared = trash.clone();
    let (bytes, replace) = (picture.bytes, request.replace);
    let written = tokio::task::spawn_blocking(move || write_all(&targets, &bytes, replace, &trash))
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())));
    // Where each displaced file went, so a chosen image can be undone.
    report.trash_moves = shared.moves();
    match written {
        Ok(done) => {
            report.trashed.extend(done.trashed);
            report.kept.extend(done.kept);
            if !done.files.is_empty() {
                report.images.push(WrittenImage {
                    kind: request.kind,
                    position: 0,
                    url: request.url.trim().to_owned(),
                    width: picture.info.width,
                    height: picture.info.height,
                    key,
                    files: done.files,
                });
            }
        }
        Err(error) => report.failures.push(Failure::new(
            request.what,
            format!("it could not be saved: {error}"),
        )),
    }
    report
}

/// The absolute files for an extension, each once (letter case ignored except on Linux, where
/// `Poster.jpg` and `poster.jpg` are two files).
fn absolute(request: &NamedImage<'_>, ext: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for name in (request.names)(ext) {
        let mut path = request.base.to_path_buf();
        path.extend(name.split('/').filter(|part| !part.is_empty()));
        let same = |other: &PathBuf| cv_fs::paths::same_path(other, &path);
        if !found.iter().any(same) {
            found.push(path);
        }
    }
    found
}

#[derive(Default)]
struct Done {
    files: Vec<PathBuf>,
    trashed: Vec<PathBuf>,
    kept: Vec<PathBuf>,
}

fn write_all(
    targets: &[PathBuf],
    bytes: &[u8],
    replace: bool,
    trash: &Trash,
) -> std::io::Result<Done> {
    let mut done = Done::default();
    for target in targets {
        // An image saved under another extension counts as the image.
        if !replace && any_variant_exists(target) {
            done.kept.push(target.clone());
            continue;
        }
        match store(target, bytes, replace, trash, &mut done.trashed)? {
            Stored::Written | Stored::Unchanged => {
                if replace {
                    trash_other_variants(target, trash, &mut done.trashed)?;
                }
                done.files.push(target.clone());
            }
            Stored::Kept => done.kept.push(target.clone()),
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::download::fetch::StaticFetcher;

    fn png(shade: u8) -> Vec<u8> {
        let mut out = Vec::new();
        let image = image::RgbImage::from_pixel(2, 3, image::Rgb([shade, 0, 0]));
        image
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn names(ext: &str) -> Vec<String> {
        vec![
            format!("season01-poster.{ext}"),
            format!("Season 1/folder.{ext}"),
        ]
    }

    fn request<'a>(
        base: &'a Path,
        url: &'a str,
        replace: bool,
        list: NameList<'a>,
    ) -> NamedImage<'a> {
        NamedImage {
            what: "season 1 poster",
            kind: MediaFileType::SeasonPoster,
            url,
            base,
            names: list,
            data_source: None,
            replace,
        }
    }

    #[test]
    fn names_differing_only_in_case_are_one_target_only_where_the_platform_folds_case() {
        let dir = tempfile::tempdir().unwrap();
        let list: NameList = &|ext| vec![format!("Poster.{ext}"), format!("poster.{ext}")];
        let targets = absolute(&request(dir.path(), "https://img/x", false, list), "jpg");
        assert_eq!(
            targets.len(),
            if cv_fs::paths::CASE_SENSITIVE { 2 } else { 1 }
        );
    }

    #[tokio::test]
    async fn one_fetch_is_written_to_every_name() {
        let dir = tempfile::tempdir().unwrap();
        let fetcher = StaticFetcher::new();
        fetcher.insert("https://img/s1", png(10));
        let list: NameList = &names;
        let report = download_named_image(
            &fetcher,
            &request(dir.path(), "https://img/s1", false, list),
        )
        .await;
        assert!(report.failures.is_empty());
        assert_eq!(report.images.len(), 1);
        assert_eq!(report.images[0].kind, MediaFileType::SeasonPoster);
        assert_eq!(report.images[0].files.len(), 2);
        assert!(dir.path().join("season01-poster.png").is_file());
        assert!(dir.path().join("Season 1").join("folder.png").is_file());
        assert_eq!(fetcher.requests().len(), 1);
    }

    #[tokio::test]
    async fn an_image_that_is_there_costs_no_download_and_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("season01-poster.jpg"), b"mine").unwrap();
        fs::create_dir_all(dir.path().join("Season 1")).unwrap();
        fs::write(dir.path().join("Season 1").join("folder.png"), b"mine too").unwrap();
        let fetcher = StaticFetcher::new();
        fetcher.insert("https://img/s1", png(10));
        let list: NameList = &names;
        let report = download_named_image(
            &fetcher,
            &request(dir.path(), "https://img/s1", false, list),
        )
        .await;
        assert!(report.images.is_empty() && report.failures.is_empty());
        assert_eq!(report.kept.len(), 2);
        assert!(fetcher.requests().is_empty(), "nothing was fetched");
        assert_eq!(
            fs::read(dir.path().join("season01-poster.jpg")).unwrap(),
            b"mine"
        );
    }

    #[tokio::test]
    async fn replacing_moves_the_old_file_to_the_trash() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("season01-poster.jpg"), b"old").unwrap();
        let fetcher = StaticFetcher::new();
        fetcher.insert("https://img/s1", png(10));
        let list: NameList = &|ext: &str| vec![format!("season01-poster.{ext}")];
        let report =
            download_named_image(&fetcher, &request(dir.path(), "https://img/s1", true, list))
                .await;
        assert_eq!(report.images.len(), 1);
        assert_eq!(report.trashed.len(), 1, "the old jpg went to the trash");
        assert!(report.trashed[0].is_file());
        assert!(dir.path().join("season01-poster.png").is_file());
        assert!(!dir.path().join("season01-poster.jpg").exists());
    }

    #[tokio::test]
    async fn a_page_that_is_not_a_picture_is_a_failure_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let fetcher = StaticFetcher::new();
        fetcher.insert("https://img/s1", b"<html>error</html>".to_vec());
        let list: NameList = &names;
        let report = download_named_image(
            &fetcher,
            &request(dir.path(), "https://img/s1", false, list),
        )
        .await;
        assert!(report.images.is_empty());
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].what, "season 1 poster");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn a_folder_that_is_gone_gets_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("Offline Show");
        let fetcher = StaticFetcher::new();
        fetcher.insert("https://img/s1", png(10));
        let list: NameList = &names;
        let report =
            download_named_image(&fetcher, &request(&gone, "https://img/s1", false, list)).await;
        assert!(!report.changed() && report.failures.is_empty());
        assert!(!gone.exists());
        assert!(fetcher.requests().is_empty());
    }
}

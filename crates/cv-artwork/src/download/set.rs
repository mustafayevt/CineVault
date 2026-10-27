//! Writing the artwork of a collection (movie set).
//!
//! Spec: `movie-collections` (Collection artwork and NFO locations), `artwork` (Download missing
//! artwork). The file names come from [`crate::naming::set`]; this module fetches each image once
//! and writes it to every file the settings ask for.
//!
//! The data-safety rules are those of the movie downloads (see the module documentation of
//! [`crate::download`]):
//!
//! * a file that is already there, under any extension, is kept unless the caller asks for
//!   replacement, and a replaced file moves to the trash folder, never into the void;
//! * every file is written next to its name and renamed into place;
//! * bytes that are not a picture are never written;
//! * nothing is written into a movie folder that does not exist, so a moved or offline movie
//!   cannot sprout a stray folder. The collection data folder is created on demand.
//!
//! The function returns a [`DownloadReport`], so [`crate::download::record::record`] stores the
//! files and the source URLs of a collection exactly as it does for a movie.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cv_domain::MediaFileType;

use super::engine::{DownloadReport, Failure, WrittenImage, load_image};
use super::fetch::ImageFetcher;
use super::files::{Stored, Trash, any_variant_exists, store, trash_other_variants};
use crate::naming::set::{SetArt, SetNames, SetPlace};
use crate::thumb::ThumbKey;

/// A folder of a movie of the collection.
#[derive(Debug, Clone)]
pub struct SetMovieFolder {
    /// Where `movieset-<type>` files go: the folder of the movie's video.
    pub dir: PathBuf,
    /// The data source the movie lives in; its trash folder takes what is replaced.
    pub data_source: Option<PathBuf>,
}

/// What one run is asked to do for one collection.
pub struct SetArtworkRequest<'a> {
    /// The collection name, as the file names are made from it.
    pub title: &'a str,
    /// The TMDB collection ID, for the optional folder suffix.
    pub tmdb_id: Option<&'a str>,
    pub names: &'a SetNames,
    /// The collection data folder; the Kodi and Automator styles are skipped without it.
    pub collection_folder: Option<&'a Path>,
    /// The folders of the movies that are in the library, for the movie-folder style.
    pub movie_folders: &'a [SetMovieFolder],
    /// The source URL of each artwork type of the collection.
    pub urls: &'a BTreeMap<MediaFileType, String>,
    /// Replace files that are there (the old ones go to the trash).
    pub replace: bool,
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetArtworkReport {
    /// The images written, files moved to the trash, files kept and failures.
    pub report: DownloadReport,
    /// Things worth telling the user that are not failures: a style skipped because no collection
    /// folder is set, a collection with no movie folders.
    pub notes: Vec<String>,
}

/// The files of one image: absolute paths with the trash that takes what they replace.
struct Destination {
    path: PathBuf,
    trash: Trash,
}

/// Fetches the images of a collection and writes them to the configured places.
pub async fn download_set_artwork(
    fetcher: &dyn ImageFetcher,
    request: &SetArtworkRequest<'_>,
) -> SetArtworkReport {
    let mut out = SetArtworkReport::default();
    if request.names.locations.is_empty() {
        out.notes
            .push("Collection artwork is switched off in the settings.".to_owned());
        return out;
    }
    let wants_folder = request.names.uses_collection_folder();
    if wants_folder && request.collection_folder.is_none() {
        out.notes.push(
            "Set a collection folder in Settings to write collection artwork in the Kodi or \
             Automator style."
                .to_owned(),
        );
    }
    let movie_style = request
        .names
        .locations
        .iter()
        .any(|l| !l.needs_collection_folder());
    if movie_style && request.movie_folders.is_empty() {
        out.notes.push(
            "The collection has no movie in the library to put movieset files next to.".to_owned(),
        );
    }

    for art in SetArt::ALL {
        let kind = art.file_type();
        let Some(url) = request
            .urls
            .get(&kind)
            .map(|u| u.trim())
            .filter(|u| !u.is_empty())
        else {
            continue;
        };
        // The extension is only known once the image is fetched; existing files are matched
        // without it, so a complete set of files costs no download.
        let probe = destinations(request, art, "jpg");
        if probe.is_empty() {
            continue;
        }
        if !request.replace && probe.iter().all(|d| any_variant_exists(&d.path)) {
            out.report
                .kept
                .extend(probe.into_iter().map(|d| d.path.clone()));
            continue;
        }
        let picture = match load_image(fetcher, url).await {
            Ok(picture) => picture,
            Err(reason) => {
                out.report
                    .failures
                    .push(Failure::new(format!("collection {}", art.key()), reason));
                continue;
            }
        };
        let ext = picture.info.ext;
        let key = ThumbKey::of_bytes(&picture.bytes).as_str().to_owned();
        let targets = destinations(request, art, ext);
        let (bytes, replace) = (picture.bytes, request.replace);
        let written = tokio::task::spawn_blocking(move || write_all(&targets, &bytes, replace))
            .await
            .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())));
        match written {
            Ok(done) => {
                out.report.trashed.extend(done.trashed);
                out.report.kept.extend(done.kept);
                if !done.files.is_empty() {
                    out.report.images.push(WrittenImage {
                        kind,
                        position: 0,
                        url: url.to_owned(),
                        width: picture.info.width,
                        height: picture.info.height,
                        key,
                        files: done.files,
                    });
                }
            }
            Err(error) => out.report.failures.push(Failure::new(
                format!("collection {}", art.key()),
                format!("it could not be saved: {error}"),
            )),
        }
    }
    out
}

/// The absolute files of one type in the configured styles. Movie folders that do not exist and
/// styles that need a missing collection folder give nothing.
fn destinations(request: &SetArtworkRequest<'_>, art: SetArt, ext: &str) -> Vec<Destination> {
    let mut found: Vec<Destination> = Vec::new();
    let mut push = |path: PathBuf, trash: Trash| {
        let same = |a: &Path, b: &Path| {
            a.to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy())
        };
        if !found.iter().any(|d| same(&d.path, &path)) {
            found.push(Destination { path, trash });
        }
    };
    for target in request
        .names
        .targets(art, ext, request.title, request.tmdb_id)
    {
        match target.place {
            SetPlace::EachMovieFolder => {
                for folder in request.movie_folders.iter().filter(|f| f.dir.is_dir()) {
                    push(
                        folder.dir.join(&target.relative),
                        Trash::new(folder.data_source.as_deref(), &folder.dir),
                    );
                }
            }
            SetPlace::CollectionFolder => {
                let Some(root) = request.collection_folder else {
                    continue;
                };
                let mut path = root.to_path_buf();
                path.extend(target.relative.split('/'));
                push(path, Trash::new(None, root));
            }
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

fn write_all(targets: &[Destination], bytes: &[u8], replace: bool) -> std::io::Result<Done> {
    let mut done = Done::default();
    for target in targets {
        // A poster saved under another extension counts as the poster.
        if !replace && any_variant_exists(&target.path) {
            done.kept.push(target.path.clone());
            continue;
        }
        match store(
            &target.path,
            bytes,
            replace,
            &target.trash,
            &mut done.trashed,
        )? {
            Stored::Written | Stored::Unchanged => {
                if replace {
                    trash_other_variants(&target.path, &target.trash, &mut done.trashed)?;
                }
                done.files.push(target.path.clone());
            }
            Stored::Kept => done.kept.push(target.path.clone()),
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::download::fetch::StaticFetcher;
    use crate::naming::set::SetLocation;

    /// A tiny valid PNG.
    fn png(shade: u8) -> Vec<u8> {
        let mut out = Vec::new();
        let image = image::RgbImage::from_pixel(2, 3, image::Rgb([shade, 0, 0]));
        image
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    struct Fixture {
        dir: tempfile::TempDir,
        fetcher: StaticFetcher,
        urls: BTreeMap<MediaFileType, String>,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let fetcher = StaticFetcher::new();
            fetcher.insert("https://img/poster", png(10));
            fetcher.insert("https://img/fanart", png(20));
            let mut urls = BTreeMap::new();
            urls.insert(MediaFileType::Poster, "https://img/poster".to_owned());
            urls.insert(MediaFileType::Fanart, "https://img/fanart".to_owned());
            Fixture { dir, fetcher, urls }
        }

        fn movie(&self, name: &str) -> SetMovieFolder {
            let dir = self.dir.path().join("Movies").join(name);
            fs::create_dir_all(&dir).unwrap();
            SetMovieFolder {
                dir,
                data_source: Some(self.dir.path().join("Movies")),
            }
        }

        fn collections(&self) -> PathBuf {
            self.dir.path().join("Collections")
        }

        async fn run(
            &self,
            names: &SetNames,
            movies: &[SetMovieFolder],
            folder: Option<&Path>,
            replace: bool,
        ) -> SetArtworkReport {
            download_set_artwork(
                &self.fetcher,
                &SetArtworkRequest {
                    title: "Dune Collection",
                    tmdb_id: Some("726871"),
                    names,
                    collection_folder: folder,
                    movie_folders: movies,
                    urls: &self.urls,
                    replace,
                },
            )
            .await
        }
    }

    fn names(locations: &[SetLocation]) -> SetNames {
        SetNames {
            locations: locations.to_vec(),
            ..SetNames::default()
        }
    }

    #[tokio::test]
    async fn the_movie_folder_style_writes_next_to_every_movie() {
        let fx = Fixture::new();
        let movies = [fx.movie("Dune (2021)"), fx.movie("Dune Part Two (2024)")];
        let out = fx.run(&SetNames::default(), &movies, None, false).await;
        assert!(out.report.failures.is_empty(), "{:?}", out.report.failures);
        for movie in &movies {
            assert!(movie.dir.join("movieset-poster.png").is_file());
            assert!(movie.dir.join("movieset-fanart.png").is_file());
        }
        assert_eq!(out.report.images.len(), 2);
        assert_eq!(out.report.images[0].files.len(), 2);
        assert_eq!(out.report.images[0].kind, MediaFileType::Poster);
        assert_eq!(out.report.images[0].width, 2);
        // One download per image, however many files it is written to.
        assert_eq!(fx.fetcher.requests().len(), 2);
    }

    #[tokio::test]
    async fn the_kodi_style_writes_into_a_folder_named_after_the_collection() {
        let fx = Fixture::new();
        let root = fx.collections();
        let out = fx
            .run(&names(&[SetLocation::KodiStyle]), &[], Some(&root), false)
            .await;
        assert!(out.report.failures.is_empty(), "{:?}", out.report.failures);
        assert!(root.join("Dune Collection").join("poster.png").is_file());
        assert!(root.join("Dune Collection").join("fanart.png").is_file());
    }

    #[tokio::test]
    async fn the_automator_style_and_the_tmdb_suffix() {
        let fx = Fixture::new();
        let root = fx.collections();
        let mut names = names(&[SetLocation::AutomatorStyle]);
        names.append_tmdb_id = true;
        fx.run(&names, &[], Some(&root), false).await;
        assert!(
            root.join("Dune Collection-tmdb-726871-poster.png")
                .is_file()
        );
    }

    #[tokio::test]
    async fn a_folder_style_without_a_collection_folder_is_skipped_with_a_note() {
        let fx = Fixture::new();
        let movies = [fx.movie("Dune (2021)")];
        let both = names(&[SetLocation::MovieFolder, SetLocation::KodiStyle]);
        let out = fx.run(&both, &movies, None, false).await;
        assert!(movies[0].dir.join("movieset-poster.png").is_file());
        assert!(out.notes.iter().any(|n| n.contains("collection folder")));
        assert!(!fx.collections().exists());
    }

    #[tokio::test]
    async fn existing_files_are_kept_without_a_download() {
        let fx = Fixture::new();
        let movies = [fx.movie("Dune (2021)")];
        fs::write(movies[0].dir.join("movieset-poster.jpg"), b"mine").unwrap();
        fs::write(movies[0].dir.join("movieset-fanart.png"), b"mine").unwrap();
        let out = fx.run(&SetNames::default(), &movies, None, false).await;
        assert!(out.report.images.is_empty());
        assert_eq!(out.report.kept.len(), 2);
        assert!(fx.fetcher.requests().is_empty());
        assert_eq!(
            fs::read(movies[0].dir.join("movieset-poster.jpg")).unwrap(),
            b"mine"
        );
        assert!(!movies[0].dir.join("movieset-poster.png").exists());
    }

    #[tokio::test]
    async fn replacing_moves_the_old_file_to_the_trash() {
        let fx = Fixture::new();
        let movies = [fx.movie("Dune (2021)")];
        let old = movies[0].dir.join("movieset-poster.jpg");
        fs::write(&old, b"old").unwrap();
        let out = fx.run(&SetNames::default(), &movies, None, true).await;
        assert!(out.report.failures.is_empty(), "{:?}", out.report.failures);
        assert!(!old.exists());
        assert!(movies[0].dir.join("movieset-poster.png").is_file());
        assert_eq!(out.report.trashed.len(), 1);
        assert_eq!(fs::read(&out.report.trashed[0]).unwrap(), b"old");
        assert!(
            out.report.trashed[0]
                .starts_with(fx.dir.path().join("Movies").join(".deletedByCineVault"))
        );
    }

    #[tokio::test]
    async fn nothing_is_written_into_a_movie_folder_that_is_gone() {
        let fx = Fixture::new();
        let gone = SetMovieFolder {
            dir: fx.dir.path().join("Movies").join("Gone (2000)"),
            data_source: None,
        };
        let out = fx
            .run(
                &SetNames::default(),
                std::slice::from_ref(&gone),
                None,
                false,
            )
            .await;
        assert!(!gone.dir.exists());
        assert!(out.report.images.is_empty());
    }

    #[tokio::test]
    async fn a_bad_answer_is_reported_and_nothing_is_written() {
        let fx = Fixture::new();
        fx.fetcher
            .insert("https://img/poster", b"<html>no</html>".to_vec());
        let root = fx.collections();
        let out = fx
            .run(&names(&[SetLocation::KodiStyle]), &[], Some(&root), false)
            .await;
        assert_eq!(out.report.failures.len(), 1);
        assert_eq!(out.report.failures[0].what, "collection poster");
        assert!(!root.join("Dune Collection").join("poster.png").exists());
        // The other image is still written.
        assert!(root.join("Dune Collection").join("fanart.png").is_file());
    }

    #[tokio::test]
    async fn switched_off_writes_nothing() {
        let fx = Fixture::new();
        let movies = [fx.movie("Dune (2021)")];
        let out = fx.run(&names(&[]), &movies, None, false).await;
        assert!(out.report.images.is_empty());
        assert_eq!(out.notes.len(), 1);
        assert!(fx.fetcher.requests().is_empty());
    }
}

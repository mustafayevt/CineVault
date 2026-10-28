//! Artwork of collections (movie sets): Download missing artwork and Re-download artwork for a
//! selected collection, and the step after a movie was matched into its collection.
//!
//! Spec: `movie-collections` (Collection artwork and NFO locations). The file names are those of
//! `cv_artwork::naming::set`; the writing is `cv_artwork::download::set`.
//!
//! # What is written
//!
//! The artwork URLs the collection keeps (the TMDB poster and fanart, stored when the collection
//! was created or linked) are fetched and written in the styles of `artwork.collection-locations`:
//! next to each movie of the collection, and/or in the collection data folder
//! (`metadata.nfo-collection-folder`, the folder the collection NFO uses too). A file that is
//! there is kept unless the caller asks for replacement; what a replacement displaces goes to the
//! trash folder of the data source.
//!
//! Movies in a shared folder (several movies in one folder) are left out of the movie-folder
//! style, because every collection would write the same `movieset-poster.jpg` there.
//!
//! # Clean-up
//!
//! Artwork clean-up on a collection tidies the collection's own files: an image of a type that
//! sits where the location settings no longer write (another style, or the folder name with or
//! without the TMDB ID suffix), while the image is there in a place they do use, moves to the
//! trash. Only the folders of the collection's movies and the collection data folder are looked
//! at, so the `movieset-*` file of a movie that went to another collection is never touched. An
//! image that is the only one of its type stays, wherever it is.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_artwork::download::files::{Trash, any_variant_exists, variants};
use cv_artwork::download::{
    SetArtworkReport, SetArtworkRequest, SetMovieFolder, download_set_artwork,
};
use cv_artwork::naming::FolderKind;
use cv_artwork::naming::set::{SetArt, SetLocation, SetNames, SetPlace};
use cv_db::repo::{children_of, get_item, get_summary, update_item};
use cv_domain::{Item, ItemDetail, ItemId, ItemKind};

use super::{ArtworkError, ArtworkOutcome, ArtworkService, target_of};

/// The collection data folder of the settings (shared with the collection NFO).
const FOLDER_KEY: &str = "metadata.nfo-collection-folder";

/// A collection with the folders of its movies.
pub(super) struct LoadedSet {
    pub(super) item: Item,
    movie_folders: Vec<SetMovieFolder>,
}

/// The folders `movieset-*` files go to: one per movie that has a folder to itself.
fn movie_folders(movies: &[Item]) -> Vec<SetMovieFolder> {
    let mut folders: Vec<SetMovieFolder> = Vec::new();
    for movie in movies {
        let Some(target) = target_of(movie) else {
            continue;
        };
        if target.folder == FolderKind::MultiMovie {
            continue;
        }
        let known = folders.iter().any(|f| f.dir == target.dir);
        if !known {
            folders.push(SetMovieFolder {
                dir: target.dir,
                data_source: target.data_source,
            });
        }
    }
    folders
}

/// A path without its extension, as a comparison key, for artwork names (letter case ignored
/// except on Linux, where file names keep it).
fn name_key(path: &Path) -> String {
    cv_fs::paths::key(&path.with_extension(""))
}

/// Whether two paths are the same (letter case ignored except on Linux).
fn same_path(a: &Path, b: &Path) -> bool {
    cv_fs::paths::same_path(a, b)
}

/// Whether `path` is somewhere below `dir` (letter case ignored except on Linux).
fn is_below(path: &Path, dir: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .any(|parent| same_path(parent, dir))
}

/// The absolute files of `art` in the styles of `names` (extension `jpg`). Movie folders that do
/// not exist, and folder styles without a collection folder, give nothing.
fn places(
    names: &SetNames,
    art: SetArt,
    set: &LoadedSet,
    tmdb: Option<&str>,
    folder: Option<&Path>,
) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for target in names.targets(art, "jpg", &set.item.core.title, tmdb) {
        match target.place {
            SetPlace::EachMovieFolder => {
                for movie in set.movie_folders.iter().filter(|f| f.dir.is_dir()) {
                    found.push(movie.dir.join(&target.relative));
                }
            }
            SetPlace::CollectionFolder => {
                let Some(root) = folder else {
                    continue;
                };
                let mut path = root.to_path_buf();
                path.extend(target.relative.split('/'));
                found.push(path);
            }
        }
    }
    found
}

/// The collection's image files that the location settings no longer use while the image is
/// there in a place they do use, each with the trash that takes it.
fn stale_set_artwork(
    set: &LoadedSet,
    names: &SetNames,
    tmdb: Option<&str>,
    folder: Option<&Path>,
) -> Vec<(PathBuf, Trash)> {
    // Every place CineVault writes collection artwork to, with and without the TMDB ID suffix.
    let every_style = |append_tmdb_id| SetNames {
        locations: vec![
            SetLocation::MovieFolder,
            SetLocation::KodiStyle,
            SetLocation::AutomatorStyle,
        ],
        name_replacement: names.name_replacement.clone(),
        append_tmdb_id,
    };
    let movie_of = |path: &Path| {
        set.movie_folders
            .iter()
            .find(|movie| path.parent().is_some_and(|p| same_path(p, &movie.dir)))
    };
    // A file the collection recorded counts only inside its own places.
    let ours =
        |path: &Path| movie_of(path).is_some() || folder.is_some_and(|root| is_below(path, root));

    let mut stale: Vec<(PathBuf, Trash)> = Vec::new();
    for art in SetArt::ALL {
        let used = places(names, art, set, tmdb, folder);
        if !used.iter().any(|path| any_variant_exists(path)) {
            continue;
        }
        let used: Vec<String> = used.iter().map(|path| name_key(path)).collect();
        let mut candidates: Vec<PathBuf> = Vec::new();
        for append in [false, true] {
            for path in places(&every_style(append), art, set, tmdb, folder) {
                candidates.extend(variants(&path));
            }
        }
        candidates.extend(
            set.item
                .core
                .media_files
                .iter()
                .filter(|f| f.kind == art.file_type())
                .map(cv_domain::MediaFile::path)
                .filter(|path| path.is_file() && ours(path)),
        );
        for path in candidates {
            let known = stale.iter().any(|(p, _)| same_path(p, &path));
            if known || used.contains(&name_key(&path)) {
                continue;
            }
            let trash = match movie_of(&path) {
                Some(movie) => Trash::new(movie.data_source.as_deref(), &movie.dir),
                None => Trash::new(None, folder.or(path.parent()).unwrap_or(&path)),
            };
            stale.push((path, trash));
        }
    }
    stale
}

impl ArtworkService {
    /// Tidies the artwork of one collection (see the module documentation). With `dry_run` the
    /// files that would move are listed and nothing changes.
    pub(super) async fn cleanup_set(
        &self,
        id: ItemId,
        dry_run: bool,
    ) -> Result<Vec<PathBuf>, ArtworkError> {
        let loaded = self.load_set(id).await?;
        let names = SetNames::from_values(&self.settings.snapshot().values);
        let folder = self.collection_folder();
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let tmdb = loaded.item.core.ids.get("tmdb").map(str::to_owned);
            let stale = stale_set_artwork(&loaded, &names, tmdb.as_deref(), folder.as_deref());
            if dry_run || stale.is_empty() {
                return Ok(stale.into_iter().map(|(path, _)| path).collect());
            }
            let mut moved = Vec::new();
            for (path, trash) in stale {
                match trash.put(&path) {
                    Ok(_) => moved.push(path),
                    Err(error) => tracing::warn!(
                        path = %path.display(),
                        %error,
                        "stale collection artwork not moved"
                    ),
                }
            }
            if !moved.is_empty() {
                let gone = moved.clone();
                db.write(move |tx| {
                    if let Some(mut item) = get_item(tx, id)? {
                        item.core
                            .media_files
                            .retain(|f| !gone.iter().any(|g| same_path(g, &f.path())));
                        update_item(tx, &item)?;
                    }
                    Ok(())
                })?;
            }
            Ok(moved)
        })
        .await
        .map_err(|_| ArtworkError::Worker)?
    }

    /// The collection data folder, when one is set.
    fn collection_folder(&self) -> Option<PathBuf> {
        self.settings
            .snapshot()
            .values
            .get(FOLDER_KEY)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    }

    /// Whether the item is a collection.
    pub(super) async fn is_set(&self, id: ItemId) -> bool {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            db.read(|c| get_summary(c, id))
                .ok()
                .flatten()
                .is_some_and(|s| s.kind == ItemKind::Set)
        })
        .await
        .unwrap_or(false)
    }

    pub(super) async fn load_set(&self, id: ItemId) -> Result<LoadedSet, ArtworkError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let item = db
                .read(|c| get_item(c, id))?
                .ok_or(ArtworkError::ItemNotFound)?;
            if !matches!(item.detail, ItemDetail::Set(_)) {
                return Err(ArtworkError::Unsupported);
            }
            let movies: Vec<Item> = db.read(|c| {
                let mut movies = Vec::new();
                for child in children_of(c, id)? {
                    if let Some(movie) = get_item(c, child)?
                        && matches!(movie.detail, ItemDetail::Movie(_))
                    {
                        movies.push(movie);
                    }
                }
                Ok(movies)
            })?;
            Ok(LoadedSet {
                movie_folders: movie_folders(&movies),
                item,
            })
        })
        .await
        .map_err(|_| ArtworkError::Worker)?
    }

    /// Writes the artwork of one collection from the URLs it keeps. `replace` writes every image
    /// again and moves the old files to the trash.
    pub(super) async fn run_set(
        &self,
        id: ItemId,
        replace: bool,
    ) -> Result<ArtworkOutcome, ArtworkError> {
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| ArtworkError::Worker)?;
        let loaded = self.load_set(id).await?;
        let names = SetNames::from_values(&self.settings.snapshot().values);
        let folder = self.collection_folder();
        let tmdb = loaded.item.core.ids.get("tmdb").map(str::to_owned);
        let result: SetArtworkReport = download_set_artwork(
            self.fetcher.as_ref(),
            &SetArtworkRequest {
                title: &loaded.item.core.title,
                tmdb_id: tmdb.as_deref(),
                names: &names,
                collection_folder: folder.as_deref().map(Path::new),
                movie_folders: &loaded.movie_folders,
                urls: &loaded.item.core.artwork_urls,
                replace,
            },
        )
        .await;
        self.record(id, &result.report).await?;
        let mut outcome = ArtworkOutcome::of(id, &result.report);
        if outcome.images == 0 {
            outcome.problems.extend(result.notes);
        }
        Ok(outcome)
    }

    /// Sets one image of a collection (`Poster` or `Fanart`) from `url`: written to the
    /// artwork locations of the collection in the styles of the settings, the old files going to
    /// the trash, and kept as the address a later re-download writes again.
    pub(super) async fn place_set(
        &self,
        id: ItemId,
        kind: cv_domain::MediaFileType,
        url: &str,
    ) -> Result<ArtworkOutcome, ArtworkError> {
        if !matches!(
            kind,
            cv_domain::MediaFileType::Poster | cv_domain::MediaFileType::Fanart
        ) {
            return Err(ArtworkError::UnknownSlot);
        }
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| ArtworkError::Worker)?;
        let loaded = self.load_set(id).await?;
        let names = SetNames::from_values(&self.settings.snapshot().values);
        let folder = self.collection_folder();
        let tmdb = loaded.item.core.ids.get("tmdb").map(str::to_owned);
        let urls = std::collections::BTreeMap::from([(kind, url.to_owned())]);
        let result: SetArtworkReport = download_set_artwork(
            self.fetcher.as_ref(),
            &SetArtworkRequest {
                title: &loaded.item.core.title,
                tmdb_id: tmdb.as_deref(),
                names: &names,
                collection_folder: folder.as_deref().map(Path::new),
                movie_folders: &loaded.movie_folders,
                urls: &urls,
                replace: true,
            },
        )
        .await;
        self.record(id, &result.report).await?;
        let mut outcome = ArtworkOutcome::of(id, &result.report);
        if outcome.images == 0 {
            outcome.problems.extend(result.notes);
            return Ok(outcome);
        }
        let (db, address) = (Arc::clone(&self.db), url.to_owned());
        tokio::task::spawn_blocking(move || {
            db.write(move |tx| {
                if let Some(mut item) = get_item(tx, id)? {
                    item.core.artwork_urls.insert(kind, address);
                    update_item(tx, &item)?;
                }
                Ok(())
            })
        })
        .await
        .map_err(|_| ArtworkError::Worker)??;
        Ok(outcome)
    }

    /// The step after a movie was scraped into a collection: fills the collection's missing
    /// artwork, when the after-scrape setting is on. Runs in the background.
    pub(super) fn after_match_set(self: &Arc<Self>, movie: ItemId) {
        let service = Arc::clone(self);
        cv_jobs::spawn(async move {
            let db = Arc::clone(&service.db);
            let parent = tokio::task::spawn_blocking(move || {
                db.read(|c| get_item(c, movie))
                    .ok()
                    .flatten()
                    .and_then(|m| m.parent_id)
            })
            .await
            .ok()
            .flatten();
            let Some(set) = parent else {
                return;
            };
            if !service.is_set(set).await {
                return;
            }
            match service.run_set(set, false).await {
                Ok(outcome) => {
                    if outcome.images > 0 {
                        tracing::info!(
                            item = set.0,
                            images = outcome.images,
                            "collection artwork written after a scrape"
                        );
                    }
                    for problem in &outcome.problems {
                        tracing::debug!(item = set.0, %problem, "collection artwork problem");
                    }
                }
                Err(error) => {
                    tracing::debug!(item = set.0, %error, "no collection artwork after a scrape");
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_differ_only_in_case_are_the_same_only_where_the_platform_folds_case() {
        let folds = !cv_fs::paths::CASE_SENSITIVE;
        let (a, b) = (
            Path::new("/m/Heat/Poster.jpg"),
            Path::new("/m/heat/poster.jpg"),
        );
        assert_eq!(same_path(a, b), folds);
        assert_eq!(name_key(a) == name_key(b), folds);
        assert_eq!(is_below(a, Path::new("/m/heat")), folds);
        assert!(is_below(a, Path::new("/m/Heat")));
        assert_eq!(
            name_key(Path::new("/m/Heat/poster.jpg")),
            name_key(Path::new("/m/Heat/poster.png")),
            "the extension is not part of the key"
        );
    }
}

//! Data-source management through the service, against a real library file and real
//! folders. The pure guard cases (both path syntaxes) are in `cv-scan`'s `sources` tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_app::data_sources::{DataSourceError, DataSourceService};
use cv_db::Database;
use cv_db::repo::{get_item, insert_item};
use cv_domain::{Item, SourceKind};
use cv_scan::sources::{PathStyle, SourceGuard, SystemFolders};
use tempfile::TempDir;

struct Fixture {
    dir: TempDir,
    db: Arc<Database>,
    service: DataSourceService,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
        let mut system = SystemFolders::new(PathStyle::current());
        system.protect_tree(
            &dir.path().join("system").to_string_lossy(),
            "an operating-system folder",
        );
        let guard = SourceGuard::new(PathStyle::current(), system);
        let service = DataSourceService::new(Arc::clone(&db), guard);
        Self { dir, db, service }
    }

    /// Creates (if needed) and returns a folder below the fixture root.
    fn folder(&self, relative: &str) -> PathBuf {
        // `relative` uses `/`; build the path from parts so the separators are native.
        let path = relative
            .split('/')
            .fold(self.dir.path().to_path_buf(), |p, part| p.join(part));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn text(&self, relative: &str) -> String {
        self.folder(relative).to_string_lossy().into_owned()
    }

    fn paths(&self) -> Vec<String> {
        self.service
            .list()
            .unwrap()
            .into_iter()
            .map(|s| s.path)
            .collect()
    }

    fn movie_under(&self, source: &str, folder: &str) -> i64 {
        let mut item = Item::movie(folder);
        item.core.data_source = source.to_owned();
        item.core.path = Path::new(source)
            .join(folder)
            .to_string_lossy()
            .into_owned();
        item.core.last_scraper_id = "tmdb".into();
        item.core.plot = "Already scraped".into();
        self.db.write(move |tx| insert_item(tx, &item)).unwrap().0
    }
}

fn rejected(result: Result<impl std::fmt::Debug, DataSourceError>) -> String {
    result.expect_err("should be refused").code().to_owned()
}

#[test]
fn added_sources_are_listed_in_order_and_survive_a_reopen() {
    let f = Fixture::new();
    let movies = f.text("Movies");
    let films = f.text("Films");
    let shows = f.text("Shows");
    let a = f.service.add(SourceKind::Movie, &movies).unwrap();
    let b = f.service.add(SourceKind::Movie, &films).unwrap();
    let tv = f.service.add(SourceKind::Tv, &shows).unwrap();
    assert_eq!((a.position, b.position, tv.position), (0, 1, 0));
    assert_eq!(f.paths(), [movies.clone(), films.clone(), shows.clone()]);

    drop(f.service);
    let db_path = f.db.path().to_path_buf();
    drop(f.db);
    let reopened = Database::open(db_path).unwrap();
    let stored = reopened
        .read(|c| cv_db::repo::data_sources::list_sources(c, None))
        .unwrap();
    assert_eq!(
        stored.into_iter().map(|s| s.path).collect::<Vec<_>>(),
        [movies, films, shows]
    );
}

#[test]
fn a_nested_folder_is_refused_with_an_explanation() {
    let f = Fixture::new();
    let movies = f.text("Media/Movies");
    f.service.add(SourceKind::Movie, &movies).unwrap();
    let action = f.text("Media/Movies/Action");

    let error = f.service.add(SourceKind::Movie, &action).unwrap_err();
    assert_eq!(error.code(), "inside_existing");
    assert!(error.to_string().contains("already covered"), "{error}");
    assert_eq!(f.paths(), [movies]);
}

#[test]
fn a_folder_that_contains_a_source_is_refused() {
    let f = Fixture::new();
    f.service
        .add(SourceKind::Movie, &f.text("Media/Movies"))
        .unwrap();
    assert_eq!(
        rejected(f.service.add(SourceKind::Movie, &f.text("Media"))),
        "contains_existing"
    );
}

#[test]
fn the_same_folder_twice_and_its_other_spellings_are_refused() {
    let f = Fixture::new();
    let movies = f.text("Movies");
    f.service.add(SourceKind::Movie, &movies).unwrap();
    for spelling in [
        movies.clone(),
        format!("{movies}{}", std::path::MAIN_SEPARATOR),
        movies.to_uppercase(),
    ] {
        assert_eq!(
            rejected(f.service.add(SourceKind::Movie, &spelling)),
            "already_added",
            "{spelling}"
        );
    }
}

#[test]
fn movies_and_tv_are_separate_libraries_for_nesting() {
    let f = Fixture::new();
    f.service
        .add(SourceKind::Movie, &f.text("Media/Movies"))
        .unwrap();
    // A show folder beside the movies is fine, and so is one under a movie-free parent.
    f.service
        .add(SourceKind::Tv, &f.text("Media/Shows"))
        .unwrap();
    assert_eq!(f.paths().len(), 2);
}

#[test]
fn system_folders_are_refused() {
    let f = Fixture::new();
    for folder in ["system", "system/deep/er"] {
        assert_eq!(
            rejected(f.service.add(SourceKind::Movie, &f.text(folder))),
            "system_folder",
            "{folder}"
        );
    }
    assert!(f.paths().is_empty());
}

#[test]
fn a_folder_can_be_checked_before_its_library_is_known() {
    let f = Fixture::new();
    let movies = f.text("Media/Movies");
    assert_eq!(f.service.check_folder(&movies).unwrap(), movies);
    // A folder inside a protected one is refused by the check, naming the protected folder.
    let system = f.text("system");
    let inside = f.text("system/Temp/Movies");
    let error = f.service.check_folder(&inside).unwrap_err();
    assert_eq!(error.code(), "system_folder");
    let message = error.to_string();
    assert!(
        message.contains(&format!("is inside an operating-system folder ({system})")),
        "{message}"
    );
    // The check adds nothing.
    assert!(f.paths().is_empty());
}

#[test]
fn the_real_system_guard_refuses_the_operating_system_folder() {
    let f = Fixture::new();
    let guard = SourceGuard::for_current_os(&[]);
    let service = DataSourceService::new(Arc::clone(&f.db), guard);
    let system = if cfg!(windows) {
        std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())
    } else {
        "/System".to_owned()
    };
    // The folder may not exist on a build machine of another kind; the text check still applies.
    let error = service
        .add(SourceKind::Movie, &system)
        .expect_err("a system folder is never a library folder");
    assert!(
        matches!(error.code(), "system_folder" | "not_absolute"),
        "{error}"
    );
}

#[test]
fn bad_paths_are_refused_before_anything_is_stored() {
    let f = Fixture::new();
    assert_eq!(rejected(f.service.add(SourceKind::Movie, "")), "empty");
    assert_eq!(
        rejected(f.service.add(SourceKind::Movie, "relative/Movies")),
        "not_absolute"
    );
    let missing = f
        .dir
        .path()
        .join("Unplugged")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        rejected(f.service.add(SourceKind::Movie, &missing)),
        "not_reachable"
    );
    assert!(f.paths().is_empty());
}

#[test]
fn reorder_changes_the_list_and_demands_every_source_once() {
    let f = Fixture::new();
    let a = f.service.add(SourceKind::Movie, &f.text("A")).unwrap().id;
    let b = f.service.add(SourceKind::Movie, &f.text("B")).unwrap().id;
    let c = f.service.add(SourceKind::Movie, &f.text("C")).unwrap().id;
    let t = f.service.add(SourceKind::Tv, &f.text("T")).unwrap().id;

    let list = f.service.reorder(SourceKind::Movie, &[c, a, b]).unwrap();
    let order: Vec<i64> = list.iter().map(|s| s.id).collect();
    assert_eq!(order, [c, a, b, t]);

    for bad in [vec![a, b], vec![a, a, b, c], vec![a, b, c, t]] {
        assert_eq!(
            rejected(f.service.reorder(SourceKind::Movie, &bad)),
            "bad_order",
            "{bad:?}"
        );
    }
    let order: Vec<i64> = f.service.list().unwrap().iter().map(|s| s.id).collect();
    assert_eq!(order, [c, a, b, t]);
}

#[test]
fn removing_a_source_keeps_its_items() {
    let f = Fixture::new();
    let movies = f.text("Movies");
    let source = f.service.add(SourceKind::Movie, &movies).unwrap();
    let item = f.movie_under(&movies, "Alien (1979)");

    let removed = f.service.remove(source.id).unwrap();
    assert_eq!(removed.kept_items, 1);
    assert!(f.paths().is_empty());
    let kept =
        f.db.read(|c| get_item(c, cv_domain::ItemId(item)))
            .unwrap()
            .expect("the item must still exist");
    assert_eq!(kept.core.data_source, movies);

    assert_eq!(rejected(f.service.remove(source.id)), "unknown_source");
}

#[test]
fn adding_a_removed_source_again_picks_its_items_back_up() {
    let f = Fixture::new();
    let movies = f.text("Movies");
    let source = f.service.add(SourceKind::Movie, &movies).unwrap();
    let item = f.movie_under(&movies, "Alien (1979)");
    f.service.remove(source.id).unwrap();
    f.service.add(SourceKind::Movie, &movies).unwrap();
    let count =
        f.db.read(|c| cv_db::repo::data_sources::count_items_under(c, &movies))
            .unwrap();
    assert_eq!(count, 1);
    assert!(
        f.db.read(|c| get_item(c, cv_domain::ItemId(item)))
            .unwrap()
            .is_some()
    );
}

#[test]
fn remapping_repoints_items_without_touching_their_metadata() {
    let f = Fixture::new();
    let old = f.text("OldDrive/Movies");
    let new = f.text("Nas/Movies");
    let source = f.service.add(SourceKind::Movie, &old).unwrap();
    let item = f.movie_under(&old, "Alien (1979)");

    let report = f.service.remap(source.id, &new).unwrap();

    assert_eq!(report.old_path, old);
    assert_eq!(report.source.path, new);
    assert_eq!(report.items, 1);
    assert_eq!(f.paths(), std::slice::from_ref(&new));
    let moved =
        f.db.read(|c| get_item(c, cv_domain::ItemId(item)))
            .unwrap()
            .unwrap();
    assert_eq!(moved.core.data_source, new);
    assert_eq!(
        Path::new(&moved.core.path),
        Path::new(&new).join("Alien (1979)")
    );
    assert_eq!(moved.core.last_scraper_id, "tmdb");
    assert_eq!(moved.core.plot, "Already scraped");
}

#[test]
fn remap_is_guarded_like_add() {
    let f = Fixture::new();
    let movies = f.text("Movies");
    let source = f.service.add(SourceKind::Movie, &movies).unwrap();
    let films = f.service.add(SourceKind::Movie, &f.text("Films")).unwrap();

    // Nested in, or containing, another source.
    assert_eq!(
        rejected(f.service.remap(films.id, &f.text("Movies/Action"))),
        "inside_existing"
    );
    assert_eq!(
        rejected(f.service.remap(films.id, &f.text("Movies"))),
        "already_added"
    );
    // System and unreachable.
    assert_eq!(
        rejected(f.service.remap(films.id, &f.text("system/x"))),
        "system_folder"
    );
    let missing = f.dir.path().join("Gone").to_string_lossy().into_owned();
    assert_eq!(
        rejected(f.service.remap(films.id, &missing)),
        "not_reachable"
    );
    // Unchanged and unknown.
    assert_eq!(rejected(f.service.remap(source.id, &movies)), "unchanged");
    assert_eq!(rejected(f.service.remap(9999, &movies)), "unknown_source");
    assert_eq!(f.paths().len(), 2);
}

#[test]
fn a_source_can_be_remapped_to_a_folder_inside_or_around_itself() {
    let f = Fixture::new();
    let outer = f.text("Media");
    let source = f.service.add(SourceKind::Movie, &outer).unwrap();
    let movie = f.movie_under(&outer, "Alien (1979)");

    // Narrowing to a subfolder is a remap of the same source, not a nested second one.
    let inner = f.text("Media/Movies");
    f.service.remap(source.id, &inner).unwrap();
    assert_eq!(f.paths(), std::slice::from_ref(&inner));
    // The item's stored path follows the root, so it is verified (and found missing) by a scan.
    let item =
        f.db.read(|c| get_item(c, cv_domain::ItemId(movie)))
            .unwrap()
            .unwrap();
    assert_eq!(item.core.data_source, inner);
}

#[test]
fn a_simulated_drive_that_is_offline_cannot_be_added() {
    let f = Fixture::new();
    let service = f.service.clone().with_reachability(|_| false);
    assert_eq!(
        rejected(service.add(SourceKind::Movie, &f.text("Movies"))),
        "not_reachable"
    );
}

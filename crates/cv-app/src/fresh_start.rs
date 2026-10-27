//! The fresh start of schema version 100.
//!
//! The first start of this version does not upgrade the library of an earlier build: it sets the
//! old database aside, whole and unchanged, and starts with an empty library that the first scan
//! rebuilds from the media folders.
//!
//! **Database step** ([`database_step`]). Before any SQLite connection, the header of
//! `library.db` is read directly ([`cv_db::header`]). A file of an earlier build (schema version
//! 1 to 99, or 0 with content) is moved, with its `-wal` and `-shm` files when present, to
//! `library-old.db*` in the same folder, or to the first free `library-old-N.db*`. The move is a
//! same-folder rename through `cv-fs`'s retrying rename: never a copy, and nothing is ever
//! deleted. If a rename fails part-way, what already moved is renamed back and the step stops
//! with an error naming the file, so a new `library.db` is never created next to a stale `-wal`.
//! A version-100 file and a missing one are left as they are; a newer one is left for
//! `cv_db::Database::open` to refuse unchanged.
//!
//! The step is idempotent: once moved, the next start finds no old file and does nothing. A stop
//! between two renames leaves `library.db` gone and its `-wal` or `-shm` behind; the next start
//! moves those to the `library-old*` set that lacks them.
//!
//! **Settings step** ([`settings_step`]), after the database step. A `settings.json` whose
//! envelope version is lower than [`crate::settings::SETTINGS_VERSION`] is renamed to
//! `settings-old.json` (or the first free `settings-old-N.json`) and never read again; the app
//! starts from defaults, so the first-run setup opens. The credentials of removed features ([`RETIRED_ACCOUNTS`]) are deleted
//! from the credential store first, best effort, by exact account name, never logging a value;
//! the keys of the providers that stay are never touched and are found again as they were. The
//! credentials go before the file, so a stop between the two repeats the (harmless) deletes on the
//! next start, which then sets the file aside. Without an earlier settings file the step does
//! nothing, so later starts change nothing.
//!
//! [`run`] runs both steps in this order; each is detected on its own, so a stop between them is
//! finished on the next start.

use std::io;
use std::path::{Path, PathBuf};

use cv_db::header::{self, LibraryFile};

use crate::data_location::DataLocation;
use crate::secrets::SecretStore;
use crate::settings::SettingsStore;

/// Credential-store accounts of the features this version removed: Trakt and Simkl sync (tokens
/// and app credentials), the Kodi connection, the HTTP API and the subtitle services. Deleted by
/// exact name on the fresh start. `trakt.client_id` is not here: the Trakt rating source still
/// uses it.
pub const RETIRED_ACCOUNTS: &[&str] = &[
    "trakt.access_token",
    "trakt.refresh_token",
    "trakt.token_expires_at",
    "trakt.client_secret",
    "simkl.client_id",
    "simkl.access_token",
    "kodi.password",
    "http-api.key",
    "opensubtitles.app_key",
    "opensubtitles.username",
    "opensubtitles.password",
    "opensubtitles_org.username",
    "opensubtitles_org.password",
    "opensubtitles_org.user_agent",
    "subdl.api_key",
];

/// What the fresh start did at this launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreshStartReport {
    /// The database step.
    pub database: DatabaseStep,
    /// Where the settings of an earlier version went, when they were set aside.
    pub settings: Option<PathBuf>,
    /// How many credentials of removed features were deleted.
    pub retired_credentials: usize,
}

/// Runs the database step, then the settings step, for the data folder at `location`.
///
/// # Errors
/// The database step failed (see [`database_step`]); nothing was changed then. A settings step
/// that cannot rename the file is not an error here: loading the settings meets the file again
/// and never saves over it.
pub fn run(
    location: &DataLocation,
    secrets: &SecretStore,
) -> Result<FreshStartReport, FreshStartError> {
    let database = database_step(&location.library_db())?;
    let store = SettingsStore::new(location.settings_file());
    let (settings, retired_credentials) = match settings_step(&store, secrets) {
        Ok(done) => done,
        Err(error) => {
            tracing::error!(
                %error,
                path = %store.path().display(),
                "the settings of an earlier version could not be set aside"
            );
            (None, 0)
        }
    };
    Ok(FreshStartReport {
        database,
        settings,
        retired_credentials,
    })
}

/// Refuses, without changing anything, a data folder whose library or settings still need the
/// fresh start. For processes that must not make it themselves (the command line).
///
/// # Errors
/// [`FreshStartError::NeedsApp`] names the file of an earlier version; [`FreshStartError::Read`]
/// when a file cannot be read.
pub fn refuse_pending(location: &DataLocation) -> Result<(), FreshStartError> {
    let db = location.library_db();
    let read = |path: &Path| {
        let path = path.to_path_buf();
        move |source| FreshStartError::Read { path, source }
    };
    let library = header::classify(&db, cv_db::SCHEMA_VERSION).map_err(read(&db))?;
    let interrupted =
        library == LibraryFile::Missing && SIBLINGS.iter().any(|s| exists(&with_suffix(&db, s)));
    if matches!(library, LibraryFile::Earlier { .. }) || interrupted {
        return Err(FreshStartError::NeedsApp { path: db });
    }
    let settings = location.settings_file();
    let earlier = SettingsStore::new(&settings)
        .earlier_version()
        .map_err(read(&settings))?;
    if earlier.is_some() {
        return Err(FreshStartError::NeedsApp { path: settings });
    }
    Ok(())
}

/// The settings step: see the module docs. Returns where the old file went and how many retired
/// credentials were deleted.
///
/// # Errors
/// The settings file exists but cannot be read or renamed; it is then where it was.
pub fn settings_step(
    store: &SettingsStore,
    secrets: &SecretStore,
) -> io::Result<(Option<PathBuf>, usize)> {
    if store.earlier_version()?.is_none() {
        return Ok((None, 0));
    }
    let retired = remove_retired_credentials(secrets);
    let old = store.set_aside_earlier()?;
    if let Some(old) = &old {
        tracing::info!(
            to = %old.display(),
            "the settings of an earlier CineVault were set aside; defaults are in use"
        );
    }
    Ok((old, retired))
}

/// Deletes every [`RETIRED_ACCOUNTS`] entry, best effort. Logs account names only.
pub fn remove_retired_credentials(secrets: &SecretStore) -> usize {
    let mut removed = 0;
    for account in RETIRED_ACCOUNTS {
        match secrets.delete(account) {
            Ok(true) => {
                tracing::info!(account, "credential of a removed feature deleted");
                removed += 1;
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(account, %error, "a credential of a removed feature could not be deleted");
            }
        }
    }
    removed
}

/// The suffixes of the files SQLite keeps next to a database.
const SIBLINGS: [&str; 2] = ["-wal", "-shm"];

/// What the database step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseStep {
    /// Nothing to set aside: no library yet, an empty file, or a library of this epoch (or a
    /// newer one, which opening refuses).
    Unchanged,
    /// The library of an earlier build was moved aside.
    MovedAside {
        /// Its schema version, when the header named one.
        version: Option<u32>,
        /// Where the database file went (`library-old.db` or `library-old-N.db`).
        to: PathBuf,
    },
    /// The `-wal` or `-shm` file of a move that a stop interrupted was put with its database.
    Completed {
        /// The database those files belong with.
        to: PathBuf,
    },
}

/// Why the fresh start could not set the old library aside.
#[derive(Debug, thiserror::Error)]
pub enum FreshStartError {
    /// The header of the library file could not be read.
    #[error("CineVault could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The data folder still needs the fresh start, which only the app makes (the command line
    /// never moves the library).
    #[error(
        "{path} was made by an earlier version of CineVault. Start the CineVault app once so it \
         can set the old library aside and start a new one, then run the command again."
    )]
    NeedsApp { path: PathBuf },
    /// A file could not be moved aside (another program holds it, or the folder refused). Every
    /// file is where it was, unless `stranded` names one that could not be moved back.
    #[error(
        "CineVault could not move {path} aside to start a new library ({source}). Close any \
         program that may be using it, such as an earlier CineVault or a backup tool, and start \
         CineVault again.{}",
        stranded_note(.stranded)
    )]
    Move {
        path: PathBuf,
        #[source]
        source: io::Error,
        /// Files already moved that could not be moved back: `(where it is now, where it was)`.
        stranded: Vec<(PathBuf, PathBuf)>,
    },
}

fn stranded_note(stranded: &[(PathBuf, PathBuf)]) -> String {
    stranded
        .iter()
        .map(|(now, was)| {
            format!(
                " {} could not be moved back; rename it to {} by hand.",
                now.display(),
                was.display()
            )
        })
        .collect()
}

/// Sets the library of an earlier build at `db` aside, as described in the module docs.
///
/// # Errors
/// The header cannot be read, or a file cannot be moved (everything is then where it was).
pub fn database_step(db: &Path) -> Result<DatabaseStep, FreshStartError> {
    database_step_with(db, &mut |from, to| cv_fs::rename(from, to))
}

/// [`database_step`] with the rename supplied, so tests can make one fail.
pub(crate) fn database_step_with(
    db: &Path,
    rename: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<DatabaseStep, FreshStartError> {
    let file =
        header::classify(db, cv_db::SCHEMA_VERSION).map_err(|source| FreshStartError::Read {
            path: db.to_path_buf(),
            source,
        })?;
    match file {
        LibraryFile::Earlier { version } => {
            let to = free_old_name(db);
            let mut moves = vec![(db.to_path_buf(), to.clone())];
            for suffix in SIBLINGS {
                let from = with_suffix(db, suffix);
                if exists(&from) {
                    moves.push((from, with_suffix(&to, suffix)));
                }
            }
            move_all(&moves, rename)?;
            tracing::info!(
                to = %to.display(),
                ?version,
                "the library of an earlier CineVault was set aside; a new library starts"
            );
            Ok(DatabaseStep::MovedAside { version, to })
        }
        LibraryFile::Missing => complete_interrupted_move(db, rename),
        LibraryFile::Empty | LibraryFile::Current { .. } | LibraryFile::Newer { .. } => {
            Ok(DatabaseStep::Unchanged)
        }
    }
}

/// With no `library.db`, a `-wal` or `-shm` left beside it belongs to the database a stopped
/// start already moved: put it with the newest `library-old*` that lacks it. Never deleted.
fn complete_interrupted_move(
    db: &Path,
    rename: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<DatabaseStep, FreshStartError> {
    let orphans: Vec<(&str, PathBuf)> = SIBLINGS
        .iter()
        .map(|s| (*s, with_suffix(db, s)))
        .filter(|(_, p)| exists(p))
        .collect();
    if orphans.is_empty() {
        return Ok(DatabaseStep::Unchanged);
    }
    let owner = old_names(db)
        .take_while(|p| exists(p))
        .filter(|p| orphans.iter().all(|(s, _)| !exists(&with_suffix(p, s))))
        .last();
    let Some(owner) = owner else {
        // No old database to put them with. Left alone: a new library is opened next to them
        // only if SQLite accepts it, and nothing here is deleted.
        tracing::warn!(
            "{} has SQLite files without a database; they are left as they are",
            db.display()
        );
        return Ok(DatabaseStep::Unchanged);
    };
    let moves: Vec<(PathBuf, PathBuf)> = orphans
        .into_iter()
        .map(|(s, from)| (from, with_suffix(&owner, s)))
        .collect();
    move_all(&moves, rename)?;
    tracing::info!(to = %owner.display(), "an interrupted set-aside of the old library was completed");
    Ok(DatabaseStep::Completed { to: owner })
}

/// Renames every `(from, to)` in order; on a failure renames back what already moved.
fn move_all(
    moves: &[(PathBuf, PathBuf)],
    rename: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<(), FreshStartError> {
    for (done, (from, to)) in moves.iter().enumerate() {
        // Never over an existing file: the names were chosen free, check again right before.
        let result = if exists(to) {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", to.display()),
            ))
        } else {
            rename(from, to)
        };
        if let Err(source) = result {
            let mut stranded = Vec::new();
            for (back_from, back_to) in moves[..done].iter().rev() {
                if let Err(error) = rename(back_to, back_from) {
                    tracing::error!(%error, file = %back_to.display(), "could not move a file back");
                    stranded.push((back_to.clone(), back_from.clone()));
                }
            }
            return Err(FreshStartError::Move {
                path: from.clone(),
                source,
                stranded,
            });
        }
    }
    Ok(())
}

/// `library-old.db`, `library-old-2.db`, `library-old-3.db`, ... beside `db`.
fn old_names(db: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    let stem = db
        .file_stem()
        .map_or_else(|| "library".into(), |s| s.to_string_lossy().into_owned());
    let ext = db
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (1u32..).map(move |n| {
        let name = if n == 1 {
            format!("{stem}-old{ext}")
        } else {
            format!("{stem}-old-{n}{ext}")
        };
        db.with_file_name(name)
    })
}

/// The first old name whose database, `-wal` and `-shm` are all free.
fn free_old_name(db: &Path) -> PathBuf {
    old_names(db)
        .find(|p| !exists(p) && SIBLINGS.iter().all(|s| !exists(&with_suffix(p, s))))
        .expect("an unbounded sequence has a free name")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Whether anything (file, folder or link) has this name.
fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

#[cfg(test)]
mod tests;

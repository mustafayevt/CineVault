//! Data-source management: add, remove, reorder and remap the folders a library is built from.
//!
//! This is the one place that combines the folder guards of `cv-scan` with the storage of
//! `cv-db`, so the GUI and the CLI refuse the same folders for the same reasons:
//!
//! - **Add** normalises the path, refuses system folders (also when reached through a link),
//!   a folder inside or containing a source of the same library, and a folder that cannot be
//!   reached. The check is repeated inside the write transaction, so two quick adds cannot slip
//!   a nested pair past each other.
//! - **Remove** drops the source only. Its items stay in the library (offline until the folder
//!   is added again); the library is never purged by this service.
//! - **Reorder** takes the full new order of one library's sources.
//! - **Change type** moves a source to the other library type (Movies or TV shows) when that
//!   library has no folder that equals, contains or lies inside it. No title is touched; the
//!   caller scans the folder again.
//! - **Remap** moves a source to a new path (a re-mounted drive, a NAS share) and re-points every
//!   stored item path in one transaction, without touching metadata. The next scan verifies the
//!   new location.

use std::path::Path;
use std::sync::Arc;

use cv_db::repo::data_sources as rows;
use cv_db::{Database, DbError};
use cv_domain::{DataSource, SourceKind};
use cv_scan::sources::{ExistingSource, SourceError, SourceGuard};

pub use cv_db::repo::data_sources::{RemapReport, RemovedSource};

/// Why a data-source change did not happen.
#[derive(Debug, thiserror::Error)]
pub enum DataSourceError {
    /// The folder or the request broke a rule; the message says which and why.
    #[error("{0}")]
    Rejected(#[from] SourceError),
    /// The label was refused: blank, or another source of the library type has it. The message
    /// is ready to show.
    #[error("{message}")]
    Label { code: &'static str, message: String },
    /// The library database failed.
    #[error("The library database could not be updated: {0}")]
    Storage(#[from] DbError),
}

impl DataSourceError {
    /// A stable code for callers to branch on (`storage` for database failures).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Rejected(error) => error.code(),
            Self::Label { code, .. } => code,
            Self::Storage(_) => "storage",
        }
    }
}

type Reachable = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Manages the data sources of one library database.
#[derive(Clone)]
pub struct DataSourceService {
    db: Arc<Database>,
    guard: SourceGuard,
    reachable: Reachable,
}

impl DataSourceService {
    /// A service that checks folders with `guard` and the real file system.
    #[must_use]
    pub fn new(db: Arc<Database>, guard: SourceGuard) -> Self {
        Self {
            db,
            guard,
            reachable: Arc::new(|path| Path::new(path).is_dir()),
        }
    }

    /// The library database this service manages, for services that read the same library.
    #[must_use]
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    /// Replaces the "does this folder exist" test (a network share can be slow to probe, and
    /// tests simulate drives).
    #[must_use]
    pub fn with_reachability(
        mut self,
        reachable: impl Fn(&str) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.reachable = Arc::new(reachable);
        self
    }

    /// Every source: movies first, then TV, each in the user's order.
    ///
    /// # Errors
    /// Fails when the database cannot be read.
    pub fn list(&self) -> Result<Vec<DataSource>, DataSourceError> {
        Ok(self.db.read(|conn| rows::list_sources(conn, None))?)
    }

    /// Adds `raw_path` as a source of `kind`, at the end of that library's list.
    ///
    /// # Errors
    /// [`DataSourceError::Rejected`] with the rule that failed, or a storage error.
    pub fn add(&self, kind: SourceKind, raw_path: &str) -> Result<DataSource, DataSourceError> {
        let existing = self.existing(kind)?;
        let path = self.guard.check_with_links(raw_path, &existing, None)?;
        self.require_reachable(&path)?;

        let guard = self.guard.clone();
        let outcome = self.db.write(move |tx| {
            // Check again against what is stored now, inside the transaction.
            let existing = to_existing(rows::list_sources(tx, Some(kind))?);
            Ok(match guard.check(&path, &existing, None) {
                Ok(path) => Ok(rows::insert_source(tx, kind, &path)?),
                Err(error) => Err(error),
            })
        })?;
        Ok(outcome?)
    }

    /// Checks that `raw_path` could be a library folder at all, before its type is known: a full
    /// path that is not a system folder or the app's own data folder (also when reached through
    /// a link). The first-run setup asks this as soon as a folder is chosen, so a folder that
    /// can never be added is refused there and not only when the library is built. Returns the
    /// normalised path.
    ///
    /// # Errors
    /// [`DataSourceError::Rejected`] with the rule that failed.
    pub fn check_folder(&self, raw_path: &str) -> Result<String, DataSourceError> {
        Ok(self.guard.check_with_links(raw_path, &[], None)?)
    }

    /// Removes a source. Its items stay in the library; `kept_items` says how many.
    ///
    /// # Errors
    /// [`SourceError::UnknownSource`] when it does not exist.
    pub fn remove(&self, id: i64) -> Result<RemovedSource, DataSourceError> {
        self.db
            .write(move |tx| rows::remove_source(tx, id))?
            .ok_or_else(|| SourceError::UnknownSource.into())
    }

    /// Puts one library's sources in the given order and returns the new list.
    ///
    /// # Errors
    /// [`SourceError::BadOrder`] unless `ids` lists every source of `kind` exactly once.
    pub fn reorder(
        &self,
        kind: SourceKind,
        ids: &[i64],
    ) -> Result<Vec<DataSource>, DataSourceError> {
        let ids = ids.to_vec();
        let result = self
            .db
            .write(move |tx| match rows::reorder_sources(tx, kind, &ids) {
                Ok(()) => Ok(true),
                Err(DbError::InvalidData(_)) => Ok(false),
                Err(error) => Err(error),
            })?;
        if !result {
            return Err(SourceError::BadOrder {
                kind: kind.to_string(),
            }
            .into());
        }
        self.list()
    }

    /// Gives a source a new label. Only the label changes: no file is touched, nothing is scanned
    /// again and no title moves.
    ///
    /// # Errors
    /// [`DataSourceError::Label`] when the label is blank or another source of the same library
    /// type already uses it, or when the source does not exist (`unknown_source`).
    pub fn set_label(&self, id: i64, label: &str) -> Result<DataSource, DataSourceError> {
        let label = label.to_owned();
        self.db.write(move |tx| {
            Ok(match rows::set_source_label(tx, id, &label) {
                Ok(source) => Ok(source),
                Err(DbError::SourceLabelTaken { kind, .. }) => Err(DataSourceError::Label {
                    code: "label_taken",
                    message: format!("Another {kind} source already uses this label"),
                }),
                Err(DbError::SourceLabelEmpty) => Err(DataSourceError::Label {
                    code: "label_empty",
                    message: "A source label cannot be empty".to_owned(),
                }),
                Err(DbError::SourceNotFound(_)) => Err(SourceError::UnknownSource.into()),
                Err(error) => return Err(error),
            })
        })?
    }

    /// Moves a source to the other library type, at the end of that list. No title is deleted or
    /// changed: the folder is scanned again by the caller, and titles of the old type that the
    /// scan no longer finds are shown as missing, as for any file that went away.
    ///
    /// # Errors
    /// [`SourceError::UnknownSource`], or the rule the folder breaks in the other library
    /// (already added there, inside one of its folders, or containing one).
    pub fn change_kind(&self, id: i64, kind: SourceKind) -> Result<DataSource, DataSourceError> {
        let guard = self.guard.clone();
        let outcome = self.db.write(move |tx| {
            let Some(current) = rows::get_source(tx, id)? else {
                return Ok(Err(SourceError::UnknownSource));
            };
            if current.kind == kind {
                return Ok(Ok(current));
            }
            let existing = to_existing(rows::list_sources(tx, Some(kind))?);
            Ok(match guard.check(&current.path, &existing, None) {
                Ok(_) => match rows::change_source_kind(tx, id, kind)? {
                    Some(source) => Ok(source),
                    None => Err(SourceError::UnknownSource),
                },
                Err(error) => Err(error),
            })
        })?;
        Ok(outcome?)
    }

    /// Points a source at a new folder and re-points every item stored under the old one.
    /// Metadata is not touched and nothing is deleted.
    ///
    /// # Errors
    /// [`DataSourceError::Rejected`] when the source is unknown, the path is unchanged, or the new
    /// folder breaks a rule or cannot be reached.
    pub fn remap(&self, id: i64, raw_path: &str) -> Result<RemapReport, DataSourceError> {
        let current = self
            .db
            .read(|conn| rows::get_source(conn, id))?
            .ok_or(SourceError::UnknownSource)?;
        let existing = self.existing(current.kind)?;
        let path = self.guard.check_with_links(raw_path, &existing, Some(id))?;
        if path == current.path {
            return Err(SourceError::Unchanged { path }.into());
        }
        self.require_reachable(&path)?;

        let guard = self.guard.clone();
        let outcome = self.db.write(move |tx| {
            let Some(current) = rows::get_source(tx, id)? else {
                return Ok(Err(SourceError::UnknownSource));
            };
            let existing = to_existing(rows::list_sources(tx, Some(current.kind))?);
            Ok(match guard.check(&path, &existing, Some(id)) {
                Ok(path) => match rows::remap_source(tx, id, &path)? {
                    Some(report) => Ok(report),
                    None => Err(SourceError::UnknownSource),
                },
                Err(error) => Err(error),
            })
        })?;
        Ok(outcome?)
    }

    fn existing(&self, kind: SourceKind) -> Result<Vec<ExistingSource>, DataSourceError> {
        Ok(to_existing(
            self.db.read(|conn| rows::list_sources(conn, Some(kind)))?,
        ))
    }

    fn require_reachable(&self, path: &str) -> Result<(), DataSourceError> {
        if (self.reachable)(path) {
            Ok(())
        } else {
            Err(SourceError::NotReachable {
                path: path.to_owned(),
            }
            .into())
        }
    }
}

impl std::fmt::Debug for DataSourceService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataSourceService").finish_non_exhaustive()
    }
}

fn to_existing(sources: Vec<DataSource>) -> Vec<ExistingSource> {
    sources
        .into_iter()
        .map(|s| ExistingSource {
            id: s.id,
            path: s.path,
        })
        .collect()
}

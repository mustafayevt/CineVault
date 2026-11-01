//! Network shares (spec `network-shares`): library folders on an SMB share that needs a login of
//! its own.
//!
//! [`NetworkShareService`] connects a share through the system's own tools (`cv-shares`), checks
//! the folder inside it, and adds that folder with the rules, type and messages of "Add folder…"
//! ([`DataSourceService::add`]). The share's row (`network_share`) names the server, the share,
//! the user and the folder the share appears in on this computer. It never holds the password:
//!
//! * **Remembered**: the password is kept in the system credential store under
//!   `network-share.<id>` ([`secret_account`]). The row is written first; when the password
//!   cannot be stored, a new row is rolled back and [`ShareConnectError::Credential`] lets the
//!   page offer "Connect without remembering".
//! * **Not remembered**: it is kept in this process only, until CineVault quits.
//!
//! Either way the password is registered for log redaction as soon as it is entered, and
//! [`Password`] never shows in `Debug` or `Display`. Errors carry the kind of failure and the
//! system's message (scrubbed of the password by the connector), never the password.
//!
//! Connecting writes, moves and deletes nothing on the share. A share this service connected for
//! a folder that could not be added is disconnected again.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use cv_db::repo::network_shares::{self as rows, NetworkShare, NewNetworkShare};
use cv_db::{Database, DbError};
use cv_domain::{DataSource, SourceKind};
use cv_fs::paths;
use cv_shares::{Address, Cancel, Login, Password, ShareConnector, ShareError, folder_in_share};

use crate::data_sources::{DataSourceError, DataSourceService};
use crate::redact;
use crate::secrets::{SecretError, SecretStore};

/// The credential-store account of a share's remembered password.
#[must_use]
pub fn secret_account(id: i64) -> String {
    format!("network-share.{id}")
}

/// What the Network share page sends: the address with an optional folder inside the share, the
/// library type of that folder, the login, and whether to remember the password.
#[derive(Debug, Clone)]
pub struct ConnectRequest {
    pub address: Address,
    pub kind: SourceKind,
    pub login: Login,
    pub remember: bool,
}

/// A share connected and its folder added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareAdded {
    pub share: NetworkShare,
    pub source: DataSource,
}

/// Why a share was not connected or its folder not added.
#[derive(Debug, thiserror::Error)]
pub enum ShareConnectError {
    /// The share could not be connected, or the folder is not on it; the kind says why.
    #[error("{0}")]
    Share(#[from] ShareError),
    /// The folder rules refused the folder, as they would for "Add folder…".
    #[error("{0}")]
    Folder(#[from] DataSourceError),
    /// The password could not be stored. The page offers to connect without remembering it.
    #[error("{0}")]
    Credential(SecretError),
    /// The library database failed.
    #[error("The library database could not be updated: {0}")]
    Storage(#[from] DbError),
}

impl ShareConnectError {
    /// A stable code for callers to branch on: the share error's kind (`wrong_login`, …), the
    /// folder rule (`nested`, …), `no_keyring`, `credential_store` or `storage`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Share(error) => error.kind().code(),
            Self::Folder(error) => error.code(),
            Self::Credential(SecretError::NoKeyring) => "no_keyring",
            Self::Credential(_) => "credential_store",
            Self::Storage(_) => "storage",
        }
    }
}

/// Connects network shares and adds the library folders on them.
pub struct NetworkShareService {
    db: Arc<Database>,
    sources: DataSourceService,
    connector: Arc<dyn ShareConnector>,
    secrets: Arc<SecretStore>,
    /// The passwords that are not remembered, by share id, for the life of the process.
    unsaved: Mutex<HashMap<i64, Password>>,
}

impl std::fmt::Debug for NetworkShareService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkShareService")
            .finish_non_exhaustive()
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl NetworkShareService {
    /// The service over the library folders of `sources`, connecting with `connector` and keeping
    /// remembered passwords in `secrets`.
    #[must_use]
    pub fn new(
        sources: DataSourceService,
        connector: Arc<dyn ShareConnector>,
        secrets: Arc<SecretStore>,
    ) -> Self {
        Self {
            db: Arc::clone(sources.database()),
            sources,
            connector,
            secrets,
            unsaved: Mutex::new(HashMap::new()),
        }
    }

    /// Every share CineVault connected, oldest first.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn list(&self) -> Result<Vec<NetworkShare>, DbError> {
        self.db.read(rows::list_shares)
    }

    /// Connects the share of `request` with its login, then adds the folder inside it as a library
    /// folder. Blocks while the system's tool runs: call it off the UI thread. `cancel` stops the
    /// attempt (Esc on the page).
    ///
    /// # Errors
    /// [`ShareConnectError::Share`] when the share cannot be connected or the folder is not on
    /// it, [`ShareConnectError::Folder`] when the folder rules refuse it,
    /// [`ShareConnectError::Credential`] when the password cannot be remembered. Nothing is added
    /// then, and a share connected only for this attempt is disconnected again.
    pub fn connect_and_add(
        &self,
        request: &ConnectRequest,
        cancel: &Cancel,
    ) -> Result<ShareAdded, ShareConnectError> {
        let password = request.login.password();
        // Masked in every log line from here on, whatever happens next.
        redact::register_secret(password.expose());
        let share = &request.address.share;
        let was_connected = self.connector.is_connected(share, cancel).unwrap_or(false);
        let root = self.connector.connect(share, &request.login, cancel)?;
        let local_root = root.to_string_lossy().into_owned();
        let known = self.row_at(&local_root)?;
        let added = self.add_on(&root, known.clone(), request);
        if let Err(error) = &added {
            tracing::info!(
                share = %share,
                code = error.code(),
                "a network share folder was not added"
            );
            if !was_connected && known.is_none() {
                self.disconnect_quietly(share);
            }
        }
        added
    }

    fn add_on(
        &self,
        root: &Path,
        known: Option<NetworkShare>,
        request: &ConnectRequest,
    ) -> Result<ShareAdded, ShareConnectError> {
        let folder = folder_in_share(root, &request.address.folder)?;
        let path = self
            .sources
            .check_new(request.kind, &folder.to_string_lossy())?;
        let is_new = known.is_none();
        let share = self.keep_login(known, root, request)?;
        match self.sources.add(request.kind, &path) {
            Ok(source) => {
                tracing::info!(share = %request.address.share, user = %share.user, "network share folder added");
                Ok(ShareAdded { share, source })
            }
            Err(error) => {
                if is_new {
                    self.forget_row(share.id);
                }
                Err(error.into())
            }
        }
    }

    /// Writes the row of a share that just connected (a new one, or the known one with the new
    /// login) and keeps its password as asked.
    fn keep_login(
        &self,
        known: Option<NetworkShare>,
        root: &Path,
        request: &ConnectRequest,
    ) -> Result<NetworkShare, ShareConnectError> {
        let user = request.login.user();
        let share = match known {
            Some(share) => {
                self.keep_password(share.id, request.remember, request.login.password())
                    .map_err(ShareConnectError::Credential)?;
                share
            }
            None => {
                let (server, name) = (
                    request.address.share.server().to_owned(),
                    request.address.share.share().to_owned(),
                );
                let (user, local_root) = (user.to_owned(), root.to_string_lossy().into_owned());
                let remember = request.remember;
                let share = self.db.write(move |tx| {
                    rows::insert_share(
                        tx,
                        &NewNetworkShare {
                            server: &server,
                            share: &name,
                            user: &user,
                            local_root: &local_root,
                            remember,
                        },
                    )
                })?;
                if let Err(error) =
                    self.keep_password(share.id, request.remember, request.login.password())
                {
                    let id = share.id;
                    self.db.write(move |tx| rows::delete_share(tx, id))?;
                    return Err(ShareConnectError::Credential(error));
                }
                share
            }
        };
        let share = NetworkShare {
            user: user.to_owned(),
            remember: request.remember,
            last_connected_ms: Some(now_ms()),
            last_error: None,
            ..share
        };
        let row = share.clone();
        self.db.write(move |tx| rows::update_share(tx, &row))?;
        Ok(share)
    }

    /// Keeps a share's password: in the credential store when `remember`, else in this process.
    /// An empty password is never stored; a remembered share without one connects with none.
    fn keep_password(
        &self,
        id: i64,
        remember: bool,
        password: &Password,
    ) -> Result<(), SecretError> {
        let account = secret_account(id);
        if remember && !password.expose().is_empty() {
            self.secrets.set(&account, password.expose())?;
            locked(&self.unsaved).remove(&id);
            return Ok(());
        }
        if let Err(error) = self.secrets.delete(&account) {
            tracing::debug!(%error, "no stored share password to remove");
        }
        if remember {
            locked(&self.unsaved).remove(&id);
        } else {
            locked(&self.unsaved).insert(id, password.clone());
        }
        Ok(())
    }

    /// The row of the share that appears at `local_root`.
    fn row_at(&self, local_root: &str) -> Result<Option<NetworkShare>, DbError> {
        Ok(self.list()?.into_iter().find(|share| {
            paths::key_with(&share.local_root, paths::CASE_SENSITIVE)
                == paths::key_with(local_root, paths::CASE_SENSITIVE)
        }))
    }

    /// Removes a share's row, its stored password and the one kept in memory.
    fn forget_row(&self, id: i64) {
        if let Err(error) = self.secrets.delete(&secret_account(id)) {
            tracing::debug!(%error, "no stored share password to remove");
        }
        if let Some(password) = locked(&self.unsaved).remove(&id) {
            redact::unregister_secret(password.expose());
        }
        if let Err(error) = self.db.write(move |tx| rows::delete_share(tx, id)) {
            tracing::warn!(%error, "a network share row could not be removed");
        }
    }

    fn disconnect_quietly(&self, share: &cv_shares::ShareAddress) {
        if let Err(error) = self.connector.disconnect(share, &Cancel::new()) {
            tracing::info!(
                share = %share,
                error = %redact::redact(&error.to_string()),
                "a network share could not be disconnected"
            );
        }
    }

    /// The password kept in memory for a share that is not remembered.
    #[cfg(test)]
    fn unsaved_password(&self, id: i64) -> Option<Password> {
        locked(&self.unsaved).get(&id).cloned()
    }
}

#[cfg(test)]
mod tests;

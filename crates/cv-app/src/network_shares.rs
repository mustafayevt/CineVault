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
//! # At start
//!
//! [`NetworkShareService::reconnect_at_start`] connects every share that is not connected yet,
//! before the folder health check and the scan of its folders: the shares of one server one after
//! the other, the servers in parallel, each server within [`RECONNECT_LIMIT`]. Folders on other
//! drives do not wait. While a share is being connected its folders keep their state
//! ([`ShareState::Connecting`]); when it is done, connected or not, the failure code is stored on
//! its row and its folders are handed to the after-connect hook, which scans them. A share whose
//! password is not remembered cannot connect by itself and records [`PASSWORD_NEEDED`].
//!
//! # Offline, never missing
//!
//! A library folder on a share whose last connect failed and that does not answer is offline in
//! the folder health check at once, with the [`OfflineReason`] ("Wrong password", "Server not
//! reachable", "Share not found", "Password needed"). Its titles keep everything: a scan of a
//! folder that cannot be read marks them offline, never missing, and on macOS an empty share
//! mount folder does not pass for the share (`cv_fs::paths::drive_is_mounted`).
//!
//! # Forgetting a share
//!
//! Removing the last library folder on a share marks it unused, and adding a folder on it again
//! (the Undo of the removal) clears the mark ([`DataSourceService`] keeps it in step).
//! [`NetworkShareService::sweep`] forgets a share that has stayed unused for [`FORGET_AFTER`]
//! (longer than any Undo toast), and at start every unused share: once no job is running, it
//! disconnects the share (a disconnect refused because files are open is accepted; the
//! connection then ends at sign-out), deletes the stored password, then the row. The titles of
//! the removed folders stay in the library, offline.
//!
//! [`NetworkShareService::clean_mount_folders`] removes, at start, the mount folders below
//! `~/CineVault Shares` that belong to no share (left by an interrupted connect): a mounted one is
//! unmounted first and left alone when it stays mounted, and a folder is only ever removed with
//! `rmdir`, so one that still holds anything stays.
//!
//! Connecting writes, moves and deletes nothing on the share. A share this service connected for
//! a folder that could not be added is disconnected again.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use cv_db::repo::network_shares::{self as rows, NetworkShare, NewNetworkShare};
use cv_db::{Database, DbError};
use cv_domain::{DataSource, SourceKind};
use cv_fs::paths;
use cv_shares::{
    Address, Cancel, Login, Password, ShareAddress, ShareConnector, ShareError, ShareErrorKind,
    folder_in_share,
};

use crate::data_sources::{DataSourceError, DataSourceService};
use crate::onboarding::{SuggestedKind, analyze_folder};
use crate::redact;
use crate::secrets::{SecretError, SecretStore};

/// How long the shares of one server may take to connect at start before they count as not
/// reachable for that start.
pub const RECONNECT_LIMIT: Duration = Duration::from_secs(15);

/// The failure code of a share whose password is not remembered and that is not connected: it
/// waits for "Connect…".
pub const PASSWORD_NEEDED: &str = "password_needed";

/// Called with the library folders (data source ids) of a share whose connect step ended.
pub type AfterConnect = Arc<dyn Fn(Vec<i64>) + Send + Sync>;

/// How long a share stays unused before it is forgotten: longer than any Undo toast.
pub const FORGET_AFTER: Duration = Duration::from_secs(60);

/// How often the app looks for shares to forget.
pub const SWEEP_EVERY: Duration = Duration::from_secs(15);

/// Whether work that may read or write a share is running (a scan, a rename, a move, an artwork
/// or NFO write): a share is not forgotten under it.
pub type Busy = Arc<dyn Fn() -> bool + Send + Sync>;

/// Tells whether a folder is a mount point.
type MountCheck = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// What the network shares say about a library folder, ahead of its own probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareState {
    /// Its share is being connected at start: the folder keeps its last known state until then.
    Connecting,
    /// Its share's last connect failed: a folder that does not answer is offline for this reason.
    Failed(OfflineReason),
}

/// Why the folders of a share are offline, as the Library folders row says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfflineReason {
    WrongPassword,
    Unreachable,
    ShareNotFound,
    PasswordNeeded,
    /// Any other failure (a locked account, another user connected, the system's own message).
    NotConnected,
}

impl OfflineReason {
    /// The reason of a failure code stored on a share's row (`network_share.last_error`).
    #[must_use]
    pub fn from_code(code: &str) -> Self {
        if code == PASSWORD_NEEDED {
            return Self::PasswordNeeded;
        }
        match ShareErrorKind::parse(code) {
            Some(ShareErrorKind::WrongLogin) => Self::WrongPassword,
            Some(ShareErrorKind::Unreachable) => Self::Unreachable,
            Some(ShareErrorKind::ShareNotFound) => Self::ShareNotFound,
            _ => Self::NotConnected,
        }
    }

    /// A stable code for the interface.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::WrongPassword => "wrong_password",
            Self::Unreachable => "unreachable",
            Self::ShareNotFound => "share_not_found",
            Self::PasswordNeeded => PASSWORD_NEEDED,
            Self::NotConnected => "not_connected",
        }
    }

    /// The words of the folder row: "offline · Wrong password".
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::WrongPassword => "Wrong password",
            Self::Unreachable => "Server not reachable",
            Self::ShareNotFound => "Share not found",
            Self::PasswordNeeded => "Password needed",
            Self::NotConnected => "Not connected",
        }
    }
}

/// The owner of a share's password kept in memory, in the log redaction holders.
fn memory_owner(id: i64) -> String {
    format!("network-share-memory.{id}")
}

/// The credential-store account of a share's remembered password.
#[must_use]
pub fn secret_account(id: i64) -> String {
    format!("network-share.{id}")
}

/// What the Network share page sends: the address with an optional folder inside the share, the
/// library type of that folder (`None`: what the folder holds decides, as for "Add folder…"), the
/// login, and whether to remember the password.
#[derive(Debug, Clone)]
pub struct ConnectRequest {
    pub address: Address,
    pub kind: Option<SourceKind>,
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
    /// The share asked for is not known (any more).
    #[error("This network share is no longer known.")]
    UnknownShare,
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
            Self::UnknownShare => "unknown_share",
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
    /// The shares being connected at start.
    connecting: Mutex<HashSet<i64>>,
    after_connect: Mutex<Option<AfterConnect>>,
    busy: Mutex<Option<Busy>>,
    /// Where CineVault mounts shares itself (`~/CineVault Shares` on macOS), and how to tell a
    /// mount point there.
    mount_folders: Option<std::path::PathBuf>,
    is_mounted: MountCheck,
    /// The time the shares of one server have at start ([`RECONNECT_LIMIT`]).
    reconnect_limit: Duration,
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
            connecting: Mutex::new(HashSet::new()),
            after_connect: Mutex::new(None),
            reconnect_limit: RECONNECT_LIMIT,
            busy: Mutex::new(None),
            mount_folders: own_mount_folders(),
            is_mounted: mount_check(),
        }
    }

    /// Whether this computer can connect shares for CineVault at all (a Linux desktop without
    /// user-level share mounting cannot).
    #[must_use]
    pub fn support(&self) -> cv_shares::Support {
        self.connector.support()
    }

    /// Asks `busy` before forgetting a share, and waits while it says work is running.
    pub fn set_busy(&self, busy: Busy) {
        *locked(&self.busy) = Some(busy);
    }

    /// Calls `hook` with the library folders of a share whose connect step at start ended, and of
    /// a share connected again with [`Self::connect_again`], so they are scanned.
    pub fn set_after_connect(&self, hook: AfterConnect) {
        *locked(&self.after_connect) = Some(hook);
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
        let kind = request.kind.unwrap_or_else(|| detect_kind(&folder));
        let path = self.sources.check_new(kind, &folder.to_string_lossy())?;
        let is_new = known.is_none();
        let share = self.keep_login(
            known,
            &request.address.share,
            root,
            &request.login,
            request.remember,
        )?;
        match self.sources.add(kind, &path) {
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
        address: &ShareAddress,
        root: &Path,
        login: &Login,
        remember: bool,
    ) -> Result<NetworkShare, ShareConnectError> {
        let user = login.user();
        let share = match known {
            Some(share) => {
                self.keep_password(share.id, remember, login.password())
                    .map_err(ShareConnectError::Credential)?;
                share
            }
            None => {
                let (server, name) = (address.server().to_owned(), address.share().to_owned());
                let (user, local_root) = (user.to_owned(), root.to_string_lossy().into_owned());
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
                if let Err(error) = self.keep_password(share.id, remember, login.password()) {
                    let id = share.id;
                    self.db.write(move |tx| rows::delete_share(tx, id))?;
                    return Err(ShareConnectError::Credential(error));
                }
                share
            }
        };
        let share = NetworkShare {
            user: user.to_owned(),
            remember,
            last_connected_ms: Some(now_ms()),
            last_error: None,
            ..share
        };
        let row = share.clone();
        self.db.write(move |tx| rows::update_share(tx, &row))?;
        Ok(share)
    }

    /// Connects a known share again with a new login ("Connect…" on an offline folder), keeps the
    /// password as asked and hands its folders to the after-connect hook, so they come online and
    /// are scanned. A failed attempt stores its code on the row.
    ///
    /// # Errors
    /// [`ShareConnectError::UnknownShare`], [`ShareConnectError::Share`] with the reason, or
    /// [`ShareConnectError::Credential`] when the password cannot be remembered (the share is
    /// connected then; the page offers to go on without remembering it).
    pub fn connect_again(
        &self,
        id: i64,
        login: &Login,
        remember: bool,
        cancel: &Cancel,
    ) -> Result<NetworkShare, ShareConnectError> {
        redact::register_secret(login.password().expose());
        let share = self
            .db
            .read(move |c| rows::get_share(c, id))?
            .ok_or(ShareConnectError::UnknownShare)?;
        let address = address_of(&share)?;
        let root = match self.connector.connect(&address, login, cancel) {
            Ok(root) => root,
            Err(error) => {
                if error.kind() != ShareErrorKind::Cancelled {
                    self.record_failure(&share, error.kind().code());
                }
                return Err(error.into());
            }
        };
        let share = self.keep_login(Some(share), &address, &root, login, remember)?;
        tracing::info!(share = %address, user = %share.user, "network share connected again");
        self.hand_over(&share);
        Ok(share)
    }

    /// Starts connecting every known share that is not connected yet, in the background (see the
    /// module notes), and returns the folders the shares appear in: library folders below them
    /// wait for their share, the others need not.
    pub fn reconnect_at_start(self: &Arc<Self>) -> Vec<String> {
        let shares = match self.list() {
            Ok(shares) => shares,
            Err(error) => {
                tracing::warn!(%error, "the network shares could not be read");
                return Vec::new();
            }
        };
        // An unused share is about to be forgotten; it is not connected again.
        let shares: Vec<NetworkShare> = shares
            .into_iter()
            .filter(|share| share.unused_since_ms.is_none())
            .collect();
        locked(&self.connecting).extend(shares.iter().map(|share| share.id));
        let roots = shares.iter().map(|s| s.local_root.clone()).collect();
        let mut servers: BTreeMap<String, Vec<NetworkShare>> = BTreeMap::new();
        for share in shares {
            servers
                .entry(share.server.to_lowercase())
                .or_default()
                .push(share);
        }
        for (server, shares) in servers {
            let me = Arc::clone(self);
            let spawned = std::thread::Builder::new()
                .name("cv-share-reconnect".to_owned())
                .spawn(move || {
                    let cancel = Cancel::with_limit(me.reconnect_limit);
                    for share in &shares {
                        me.reconnect(share, &cancel);
                    }
                });
            if let Err(error) = spawned {
                tracing::warn!(%error, server, "the shares of a server could not be connected");
            }
        }
        roots
    }

    /// One share's connect step at start: connected unless it already was, the result stored on
    /// its row, then its folders handed over.
    fn reconnect(&self, share: &NetworkShare, cancel: &Cancel) {
        let outcome = address_of(share)
            .map_err(|error| error.kind().code())
            .and_then(|address| self.connect_known(share, &address, cancel));
        match outcome {
            Ok(()) => {
                let row = NetworkShare {
                    last_connected_ms: Some(now_ms()),
                    last_error: None,
                    ..share.clone()
                };
                if let Err(error) = self.db.write(move |tx| rows::update_share(tx, &row)) {
                    tracing::warn!(%error, "a network share row could not be updated");
                }
            }
            Err(code) => {
                tracing::info!(
                    server = %share.server,
                    share = %share.share,
                    code,
                    "a network share did not connect at start"
                );
                self.record_failure(share, code);
            }
        }
        locked(&self.connecting).remove(&share.id);
        self.hand_over(share);
    }

    fn connect_known(
        &self,
        share: &NetworkShare,
        address: &ShareAddress,
        cancel: &Cancel,
    ) -> Result<(), &'static str> {
        if self
            .connector
            .is_connected(address, cancel)
            .unwrap_or(false)
        {
            return Ok(());
        }
        let password = self.password_of(share).ok_or(PASSWORD_NEEDED)?;
        let login = Login::new(&share.user, password).map_err(|_| ShareErrorKind::Other.code())?;
        self.connector
            .connect(address, &login, cancel)
            .map(drop)
            .map_err(|error| error.kind().code())
    }

    /// The password a known share connects with: the remembered one (none stored means an empty
    /// one), or the one kept in memory. `None` when there is none to use.
    fn password_of(&self, share: &NetworkShare) -> Option<Password> {
        if !share.remember {
            return locked(&self.unsaved).get(&share.id).cloned();
        }
        match self.secrets.get(&secret_account(share.id)) {
            Ok(stored) => Password::new(stored.as_ref().map_or("", |secret| secret.expose())).ok(),
            Err(error) => {
                tracing::warn!(%error, "a remembered share password could not be read");
                None
            }
        }
    }

    fn record_failure(&self, share: &NetworkShare, code: &str) {
        let row = NetworkShare {
            last_error: Some(code.to_owned()),
            ..share.clone()
        };
        if let Err(error) = self.db.write(move |tx| rows::update_share(tx, &row)) {
            tracing::warn!(%error, "a network share row could not be updated");
        }
    }

    /// Gives the library folders below `share` to the after-connect hook.
    fn hand_over(&self, share: &NetworkShare) {
        let Some(hook) = locked(&self.after_connect).clone() else {
            return;
        };
        match self.folders_of(share) {
            Ok(folders) => hook(folders),
            Err(error) => {
                tracing::warn!(%error, "the library folders of a share could not be read")
            }
        }
    }

    /// The library folders (data source ids) below `share`.
    fn folders_of(&self, share: &NetworkShare) -> Result<Vec<i64>, DbError> {
        let sources = self
            .db
            .read(|c| cv_db::repo::data_sources::list_sources(c, None))?;
        Ok(sources
            .into_iter()
            .filter(|s| paths::is_within_with(&s.path, &share.local_root, paths::CASE_SENSITIVE))
            .map(|s| s.id)
            .collect())
    }

    /// What the shares say about the library folder at `path`: [`ShareState::Connecting`] while
    /// its share is being connected at start, [`ShareState::Failed`] after its last connect
    /// failed, else nothing.
    #[must_use]
    pub fn folder_state(&self, path: &str) -> Option<ShareState> {
        self.state_of(&self.share_of(path)?)
    }

    /// The share the library folder at `path` is on, if CineVault connected one for it.
    #[must_use]
    pub fn share_of(&self, path: &str) -> Option<NetworkShare> {
        let shares = self.list().ok()?;
        rows::share_holding(&shares, path, paths::CASE_SENSITIVE).cloned()
    }

    /// What `share` says about its folders (see [`Self::folder_state`]).
    #[must_use]
    pub fn state_of(&self, share: &NetworkShare) -> Option<ShareState> {
        if locked(&self.connecting).contains(&share.id) {
            return Some(ShareState::Connecting);
        }
        let code = share.last_error.as_deref()?;
        Some(ShareState::Failed(OfflineReason::from_code(code)))
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
            redact::release_secret(&memory_owner(id));
            return Ok(());
        }
        if let Err(error) = self.secrets.delete(&account) {
            tracing::debug!(%error, "no stored share password to remove");
        }
        if remember {
            locked(&self.unsaved).remove(&id);
            redact::release_secret(&memory_owner(id));
        } else {
            locked(&self.unsaved).insert(id, password.clone());
            redact::hold_secret(&memory_owner(id), password.expose());
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
        locked(&self.unsaved).remove(&id);
        redact::release_secret(&memory_owner(id));
        if let Err(error) = self.db.write(move |tx| rows::delete_share(tx, id)) {
            tracing::warn!(%error, "a network share row could not be removed");
        }
    }

    /// Forgets every share that has been unused for at least `unused_for` (see the module notes),
    /// unless work is running, and returns how many. Call it off the UI thread: a disconnect runs
    /// the system's tool.
    pub fn sweep(&self, unused_for: Duration) -> usize {
        let due_ms =
            now_ms().saturating_sub(i64::try_from(unused_for.as_millis()).unwrap_or(i64::MAX));
        let due: Vec<NetworkShare> = match self.list() {
            Ok(shares) => shares
                .into_iter()
                .filter(|share| share.unused_since_ms.is_some_and(|since| since <= due_ms))
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "the network shares could not be read");
                return 0;
            }
        };
        if due.is_empty() {
            return 0;
        }
        let busy = locked(&self.busy).clone();
        if busy.is_some_and(|busy| busy()) {
            tracing::debug!("work is running; unused network shares are forgotten later");
            return 0;
        }
        due.iter().filter(|share| self.forget(share)).count()
    }

    /// Disconnects a share, deletes its password and its row. Returns whether it was forgotten.
    fn forget(&self, share: &NetworkShare) -> bool {
        // A folder added on it since it was marked keeps it.
        if self
            .folders_of(share)
            .map_or(true, |folders| !folders.is_empty())
        {
            return false;
        }
        if let Ok(address) = address_of(share) {
            self.disconnect_quietly(&address);
        }
        if let Err(error) = self.secrets.delete(&secret_account(share.id)) {
            tracing::warn!(%error, "the password of an unused network share could not be deleted; trying later");
            return false;
        }
        locked(&self.unsaved).remove(&share.id);
        redact::release_secret(&memory_owner(share.id));
        let id = share.id;
        match self.db.write(move |tx| rows::delete_share(tx, id)) {
            Ok(_) => {
                tracing::info!(server = %share.server, share = %share.share, "unused network share forgotten");
                true
            }
            Err(error) => {
                tracing::warn!(%error, "an unused network share could not be removed");
                false
            }
        }
    }

    /// Removes the mount folders CineVault made for shares it no longer knows (see the module
    /// notes). Never recursive: only empty folders that are not mount points go.
    pub fn clean_mount_folders(&self) {
        let Some(base) = &self.mount_folders else {
            return;
        };
        let Ok(shares) = self.list() else {
            return;
        };
        let known = |folder: &Path| {
            shares
                .iter()
                .any(|share| paths::same_path(Path::new(&share.local_root), folder))
        };
        for server in subfolders(base) {
            for folder in subfolders(&server) {
                if known(&folder) {
                    continue;
                }
                if (self.is_mounted)(&folder) {
                    let names = (server.file_name(), folder.file_name());
                    if let (Some(host), Some(name)) = names
                        && let Ok(address) =
                            ShareAddress::new(&host.to_string_lossy(), &name.to_string_lossy())
                    {
                        self.disconnect_quietly(&address);
                    }
                    if (self.is_mounted)(&folder) {
                        tracing::info!(folder = %folder.display(), "a share mount folder is still mounted; left alone");
                        continue;
                    }
                }
                // `remove_dir` removes only an empty folder.
                if std::fs::remove_dir(&folder).is_ok() {
                    tracing::info!(folder = %folder.display(), "removed an unused share mount folder");
                }
            }
            let _ = std::fs::remove_dir(&server);
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

/// The folders right inside `folder`, links not followed.
fn subfolders(folder: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path())
                .collect()
        })
        .unwrap_or_default()
}

/// `~/CineVault Shares` on macOS, where the connector mounts shares; nowhere elsewhere.
fn own_mount_folders() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|home| home.join(paths::MAC_SHARE_MOUNTS))
    } else {
        None
    }
}

fn mount_check() -> MountCheck {
    #[cfg(unix)]
    {
        Arc::new(paths::is_mount_point)
    }
    #[cfg(not(unix))]
    {
        Arc::new(|_| false)
    }
}

/// The library a folder suits, from what it holds; movies when it cannot be read or is mixed.
fn detect_kind(folder: &Path) -> SourceKind {
    match analyze_folder(folder).map(|found| found.suggested) {
        Ok(SuggestedKind::Tv) => SourceKind::Tv,
        _ => SourceKind::Movie,
    }
}

/// The address of a known share.
fn address_of(share: &NetworkShare) -> Result<ShareAddress, ShareError> {
    ShareAddress::new(&share.server, &share.share)
        .map_err(|error| ShareError::new(ShareErrorKind::Other, error.to_string()))
}

#[cfg(test)]
mod tests;

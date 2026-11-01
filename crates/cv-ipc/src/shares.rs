//! Typed commands for network shares: what this computer can connect, connecting a share and
//! adding its folder, connecting a known share again, stopping an attempt, and the known shares.
//!
//! The rules live in `cv_app::network_shares`; this module converts to DTOs and runs the system's
//! tools off the async workers. A password comes in as text and never goes back out: no DTO that
//! leaves holds one, and the input types hide it from `Debug`, so a logged command shows none.

use std::sync::{Arc, Mutex, PoisonError};

use cv_app::network_shares::{
    ConnectRequest, NetworkShareService, OfflineReason, ShareAdded, ShareConnectError,
};
use cv_app::redact;
use cv_app::scanning::ScanService;
use cv_app::services::AppServices;
use cv_db::repo::network_shares::NetworkShare;
use cv_shares::{Address, Cancel, InputError, Login, Password, Support};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::sources::{DataSourceDto, SourceKindDto};

/// Held in Tauri state.
pub struct SharesState {
    service: Option<Arc<NetworkShareService>>,
    /// Told when the list of folders changes, so the folder watcher follows it.
    scan: Option<Arc<ScanService>>,
    /// Stops the connect that is running (Esc on the page); one runs at a time.
    running: Mutex<Option<Cancel>>,
}

impl SharesState {
    /// The share service of the app services; without them every command says so.
    pub fn from_services(services: Option<&Arc<AppServices>>) -> Self {
        Self {
            service: services.map(|services| Arc::clone(services.shares())),
            scan: services.map(|services| Arc::clone(services.scan())),
            running: Mutex::new(None),
        }
    }

    fn service(&self) -> Result<Arc<NetworkShareService>, ShareCommandError> {
        self.service.clone().ok_or_else(|| {
            ShareCommandError::new(
                "storage",
                "CineVault could not open the library, so it cannot connect shares.",
            )
        })
    }

    /// Starts an attempt, or refuses while another one runs.
    fn begin(&self) -> Result<Attempt<'_>, ShareCommandError> {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if running.is_some() {
            return Err(ShareCommandError::new(
                "busy",
                "A network share is being connected.",
            ));
        }
        let cancel = Cancel::new();
        *running = Some(cancel.clone());
        Ok(Attempt {
            state: self,
            cancel,
        })
    }

    /// Restarts the folder watcher for the current list of folders (blocking: it probes them).
    async fn watch_again(&self) {
        if let Some(scan) = self.scan.clone() {
            let _ = cv_jobs::client::spawn_blocking(move || scan.refresh_watcher()).await;
        }
    }
}

/// The connect that is running; it is over, however it ends, when this is dropped.
struct Attempt<'a> {
    state: &'a SharesState,
    cancel: Cancel,
}

impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        *self
            .state
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// Why a share command did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ShareCommandError {
    /// Stable code: `invalid_address`, `invalid_user`, `invalid_password`, `wrong_login`,
    /// `unreachable`, `share_not_found`, `folder_not_found`, `other_user_connected`,
    /// `session_in_use`, `account_locked`, `in_use`, `no_user_mounting`, `cancelled`, `other`, the
    /// folder rules' codes (`already_added`, `inside_existing`, …), `no_keyring`,
    /// `credential_store`, `unknown_share`, `busy` or `storage`.
    pub code: String,
    /// Plain sentence for the user; the system's own message for `other`.
    pub message: String,
}

impl ShareCommandError {
    fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }
}

impl From<ShareConnectError> for ShareCommandError {
    fn from(error: ShareConnectError) -> Self {
        // The tools scrub the password from their messages already; this keeps it out of what
        // the page shows whatever else produced the text.
        Self {
            code: error.code().to_owned(),
            message: redact::redact(&error.to_string()).into_owned(),
        }
    }
}

impl From<InputError> for ShareCommandError {
    fn from(error: InputError) -> Self {
        let code = match error {
            InputError::EmptyUser | InputError::BadUser => "invalid_user",
            InputError::BadPassword => "invalid_password",
            _ => "invalid_address",
        };
        Self::new(code, &error.to_string())
    }
}

/// The operating system, which decides where the page says a share appears and which credential
/// store keeps its password.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ShareSystemDto {
    Windows,
    Macos,
    Linux,
}

impl ShareSystemDto {
    const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

/// What this computer can do with network shares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ShareSupportDto {
    /// False on a Linux desktop without user-level share mounting: the page explains the
    /// system's own way instead of asking for a password.
    pub supported: bool,
    pub system: ShareSystemDto,
    /// The folder a connected share appears in, on macOS and Linux; nothing on Windows, where a
    /// share keeps its network path.
    pub mount_base: Option<String>,
}

/// Why the folders of a share are offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OfflineReasonDto {
    WrongPassword,
    Unreachable,
    ShareNotFound,
    PasswordNeeded,
    /// Windows still holds a sign-in to the server with the earlier password.
    NeedsRestart,
    /// Any other failure: another user connected, a locked account, the system's own message.
    NotConnected,
}

impl From<OfflineReason> for OfflineReasonDto {
    fn from(reason: OfflineReason) -> Self {
        match reason {
            OfflineReason::WrongPassword => Self::WrongPassword,
            OfflineReason::Unreachable => Self::Unreachable,
            OfflineReason::ShareNotFound => Self::ShareNotFound,
            OfflineReason::PasswordNeeded => Self::PasswordNeeded,
            OfflineReason::NeedsRestart => Self::NeedsRestart,
            OfflineReason::NotConnected => Self::NotConnected,
        }
    }
}

/// One network share CineVault connected. It never holds the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NetworkShareDto {
    pub id: u32,
    /// The share as it is written: `\\nas\movies`.
    pub address: String,
    pub user: String,
    /// Whether the password is kept in the system credential store.
    pub remember: bool,
    /// Why the last connect failed; nothing when it worked.
    pub reason: Option<OfflineReasonDto>,
}

impl From<&NetworkShare> for NetworkShareDto {
    fn from(share: &NetworkShare) -> Self {
        Self {
            id: u32::try_from(share.id).unwrap_or(u32::MAX),
            address: format!(r"\\{}\{}", share.server, share.share),
            user: share.user.clone(),
            remember: share.remember,
            reason: share
                .last_error
                .as_deref()
                .map(|code| OfflineReason::from_code(code).into()),
        }
    }
}

/// What the Network share page sends to connect a share and add its folder.
#[derive(Clone, Deserialize, Type)]
pub struct ConnectShareInput {
    /// `\\nas\movies\Films`, `//nas/movies/Films` or `smb://nas/movies/Films`.
    pub address: String,
    /// The library the folder goes to; left out, what the folder holds decides, as "Add folder…"
    /// does.
    pub kind: Option<SourceKindDto>,
    /// With an optional `DOMAIN\` part. Empty takes the user an `smb://user@…` address names.
    pub user: String,
    pub password: String,
    /// Keep the password in the system credential store.
    pub remember: bool,
}

impl std::fmt::Debug for ConnectShareInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectShareInput")
            .field("address", &redact::redact(&self.address))
            .field("kind", &self.kind)
            .field("user", &self.user)
            .field("password", &redact::MASK)
            .field("remember", &self.remember)
            .finish()
    }
}

impl ConnectShareInput {
    fn into_request(self) -> Result<ConnectRequest, ShareCommandError> {
        let address = Address::parse(&self.address)?;
        let user = match (self.user.trim(), &address.user) {
            ("", Some(named)) => named.clone(),
            (typed, _) => typed.to_owned(),
        };
        Ok(ConnectRequest {
            login: Login::new(&user, Password::new(self.password)?)?,
            kind: self.kind.map(Into::into),
            remember: self.remember,
            address,
        })
    }
}

/// What the Network share page sends to connect a known share again ("Connect…").
#[derive(Clone, Deserialize, Type)]
pub struct ReconnectShareInput {
    pub id: u32,
    pub user: String,
    pub password: String,
    pub remember: bool,
}

impl std::fmt::Debug for ReconnectShareInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReconnectShareInput")
            .field("id", &self.id)
            .field("user", &self.user)
            .field("password", &redact::MASK)
            .field("remember", &self.remember)
            .finish()
    }
}

/// A share connected and its folder added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ShareAddedDto {
    pub share: NetworkShareDto,
    pub source: DataSourceDto,
}

impl From<ShareAdded> for ShareAddedDto {
    fn from(added: ShareAdded) -> Self {
        Self {
            share: (&added.share).into(),
            source: added.source.into(),
        }
    }
}

/// What this computer can do with network shares. Asking a Linux desktop runs a tool, so it runs
/// off the async workers.
pub async fn network_share_support(
    state: &SharesState,
) -> Result<ShareSupportDto, ShareCommandError> {
    let service = state.service()?;
    let support = cv_jobs::client::spawn_blocking(move || service.support())
        .await
        .map_err(|error| ShareCommandError::new("storage", &error.to_string()))?;
    let system = ShareSystemDto::current();
    let mount_base = match system {
        ShareSystemDto::Windows => None,
        ShareSystemDto::Macos => Some(format!("~/{}", cv_shares::MAC_MOUNT_FOLDER)),
        ShareSystemDto::Linux => {
            let runtime =
                std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_owned());
            Some(format!("{runtime}/gvfs"))
        }
    };
    Ok(ShareSupportDto {
        supported: support == Support::Supported,
        system,
        mount_base,
    })
}

/// Connects the share with the login and adds the folder inside it as a library folder. Runs the
/// system's tool, so it takes a while; [`cancel_network_share_connect`] stops it.
pub async fn connect_network_share(
    state: &SharesState,
    input: ConnectShareInput,
) -> Result<ShareAddedDto, ShareCommandError> {
    let request = input.into_request()?;
    let service = state.service()?;
    let attempt = state.begin()?;
    let cancel = attempt.cancel.clone();
    let added = cv_jobs::client::spawn_blocking(move || {
        let added = service.connect_and_add(&request, &cancel);
        added
            .map(ShareAddedDto::from)
            .map_err(ShareCommandError::from)
    })
    .await
    .map_err(|error| ShareCommandError::new("storage", &error.to_string()))??;
    drop(attempt);
    state.watch_again().await;
    Ok(added)
}

/// Connects a known share again with a new login; its folders come online and are scanned.
pub async fn connect_network_share_again(
    state: &SharesState,
    input: ReconnectShareInput,
) -> Result<NetworkShareDto, ShareCommandError> {
    let login = Login::new(&input.user, Password::new(input.password.clone())?)?;
    let service = state.service()?;
    let attempt = state.begin()?;
    let cancel = attempt.cancel.clone();
    let (id, remember) = (i64::from(input.id), input.remember);
    cv_jobs::client::spawn_blocking(move || {
        let share = service.connect_again(id, &login, remember, &cancel);
        share
            .map(|share| NetworkShareDto::from(&share))
            .map_err(ShareCommandError::from)
    })
    .await
    .map_err(|error| ShareCommandError::new("storage", &error.to_string()))?
}

/// Stops the connect that is running. Returns false when none is.
pub fn cancel_network_share_connect(state: &SharesState) -> bool {
    let running = state.running.lock().unwrap_or_else(PoisonError::into_inner);
    running.as_ref().map(Cancel::cancel).is_some()
}

/// Every share CineVault connected, oldest first.
pub async fn list_network_shares(
    state: &SharesState,
) -> Result<Vec<NetworkShareDto>, ShareCommandError> {
    let service = state.service()?;
    let shares = cv_jobs::client::spawn_blocking(move || service.list())
        .await
        .map_err(|error| ShareCommandError::new("storage", &error.to_string()))?
        .map_err(|error| ShareCommandError::new("storage", &error.to_string()))?;
    Ok(shares.iter().map(Into::into).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_logged_input_holds_no_password() {
        let connect = ConnectShareInput {
            address: "smb://alex:Example-Pass-42@nas/movies".to_owned(),
            kind: None,
            user: "alex".to_owned(),
            password: "Example-Pass-42".to_owned(),
            remember: true,
        };
        let again = ReconnectShareInput {
            id: 3,
            user: "alex".to_owned(),
            password: "Example-Pass-42".to_owned(),
            remember: false,
        };
        for text in [
            format!("{connect:?}"),
            format!("{connect:#?}"),
            format!("{again:?}"),
        ] {
            assert!(!text.contains("Example-Pass-42"), "{text}");
            assert!(text.contains("alex"), "{text}");
        }
    }

    #[test]
    fn the_address_user_fills_a_missing_user_and_a_typed_one_wins() {
        let input = |user: &str| ConnectShareInput {
            address: r"smb://sam@nas/movies/Films".to_owned(),
            kind: Some(SourceKindDto::Tv),
            user: user.to_owned(),
            password: "pw".to_owned(),
            remember: true,
        };
        let request = input("").into_request().unwrap();
        assert_eq!(request.login.user(), "sam");
        assert_eq!(request.kind, Some(cv_domain::SourceKind::Tv));
        assert_eq!(request.address.folder, ["Films"]);
        assert_eq!(input("alex").into_request().unwrap().login.user(), "alex");
    }

    #[test]
    fn a_bad_input_says_which_field_is_wrong() {
        let bad = |address: &str, user: &str, password: &str| {
            ConnectShareInput {
                address: address.to_owned(),
                kind: None,
                user: user.to_owned(),
                password: password.to_owned(),
                remember: true,
            }
            .into_request()
            .unwrap_err()
            .code
        };
        assert_eq!(bad("movies", "alex", "pw"), "invalid_address");
        assert_eq!(bad(r"\\nas\movies", "", "pw"), "invalid_user");
        assert_eq!(bad(r"\\nas\movies", "alex", "a\nb"), "invalid_password");
    }

    #[test]
    fn a_share_shows_its_reason_and_never_a_password() {
        let share = NetworkShare {
            id: 4,
            server: "nas".to_owned(),
            share: "movies".to_owned(),
            user: "alex".to_owned(),
            local_root: r"\\nas\movies".to_owned(),
            remember: true,
            created_ms: 0,
            last_connected_ms: None,
            last_error: Some("wrong_login".to_owned()),
            unused_since_ms: None,
            connected_by_us: false,
        };
        let dto = NetworkShareDto::from(&share);
        assert_eq!(dto.address, r"\\nas\movies");
        assert_eq!(dto.reason, Some(OfflineReasonDto::WrongPassword));
    }

    #[test]
    fn only_one_connect_runs_at_a_time_and_cancel_stops_it() {
        let state = SharesState {
            service: None,
            scan: None,
            running: Mutex::new(None),
        };
        assert!(!cancel_network_share_connect(&state));
        let attempt = state.begin().unwrap();
        assert_eq!(state.begin().err().map(|e| e.code), Some("busy".to_owned()));
        assert!(cancel_network_share_connect(&state));
        assert!(attempt.cancel.is_cancelled());
        drop(attempt);
        assert!(state.begin().is_ok());
    }
}

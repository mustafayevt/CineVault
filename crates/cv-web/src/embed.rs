//! The browser server inside the desktop app ("Browser access").
//!
//! The desktop app already has the library open, so the server it runs here boots nothing: it
//! serves the app's own `AppServices`, image index, batch runner, drives service and move slot
//! ([`Shared`]). It takes no owner file and opens no second library; the command line keeps
//! forwarding to the app. [`start`] binds the port at once, so "port in use" is a typed error,
//! and [`RunningServer::stop`] ends every session, closes every stream and the port, and stops the
//! server's tasks.
//!
//! The settings of the desktop's Browser access section live in `browser-access.json` in the data
//! folder ([`BrowserAccessConfig`]), not in `settings.json`, so a browser cannot change them
//! through the settings commands.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use cv_app::services::AppServices;
use cv_ipc::image::ImageContext;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::auth::{self, Auth, AuthError, HostMode, SessionInfo};
use crate::server;
use crate::state::{Borrowed, Commands, DeleteGuard, Streams, UiAssets, WebState};

pub use crate::network::{is_home_address, lan_addresses};

/// The port the section proposes first, the same as the headless server's.
pub const DEFAULT_PORT: u16 = 8787;
/// The lowest port the section accepts; below it a port needs administrator rights on Linux.
pub const MIN_PORT: u16 = 1024;
/// The file the section's settings live in, inside the data folder.
pub const CONFIG_FILE: &str = "browser-access.json";
/// How long a stop waits for open connections before it drops them.
const STOP_GRACE: Duration = Duration::from_secs(3);

/// The Browser access settings the desktop keeps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BrowserAccessConfig {
    pub enabled: bool,
    pub port: u16,
    pub lan_only: bool,
    pub keep_in_tray: bool,
}

impl Default for BrowserAccessConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            lan_only: true,
            keep_in_tray: false,
        }
    }
}

impl BrowserAccessConfig {
    /// Reads the settings of a data folder; a missing or unreadable file is the defaults (off).
    #[must_use]
    pub fn load(data_root: &Path) -> Self {
        let mut config: Self = std::fs::read(data_root.join(CONFIG_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        if config.port < MIN_PORT {
            config.port = DEFAULT_PORT;
        }
        config
    }

    /// Writes the settings atomically.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn save(&self, data_root: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        cv_fs::write_atomic(&data_root.join(CONFIG_FILE), &bytes).map_err(std::io::Error::other)
    }
}

/// How the desktop wants the server to run.
pub struct EmbedOptions {
    pub port: u16,
    pub lan_only: bool,
    /// The computer's name, for "Change it in CineVault's Settings on <computer>".
    pub computer: String,
    pub ui: UiAssets,
}

/// What the desktop shares with the server.
pub struct Shared {
    pub services: Arc<AppServices>,
    pub images: ImageContext,
    pub borrowed: Borrowed,
}

/// Why the server did not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("Create a password first.")]
    NoPassword,
    #[error("Port {port} is in use by another program. Choose another port, such as {free}.")]
    PortInUse { port: u16, free: u16 },
    #[error("Choose a port from 1024 to 65535.")]
    BadPort,
    #[error("CineVault cannot listen on port {port}: {message}")]
    Bind { port: u16, message: String },
    #[error("{0}")]
    Auth(#[from] AuthError),
}

/// The server while it runs.
pub struct RunningServer {
    addr: SocketAddr,
    auth: Arc<Auth>,
    streams: Arc<Streams>,
    shutdown: Arc<Notify>,
    serve: JoinHandle<()>,
    feed: JoinHandle<()>,
}

/// Starts the server over the desktop's services. Must be called inside a tokio runtime (every
/// Tauri async command is). The port is bound before this returns.
///
/// # Errors
///
/// [`StartError::NoPassword`] without a password, [`StartError::PortInUse`] when another program
/// holds the port, and the others when the port or the sign-in file cannot be used.
pub fn start(options: EmbedOptions, shared: Shared) -> Result<RunningServer, StartError> {
    if options.port < MIN_PORT {
        return Err(StartError::BadPort);
    }
    let data_root = shared.services.location().root().to_path_buf();
    let auth = Arc::new(Auth::open_as(
        &data_root,
        HostMode::Desktop {
            computer: options.computer.clone(),
        },
    )?);
    if !auth.has_password() {
        return Err(StartError::NoPassword);
    }
    let listener = bind(options.port)?;
    let addr = listener.local_addr().map_err(|error| StartError::Bind {
        port: options.port,
        message: error.to_string(),
    })?;
    let listener =
        tokio::net::TcpListener::from_std(listener).map_err(|error| StartError::Bind {
            port: options.port,
            message: error.to_string(),
        })?;
    server::clean_uploads(&data_root);

    let images = Arc::new(shared.images);
    let commands = Arc::new(Commands::build(&shared.services, &images, shared.borrowed));
    let streams = Arc::new(Streams::default());
    let state = WebState {
        notices: cv_ipc::notices::NoticeHub::from_app(Arc::clone(shared.services.notices())),
        services: shared.services,
        images,
        auth: Arc::clone(&auth),
        progress: Arc::new(cv_ipc::jobs::ProgressHub::default()),
        streams: Arc::clone(&streams),
        commands,
        delete_guard: Arc::new(DeleteGuard::default()),
        data_root,
        bind: Ipv4Addr::UNSPECIFIED.to_string(),
        port: addr.port(),
        keys_locked: false,
        // The desktop trusts no proxy: the socket peer is the client.
        trust_proxy: Vec::new(),
        ui: options.ui,
        lan_only: options.lan_only,
    };
    let feed = server::wire(&state);
    let app = server::router(state);
    let shutdown = Arc::new(Notify::new());
    let signal = Arc::clone(&shutdown);
    let serve = tokio::spawn(async move {
        let result = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move { signal.notified().await })
        .await;
        if let Err(error) = result {
            tracing::warn!(%error, "browser access stopped with an error");
        }
    });
    tracing::info!(port = addr.port(), "browser access started");
    Ok(RunningServer {
        addr,
        auth,
        streams,
        shutdown,
        serve,
        feed,
    })
}

/// Binds every IPv4 address on `port`, telling "in use" apart from other failures.
fn bind(port: u16) -> Result<TcpListener, StartError> {
    let listener = match TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)) {
        Ok(listener) => listener,
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            return Err(StartError::PortInUse {
                port,
                free: free_port_after(port),
            });
        }
        Err(error) => {
            return Err(StartError::Bind {
                port,
                message: error.to_string(),
            });
        }
    };
    listener
        .set_nonblocking(true)
        .map_err(|error| StartError::Bind {
            port,
            message: error.to_string(),
        })?;
    Ok(listener)
}

/// The first port after `port` that nothing listens on, for the "choose another port" sentence.
#[must_use]
pub fn free_port_after(port: u16) -> u16 {
    (port.saturating_add(1)..=port.saturating_add(50))
        .find(|candidate| TcpListener::bind((Ipv4Addr::UNSPECIFIED, *candidate)).is_ok())
        .unwrap_or_else(|| port.saturating_add(1))
}

impl RunningServer {
    /// The port it listens on.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// The signed-in browsers, the most recently used first.
    #[must_use]
    pub fn sessions(&self) -> Vec<SessionInfo> {
        self.auth.sessions()
    }

    /// Signs one browser out by its public handle and ends its streams.
    pub fn sign_out(&self, handle: &str) -> bool {
        match self.auth.logout_handle(handle) {
            Some(id) => {
                self.streams.close_session(&id);
                true
            }
            None => false,
        }
    }

    /// Signs every browser out and ends every stream.
    pub fn sign_out_all(&self) {
        self.auth.logout_all_except(None);
        self.streams.close_all();
    }

    /// Ends every session and stream, closes the port and stops the tasks. Returns once the port
    /// no longer accepts connections; a connection that will not close is dropped after three
    /// seconds.
    pub async fn stop(self) {
        let Self {
            auth,
            streams,
            shutdown,
            mut serve,
            feed,
            addr,
        } = self;
        auth.logout_all_except(None);
        streams.close_all();
        feed.abort();
        shutdown.notify_one();
        if tokio::time::timeout(STOP_GRACE, &mut serve).await.is_err() {
            serve.abort();
            let _ = serve.await;
        }
        tracing::info!(port = addr.port(), "browser access stopped");
    }
}

/// Whether the data folder has a browser password.
#[must_use]
pub fn has_password(data_root: &Path) -> bool {
    Auth::open_as(
        data_root,
        HostMode::Desktop {
            computer: String::new(),
        },
    )
    .is_ok_and(|auth| auth.has_password())
}

/// Sets the browser password from the desktop's Settings: the person is at the computer, so no
/// current password is asked for. A running server reads the new password at once; the caller
/// signs its browsers out.
///
/// # Errors
///
/// When the password is too short or does not match, or the file cannot be written.
pub fn set_password(data_root: &Path, password: &str, confirm: &str) -> Result<(), AuthError> {
    auth::check_new_password(password, confirm)?;
    let auth = Auth::open_as(
        data_root,
        HostMode::Desktop {
            computer: String::new(),
        },
    )?;
    auth.set_password_unattended(password)
}

/// The addresses to open from another device: each home-network address of the computer, then
/// `localhost`.
#[must_use]
pub fn addresses(port: u16) -> Vec<String> {
    let mut list: Vec<String> = lan_addresses()
        .into_iter()
        .map(|ip| format!("http://{ip}:{port}"))
        .collect();
    list.push(format!("http://localhost:{port}"));
    list
}

/// The computer's name, as the sign-in page and the owner file name it.
#[must_use]
pub fn computer_name() -> String {
    let name = cv_app::owner::host_name();
    if name == "unknown" {
        "this computer".to_owned()
    } else {
        name
    }
}

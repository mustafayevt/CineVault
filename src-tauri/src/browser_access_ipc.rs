//! Browser access: the desktop app serves the web interface to other devices on the network.
//!
//! The server is `cv_web::embed`, run inside this process over the library the app already has
//! open (see the `desktop-browser-access` design): no second process, no owner-file conflict, and
//! the command line keeps forwarding to the app. This module keeps the server's lifecycle (start
//! on launch when it was on, stop on quit, restart on a port change), the section's commands, the
//! tray icon and the close-to-tray rule, and "Start CineVault when I sign in".
//!
//! Every command here is desktop-only: the server's dispatcher answers them `not_applicable`, so
//! a browser cannot switch Browser access, change its password or its port.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cv_ipc::library_ops::LibraryOpsState;
use cv_web::embed::{self, BrowserAccessConfig, EmbedOptions, RunningServer, Shared, StartError};
use cv_web::state::{Borrowed, UiAssets, UiFile};
use serde::Serialize;
use specta::Type;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt as _;

use crate::automation_ipc::AutomationState;
use crate::boot::Boot;
use crate::drives_ipc::DrivesState;
use crate::img_ipc::ImageState;
use crate::instance;

/// The tray icon's id.
const TRAY_ID: &str = "browser-access";
/// The argument a start at sign-in carries.
pub const BACKGROUND_ARG: &str = "--background";

/// Held in Tauri state.
pub struct BrowserAccessState {
    data_root: Option<PathBuf>,
    inner: tokio::sync::Mutex<Inner>,
    /// Read by the close handler on the window thread, which must not wait for the mutex.
    hide_on_close: AtomicBool,
}

struct Inner {
    config: BrowserAccessConfig,
    running: Option<RunningServer>,
    problem: Option<Problem>,
}

enum Problem {
    PortInUse(String),
    Error(String),
}

/// One signed-in browser.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSessionDto {
    /// A public id for "Sign out"; never the session's cookie.
    pub handle: String,
    pub address: String,
    /// The browser's `User-Agent`, shortened by the interface.
    pub agent: String,
    /// Epoch milliseconds.
    pub signed_in_ms: f64,
    pub last_used_ms: f64,
}

/// What the Browser access section shows.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAccessDto {
    /// The library is open; without it nothing can be served.
    pub available: bool,
    /// The switch.
    pub enabled: bool,
    /// `off`, `running`, `portInUse`, `error` or `noPassword`.
    pub state: String,
    /// The sentence for `portInUse` and `error`.
    pub message: Option<String>,
    pub port: u16,
    /// What to open from another device, the computer's network address first.
    pub addresses: Vec<String>,
    pub lan_only: bool,
    pub has_password: bool,
    pub sessions: Vec<BrowserSessionDto>,
    pub keep_in_tray: bool,
    pub start_at_login: bool,
    /// Windows shows a firewall prompt the first time; the section explains it.
    pub windows: bool,
    pub computer: String,
}

/// Why a Browser access command did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct BrowserAccessError {
    /// `unavailable`, `password`, `port` or `storage`.
    pub code: String,
    pub message: String,
}

impl BrowserAccessError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

impl BrowserAccessState {
    /// Reads `browser-access.json` from the data folder. Nothing starts here; see
    /// [`start_on_launch`].
    pub fn from_boot(boot: &Boot) -> Self {
        let data_root = boot
            .services()
            .map(|services| services.location().root().to_path_buf());
        let config = data_root
            .as_deref()
            .map(BrowserAccessConfig::load)
            .unwrap_or_default();
        Self {
            hide_on_close: AtomicBool::new(config.enabled && config.keep_in_tray),
            data_root,
            inner: tokio::sync::Mutex::new(Inner {
                config,
                running: None,
                problem: None,
            }),
        }
    }

    /// Whether closing the main window hides it instead of quitting.
    pub fn hide_on_close(&self) -> bool {
        self.hide_on_close.load(Ordering::Relaxed)
    }

    /// Whether a start at sign-in should keep the window hidden.
    pub fn starts_hidden(&self) -> bool {
        self.hide_on_close()
    }

    /// Ends every browser session and closes the port without waiting, for quitting.
    pub fn stop_now(&self) {
        if let Ok(mut inner) = self.inner.try_lock()
            && let Some(running) = inner.running.take()
        {
            running.begin_stop();
        }
    }

    fn save(&self, config: &BrowserAccessConfig) -> Result<(), BrowserAccessError> {
        let Some(root) = &self.data_root else {
            return Err(unavailable());
        };
        config
            .save(root)
            .map_err(|error| BrowserAccessError::new("storage", error.to_string()))
    }
}

fn unavailable() -> BrowserAccessError {
    BrowserAccessError::new(
        "unavailable",
        "CineVault could not open the library, so it cannot be shared with a browser.",
    )
}

/// The interface the app was built with, served to browsers. A debug build against the dev
/// server reads the built `ui/dist` instead (Tauri does that by itself).
fn ui_assets(app: &AppHandle) -> UiAssets {
    let handle = app.clone();
    UiAssets::Embedded(Arc::new(move |path: &str| {
        handle
            .asset_resolver()
            .get(format!("/{path}"))
            .map(|asset| UiFile {
                bytes: asset.bytes,
                mime: asset.mime_type,
            })
    }))
}

/// Starts the server over the app's own services and state.
fn start_server(
    app: &AppHandle,
    config: &BrowserAccessConfig,
) -> Result<RunningServer, StartError> {
    let boot = app.state::<Boot>();
    let Some(services) = boot.services() else {
        return Err(StartError::Bind {
            port: config.port,
            message: "the library is not open".to_owned(),
        });
    };
    let images = app.state::<ImageState>().context();
    let ops = app.state::<LibraryOpsState>();
    let shared = Shared {
        services: Arc::clone(services),
        borrowed: Borrowed {
            actions: app.state::<AutomationState>().backend(),
            drives: app.state::<DrivesState>().service(),
            library_ops: Some(LibraryOpsState::sharing_moves_with(
                &ops,
                Arc::clone(images.sources()),
            )),
        },
        images,
    };
    embed::start(
        EmbedOptions {
            port: config.port,
            lan_only: config.lan_only,
            computer: embed::computer_name(),
            ui: ui_assets(app),
        },
        shared,
    )
}

/// Brings the server in line with the settings: started, stopped or restarted.
async fn reconcile(app: &AppHandle, state: &BrowserAccessState, inner: &mut Inner) {
    if let Some(running) = inner.running.take() {
        running.stop().await;
    }
    inner.problem = None;
    let has_password = state.data_root.as_deref().is_some_and(embed::has_password);
    if inner.config.enabled && has_password {
        match start_server(app, &inner.config) {
            Ok(running) => inner.running = Some(running),
            Err(error @ StartError::PortInUse { .. }) => {
                tracing::warn!(%error, "browser access did not start");
                inner.problem = Some(Problem::PortInUse(error.to_string()));
            }
            Err(error) => {
                tracing::warn!(%error, "browser access did not start");
                inner.problem = Some(Problem::Error(error.to_string()));
            }
        }
    }
    state.hide_on_close.store(
        inner.config.enabled && inner.config.keep_in_tray,
        Ordering::Relaxed,
    );
    sync_tray(app, inner.config.enabled && inner.config.keep_in_tray);
}

fn status(app: &AppHandle, state: &BrowserAccessState, inner: &Inner) -> BrowserAccessDto {
    let has_password = state.data_root.as_deref().is_some_and(embed::has_password);
    let (state_name, message) = match (&inner.running, &inner.problem) {
        (Some(_), _) => ("running", None),
        (None, Some(Problem::PortInUse(message))) => ("portInUse", Some(message.clone())),
        (None, Some(Problem::Error(message))) => ("error", Some(message.clone())),
        (None, None) if inner.config.enabled && !has_password => ("noPassword", None),
        (None, None) => ("off", None),
    };
    let sessions = inner
        .running
        .as_ref()
        .map(|running| {
            running
                .sessions()
                .into_iter()
                .map(|session| BrowserSessionDto {
                    handle: session.handle,
                    address: session.address,
                    agent: session.agent,
                    #[allow(clippy::cast_precision_loss)]
                    signed_in_ms: session.signed_in_ms as f64,
                    #[allow(clippy::cast_precision_loss)]
                    last_used_ms: session.last_used_ms as f64,
                })
                .collect()
        })
        .unwrap_or_default();
    let port = inner
        .running
        .as_ref()
        .map_or(inner.config.port, RunningServer::port);
    BrowserAccessDto {
        available: state.data_root.is_some(),
        enabled: inner.config.enabled,
        state: state_name.to_owned(),
        message,
        port,
        addresses: if inner.running.is_some() {
            embed::addresses(port)
        } else {
            Vec::new()
        },
        lan_only: inner.config.lan_only,
        has_password,
        sessions,
        keep_in_tray: inner.config.keep_in_tray,
        start_at_login: app.autolaunch().is_enabled().unwrap_or(false),
        windows: cfg!(windows),
        computer: embed::computer_name(),
    }
}

/// Starts Browser access when it was on at the last quit. Runs once the services have started.
pub async fn start_on_launch(app: &AppHandle) {
    let state = app.state::<BrowserAccessState>();
    let mut inner = state.inner.lock().await;
    if inner.config.enabled {
        reconcile(app, &state, &mut inner).await;
        if let Some(Problem::PortInUse(message) | Problem::Error(message)) = &inner.problem
            && let Some(services) = app.state::<Boot>().services()
        {
            services.notices().publish(
                cv_app::notices::AppNotice::new(
                    cv_app::notices::NoticeLevel::Warning,
                    "Browser access did not start",
                )
                .detail(message.clone())
                .source("Browser access")
                .settings_link("/settings?section=browser-access"),
            );
        }
    } else {
        sync_tray(app, false);
    }
}

/// Changes the settings with `change`, saves them and brings the server in line.
async fn update(
    app: &AppHandle,
    change: impl FnOnce(&mut BrowserAccessConfig),
) -> Result<BrowserAccessDto, BrowserAccessError> {
    let state = app.state::<BrowserAccessState>();
    let mut inner = state.inner.lock().await;
    let mut config = inner.config.clone();
    change(&mut config);
    state.save(&config)?;
    inner.config = config;
    reconcile(app, &state, &mut inner).await;
    Ok(status(app, &state, &inner))
}

/// The section's state.
#[tauri::command]
#[specta::specta]
pub async fn get_browser_access(app: AppHandle) -> Result<BrowserAccessDto, BrowserAccessError> {
    let state = app.state::<BrowserAccessState>();
    let inner = state.inner.lock().await;
    Ok(status(&app, &state, &inner))
}

/// Turns Browser access on or off. Off stops the server, closes the port and signs every browser
/// out. On needs a password.
#[tauri::command]
#[specta::specta]
pub async fn set_browser_access_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<BrowserAccessDto, BrowserAccessError> {
    let state = app.state::<BrowserAccessState>();
    if enabled && !state.data_root.as_deref().is_some_and(embed::has_password) {
        return Err(BrowserAccessError::new(
            "password",
            "Create a password first.",
        ));
    }
    update(&app, |config| config.enabled = enabled).await
}

/// Changes the port and restarts the server on it.
#[tauri::command]
#[specta::specta]
pub async fn set_browser_access_port(
    app: AppHandle,
    port: u16,
) -> Result<BrowserAccessDto, BrowserAccessError> {
    if port < embed::MIN_PORT {
        return Err(BrowserAccessError::new(
            "port",
            "Choose a port from 1024 to 65535.",
        ));
    }
    update(&app, |config| config.port = port).await
}

/// "Only devices on my home network". Restarts the server.
#[tauri::command]
#[specta::specta]
pub async fn set_browser_access_lan_only(
    app: AppHandle,
    on: bool,
) -> Result<BrowserAccessDto, BrowserAccessError> {
    update(&app, |config| config.lan_only = on).await
}

/// "Keep running in the tray when the window is closed".
#[tauri::command]
#[specta::specta]
pub async fn set_browser_access_tray(
    app: AppHandle,
    on: bool,
) -> Result<BrowserAccessDto, BrowserAccessError> {
    update(&app, |config| config.keep_in_tray = on).await
}

/// Creates or changes the browser password. The person is at the computer, so the current
/// password is not asked for. Every signed-in browser is signed out.
#[tauri::command]
#[specta::specta]
pub async fn set_browser_access_password(
    app: AppHandle,
    password: String,
    confirm: String,
) -> Result<BrowserAccessDto, BrowserAccessError> {
    let state = app.state::<BrowserAccessState>();
    let Some(root) = state.data_root.clone() else {
        return Err(unavailable());
    };
    let written = tauri::async_runtime::spawn_blocking(move || {
        embed::set_password(&root, &password, &confirm)
    })
    .await
    .map_err(|error| BrowserAccessError::new("storage", error.to_string()))?;
    written.map_err(|error| BrowserAccessError::new("password", error.to_string()))?;
    let mut inner = state.inner.lock().await;
    if let Some(running) = &inner.running {
        running.sign_out_all();
    } else if inner.config.enabled {
        // Switched on before a password existed: start now.
        reconcile(&app, &state, &mut inner).await;
    }
    Ok(status(&app, &state, &inner))
}

/// Signs one browser out by its handle, or every browser when no handle is given.
#[tauri::command]
#[specta::specta]
pub async fn sign_out_browsers(
    app: AppHandle,
    handle: Option<String>,
) -> Result<BrowserAccessDto, BrowserAccessError> {
    let state = app.state::<BrowserAccessState>();
    let inner = state.inner.lock().await;
    if let Some(running) = &inner.running {
        match handle {
            Some(handle) => {
                running.sign_out(&handle);
            }
            None => running.sign_out_all(),
        }
    }
    Ok(status(&app, &state, &inner))
}

/// "Start CineVault when I sign in": the per-user Run entry on Windows, an autostart `.desktop`
/// file on Linux, a login item on macOS.
#[tauri::command]
#[specta::specta]
pub async fn set_start_at_login(
    app: AppHandle,
    on: bool,
) -> Result<BrowserAccessDto, BrowserAccessError> {
    let launcher = app.autolaunch();
    let result = if on {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|error| {
        BrowserAccessError::new(
            "storage",
            format!("CineVault could not change the start at sign-in: {error}"),
        )
    })?;
    let state = app.state::<BrowserAccessState>();
    let inner = state.inner.lock().await;
    Ok(status(&app, &state, &inner))
}

/// Shows the tray icon while Browser access and "Keep running in the tray" are both on, and
/// removes it otherwise.
fn sync_tray(app: &AppHandle, show: bool) {
    let exists = app.tray_by_id(TRAY_ID).is_some();
    if show && !exists {
        if let Err(error) = build_tray(app) {
            tracing::warn!(%error, "the tray icon could not be shown");
        }
    } else if !show && exists {
        let _ = app.remove_tray_by_id(TRAY_ID);
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("tray-open", "Open CineVault").build(app)?;
    let quit_item = MenuItemBuilder::with_id("tray-quit", "Quit CineVault").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&open, &quit_item]).build()?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("CineVault: Browser access is on")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray-open" => {
                instance::focus_main_window(app);
            }
            "tray-quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                instance::focus_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// "Quit CineVault" from the tray: what closing the window does when the tray is off, including
/// an update waiting to install on close.
fn quit(app: &AppHandle) {
    if let Some(handle) = app.try_state::<BrowserAccessState>() {
        handle.hide_on_close.store(false, Ordering::Relaxed);
    }
    cv_app::guard::contain("update on quit", || {
        crate::updates_ipc::install_on_close(app)
    });
    app.exit(0);
}

/// Hides the main window instead of closing it, when the tray keeps Browser access running.
/// Returns whether it did.
pub fn hide_instead_of_close(app: &AppHandle) -> bool {
    let hide = app
        .try_state::<BrowserAccessState>()
        .is_some_and(|state| state.hide_on_close());
    if hide && let Some(window) = app.get_webview_window(instance::MAIN_WINDOW_LABEL) {
        let _ = window.hide();
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_hides_only_while_both_options_are_on() {
        for (enabled, tray, hides) in [
            (true, true, true),
            (true, false, false),
            (false, true, false),
            (false, false, false),
        ] {
            let state = BrowserAccessState {
                data_root: None,
                inner: tokio::sync::Mutex::new(Inner {
                    config: BrowserAccessConfig {
                        enabled,
                        keep_in_tray: tray,
                        ..BrowserAccessConfig::default()
                    },
                    running: None,
                    problem: None,
                }),
                hide_on_close: AtomicBool::new(enabled && tray),
            };
            assert_eq!(state.hide_on_close(), hides, "{enabled} {tray}");
        }
    }

    #[test]
    fn a_library_that_did_not_open_offers_nothing() {
        let state = BrowserAccessState {
            data_root: None,
            inner: tokio::sync::Mutex::new(Inner {
                config: BrowserAccessConfig::default(),
                running: None,
                problem: None,
            }),
            hide_on_close: AtomicBool::new(false),
        };
        assert_eq!(
            state.save(&BrowserAccessConfig::default()),
            Err(unavailable())
        );
    }
}

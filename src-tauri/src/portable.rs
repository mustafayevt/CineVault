//! Portable mode for the web view.
//!
//! `cv_app::data_location` already moves the library, settings and caches next to the executable
//! when a `portable` marker file sits there. The Windows web view (WebView2) keeps its own
//! profile (local storage, cache), so without this module a "portable" copy would still write to
//! `%LOCALAPPDATA%\app.cinevault.desktop`. In portable mode every window of the configuration is
//! given a profile folder inside the data folder instead.
//!
//! The window-state plugin keeps the window's size and place in `%APPDATA%\<identifier>` (it
//! always creates that folder, whatever file name it is given), so a portable copy does not use
//! it: it keeps the main window's bounds in `window-state.json` inside its data folder instead
//! ([`WindowBounds`]). A portable copy writes nothing outside its own folder.
//!
//! The configuration's own `dataDirectory` cannot do this: it only takes a folder relative to the
//! app's folder in `%LOCALAPPDATA%`, and the runtime does not pass it on to the web view. So in
//! portable mode the configured windows are not created at start-up ([`prepare`]); the app builds
//! them itself in its setup, with the absolute profile folder ([`PortableWindows::create`]).

use std::path::{Path, PathBuf};

use cv_app::data_location::{DataLocation, LocationMode};
use serde::{Deserialize, Serialize};
use tauri::utils::config::WindowConfig;
use tauri::{
    Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

use crate::instance::MAIN_WINDOW_LABEL;

/// Folder of the web view profile, inside the data folder.
pub const WEBVIEW_DIR: &str = "webview";

/// The main window's size and place, inside the data folder of a portable copy.
pub const WINDOW_STATE_FILE: &str = "window-state.json";

/// The smallest window a saved state may restore; anything smaller is a broken file.
const MIN_SIDE: u32 = 400;

/// The main window's bounds in physical pixels, and whether it was maximized. The bounds are the
/// window's last normal (not maximized) ones, so un-maximizing after a start puts it back there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// A monitor's work area in physical pixels: `(x, y, width, height)`.
pub type Area = (i32, i32, u32, u32);

impl WindowBounds {
    /// Whether the window would be usable on one of `areas`: big enough, and with at least a
    /// grip of its title bar (100 by 40 pixels) on a screen, so a monitor that was unplugged
    /// never leaves it out of reach.
    #[must_use]
    pub fn fits(&self, areas: &[Area]) -> bool {
        if self.width < MIN_SIDE || self.height < MIN_SIDE {
            return false;
        }
        areas.iter().any(|&(ax, ay, aw, ah)| {
            let (right, bottom) = (ax + aw as i32, ay + ah as i32);
            let grip_x = self.x.max(ax) < (self.x + self.width as i32).min(right) - 100;
            let grip_y = self.y >= ay && self.y + 40 <= bottom;
            grip_x && grip_y
        })
    }

    /// Reads a saved state; `None` when there is none or it cannot be read.
    #[must_use]
    pub fn read(path: &Path) -> Option<Self> {
        let text = std::fs::read(path).ok()?;
        serde_json::from_slice(&text).ok()
    }

    /// Writes the state through a temporary file, so a crash never leaves half a file.
    ///
    /// # Errors
    /// The data folder cannot be written.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&temp, path)
    }
}

fn work_areas<R: Runtime>(window: &WebviewWindow<R>) -> Vec<Area> {
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|monitor| {
            let area = monitor.work_area();
            (
                area.position.x,
                area.position.y,
                area.size.width,
                area.size.height,
            )
        })
        .collect()
}

/// Puts the main window where the last session left it, when that place is still on a screen.
fn restore<R: Runtime>(window: &WebviewWindow<R>, path: &Path) {
    let Some(saved) = WindowBounds::read(path) else {
        return;
    };
    if !saved.fits(&work_areas(window)) {
        return;
    }
    let size = window.set_size(PhysicalSize::new(saved.width, saved.height));
    let place = window.set_position(PhysicalPosition::new(saved.x, saved.y));
    if let Err(error) = size.and(place) {
        tracing::warn!(%error, "the saved window bounds could not be applied");
    }
    if saved.maximized {
        let _ = window.maximize();
    }
}

/// Saves the main window's bounds as it closes. A maximized or minimized window keeps the normal
/// bounds saved before, with the maximized flag.
fn save<R: Runtime>(window: &WebviewWindow<R>, path: &Path) {
    let maximized = window.is_maximized().unwrap_or(false);
    let minimized = window.is_minimized().unwrap_or(false);
    let previous = WindowBounds::read(path);
    let bounds = if maximized || minimized {
        match previous {
            Some(previous) => WindowBounds {
                maximized,
                ..previous
            },
            None => return,
        }
    } else {
        let (Ok(place), Ok(size)) = (window.outer_position(), window.inner_size()) else {
            return;
        };
        WindowBounds {
            x: place.x,
            y: place.y,
            width: size.width,
            height: size.height,
            maximized: false,
        }
    };
    if let Err(error) = bounds.write(path) {
        tracing::warn!(%error, "the window bounds could not be saved");
    }
}

/// The web view profile folder to use, or `None` when the data is not portable.
pub fn webview_dir(location: &DataLocation) -> Option<PathBuf> {
    (location.mode() == LocationMode::Portable).then(|| location.root().join(WEBVIEW_DIR))
}

/// The windows a portable copy builds itself, and the profile folder they share.
#[derive(Debug, Clone)]
pub struct PortableWindows {
    dir: PathBuf,
    windows: Vec<WindowConfig>,
}

impl PortableWindows {
    /// Where the main window's bounds are kept: beside the web view profile, in the data folder.
    #[must_use]
    pub fn state_file(&self) -> PathBuf {
        self.dir
            .parent()
            .unwrap_or(&self.dir)
            .join(WINDOW_STATE_FILE)
    }
}

impl PortableWindows {
    /// Builds the windows the configuration would have built, each with the profile folder.
    ///
    /// # Errors
    /// A window that cannot be built.
    pub fn create<R: Runtime, M: Manager<R>>(&self, manager: &M) -> tauri::Result<()> {
        for config in &self.windows {
            let mut config = config.clone();
            config.create = true;
            let window = WebviewWindowBuilder::from_config(manager, &config)?
                .data_directory(self.dir.clone())
                .build()?;
            if config.label == MAIN_WINDOW_LABEL {
                // The window-state plugin is not used in portable mode (see the module docs).
                let path = self.state_file();
                restore(&window, &path);
                let handle = window.clone();
                window.on_window_event(move |event| {
                    if matches!(event, WindowEvent::CloseRequested { .. }) {
                        save(&handle, &path);
                    }
                });
            }
        }
        Ok(())
    }
}

/// Takes the windows the configuration creates at start-up, so that the app builds them itself
/// with the profile folder `dir`. They stay in the configuration with `create` off.
pub fn take_windows(windows: &mut [WindowConfig], dir: &Path) -> PortableWindows {
    let taken = windows
        .iter_mut()
        .filter(|window| window.create)
        .map(|window| {
            window.create = false;
            window.clone()
        })
        .collect();
    PortableWindows {
        dir: dir.to_path_buf(),
        windows: taken,
    }
}

/// Applies portable mode to the app context before the app is built (Windows only: the other
/// platforms keep the web view profile in a system location and the marker is not offered there).
/// Returns the windows the app then has to build in its setup.
pub fn prepare(context: &mut tauri::Context) -> Option<PortableWindows> {
    if !cfg!(windows) {
        return None;
    }
    let dir = DataLocation::resolve_for_process().and_then(|l| webview_dir(&l))?;
    Some(take_windows(&mut context.config_mut().app.windows, &dir))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use cv_app::data_location::LocationInputs;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    use super::*;

    fn inputs(root: &Path) -> LocationInputs {
        let exe_dir = root.join("exe");
        fs::create_dir_all(&exe_dir).unwrap();
        LocationInputs {
            exe_dir,
            user_data_base: root.join("appdata"),
            user_config_base: root.join("config"),
        }
    }

    fn window(label: &str, create: bool) -> WindowConfig {
        WindowConfig {
            label: label.to_owned(),
            create,
            ..Default::default()
        }
    }

    #[test]
    fn portable_marker_puts_the_web_view_profile_in_the_data_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let i = inputs(tmp.path());
        fs::write(i.exe_dir.join("portable"), "").unwrap();
        let location = DataLocation::resolve(&i);
        assert_eq!(
            webview_dir(&location),
            Some(i.exe_dir.join("data").join(WEBVIEW_DIR))
        );
    }

    #[test]
    fn without_the_marker_the_web_view_keeps_its_default_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let i = inputs(tmp.path());
        assert_eq!(webview_dir(&DataLocation::resolve(&i)), None);
    }

    #[test]
    fn the_window_state_is_kept_in_the_data_folder_beside_the_web_view_profile() {
        let mut windows = vec![window("main", true)];
        let dir = PathBuf::from("data").join(WEBVIEW_DIR);
        let taken = take_windows(&mut windows, &dir);
        assert_eq!(
            taken.state_file(),
            PathBuf::from("data").join(WINDOW_STATE_FILE)
        );
    }

    #[test]
    fn saved_bounds_round_trip_and_restore_only_where_a_screen_shows_them() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(WINDOW_STATE_FILE);
        let bounds = WindowBounds {
            x: 120,
            y: 80,
            width: 1440,
            height: 900,
            maximized: true,
        };
        bounds.write(&path).unwrap();
        assert_eq!(WindowBounds::read(&path), Some(bounds));
        assert!(!tmp.path().join("window-state.json.tmp").exists());

        let screen: Area = (0, 0, 1920, 1040);
        assert!(bounds.fits(&[screen]));
        // On a monitor that is gone, or too small to be a window: the default place is kept.
        let away = WindowBounds { x: 3000, ..bounds };
        assert!(!away.fits(&[screen]));
        let above = WindowBounds { y: -500, ..bounds };
        assert!(!above.fits(&[screen]));
        let tiny = WindowBounds {
            width: 50,
            ..bounds
        };
        assert!(!tiny.fits(&[screen]));
        // A second screen to the left counts.
        let left = WindowBounds { x: -1500, ..bounds };
        assert!(left.fits(&[screen, (-1920, 0, 1920, 1040)]));
        // A file that is not a state is ignored.
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(WindowBounds::read(&path), None);
    }

    #[test]
    fn the_configured_windows_are_left_for_the_app_to_build() {
        let mut windows = vec![window("main", true), window("later", false)];
        let dir = PathBuf::from("data").join(WEBVIEW_DIR);
        let taken = take_windows(&mut windows, &dir);
        // The runtime no longer builds them (it would ignore the absolute folder)...
        assert!(windows.iter().all(|w| !w.create));
        // ...the app does, and only those the configuration would have built.
        let labels: Vec<&str> = taken.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, ["main"]);
        assert_eq!(taken.dir, dir);
    }

    #[test]
    fn the_taken_windows_are_built_on_the_app() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let mut windows = vec![window("main", true)];
        let tmp = tempfile::tempdir().unwrap();
        let taken = take_windows(&mut windows, &tmp.path().join(WEBVIEW_DIR));
        taken.create(&app).expect("windows build");
        assert!(app.get_webview_window("main").is_some());
    }
}

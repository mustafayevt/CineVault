//! Single-instance behaviour and window-state persistence.
//!
//! A second launch of CineVault must not start another process that could corrupt the library
//! database. `tauri-plugin-single-instance` makes the second process hand its arguments to the
//! running one and exit; [`focus_main_window`] is what the running one does with that hand-off.
//! `tauri-plugin-window-state` stores the main window's size, position and maximized state and
//! restores them on the next launch.

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_window_state::StateFlags;

/// Label of the main window (see `tauri.conf.json`).
pub const MAIN_WINDOW_LABEL: &str = "main";

/// Brings the main window to the front: shows it if hidden, restores it if minimized and gives
/// it keyboard focus. Returns `false` when the window does not exist.
pub fn focus_main_window<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        tracing::warn!("second launch: the main window does not exist");
        return false;
    };
    let mut focused = true;
    for result in [window.show(), window.unminimize(), window.set_focus()] {
        if let Err(error) = result {
            tracing::warn!(%error, "second launch: could not fully focus the main window");
            focused = false;
        }
    }
    focused
}

/// The single-instance plugin. It must be the first plugin registered, so a second process exits
/// before anything else (settings, database) is opened.
pub fn single_instance_plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_single_instance::init(|app, args, _cwd| {
        tracing::info!(
            arg_count = args.len(),
            "second launch detected; focusing the window"
        );
        focus_main_window(app);
    })
}

/// Window-state flags that are persisted: size, position, maximized and fullscreen. Decorations
/// are left out because the custom title bar owns them (`tauri.windows.conf.json`).
pub fn persisted_state_flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED | StateFlags::FULLSCREEN
}

/// The window-state plugin, restoring and saving the main window.
pub fn window_state_plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_window_state::Builder::default()
        .with_state_flags(persisted_state_flags())
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::WebviewWindowBuilder;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    fn app_with_main_window() -> tauri::App<MockRuntime> {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        WebviewWindowBuilder::new(&app, MAIN_WINDOW_LABEL, Default::default())
            .build()
            .expect("main window builds");
        app
    }

    #[test]
    fn second_launch_handler_shows_and_focuses_the_main_window() {
        let app = app_with_main_window();
        let window = app.get_webview_window(MAIN_WINDOW_LABEL).expect("window");
        window.hide().expect("hide");
        window.minimize().expect("minimize");

        assert!(focus_main_window(app.handle()));

        assert!(window.is_visible().expect("visible"));
        assert!(!window.is_minimized().expect("minimized"));
    }

    #[test]
    fn second_launch_without_a_window_is_harmless() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        assert!(!focus_main_window(app.handle()));
    }

    #[test]
    fn persisted_flags_keep_geometry_but_not_decorations() {
        let flags = persisted_state_flags();
        for needed in [
            StateFlags::SIZE,
            StateFlags::POSITION,
            StateFlags::MAXIMIZED,
        ] {
            assert!(flags.contains(needed));
        }
        assert!(!flags.contains(StateFlags::DECORATIONS));
    }

    #[test]
    fn plugins_register_on_a_runtime_in_the_required_order() {
        // The mock context has an empty identifier, which Linux turns into an invalid D-Bus name
        // for the single-instance plugin; give it the app's own.
        let mut context = mock_context(noop_assets());
        context.config_mut().identifier = "app.cinevault.desktop".into();
        let app = mock_builder()
            .plugin(single_instance_plugin())
            .plugin(window_state_plugin())
            .build(context);
        assert!(app.is_ok(), "{:?}", app.err());
    }
}

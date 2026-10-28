//! Custom window chrome: the native side of the title bar drawn by the UI.
//!
//! The title bar itself is React (`ui/src/features/shell`). The window is created without native
//! decorations on Windows (`tauri.windows.conf.json`) and on Linux (`tauri.linux.conf.json`), and
//! with an overlay title bar that keeps the native traffic lights on macOS
//! (`tauri.macos.conf.json`). On Windows 11 the snap-layout flyout needs a native hit-test region
//! over the maximize button, which `tauri-plugin-snap-layout` provides; it finds the button by the
//! id below.

/// DOM id of the Windows maximize button. Must match `MAXIMIZE_BUTTON_ID` in the UI.
pub const MAXIMIZE_BUTTON_ID: &str = "cv-caption-maximize";

#[cfg(test)]
mod tests {
    use super::MAXIMIZE_BUTTON_ID;
    use serde_json::Value;
    use std::path::Path;

    fn read_json(name: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
        let text = std::fs::read_to_string(&path).expect("config file is readable");
        serde_json::from_str(&text).expect("config file is valid JSON")
    }

    fn main_window(config: &Value) -> &Value {
        &config["app"]["windows"][0]
    }

    /// RFC 7396 JSON Merge Patch, as used by Tauri to layer platform config files
    /// over `tauri.conf.json`: objects merge key by key, arrays are replaced whole.
    fn merge_patch(target: &mut Value, patch: &Value) {
        match patch {
            Value::Object(patch_map) => {
                if !target.is_object() {
                    *target = Value::Object(Default::default());
                }
                let target_map = target.as_object_mut().expect("object");
                for (key, value) in patch_map {
                    if value.is_null() {
                        target_map.remove(key);
                    } else {
                        merge_patch(target_map.entry(key.clone()).or_insert(Value::Null), value);
                    }
                }
            }
            other => *target = other.clone(),
        }
    }

    fn effective_config(platform_file: &str) -> Value {
        let mut merged = read_json("tauri.conf.json");
        merge_patch(&mut merged, &read_json(platform_file));
        merged
    }

    #[test]
    fn platform_windows_keep_the_base_title_and_sizes_after_merge() {
        let base = read_json("tauri.conf.json");
        for file in ["tauri.windows.conf.json", "tauri.macos.conf.json"] {
            let merged = effective_config(file);
            let windows = merged["app"]["windows"].as_array().expect("windows array");
            assert_eq!(windows.len(), 1, "{file}: exactly one window");
            let window = &windows[0];
            for key in ["label", "title", "width", "height", "minWidth", "minHeight"] {
                assert!(
                    !window[key].is_null(),
                    "{file}: {key} is missing after merge"
                );
                assert_eq!(
                    window[key],
                    main_window(&base)[key],
                    "{file}: {key} changed"
                );
            }
            // The window never gets smaller than the app can lay out (spec app-shell).
            assert_eq!(window["minWidth"], 1100);
            assert_eq!(window["minHeight"], 680);
            // The first frame is the dark window ground, never white (spec desktop-interaction).
            assert_eq!(window["backgroundColor"], "#121211");
            // Browser zoom keys do nothing; the app follows the system's display scaling.
            assert_ne!(window["zoomHotkeysEnabled"], true);
        }
    }

    #[test]
    fn base_window_turns_browser_zoom_keys_off() {
        let config = read_json("tauri.conf.json");
        assert_eq!(main_window(&config)["zoomHotkeysEnabled"], false);
    }

    #[test]
    fn release_builds_have_no_developer_tools() {
        // Tauri opens the developer tools only in debug builds unless its `devtools` feature is
        // on, so the app must never ask for that feature.
        let manifest =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
                .expect("manifest is readable");
        for line in manifest
            .lines()
            .filter(|line| line.trim_start().starts_with("tauri ="))
        {
            assert!(
                !line.contains("devtools"),
                "tauri must not enable devtools: {line}"
            );
        }
    }

    #[test]
    fn windows_window_has_no_native_decorations() {
        let config = read_json("tauri.windows.conf.json");
        assert_eq!(main_window(&config)["label"], "main");
        assert_eq!(main_window(&config)["decorations"], false);
    }

    #[test]
    fn platform_linux_keeps_the_base_title_and_sizes_after_merge() {
        let base = read_json("tauri.conf.json");
        let merged = effective_config("tauri.linux.conf.json");
        let windows = merged["app"]["windows"].as_array().expect("windows array");
        assert_eq!(
            windows.len(),
            1,
            "tauri.linux.conf.json: exactly one window"
        );
        let window = &windows[0];
        for key in ["label", "title", "width", "height", "minWidth", "minHeight"] {
            assert!(
                !window[key].is_null(),
                "tauri.linux.conf.json: {key} is missing after merge"
            );
            assert_eq!(
                window[key],
                main_window(&base)[key],
                "tauri.linux.conf.json: {key} changed"
            );
        }
        assert_eq!(window["minWidth"], 1100);
        assert_eq!(window["minHeight"], 680);
        assert_eq!(window["decorations"], false);
    }

    #[test]
    fn macos_window_keeps_traffic_lights_over_an_overlay_title_bar() {
        let config = read_json("tauri.macos.conf.json");
        let window = main_window(&config);
        assert_eq!(window["label"], "main");
        assert_eq!(window["titleBarStyle"], "Overlay");
        assert_eq!(window["hiddenTitle"], true);
        // The lights sit in the sidebar panel's top row: the panel starts 8px in and its logo
        // row is 37px high, so the lights line up with the nav icons (x 18) and the row centre.
        assert_eq!(window["trafficLightPosition"]["x"], 18);
        assert_eq!(window["trafficLightPosition"]["y"], 20);
        // The default decorations stay on: the traffic lights are the native ones.
        assert_ne!(window["decorations"], false);
    }

    #[test]
    fn base_window_is_decorated_so_other_platforms_keep_native_controls() {
        let config = read_json("tauri.conf.json");
        assert_ne!(main_window(&config)["decorations"], false);
    }

    #[test]
    fn capability_grants_what_the_title_bar_calls() {
        let capability = read_json("capabilities/default.json");
        let granted: Vec<&str> = capability["permissions"]
            .as_array()
            .expect("permissions array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for needed in [
            "core:window:allow-minimize",
            "core:window:allow-toggle-maximize",
            "core:window:allow-close",
            "core:window:allow-start-dragging",
            "core:window:allow-internal-toggle-maximize",
            "core:window:allow-is-maximized",
            "snap-layout:default",
        ] {
            assert!(granted.contains(&needed), "missing permission {needed}");
        }
    }

    #[test]
    fn maximize_button_id_is_a_valid_dom_id() {
        assert!(!MAXIMIZE_BUTTON_ID.contains(char::is_whitespace));
        assert!(!MAXIMIZE_BUTTON_ID.is_empty());
    }
}

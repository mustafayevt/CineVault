//! Data-location resolution: per-user default, portable marker, custom folder.

use cv_app::data_location::{
    APP_DIR_NAME, DataLocation, LocationError, LocationInputs, LocationMode, POINTER_FILE,
    PORTABLE_MARKER,
};
use std::fs;
use tempfile::TempDir;

fn inputs(t: &TempDir) -> LocationInputs {
    let i = LocationInputs {
        exe_dir: t.path().join("app"),
        user_data_base: t.path().join("userdata"),
        user_config_base: t.path().join("userconfig"),
    };
    fs::create_dir_all(&i.exe_dir).unwrap();
    i
}

#[test]
fn default_mode_uses_per_user_app_data_folder() {
    let t = TempDir::new().unwrap();
    let i = inputs(&t);
    let loc = DataLocation::resolve(&i);
    assert_eq!(loc.mode(), LocationMode::UserDefault);
    assert_eq!(loc.root(), i.user_data_base.join(APP_DIR_NAME));
    assert!(loc.warning().is_none());
}

#[test]
fn portable_marker_puts_everything_in_data_beside_the_executable() {
    let t = TempDir::new().unwrap();
    let i = inputs(&t);
    fs::write(i.exe_dir.join(PORTABLE_MARKER), "").unwrap();
    let loc = DataLocation::resolve(&i);
    let data = i.exe_dir.join("data");
    assert_eq!(loc.mode(), LocationMode::Portable);
    assert_eq!(loc.root(), data);
    assert_eq!(loc.library_db(), data.join("library.db"));
    assert_eq!(loc.settings_file(), data.join("settings.json"));
    assert_eq!(loc.cache_dir(), data.join("cache"));
    loc.ensure_dirs().unwrap();
    assert!(data.join("cache").is_dir());
}

#[test]
fn a_portable_directory_is_not_a_marker() {
    let t = TempDir::new().unwrap();
    let i = inputs(&t);
    fs::create_dir(i.exe_dir.join(PORTABLE_MARKER)).unwrap();
    assert_eq!(DataLocation::resolve(&i).mode(), LocationMode::UserDefault);
}

#[test]
fn custom_folder_is_remembered_and_can_be_cleared() {
    let t = TempDir::new().unwrap();
    let i = inputs(&t);
    let chosen = t.path().join("my media data");
    DataLocation::set_custom_folder(&i, &chosen).unwrap();

    let loc = DataLocation::resolve(&i);
    assert_eq!(loc.mode(), LocationMode::Custom);
    assert_eq!(loc.root(), chosen);

    DataLocation::clear_custom_folder(&i).unwrap();
    DataLocation::clear_custom_folder(&i).unwrap(); // idempotent
    assert_eq!(DataLocation::resolve(&i).mode(), LocationMode::UserDefault);
}

#[test]
fn portable_marker_wins_over_custom_folder() {
    let t = TempDir::new().unwrap();
    let i = inputs(&t);
    DataLocation::set_custom_folder(&i, &t.path().join("custom")).unwrap();
    fs::write(i.exe_dir.join(PORTABLE_MARKER), "").unwrap();
    assert_eq!(DataLocation::resolve(&i).mode(), LocationMode::Portable);
}

#[test]
fn relative_custom_folder_is_rejected() {
    let t = TempDir::new().unwrap();
    let i = inputs(&t);
    let err = DataLocation::set_custom_folder(&i, std::path::Path::new("relative/dir"));
    assert!(matches!(err, Err(LocationError::NotAbsolute(_))));
}

#[test]
fn unusable_pointer_falls_back_with_a_warning_and_is_kept() {
    let t = TempDir::new().unwrap();
    let i = inputs(&t);
    let dir = i.user_config_base.join(APP_DIR_NAME);
    fs::create_dir_all(&dir).unwrap();
    let pointer = dir.join(POINTER_FILE);

    fs::write(&pointer, "not json").unwrap();
    let loc = DataLocation::resolve(&i);
    assert_eq!(loc.mode(), LocationMode::UserDefault);
    assert_eq!(loc.root(), i.user_data_base.join(APP_DIR_NAME));
    assert!(loc.warning().is_some());
    assert_eq!(fs::read_to_string(&pointer).unwrap(), "not json");

    fs::write(&pointer, r#"{"data_folder":"relative"}"#).unwrap();
    let loc = DataLocation::resolve(&i);
    assert_eq!(loc.mode(), LocationMode::UserDefault);
    assert!(loc.warning().is_some());
}

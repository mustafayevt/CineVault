//! The Play action: opens a movie or an episode in the external player the settings name.
//!
//! The files come from the library (the item's own video files, stacked parts in order, disc
//! folders as folders), never from the page. The player is read from the `player.*` settings on
//! every press, so a change on the settings page applies at once. Starting the player does not
//! wait for it to close. See `cv_app::playback` for the rules.

use std::collections::BTreeMap;
use std::path::Path;

use cv_app::app_settings::SettingsService;
use cv_app::playback::player::PlayerConfig;
use cv_app::playback::{
    CUSTOM_COMMAND_KEY, KIND_KEY, PATH_KEY, PlayError, PlayOutcome, play, player_config,
};
use cv_db::Database;
use cv_db::repo::get_item;
use cv_domain::ItemId;
use serde::Serialize;
use specta::Type;
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::detail_ipc::parse_id;
use crate::library_ipc::{LibraryCommandError, blocking, storage};
use crate::settings_ipc::SettingsState;
use crate::sources_ipc::LibraryState;

/// What pressing Play did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PlayOutcomeDto {
    /// The player that was started: `system`, `mpv`, `vlc`, `mpc-hc`, `iina` or `custom`.
    pub player: String,
    /// Files (or discs) the title is made of.
    pub parts: u32,
    /// How many of them the player was given. Fewer than `parts` only for the system default
    /// player, which opens one file at a time.
    pub opened: u32,
}

impl From<PlayOutcome> for PlayOutcomeDto {
    fn from(outcome: PlayOutcome) -> Self {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Self {
            player: outcome.player.as_str().to_owned(),
            parts: count(outcome.parts),
            opened: count(outcome.opened),
        }
    }
}

/// The player the settings name. Reads the keys one by one so the settings-load notice, which
/// only a snapshot hands out, stays for the settings page.
pub(crate) fn configured_player(settings: &SettingsService) -> PlayerConfig {
    let values: BTreeMap<String, serde_json::Value> = [KIND_KEY, PATH_KEY, CUSTOM_COMMAND_KEY]
        .into_iter()
        .filter_map(|key| settings.get(key).map(|value| (key.to_owned(), value)))
        .collect();
    player_config(&values)
}

fn failure(error: &PlayError) -> LibraryCommandError {
    LibraryCommandError {
        code: error.code().to_owned(),
        message: error.to_string(),
    }
}

/// Plays the stored item, with `open_system` as the hand-over to the system default player.
/// Blocking.
pub(crate) fn play_stored(
    db: &Database,
    id: ItemId,
    config: &PlayerConfig,
    open_system: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<PlayOutcomeDto, LibraryCommandError> {
    let item = db
        .read(|conn| get_item(conn, id))
        .map_err(|error| storage(error.to_string()))?
        .ok_or_else(|| LibraryCommandError {
            code: "not_found".to_owned(),
            message: "That title is not in the library any more.".to_owned(),
        })?;
    play(&item, config, &|path| path.exists(), open_system)
        .map(PlayOutcomeDto::from)
        .map_err(|error| {
            tracing::warn!(%error, "could not play a title");
            failure(&error)
        })
}

/// Plays a movie or an episode in the external player.
#[tauri::command]
#[specta::specta]
pub async fn play_item(
    app: tauri::AppHandle,
    library: State<'_, LibraryState>,
    settings: State<'_, SettingsState>,
    id: String,
) -> Result<PlayOutcomeDto, LibraryCommandError> {
    let id = parse_id(&id)?;
    let db = library.database().map_err(storage)?;
    let config = configured_player(settings.service());
    blocking(move || {
        play_stored(&db, id, &config, &|path| {
            app.opener()
                .open_path(path.to_string_lossy().into_owned(), None::<&str>)
                .map_err(|e| e.to_string())
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_app::playback::player::PlayerKind;
    use cv_domain::{Item, ItemDetail, MediaFile, MediaFileType};
    use serde_json::json;
    use std::cell::RefCell;

    fn stored_movie(db: &Database, dir: &Path, files: &[&str], stacked: bool) -> ItemId {
        let mut item = Item::movie("Heat");
        if let ItemDetail::Movie(m) = &mut item.detail {
            m.stacked = stacked;
        }
        for name in files {
            std::fs::write(dir.join(name), b"video").unwrap();
            item.core.media_files.push(MediaFile::new(
                MediaFileType::Video,
                dir.to_string_lossy(),
                *name,
            ));
        }
        db.write(move |conn| cv_db::repo::insert_item(conn, &item))
            .unwrap()
    }

    #[test]
    fn the_settings_choose_the_player() {
        let settings = SettingsService::in_memory("test");
        assert_eq!(configured_player(&settings), PlayerConfig::default());
        settings.set(KIND_KEY, json!("mpv")).unwrap();
        settings.set(PATH_KEY, json!("D:\\mpv\\mpv.exe")).unwrap();
        let config = configured_player(&settings);
        assert_eq!(config.kind, PlayerKind::Mpv);
        assert_eq!(config.program, Some("D:\\mpv\\mpv.exe".into()));
        // Reading the player leaves the load notice for the settings page.
        assert!(settings.snapshot().notice.is_some());
    }

    #[test]
    fn pressing_play_opens_the_stored_files_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        let id = stored_movie(&db, dir.path(), &["Heat cd2.mkv", "Heat cd1.mkv"], true);
        let opened = RefCell::new(Vec::new());
        let outcome = play_stored(&db, id, &PlayerConfig::default(), &|path| {
            opened.borrow_mut().push(path.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(outcome.player, "system");
        assert_eq!((outcome.parts, outcome.opened), (2, 1));
        assert_eq!(opened.borrow()[0].file_name().unwrap(), "Heat cd1.mkv");
    }

    #[test]
    fn play_says_why_it_cannot_play() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        let gone = play_stored(&db, ItemId(404), &PlayerConfig::default(), &|_| Ok(()));
        assert_eq!(gone.unwrap_err().code, "not_found");

        let id = stored_movie(&db, dir.path(), &["Heat.mkv"], false);
        let broken = PlayerConfig {
            kind: PlayerKind::Custom,
            program: None,
            custom_command: String::new(),
        };
        let error = play_stored(&db, id, &broken, &|_| Ok(())).unwrap_err();
        assert_eq!(error.code, "empty_custom_command");

        std::fs::remove_file(dir.path().join("Heat.mkv")).unwrap();
        let missing = play_stored(&db, id, &PlayerConfig::default(), &|_| Ok(())).unwrap_err();
        assert_eq!(missing.code, "file_missing");
    }
}

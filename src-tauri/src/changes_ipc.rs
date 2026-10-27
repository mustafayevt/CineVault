//! The typed `ItemsChanged` event: what changed for which titles, so open pages refresh in place.
//!
//! `cv_app::changes` collects the announcements of the services; [`relay`] turns each one into
//! this event. The UI maps the parts to its queries and invalidates only those.

use std::sync::Arc;

use cv_app::changes::{ChangeHub, ChangePart, ItemsChanged as Change};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_specta::Event;

/// Which part of a title changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ChangePartDto {
    /// The stored ratings.
    Ratings,
    /// Artwork files, or the artwork the title uses.
    Artwork,
    /// The fields of the title (an edit or a scrape).
    Details,
    /// The files of the title: paths, media info, extras.
    Files,
    /// The lifecycle: stage, flags and explanation (also "fetching...").
    Stage,
}

impl From<ChangePart> for ChangePartDto {
    fn from(part: ChangePart) -> Self {
        match part {
            ChangePart::Ratings => Self::Ratings,
            ChangePart::Artwork => Self::Artwork,
            ChangePart::Details => Self::Details,
            ChangePart::Files => Self::Files,
            ChangePart::Stage => Self::Stage,
        }
    }
}

/// Titles whose parts changed. Emitted after every write that a page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct ItemsChanged {
    /// Item ids as text.
    pub items: Vec<String>,
    pub parts: Vec<ChangePartDto>,
}

impl From<&Change> for ItemsChanged {
    fn from(change: &Change) -> Self {
        Self {
            items: change.items.iter().map(|id| id.0.to_string()).collect(),
            parts: change.parts.iter().map(|part| (*part).into()).collect(),
        }
    }
}

/// Emits an [`ItemsChanged`] event for every change the services announce.
pub fn relay(app: AppHandle, hub: &Arc<ChangeHub>) {
    hub.subscribe(Arc::new(move |change| {
        // A page that misses one event shows the old value until its next refresh: log, move on.
        if let Err(error) = ItemsChanged::from(change).emit(&app) {
            tracing::warn!(%error, "could not emit the ItemsChanged event");
        }
    }));
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::Duration;

    use cv_app::data_location::{DataLocation, LocationInputs};
    use cv_app::secrets::SecretStore;
    use cv_app::services::{AppServices, BootOptions};
    use cv_db::repo::insert_item;
    use cv_domain::{Item, ItemId};
    use cv_scan::sources::{PathStyle, SourceGuard, SystemFolders};
    use tauri::Manager;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    use super::*;
    use crate::boot::Boot;
    use crate::edit_ipc::{EditFieldDto, EditState, EditValueDto, save_item_edit};
    use crate::nfo_ipc::NfoState;
    use crate::settings_ipc::SettingsState;
    use crate::sources_ipc::LibraryState;

    #[test]
    fn the_wire_event_carries_item_ids_as_text_and_the_parts_by_name() {
        let change = Change {
            items: vec![ItemId(7), ItemId(12)],
            parts: vec![ChangePart::Ratings, ChangePart::Stage],
        };
        let dto = ItemsChanged::from(&change);
        assert_eq!(dto.items, ["7", "12"]);
        assert_eq!(dto.parts, [ChangePartDto::Ratings, ChangePartDto::Stage]);
        assert_eq!(
            serde_json::to_value(&dto).unwrap(),
            serde_json::json!({"items": ["7", "12"], "parts": ["ratings", "stage"]})
        );
    }

    fn app(root: &Path) -> (tauri::App<tauri::test::MockRuntime>, Arc<AppServices>) {
        let inputs = LocationInputs {
            exe_dir: root.join("app"),
            user_data_base: root.join("userdata"),
            user_config_base: root.join("userconfig"),
        };
        std::fs::create_dir_all(&inputs.exe_dir).unwrap();
        let mut options = BootOptions::new(
            DataLocation::resolve(&inputs),
            Arc::new(SecretStore::in_memory()),
        )
        .without_providers();
        options.change_window = Duration::ZERO;
        let services = AppServices::boot(options).expect("boot");
        let boot = Boot::for_services(Arc::clone(&services));
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let guard = SourceGuard::new(
            PathStyle::current(),
            SystemFolders::new(PathStyle::current()),
        );
        app.manage(LibraryState::with_guard(&boot, guard));
        app.manage(NfoState::new());
        app.manage(EditState::new());
        app.manage(SettingsState::from_boot(&boot));
        app.manage(boot);
        (app, services)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn saving_an_edit_announces_the_details_and_the_stage_of_the_title() {
        let temp = tempfile::tempdir().unwrap();
        let (app, services) = app(temp.path());
        let id = services
            .db()
            .write(|tx| insert_item(tx, &Item::movie("Heat")))
            .unwrap();
        let seen = Arc::new(Mutex::new(Vec::<Change>::new()));
        let sink = Arc::clone(&seen);
        services.changes().subscribe(Arc::new(move |change| {
            sink.lock().unwrap().push(change.clone())
        }));

        save_item_edit(
            app.state::<LibraryState>(),
            app.state::<NfoState>(),
            app.state::<EditState>(),
            app.state::<SettingsState>(),
            id.0.to_string(),
            vec![EditFieldDto {
                key: "title".to_owned(),
                value: EditValueDto::Text {
                    value: "Heat (Director's Cut)".to_owned(),
                },
                lock: None,
            }],
        )
        .await
        .expect("saved");
        assert_eq!(
            *seen.lock().unwrap(),
            vec![Change {
                items: vec![id],
                parts: vec![ChangePart::Details, ChangePart::Stage],
            }]
        );

        // Saving the same value again changes nothing, so nothing is announced.
        seen.lock().unwrap().clear();
        save_item_edit(
            app.state::<LibraryState>(),
            app.state::<NfoState>(),
            app.state::<EditState>(),
            app.state::<SettingsState>(),
            id.0.to_string(),
            vec![EditFieldDto {
                key: "title".to_owned(),
                value: EditValueDto::Text {
                    value: "Heat (Director's Cut)".to_owned(),
                },
                lock: None,
            }],
        )
        .await
        .expect("saved again");
        assert!(seen.lock().unwrap().is_empty());
        services.shutdown();
    }
}

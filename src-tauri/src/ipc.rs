//! Typed IPC surface: every Tauri command and event is registered here so `tauri-specta`
//! can generate the TypeScript bindings in `ui/src/bindings.ts`.

#[cfg(debug_assertions)]
use serde::{Deserialize, Serialize};
#[cfg(debug_assertions)]
use specta::Type;
#[cfg(any(debug_assertions, test))]
use specta_typescript::Typescript;
#[cfg(debug_assertions)]
use tauri::AppHandle;
#[cfg(debug_assertions)]
use tauri_specta::Event;
use tauri_specta::{Builder, collect_commands, collect_events};

// The sample `ping` command and its event, `start_sample_job` and `register_image_source` are
// development aids: release builds do not compile or register them, so the web view cannot reach
// them. `collect_commands!` takes no `#[cfg]` on single entries, hence the two macros below.
#[cfg(debug_assertions)]
macro_rules! commands_with_dev {
    ($($common:tt)*) => {
        collect_commands![
            ping,
            crate::jobs_ipc::start_sample_job,
            crate::img_ipc::register_image_source,
            $($common)*
        ]
    };
}

#[cfg(not(debug_assertions))]
macro_rules! commands_with_dev {
    ($($common:tt)*) => {
        collect_commands![$($common)*]
    };
}

#[cfg(debug_assertions)]
macro_rules! events_with_dev {
    () => {
        collect_events![
            Pinged,
            crate::changes_ipc::ItemsChanged,
            crate::drives_ipc::DrivesChanged,
            crate::history_ipc::HistoryChanged
        ]
    };
}

#[cfg(not(debug_assertions))]
macro_rules! events_with_dev {
    () => {
        collect_events![
            crate::changes_ipc::ItemsChanged,
            crate::drives_ipc::DrivesChanged,
            crate::history_ipc::HistoryChanged
        ]
    };
}

/// Reply to the sample `ping` command.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PingReply {
    /// The message that was sent, echoed back.
    pub echo: String,
    /// The application version reported by the backend.
    pub app_version: String,
    /// How many pings the backend has answered so far, starting at 1.
    pub sequence: u32,
}

/// Event emitted by the backend after every answered `ping`.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct Pinged {
    pub sequence: u32,
}

/// Counter of answered pings, held in Tauri state.
#[cfg(debug_assertions)]
#[derive(Default)]
pub struct PingState(std::sync::atomic::AtomicU32);

#[cfg(debug_assertions)]
impl PingState {
    fn next(&self) -> u32 {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
    }
}

/// Sample command used to prove the typed round trip UI -> Rust -> UI.
#[cfg(debug_assertions)]
#[tauri::command]
#[specta::specta]
pub fn ping(
    app: AppHandle,
    state: tauri::State<'_, PingState>,
    message: String,
) -> Result<PingReply, String> {
    let reply = answer_ping(&state, message);
    let sequence = reply.sequence;
    // A failed notification must not fail the command itself.
    if let Err(error) = (Pinged { sequence }).emit(&app) {
        tracing::warn!(%error, "could not emit the Pinged event");
    }
    Ok(reply)
}

/// The command's logic without the Tauri plumbing.
#[cfg(debug_assertions)]
fn answer_ping(state: &PingState, message: String) -> PingReply {
    PingReply {
        echo: message,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        sequence: state.next(),
    }
}

/// Adds the state that the development-only commands use; release builds add none.
#[cfg(debug_assertions)]
pub fn with_dev_state(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.manage(PingState::default())
}

/// Adds the state that the development-only commands use; release builds add none.
#[cfg(not(debug_assertions))]
pub fn with_dev_state(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
}

/// Builds the command and event registry shared by the app and the bindings exporter.
pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(commands_with_dev![
            crate::jobs_ipc::subscribe_job_progress,
            crate::jobs_ipc::cancel_job,
            crate::notices_ipc::subscribe_notices,
            crate::settings_ipc::get_settings,
            crate::settings_ipc::set_setting,
            crate::settings_ipc::reset_settings,
            crate::sources_ipc::list_data_sources,
            crate::sources_ipc::add_data_source,
            crate::sources_ipc::remove_data_source,
            crate::sources_ipc::reorder_data_sources,
            crate::sources_ipc::remap_data_source,
            crate::sources_ipc::change_data_source_kind,
            crate::sources_ipc::rename_data_source_label,
            crate::onboarding_ipc::analyze_library_folder,
            crate::onboarding_ipc::pick_library_folders,
            crate::picker_ipc::pick_folder,
            crate::picker_ipc::open_folder,
            crate::picker_ipc::pick_files,
            crate::scan_ipc::rescan_library,
            crate::scan_ipc::find_missing_files,
            crate::scan_ipc::list_source_health,
            crate::drives_ipc::drives_overview,
            crate::drives_ipc::drives_trash_contents,
            crate::drives_ipc::empty_drives_trash,
            crate::scan_ipc::read_pending_media_info,
            crate::actions_ipc::run_item_action,
            crate::match_ipc::start_match_batch,
            crate::match_ipc::match_library_items,
            crate::match_ipc::list_review_items,
            crate::match_ipc::count_review_items,
            crate::match_ipc::accept_review_candidate,
            crate::match_ipc::accept_top_review_candidates,
            crate::match_ipc::ignore_review_item,
            crate::match_ipc::reopen_review_items,
            crate::match_ipc::search_review_candidates,
            crate::match_ipc::list_metadata_providers,
            crate::scrape_stream_ipc::stream_scrape_candidates,
            crate::scrape_preview_ipc::stream_scrape_preview,
            crate::scrape_preview_ipc::scrape_field_selection,
            crate::match_ipc::apply_scrape,
            crate::match_ipc::undo_scrape,
            crate::match_ipc::search_new_title,
            crate::match_ipc::current_match_candidate,
            crate::match_ipc::accept_confident_titles,
            crate::tools_ipc::list_tools,
            crate::tools_ipc::set_tool_path,
            crate::tools_ipc::install_tool,
            crate::tools_ipc::cancel_tool_install,
            crate::tools_ipc::expect_tools,
            crate::updates_ipc::get_update_status,
            crate::updates_ipc::check_for_update,
            crate::updates_ipc::install_update,
            crate::updates_ipc::cancel_update_download,
            crate::updates_ipc::update_blocking_jobs,
            crate::library_ipc::list_library_window,
            crate::library_ipc::list_library_rows,
            crate::library_ipc::library_counts,
            crate::library_ipc::library_source_groups,
            crate::library_ipc::library_size_total,
            crate::library_ipc::library_filter_catalogue,
            crate::library_ipc::library_facet_values,
            crate::library_ipc::search_library_people,
            crate::providers_ipc::list_provider_credentials,
            crate::providers_ipc::set_provider_credential,
            crate::providers_ipc::remove_provider_credentials,
            crate::providers_ipc::restore_provider_credentials,
            crate::providers_ipc::get_key_attention,
            crate::providers_ipc::test_provider_credentials,
            crate::providers_ipc::list_provider_attributions,
            crate::ratings_ipc::fetch_imdb_top250,
            crate::ratings_ipc::open_rating_page,
            crate::ratings_ipc::count_rating_sweep,
            crate::ratings_ipc::start_rating_sweep,
            crate::nfo_ipc::read_nfo,
            crate::nfo_ipc::rewrite_nfo,
            crate::nfo_ipc::undo_nfo_action,
            crate::nfo_ipc::preview_movie_nfo,
            crate::nfo_ipc::preview_show_nfo,
            crate::nfo_ipc::preview_episode_nfo,
            crate::organize_ipc::organize_profiles,
            crate::organize_ipc::organize_select_profile,
            crate::organize_ipc::organize_save_profile,
            crate::organize_ipc::organize_update_profile,
            crate::organize_ipc::organize_delete_profile,
            crate::organize_ipc::organize_check_template,
            crate::organize_ipc::organize_example_for_file,
            crate::organize_ipc::organize_tokens,
            crate::organize_ipc::organize_preview,
            crate::organize_ipc::organize_apply,
            crate::organize_ipc::organize_journal,
            crate::organize_ipc::organize_undo,
            crate::library_ops_ipc::plan_change_source,
            crate::library_ops_ipc::preview_change_source_title,
            crate::library_ops_ipc::apply_change_source,
            crate::library_ops_ipc::control_move,
            crate::library_ops_ipc::list_unfinished_batches,
            crate::library_ops_ipc::resume_unfinished_batch,
            crate::library_ops_ipc::roll_back_unfinished_batch,
            crate::library_ops_ipc::plan_removal,
            crate::library_ops_ipc::remove_from_library,
            crate::library_ops_ipc::delete_with_files,
            crate::library_ops_ipc::list_library_review,
            crate::library_ops_ipc::remove_review_rows,
            crate::library_ops_ipc::undo_review_removal,
            crate::library_ops_ipc::keep_review_rows,
            crate::library_ops_ipc::unkeep_review_rows,
            crate::library_ops_ipc::relocate_review_row,
            crate::history_ipc::history_list,
            crate::history_ipc::history_titles,
            crate::history_ipc::history_for_title,
            crate::history_ipc::history_entry_for_batch,
            crate::history_ipc::history_bin_total,
            crate::history_ipc::history_undo,
            crate::history_ipc::history_undo_title,
            crate::history_ipc::delete_titles,
            crate::history_ipc::erase_titles,
            crate::detail_ipc::get_movie_detail,
            crate::people_ipc::get_cast_portraits,
            crate::people_ipc::get_person_imdb_id,
            crate::detail_ipc::set_movie_watched,
            crate::curation_ipc::set_watched,
            crate::curation_ipc::toggle_watched,
            crate::curation_ipc::restore_watched,
            crate::curation_ipc::clear_new_flags,
            crate::curation_ipc::clear_new_on_open,
            crate::curation_ipc::clear_all_new_flags,
            crate::curation_ipc::restore_new_flags,
            crate::collections_ipc::get_collection,
            crate::artwork_stream_ipc::stream_artwork_candidates,
            crate::artwork_stream_ipc::artwork_candidate_counts,
            crate::artwork_ipc::place_artwork,
            crate::artwork_ipc::place_extra_artwork,
            crate::artwork_ipc::artwork_full_image,
            crate::artwork_ipc::download_missing_artwork,
            crate::artwork_ipc::fill_season_artwork,
            crate::play_ipc::play_item,
            crate::activity_ipc::set_job_paused,
            crate::torrent_stream_ipc::torrent_title_search,
            crate::torrent_stream_ipc::torrent_title_details,
            crate::torrent_stream_ipc::torrent_title_seasons,
            crate::torrent_stream_ipc::torrent_title_episodes,
            crate::torrent_stream_ipc::torrent_landing,
            crate::torrent_stream_ipc::torrent_release_search,
            crate::torrent_stream_ipc::torrent_latest,
            crate::torrent_stream_ipc::torrent_feed_lookup,
            crate::torrent_ipc::torrent_open_magnet,
            crate::torrent_ipc::torrent_open_page,
            crate::torrent_ipc::torrent_open_torrent_file,
            crate::torrent_ipc::torrent_sources_status,
            crate::torrent_ipc::torrent_set_source,
            crate::torrent_ipc::torrent_test_torznab,
            crate::torrent_ipc::torrent_set_torznab,
            crate::torrent_ipc::torrent_detect_jackett,
            crate::torrent_indexer_ipc::torrent_indexer_service_status,
            crate::torrent_indexer_ipc::torrent_install_indexer_service,
            crate::torrent_indexer_ipc::torrent_update_indexer_service,
            crate::torrent_indexer_ipc::torrent_check_sources,
            crate::torrent_indexer_ipc::torrent_open_indexer_manager,
            crate::torrent_indexer_ipc::torrent_indexers,
            crate::torrent_indexer_ipc::torrent_set_indexer,
            crate::provider_stream::cancel_search,
            crate::provider_stream::retry_provider,
            crate::lifecycle_ipc::lifecycle_counts,
            crate::lifecycle_ipc::get_lifecycles,
            crate::tv_detail_ipc::get_show_detail,
            crate::tv_detail_ipc::get_episode_detail,
            crate::tv_detail_ipc::set_episodes_watched,
            crate::edit_ipc::get_item_edit,
            crate::edit_ipc::save_item_edit,
            crate::edit_ipc::set_field_lock,
            crate::edit_ipc::set_item_lock,
            crate::episode_order_ipc::get_show_orderings,
            crate::episode_order_ipc::set_show_ordering,
            crate::episode_scrape_ipc::search_show_episodes,
            crate::episode_scrape_ipc::assign_episode,
            crate::episode_scrape_ipc::scrape_show_episodes,
            crate::episode_scrape_ipc::scrape_episode,
            crate::browser_access_ipc::get_browser_access,
            crate::browser_access_ipc::set_browser_access_enabled,
            crate::browser_access_ipc::set_browser_access_port,
            crate::browser_access_ipc::set_browser_access_lan_only,
            crate::browser_access_ipc::set_browser_access_tray,
            crate::browser_access_ipc::set_browser_access_password,
            crate::browser_access_ipc::sign_out_browsers,
            crate::browser_access_ipc::set_start_at_login
        ])
        .events(events_with_dev!())
        // Every streaming search sends this over its Channel; the item type is the flow's own.
        .typ::<crate::provider_stream::ProviderEvent<()>>()
}

/// Path of the generated bindings, relative to this crate's manifest directory.
#[cfg(any(debug_assertions, test))]
pub const BINDINGS_PATH: &str = "../ui/src/bindings.ts";

/// The exporter configuration, including the generated-file header.
#[cfg(any(debug_assertions, test))]
pub fn typescript() -> Typescript {
    Typescript::default().header("// Generated by tauri-specta. Do not edit; run `cargo test -p cinevault-app` with CINEVAULT_UPDATE_BINDINGS=1 to regenerate.\n/* eslint-disable */")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn committed_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(BINDINGS_PATH)
    }

    /// Fails when `ui/src/bindings.ts` is stale. Set `CINEVAULT_UPDATE_BINDINGS=1` to rewrite it.
    #[test]
    fn generated_bindings_are_up_to_date() {
        let path = committed_path();
        if std::env::var_os("CINEVAULT_UPDATE_BINDINGS").is_some() {
            builder()
                .export(typescript(), &path)
                .expect("export bindings");
        }
        let scratch = std::env::temp_dir().join("cinevault-bindings-check.ts");
        builder()
            .export(typescript(), &scratch)
            .expect("export bindings to scratch");
        let fresh = std::fs::read_to_string(&scratch).expect("read fresh bindings");
        let _ = std::fs::remove_file(&scratch);
        let committed = std::fs::read_to_string(&path).expect("ui/src/bindings.ts must exist");
        assert_eq!(
            fresh.replace("\r\n", "\n"),
            committed.replace("\r\n", "\n"),
            "ui/src/bindings.ts is stale"
        );
    }

    /// External links (`target="_blank"` anchors) open through the opener plugin's `open_url`.
    /// The web view may open web and mail addresses only, never paths or other schemes, and the
    /// plugin's broader default set (reveal in folder, telephone) stays off.
    #[test]
    fn web_view_may_open_only_web_and_mail_links() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).expect("valid json");
        let permissions = capability["permissions"].as_array().expect("permissions");
        assert!(
            permissions
                .iter()
                .all(|p| p.as_str() != Some("opener:default")),
            "opener:default is broader than the app needs"
        );
        let grant = permissions
            .iter()
            .find(|p| p["identifier"] == "opener:allow-open-url")
            .expect("opener:allow-open-url is granted");
        let mut urls: Vec<&str> = grant["allow"]
            .as_array()
            .expect("a scope")
            .iter()
            .map(|entry| entry["url"].as_str().expect("url"))
            .collect();
        urls.sort_unstable();
        assert_eq!(urls, ["http://*", "https://*", "mailto:*"]);
        assert!(
            permissions
                .iter()
                .all(|p| !p.to_string().contains("reveal-item-in-dir")
                    && !p.to_string().contains("open-path")),
        );
    }

    /// The development aids are in the registry of debug builds and absent from release builds,
    /// so the web view of a shipped app cannot call them.
    #[test]
    fn dev_commands_are_registered_only_in_debug_builds() {
        let scratch = std::env::temp_dir().join("cinevault-bindings-dev.ts");
        builder().export(typescript(), &scratch).expect("export");
        let text = std::fs::read_to_string(&scratch).expect("read");
        let _ = std::fs::remove_file(&scratch);
        for name in ["registerImageSource", "startSampleJob", "	ping: ("] {
            assert_eq!(text.contains(name), cfg!(debug_assertions), "{name}");
        }
        // Everything else stays: the image protocol's own commands are not dev aids.
        assert!(text.contains("subscribeJobProgress"));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn bindings_expose_ping_and_its_event() {
        let scratch = std::env::temp_dir().join("cinevault-bindings-names.ts");
        builder().export(typescript(), &scratch).expect("export");
        let text = std::fs::read_to_string(&scratch).expect("read");
        let _ = std::fs::remove_file(&scratch);
        assert!(text.contains("ping"));
        assert!(text.contains("PingReply"));
        assert!(text.contains("pinged"));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn ping_echoes_the_message_and_counts_from_one() {
        let state = PingState::default();
        let first = answer_ping(&state, "one".to_owned());
        assert_eq!(first.echo, "one");
        assert_eq!(first.sequence, 1);
        assert_eq!(first.app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(answer_ping(&state, "two".to_owned()).sequence, 2);
    }
}

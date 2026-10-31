//! Every command the browser server dispatches.
//!
//! The desktop registry is built by `tauri-specta`; the server needs its own list. Each entry
//! names the command, the `cv-ipc` module that implements it and the argument names the UI sends
//! (camelCase, as `ui/src/bindings.ts` writes them). Desktop-only commands and the development
//! aids are not here: a browser has no equivalent for them.

/// One command the server must dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandInfo {
    /// The IPC name, as `ui/src/bindings.ts` invokes it.
    pub name: &'static str,
    /// The `cv-ipc` module that implements it.
    pub module: &'static str,
    /// The argument names, camelCase as the UI sends them; empty for a command without arguments.
    pub args: &'static [&'static str],
}

/// Every non-desktop command, sorted by name.
pub const COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        name: "accept_confident_titles",
        module: "matching",
        args: &["itemIds"],
    },
    CommandInfo {
        name: "accept_review_candidate",
        module: "matching",
        args: &["reviewId", "provider", "providerId"],
    },
    CommandInfo {
        name: "accept_top_review_candidates",
        module: "matching",
        args: &["reviewIds"],
    },
    CommandInfo {
        name: "add_data_source",
        module: "sources",
        args: &["kind", "path"],
    },
    CommandInfo {
        name: "analyze_library_folder",
        module: "onboarding",
        args: &["path"],
    },
    CommandInfo {
        name: "apply_change_source",
        module: "library_ops",
        args: &["ids", "target", "onProgress"],
    },
    CommandInfo {
        name: "apply_scrape",
        module: "matching",
        args: &["itemId", "request"],
    },
    CommandInfo {
        name: "artwork_candidate_counts",
        module: "artwork_stream",
        args: &["itemId"],
    },
    CommandInfo {
        name: "artwork_full_image",
        module: "artwork",
        args: &["candidateId"],
    },
    CommandInfo {
        name: "assign_episode",
        module: "episode_scrape",
        args: &["episodeId", "pick", "fields", "doNotOverwrite"],
    },
    CommandInfo {
        name: "cancel_job",
        module: "jobs",
        args: &["id"],
    },
    CommandInfo {
        name: "cancel_search",
        module: "provider_stream",
        args: &["searchId"],
    },
    CommandInfo {
        name: "cancel_tool_install",
        module: "tools",
        args: &[],
    },
    CommandInfo {
        name: "cancel_update_download",
        module: "updates",
        args: &[],
    },
    CommandInfo {
        name: "change_data_source_kind",
        module: "sources",
        args: &["id", "kind"],
    },
    CommandInfo {
        name: "check_for_update",
        module: "updates",
        args: &["manual"],
    },
    CommandInfo {
        name: "clear_all_new_flags",
        module: "curation",
        args: &["kind"],
    },
    CommandInfo {
        name: "clear_new_flags",
        module: "curation",
        args: &["ids"],
    },
    CommandInfo {
        name: "clear_new_on_open",
        module: "curation",
        args: &["id"],
    },
    CommandInfo {
        name: "control_move",
        module: "library_ops",
        args: &["action"],
    },
    CommandInfo {
        name: "count_rating_sweep",
        module: "ratings",
        args: &["scope", "provider"],
    },
    CommandInfo {
        name: "count_review_items",
        module: "matching",
        args: &[],
    },
    CommandInfo {
        name: "current_match_candidate",
        module: "matching",
        args: &["itemId", "language"],
    },
    CommandInfo {
        name: "delete_titles",
        module: "history",
        args: &["ids"],
    },
    CommandInfo {
        name: "delete_with_files",
        module: "library_ops",
        args: &["ids"],
    },
    CommandInfo {
        name: "download_missing_artwork",
        module: "artwork",
        args: &["itemIds"],
    },
    CommandInfo {
        name: "drives_overview",
        module: "drives",
        args: &[],
    },
    CommandInfo {
        name: "drives_trash_contents",
        module: "drives",
        args: &["driveId"],
    },
    CommandInfo {
        name: "empty_drives_trash",
        module: "drives",
        args: &["driveId", "confirm"],
    },
    CommandInfo {
        name: "erase_titles",
        module: "history",
        args: &["ids", "confirm"],
    },
    CommandInfo {
        name: "expect_tools",
        module: "tools",
        args: &["tools"],
    },
    CommandInfo {
        name: "fetch_imdb_top250",
        module: "ratings",
        args: &["list"],
    },
    CommandInfo {
        name: "fill_season_artwork",
        module: "artwork",
        args: &["itemId", "season"],
    },
    CommandInfo {
        name: "find_missing_files",
        module: "scan",
        args: &[],
    },
    CommandInfo {
        name: "get_cast_portraits",
        module: "people",
        args: &["id"],
    },
    CommandInfo {
        name: "get_collection",
        module: "collections",
        args: &["id"],
    },
    CommandInfo {
        name: "get_episode_detail",
        module: "tv",
        args: &["id"],
    },
    CommandInfo {
        name: "get_item_edit",
        module: "edit",
        args: &["id"],
    },
    CommandInfo {
        name: "get_key_attention",
        module: "providers",
        args: &[],
    },
    CommandInfo {
        name: "get_lifecycles",
        module: "lifecycle",
        args: &["ids"],
    },
    CommandInfo {
        name: "get_movie_detail",
        module: "detail",
        args: &["id"],
    },
    CommandInfo {
        name: "get_person_imdb_id",
        module: "people",
        args: &["tmdbId"],
    },
    CommandInfo {
        name: "get_settings",
        module: "settings",
        args: &[],
    },
    CommandInfo {
        name: "get_show_detail",
        module: "tv",
        args: &["id"],
    },
    CommandInfo {
        name: "get_show_orderings",
        module: "episode_order",
        args: &["showId"],
    },
    CommandInfo {
        name: "get_update_status",
        module: "updates",
        args: &[],
    },
    CommandInfo {
        name: "history_bin_total",
        module: "history",
        args: &[],
    },
    CommandInfo {
        name: "history_entry_for_batch",
        module: "history",
        args: &["batchId"],
    },
    CommandInfo {
        name: "history_for_title",
        module: "history",
        args: &["id"],
    },
    CommandInfo {
        name: "history_list",
        module: "history",
        args: &["filter", "offset", "limit"],
    },
    CommandInfo {
        name: "history_titles",
        module: "history",
        args: &["entryId"],
    },
    CommandInfo {
        name: "history_undo",
        module: "history",
        args: &["entryId"],
    },
    CommandInfo {
        name: "history_undo_title",
        module: "history",
        args: &["entryId", "id"],
    },
    CommandInfo {
        name: "ignore_review_item",
        module: "matching",
        args: &["reviewId"],
    },
    CommandInfo {
        name: "install_tool",
        module: "tools",
        args: &["tool", "onProgress"],
    },
    CommandInfo {
        name: "keep_review_rows",
        module: "library_ops",
        args: &["ids"],
    },
    CommandInfo {
        name: "library_counts",
        module: "library",
        args: &[],
    },
    CommandInfo {
        name: "library_facet_values",
        module: "library",
        args: &["query", "facet"],
    },
    CommandInfo {
        name: "library_filter_catalogue",
        module: "library",
        args: &["kind"],
    },
    CommandInfo {
        name: "library_size_total",
        module: "library",
        args: &["kind"],
    },
    CommandInfo {
        name: "library_source_groups",
        module: "library",
        args: &["query"],
    },
    CommandInfo {
        name: "lifecycle_counts",
        module: "lifecycle",
        args: &["kind"],
    },
    CommandInfo {
        name: "list_data_sources",
        module: "sources",
        args: &[],
    },
    CommandInfo {
        name: "list_library_review",
        module: "library_ops",
        args: &[],
    },
    CommandInfo {
        name: "list_library_rows",
        module: "library",
        args: &["query", "offset", "limit"],
    },
    CommandInfo {
        name: "list_library_window",
        module: "library",
        args: &["query", "offset", "limit"],
    },
    CommandInfo {
        name: "list_metadata_providers",
        module: "matching",
        args: &[],
    },
    CommandInfo {
        name: "list_provider_attributions",
        module: "providers",
        args: &[],
    },
    CommandInfo {
        name: "list_provider_credentials",
        module: "providers",
        args: &[],
    },
    CommandInfo {
        name: "list_review_items",
        module: "matching",
        args: &[],
    },
    CommandInfo {
        name: "list_source_health",
        module: "scan",
        args: &[],
    },
    CommandInfo {
        name: "list_tools",
        module: "tools",
        args: &[],
    },
    CommandInfo {
        name: "list_unfinished_batches",
        module: "library_ops",
        args: &[],
    },
    CommandInfo {
        name: "match_library_items",
        module: "matching",
        args: &["itemIds"],
    },
    CommandInfo {
        name: "organize_apply",
        module: "organize",
        args: &["request", "include"],
    },
    CommandInfo {
        name: "organize_check_template",
        module: "organize",
        args: &["field", "template"],
    },
    CommandInfo {
        name: "organize_delete_profile",
        module: "organize",
        args: &["id"],
    },
    CommandInfo {
        name: "organize_example_for_file",
        module: "organize",
        args: &["profileId", "fileName"],
    },
    CommandInfo {
        name: "organize_journal",
        module: "organize",
        args: &[],
    },
    CommandInfo {
        name: "organize_preview",
        module: "organize",
        args: &["request"],
    },
    CommandInfo {
        name: "organize_profiles",
        module: "organize",
        args: &[],
    },
    CommandInfo {
        name: "organize_save_profile",
        module: "organize",
        args: &["name", "from", "patterns"],
    },
    CommandInfo {
        name: "organize_select_profile",
        module: "organize",
        args: &["id"],
    },
    CommandInfo {
        name: "organize_tokens",
        module: "organize",
        args: &["tv"],
    },
    CommandInfo {
        name: "organize_undo",
        module: "organize",
        args: &["batchId"],
    },
    CommandInfo {
        name: "organize_update_profile",
        module: "organize",
        args: &["id", "patterns"],
    },
    CommandInfo {
        name: "place_artwork",
        module: "artwork",
        args: &["itemId", "slot", "source"],
    },
    CommandInfo {
        name: "place_extra_artwork",
        module: "artwork",
        args: &["itemId", "slot", "sources"],
    },
    CommandInfo {
        name: "plan_change_source",
        module: "library_ops",
        args: &["ids", "target"],
    },
    CommandInfo {
        name: "plan_removal",
        module: "library_ops",
        args: &["ids"],
    },
    CommandInfo {
        name: "preview_change_source_title",
        module: "library_ops",
        args: &["id", "target"],
    },
    CommandInfo {
        name: "preview_episode_nfo",
        module: "nfo",
        args: &["id"],
    },
    CommandInfo {
        name: "preview_movie_nfo",
        module: "nfo",
        args: &["id"],
    },
    CommandInfo {
        name: "preview_show_nfo",
        module: "nfo",
        args: &["id"],
    },
    CommandInfo {
        name: "read_nfo",
        module: "nfo",
        args: &["scope"],
    },
    CommandInfo {
        name: "read_pending_media_info",
        module: "scan",
        args: &[],
    },
    CommandInfo {
        name: "relocate_review_row",
        module: "library_ops",
        args: &["id", "folder"],
    },
    CommandInfo {
        name: "remap_data_source",
        module: "sources",
        args: &["id", "path"],
    },
    CommandInfo {
        name: "remove_data_source",
        module: "sources",
        args: &["id"],
    },
    CommandInfo {
        name: "remove_from_library",
        module: "library_ops",
        args: &["ids", "ignoreFolders"],
    },
    CommandInfo {
        name: "remove_provider_credentials",
        module: "providers",
        args: &["provider"],
    },
    CommandInfo {
        name: "remove_review_rows",
        module: "library_ops",
        args: &["ids"],
    },
    CommandInfo {
        name: "rename_data_source_label",
        module: "sources",
        args: &["id", "label"],
    },
    CommandInfo {
        name: "reopen_review_items",
        module: "matching",
        args: &["reviewIds"],
    },
    CommandInfo {
        name: "reorder_data_sources",
        module: "sources",
        args: &["kind", "ids"],
    },
    CommandInfo {
        name: "rescan_library",
        module: "scan",
        args: &[],
    },
    CommandInfo {
        name: "reset_settings",
        module: "settings",
        args: &["keys"],
    },
    CommandInfo {
        name: "restore_new_flags",
        module: "curation",
        args: &["ids"],
    },
    CommandInfo {
        name: "restore_provider_credentials",
        module: "providers",
        args: &["provider"],
    },
    CommandInfo {
        name: "restore_watched",
        module: "curation",
        args: &["previous"],
    },
    CommandInfo {
        name: "resume_unfinished_batch",
        module: "library_ops",
        args: &["batchId", "onProgress"],
    },
    CommandInfo {
        name: "retry_provider",
        module: "provider_stream",
        args: &["searchId", "provider"],
    },
    CommandInfo {
        name: "rewrite_nfo",
        module: "nfo",
        args: &["scope"],
    },
    CommandInfo {
        name: "roll_back_unfinished_batch",
        module: "library_ops",
        args: &["batchId"],
    },
    CommandInfo {
        name: "run_item_action",
        module: "actions",
        args: &["ids", "action"],
    },
    CommandInfo {
        name: "save_item_edit",
        module: "edit",
        args: &["id", "changes"],
    },
    CommandInfo {
        name: "scrape_episode",
        module: "episode_scrape",
        args: &["episodeId", "fields", "doNotOverwrite"],
    },
    CommandInfo {
        name: "scrape_field_selection",
        module: "scrape_preview",
        args: &["kind", "preset", "custom"],
    },
    CommandInfo {
        name: "scrape_show_episodes",
        module: "episode_scrape",
        args: &["showId", "fields", "doNotOverwrite"],
    },
    CommandInfo {
        name: "search_library_people",
        module: "library",
        args: &["term", "limit"],
    },
    CommandInfo {
        name: "search_new_title",
        module: "matching",
        args: &["itemId", "query", "language"],
    },
    CommandInfo {
        name: "search_review_candidates",
        module: "matching",
        args: &["reviewId", "query"],
    },
    CommandInfo {
        name: "search_show_episodes",
        module: "episode_scrape",
        args: &["showId", "query"],
    },
    CommandInfo {
        name: "set_episodes_watched",
        module: "curation",
        args: &["ids", "watched"],
    },
    CommandInfo {
        name: "set_field_lock",
        module: "edit",
        args: &["id", "field", "locked"],
    },
    CommandInfo {
        name: "set_item_lock",
        module: "edit",
        args: &["id", "locked"],
    },
    CommandInfo {
        name: "set_job_paused",
        module: "jobs",
        args: &["id", "paused"],
    },
    CommandInfo {
        name: "set_movie_watched",
        module: "curation",
        args: &["id", "watched"],
    },
    CommandInfo {
        name: "set_provider_credential",
        module: "providers",
        args: &["provider", "field", "value"],
    },
    CommandInfo {
        name: "set_setting",
        module: "settings",
        args: &["key", "valueJson"],
    },
    CommandInfo {
        name: "set_show_ordering",
        module: "episode_order",
        args: &["showId", "ordering"],
    },
    CommandInfo {
        name: "set_tool_path",
        module: "tools",
        args: &["tool", "path"],
    },
    CommandInfo {
        name: "set_watched",
        module: "curation",
        args: &["ids", "watched"],
    },
    CommandInfo {
        name: "start_match_batch",
        module: "matching",
        args: &["mode", "itemIds"],
    },
    CommandInfo {
        name: "start_rating_sweep",
        module: "ratings",
        args: &["scope", "provider", "onDone"],
    },
    CommandInfo {
        name: "stream_artwork_candidates",
        module: "artwork_stream",
        args: &["itemId", "slot", "searchId", "onEvent"],
    },
    CommandInfo {
        name: "stream_scrape_candidates",
        module: "scrape_stream",
        args: &["itemId", "query", "kind", "language", "searchId", "onEvent"],
    },
    CommandInfo {
        name: "stream_scrape_preview",
        module: "scrape_preview",
        args: &["itemId", "candidate", "language", "searchId", "onEvent"],
    },
    CommandInfo {
        name: "subscribe_job_progress",
        module: "jobs",
        args: &["onProgress"],
    },
    CommandInfo {
        name: "subscribe_notices",
        module: "notices",
        args: &["onNotice"],
    },
    CommandInfo {
        name: "test_provider_credentials",
        module: "providers",
        args: &["provider", "drafts"],
    },
    CommandInfo {
        name: "toggle_watched",
        module: "curation",
        args: &["ids"],
    },
    CommandInfo {
        name: "torrent_check_sources",
        module: "torrent_indexer",
        args: &[],
    },
    CommandInfo {
        name: "torrent_detect_jackett",
        module: "torrent",
        args: &[],
    },
    CommandInfo {
        name: "torrent_feed_lookup",
        module: "torrent_feed",
        args: &["request"],
    },
    CommandInfo {
        name: "torrent_indexer_service_status",
        module: "torrent_indexer",
        args: &[],
    },
    CommandInfo {
        name: "torrent_indexers",
        module: "torrent_indexer",
        args: &[],
    },
    CommandInfo {
        name: "torrent_install_indexer_service",
        module: "torrent_indexer",
        args: &[],
    },
    CommandInfo {
        name: "torrent_landing",
        module: "torrent_stream",
        args: &[],
    },
    CommandInfo {
        name: "torrent_latest",
        module: "torrent_feed",
        args: &["request", "onEvent"],
    },
    CommandInfo {
        name: "torrent_release_search",
        module: "torrent_stream",
        args: &["request", "onEvent"],
    },
    CommandInfo {
        name: "torrent_set_indexer",
        module: "torrent_indexer",
        args: &["id", "on"],
    },
    CommandInfo {
        name: "torrent_set_source",
        module: "torrent",
        args: &["id", "on"],
    },
    CommandInfo {
        name: "torrent_set_torznab",
        module: "torrent",
        args: &["url", "apiKey"],
    },
    CommandInfo {
        name: "torrent_sources_status",
        module: "torrent",
        args: &[],
    },
    CommandInfo {
        name: "torrent_test_torznab",
        module: "torrent",
        args: &["url", "apiKey"],
    },
    CommandInfo {
        name: "torrent_title_details",
        module: "torrent_stream",
        args: &["kind", "tmdbId"],
    },
    CommandInfo {
        name: "torrent_title_episodes",
        module: "torrent_stream",
        args: &["tmdbId", "season"],
    },
    CommandInfo {
        name: "torrent_title_search",
        module: "torrent_stream",
        args: &["request", "onEvent"],
    },
    CommandInfo {
        name: "torrent_title_seasons",
        module: "torrent_stream",
        args: &["tmdbId"],
    },
    CommandInfo {
        name: "torrent_update_indexer_service",
        module: "torrent_indexer",
        args: &[],
    },
    CommandInfo {
        name: "undo_nfo_action",
        module: "nfo",
        args: &["token"],
    },
    CommandInfo {
        name: "undo_review_removal",
        module: "library_ops",
        args: &["token"],
    },
    CommandInfo {
        name: "undo_scrape",
        module: "matching",
        args: &["token"],
    },
    CommandInfo {
        name: "unkeep_review_rows",
        module: "library_ops",
        args: &["ids"],
    },
    CommandInfo {
        name: "update_blocking_jobs",
        module: "updates",
        args: &[],
    },
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_command_name_is_unique() {
        let mut seen = HashSet::new();
        for command in COMMANDS {
            assert!(
                seen.insert(command.name),
                "the command {} is listed twice",
                command.name
            );
        }
    }

    #[test]
    fn every_command_names_a_module_and_camel_case_arguments() {
        for command in COMMANDS {
            assert!(!command.module.is_empty(), "{} has no module", command.name);
            for arg in command.args {
                assert!(
                    arg.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                        && arg.chars().all(|c| c.is_ascii_alphanumeric()),
                    "{} has an argument that is not camelCase: {arg}",
                    command.name
                );
            }
        }
    }
}

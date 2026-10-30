//! CineVault desktop application entry point.
//!
//! Commands and events are registered in [`ipc`]; job progress channels and the taskbar/dock
//! mirror live in [`jobs_ipc`]; provider credential commands live in [`providers_ipc`]; settings commands live in [`settings_ipc`]; NFO read/rewrite/undo commands live in [`nfo_ipc`]; organize (preview, apply, journal, undo, crash recovery) commands live in [`organize_ipc`]; movie detail commands live in [`detail_ipc`]; the Play action (external player) lives in [`play_ipc`]; show, season and episode page commands live in [`tv_detail_ipc`]; episode ordering commands live in [`episode_order_ipc`]; episode scraping and title search commands live in [`episode_scrape_ipc`]; library folder commands live in [`sources_ipc`]; the first-run folder analysis, player detection and folder picker live in [`onboarding_ipc`]; the single-folder and file pickers live in [`picker_ipc`]; Rescan, Find missing and source health commands live in [`scan_ipc`]; match batches, the Needs review inbox and the search-and-scrape panel live in [`match_ipc`]; the artwork picker and artwork downloads live in [`artwork_ipc`]; the Activity drawer's lanes and failures live in [`activity_ipc`]; external tool commands live in [`tools_ipc`]; update check and install commands live in [`updates_ipc`]; the custom window chrome wiring lives in [`chrome`]; single-instance and window-state
//! wiring lives in [`instance`]; the channel by which the `cinevault` command line runs batches in the open app lives in [`automation_ipc`]. The
//! `cv://img` image protocol and its key command live in [`img_ipc`]; the library listing and
//! count commands live in [`library_ipc`].

// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use tauri::Manager;

mod actions_ipc;
mod activity_ipc;
mod artwork_ipc;
mod artwork_stream_ipc;
mod automation_ipc;
mod boot;
mod changes_ipc;
mod chrome;
mod collections_ipc;
mod curation_ipc;
mod detail_ipc;
mod drives_ipc;
mod edit_ipc;
mod episode_order_ipc;
mod episode_scrape_ipc;
mod history_ipc;
mod img_ipc;
mod instance;
mod instance_lock;
mod ipc;
mod ipc_sink;
mod jobs_ipc;
mod library_facets;
mod library_ipc;
mod library_ops_ipc;
mod lifecycle_ipc;
mod match_ipc;
mod nfo_ipc;
mod notices_ipc;
mod onboarding_ipc;
mod organize_ipc;
mod people_ipc;
mod picker_ipc;
mod play_ipc;
mod portable;
// The streaming commands of the artwork and scrape flows build on this engine.
mod provider_stream;
mod providers_ipc;
mod ratings_ipc;
mod scan_ipc;
mod scrape_preview_ipc;
mod scrape_stream_ipc;
mod settings_ipc;
mod sources_ipc;
mod tools_ipc;
mod torrent_indexer_ipc;
mod torrent_ipc;
mod torrent_stream_ipc;
mod tv_detail_ipc;
mod updates_ipc;

/// The app context and, in portable mode, the windows the app builds itself so their web view
/// profile lives in the data folder.
fn app_context() -> (tauri::Context, Option<portable::PortableWindows>) {
    let mut context = tauri::generate_context!();
    let windows = portable::prepare(&mut context);
    (context, windows)
}

/// Wraps the command handler. Synchronous commands run on the window thread inside a web view
/// callback that cannot unwind, so a panic in one would abort the whole app. A panicking command
/// is logged as an error (the panic hook prints it too) and its call is rejected, so the page
/// sees a failure instead of the window closing.
fn contained<R: tauri::Runtime>(
    handler: impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke| {
        let command = invoke.message.command().to_owned();
        let resolver = invoke.resolver.clone();
        cv_app::guard::contain(&command, || handler(invoke)).unwrap_or_else(|| {
            resolver.reject(format!(
                "CineVault hit an internal error running {command}."
            ));
            true
        })
    }
}

fn main() {
    // Synchronous commands and window callbacks run on this thread, which belongs to no tokio
    // runtime: a task started from there (submitting a job, asking a provider again) would panic
    // and abort the app. Entering the app's runtime here, and handing it to the job lanes for
    // any other thread outside a runtime, lets such work start from anywhere. The guard lives
    // until the process ends.
    let runtime = tauri::async_runtime::handle();
    cv_jobs::set_fallback_runtime(runtime.inner().clone());
    let _inside_runtime = runtime.inner().enter();

    let builder = ipc::builder();
    let (context, portable_windows) = app_context();

    // Keep the TypeScript bindings fresh while developing.
    #[cfg(debug_assertions)]
    {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(ipc::BINDINGS_PATH);
        builder
            .export(ipc::typescript(), path)
            .expect("failed to export the TypeScript bindings");
    }

    // First: a second process hands over to the running one and exits before anything else
    // (settings, database) is opened. The rest happens in `setup`, after this, in this order: the
    // fresh start's database step (an earlier version's library set aside as `library-old.db`),
    // its settings step (`settings-old.json`, defaults, the first-run setup opens again), then the
    // app services (`boot::Boot::start`).
    let mut app =
        ipc::with_dev_state(tauri::Builder::default()).plugin(instance::single_instance_plugin());
    // The window-state plugin writes to %APPDATA% whatever it is told, so a portable copy keeps
    // the window's bounds in its own data folder instead (`portable`).
    if portable_windows.is_none() {
        app = app.plugin(instance::window_state_plugin());
    }
    app
        // Opens folders and web addresses from Rust (the web view's external links). No
        // capability grants the web view the plugin's own commands.
        .plugin(tauri_plugin_opener::init())
        // The first-run setup opens the system folder picker from Rust; no capability grants the
        // web view the plugin's own commands.
        .plugin(tauri_plugin_dialog::init())
        // The maximize button of the custom title bar carries this id; the plugin lays a native
        // hit-test region over it so Windows 11 shows the snap-layout flyout on hover.
        .plugin(
            tauri_plugin_snap_layout::init()
                .button_id(chrome::MAXIMIZE_BUTTON_ID)
                .build(),
        )
        .manage(nfo_ipc::NfoState::new())
        .manage(edit_ipc::EditState::new())
        // Thumbnails reach the web view as plain HTTP-style responses, never over IPC.
        .register_asynchronous_uri_scheme_protocol(
            cv_artwork::img::SCHEME,
            img_ipc::handle_protocol,
        )
        .invoke_handler(contained(builder.invoke_handler()))
        .setup(move |app| {
            // A portable copy builds its windows here, with the web view profile beside the exe.
            if let Some(windows) = &portable_windows {
                windows.create(app)?;
            }
            builder.mount_events(app);
            // Logging, the library, settings, the thumbnail cache and the job manager are built
            // once, here; the state objects borrow from this.
            let boot = boot::Boot::start();
            // Another copy holds the data folder's lock (Linux). Say so and end this copy; it opens
            // no library, so the rest of setup only fills the state objects with empty services.
            if boot.already_running() {
                instance_lock::report_already_running(app.handle());
            }
            // What changed for which titles reaches open pages as one typed event.
            if let Some(services) = boot.services() {
                changes_ipc::relay(app.handle().clone(), services.changes());
            }
            let settings = settings_ipc::SettingsState::from_services(boot.services());
            app.manage(tools_ipc::ToolsState::from_services(
                boot.services(),
                &settings,
            ));
            app.manage(scan_ipc::ScanState::from_services(boot.services()));
            app.manage(drives_ipc::DrivesState::from_boot(&boot, app.handle()));
            app.manage(match_ipc::MatchState::from_services(boot.services()));
            app.manage(episode_order_ipc::EpisodeOrderState::from_services(
                boot.services(),
            ));
            app.manage(episode_scrape_ipc::EpisodeScrapeState::from_services(
                boot.services(),
            ));
            app.manage(artwork_ipc::ArtworkState::from_services(boot.services()));
            app.manage(providers_ipc::ProvidersState::from_services(
                boot.services(),
            ));
            app.manage(curation_ipc::CurationState::default());
            app.manage(collections_ipc::CollectionsState::from_boot(&boot));
            app.manage(activity_ipc::ActivityState::from_boot(&boot));
            let library = match boot.services() {
                Some(services) => sources_ipc::LibraryState::from_services(services),
                None => sources_ipc::LibraryState::unavailable(
                    "CineVault could not open the library, so library folders cannot be saved.",
                ),
            };
            app.manage(library);
            let images = img_ipc::ImageState::from_boot(&boot);
            app.manage(organize_ipc::OrganizeState::new(Arc::clone(
                images.sources(),
            )));
            app.manage(library_ops_ipc::LibraryOpsState::new(Arc::clone(
                images.sources(),
            )));
            app.manage(history_ipc::HistoryState::new(Arc::clone(images.sources())));
            // The poster index is told about the moves of batches that the command line sends.
            let image_sources = Arc::clone(images.sources());
            app.manage(images);
            app.manage(automation_ipc::AutomationState::default());
            app.manage(jobs_ipc::JobsState::from_boot(&boot));
            app.manage(notices_ipc::NoticesState::from_boot(&boot));
            app.manage(updates_ipc::UpdatesState::new(&settings));
            app.manage(provider_stream::SearchState::from_services(boot.services()));
            app.manage(torrent_stream_ipc::TorrentState::from_services(
                boot.services(),
            ));
            app.manage(settings);
            app.manage(boot);
            // Stream job progress to the UI and mirror it to the taskbar and dock.
            let taskbar: Arc<dyn jobs_ipc::Taskbar> = Arc::new(app.handle().clone());
            let handle = app.handle().clone();
            let background = app.handle().clone();
            // The background work (job failure watcher, daily backups, resuming the jobs of
            // the previous session) needs the async runtime.
            tauri::async_runtime::spawn(async move {
                // Settle what a crash left half done before anything scans the library folders
                // (the startup scan begins in `start_background`). It looks at the folders, so it
                // runs on a blocking thread: a drive that does not answer must not hold the
                // window.
                let recovering = background.clone();
                let recovered = tauri::async_runtime::spawn_blocking(move || {
                    let library = recovering.state::<sources_ipc::LibraryState>();
                    let boot = recovering.state::<boot::Boot>();
                    organize_ipc::recover_at_startup(&library, &boot);
                })
                .await;
                if let Err(error) = recovered {
                    tracing::warn!(%error, "organize crash recovery stopped");
                }
                let boot = background.state::<boot::Boot>();
                boot.start_background();
                // Open the channel for the `cinevault` command line once the services run.
                background
                    .state::<automation_ipc::AutomationState>()
                    .start(&boot, &image_sources);
            });
            tauri::async_runtime::spawn(async move {
                let jobs = handle.state::<jobs_ipc::JobsState>();
                jobs.run_feed(taskbar).await;
            });
            Ok(())
        })
        .build(context)
        .expect("failed to build the CineVault application")
        .run(|app, event| {
            // Closing the main window installs a downloaded update when the user asked for it.
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { .. },
                ..
            } = &event
                && label == instance::MAIN_WINDOW_LABEL
            {
                cv_app::guard::contain("update on close", || updates_ipc::install_on_close(app));
            }
            // On quit, pause the jobs so the next launch resumes what was unfinished.
            if let tauri::RunEvent::Exit = event {
                // Runs inside a window callback that cannot unwind: a panic must not abort the
                // shutdown halfway.
                cv_app::guard::contain("quit", || {
                    // Close the command-line channel first, so no new batch starts while quitting.
                    if let Some(automation) = app.try_state::<automation_ipc::AutomationState>() {
                        automation.stop();
                    }
                    if let Some(boot) = app.try_state::<boot::Boot>() {
                        boot.shutdown();
                    }
                });
            }
        });
}

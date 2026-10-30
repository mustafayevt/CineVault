//! The state the server builds once at start-up.
//!
//! The desktop app builds its command state objects lazily, from the window's setup hook. The
//! server has no window and one lifetime, so every state object is built here, beside the
//! services, and lives as long as the process. The command bodies themselves are the ones in
//! `cv-ipc`; nothing in this module decides what a command does.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use axum::response::sse::Event;
use cv_app::collections::CollectionService;
use cv_app::drives::DrivesService;
use cv_app::history::HistoryService;
use cv_app::services::AppServices;
use cv_ipc::artwork::ArtworkState;
use cv_ipc::curation::CurationState;
use cv_ipc::edit::EditState;
use cv_ipc::episode_order::EpisodeOrderState;
use cv_ipc::episode_scrape::EpisodeScrapeState;
use cv_ipc::history::HistoryState;
use cv_ipc::image::ImageContext;
use cv_ipc::library_ops::LibraryOpsState;
use cv_ipc::matching::MatchState;
use cv_ipc::nfo::NfoState;
use cv_ipc::organize::OrganizeState;
use cv_ipc::provider_stream::SearchState;
use cv_ipc::providers::ProvidersState;
use cv_ipc::scan::ScanState;
use cv_ipc::settings::SettingsState;
use cv_ipc::sources::LibraryState;
use cv_ipc::tools::ToolsState;
use cv_ipc::torrent_stream::TorrentState;
use cv_ipc::updates::UpdatesState;
use ipnet::IpNet;
use serde::Serialize;
use serde_json::json;
use tokio::sync::mpsc;

type SseSender = mpsc::Sender<Result<Event, std::convert::Infallible>>;

/// One Server-Sent Events connection per page, by the client id the page minted.
#[derive(Default)]
pub(crate) struct Streams {
    clients: StdMutex<HashMap<String, SseSender>>,
}

impl Streams {
    pub(crate) fn connect(
        &self,
        client: &str,
    ) -> mpsc::Receiver<Result<Event, std::convert::Infallible>> {
        let (sender, receiver) = mpsc::channel(256);
        if let Ok(mut clients) = self.clients.lock() {
            clients.insert(client.to_owned(), sender);
        }
        receiver
    }

    pub(crate) fn send(&self, client: &str, event: Event) -> bool {
        let Ok(clients) = self.clients.lock() else {
            return false;
        };
        let Some(sender) = clients.get(client) else {
            return false;
        };
        sender.try_send(Ok(event)).is_ok()
    }

    /// One channel message: the shape `ui/src/lib/ipc.ts` reads as `{kind, stream, index, message}`.
    pub(crate) fn channel<T: Serialize>(
        &self,
        client: &str,
        stream: u64,
        index: &AtomicU64,
        message: &T,
    ) -> bool {
        let index = index.fetch_add(1, Ordering::Relaxed);
        let data = json!({
            "kind": "channel",
            "stream": stream,
            "index": index,
            "message": message,
        });
        self.send(client, Event::default().data(data.to_string()))
    }

    /// A typed event, as the UI's `events` map names it.
    pub(crate) fn broadcast_event<T: Serialize>(&self, name: &str, payload: &T) {
        let Ok(clients) = self.clients.lock() else {
            return;
        };
        let data = json!({ "kind": "event", "event": name, "payload": payload }).to_string();
        for sender in clients.values() {
            let _ = sender.try_send(Ok(Event::default().data(data.clone())));
        }
    }
}

/// The command state objects, built once. They borrow the one set of services, exactly as the
/// desktop app's state objects do.
pub(crate) struct Commands {
    pub(crate) library: LibraryState,
    pub(crate) settings: SettingsState,
    pub(crate) scan: ScanState,
    pub(crate) matching: MatchState,
    pub(crate) artwork: ArtworkState,
    pub(crate) edit: EditState,
    pub(crate) organize: OrganizeState,
    pub(crate) nfo: NfoState,
    pub(crate) library_ops: LibraryOpsState,
    pub(crate) history: HistoryState,
    pub(crate) curation: CurationState,
    pub(crate) providers: ProvidersState,
    pub(crate) episode_order: EpisodeOrderState,
    pub(crate) episode_scrape: EpisodeScrapeState,
    pub(crate) torrent: TorrentState,
    pub(crate) tools: ToolsState,
    pub(crate) updates: UpdatesState,
    pub(crate) search: SearchState,
    /// The batch runner the selection actions queue their plans on.
    pub(crate) actions: Arc<cv_app::automation::CommandBackend>,
    pub(crate) drives: Option<Arc<DrivesService>>,
    pub(crate) collections: Option<Arc<CollectionService>>,
    /// The History reads. The undo commands go through the `history` state object beside it,
    /// which also knows the organize service and the deleted-files destination.
    pub(crate) history_reads: Arc<HistoryService>,
}

impl Commands {
    /// Every state object the commands need, over `services`. Must run inside the tokio runtime:
    /// the batch runner the selection actions use is a task.
    pub(crate) fn build(services: &Arc<AppServices>, images: &ImageContext) -> Self {
        let settings = SettingsState::from_services(Some(services));
        let tools = ToolsState::from_services(Some(services), &settings);
        let updates = UpdatesState::new(&settings);
        let library = LibraryState::from_services(services);
        let nfo = NfoState::new();
        let collections = nfo
            .service(&library)
            .ok()
            .map(|nfo| Arc::new(CollectionService::new(Arc::clone(services.db()), nfo)));
        let observer: Arc<dyn cv_db::repo::rename_journal::PathObserver> =
            Arc::clone(images.sources()) as _;
        let engine = cv_app::automation::BatchEngine::new(
            Arc::clone(services),
            cv_app::automation::EngineOptions {
                path_observer: Some(observer),
                ..cv_app::automation::EngineOptions::default()
            },
        );
        let actions = cv_app::automation::CommandBackend::start(engine);
        let drives = DrivesService::new(Arc::clone(services.db()), Arc::clone(services.scan()));
        let choice: Arc<dyn cv_app::deleted_files::DeletedFilesSource> =
            Arc::clone(services.settings()) as _;
        drives.set_trash(
            cv_app::library_ops::PolicyBin::os(Arc::clone(&choice)),
            choice,
        );
        Self {
            settings,
            tools,
            updates,
            scan: ScanState::from_services(Some(services)),
            matching: MatchState::from_services(Some(services)),
            episode_order: EpisodeOrderState::from_services(Some(services)),
            episode_scrape: EpisodeScrapeState::from_services(Some(services)),
            artwork: ArtworkState::from_services(Some(services)),
            providers: ProvidersState::from_services(Some(services)),
            curation: CurationState::default(),
            organize: OrganizeState::new(Arc::clone(images.sources())),
            library_ops: LibraryOpsState::new(Arc::clone(images.sources())),
            history: HistoryState::new(Arc::clone(images.sources())),
            history_reads: Arc::new(HistoryService::new(Arc::clone(services.db()))),
            search: SearchState::from_services(Some(services)),
            actions,
            torrent: TorrentState::from_services(Some(services)),
            edit: EditState::new(),
            nfo,
            library,
            drives: Some(drives),
            collections,
        }
    }
}

/// Everything a handler needs. Cheap to clone: every field is shared.
#[derive(Clone)]
pub(crate) struct WebState {
    pub(crate) services: Arc<AppServices>,
    pub(crate) images: Arc<ImageContext>,
    pub(crate) auth: Arc<crate::auth::Auth>,
    pub(crate) progress: Arc<cv_ipc::jobs::ProgressHub>,
    pub(crate) notices: Arc<cv_ipc::notices::NoticeHub>,
    pub(crate) streams: Arc<Streams>,
    pub(crate) commands: Arc<Commands>,
    pub(crate) data_root: std::path::PathBuf,
    pub(crate) bind: String,
    pub(crate) port: u16,
    pub(crate) keys_locked: bool,
    pub(crate) trust_proxy: Vec<IpNet>,
    pub(crate) ui_dir: std::path::PathBuf,
}

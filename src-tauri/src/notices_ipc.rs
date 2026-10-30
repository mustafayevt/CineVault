//! Messages from background work for the notification centre.
//!
//! The shell opens one channel at start-up. Notices that happened before it was open (a damaged
//! settings file found while the window was still loading) arrive first, then live ones. The DTO
//! and the replaying hub live in [`cv_ipc::notices`].

use std::sync::Arc;

use cv_ipc::notices::NoticeHub;
use tauri::State;
use tauri::ipc::Channel;

use crate::boot::Boot;

use cv_ipc::notices::NoticeDto;

/// Held in Tauri state.
pub struct NoticesState {
    hub: Arc<NoticeHub>,
}

impl NoticesState {
    pub fn from_boot(boot: &Boot) -> Self {
        Self {
            hub: NoticeHub::from_app(Arc::clone(boot.notices())),
        }
    }
}

/// Opens the stream of notices. Call it once; the stream lives as long as the window.
#[tauri::command]
#[specta::specta]
pub fn subscribe_notices(state: State<'_, NoticesState>, on_notice: Channel<NoticeDto>) {
    state
        .hub
        .subscribe(move |notice| on_notice.send(notice).is_ok());
}

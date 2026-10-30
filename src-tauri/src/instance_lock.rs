//! Reporting a second copy that found the library held.
//!
//! The one-writer rule itself lives in `cv_app::owner`: every surface takes the data folder's
//! owner file and a second process opens nothing. This module only tells the person why the
//! window they are looking at is empty.

use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// Tells the user that another process holds the library, then ends this copy when the message is
/// closed. The dialog is shown without waiting: setup runs on the main thread, and a blocking
/// dialog must not run there.
pub fn report_already_running<R: Runtime>(app: &AppHandle<R>, message: &str) {
    tracing::warn!(%message, "another CineVault process holds the data folder; this copy exits");
    let handle = app.clone();
    let message = message.to_owned();
    app.dialog()
        .message(message)
        .kind(MessageDialogKind::Info)
        .show(move |_| handle.exit(0));
}

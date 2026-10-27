//! Messages from background work for the notification centre.
//!
//! The shell opens one channel at start-up. Notices that happened before it was open (a damaged
//! settings file found while the window was still loading) arrive first, then live ones.

use std::sync::Arc;

use cv_app::notices::{AppNotice, NoticeHub, NoticeLevel};
use serde::Serialize;
use specta::Type;
use tauri::State;
use tauri::ipc::Channel;

use crate::boot::Boot;

/// Held in Tauri state.
pub struct NoticesState {
    hub: Arc<NoticeHub>,
}

impl NoticesState {
    pub fn from_boot(boot: &Boot) -> Self {
        Self {
            hub: Arc::clone(boot.notices()),
        }
    }
}

/// How serious a notice is; matches the notification centre's severities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum NoticeSeverityDto {
    Info,
    Warning,
    Error,
}

/// One message for the notification centre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NoticeDto {
    pub severity: NoticeSeverityDto,
    /// One line: what happened.
    pub title: String,
    /// More detail, such as the reasons.
    pub detail: Option<String>,
    /// Where it came from, such as a provider or job name.
    pub source: Option<String>,
    /// The provider whose key was rejected or rate limited: the notice links to that provider's
    /// card on the Providers page.
    pub provider_key: Option<String>,
    /// A Settings page that fixes what the notice is about, such as `/settings/tools`; the
    /// notice links to it.
    pub settings_link: Option<String>,
    /// The fix the notice offers, such as `retry_imdb_dataset`; None when there is none.
    pub action: Option<String>,
    /// The unfinished move or rename the notice is about; Resume and Roll back act on it.
    pub batch_id: Option<String>,
    /// What the notice is about, such as `source-skipped:<folder>`: a later notice of the same
    /// topic replaces it.
    pub topic: Option<String>,
    /// Not a message: the notices of `topic` no longer hold and leave Activity.
    pub resolves: bool,
}

impl From<&AppNotice> for NoticeDto {
    fn from(notice: &AppNotice) -> Self {
        Self {
            severity: match notice.level {
                NoticeLevel::Info => NoticeSeverityDto::Info,
                NoticeLevel::Warning => NoticeSeverityDto::Warning,
                NoticeLevel::Error => NoticeSeverityDto::Error,
            },
            title: notice.title.clone(),
            detail: notice.detail.clone(),
            source: notice.source.clone(),
            provider_key: notice.provider_key.clone(),
            settings_link: notice
                .settings_link
                .clone()
                .filter(|path| path.starts_with("/settings/")),
            action: notice.action.map(|action| action.as_str().to_owned()),
            batch_id: notice.batch_id.clone(),
            topic: notice.topic.clone(),
            resolves: notice.resolves,
        }
    }
}

/// Opens the stream of notices. Call it once; the stream lives as long as the window.
#[tauri::command]
#[specta::specta]
pub fn subscribe_notices(state: State<'_, NoticesState>, on_notice: Channel<NoticeDto>) {
    state
        .hub
        .subscribe(move |notice| on_notice.send(NoticeDto::from(notice)).is_ok());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_settings_link_is_passed_on_only_for_a_settings_page() {
        let tools =
            AppNotice::new(NoticeLevel::Info, "Needs MediaInfo").settings_link("/settings/tools");
        assert_eq!(
            NoticeDto::from(&tools).settings_link.as_deref(),
            Some("/settings/tools")
        );
        let elsewhere =
            AppNotice::new(NoticeLevel::Info, "Odd").settings_link("https://example.com");
        assert_eq!(NoticeDto::from(&elsewhere).settings_link, None);
    }

    #[test]
    fn levels_map_to_the_centre_severities() {
        let notice = AppNotice::new(NoticeLevel::Warning, "Slow")
            .detail("why")
            .source("tmdb")
            .provider_key("tmdb");
        let dto = NoticeDto::from(&notice);
        assert_eq!(dto.severity, NoticeSeverityDto::Warning);
        assert_eq!(dto.detail.as_deref(), Some("why"));
        assert_eq!(dto.source.as_deref(), Some("tmdb"));
        assert_eq!(dto.provider_key.as_deref(), Some("tmdb"));
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""severity":"warning""#), "{json}");
    }
}

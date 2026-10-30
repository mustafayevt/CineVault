//! Messages from background work for the notification centre, and the replaying hub behind them.
//!
//! The app publishes [`AppNotice`]s; [`NoticeHub`] converts them once and fans the [`NoticeDto`]
//! out to every open page. Its bounded ring is replayed to a new subscriber, so a second page does
//! not miss what happened while the first one was open.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use cv_app::notices::{AppNotice, NoticeHub as AppNoticeHub, NoticeLevel};
use serde::Serialize;
use specta::Type;

/// How many notices are kept for a page that opens later.
const KEEP: usize = 100;

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

struct Inner {
    ring: VecDeque<NoticeDto>,
    sinks: Vec<Box<dyn FnMut(NoticeDto) -> bool + Send>>,
}

/// Fans notices out to subscribers and keeps the newest [`KEEP`] for replay.
pub struct NoticeHub {
    inner: Mutex<Inner>,
}

impl Default for NoticeHub {
    fn default() -> Self {
        Self::new()
    }
}

impl NoticeHub {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                ring: VecDeque::new(),
                sinks: Vec::new(),
            }),
        }
    }

    /// A hub that forwards every notice the app publishes. The subscription lives as long as the
    /// app hub.
    #[must_use]
    pub fn from_app(app: Arc<AppNoticeHub>) -> Arc<Self> {
        let hub = Arc::new(Self::new());
        let forward = Arc::downgrade(&hub);
        app.subscribe(move |notice| {
            if let Some(hub) = forward.upgrade() {
                hub.publish(notice);
            }
            true
        });
        hub
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Converts and sends a notice to every subscriber and keeps it for replay.
    pub fn publish(&self, notice: &AppNotice) {
        let dto = NoticeDto::from(notice);
        let mut inner = self.lock();
        inner.sinks.retain_mut(|sink| sink(dto.clone()));
        if inner.ring.len() == KEEP {
            inner.ring.pop_front();
        }
        inner.ring.push_back(dto);
    }

    /// Adds a subscriber and first replays the kept notices, oldest first.
    pub fn subscribe(&self, mut sink: impl FnMut(NoticeDto) -> bool + Send + 'static) {
        let mut inner = self.lock();
        for notice in &inner.ring {
            if !sink(notice.clone()) {
                return;
            }
        }
        inner.sinks.push(Box::new(sink));
    }

    /// How many subscribers are live.
    #[must_use]
    pub fn subscribers(&self) -> usize {
        self.lock().sinks.len()
    }
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

    #[test]
    fn a_new_subscriber_replays_the_kept_notices() {
        let hub = NoticeHub::new();
        hub.publish(&AppNotice::new(NoticeLevel::Info, "first"));
        hub.publish(&AppNotice::new(NoticeLevel::Warning, "second"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        hub.subscribe(move |notice| {
            sink.lock().unwrap().push(notice.title);
            true
        });
        assert_eq!(*seen.lock().unwrap(), vec!["first", "second"]);
        assert_eq!(hub.subscribers(), 1);
    }
}

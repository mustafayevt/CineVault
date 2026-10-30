//! Messages from background work for the notification centre.
//!
//! Anything that fails away from the user's current action (a job item, a provider request, a
//! damaged settings file) is published here as an [`AppNotice`]. The shell subscribes once and
//! shows each notice in the notification centre; it never opens a dialog.
//!
//! A notice published while nobody listens yet (the window is still starting) is kept and
//! handed to the first subscriber, so nothing that happens during startup is lost.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use cv_jobs::{JobManager, JobSnapshot, JobState, LaneId};
use cv_providers::ProviderError;

/// How many notices wait for the first subscriber.
const BACKLOG_LIMIT: usize = 100;
/// How many failure reasons a job notice spells out.
const REASONS_SHOWN: usize = 3;

/// How serious a notice is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeLevel {
    Info,
    Warning,
    Error,
}

/// One message for the notification centre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppNotice {
    pub level: NoticeLevel,
    /// One line: what happened.
    pub title: String,
    /// More detail, such as the reasons.
    pub detail: Option<String>,
    /// Where it came from, such as a provider or job name.
    pub source: Option<String>,
    /// Set when the notice is about the user's key being rejected or rate limited: the provider
    /// key (for example `tmdb`) whose card on the Providers page the notice links to.
    pub provider_key: Option<String>,
    /// A Settings page that fixes what the notice is about (for example `/settings/tools`): the
    /// notice links to it, so the text never has to spell out a route.
    pub settings_link: Option<String>,
    /// What the user can do about it from Activity's Needs attention, besides reading it.
    pub action: Option<NoticeAction>,
    /// The unfinished journal batch the notice is about (with [`NoticeAction::ResolveUnfinished`]).
    pub batch_id: Option<String>,
    /// What the notice is about, such as `source:<folder>`: a later notice of the same topic
    /// replaces it, and [`AppNotice::resolved`] takes it away once it no longer holds.
    pub topic: Option<String>,
    /// This is not a message but says the notices of `topic` no longer hold.
    pub resolves: bool,
}

/// A fix a notice offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeAction {
    /// Download the IMDb ratings dataset again and fetch the ratings that failed.
    RetryImdbDataset,
    /// Resume an unfinished move or rename, or roll it back (the notice's `batch_id`).
    ResolveUnfinished,
}

impl NoticeAction {
    /// The stable word sent to the UI.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RetryImdbDataset => "retry_imdb_dataset",
            Self::ResolveUnfinished => "resolve_unfinished",
        }
    }
}

impl AppNotice {
    pub fn new(level: NoticeLevel, title: impl Into<String>) -> Self {
        Self {
            level,
            title: title.into(),
            detail: None,
            source: None,
            provider_key: None,
            settings_link: None,
            action: None,
            batch_id: None,
            topic: None,
            resolves: false,
        }
    }

    /// Says the notices of `topic` no longer hold: the notification centre drops them.
    #[must_use]
    pub fn resolved(topic: impl Into<String>) -> Self {
        Self {
            topic: Some(topic.into()),
            resolves: true,
            ..Self::new(NoticeLevel::Info, "")
        }
    }

    /// Names what the notice is about, so a later one replaces it and it can be taken back.
    #[must_use]
    pub fn topic(mut self, topic: impl Into<String>) -> Self {
        self.topic = Some(topic.into());
        self
    }

    /// Names the unfinished journal batch the notice is about.
    #[must_use]
    pub fn batch_id(mut self, batch_id: impl Into<String>) -> Self {
        self.batch_id = Some(batch_id.into());
        self
    }

    /// Offers a fix (a Retry button) on the notice.
    #[must_use]
    pub fn action(mut self, action: NoticeAction) -> Self {
        self.action = Some(action);
        self
    }

    /// Links the notice to a Settings page (a path under `/settings`).
    #[must_use]
    pub fn settings_link(mut self, path: impl Into<String>) -> Self {
        self.settings_link = Some(path.into());
        self
    }

    /// Links the notice to the provider's card on the Providers page.
    #[must_use]
    pub fn provider_key(mut self, provider_key: impl Into<String>) -> Self {
        self.provider_key = Some(provider_key.into());
        self
    }

    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    #[must_use]
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

/// Receives notices. Returns `false` once it is gone, and the hub forgets it.
type Sink = Box<dyn FnMut(&AppNotice) -> bool + Send>;

#[derive(Default)]
struct Inner {
    backlog: VecDeque<AppNotice>,
    sinks: Vec<Sink>,
}

/// Fans notices out to the subscribers.
#[derive(Default)]
pub struct NoticeHub {
    inner: Mutex<Inner>,
}

impl NoticeHub {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sends a notice to every subscriber, or keeps it for the first one.
    pub fn publish(&self, notice: AppNotice) {
        tracing::debug!(title = %notice.title, "notice published");
        let mut inner = self.lock();
        inner.sinks.retain_mut(|sink| sink(&notice));
        if inner.sinks.is_empty() {
            if inner.backlog.len() == BACKLOG_LIMIT {
                inner.backlog.pop_front();
            }
            inner.backlog.push_back(notice);
        }
    }

    /// Adds a subscriber. It first receives the notices that were kept while nobody listened.
    pub fn subscribe(&self, mut sink: impl FnMut(&AppNotice) -> bool + Send + 'static) {
        let mut inner = self.lock();
        while let Some(notice) = inner.backlog.pop_front() {
            if !sink(&notice) {
                inner.backlog.push_front(notice);
                return;
            }
        }
        inner.sinks.push(Box::new(sink));
    }

    /// Publishes the notice for a provider request that failed outside a job, or `None` for
    /// errors that are no problem to report (a title the provider simply does not know).
    pub fn provider_failure(&self, provider: &str, error: &ProviderError) {
        if let Some(notice) = provider_notice(provider, error) {
            self.publish(notice);
        }
    }

    /// Publishes a notice for every job that ends with failed items, once per job. Runs until
    /// the task is dropped, so spawn it before any job is submitted.
    pub async fn watch_jobs(self: Arc<Self>, manager: JobManager) {
        let mut reported = std::collections::HashSet::new();
        manager
            .progress_feed(cv_jobs::PROGRESS_INTERVAL)
            .run(move |tick| {
                for snapshot in &tick.jobs {
                    if !snapshot.state.is_terminal() || !reported.insert(snapshot.id) {
                        continue;
                    }
                    let groups = manager
                        .get(snapshot.id)
                        .map(|job| job.failure_groups())
                        .unwrap_or_default();
                    if let Some(notice) = job_notice(snapshot, &groups) {
                        self.publish(notice);
                    }
                }
            })
            .await;
    }
}

/// The notice for a finished job, or `None` when nothing went wrong. A job on a provider's lane
/// is reported as that provider's failure.
pub fn job_notice(snapshot: &JobSnapshot, groups: &[cv_jobs::FailureGroup]) -> Option<AppNotice> {
    if snapshot.state == JobState::Cancelled
        || (snapshot.failed == 0 && snapshot.state != JobState::Failed)
    {
        return None;
    }
    let level = if snapshot.state == JobState::Failed {
        NoticeLevel::Error
    } else {
        NoticeLevel::Warning
    };
    let failed = snapshot.failed;
    let title = if snapshot.state == JobState::Failed && failed == 0 {
        format!("{} failed", snapshot.name)
    } else if failed == 1 {
        format!("{}: 1 item failed", snapshot.name)
    } else {
        format!("{}: {failed} items failed", snapshot.name)
    };
    let mut notice = AppNotice::new(level, title).source(match &snapshot.lane {
        LaneId::Provider(id) => id.clone(),
        _ => snapshot.name.clone(),
    });
    if !groups.is_empty() {
        let mut lines: Vec<String> = groups
            .iter()
            .take(REASONS_SHOWN)
            .map(cv_jobs::FailureGroup::summary)
            .collect();
        if groups.len() > REASONS_SHOWN {
            lines.push(format!("and {} more reasons", groups.len() - REASONS_SHOWN));
        }
        notice = notice.detail(lines.join("\n"));
    }
    // A provider lane whose items failed for the key or the key's limit links to its card.
    if let LaneId::Provider(id) = &snapshot.lane
        && groups.iter().any(|group| {
            matches!(
                group.reason.as_str(),
                "provider credentials" | "provider rate limit"
            )
        })
    {
        notice = notice.provider_key(id.clone());
    }
    Some(notice)
}

/// The notice for a provider error, or `None` for "no match", which is a normal answer.
pub fn provider_notice(provider: &str, error: &ProviderError) -> Option<AppNotice> {
    let level = match error.root() {
        ProviderError::NotFound(_) | ProviderError::Unsupported { .. } => return None,
        ProviderError::RateLimited { .. } | ProviderError::FormatChanged(_) => NoticeLevel::Warning,
        _ => NoticeLevel::Error,
    };
    let mut notice = AppNotice::new(level, error.to_string()).source(provider);
    if let Some(key) = error.key_problem_provider() {
        notice = notice.provider_key(key);
    }
    Some(notice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn notice(title: &str) -> AppNotice {
        AppNotice::new(NoticeLevel::Info, title)
    }

    #[test]
    fn the_first_subscriber_gets_what_was_published_before_it() {
        let hub = NoticeHub::new();
        hub.publish(notice("one"));
        hub.publish(notice("two"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        hub.subscribe(move |n| {
            sink.lock().unwrap().push(n.title.clone());
            true
        });
        hub.publish(notice("three"));
        assert_eq!(*seen.lock().unwrap(), ["one", "two", "three"]);
    }

    #[test]
    fn a_backlog_is_handed_out_once() {
        let hub = NoticeHub::new();
        hub.publish(notice("early"));
        let counts: Vec<Arc<AtomicUsize>> = (0..2).map(|_| Arc::default()).collect();
        for count in &counts {
            let count = Arc::clone(count);
            hub.subscribe(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
                true
            });
        }
        // The first subscriber took the notice; the second saw nothing.
        assert_eq!(counts[0].load(Ordering::SeqCst), 1);
        assert_eq!(counts[1].load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_subscriber_that_is_already_gone_does_not_take_the_backlog() {
        let hub = NoticeHub::new();
        hub.publish(notice("early"));
        hub.subscribe(|_| false);
        let seen = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&seen);
        hub.subscribe(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            true
        });
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_gone_subscriber_is_forgotten() {
        let hub = NoticeHub::new();
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        hub.subscribe(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            false
        });
        hub.publish(notice("a"));
        hub.publish(notice("b"));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_backlog_keeps_the_newest() {
        let hub = NoticeHub::new();
        for i in 0..(BACKLOG_LIMIT + 5) {
            hub.publish(notice(&i.to_string()));
        }
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        hub.subscribe(move |n| {
            sink.lock().unwrap().push(n.title.clone());
            true
        });
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), BACKLOG_LIMIT);
        assert_eq!(seen[0], "5");
    }

    fn snapshot(state: JobState, failed: usize, lane: LaneId) -> JobSnapshot {
        JobSnapshot {
            id: cv_jobs::JobId(1),
            name: "Scraping 3 titles".to_owned(),
            lane,
            started_by: String::new(),
            state,
            priority: cv_jobs::Priority::Normal,
            total: 10,
            done: 10,
            failed,
            rolled_back: 0,
            current: Vec::new(),
            throughput_per_sec: None,
            eta_secs: None,
            elapsed_secs: 1.0,
            outcome: None,
            result: None,
        }
    }

    #[test]
    fn a_clean_or_cancelled_job_says_nothing() {
        assert!(job_notice(&snapshot(JobState::Completed, 0, LaneId::Disk), &[]).is_none());
        assert!(job_notice(&snapshot(JobState::Cancelled, 3, LaneId::Disk), &[]).is_none());
    }

    #[test]
    fn a_job_with_failures_lists_the_reasons() {
        let groups = vec![
            cv_jobs::FailureGroup {
                reason: "rate limit".to_owned(),
                items: vec![
                    cv_jobs::FailedItem {
                        index: 0,
                        label: "a".to_owned(),
                    },
                    cv_jobs::FailedItem {
                        index: 1,
                        label: "b".to_owned(),
                    },
                ],
            },
            cv_jobs::FailureGroup {
                reason: "timeout".to_owned(),
                items: vec![cv_jobs::FailedItem {
                    index: 2,
                    label: "c".to_owned(),
                }],
            },
        ];
        let notice =
            job_notice(&snapshot(JobState::Completed, 3, LaneId::Disk), &groups).expect("a notice");
        assert_eq!(notice.level, NoticeLevel::Warning);
        assert_eq!(notice.title, "Scraping 3 titles: 3 items failed");
        assert_eq!(
            notice.detail.as_deref(),
            Some("2 failed · rate limit\n1 failed · timeout")
        );
        assert_eq!(notice.source.as_deref(), Some("Scraping 3 titles"));
    }

    #[test]
    fn a_provider_job_names_the_provider() {
        let notice = job_notice(
            &snapshot(JobState::Failed, 10, LaneId::provider("tmdb")),
            &[],
        )
        .expect("a notice");
        assert_eq!(notice.level, NoticeLevel::Error);
        assert_eq!(notice.source.as_deref(), Some("tmdb"));
    }

    #[test]
    fn provider_errors_are_reported_except_no_match() {
        let auth = ProviderError::Auth {
            provider: "TMDB".to_owned(),
            message: "bad key".to_owned(),
        };
        let notice = provider_notice("tmdb", &auth).expect("a notice");
        assert_eq!(notice.level, NoticeLevel::Error);
        assert!(notice.title.contains("rejected the credentials"));
        let limited = ProviderError::RateLimited {
            provider: "TMDB".to_owned(),
            retry_after_secs: 5,
        };
        assert_eq!(
            provider_notice("tmdb", &limited).map(|n| n.level),
            Some(NoticeLevel::Warning)
        );
        assert!(provider_notice("tmdb", &ProviderError::NotFound("x".into())).is_none());
    }

    #[test]
    fn a_rejected_or_rate_limited_key_links_the_notice_to_its_card() {
        let none = cv_providers::ResolvedCredentials::default();
        let rejected = ProviderError::Auth {
            provider: "TMDB".to_owned(),
            message: "bad key".to_owned(),
        }
        .with_key_hint(&none, "tmdb");
        let notice = provider_notice("tmdb", &rejected).expect("a notice");
        assert_eq!(notice.provider_key.as_deref(), Some("tmdb"));
        assert!(
            notice.title.contains("rejected your key"),
            "{}",
            notice.title
        );

        let limited = ProviderError::RateLimited {
            provider: "TMDB".to_owned(),
            retry_after_secs: 5,
        }
        .with_key_hint(&none, "tmdb");
        let notice = provider_notice("tmdb", &limited).expect("a notice");
        assert_eq!(notice.level, NoticeLevel::Warning);
        assert_eq!(notice.provider_key.as_deref(), Some("tmdb"));

        // Other failures carry no link.
        let down = ProviderError::Request {
            provider: "TMDB".to_owned(),
            message: "timeout".to_owned(),
        };
        assert_eq!(provider_notice("tmdb", &down).unwrap().provider_key, None);
    }

    #[test]
    fn a_provider_job_that_failed_for_the_key_links_to_the_providers_card() {
        let groups = vec![cv_jobs::FailureGroup {
            reason: "provider credentials".to_owned(),
            items: vec![cv_jobs::FailedItem {
                index: 0,
                label: "a".to_owned(),
            }],
        }];
        let notice = job_notice(
            &snapshot(JobState::Completed, 1, LaneId::provider("tmdb")),
            &groups,
        )
        .expect("a notice");
        assert_eq!(notice.provider_key.as_deref(), Some("tmdb"));
    }
}

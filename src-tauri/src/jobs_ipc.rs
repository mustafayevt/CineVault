//! Job progress over Tauri Channels, mirrored to the taskbar and dock.
//!
//! The UI opens one [`Channel`] with [`subscribe_job_progress`]. A [`ProgressFeed`] polls the
//! [`JobManager`] at most every 100 ms, so each job is reported at most 10 times a second, and
//! every report is one [`JobProgressUpdate`] carrying only the jobs that changed. The same
//! report moves the Windows taskbar button and the macOS dock tile through the window's
//! progress bar.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use cv_jobs::{
    JobId, JobManager, JobSnapshot, JobState, LaneConfig, Lanes, ManagerConfig, Overall,
    OverallKind, OverallTracker, PROGRESS_INTERVAL, Priority, ProgressTick,
};
// Only the sample job (a development aid) and its tests use these.
#[cfg(any(debug_assertions, test))]
use cv_jobs::{ItemControl, ItemError, ItemWork, JobSpec, LaneId};
use serde::Serialize;
use specta::Type;
use tauri::ipc::Channel;
use tauri::window::{ProgressBarState, ProgressBarStatus};
use tauri::{AppHandle, Manager, State};

use crate::boot::Boot;

/// Job lifecycle state as the UI sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum JobStateDto {
    Queued,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// Scheduling priority as the UI sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PriorityDto {
    Low,
    Normal,
    High,
}

/// Progress of one job.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct JobProgress {
    /// Session-local job id.
    pub id: u32,
    /// Display name.
    pub name: String,
    /// Lane the job runs on, such as `disk`, `cpu` or `provider:tmdb`.
    pub lane: String,
    pub state: JobStateDto,
    pub priority: PriorityDto,
    /// Number of items.
    pub total: u32,
    /// Items processed so far, successfully or not.
    pub done: u32,
    /// Items that failed (a subset of `done`).
    pub failed: u32,
    /// Items rolled back by a cancel.
    pub rolled_back: u32,
    /// Labels of the items being processed right now.
    pub current: Vec<String>,
    /// Items per second while the job ran.
    pub throughput_per_sec: Option<f64>,
    /// Estimated seconds remaining.
    pub eta_secs: Option<f64>,
    /// Seconds the job has run, pauses excluded.
    pub elapsed_secs: f64,
    /// Once the job is over, what it did beyond its counts (`6 titles wait in Needs review`).
    pub outcome: Option<String>,
    /// Once the job is over, what it produced, as its step in the run history (`212 new`,
    /// `198 scraped`, `artwork for 190`); `None` shows the job's name.
    pub result: Option<String>,
}

/// What the combined taskbar and dock progress shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OverallKindDto {
    Idle,
    Indeterminate,
    Normal,
    Paused,
}

/// Combined progress of all active jobs (the value mirrored to the taskbar and dock).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct OverallProgress {
    pub kind: OverallKindDto,
    /// Whole percentage in 0..=100.
    pub percent: u8,
    /// Jobs that are queued, running or paused.
    pub active_jobs: u32,
}

/// One message on the progress channel.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct JobProgressUpdate {
    /// The jobs that changed since the previous message (every job in the first message).
    pub jobs: Vec<JobProgress>,
    /// The combined progress.
    pub overall: OverallProgress,
}

fn small(n: impl TryInto<u32>) -> u32 {
    n.try_into().unwrap_or(u32::MAX)
}

impl From<&JobSnapshot> for JobProgress {
    fn from(s: &JobSnapshot) -> Self {
        Self {
            id: small(s.id.0),
            name: s.name.clone(),
            lane: s.lane.to_string(),
            state: match s.state {
                JobState::Queued => JobStateDto::Queued,
                JobState::Running => JobStateDto::Running,
                JobState::Paused => JobStateDto::Paused,
                JobState::Completed => JobStateDto::Completed,
                JobState::Failed => JobStateDto::Failed,
                JobState::Cancelled => JobStateDto::Cancelled,
            },
            priority: match s.priority {
                Priority::Low => PriorityDto::Low,
                Priority::Normal => PriorityDto::Normal,
                Priority::High => PriorityDto::High,
            },
            total: small(s.total),
            done: small(s.done),
            failed: small(s.failed),
            rolled_back: small(s.rolled_back),
            current: s.current.clone(),
            throughput_per_sec: s.throughput_per_sec,
            eta_secs: s.eta_secs,
            elapsed_secs: s.elapsed_secs,
            outcome: s.outcome.clone(),
            result: s.result.clone(),
        }
    }
}

impl From<&Overall> for OverallProgress {
    fn from(o: &Overall) -> Self {
        Self {
            kind: match o.kind {
                OverallKind::Idle => OverallKindDto::Idle,
                OverallKind::Indeterminate => OverallKindDto::Indeterminate,
                OverallKind::Normal => OverallKindDto::Normal,
                OverallKind::Paused => OverallKindDto::Paused,
            },
            percent: o.percent(),
            active_jobs: small(o.active_jobs),
        }
    }
}

fn update_of(jobs: &[JobSnapshot], overall: &Overall) -> JobProgressUpdate {
    JobProgressUpdate {
        jobs: jobs.iter().map(JobProgress::from).collect(),
        overall: overall.into(),
    }
}

/// How the combined progress looks on the taskbar button (Windows) and the dock tile (macOS).
pub fn taskbar_state(overall: &Overall) -> ProgressBarState {
    let (status, progress) = match overall.kind {
        OverallKind::Idle => (ProgressBarStatus::None, None),
        OverallKind::Indeterminate => (ProgressBarStatus::Indeterminate, None),
        OverallKind::Normal => (ProgressBarStatus::Normal, Some(overall.percent())),
        OverallKind::Paused => (ProgressBarStatus::Paused, Some(overall.percent())),
    };
    ProgressBarState {
        status: Some(status),
        progress: progress.map(u64::from),
    }
}

/// Where the combined progress is shown. The app implements it with the main window.
pub trait Taskbar: Send + Sync {
    fn show(&self, state: ProgressBarState);
}

impl Taskbar for AppHandle {
    fn show(&self, state: ProgressBarState) {
        let Some(window) = self.get_webview_window("main") else {
            return;
        };
        if let Err(error) = window.set_progress_bar(state) {
            tracing::warn!(%error, "could not update the taskbar/dock progress");
        }
    }
}

struct Subscriber {
    channel: Channel<JobProgressUpdate>,
    /// The next report is sent in full, so a report that was prepared before this subscriber
    /// joined cannot leave it with stale jobs.
    needs_full: bool,
}

/// The channels the UI opened. Dead channels (a closed window) are dropped on the next send.
#[derive(Default)]
pub struct ProgressHub {
    subscribers: Mutex<Vec<Subscriber>>,
}

impl ProgressHub {
    fn lock(&self) -> MutexGuard<'_, Vec<Subscriber>> {
        self.subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().len()
    }

    /// Adds a channel and immediately sends it `initial`, the state of every job.
    pub fn subscribe(&self, channel: Channel<JobProgressUpdate>, initial: JobProgressUpdate) {
        if channel.send(initial).is_err() {
            return;
        }
        self.lock().push(Subscriber {
            channel,
            needs_full: true,
        });
    }

    /// Sends one report to every subscriber: the changed jobs, or for a new subscriber every job
    /// (`all_jobs` is only called when someone needs it).
    pub fn publish(&self, tick: &ProgressTick, all_jobs: impl FnOnce() -> Vec<JobSnapshot>) {
        let mut subscribers = self.lock();
        if subscribers.is_empty() {
            return;
        }
        let delta = update_of(&tick.jobs, &tick.overall);
        let mut full: Option<JobProgressUpdate> = None;
        let mut all_jobs = Some(all_jobs);
        subscribers.retain_mut(|subscriber| {
            let update = if subscriber.needs_full {
                subscriber.needs_full = false;
                if full.is_none() {
                    let jobs = all_jobs.take().map(|f| f()).unwrap_or_default();
                    full = Some(update_of(&jobs, &tick.overall));
                }
                full.clone().unwrap_or_else(|| delta.clone())
            } else {
                delta.clone()
            };
            subscriber.channel.send(update).is_ok()
        });
    }
}

/// Job services held in Tauri state.
pub struct JobsState {
    pub manager: JobManager,
    pub hub: Arc<ProgressHub>,
}

impl JobsState {
    pub fn new(manager: JobManager) -> Self {
        Self {
            manager,
            hub: Arc::new(ProgressHub::default()),
        }
    }

    /// The job manager of the app services, which keeps its jobs in the library. When the
    /// services could not start, jobs run in memory only.
    pub fn from_boot(boot: &Boot) -> Self {
        Self::new(boot.services().map_or_else(
            || JobManager::new(Lanes::new(LaneConfig::default()), ManagerConfig::default()),
            |services| services.jobs().clone(),
        ))
    }

    /// Runs forever: polls the manager every [`PROGRESS_INTERVAL`], sends changes to the UI and
    /// moves the taskbar/dock progress when the combined figure changes.
    pub async fn run_feed(&self, taskbar: Arc<dyn Taskbar>) {
        self.run_feed_every(PROGRESS_INTERVAL, taskbar).await;
    }

    async fn run_feed_every(&self, interval: Duration, taskbar: Arc<dyn Taskbar>) {
        let manager = self.manager.clone();
        let hub = Arc::clone(&self.hub);
        self.manager
            .progress_feed(interval)
            .run(move |tick| {
                hub.publish(&tick, || manager.list());
                if tick.overall_changed {
                    taskbar.show(taskbar_state(&tick.overall));
                }
            })
            .await;
    }
}

/// Opens the progress stream: the first message holds every job, later ones only what changed,
/// at most 10 per second per job.
#[tauri::command]
#[specta::specta]
pub fn subscribe_job_progress(
    state: State<'_, JobsState>,
    on_progress: Channel<JobProgressUpdate>,
) {
    let jobs = state.manager.list();
    let overall = OverallTracker::default().update(&jobs);
    state.hub.subscribe(on_progress, update_of(&jobs, &overall));
}

/// Cancels one job. Returns false when it is unknown or already over.
#[tauri::command]
#[specta::specta]
pub fn cancel_job(state: State<'_, JobsState>, id: u32) -> bool {
    state.manager.cancel(JobId(u64::from(id)))
}

#[cfg(any(debug_assertions, test))]
struct SampleItem(u32);

#[cfg(any(debug_assertions, test))]
impl std::fmt::Display for SampleItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sample item {}", self.0 + 1)
    }
}

#[cfg(any(debug_assertions, test))]
struct SampleWork {
    delay: Duration,
}

#[cfg(any(debug_assertions, test))]
impl ItemWork for SampleWork {
    type Item = SampleItem;

    async fn process(&self, _item: &SampleItem, control: &ItemControl) -> Result<(), ItemError> {
        tokio::select! {
            () = tokio::time::sleep(self.delay) => Ok(()),
            () = control.cancelled() => Err(ItemError::Cancelled),
        }
    }
}

#[cfg(any(debug_assertions, test))]
/// Largest sample job, so a stray call cannot queue unbounded work.
const MAX_SAMPLE_ITEMS: u32 = 1000;
#[cfg(debug_assertions)]
const MAX_SAMPLE_ITEM_MILLIS: u32 = 5000;

/// Starts a job of `items` fake items that each take `item_millis`, to watch progress reach the
/// UI, the taskbar and the dock. Returns the job id. Debug builds only (see `ipc`).
#[cfg(debug_assertions)]
#[tauri::command]
#[specta::specta]
pub async fn start_sample_job(
    state: State<'_, JobsState>,
    items: u32,
    item_millis: u32,
) -> Result<u32, String> {
    let items = items.clamp(1, MAX_SAMPLE_ITEMS);
    let delay = Duration::from_millis(u64::from(item_millis.min(MAX_SAMPLE_ITEM_MILLIS)));
    let handle = state.manager.submit(
        JobSpec::new(format!("Sample job ({items} items)"), LaneId::Cpu),
        SampleWork { delay },
        (0..items).map(SampleItem).collect(),
    );
    Ok(small(handle.id().0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_jobs::{LaneConfig, Lanes, ManagerConfig};
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::ipc::InvokeResponseBody;
    use tokio::time::{Instant, sleep};

    #[derive(Default)]
    struct Fake {
        shown: Mutex<Vec<(Instant, ProgressBarState)>>,
    }

    impl Taskbar for Fake {
        fn show(&self, state: ProgressBarState) {
            self.shown.lock().unwrap().push((Instant::now(), state));
        }
    }

    type Received = Arc<Mutex<Vec<(Instant, serde_json::Value)>>>;

    /// A channel that records every message as JSON, as the UI would receive it.
    fn recording_channel() -> (Channel<JobProgressUpdate>, Received) {
        let received: Received = Arc::default();
        let sink = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            let InvokeResponseBody::Json(json) = body else {
                panic!("progress messages are JSON");
            };
            sink.lock()
                .unwrap()
                .push((Instant::now(), serde_json::from_str(&json).unwrap()));
            Ok(())
        });
        (channel, received)
    }

    fn state() -> JobsState {
        JobsState::new(JobManager::new(
            Lanes::new(LaneConfig::default()),
            ManagerConfig::default(),
        ))
    }

    fn start(state: &JobsState, items: u32, item_ms: u64) -> cv_jobs::JobHandle {
        state.manager.submit(
            JobSpec::new("sample", LaneId::Cpu),
            SampleWork {
                delay: Duration::from_millis(item_ms),
            },
            (0..items).map(SampleItem).collect(),
        )
    }

    fn overall(kind: OverallKind, fraction: f64) -> Overall {
        Overall {
            kind,
            fraction,
            active_jobs: 1,
        }
    }

    #[test]
    fn taskbar_shows_nothing_when_idle() {
        let state = taskbar_state(&Overall::IDLE);
        assert!(matches!(state.status, Some(ProgressBarStatus::None)));
        assert_eq!(state.progress, None);
    }

    #[test]
    fn taskbar_shows_the_percentage_while_running() {
        let state = taskbar_state(&overall(OverallKind::Normal, 0.4));
        assert!(matches!(state.status, Some(ProgressBarStatus::Normal)));
        assert_eq!(state.progress, Some(40));
    }

    #[test]
    fn taskbar_uses_the_paused_style_when_everything_is_paused() {
        let state = taskbar_state(&overall(OverallKind::Paused, 0.5));
        assert!(matches!(state.status, Some(ProgressBarStatus::Paused)));
        assert_eq!(state.progress, Some(50));
    }

    #[test]
    fn taskbar_is_indeterminate_until_the_size_is_known() {
        let state = taskbar_state(&overall(OverallKind::Indeterminate, 0.0));
        assert!(matches!(
            state.status,
            Some(ProgressBarStatus::Indeterminate)
        ));
        assert_eq!(state.progress, None);
    }

    #[test]
    fn progress_messages_use_snake_case_field_and_state_names() {
        let (channel, received) = recording_channel();
        let hub = ProgressHub::default();
        let initial = JobProgressUpdate {
            jobs: Vec::new(),
            overall: (&Overall::IDLE).into(),
        };
        hub.subscribe(channel, initial);
        let messages = received.lock().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].1["overall"]["kind"], "idle");
        assert_eq!(messages[0].1["overall"]["active_jobs"], 0);
    }

    #[tokio::test(start_paused = true)]
    async fn channel_receives_job_progress_at_most_ten_times_a_second() {
        let state = Arc::new(state());
        let (channel, received) = recording_channel();
        state.hub.subscribe(
            channel,
            update_of(&[], &OverallTracker::default().update(&[])),
        );
        let taskbar = Arc::new(Fake::default());
        let feed = tokio::spawn({
            let state = Arc::clone(&state);
            let taskbar = Arc::clone(&taskbar);
            async move { state.run_feed(taskbar).await }
        });

        let job = start(&state, 1500, 1);
        job.wait().await;
        sleep(Duration::from_millis(300)).await;
        feed.abort();

        let messages = received.lock().unwrap();
        let about_job: Vec<_> = messages
            .iter()
            .filter(|(_, m)| {
                m["jobs"]
                    .as_array()
                    .is_some_and(|jobs| jobs.iter().any(|j| j["name"] == "sample"))
            })
            .collect();
        assert!(
            about_job.len() >= 10,
            "progress must arrive along the way, got {}",
            about_job.len()
        );
        assert!(
            about_job.len() <= 20,
            "1,500 changes in 1.5 s collapse to about 15 messages, got {}",
            about_job.len()
        );
        for pair in about_job.windows(2) {
            assert!(
                pair[1].0 - pair[0].0 >= PROGRESS_INTERVAL,
                "messages closer than 100 ms exceed 10 Hz"
            );
        }
        let last = about_job.last().unwrap().1["jobs"][0].clone();
        assert_eq!(last["state"], "completed");
        assert_eq!(last["done"], 1500);
        assert_eq!(last["total"], 1500);
    }

    #[tokio::test(start_paused = true)]
    async fn taskbar_follows_the_combined_progress_and_clears_at_the_end() {
        let state = Arc::new(state());
        let taskbar = Arc::new(Fake::default());
        let feed = tokio::spawn({
            let state = Arc::clone(&state);
            let taskbar = Arc::clone(&taskbar);
            async move { state.run_feed(taskbar).await }
        });

        let job = start(&state, 10, 100);
        sleep(Duration::from_millis(450)).await;
        state.manager.pause_all();
        sleep(Duration::from_millis(300)).await;
        state.manager.resume_all();
        job.wait().await;
        sleep(Duration::from_millis(300)).await;
        feed.abort();

        let shown = taskbar.shown.lock().unwrap();
        let steps: Vec<(Option<u64>, bool)> = shown
            .iter()
            .map(|(_, s)| {
                (
                    s.progress,
                    matches!(s.status, Some(ProgressBarStatus::Paused)),
                )
            })
            .collect();
        let percentages: Vec<u64> = steps.iter().filter_map(|(p, _)| *p).collect();
        assert!(
            percentages.windows(2).all(|w| w[0] <= w[1]),
            "the bar never moves back: {percentages:?}"
        );
        assert!(
            percentages.iter().any(|p| (30..=60).contains(p)),
            "mid-way progress is shown: {percentages:?}"
        );
        assert!(
            steps.iter().any(|(_, paused)| *paused),
            "the paused style is shown while paused"
        );
        assert!(
            matches!(
                shown.last().unwrap().1.status,
                Some(ProgressBarStatus::None)
            ),
            "the bar is cleared when the job is over"
        );
        // The taskbar changes only when the figure changes: far fewer updates than polls.
        assert!(shown.len() <= 25, "{} taskbar updates", shown.len());
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_subscriber_gets_every_job_first_and_then_changes() {
        let state = Arc::new(state());
        let taskbar = Arc::new(Fake::default());
        let slow = start(&state, 3, 1000);
        let feed = tokio::spawn({
            let state = Arc::clone(&state);
            let taskbar = Arc::clone(&taskbar);
            async move { state.run_feed(taskbar).await }
        });
        sleep(Duration::from_millis(1500)).await;

        let (channel, received) = recording_channel();
        let jobs = state.manager.list();
        let overall = OverallTracker::default().update(&jobs);
        state.hub.subscribe(channel, update_of(&jobs, &overall));
        assert_eq!(received.lock().unwrap().len(), 1);
        assert_eq!(received.lock().unwrap()[0].1["jobs"][0]["done"], 1);

        slow.wait().await;
        sleep(Duration::from_millis(300)).await;
        feed.abort();
        let messages = received.lock().unwrap();
        assert!(messages.len() >= 2);
        let last = &messages.last().unwrap().1["jobs"][0];
        assert_eq!(last["state"], "completed");
    }

    #[test]
    fn a_closed_channel_is_dropped_and_does_not_stop_the_others() {
        let hub = ProgressHub::default();
        let dead_flag = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&dead_flag);
        let dead: Channel<JobProgressUpdate> = Channel::new(move |_| {
            if flag.load(Ordering::SeqCst) {
                Err(tauri::Error::WebviewNotFound)
            } else {
                Ok(())
            }
        });
        let (alive, received) = recording_channel();
        let empty = || JobProgressUpdate {
            jobs: Vec::new(),
            overall: (&Overall::IDLE).into(),
        };
        hub.subscribe(dead, empty());
        hub.subscribe(alive, empty());
        assert_eq!(hub.len(), 2);

        dead_flag.store(true, Ordering::SeqCst);
        let tick = ProgressTick {
            jobs: Vec::new(),
            overall: Overall::IDLE,
            overall_changed: true,
        };
        hub.publish(&tick, Vec::new);
        assert_eq!(hub.len(), 1);
        // The initial message plus this report.
        assert_eq!(received.lock().unwrap().len(), 2);
    }
}

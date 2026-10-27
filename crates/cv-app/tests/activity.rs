//! What the Activity drawer reads: a throttled provider lane while a 429 is waited out, and
//! failures grouped by reason with retry. The provider is a local mock server.

mod support;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cv_app::activity::ActivityService;
use cv_jobs::{ItemControl, ItemError, ItemWork, JobSpec, LaneId, StageError};
use cv_providers::http::{HttpClient, HttpRequest, Pause, RetryPolicy};
use cv_providers::{CredentialResolver, MemoryUserCredentials};
use support::Lab;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// --- a mock provider: one scripted reply per path, the last one repeated -------------------------

type Script = Arc<Mutex<HashMap<String, Vec<(u16, String)>>>>;

async fn serve(script: Script) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let script = Arc::clone(&script);
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 16384];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .to_owned();
                let (status, body) = {
                    let mut script = script.lock().unwrap();
                    match script.get_mut(&path) {
                        Some(replies) if replies.len() > 1 => replies.remove(0),
                        Some(replies) if !replies.is_empty() => replies[0].clone(),
                        _ => (200, "[]".to_owned()),
                    }
                };
                let reply = format!(
                    "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    format!("http://{addr}")
}

fn script(entries: &[(&str, Vec<(u16, &str)>)]) -> Script {
    Arc::new(Mutex::new(
        entries
            .iter()
            .map(|(path, replies)| {
                (
                    (*path).to_owned(),
                    replies.iter().map(|(s, b)| (*s, (*b).to_owned())).collect(),
                )
            })
            .collect(),
    ))
}

/// Waits a short real time, so the lane stays visibly paused while the test looks at it.
struct ShortWait;

impl Pause for ShortWait {
    fn pause(&self, _duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(Duration::from_millis(10)))
    }
}

/// A provider HTTP client over a mock server, and the activity service.
struct Rig {
    lab: Lab,
    http: HttpClient,
    base: String,
    activity: ActivityService,
}

async fn rig(script: Script) -> Rig {
    let lab = Lab::new();
    let base = serve(script).await;
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    // Waits of 400 ms, 800 ms and 1.6 s: long enough for the test to see the lane throttled.
    let http = HttpClient::builder()
        .pause(Arc::new(ShortWait))
        .retry_policy(RetryPolicy {
            max_retries: 3,
            max_wait: Duration::from_secs(30),
            base_backoff: Duration::from_millis(400),
        })
        .build()
        .unwrap();
    let activity = ActivityService::new(lab.manager.clone(), Some(http.clone()), resolver);
    Rig {
        lab,
        http,
        base,
        activity,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_that_throttles_shows_its_lane_as_throttled_until_the_retries_end() {
    let rig = rig(script(&[("/lookup", vec![(429, "")])])).await;

    // A job on the provider's lane asks it, so the lane is one the drawer lists.
    let job = rig.lab.manager.submit(
        JobSpec::new("Lookup", LaneId::provider("trakt")),
        Lookup {
            http: rig.http.clone(),
            url: format!("{}/lookup", rig.base),
        },
        vec![Item(0)],
    );

    // While the 429 is being waited out the drawer shows the provider's lane as throttled.
    let seen_throttled = Arc::new(AtomicBool::new(false));
    for _ in 0..200 {
        let snapshot = rig.activity.snapshot();
        let lane = snapshot.lanes.iter().find(|l| l.id == "provider:trakt");
        if let Some(lane) = lane
            && lane.throttled_for.is_some()
        {
            seen_throttled.store(true, Ordering::SeqCst);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        seen_throttled.load(Ordering::SeqCst),
        "the lane was never shown as throttled"
    );
    assert!(rig.http.lane_status("trakt").throttled_for.is_some());

    // After the retries the job ends with its item failed.
    job.wait().await;
    assert_eq!(job.snapshot().failed, 1);
}

/// Asks one provider for one address.
struct Lookup {
    http: HttpClient,
    url: String,
}

impl ItemWork for Lookup {
    type Item = Item;

    async fn process(&self, _item: &Item, _control: &ItemControl) -> Result<(), ItemError> {
        self.http
            .send(HttpRequest::get("trakt", &self.url))
            .await
            .map(|_| ())
            .map_err(|error| ItemError::Failed(StageError::new(error.to_string())))
    }
}

// --- failures and retry -----------------------------------------------------------------------

struct Item(u32);

impl std::fmt::Display for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rating fetch {}", self.0)
    }
}

/// Fails the first three items with a rate limit on the first attempt; succeeds when retried.
struct Search {
    attempts: AtomicUsize,
}

impl ItemWork for Search {
    type Item = Item;

    async fn process(&self, item: &Item, _control: &ItemControl) -> Result<(), ItemError> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        if item.0 < 3 && attempt < 6 {
            Err(ItemError::Failed(StageError::new("rate limit")))
        } else {
            Ok(())
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failures_are_grouped_by_reason_and_retrying_lists_them_only_once() {
    let rig = rig(script(&[])).await;
    let job = rig.lab.manager.submit(
        JobSpec::new("Rating fetch", LaneId::provider("trakt")),
        Search {
            attempts: AtomicUsize::new(0),
        },
        (0..6).map(Item).collect(),
    );
    job.wait().await;

    let snapshot = rig.activity.snapshot();
    assert_eq!(snapshot.failures.len(), 1);
    let group = &snapshot.failures[0];
    assert_eq!(group.reason, "rate limit");
    assert_eq!(group.count, 3);
    assert_eq!(group.job_name, "Rating fetch");
    assert_eq!(group.samples.len(), 3);
    // The job runs on a provider lane, so the drawer can link to that provider's card.
    assert_eq!(group.lane, "provider:trakt");

    let retry = rig
        .activity
        .retry_failed(job.id())
        .expect("failed items to retry");
    rig.lab.manager.get(retry).unwrap().wait().await;
    assert!(rig.activity.snapshot().failures.is_empty());
    // A job that did not fail has nothing to retry.
    assert!(rig.activity.retry_failed(retry).is_none());
}

/// Takes a while per item, so the job is still going when the test pauses it.
struct Slow;

impl ItemWork for Slow {
    type Item = Item;

    async fn process(&self, _item: &Item, _control: &ItemControl) -> Result<(), ItemError> {
        tokio::time::sleep(Duration::from_millis(40)).await;
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_job_can_be_paused_and_resumed_from_the_drawer() {
    let rig = rig(script(&[])).await;
    let job = rig.lab.manager.submit(
        JobSpec::new("Slow", LaneId::Cpu),
        Slow,
        (0..20).map(Item).collect(),
    );
    assert!(rig.activity.set_job_paused(job.id(), true));
    assert_eq!(job.snapshot().state, cv_jobs::JobState::Paused);
    assert!(rig.activity.set_job_paused(job.id(), false));
    job.wait().await;
    assert_eq!(job.snapshot().state, cv_jobs::JobState::Completed);
    // A finished job cannot be paused.
    assert!(!rig.activity.set_job_paused(job.id(), true));
}

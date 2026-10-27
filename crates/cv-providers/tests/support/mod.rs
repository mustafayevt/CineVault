//! A small scriptable HTTP/1.1 server for the HTTP client tests. It listens on a loopback port,
//! records every request and answers from a per-path script, so no test touches the network.

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What the server answers with.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub delay: Duration,
}

impl Reply {
    pub fn ok(body: &str) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.as_bytes().to_vec(),
            delay: Duration::ZERO,
        }
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            delay: Duration::ZERO,
        }
    }

    pub fn rate_limited(retry_after: Option<&str>) -> Self {
        let mut reply = Self::status(429);
        if let Some(value) = retry_after {
            reply.headers.push(("Retry-After".into(), value.into()));
        }
        reply
    }

    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

/// A request as the server saw it.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub target: String,
    /// Header names are lower-cased.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Recorded {
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }
}

#[derive(Default)]
struct State {
    scripts: Mutex<HashMap<String, VecDeque<Reply>>>,
    fallbacks: Mutex<HashMap<String, Reply>>,
    /// `(path, text the query must contain, reply)`, checked before scripts and routes.
    query_routes: Mutex<Vec<(String, String, Reply)>>,
    requests: Mutex<Vec<Recorded>>,
    active: AtomicUsize,
    peak: AtomicUsize,
}

pub struct MockServer {
    addr: SocketAddr,
    state: Arc<State>,
}

impl MockServer {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(State::default());
        let shared = Arc::clone(&state);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(handle(stream, Arc::clone(&shared)));
            }
        });
        Self { addr, state }
    }

    pub fn url(&self, path_and_query: &str) -> String {
        format!("http://{}{}", self.addr, path_and_query)
    }

    /// The reply for `path` once its script is used up (or when it has none).
    pub fn route(&self, path: &str, reply: Reply) {
        self.state
            .fallbacks
            .lock()
            .unwrap()
            .insert(path.to_owned(), reply);
    }

    /// The reply for requests to `path` whose target (path and query, as sent) contains
    /// `needle`, for APIs that serve everything from one path. The first matching route wins.
    pub fn route_query(&self, path: &str, needle: &str, reply: Reply) {
        self.state
            .query_routes
            .lock()
            .unwrap()
            .push((path.to_owned(), needle.to_owned(), reply));
    }

    /// Replies used one per request, in order, before the route's own reply.
    pub fn script(&self, path: &str, replies: Vec<Reply>) {
        self.state
            .scripts
            .lock()
            .unwrap()
            .insert(path.to_owned(), replies.into());
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.state.requests.lock().unwrap().clone()
    }

    pub fn count(&self, path: &str) -> usize {
        self.requests().iter().filter(|r| r.path() == path).count()
    }

    /// The most requests that were being answered at the same moment.
    pub fn peak_concurrency(&self) -> usize {
        self.state.peak.load(Ordering::SeqCst)
    }
}

async fn handle(mut stream: TcpStream, state: Arc<State>) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    let path = request.path().to_owned();
    let by_query = state
        .query_routes
        .lock()
        .unwrap()
        .iter()
        .find(|(route, needle, _)| *route == path && request.target.contains(needle.as_str()))
        .map(|(_, _, reply)| reply.clone());
    state.requests.lock().unwrap().push(request);

    let active = state.active.fetch_add(1, Ordering::SeqCst) + 1;
    state.peak.fetch_max(active, Ordering::SeqCst);

    let reply = by_query
        .or_else(|| {
            let scripted = state
                .scripts
                .lock()
                .unwrap()
                .get_mut(&path)
                .and_then(VecDeque::pop_front);
            scripted.or_else(|| state.fallbacks.lock().unwrap().get(&path).cloned())
        })
        .unwrap_or_else(|| Reply::status(404));

    if !reply.delay.is_zero() {
        tokio::time::sleep(reply.delay).await;
    }

    let mut head = format!(
        "HTTP/1.1 {} Mock\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.body.len()
    );
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(&reply.body).await;
    let _ = stream.shutdown().await;
    state.active.fetch_sub(1, Ordering::SeqCst);
}

async fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break position;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let wanted: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < wanted {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some(Recorded {
        method,
        target,
        headers,
        body,
    })
}

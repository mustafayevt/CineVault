//! A scriptable HTTP/1.1 server on a loopback port, for the source tests. It records every
//! request, answers from fixture files by path (and optionally by query), and can answer slowly,
//! with a rate-limit status, a server error, a challenge page, a body cut short or a dropped
//! connection. No test touches the network.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The folder fixtures are read from: `tests/fixtures`.
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// The bytes of a fixture file (`"yts/list_movies.json"`).
pub fn fixture(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

/// The text of a fixture file.
pub fn fixture_text(name: &str) -> String {
    String::from_utf8(fixture(name)).expect("fixture is UTF-8")
}

/// What the server answers with.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub delay: Duration,
    /// Declare this many bytes in `Content-Length` but send only `body`, then close.
    pub declared_length: Option<usize>,
    /// Close the connection without answering.
    pub drop_connection: bool,
}

impl Reply {
    fn base(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), content_type.into())],
            body,
            delay: Duration::ZERO,
            declared_length: None,
            drop_connection: false,
        }
    }

    pub fn json(body: &str) -> Self {
        Self::base(200, "application/json", body.as_bytes().to_vec())
    }

    pub fn xml(body: &str) -> Self {
        Self::base(200, "application/xml", body.as_bytes().to_vec())
    }

    pub fn text(body: &str) -> Self {
        Self::base(200, "text/plain", body.as_bytes().to_vec())
    }

    pub fn bytes(content_type: &str, body: Vec<u8>) -> Self {
        Self::base(200, content_type, body)
    }

    /// A JSON fixture file.
    pub fn json_fixture(name: &str) -> Self {
        Self::base(200, "application/json", fixture(name))
    }

    /// An XML fixture file.
    pub fn xml_fixture(name: &str) -> Self {
        Self::base(200, "application/xml", fixture(name))
    }

    pub fn status(status: u16) -> Self {
        Self::base(status, "text/plain", Vec::new())
    }

    pub fn too_many_requests() -> Self {
        let mut reply = Self::status(429);
        reply.headers.push(("Retry-After".into(), "1".into()));
        reply
    }

    pub fn server_error() -> Self {
        Self::base(500, "text/plain", b"internal error".to_vec())
    }

    /// The page a site behind a browser check answers with, status 200.
    pub fn challenge_page() -> Self {
        Self::base(
            200,
            "text/html; charset=utf-8",
            b"<!DOCTYPE html><html><head><title>Just a moment...</title></head><body>Checking your browser</body></html>".to_vec(),
        )
    }

    /// The same page with status 403, as a browser check sends it.
    pub fn challenge_403() -> Self {
        let mut reply = Self::challenge_page();
        reply.status = 403;
        reply
    }

    /// A body cut off after `keep` bytes while the headers promise all of it.
    pub fn truncated(mut self, keep: usize) -> Self {
        self.declared_length = Some(self.body.len());
        self.body.truncate(keep);
        self
    }

    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Closes the connection without a word.
    pub fn dropped() -> Self {
        let mut reply = Self::status(200);
        reply.drop_connection = true;
        reply
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// A request as the server saw it.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    /// Path and query, as sent.
    pub target: String,
    /// Header names are lower-cased.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Recorded {
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }

    /// The decoded value of a query parameter.
    pub fn query(&self, name: &str) -> Option<String> {
        let query = self.target.split_once('?')?.1;
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key) == name).then(|| decode(value))
        })
    }
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Route {
    method: Option<String>,
    path: String,
    query: Vec<(String, String)>,
    /// Replies not used yet, next one last.
    pending: Mutex<Vec<Reply>>,
    last: Mutex<Reply>,
}

#[derive(Default)]
struct State {
    routes: Mutex<Vec<Arc<Route>>>,
    requests: Mutex<Vec<Recorded>>,
    active: AtomicUsize,
    peak: AtomicUsize,
}

pub struct FakeServer {
    addr: SocketAddr,
    state: Arc<State>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeServer {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("address");
        let state = Arc::new(State::default());
        let shared = Arc::clone(&state);
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(handle(stream, Arc::clone(&shared)));
            }
        });
        Self { addr, state, task }
    }

    /// `http://127.0.0.1:<port>`, no trailing slash.
    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn url(&self, path_and_query: &str) -> String {
        format!("{}{}", self.base(), path_and_query)
    }

    fn add(&self, method: Option<&str>, path: &str, query: &[(&str, &str)], replies: Vec<Reply>) {
        let last = replies.last().cloned().expect("at least one reply");
        self.state.routes.lock().unwrap().push(Arc::new(Route {
            method: method.map(str::to_owned),
            path: path.to_owned(),
            query: query
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            pending: Mutex::new(replies.into_iter().rev().collect()),
            last: Mutex::new(last),
        }));
    }

    /// Answers every request to `path` (any method, any query).
    pub fn route(&self, path: &str, reply: Reply) {
        self.add(None, path, &[], vec![reply]);
    }

    /// Answers requests to `path` whose query holds all the given decoded pairs. A route with
    /// more pairs wins over one with fewer.
    pub fn route_query(&self, path: &str, query: &[(&str, &str)], reply: Reply) {
        self.add(None, path, query, vec![reply]);
    }

    /// Answers only the given method.
    pub fn route_method(&self, method: &str, path: &str, reply: Reply) {
        self.add(Some(method), path, &[], vec![reply]);
    }

    /// Answers `path` with each reply in turn, repeating the last.
    pub fn script(&self, path: &str, replies: Vec<Reply>) {
        self.add(None, path, &[], replies);
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.state.requests.lock().unwrap().clone()
    }

    /// How many requests reached `path`.
    pub fn count(&self, path: &str) -> usize {
        self.requests().iter().filter(|r| r.path() == path).count()
    }

    /// The most requests that were being answered at once.
    pub fn peak_concurrency(&self) -> usize {
        self.state.peak.load(Ordering::SeqCst)
    }
}

fn pick(routes: &[Arc<Route>], request: &Recorded) -> Option<Reply> {
    let mut best: Option<(&Arc<Route>, usize)> = None;
    for route in routes {
        if route.path != request.path() {
            continue;
        }
        if route
            .method
            .as_deref()
            .is_some_and(|m| !m.eq_ignore_ascii_case(&request.method))
        {
            continue;
        }
        if !route
            .query
            .iter()
            .all(|(k, v)| request.query(k).as_deref() == Some(v.as_str()))
        {
            continue;
        }
        let score = route.query.len() + usize::from(route.method.is_some());
        if best.is_none_or(|(_, s)| score > s) {
            best = Some((route, score));
        }
    }
    let (route, _) = best?;
    let next = route.pending.lock().unwrap().pop();
    match next {
        Some(reply) => {
            *route.last.lock().unwrap() = reply.clone();
            Some(reply)
        }
        None => Some(route.last.lock().unwrap().clone()),
    }
}

async fn handle(mut stream: TcpStream, state: Arc<State>) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    state.requests.lock().unwrap().push(request.clone());
    let active = state.active.fetch_add(1, Ordering::SeqCst) + 1;
    state.peak.fetch_max(active, Ordering::SeqCst);

    let reply = {
        let routes = state.routes.lock().unwrap().clone();
        pick(&routes, &request)
    }
    .unwrap_or_else(|| Reply::status(404));

    if !reply.delay.is_zero() {
        tokio::time::sleep(reply.delay).await;
    }
    if !reply.drop_connection {
        let length = reply.declared_length.unwrap_or(reply.body.len());
        let mut head = format!(
            "HTTP/1.1 {} Fake\r\nContent-Length: {length}\r\nConnection: close\r\n",
            reply.status
        );
        for (name, value) in &reply.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        let _ = stream.write_all(head.as_bytes()).await;
        let _ = stream.write_all(&reply.body).await;
        let _ = stream.flush().await;
    }
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

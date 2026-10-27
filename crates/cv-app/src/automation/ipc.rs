//! The local channel between the `cinevault` command line and a running app.
//!
//! The library has one writer. When the app is open, a command-line batch must not open the
//! library a second time; it asks the app to run the batch instead:
//!
//! 1. The app calls [`IpcServer::start`]. It listens on a per-user **named pipe** (Windows) or
//!    **Unix socket** (macOS), and writes an *endpoint file* (`automation-endpoint.json` in the
//!    data folder) holding the address and a random token.
//! 2. The command line reads the endpoint file ([`forward`]). No file, or nobody listening at the
//!    address (the app crashed and left the file), means no app is running, and the command line
//!    runs the batch itself.
//! 3. It sends one JSON line: the token and the [`BatchRequest`]. The app checks the token (a
//!    process that cannot read the user's data folder cannot guess it), runs the batch on its own
//!    services and answers with JSON lines: [`Progress`] messages, then the [`BatchReport`].
//!
//! The pipe is closed to remote clients, the socket file is private to the user, and the token
//! file lives in the user's data folder. A request that arrives without the right token is
//! refused before it is parsed any further.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::engine::{BatchEngine, Progress};
use super::model::{BatchReport, BatchRequest};
use crate::data_location::DataLocation;

/// The version of the line protocol. A different version is refused with a clear message.
pub const PROTOCOL: u32 = 1;

/// The endpoint file's name in the data folder.
pub const ENDPOINT_FILE: &str = "automation-endpoint.json";

/// The largest request line the server reads.
const MAX_REQUEST_BYTES: u64 = 1 << 20;

/// How long a client has to send its request after connecting.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Where a running app listens, and the token that opens it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub protocol: u32,
    /// The pipe name or the socket path.
    pub address: String,
    /// Proves the caller can read the user's data folder.
    pub token: String,
    /// The app's process id, for messages.
    pub pid: u32,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("address", &self.address)
            .field("pid", &self.pid)
            .finish_non_exhaustive()
    }
}

/// The endpoint file of a data location.
#[must_use]
pub fn endpoint_file(location: &DataLocation) -> PathBuf {
    location.root().join(ENDPOINT_FILE)
}

/// What the client sends: one line.
#[derive(Serialize, Deserialize)]
struct ClientMessage {
    token: String,
    request: BatchRequest,
}

/// What the server sends: one line each.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Progress { progress: Progress },
    Report { report: Box<BatchReport> },
    Error { message: String },
}

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
type BoxStream = Box<dyn Stream>;

fn random_token() -> io::Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0_u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

// ---- the server ----------------------------------------------------------------------------

/// A running listener. Dropping it stops listening and removes the endpoint file.
pub struct IpcServer {
    task: JoinHandle<()>,
    file: PathBuf,
    endpoint: Endpoint,
}

impl IpcServer {
    /// Starts listening and publishes the endpoint file. Must be called inside a tokio runtime.
    ///
    /// # Errors
    /// The pipe or socket cannot be created, or the endpoint file cannot be written. The app
    /// goes on without the command-line channel then.
    pub fn start(engine: Arc<BatchEngine>, location: &DataLocation) -> io::Result<Self> {
        let token = random_token()?;
        let file = endpoint_file(location);
        let (listener, address) = Listener::bind(location)?;
        let endpoint = Endpoint {
            protocol: PROTOCOL,
            address,
            token,
            pid: std::process::id(),
        };
        write_endpoint(&file, &endpoint)?;
        let secret = Arc::new(endpoint.token.clone());
        let task = tokio::spawn(async move {
            let mut listener = listener;
            loop {
                match listener.accept().await {
                    Ok(stream) => {
                        let engine = Arc::clone(&engine);
                        let secret = Arc::clone(&secret);
                        tokio::spawn(async move {
                            if let Err(error) = serve_connection(stream, &engine, &secret).await {
                                tracing::debug!(%error, "command-line connection ended early");
                            }
                        });
                    }
                    Err(error) => {
                        tracing::warn!(%error, "the command-line channel stopped");
                        break;
                    }
                }
            }
        });
        tracing::info!(address = %endpoint.address, "command-line channel is open");
        Ok(Self {
            task,
            file,
            endpoint,
        })
    }

    /// Where the server listens.
    #[must_use]
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Stops listening and removes the endpoint file (when it is still this server's).
    pub fn stop(&self) {
        self.task.abort();
        if let Some(current) = read_endpoint(&self.file)
            && current.token == self.endpoint.token
        {
            let _ = std::fs::remove_file(&self.file);
        }
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.endpoint.address);
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn write_endpoint(path: &Path, endpoint: &Endpoint) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_vec_pretty(endpoint).map_err(io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&temporary, path)
}

/// Reads an endpoint file; `None` when it is missing or damaged.
#[must_use]
pub fn read_endpoint(path: &Path) -> Option<Endpoint> {
    let text = std::fs::read(path).ok()?;
    serde_json::from_slice(&text).ok()
}

async fn serve_connection(
    stream: BoxStream,
    engine: &Arc<BatchEngine>,
    secret: &str,
) -> io::Result<()> {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines =
        BufReader::new(tokio::io::AsyncReadExt::take(reader, MAX_REQUEST_BYTES)).lines();
    let line = match tokio::time::timeout(REQUEST_TIMEOUT, lines.next_line()).await {
        Ok(Ok(Some(line))) => line,
        _ => return Ok(()),
    };
    // The token is checked before anything else of the request is looked at.
    let request = match serde_json::from_str::<ClientMessage>(&line) {
        Ok(message) if same_token(&message.token, secret) => message.request,
        Ok(_) => return send(&mut writer, &error("This request is not allowed.")).await,
        Err(error_value) => {
            let token_ok = serde_json::from_str::<serde_json::Value>(&line)
                .ok()
                .and_then(|v| v.get("token").and_then(|t| t.as_str().map(str::to_owned)))
                .is_some_and(|t| same_token(&t, secret));
            let message = if token_ok {
                format!("The request could not be read: {error_value}")
            } else {
                "This request is not allowed.".to_owned()
            };
            return send(&mut writer, &error(&message)).await;
        }
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();
    let forward_progress = tx.clone();
    let sink = move |progress: Progress| {
        let _ = forward_progress.send(ServerMessage::Progress { progress });
    };
    let pump = tokio::spawn(async move {
        let mut open = true;
        while let Some(message) = rx.recv().await {
            // A client that went away must not stop the batch: keep draining.
            if open && send(&mut writer, &message).await.is_err() {
                open = false;
            }
        }
        let _ = writer.shutdown().await;
    });
    let report = engine.run(&request, &sink).await;
    let _ = tx.send(ServerMessage::Report {
        report: Box::new(report),
    });
    drop(tx);
    let _ = pump.await;
    Ok(())
}

fn error(message: &str) -> ServerMessage {
    ServerMessage::Error {
        message: message.to_owned(),
    }
}

async fn send<W: AsyncWrite + Unpin>(writer: &mut W, message: &ServerMessage) -> io::Result<()> {
    let mut line = serde_json::to_vec(message).map_err(io::Error::other)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    writer.flush().await
}

// ---- the listener, per platform ------------------------------------------------------------

#[cfg(windows)]
struct Listener {
    address: String,
    next: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
}

#[cfg(windows)]
impl Listener {
    fn bind(_location: &DataLocation) -> io::Result<(Self, String)> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let address = format!(r"\\.\pipe\cinevault-cli-{}", &random_token()?[..24]);
        let first = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&address)?;
        Ok((
            Self {
                address: address.clone(),
                next: Some(first),
            },
            address,
        ))
    }

    async fn accept(&mut self) -> io::Result<BoxStream> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let server = self
            .next
            .take()
            .ok_or_else(|| io::Error::other("no pipe instance"))?;
        server.connect().await?;
        self.next = Some(
            ServerOptions::new()
                .reject_remote_clients(true)
                .create(&self.address)?,
        );
        Ok(Box::new(server))
    }
}

#[cfg(unix)]
struct Listener {
    inner: tokio::net::UnixListener,
}

#[cfg(unix)]
impl Listener {
    fn bind(location: &DataLocation) -> io::Result<(Self, String)> {
        use std::os::unix::fs::PermissionsExt;
        let mut path = location.root().join("cli.sock");
        // A socket path is limited to about 100 bytes.
        if path.as_os_str().len() > 100 {
            path =
                std::env::temp_dir().join(format!("cinevault-cli-{}.sock", &random_token()?[..16]));
        }
        let _ = std::fs::remove_file(&path);
        let inner = tokio::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok((Self { inner }, path.to_string_lossy().into_owned()))
    }

    async fn accept(&mut self) -> io::Result<BoxStream> {
        let (stream, _) = self.inner.accept().await?;
        Ok(Box::new(stream))
    }
}

// ---- the client ----------------------------------------------------------------------------

/// Why a batch could not be handed to a running app.
#[derive(Debug, thiserror::Error)]
pub enum ForwardError {
    /// No app is listening. The caller runs the batch itself.
    #[error("no running CineVault app was found")]
    NoRunningApp,
    /// An app answered but will not run the batch.
    #[error("the running CineVault app refused the batch: {0}")]
    Refused(String),
    /// The conversation broke off; the batch may or may not have started.
    #[error("the connection to the running CineVault app failed: {0}")]
    Connection(String),
}

async fn connect(address: &str) -> io::Result<BoxStream> {
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        // ERROR_PIPE_BUSY: every instance is serving someone; the server makes a new one at once.
        const PIPE_BUSY: i32 = 231;
        let mut tries = 0;
        loop {
            match ClientOptions::new().open(address) {
                Ok(client) => return Ok(Box::new(client)),
                Err(error) if error.raw_os_error() == Some(PIPE_BUSY) && tries < 40 => {
                    tries += 1;
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
    #[cfg(unix)]
    {
        Ok(Box::new(tokio::net::UnixStream::connect(address).await?))
    }
}

/// Hands `request` to the app that wrote `endpoint`, passing its progress on, and returns its
/// report (marked `forwarded`).
///
/// # Errors
/// [`ForwardError::NoRunningApp`] when nobody listens at the address; the other variants when
/// an app is there but the batch could not be completed through it.
pub async fn forward_to(
    endpoint: &Endpoint,
    request: &BatchRequest,
    mut on_progress: impl FnMut(Progress),
) -> Result<BatchReport, ForwardError> {
    if endpoint.protocol != PROTOCOL {
        return Err(ForwardError::Refused(format!(
            "it speaks protocol {} and this command line speaks {PROTOCOL}; update both to the \
             same version",
            endpoint.protocol
        )));
    }
    let stream = match connect(&endpoint.address).await {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(ForwardError::NoRunningApp);
        }
        Err(error) => return Err(ForwardError::Connection(error.to_string())),
    };
    let (reader, mut writer) = tokio::io::split(stream);
    let message = ClientMessage {
        token: endpoint.token.clone(),
        request: request.clone(),
    };
    let mut line =
        serde_json::to_vec(&message).map_err(|e| ForwardError::Connection(e.to_string()))?;
    line.push(b'\n');
    writer
        .write_all(&line)
        .await
        .map_err(|e| ForwardError::Connection(e.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|e| ForwardError::Connection(e.to_string()))?;

    let mut lines = BufReader::new(reader).lines();
    loop {
        let next = lines
            .next_line()
            .await
            .map_err(|e| ForwardError::Connection(e.to_string()))?;
        let Some(line) = next else {
            return Err(ForwardError::Connection(
                "the app closed the connection before it answered".to_owned(),
            ));
        };
        match serde_json::from_str::<ServerMessage>(&line) {
            Ok(ServerMessage::Progress { progress }) => on_progress(progress),
            Ok(ServerMessage::Report { mut report }) => {
                report.forwarded = true;
                return Ok(*report);
            }
            Ok(ServerMessage::Error { message }) => return Err(ForwardError::Refused(message)),
            Err(error) => return Err(ForwardError::Connection(format!("bad answer: {error}"))),
        }
    }
}

/// Hands `request` to the app running with this data location, if one is.
///
/// # Errors
/// As [`forward_to`]; [`ForwardError::NoRunningApp`] also when there is no endpoint file.
pub async fn forward(
    location: &DataLocation,
    request: &BatchRequest,
    on_progress: impl FnMut(Progress),
) -> Result<BatchReport, ForwardError> {
    let Some(endpoint) = read_endpoint(&endpoint_file(location)) else {
        return Err(ForwardError::NoRunningApp);
    };
    forward_to(&endpoint, request, on_progress).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_hex_and_compared_whole() {
        let a = random_token().unwrap();
        let b = random_token().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert!(same_token(&a, &a.clone()));
        assert!(!same_token(&a, &b));
        assert!(!same_token(&a, &a[..63]));
    }

    #[test]
    fn the_endpoint_file_round_trips_and_a_damaged_one_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ENDPOINT_FILE);
        assert!(read_endpoint(&path).is_none());
        let endpoint = Endpoint {
            protocol: PROTOCOL,
            address: "x".into(),
            token: "t".into(),
            pid: 7,
        };
        write_endpoint(&path, &endpoint).unwrap();
        assert_eq!(read_endpoint(&path), Some(endpoint));
        std::fs::write(&path, "{not json").unwrap();
        assert!(read_endpoint(&path).is_none());
    }

    #[test]
    fn an_endpoint_never_prints_its_token() {
        let endpoint = Endpoint {
            protocol: PROTOCOL,
            address: "pipe".into(),
            token: "secret-token-value".into(),
            pid: 1,
        };
        assert!(!format!("{endpoint:?}").contains("secret-token-value"));
    }
}

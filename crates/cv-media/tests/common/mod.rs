//! Helpers shared by the tool-manager tests: a local HTTP server that serves fixture files, so
//! no test touches the network, and builders for the archive formats the manifest supports.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use sha2::{Digest, Sha256};

/// What the server answers for one path.
#[derive(Clone)]
pub enum Reply {
    Body(Vec<u8>),
    Status(u16),
}

/// A throwaway HTTP/1.1 server on the loopback interface.
pub struct TestServer {
    addr: SocketAddr,
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    hits: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TestServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).expect("non-blocking");
        let addr = listener.local_addr().expect("address");
        let routes: Arc<Mutex<HashMap<String, Reply>>> = Arc::default();
        let hits: Arc<Mutex<Vec<String>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (routes, hits, stop) = (routes.clone(), hits.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let (routes, hits) = (routes.clone(), hits.clone());
                            std::thread::spawn(move || serve(stream, &routes, &hits));
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(5)),
                    }
                }
            })
        };
        Self {
            addr,
            routes,
            hits,
            stop,
            thread: Some(thread),
        }
    }

    /// Serves `reply` at `path` (which starts with `/`) and returns the full URL.
    pub fn route(&self, path: &str, reply: Reply) -> String {
        self.routes
            .lock()
            .expect("routes")
            .insert(path.to_owned(), reply);
        self.url(path)
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// The paths requested so far, in order.
    pub fn hits(&self) -> Vec<String> {
        self.hits.lock().expect("hits").clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(mut stream: TcpStream, routes: &Mutex<HashMap<String, Reply>>, hits: &Mutex<Vec<String>>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(n) => request.extend_from_slice(&buffer[..n]),
        }
    }
    let text = String::from_utf8_lossy(&request);
    let path = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_owned();
    hits.lock().expect("hits").push(path.clone());
    let reply = routes.lock().expect("routes").get(&path).cloned();
    let (status, body) = match reply {
        Some(Reply::Body(body)) => (200, body),
        Some(Reply::Status(code)) => (code, Vec::new()),
        None => (404, Vec::new()),
    };
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
        body.len()
    );
    // The client may hang up early (a cancelled or oversized download); that is not an error.
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

/// Lower-case hex SHA-256.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A zip holding `(path, contents)` entries.
pub fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (path, contents) in entries {
        writer.start_file(*path, options).expect("start file");
        writer.write_all(contents).expect("write entry");
    }
    writer.finish().expect("finish zip").into_inner()
}

/// A `.tar.bz2` holding `(path, contents)` regular files.
pub fn tar_bz2_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for (path, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, path, *contents)
            .expect("append");
    }
    builder
        .into_inner()
        .expect("finish tar")
        .finish()
        .expect("finish bzip2")
}

/// A `.tar.gz` holding `(path, contents)` regular files.
pub fn tar_gz_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for (path, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, path, *contents)
            .expect("append");
    }
    builder
        .into_inner()
        .expect("finish tar")
        .finish()
        .expect("finish gzip")
}

/// What kind of tar entry [`tar_gz_raw`] writes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TarKind {
    File,
    Dir,
    Symlink,
    Hardlink,
}

/// A `.tar.gz` whose entry names are written as given, even when a tar library would refuse them
/// (`..`, absolute paths). `(name, kind, contents-or-link-target, mode)`.
pub fn tar_gz_raw(entries: &[(&str, TarKind, &[u8], u32)]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    for (name, kind, data, mode) in entries {
        let mut header = tar::Header::new_gnu();
        {
            let gnu = header.as_gnu_mut().expect("gnu header");
            gnu.name[..name.len()].copy_from_slice(name.as_bytes());
        }
        header.set_mode(*mode);
        match kind {
            TarKind::File => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(data.len() as u64);
            }
            TarKind::Dir => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
            }
            TarKind::Symlink | TarKind::Hardlink => {
                header.set_entry_type(if *kind == TarKind::Symlink {
                    tar::EntryType::Symlink
                } else {
                    tar::EntryType::Link
                });
                header.set_size(0);
                header
                    .set_link_name(std::str::from_utf8(data).expect("utf-8 target"))
                    .expect("link name");
            }
        }
        header.set_cksum();
        let body: &[u8] = if *kind == TarKind::File { data } else { &[] };
        encoder.write_all(header.as_bytes()).expect("header");
        encoder.write_all(body).expect("body");
        let pad = (512 - body.len() % 512) % 512;
        encoder.write_all(&vec![0; pad]).expect("padding");
    }
    encoder.write_all(&[0; 1024]).expect("end of archive");
    encoder.finish().expect("finish gzip")
}

/// A zip with a symbolic link `link` pointing at `target`, next to the regular `files`.
pub fn zip_with_symlink(files: &[(&str, &[u8])], link: &str, target: &str) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (path, contents) in files {
        writer.start_file(*path, options).expect("start file");
        writer.write_all(contents).expect("write entry");
    }
    writer.add_symlink(link, target, options).expect("symlink");
    writer.finish().expect("finish zip").into_inner()
}

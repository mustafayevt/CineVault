//! A stand-in for the indexer service program, used by the tests of `indexer_service`.
//!
//! It takes the same command line the real program gets (`--NoUpdates`, `--PIDFile <file>` and, on
//! Windows, `-d <folder>`; elsewhere the folder is `$XDG_CONFIG_HOME`), writes its process ID into
//! the PID file, reads the port and API key from `ServerConfig.json` and answers
//! `GET /api/v2.0/indexers/all/results/torznab/api?t=caps&apikey=<key>` like the service does:
//! the capabilities page for the right key, an `<error code="100">` page (status 200) for any other.
//!
//! `--idle` only writes the PID file and sleeps (a leftover process). A file
//! `stub-behavior.txt` in the data folder changes how it behaves: `exit_immediately`,
//! `never_ready` or `delay_ms=<n>` before it starts answering.

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

fn main() {
    let mut pid_file: Option<PathBuf> = None;
    let mut data_dir: Option<PathBuf> = None;
    let mut idle = false;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--PIDFile" => pid_file = iter.next().map(PathBuf::from),
            "-d" => data_dir = iter.next().map(PathBuf::from),
            "--idle" => idle = true,
            _ => {}
        }
    }
    if let Some(file) = &pid_file {
        fs::write(file, std::process::id().to_string()).expect("write the PID file");
    }
    if idle {
        sleep_forever();
    }
    let data = data_dir
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .expect("a data folder (-d or XDG_CONFIG_HOME)");
    // What the program was given, for the tests to look at.
    let cwd = std::env::current_dir().unwrap_or_default();
    let report = format!(
        "args={}\nxdg={}\nhome={}\ncwd={}\n",
        args.join(" "),
        std::env::var("XDG_CONFIG_HOME").unwrap_or_default(),
        std::env::var("HOME").unwrap_or_default(),
        cwd.display()
    );
    let _ = fs::write(data.join("stub-args.txt"), report);

    let behavior = fs::read_to_string(data.join("stub-behavior.txt")).unwrap_or_default();
    match behavior.trim() {
        "exit_immediately" => std::process::exit(3),
        "never_ready" => sleep_forever(),
        other => {
            if let Some(ms) = other.strip_prefix("delay_ms=").and_then(|n| n.parse().ok()) {
                thread::sleep(Duration::from_millis(ms));
            }
        }
    }

    let (port, key) = read_config(&data);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("bind the port");
    for stream in listener.incoming().flatten() {
        let key = key.clone();
        thread::spawn(move || answer(stream, &key));
    }
}

fn sleep_forever() -> ! {
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn read_config(data: &Path) -> (u16, String) {
    let candidates = [
        data.join("ServerConfig.json"),
        data.join("Jackett").join("ServerConfig.json"),
    ];
    for path in candidates {
        if let Ok(text) = fs::read_to_string(path) {
            let value: serde_json::Value = serde_json::from_str(&text).expect("valid config");
            let port = value["Port"].as_u64().expect("Port") as u16;
            let key = value["APIKey"].as_str().expect("APIKey").to_owned();
            return (port, key);
        }
    }
    panic!("no ServerConfig.json below {}", data.display());
}

fn answer(mut stream: TcpStream, key: &str) {
    let mut buffer = [0_u8; 4096];
    let read = stream.read(&mut buffer).unwrap_or(0);
    let head = String::from_utf8_lossy(&buffer[..read]);
    let line = head.lines().next().unwrap_or_default();
    let target = line.split_whitespace().nth(1).unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let authorised = query.split('&').any(|pair| pair == format!("apikey={key}"));
    let (status, body) = if path != "/api/v2.0/indexers/all/results/torznab/api" {
        ("404 Not Found", "")
    } else if authorised {
        (
            "200 OK",
            "<?xml version=\"1.0\"?><caps><server title=\"Jackett\" /></caps>",
        )
    } else {
        (
            "200 OK",
            "<?xml version=\"1.0\"?><error code=\"100\" description=\"Invalid API Key\" />",
        )
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/rss+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes());
}

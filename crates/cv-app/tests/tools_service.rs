//! The app-level tools service against a local server: a corrupted download is refused with the
//! `checksum` code, a good one installs, and nothing touches the network.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;

use cv_app::app_settings::SettingsService;
use cv_app::settings::SettingsStore;
use cv_app::tools::ToolsService;
use cv_media::tools::{Manifest, Platform, ToolId, ToolState, ToolsManager};
use serde_json::json;
use sha2::{Digest, Sha256};

/// Serves `body` to every request on a loopback port and returns the URL.
fn serve(body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0_u8; 2048];
            let _ = stream.read(&mut buffer);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}/ffmpeg.exe")
}

fn service(data: &Path, url: &str, pinned: &[u8]) -> (SettingsService, ToolsService) {
    let hash: String = Sha256::digest(pinned)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let manifest = Manifest::parse(
        &json!({
            "schema": 1,
            "packages": { "ffmpeg": {
                "display": "ffmpeg", "license": "Unlicense", "homepage": "https://example.test",
                "platforms": { "windows-x86_64": { "version": "1.0", "artifacts": [{
                    "url": url, "sha256": hash, "size": pinned.len(), "format": "raw",
                    "files": [{ "provides": "ffmpeg", "from": "ffmpeg.exe", "to": "ffmpeg.exe" }]
                }]}}
            }}
        })
        .to_string(),
    )
    .expect("manifest");
    let settings = SettingsService::open(SettingsStore::new(data.join("settings.json")));
    let manager =
        ToolsManager::with_manifest(data.join("tools"), manifest, Some(Platform::WindowsX64));
    let service = ToolsService::with_manager(manager, &settings);
    (settings, service)
}

fn state_of(service: &ToolsService, tool: ToolId) -> ToolState {
    service
        .status()
        .into_iter()
        .find(|s| s.id == tool)
        .map(|s| s.state)
        .expect("row")
}

#[test]
fn a_corrupted_download_is_refused_with_a_checksum_code() {
    let data = tempfile::tempdir().expect("temp dir");
    let pinned = b"the real program".to_vec();
    let mut corrupted = pinned.clone();
    corrupted[3] ^= 1;
    let (_settings, service) = service(data.path(), &serve(corrupted), &pinned);

    let error = service
        .install(ToolId::Ffmpeg, &mut |_| {})
        .expect_err("must be rejected");
    assert_eq!(error.code(), "checksum");
    assert_eq!(state_of(&service, ToolId::Ffmpeg), ToolState::NotInstalled);
    assert!(!data.path().join("tools").join("ffmpeg").exists());
    // The service is free again afterwards.
    assert!(!service.cancel_install());
}

#[test]
fn a_good_download_installs_and_shows_its_version() {
    let data = tempfile::tempdir().expect("temp dir");
    let pinned = b"the real program".to_vec();
    let (_settings, service) = service(data.path(), &serve(pinned.clone()), &pinned);
    service
        .install(ToolId::Ffmpeg, &mut |_| {})
        .expect("install");
    let status = service
        .status()
        .into_iter()
        .find(|s| s.id == ToolId::Ffmpeg)
        .expect("row");
    assert_eq!(status.state, ToolState::Ready);
    assert_eq!(status.version.as_deref(), Some("1.0"));
}

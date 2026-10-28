//! Update checker tests. A tiny HTTP server on 127.0.0.1 serves a manifest signed with a key
//! pair generated in the test; no private key exists anywhere in the repository.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::*;
use crate::app_settings::SettingsService;
use crate::settings::SettingsStore;

const PLATFORM: &str = "test-platform";
const NOW: i64 = 1_800_000_000;
const DAY: i64 = 24 * 3600;

type Routes = Arc<Mutex<HashMap<String, (u16, Vec<u8>)>>>;

/// Serves fixed answers by path and counts the requests it got. A 3xx answer sends its body as
/// the `Location` header.
struct TestServer {
    base: String,
    routes: Routes,
    hits: Arc<AtomicUsize>,
}

impl TestServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!(
            "http://127.0.0.1:{}",
            listener.local_addr().expect("addr").port()
        );
        let routes: Routes = Arc::default();
        let hits = Arc::new(AtomicUsize::new(0));
        let (served, counter) = (Arc::clone(&routes), Arc::clone(&hits));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = [0_u8; 2048];
                let read = stream.read(&mut request).unwrap_or(0);
                let head = String::from_utf8_lossy(&request[..read]).into_owned();
                let path = head
                    .lines()
                    .next()
                    .and_then(|line| line.split(' ').nth(1))
                    .unwrap_or("/")
                    .to_owned();
                counter.fetch_add(1, Ordering::SeqCst);
                let (status, body) = served
                    .lock()
                    .expect("routes")
                    .get(&path)
                    .cloned()
                    .unwrap_or((404, b"not found".to_vec()));
                let header = if (300..400).contains(&status) {
                    format!(
                        "HTTP/1.1 {status} X\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        String::from_utf8_lossy(&body)
                    )
                } else {
                    format!(
                        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                };
                let body = if (300..400).contains(&status) {
                    Vec::new()
                } else {
                    body
                };
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        Self { base, routes, hits }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn serve(&self, path: &str, body: Vec<u8>) {
        self.routes
            .lock()
            .expect("routes")
            .insert(path.to_owned(), (200, body));
    }

    fn redirect(&self, path: &str, location: &str) {
        self.routes
            .lock()
            .expect("routes")
            .insert(path.to_owned(), (302, location.as_bytes().to_vec()));
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

fn signing_key() -> SigningKey {
    let mut seed = [0_u8; 32];
    getrandom::fill(&mut seed).expect("random bytes");
    SigningKey::from_bytes(&seed)
}

fn public_text(key: &SigningKey) -> String {
    STANDARD.encode(key.verifying_key().to_bytes())
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn envelope(key: &SigningKey, payload: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(payload).expect("payload");
    let signature = key.sign(&bytes);
    serde_json::to_vec(&json!({
        "payload": STANDARD.encode(&bytes),
        "signature": STANDARD.encode(signature.to_bytes()),
    }))
    .expect("envelope")
}

fn payload(version: &str, installer_url: &str, installer: &[u8]) -> Value {
    json!({
        "version": version,
        "pub_date": "2026-11-01T10:00:00Z",
        "notes": "Faster scans.\nA fix for renamed folders.",
        "platforms": {
            PLATFORM: {
                "url": installer_url,
                "sha256": sha_hex(installer),
                "size": installer.len(),
            }
        }
    })
}

struct Fixture {
    server: TestServer,
    key: SigningKey,
    settings: Arc<SettingsService>,
    service: UpdateService,
    _dir: tempfile::TempDir,
}

/// A service at version 1.0.0 whose channel and key are set to the test server. The built-in
/// channel and key are switched off, so no test ever reaches the real channel.
fn fixture(configure: bool) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let settings = Arc::new(SettingsService::open(SettingsStore::new(
        dir.path().join("settings.json"),
    )));
    let server = TestServer::start();
    let key = signing_key();
    if configure {
        settings
            .set(CHANNEL_KEY, json!(server.url("/manifest.json")))
            .expect("channel");
        settings
            .set(PUBLIC_KEY_KEY, json!(public_text(&key)))
            .expect("key");
    }
    let service = UpdateService::new(Arc::clone(&settings), "1.0.0", dir.path().join("updates"))
        .with_platform(PLATFORM)
        .with_default_channel(None)
        .with_built_in_key(None);
    Fixture {
        server,
        key,
        settings,
        service,
        _dir: dir,
    }
}

fn publish(f: &Fixture, version: &str, installer: &[u8]) {
    f.server.serve("/CineVault-setup.exe", installer.to_vec());
    let doc = payload(version, &f.server.url("/CineVault-setup.exe"), installer);
    f.server.serve("/manifest.json", envelope(&f.key, &doc));
}

fn offer_of(outcome: CheckOutcome) -> UpdateOffer {
    match outcome {
        CheckOutcome::Available(offer) => offer,
        other => panic!("expected an offer, got {other:?}"),
    }
}

#[test]
fn an_empty_channel_setting_means_the_built_in_channel() {
    let dir = tempfile::tempdir().expect("tempdir");
    let settings = Arc::new(SettingsService::open(SettingsStore::new(
        dir.path().join("settings.json"),
    )));
    let service = UpdateService::new(Arc::clone(&settings), "1.0.0", dir.path().join("updates"));
    // A fresh install: checks on, the built-in channel.
    let config = service.config();
    assert!(config.enabled);
    assert!(config.active());
    assert_eq!(config.channel.as_deref(), Some(BUILT_IN_CHANNEL));
    // An existing install with the key stored empty is not switched off by it.
    for empty in ["", "   "] {
        settings.set(CHANNEL_KEY, json!(empty)).expect("empty");
        assert_eq!(service.config().channel.as_deref(), Some(BUILT_IN_CHANNEL));
    }
    // An address in Settings still overrides it.
    settings
        .set(CHANNEL_KEY, json!("https://updates.example/manifest.json"))
        .expect("own");
    assert_eq!(
        service.config().channel.as_deref(),
        Some("https://updates.example/manifest.json")
    );
    assert!(BUILT_IN_CHANNEL.starts_with("https://"));
    let key = BUILT_IN_PUBLIC_KEY.expect("a built-in key");
    parse_public_key(key).expect("the built-in key is valid");
}

#[test]
fn the_built_in_channel_is_read_when_the_setting_is_empty() {
    let f = fixture(false);
    publish(&f, "1.2.0", b"installer");
    f.settings
        .set(PUBLIC_KEY_KEY, json!(public_text(&f.key)))
        .expect("key");
    f.settings.set(CHANNEL_KEY, json!("")).expect("empty");
    let dir = tempfile::tempdir().expect("tempdir");
    let service = UpdateService::new(Arc::clone(&f.settings), "1.0.0", dir.path().join("u"))
        .with_platform(PLATFORM)
        .with_default_channel(Some(&f.server.url("/manifest.json")));
    let offer = offer_of(service.check_if_due(NOW).expect("check"));
    assert_eq!(offer.version, "1.2.0");
    assert_eq!(f.server.hits(), 1);
}

/// A service on the built-in channel (setting empty) whose channels are the two test paths.
fn built_in_service(f: &Fixture, primary: &str, fallback: &str) -> UpdateService {
    f.settings.set(CHANNEL_KEY, json!("")).expect("empty");
    f.settings
        .set(PUBLIC_KEY_KEY, json!(public_text(&f.key)))
        .expect("key");
    let dir = tempfile::tempdir().expect("tempdir");
    UpdateService::new(Arc::clone(&f.settings), "1.0.0", dir.path().join("u"))
        .with_platform(PLATFORM)
        .with_default_channel(Some(&f.server.url(primary)))
        .with_fallback_channel(Some(&f.server.url(fallback)))
}

fn signed_offer(f: &Fixture, version: &str) -> Vec<u8> {
    let doc = payload(version, &f.server.url("/i.exe"), b"installer");
    envelope(&f.key, &doc)
}

#[test]
fn the_new_channel_is_read_first_and_the_old_one_is_left_alone() {
    assert!(BUILT_IN_CHANNEL.contains("/mustafayevt/CineVault/releases/"));
    assert!(FALLBACK_CHANNEL.contains("/mustafayevt/CineVault-releases/releases/"));
    let f = fixture(false);
    f.server.serve("/manifest.json", signed_offer(&f, "1.2.0"));
    f.server
        .serve("/old/manifest.json", signed_offer(&f, "1.3.0"));
    f.server.redirect("/new/manifest.json", "/manifest.json");
    let service = built_in_service(&f, "/new/manifest.json", "/old/manifest.json");
    let offer = offer_of(service.check_if_due(NOW).expect("check"));
    assert_eq!(offer.version, "1.2.0");
    // The redirect and the manifest: the fallback was never asked.
    assert_eq!(f.server.hits(), 2);
}

#[test]
fn a_missing_new_channel_falls_back_to_the_old_one() {
    let f = fixture(false);
    f.server
        .serve("/old/manifest.json", signed_offer(&f, "1.2.0"));
    // /new/manifest.json is not served: the server answers 404.
    let service = built_in_service(&f, "/new/manifest.json", "/old/manifest.json");
    let offer = offer_of(service.check_if_due(NOW).expect("check"));
    assert_eq!(offer.version, "1.2.0");
    assert_eq!(f.server.hits(), 2);
}

#[test]
fn an_unreachable_new_channel_falls_back_to_the_old_one() {
    let f = fixture(false);
    f.server
        .serve("/old/manifest.json", signed_offer(&f, "1.2.0"));
    let dead = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = dead.local_addr().expect("addr").port();
    drop(dead);
    f.settings.set(CHANNEL_KEY, json!("")).expect("empty");
    f.settings
        .set(PUBLIC_KEY_KEY, json!(public_text(&f.key)))
        .expect("key");
    let dir = tempfile::tempdir().expect("tempdir");
    let service = UpdateService::new(Arc::clone(&f.settings), "1.0.0", dir.path().join("u"))
        .with_platform(PLATFORM)
        .with_default_channel(Some(&format!("http://127.0.0.1:{port}/manifest.json")))
        .with_fallback_channel(Some(&f.server.url("/old/manifest.json")));
    let offer = offer_of(service.check_if_due(NOW).expect("check"));
    assert_eq!(offer.version, "1.2.0");
}

#[test]
fn a_bad_signature_on_the_new_channel_does_not_fall_back() {
    let f = fixture(false);
    let doc = payload("1.2.0", &f.server.url("/i.exe"), b"installer");
    f.server
        .serve("/new/manifest.json", envelope(&signing_key(), &doc));
    f.server
        .serve("/old/manifest.json", signed_offer(&f, "1.2.0"));
    let service = built_in_service(&f, "/new/manifest.json", "/old/manifest.json");
    let error = service.check_if_due(NOW).expect_err("refused");
    assert!(matches!(error, UpdateError::BadSignature), "{error:?}");
    assert_eq!(f.server.hits(), 1);
}

#[test]
fn both_channels_failing_reports_the_first_error() {
    let f = fixture(false);
    let service = built_in_service(&f, "/new/manifest.json", "/old/manifest.json");
    let error = service.check_if_due(NOW).expect_err("both missing");
    assert!(matches!(error, UpdateError::Network(_)), "{error:?}");
    assert_eq!(f.server.hits(), 2);
}

#[test]
fn a_channel_the_user_set_never_falls_back() {
    let f = fixture(true);
    f.server
        .serve("/old/manifest.json", signed_offer(&f, "1.2.0"));
    let service = UpdateService::new(Arc::clone(&f.settings), "1.0.0", f._dir.path().join("u"))
        .with_platform(PLATFORM)
        .with_fallback_channel(Some(&f.server.url("/old/manifest.json")));
    // The user's own address (/manifest.json) is not served.
    let error = service.check_if_due(NOW).expect_err("no fallback");
    assert!(matches!(error, UpdateError::Network(_)), "{error:?}");
    assert_eq!(f.server.hits(), 1);
}

#[test]
fn redirects_are_followed_to_the_signed_manifest_and_the_installer() {
    let f = fixture(true);
    let installer = vec![5_u8; 70_000];
    f.server
        .serve("/download/v1.2.0/setup-file.exe", installer.clone());
    // Like a release download: hops by path and by full address; the installer by a relative one.
    f.server
        .redirect("/download/v1.2.0/setup.exe", "setup-file.exe");
    let doc = payload(
        "1.2.0",
        &f.server.url("/download/v1.2.0/setup.exe"),
        &installer,
    );
    f.server
        .serve("/assets/manifest.json", envelope(&f.key, &doc));
    f.server.redirect(
        "/download/v1.2.0/manifest.json",
        &f.server.url("/assets/manifest.json"),
    );
    f.server
        .redirect("/latest/manifest.json", "/download/v1.2.0/manifest.json");
    f.settings
        .set(CHANNEL_KEY, json!(f.server.url("/latest/manifest.json")))
        .expect("channel");

    let offer = offer_of(f.service.check_now(NOW).expect("check"));
    assert_eq!(offer.version, "1.2.0");
    assert_eq!(f.server.hits(), 3);
    let path = confirmed_download(&f, "1.2.0").expect("download");
    assert_eq!(std::fs::read(&path).expect("file"), installer);
    assert_eq!(f.server.hits(), 5);
}

#[test]
fn a_redirect_to_plain_http_is_refused() {
    let f = fixture(true);
    f.server
        .redirect("/manifest.json", "http://updates.example.com/manifest.json");
    let error = f.service.check_now(NOW).expect_err("must refuse");
    assert!(matches!(error, UpdateError::InsecureRedirect(_)), "{error}");
    assert_eq!(error.code(), "insecure");
    assert!(
        error
            .to_string()
            .contains("http://updates.example.com/manifest.json"),
        "{error}"
    );
    assert_eq!(f.settings.get(LAST_CHECK_KEY), None);

    // The installer download is held to the same rule.
    publish(&f, "1.2.0", b"installer");
    offer_of(f.service.check_now(NOW).expect("check"));
    f.server.redirect(
        "/CineVault-setup.exe",
        "http://downloads.example.com/setup.exe",
    );
    assert!(matches!(
        confirmed_download(&f, "1.2.0"),
        Err(UpdateError::InsecureRedirect(_))
    ));
    assert!(walk(&f._dir.path().join("updates")).is_empty());
}

#[test]
fn a_redirect_loop_gives_up() {
    let f = fixture(true);
    f.server.redirect("/manifest.json", "/manifest.json");
    let error = f.service.check_now(NOW).expect_err("loop");
    assert!(matches!(error, UpdateError::Network(_)), "{error}");
    assert!(error.to_string().contains("too many"), "{error}");
}

#[test]
fn redirect_targets_resolve_against_the_answering_address() {
    let base = "https://github.com/o/r/releases/latest/download/manifest.json";
    assert_eq!(
        service::resolve_location(base, "https://cdn.example/x?sig=1"),
        "https://cdn.example/x?sig=1"
    );
    assert_eq!(
        service::resolve_location(base, "/o/r/releases/download/v1/manifest.json"),
        "https://github.com/o/r/releases/download/v1/manifest.json"
    );
    assert_eq!(
        service::resolve_location(base, "//cdn.example/y"),
        "https://cdn.example/y"
    );
    assert_eq!(
        service::resolve_location("http://127.0.0.1:9/a/b?q=1", "c"),
        "http://127.0.0.1:9/a/c"
    );
    assert_eq!(
        service::resolve_location(base, "/next?to=http://elsewhere/x"),
        "https://github.com/next?to=http://elsewhere/x"
    );
}

#[test]
fn off_until_a_channel_is_configured() {
    // Only a build without a built-in channel (the fixture) can have no channel at all.
    let f = fixture(false);
    // The toggle is on by default, yet without a channel nothing is requested.
    assert!(f.service.config().enabled);
    assert!(!f.service.config().active());
    assert_eq!(
        f.service.check_if_due(NOW).expect("check"),
        CheckOutcome::Disabled
    );
    assert_eq!(f.server.hits(), 0);
    // A manual check explains what is missing instead of guessing.
    assert!(matches!(
        f.service.check_now(NOW),
        Err(UpdateError::NoChannel)
    ));
    assert_eq!(f.server.hits(), 0);
}

#[test]
fn the_toggle_stops_automatic_checks_but_not_a_manual_one() {
    let f = fixture(true);
    publish(&f, "1.1.0", b"installer");
    f.settings.set(CHECK_KEY, json!(false)).expect("toggle");
    assert_eq!(
        f.service.check_if_due(NOW).expect("check"),
        CheckOutcome::Disabled
    );
    assert_eq!(f.server.hits(), 0);
    assert!(matches!(
        f.service.check_now(NOW).expect("manual"),
        CheckOutcome::Available(_)
    ));
}

#[test]
fn a_newer_signed_release_is_offered_with_its_notes() {
    let f = fixture(true);
    publish(&f, "1.2.0", b"installer bytes");
    let offer = offer_of(f.service.check_if_due(NOW).expect("check"));
    assert_eq!(offer.version, "1.2.0");
    assert_eq!(offer.current, "1.0.0");
    assert_eq!(offer.notes, "Faster scans.\nA fix for renamed folders.");
    assert_eq!(offer.published.as_deref(), Some("2026-11-01T10:00:00Z"));
    assert_eq!(offer.size_bytes, b"installer bytes".len() as u64);
    assert_eq!(f.service.offer(), Some(offer));
}

#[test]
fn the_same_or_an_older_release_is_not_an_update() {
    let f = fixture(true);
    publish(&f, "1.0.0", b"x");
    assert_eq!(
        f.service.check_now(NOW).expect("check"),
        CheckOutcome::UpToDate
    );
    publish(&f, "0.9.0", b"x");
    assert_eq!(
        f.service.check_now(NOW).expect("check"),
        CheckOutcome::UpToDate
    );
    assert_eq!(f.service.offer(), None);
}

#[test]
fn a_prerelease_is_older_than_its_release() {
    let f = fixture(true);
    publish(&f, "1.0.0-beta.1", b"x");
    assert_eq!(
        f.service.check_now(NOW).expect("check"),
        CheckOutcome::UpToDate
    );
}

#[test]
fn a_manifest_signed_by_another_key_is_refused_with_a_plain_message() {
    let f = fixture(true);
    let intruder = signing_key();
    let doc = payload("9.0.0", &f.server.url("/CineVault-setup.exe"), b"evil");
    f.server.serve("/manifest.json", envelope(&intruder, &doc));
    let error = f.service.check_now(NOW).expect_err("must refuse");
    assert!(matches!(error, UpdateError::BadSignature));
    assert_eq!(
        error.to_string(),
        "The update information is not signed by the expected key, so it was ignored. Nothing was installed."
    );
    assert_eq!(f.service.offer(), None);
    // The refused check does not count as done.
    assert_eq!(f.settings.get(LAST_CHECK_KEY), None);
}

#[test]
fn a_tampered_payload_is_refused() {
    let f = fixture(true);
    publish(&f, "1.2.0", b"installer");
    let genuine: Value =
        serde_json::from_slice(&f.server.routes.lock().expect("routes")["/manifest.json"].1)
            .expect("json");
    let mut doc = payload("1.2.0", &f.server.url("/CineVault-setup.exe"), b"installer");
    doc["notes"] = json!("Changed after signing");
    let forged = json!({
        "payload": STANDARD.encode(serde_json::to_vec(&doc).expect("doc")),
        "signature": genuine["signature"],
    });
    f.server.serve(
        "/manifest.json",
        serde_json::to_vec(&forged).expect("forged"),
    );
    assert!(matches!(
        f.service.check_now(NOW),
        Err(UpdateError::BadSignature)
    ));
}

#[test]
fn a_missing_or_garbled_signature_is_refused() {
    let f = fixture(true);
    let doc = serde_json::to_vec(&payload("2.0.0", "http://127.0.0.1:1/x", b"x")).expect("doc");
    for signature in [
        json!(""),
        json!("not base64 !!"),
        json!(STANDARD.encode([1_u8; 10])),
    ] {
        let body = json!({ "payload": STANDARD.encode(&doc), "signature": signature });
        f.server
            .serve("/manifest.json", serde_json::to_vec(&body).expect("body"));
        assert!(matches!(
            f.service.check_now(NOW),
            Err(UpdateError::BadSignature)
        ));
    }
    f.server
        .serve("/manifest.json", b"<html>login</html>".to_vec());
    assert!(matches!(
        f.service.check_now(NOW),
        Err(UpdateError::BadManifest(_))
    ));
}

#[test]
fn a_signed_payload_that_is_not_a_manifest_is_refused() {
    let f = fixture(true);
    f.server.serve(
        "/manifest.json",
        envelope(&f.key, &json!({ "version": "not-a-version" })),
    );
    assert!(matches!(
        f.service.check_now(NOW),
        Err(UpdateError::BadManifest(_))
    ));
}

#[test]
fn without_a_key_nothing_is_trusted() {
    let f = fixture(true);
    f.settings.reset(&[PUBLIC_KEY_KEY]).expect("reset");
    publish(&f, "1.2.0", b"x");
    assert!(matches!(f.service.check_now(NOW), Err(UpdateError::NoKey)));
    assert_eq!(f.server.hits(), 0);
    f.settings.set(PUBLIC_KEY_KEY, json!("AAAA")).expect("set");
    assert!(matches!(f.service.check_now(NOW), Err(UpdateError::BadKey)));
}

#[test]
fn a_release_without_a_build_for_this_platform_says_so() {
    let f = fixture(true);
    let doc = json!({
        "version": "1.5.0",
        "platforms": { "other-os": { "url": "http://127.0.0.1:1/x", "sha256": "00", "size": 1 } }
    });
    f.server.serve("/manifest.json", envelope(&f.key, &doc));
    let error = f.service.check_now(NOW).expect_err("no build");
    assert!(matches!(error, UpdateError::NoBuild(_)));
    assert_eq!(
        error.to_string(),
        "Version 1.5.0 has no build for this computer."
    );
}

#[test]
fn the_interval_decides_when_the_channel_is_read_again() {
    let f = fixture(true);
    publish(&f, "1.2.0", b"x");
    offer_of(f.service.check_if_due(NOW).expect("first"));
    assert_eq!(f.server.hits(), 1);
    // Less than a day later: not due, no request.
    assert_eq!(
        f.service.check_if_due(NOW + DAY - 1).expect("soon"),
        CheckOutcome::NotDue
    );
    assert_eq!(f.server.hits(), 1);
    offer_of(f.service.check_if_due(NOW + DAY).expect("daily"));
    assert_eq!(f.server.hits(), 2);

    f.settings
        .set(INTERVAL_KEY, json!("weekly"))
        .expect("weekly");
    assert_eq!(
        f.service.check_if_due(NOW + DAY + 6 * DAY).expect("weekly"),
        CheckOutcome::NotDue
    );
    offer_of(f.service.check_if_due(NOW + DAY + 7 * DAY).expect("due"));

    f.settings
        .set(INTERVAL_KEY, json!("monthly"))
        .expect("monthly");
    let last = NOW + DAY + 7 * DAY;
    assert_eq!(
        f.service.check_if_due(last + 29 * DAY).expect("monthly"),
        CheckOutcome::NotDue
    );
    // A clock that moved backwards counts as due rather than waiting for ever.
    offer_of(f.service.check_if_due(NOW - 10 * DAY).expect("clock"));
}

#[test]
fn a_failed_check_is_retried_on_the_next_start() {
    let f = fixture(true);
    // Nothing is served: the channel answers 404.
    let error = f.service.check_if_due(NOW).expect_err("404");
    assert!(matches!(error, UpdateError::Network(_)));
    assert!(error.to_string().contains("404"));
    assert_eq!(f.settings.get(LAST_CHECK_KEY), None);
    publish(&f, "1.2.0", b"x");
    offer_of(f.service.check_if_due(NOW).expect("retry"));
}

#[test]
fn a_channel_must_be_https_or_loopback() {
    let f = fixture(true);
    f.settings
        .set(
            CHANNEL_KEY,
            json!("http://updates.example.com/manifest.json"),
        )
        .expect("channel");
    assert!(matches!(
        f.service.check_now(NOW),
        Err(UpdateError::InsecureUrl(_))
    ));
}

#[test]
fn a_bad_address_is_reported_before_a_missing_key() {
    let f = fixture(true);
    f.settings.reset(&[PUBLIC_KEY_KEY]).expect("reset");
    for (channel, insecure) in [
        ("http://insecure.example/manifest.json", true),
        ("not a url", false),
        ("https://", false),
    ] {
        f.settings
            .set(CHANNEL_KEY, json!(channel))
            .expect("channel");
        let error = f.service.check_now(NOW).expect_err(channel);
        if insecure {
            assert!(
                matches!(error, UpdateError::InsecureUrl(_)),
                "{channel}: {error}"
            );
        } else {
            assert!(
                matches!(error, UpdateError::BadUrl(_)),
                "{channel}: {error}"
            );
            assert!(error.to_string().contains("not a web address"), "{error}");
        }
    }
    assert_eq!(f.server.hits(), 0);
}

#[test]
fn a_missing_key_says_this_version_has_none_built_in() {
    let f = fixture(true);
    f.settings.reset(&[PUBLIC_KEY_KEY]).expect("reset");
    let error = f.service.check_now(NOW).expect_err("no key");
    assert!(
        error.to_string().contains("no built-in update signing key"),
        "{error}"
    );
}

fn confirmed_download(f: &Fixture, version: &str) -> Result<std::path::PathBuf, UpdateError> {
    let cancel = AtomicBool::new(false);
    f.service
        .download(version, &cancel, &mut |_| {})
        .map(|installer| installer.path().to_path_buf())
}

#[test]
fn a_confirmed_install_downloads_and_verifies_the_installer() {
    let f = fixture(true);
    let installer = vec![7_u8; 200_000];
    publish(&f, "1.2.0", &installer);
    offer_of(f.service.check_now(NOW).expect("check"));

    let mut last = 0;
    let cancel = AtomicBool::new(false);
    let verified = f
        .service
        .download("1.2.0", &cancel, &mut |total| last = total)
        .expect("download");
    let path = verified.path().to_path_buf();
    assert_eq!(verified.version(), "1.2.0");
    assert!(path.starts_with(f.service.downloads()));
    drop(
        verified
            .reverify()
            .expect("still matches the signed manifest"),
    );
    assert_eq!(std::fs::read(&path).expect("file"), installer);
    assert_eq!(last, installer.len() as u64);
    assert_eq!(
        path.file_name().and_then(|n| n.to_str()),
        Some("CineVault-setup.exe")
    );
}

#[test]
fn nothing_downloads_without_an_offer_or_for_another_version() {
    let f = fixture(true);
    assert!(matches!(
        confirmed_download(&f, "1.2.0"),
        Err(UpdateError::NothingOffered)
    ));
    publish(&f, "1.2.0", b"x");
    offer_of(f.service.check_now(NOW).expect("check"));
    assert!(matches!(
        confirmed_download(&f, "9.9.9"),
        Err(UpdateError::OfferChanged)
    ));
    assert_eq!(f.server.hits(), 1, "no installer was requested");
}

#[test]
fn a_corrupted_installer_is_rejected_and_deleted() {
    let f = fixture(true);
    let installer = b"the real installer".to_vec();
    publish(&f, "1.2.0", &installer);
    offer_of(f.service.check_now(NOW).expect("check"));
    // Same length, different bytes: the signed checksum no longer matches.
    f.server
        .serve("/CineVault-setup.exe", b"the evil installer".to_vec());
    let error = confirmed_download(&f, "1.2.0").expect_err("checksum");
    assert!(matches!(error, UpdateError::ChecksumMismatch));
    let leftovers: Vec<_> = walk(&f._dir.path().join("updates"));
    assert!(leftovers.is_empty(), "left {leftovers:?}");
}

#[test]
fn an_installer_of_the_wrong_size_is_rejected() {
    let f = fixture(true);
    publish(&f, "1.2.0", b"twelve bytes");
    offer_of(f.service.check_now(NOW).expect("check"));
    f.server.serve("/CineVault-setup.exe", b"short".to_vec());
    assert!(matches!(
        confirmed_download(&f, "1.2.0"),
        Err(UpdateError::WrongSize)
    ));
    f.server.serve("/CineVault-setup.exe", vec![1_u8; 5000]);
    assert!(matches!(
        confirmed_download(&f, "1.2.0"),
        Err(UpdateError::WrongSize)
    ));
    assert!(walk(&f._dir.path().join("updates")).is_empty());
}

#[test]
fn a_cancelled_download_leaves_no_file() {
    let f = fixture(true);
    publish(&f, "1.2.0", &vec![3_u8; 100_000]);
    offer_of(f.service.check_now(NOW).expect("check"));
    let cancel = AtomicBool::new(true);
    let error = f
        .service
        .download("1.2.0", &cancel, &mut |_| {})
        .expect_err("cancelled");
    assert!(matches!(error, UpdateError::Cancelled));
    assert!(walk(&f._dir.path().join("updates")).is_empty());
}

#[test]
fn an_installer_name_cannot_leave_the_download_folder() {
    let version = semver::Version::new(1, 2, 0);
    assert_eq!(
        service::installer_name("https://x.test/a/..%2F..%2Fevil.exe?token=1", &version),
        "2F..2Fevil.exe"
    );
    assert_eq!(
        service::installer_name("https://x.test/", &version),
        "CineVault-1.2.0-update"
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(walk(&path));
            } else {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn old_downloads_are_swept_at_start() {
    let f = fixture(true);
    publish(&f, "1.2.0", b"installer");
    offer_of(f.service.check_now(NOW).expect("check"));
    let path = confirmed_download(&f, "1.2.0").expect("download");
    assert!(path.is_file());
    f.service.sweep_downloads();
    assert!(walk(f.service.downloads()).is_empty());
    // Nothing to sweep is fine too.
    f.service.sweep_downloads();
}

// ---- install on close ---------------------------------------------------------------------------

/// A folder that looks like a copy the setup installed (its uninstaller beside the executable).
fn setup_install_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("install dir");
    std::fs::write(dir.path().join(install::SETUP_UNINSTALLER), b"").expect("uninstaller");
    dir
}

/// Records what the fake starter was asked to run.
#[derive(Default)]
struct Started(Mutex<Vec<(std::path::PathBuf, std::path::PathBuf)>>);

impl Started {
    fn starter(
        &self,
    ) -> impl Fn(&VerifiedInstaller, &std::path::Path) -> Result<(), UpdateError> + '_ {
        |installer, dir| {
            // As the real starter does, check the file once more first.
            drop(installer.reverify()?);
            self.0
                .lock()
                .expect("started")
                .push((installer.path().to_path_buf(), dir.to_path_buf()));
            Ok(())
        }
    }

    fn count(&self) -> usize {
        self.0.lock().expect("started").len()
    }
}

#[test]
fn installing_on_close_is_off_by_default() {
    let f = fixture(true);
    assert!(!f.service.install_on_close());
    f.settings
        .set(INSTALL_ON_CLOSE_KEY, json!(true))
        .expect("switch on");
    assert!(f.service.install_on_close());
    f.settings
        .set(INSTALL_ON_CLOSE_KEY, json!(false))
        .expect("switch off");
    assert!(!f.service.install_on_close());
}

#[test]
fn closing_with_no_update_ready_installs_nothing() {
    let f = fixture(true);
    f.settings
        .set(INSTALL_ON_CLOSE_KEY, json!(true))
        .expect("on");
    let exe = setup_install_dir();
    let started = Started::default();

    // Nothing was checked.
    let nothing = install_on_close(&f.service, exe.path(), true, &started.starter());
    assert!(matches!(nothing, CloseUpdate::Nothing), "{nothing:?}");

    // An update is on offer but was not downloaded: closing does not download it.
    publish(&f, "1.2.0", b"the installer");
    offer_of(f.service.check_now(NOW).expect("check"));
    let hits = f.server.hits();
    let nothing = install_on_close(&f.service, exe.path(), true, &started.starter());
    assert!(matches!(nothing, CloseUpdate::Nothing), "{nothing:?}");
    assert_eq!(f.server.hits(), hits, "nothing was requested on close");
    assert_eq!(started.count(), 0);
}

#[test]
fn a_verified_download_installs_on_close_only_when_the_setting_is_on() {
    let f = fixture(true);
    let installer = vec![9_u8; 50_000];
    publish(&f, "1.2.0", &installer);
    offer_of(f.service.check_now(NOW).expect("check"));
    confirmed_download(&f, "1.2.0").expect("download");
    assert_eq!(f.service.ready_version().as_deref(), Some("1.2.0"));
    let exe = setup_install_dir();
    let started = Started::default();

    // Off: closing installs nothing, whatever is ready.
    let off = install_on_close(&f.service, exe.path(), true, &started.starter());
    assert!(matches!(off, CloseUpdate::Nothing), "{off:?}");
    assert_eq!(started.count(), 0);

    // On: the verified setup of that version starts over the running copy.
    f.settings
        .set(INSTALL_ON_CLOSE_KEY, json!(true))
        .expect("on");
    match install_on_close(&f.service, exe.path(), true, &started.starter()) {
        CloseUpdate::Started { version } => assert_eq!(version, "1.2.0"),
        other => panic!("expected the setup to start, got {other:?}"),
    }
    let ran = started.0.lock().expect("started").clone();
    assert_eq!(ran.len(), 1);
    assert!(ran[0].0.starts_with(f.service.downloads()));
    assert_eq!(ran[0].1, exe.path());
}

#[test]
fn a_changed_installer_is_not_started_on_close() {
    let f = fixture(true);
    publish(&f, "1.2.0", b"the real installer");
    offer_of(f.service.check_now(NOW).expect("check"));
    let path = confirmed_download(&f, "1.2.0").expect("download");
    f.settings
        .set(INSTALL_ON_CLOSE_KEY, json!(true))
        .expect("on");
    // Same length, other bytes: the signed checksum no longer matches.
    std::fs::write(&path, b"the evil installer").expect("tamper");
    let exe = setup_install_dir();
    let started = Started::default();

    let failed = install_on_close(&f.service, exe.path(), true, &started.starter());
    assert!(
        matches!(failed, CloseUpdate::Failed(UpdateError::ChecksumMismatch)),
        "{failed:?}"
    );
    assert_eq!(started.count(), 0);
}

#[test]
fn a_copy_that_cannot_update_itself_installs_nothing_on_close() {
    let f = fixture(true);
    publish(&f, "1.2.0", b"installer");
    offer_of(f.service.check_now(NOW).expect("check"));
    confirmed_download(&f, "1.2.0").expect("download");
    f.settings
        .set(INSTALL_ON_CLOSE_KEY, json!(true))
        .expect("on");
    let started = Started::default();

    // A build run from a folder without the setup's uninstaller.
    let bare = tempfile::tempdir().expect("bare");
    let nothing = install_on_close(&f.service, bare.path(), true, &started.starter());
    assert!(matches!(nothing, CloseUpdate::Nothing), "{nothing:?}");
    // A portable copy.
    let portable = setup_install_dir();
    std::fs::write(
        portable.path().join(crate::data_location::PORTABLE_MARKER),
        b"",
    )
    .expect("marker");
    let nothing = install_on_close(&f.service, portable.path(), true, &started.starter());
    assert!(matches!(nothing, CloseUpdate::Nothing), "{nothing:?}");
    // A Mac.
    let mac = setup_install_dir();
    let nothing = install_on_close(&f.service, mac.path(), false, &started.starter());
    assert!(matches!(nothing, CloseUpdate::Nothing), "{nothing:?}");
    assert_eq!(started.count(), 0);
}

#[test]
fn an_appimage_is_replaced_on_close_and_a_package_install_is_left_alone() {
    use std::path::Path;

    let f = fixture(true);
    publish(&f, "1.2.0", b"installer");
    offer_of(f.service.check_now(NOW).expect("check"));
    confirmed_download(&f, "1.2.0").expect("download");
    f.settings
        .set(INSTALL_ON_CLOSE_KEY, json!(true))
        .expect("on");
    let started = Started::default();
    let replaced = Started::default();
    let exe = tempfile::tempdir().expect("exe");
    let appimage = Path::new("/apps/CineVault.AppImage");
    let on_close =
        |method: InstallMethod,
         replace: &dyn Fn(&VerifiedInstaller, &Path) -> Result<(), UpdateError>| {
            install_on_close_with(
                &f.service,
                exe.path(),
                &|_, _| method.clone(),
                &started.starter(),
                replace,
            )
        };

    // An AppImage that can replace itself: the replace runs on that file and nothing else starts.
    let target = InstallMethod::ReplaceAppImage {
        target: appimage.to_path_buf(),
    };
    match on_close(target.clone(), &replaced.starter()) {
        CloseUpdate::Started { version } => assert_eq!(version, "1.2.0"),
        other => panic!("expected the replace to run, got {other:?}"),
    }
    let ran = replaced.0.lock().expect("replaced").clone();
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0].1, appimage);
    assert_eq!(started.count(), 0);

    // A replace that fails leaves the update on offer and the app closing as usual.
    let failing = |_: &VerifiedInstaller, _: &Path| Err(UpdateError::ChecksumMismatch);
    let failed = on_close(target, &failing);
    assert!(
        matches!(failed, CloseUpdate::Failed(UpdateError::ChecksumMismatch)),
        "{failed:?}"
    );

    // A package install is handed nothing on close.
    let package = InstallMethod::OpenInstaller(OpenReason::LinuxPackage);
    let nothing = on_close(package, &replaced.starter());
    assert!(matches!(nothing, CloseUpdate::Nothing), "{nothing:?}");
    assert_eq!(replaced.count(), 1);
    assert_eq!(started.count(), 0);
}

#[test]
fn a_newer_release_replaces_the_waiting_installer_and_a_download_is_not_repeated() {
    let f = fixture(true);
    publish(&f, "1.2.0", b"first installer");
    offer_of(f.service.check_now(NOW).expect("check"));
    confirmed_download(&f, "1.2.0").expect("download");
    let hits = f.server.hits();
    // Confirming after a background download reuses the verified file.
    confirmed_download(&f, "1.2.0").expect("again");
    assert_eq!(f.server.hits(), hits, "the installer was not fetched twice");

    publish(&f, "1.3.0", b"second installer");
    offer_of(f.service.check_now(NOW + 1).expect("check"));
    assert_eq!(
        f.service.ready_version(),
        None,
        "the installer of the older release no longer waits"
    );
}

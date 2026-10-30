//! The hand-over and the settings commands with a stub opener: a valid magnet opens, a bad one is
//! refused before anything opens, the operating system's "no program" error is told apart, a
//! `.torrent` that is a web page is refused, and the Torznab key never reaches the settings.

use std::sync::{Arc, Mutex};

use cv_app::app_settings::SettingsService;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_db::{Database, DbOptions};
use cv_providers::{CredentialResolver, MemoryUserCredentials, ProviderRegistry};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

/// Records what was opened; answers with a scripted error.
#[derive(Default)]
struct Stub {
    urls: Mutex<Vec<String>>,
    paths: Mutex<Vec<PathBuf>>,
    error: Mutex<Option<String>>,
}

impl Stub {
    fn failing(message: &str) -> Self {
        let stub = Self::default();
        *stub.error.lock().unwrap() = Some(message.to_owned());
        stub
    }

    fn opened(&self) -> usize {
        self.urls.lock().unwrap().len() + self.paths.lock().unwrap().len()
    }
}

impl Handoff for Stub {
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.urls.lock().unwrap().push(url.to_owned());
        self.error.lock().unwrap().clone().map_or(Ok(()), Err)
    }

    fn open_path(&self, path: &Path) -> Result<(), String> {
        self.paths.lock().unwrap().push(path.to_path_buf());
        self.error.lock().unwrap().clone().map_or(Ok(()), Err)
    }
}

struct Lab {
    service: Arc<TorrentService>,
    settings: Arc<SettingsService>,
    _dir: tempfile::TempDir,
}

fn lab(accepted: bool) -> Lab {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::open_with(dir.path().join("library.db"), &DbOptions { readers: 1 }).unwrap(),
    );
    let settings = Arc::new(SettingsService::in_memory("test"));
    let providers = Arc::new(ProviderCredentialsService::with_resolver(
        CredentialResolver::new(Arc::new(MemoryUserCredentials::default())),
        ProviderRegistry::new(),
    ));
    let service = Arc::new(TorrentService::new(
        Arc::clone(&settings),
        Arc::new(SecretStore::in_memory()),
        db,
        providers,
    ));
    if accepted {
        settings
            .set("torrents.notice-accepted", serde_json::json!(true))
            .unwrap();
    }
    Lab {
        service,
        settings,
        _dir: dir,
    }
}

fn good_magnet() -> String {
    format!(
        "magnet:?xt=urn:btih:{HASH}&dn=Night%20of%20the%20Living%20Dead&tr=udp%3A%2F%2Ftracker.example%3A80"
    )
}

// ------------------------------------------------------------------ magnets

#[test]
fn a_valid_magnet_is_handed_to_the_system_once() {
    let lab = lab(true);
    let stub = Stub::default();
    open_magnet(&lab.service, &stub, &good_magnet()).expect("opened");
    assert_eq!(*stub.urls.lock().unwrap(), vec![good_magnet()]);
}

#[test]
fn an_unusable_magnet_is_refused_before_anything_is_opened() {
    let lab = lab(true);
    let stub = Stub::default();
    for bad in [
        "magnet:?xt=urn:btih:zz",
        "https://example.com/a.torrent",
        "magnet:?dn=only-a-name",
        "javascript:alert(1)",
        "",
        &format!("magnet:?xt=urn:btih:{HASH}&x.pe=1.2.3.4:5"),
    ] {
        let error = open_magnet(&lab.service, &stub, bad).unwrap_err();
        assert_eq!(error.code, "invalid_magnet", "{bad}");
    }
    assert_eq!(stub.opened(), 0);
}

#[test]
fn no_registered_program_is_told_apart_from_other_failures() {
    let lab = lab(true);
    for message in [
        "No application is associated with the specified file for this operation. (os error 1155)",
        "LSOpenURLsWithRole() failed with error -10814 for the URL",
        "The application can't be found (kLSApplicationNotFoundErr)",
    ] {
        let stub = Stub::failing(message);
        let error = open_magnet(&lab.service, &stub, &good_magnet()).unwrap_err();
        assert_eq!(error.code, "no_handler", "{message}");
        assert_eq!(error.message, "No torrent app is set up for magnet links.");
    }
    let stub = Stub::failing("The device is not ready (os error 21)");
    let error = open_magnet(&lab.service, &stub, &good_magnet()).unwrap_err();
    assert_eq!(error.code, "failed");
    assert!(
        !error.message.contains("os error"),
        "the system text is logged, not shown"
    );
}

#[test]
fn nothing_opens_while_the_feature_is_off() {
    let lab = lab(false);
    let stub = Stub::default();
    assert_eq!(
        open_magnet(&lab.service, &stub, &good_magnet())
            .unwrap_err()
            .code,
        "notice_not_accepted"
    );
    assert_eq!(stub.opened(), 0);
}

// ------------------------------------------------------------------ torrent files

fn torrent_bytes() -> Vec<u8> {
    let mut bytes = b"d8:announce23:udp://tracker.example:804:infod6:lengthi5e4:name1:a12:piece lengthi16384e6:pieces20:".to_vec();
    bytes.extend_from_slice(&[7u8; 20]);
    bytes.extend_from_slice(b"ee");
    bytes
}

/// Serves one fixed answer to every request.
async fn serve(status: u16, content_type: &'static str, body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let body = body.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 4096];
                let _ = stream.read(&mut buffer).await;
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    format!("http://{addr}/a.torrent")
}

/// Answers every request with a redirect to `location` (a site that only has magnet links, as the
/// indexer service's download address does).
async fn serve_redirect(location: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let location = location.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 4096];
                let _ = stream.read(&mut buffer).await;
                let head = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    format!("http://{addr}/dl/lime/?file=x")
}

#[test]
fn only_web_addresses_open_as_a_release_page() {
    let lab = lab(true);
    let stub = Stub::default();
    open_page(
        &lab.service,
        &stub,
        " https://thepiratebay.org/description.php?id=1 ",
    )
    .unwrap();
    open_page(&lab.service, &stub, "http://127.0.0.1:9117/x").unwrap();
    assert_eq!(
        stub.urls.lock().unwrap().clone(),
        vec![
            "https://thepiratebay.org/description.php?id=1".to_owned(),
            "http://127.0.0.1:9117/x".to_owned()
        ]
    );
    for bad in [
        "",
        "magnet:?xt=urn:btih:aa",
        "file:///C:/Windows/System32/calc.exe",
        "javascript:alert(1)",
        "ftp://x.example/a",
        "https://",
        "https:///x",
        "https://a b.example",
        r"C:/Windows/notepad.exe",
    ] {
        let error = open_page(&lab.service, &stub, bad).unwrap_err();
        assert_eq!(error.code, "invalid_address", "{bad:?}");
    }
    assert_eq!(stub.opened(), 2, "nothing else was opened");
}

#[test]
fn a_release_page_is_refused_while_the_feature_is_off() {
    let lab = lab(false);
    let stub = Stub::default();
    assert_eq!(
        open_page(&lab.service, &stub, "https://x.example/")
            .unwrap_err()
            .code,
        "notice_not_accepted"
    );
    assert_eq!(stub.opened(), 0);
}

#[tokio::test]
async fn a_download_address_that_redirects_to_a_magnet_hands_the_magnet_over() {
    let lab = lab(true);
    let magnet = format!("magnet:?xt=urn:btih:{HASH}&dn=Night+of+the+Living+Dead");
    let url = serve_redirect(magnet.clone()).await;
    let tmp = lab._dir.path().join("torrents").join("tmp");
    let stub = Stub::default();
    open_torrent_file(&lab.service, &stub, &tmp, &url, "Night of the Living Dead")
        .await
        .expect("opened");
    assert_eq!(stub.urls.lock().unwrap().clone(), vec![magnet]);
    assert!(stub.paths.lock().unwrap().is_empty());
    assert!(!tmp.exists(), "no file was written");
}

#[tokio::test]
async fn a_redirect_to_anything_else_is_not_opened() {
    let lab = lab(true);
    let url = serve_redirect("file:///C:/Windows/System32/calc.exe".to_owned()).await;
    let tmp = lab._dir.path().join("torrents").join("tmp");
    let stub = Stub::default();
    let error = open_torrent_file(&lab.service, &stub, &tmp, &url, "x")
        .await
        .unwrap_err();
    assert_eq!(error.code, "failed");
    assert_eq!(stub.opened(), 0);
}

#[tokio::test]
async fn a_torrent_file_is_downloaded_saved_and_opened() {
    let lab = lab(true);
    let url = serve(200, "application/x-bittorrent", torrent_bytes()).await;
    let tmp = lab._dir.path().join("torrents").join("tmp");
    let stub = Stub::default();
    open_torrent_file(
        &lab.service,
        &stub,
        &tmp,
        &url,
        "Night of the Living Dead (1968) [1080p]",
    )
    .await
    .expect("opened");
    let paths = stub.paths.lock().unwrap().clone();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].parent().unwrap(), tmp);
    assert_eq!(
        paths[0].file_name().unwrap(),
        "Night of the Living Dead _1968_ _1080p.torrent"
    );
    assert_eq!(std::fs::read(&paths[0]).unwrap(), torrent_bytes());
}

#[tokio::test]
async fn a_web_page_a_wrong_file_and_a_big_answer_are_refused() {
    let lab = lab(true);
    let tmp = lab._dir.path().join("tmp");
    let stub = Stub::default();
    let page = serve(
        200,
        "text/html",
        b"<!DOCTYPE html><html>Just a moment</html>".to_vec(),
    )
    .await;
    let error = open_torrent_file(&lab.service, &stub, &tmp, &page, "x")
        .await
        .unwrap_err();
    assert_eq!(error.code, "failed");

    let junk = serve(
        200,
        "application/octet-stream",
        b"MZ\x90\x00 this is a program".to_vec(),
    )
    .await;
    let error = open_torrent_file(&lab.service, &stub, &tmp, &junk, "x")
        .await
        .unwrap_err();
    assert_eq!(error.message, "That address did not give a torrent file.");

    let big = serve(200, "application/x-bittorrent", vec![b'd'; 9 * 1024 * 1024]).await;
    assert_eq!(
        open_torrent_file(&lab.service, &stub, &tmp, &big, "x")
            .await
            .unwrap_err()
            .code,
        "failed"
    );

    let missing = serve(404, "text/plain", Vec::new()).await;
    assert_eq!(
        open_torrent_file(&lab.service, &stub, &tmp, &missing, "x")
            .await
            .unwrap_err()
            .code,
        "failed"
    );
    assert_eq!(stub.opened(), 0, "nothing was opened");
    assert!(
        !tmp.exists() || std::fs::read_dir(&tmp).unwrap().next().is_none(),
        "nothing was saved"
    );
}

#[tokio::test]
async fn a_torrent_with_no_program_for_it_says_so() {
    let lab = lab(true);
    let url = serve(200, "application/x-bittorrent", torrent_bytes()).await;
    let stub =
        Stub::failing("No application is associated with the specified file (os error 1155)");
    let error = open_torrent_file(&lab.service, &stub, &lab._dir.path().join("t"), &url, "x")
        .await
        .unwrap_err();
    assert_eq!(error.code, "no_handler");
}

#[tokio::test]
async fn a_file_download_is_refused_while_the_feature_is_off() {
    let lab = lab(false);
    let stub = Stub::default();
    let error = open_torrent_file(
        &lab.service,
        &stub,
        &lab._dir.path().join("t"),
        "http://127.0.0.1:9/a",
        "x",
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "notice_not_accepted");
}

#[test]
fn torrent_files_are_recognised_by_their_shape() {
    assert!(is_torrent_file(&torrent_bytes()));
    assert!(is_torrent_file(
        b"d13:announce-listll3:abcee4:infod4:name1:aee"
    ));
    assert!(is_torrent_file(b"d4:infod6:lengthi1e4:name1:aee"));
    assert!(is_torrent_file(b"d7:comment3:abc4:infod4:name1:aee"));
    for bad in [
        &b""[..],
        b"<html>",
        b"{\"a\":1}",
        b"d",
        b"d99999:x",
        b"d3:foo3:bar4:infod4:name1:aee",
        b"d8:announce3:abc",
        b"MZ\x90\x00",
    ] {
        assert!(!is_torrent_file(bad), "{bad:?}");
    }
}

#[test]
fn file_names_are_safe() {
    assert_eq!(
        torrent_file_name("Heat (1995) 1080p"),
        "Heat _1995_ 1080p.torrent"
    );
    assert_eq!(torrent_file_name("../../etc/passwd"), "etc_passwd.torrent");
    assert_eq!(torrent_file_name("C:\\Windows\\x"), "C__Windows_x.torrent");
    assert_eq!(torrent_file_name("   "), "release.torrent");
    assert_eq!(torrent_file_name("..."), "release.torrent");
    assert_eq!(
        torrent_file_name(&"a".repeat(300)).len(),
        80 + ".torrent".len()
    );
    assert!(
        !torrent_file_name("a/b\\c:d*e?f\"g<h>i|j")
            .contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
    );
}

#[test]
fn the_temporary_folder_is_emptied_but_nothing_else_is_touched() {
    let dir = tempfile::tempdir().unwrap();
    let tmp = dir.path().join("torrents").join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("a.torrent"), b"x").unwrap();
    std::fs::write(tmp.join("b.torrent"), b"x").unwrap();
    std::fs::write(dir.path().join("keep.txt"), b"x").unwrap();
    clear_tmp(&tmp);
    assert!(std::fs::read_dir(&tmp).unwrap().next().is_none());
    assert!(dir.path().join("keep.txt").exists());
    clear_tmp(&dir.path().join("does-not-exist"));
}

// ------------------------------------------------------------------ settings commands

#[test]
fn the_status_dto_uses_the_documented_words() {
    let lab = lab(true);
    lab.service.set_source("yts", false).unwrap();
    let list = statuses(&lab.service).unwrap();
    assert_eq!(list.len(), 6);
    assert_eq!(list[0].health, "untested");
    assert_eq!(list[0].kind, "built-in");
    assert_eq!(list[1].health, "off");
    assert!(!list[1].on);
}

#[test]
fn the_torznab_key_goes_to_the_store_and_never_back_out() {
    let lab = lab(true);
    lab.service
        .set_torznab(
            "http://127.0.0.1:9117/api",
            KeyChange::Set("never-shown-key-1".into()),
        )
        .unwrap();
    let dto = serde_json::to_string(&statuses(&lab.service).unwrap()).unwrap();
    assert!(!dto.contains("never-shown-key-1"));
    assert!(
        !serde_json::to_string(&lab.settings.values())
            .unwrap()
            .contains("never-shown-key-1")
    );
    assert!(dto.contains("\"torznab\""));
}

#[test]
fn failed_tests_have_plain_messages() {
    assert_eq!(
        torznab_failure_message(&SourceError::KeyRejected),
        "The key was rejected"
    );
    assert_eq!(
        torznab_failure_message(&SourceError::Unreachable("dns".into())),
        "Nothing answers at this address"
    );
    assert_eq!(
        torznab_failure_message(&SourceError::Timeout),
        "Nothing answers at this address"
    );
    assert_eq!(
        torznab_failure_message(&SourceError::BadAnswer("This is not a Torznab feed".into())),
        "This is not a Torznab feed"
    );
    assert_eq!(
        torznab_failure_message(&SourceError::BadAnswer("Enter the API key.".into())),
        "Enter the API key."
    );
    assert_eq!(
        torznab_failure_message(&SourceError::BadAnswer("some parser detail".into())),
        "This is not a Torznab feed"
    );
    assert!(torznab_failure_message(&SourceError::Blocked).contains("web page"));
}

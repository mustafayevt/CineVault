//! The external tools manager against a local test server: good downloads install, corrupted
//! ones are rejected by checksum and leave nothing behind, and user paths take precedence.
//! No test touches the network.

mod common;

use std::fs;
use std::path::Path;

use common::{Reply, TestServer, sha256, tar_bz2_of, zip_of};
use cv_media::tools::{
    Cancel, InstallPhase, Manifest, Package, Platform, ToolError, ToolId, ToolSource, ToolState,
    ToolsManager,
};
use serde_json::json;

const FFMPEG_BYTES: &[u8] = b"pretend this is ffmpeg, version one";
const LICENSE_BYTES: &[u8] = b"licence text";

/// A manifest with an FFmpeg package served from `url` and described by `archive`.
fn ffmpeg_manifest(url: &str, archive: &[u8], format: &str, version: &str) -> Manifest {
    let files = if format == "zip" {
        json!([
            { "provides": "ffmpeg", "from": "pkg/bin/ffmpeg.exe", "to": "ffmpeg.exe" },
            { "from": "pkg/LICENSE", "to": "LICENSE.txt" }
        ])
    } else {
        json!([
            { "provides": "ffmpeg", "from": "pkg/ffmpeg", "to": "ffmpeg.exe" }
        ])
    };
    manifest_json(&json!({
        "schema": 1,
        "packages": {
            "ffmpeg": {
                "display": "FFmpeg",
                "license": "test",
                "homepage": "https://example.test",
                "platforms": {
                    "windows-x86_64": {
                        "version": version,
                        "artifacts": [{
                            "url": url,
                            "sha256": sha256(archive),
                            "size": archive.len(),
                            "format": format,
                            "files": files
                        }]
                    }
                }
            }
        }
    }))
}

fn manifest_json(value: &serde_json::Value) -> Manifest {
    Manifest::parse(&value.to_string()).expect("test manifest is valid")
}

fn zip_archive() -> Vec<u8> {
    zip_of(&[
        ("pkg/bin/ffmpeg.exe", FFMPEG_BYTES),
        ("pkg/bin/ffplay.exe", b"not wanted"),
        ("pkg/LICENSE", LICENSE_BYTES),
    ])
}

fn manager(dir: &Path, manifest: Manifest) -> ToolsManager {
    ToolsManager::with_manifest(dir, manifest, Some(Platform::WindowsX64))
}

fn install(manager: &ToolsManager) -> Result<(), ToolError> {
    manager.install(Package::Ffmpeg, &Cancel::new(), &mut |_| {})
}

/// The entries directly inside `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn a_good_download_is_verified_unpacked_and_found() {
    let server = TestServer::start();
    let archive = zip_archive();
    let url = server.route("/ffmpeg.zip", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "1.0"));

    assert!(matches!(
        manager.resolve(ToolId::Ffmpeg),
        Err(ToolError::NotInstalled(ToolId::Ffmpeg))
    ));

    let mut phases = Vec::new();
    manager
        .install(Package::Ffmpeg, &Cancel::new(), &mut |p| {
            phases.push((p.phase, p.downloaded))
        })
        .expect("install");

    let ffmpeg = manager.resolve(ToolId::Ffmpeg).expect("ffmpeg");
    assert_eq!(ffmpeg.source, ToolSource::Managed);
    assert_eq!(fs::read(&ffmpeg.path).expect("read"), FFMPEG_BYTES);
    assert_eq!(manager.version_of(&ffmpeg).as_deref(), Some("1.0"));

    // Only the listed files were taken; the unlisted ffplay was not written.
    let dir = tools.path().join("ffmpeg").join("1.0");
    assert_eq!(names(&dir), ["LICENSE.txt", "ffmpeg.exe", "install.json"]);
    // No staging folder is left behind.
    assert_eq!(names(tools.path()), ["ffmpeg"]);

    // Progress runs from the start to the full size and passes through every phase.
    assert_eq!(phases.first(), Some(&(InstallPhase::Downloading, 0)));
    assert!(phases.iter().any(|p| p.0 == InstallPhase::Extracting));
    assert_eq!(
        phases.last(),
        Some(&(InstallPhase::Finishing, archive.len() as u64))
    );
    assert_eq!(server.hits(), ["/ffmpeg.zip"]);
}

#[test]
fn a_corrupted_download_is_rejected_by_checksum_and_nothing_is_installed() {
    let server = TestServer::start();
    let archive = zip_archive();
    // Same length, one byte flipped: a typical bit-rot or tampering case.
    let mut corrupted = archive.clone();
    let middle = corrupted.len() / 2;
    corrupted[middle] ^= 0xff;
    let url = server.route("/ffmpeg.zip", Reply::Body(corrupted));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "1.0"));

    let error = install(&manager).expect_err("corrupted download must fail");
    match &error {
        ToolError::ChecksumMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, &sha256(&archive));
            assert_ne!(actual, expected);
        }
        other => panic!("expected a checksum mismatch, got {other:?}"),
    }
    assert!(error.to_string().contains("checksum"), "{error}");

    assert!(matches!(
        manager.resolve(ToolId::Ffmpeg),
        Err(ToolError::NotInstalled(_))
    ));
    // The bad file and the staging area are gone; no tool folder was created.
    assert!(!tools.path().join("ffmpeg").exists());
    assert!(!tools.path().join(".staging").exists());
}

#[test]
fn a_corrupted_download_does_not_replace_a_working_install() {
    let server = TestServer::start();
    let good = zip_archive();
    let url = server.route("/ffmpeg.zip", Reply::Body(good.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let first = manager(tools.path(), ffmpeg_manifest(&url, &good, "zip", "1.0"));
    install(&first).expect("first install");

    // A newer pinned version whose download is corrupted.
    let bad = zip_of(&[("pkg/bin/ffmpeg.exe", b"other")]);
    let bad_url = server.route("/new.zip", Reply::Body(bad));
    let second = manager(tools.path(), ffmpeg_manifest(&bad_url, &good, "zip", "2.0"));
    assert!(matches!(
        install(&second),
        Err(ToolError::ChecksumMismatch { .. })
    ));

    let ffmpeg = second
        .resolve(ToolId::Ffmpeg)
        .expect("old install still works");
    assert_eq!(fs::read(&ffmpeg.path).expect("read"), FFMPEG_BYTES);
    assert_eq!(second.version_of(&ffmpeg).as_deref(), Some("1.0"));
    assert!(names(&tools.path().join("ffmpeg")).contains(&"1.0".to_owned()));
}

#[test]
fn a_truncated_download_is_rejected() {
    let server = TestServer::start();
    let archive = zip_archive();
    let url = server.route(
        "/ffmpeg.zip",
        Reply::Body(archive[..archive.len() - 10].to_vec()),
    );
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "1.0"));
    assert!(matches!(
        install(&manager),
        Err(ToolError::ChecksumMismatch { .. })
    ));
    assert!(!tools.path().join("ffmpeg").exists());
}

#[test]
fn a_download_longer_than_its_pinned_size_is_stopped() {
    let server = TestServer::start();
    let archive = zip_archive();
    let mut longer = archive.clone();
    longer.extend_from_slice(&[0; 4096]);
    let url = server.route("/ffmpeg.zip", Reply::Body(longer));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "1.0"));
    assert!(matches!(install(&manager), Err(ToolError::TooLarge { .. })));
    assert!(!tools.path().join("ffmpeg").exists());
}

#[test]
fn server_errors_are_reported_and_leave_nothing() {
    let server = TestServer::start();
    let archive = zip_archive();
    let url = server.route("/ffmpeg.zip", Reply::Status(500));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "1.0"));
    let error = install(&manager).expect_err("500");
    assert!(error.to_string().contains("500"), "{error}");

    let missing = manager_for_missing(tools.path(), &server, &archive);
    let error = install(&missing).expect_err("404");
    assert!(error.to_string().contains("404"), "{error}");
    assert!(!tools.path().join("ffmpeg").exists());
}

fn manager_for_missing(dir: &Path, server: &TestServer, archive: &[u8]) -> ToolsManager {
    let url = server.url("/not-there.zip");
    manager(dir, ffmpeg_manifest(&url, archive, "zip", "1.0"))
}

#[test]
fn a_cancelled_download_leaves_nothing() {
    let server = TestServer::start();
    // Big enough for several chunks.
    let big = vec![7_u8; 600 * 1024];
    let archive = zip_of(&[("pkg/bin/ffmpeg.exe", &big), ("pkg/LICENSE", LICENSE_BYTES)]);
    let url = server.route("/ffmpeg.zip", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "1.0"));
    let cancel = Cancel::new();
    let result = manager.install(Package::Ffmpeg, &cancel, &mut |p| {
        if p.downloaded > 0 {
            cancel.cancel();
        }
    });
    assert!(matches!(result, Err(ToolError::Cancelled)), "{result:?}");
    assert!(!tools.path().join("ffmpeg").exists());
    assert!(!tools.path().join(".staging").exists());
}

#[test]
fn plain_http_to_a_remote_host_is_refused_before_any_request() {
    let archive = zip_archive();
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(
        tools.path(),
        ffmpeg_manifest("http://example.invalid/ffmpeg.zip", &archive, "zip", "1.0"),
    );
    assert!(matches!(
        install(&manager),
        Err(ToolError::InsecureUrl { .. })
    ));
}

#[test]
fn an_archive_missing_a_listed_file_is_rejected() {
    let server = TestServer::start();
    let archive = zip_of(&[("pkg/bin/ffmpeg.exe", FFMPEG_BYTES)]);
    let url = server.route("/ffmpeg.zip", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "1.0"));
    let error = install(&manager).expect_err("missing licence");
    assert!(matches!(error, ToolError::Archive(_)), "{error:?}");
    assert!(error.to_string().contains("LICENSE"), "{error}");
    assert!(!tools.path().join("ffmpeg").exists());
}

#[test]
fn tar_bz2_archives_are_supported() {
    let server = TestServer::start();
    let archive = tar_bz2_of(&[("pkg/ffmpeg", FFMPEG_BYTES), ("pkg/other", b"skipped")]);
    let url = server.route("/ffmpeg.tar.bz2", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(
        tools.path(),
        ffmpeg_manifest(&url, &archive, "tar.bz2", "3.1"),
    );
    install(&manager).expect("install");
    let ffmpeg = manager.resolve(ToolId::Ffmpeg).expect("ffmpeg");
    assert_eq!(fs::read(ffmpeg.path).expect("read"), FFMPEG_BYTES);
}

#[test]
fn raw_downloads_are_supported() {
    let server = TestServer::start();
    let program = b"pretend ffmpeg".to_vec();
    let url = server.route("/ffmpeg.exe", Reply::Body(program.clone()));
    let manifest = manifest_json(&json!({
        "schema": 1,
        "packages": { "ffmpeg": {
            "display": "FFmpeg", "license": "Unlicense", "homepage": "https://example.test",
            "platforms": { "windows-x86_64": { "version": "2026.01.01", "artifacts": [{
                "url": url, "sha256": sha256(&program), "size": program.len(), "format": "raw",
                "files": [{ "provides": "ffmpeg", "from": "ffmpeg.exe", "to": "ffmpeg.exe" }]
            }]}}
        }}
    }));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), manifest);
    manager
        .install(Package::Ffmpeg, &Cancel::new(), &mut |_| {})
        .expect("install");
    let tool = manager.resolve(ToolId::Ffmpeg).expect("ffmpeg");
    assert_eq!(fs::read(tool.path).expect("read"), program);
}

#[test]
fn a_new_version_replaces_and_removes_the_old_one() {
    let server = TestServer::start();
    let archive = zip_archive();
    let url = server.route("/ffmpeg.zip", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    install(&manager(
        tools.path(),
        ffmpeg_manifest(&url, &archive, "zip", "1.0"),
    ))
    .expect("v1");
    let newer = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "2.0"));

    // Before the update the old install is usable and flagged as out of date.
    let before = newer.status();
    let status = before
        .iter()
        .find(|s| s.id == ToolId::Ffmpeg)
        .expect("ffmpeg");
    assert_eq!(status.state, ToolState::Ready);
    assert_eq!(status.version.as_deref(), Some("1.0"));
    assert_eq!(status.pinned_version.as_deref(), Some("2.0"));
    assert!(status.update_available);

    install(&newer).expect("v2");
    assert_eq!(names(&tools.path().join("ffmpeg")), ["2.0"]);
    let status = newer
        .status()
        .into_iter()
        .find(|s| s.id == ToolId::Ffmpeg)
        .expect("ffmpeg");
    assert_eq!(status.version.as_deref(), Some("2.0"));
    assert!(!status.update_available);
}

#[test]
fn a_damaged_install_is_reported_and_repaired_by_installing_again() {
    let server = TestServer::start();
    let archive = zip_archive();
    let url = server.route("/ffmpeg.zip", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "1.0"));
    install(&manager).expect("install");

    // Truncate the installed program, as a full disk or a stray tool might.
    let path = manager.resolve(ToolId::Ffmpeg).expect("ffmpeg").path;
    fs::write(&path, b"x").expect("damage");
    assert!(matches!(
        manager.resolve(ToolId::Ffmpeg),
        Err(ToolError::NotInstalled(_))
    ));
    let state = |m: &ToolsManager| {
        m.status()
            .into_iter()
            .find(|s| s.id == ToolId::Ffmpeg)
            .map(|s| s.state)
    };
    assert_eq!(state(&manager), Some(ToolState::Damaged));

    install(&manager).expect("repair");
    assert_eq!(state(&manager), Some(ToolState::Ready));
    assert_eq!(fs::read(path).expect("read"), FFMPEG_BYTES);
}

#[test]
fn a_user_path_wins_over_a_download() {
    let server = TestServer::start();
    let archive = zip_archive();
    let url = server.route("/ffmpeg.zip", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "1.0"));
    install(&manager).expect("install");

    let mine = tempfile::tempdir().expect("temp dir");
    let program = mine.path().join("my-ffmpeg");
    fs::write(&program, b"mine").expect("write");
    let set = manager
        .set_override(ToolId::Ffmpeg, Some(&program))
        .expect("set");
    assert_eq!(set.as_deref(), Some(program.as_path()));

    let resolved = manager.resolve(ToolId::Ffmpeg).expect("ffmpeg");
    assert_eq!(resolved.source, ToolSource::Override);
    assert_eq!(resolved.path, program);

    manager.set_override(ToolId::Ffmpeg, None).expect("clear");
    assert_eq!(
        manager.resolve(ToolId::Ffmpeg).expect("ffmpeg").source,
        ToolSource::Managed
    );
}

#[test]
fn a_user_path_that_disappears_is_an_error_and_never_falls_back_silently() {
    let tools = tempfile::tempdir().expect("temp dir");
    let archive = zip_archive();
    let manager = manager(
        tools.path(),
        ffmpeg_manifest("https://example.test/ffmpeg.zip", &archive, "zip", "1.0"),
    );
    let gone = tools.path().join("gone");
    manager.restore_override(ToolId::Ffmpeg, &gone);
    assert!(matches!(
        manager.resolve(ToolId::Ffmpeg),
        Err(ToolError::OverrideMissing { .. })
    ));
    let status = manager
        .status()
        .into_iter()
        .find(|s| s.id == ToolId::Ffmpeg)
        .expect("ffmpeg");
    assert_eq!(status.state, ToolState::OverrideMissing);
    assert_eq!(status.override_path.as_deref(), Some(gone.as_path()));
}

#[test]
fn setting_a_path_checks_it_and_accepts_a_folder() {
    let tools = tempfile::tempdir().expect("temp dir");
    let archive = zip_archive();
    let manager = manager(
        tools.path(),
        ffmpeg_manifest("https://example.test/ffmpeg.zip", &archive, "zip", "1.0"),
    );
    let mine = tempfile::tempdir().expect("temp dir");

    assert!(matches!(
        manager.set_override(ToolId::Ffmpeg, Some(Path::new("relative/ffmpeg"))),
        Err(ToolError::OverrideInvalid { .. })
    ));
    assert!(matches!(
        manager.set_override(ToolId::Ffmpeg, Some(&mine.path().join("nope"))),
        Err(ToolError::OverrideInvalid { .. })
    ));
    // An empty folder has no ffmpeg in it.
    assert!(matches!(
        manager.set_override(ToolId::Ffmpeg, Some(mine.path())),
        Err(ToolError::OverrideInvalid { .. })
    ));
    assert_eq!(manager.override_for(ToolId::Ffmpeg), None);

    let file = mine.path().join(ToolId::Ffmpeg.file_name());
    fs::write(&file, b"x").expect("write");
    let set = manager
        .set_override(ToolId::Ffmpeg, Some(mine.path()))
        .expect("folder");
    assert_eq!(set, Some(file));
}

#[test]
fn the_version_of_a_user_program_is_asked_of_the_program() {
    let tools = tempfile::tempdir().expect("temp dir");
    let archive = zip_archive();
    let manager = manager(
        tools.path(),
        ffmpeg_manifest("https://example.test/ffmpeg.zip", &archive, "zip", "1.0"),
    );
    let mine = tempfile::tempdir().expect("temp dir");
    let script = write_fake_ffmpeg(mine.path());
    manager
        .set_override(ToolId::Ffmpeg, Some(&script))
        .expect("set");
    let status = manager
        .status()
        .into_iter()
        .find(|s| s.id == ToolId::Ffmpeg)
        .expect("ffmpeg");
    assert_eq!(status.state, ToolState::Ready);
    assert_eq!(status.source, Some(ToolSource::Override));
    assert_eq!(status.version.as_deref(), Some("4.5.6-test"));
}

#[test]
fn another_program_is_refused_by_its_name_and_never_started() {
    let tools = tempfile::tempdir().expect("temp dir");
    let archive = zip_archive();
    let manager = manager(
        tools.path(),
        ffmpeg_manifest("https://example.test/ffmpeg.zip", &archive, "zip", "1.0"),
    );
    let mine = tempfile::tempdir().expect("temp dir");
    let marker = mine.path().join("started");
    let other = write_program(mine.path(), "notepad", "ffmpeg version 1.0", Some(&marker));
    for result in [
        manager.check_override(ToolId::Ffmpeg, &other),
        manager
            .set_override(ToolId::Ffmpeg, Some(&other))
            .map(|_| other.clone()),
    ] {
        let Err(ToolError::OverrideInvalid { reason, .. }) = result else {
            panic!("notepad accepted as ffmpeg: {result:?}");
        };
        assert!(reason.contains("not ffmpeg"), "{reason}");
    }
    assert!(!marker.exists(), "the other program was started");
    assert_eq!(manager.override_for(ToolId::Ffmpeg), None);
}

#[test]
fn a_program_with_the_name_but_not_the_answer_is_refused() {
    let tools = tempfile::tempdir().expect("temp dir");
    let archive = zip_archive();
    let manager = manager(
        tools.path(),
        ffmpeg_manifest("https://example.test/ffmpeg.zip", &archive, "zip", "1.0"),
    );
    let mine = tempfile::tempdir().expect("temp dir");
    let silent = write_program(mine.path(), "ffmpeg-renamed", "hello", None);
    let error = manager
        .check_override(ToolId::Ffmpeg, &silent)
        .expect_err("not ffmpeg");
    assert!(error.to_string().contains("did not answer"), "{error}");

    let real = write_fake_ffmpeg(mine.path());
    assert_eq!(
        manager
            .check_override(ToolId::Ffmpeg, &real)
            .expect("ffmpeg"),
        real
    );
}

/// A small program that prints `output` and, given a marker path, creates it when it runs.
#[cfg(windows)]
fn write_program(
    dir: &Path,
    name: &str,
    output: &str,
    marker: Option<&Path>,
) -> std::path::PathBuf {
    let path = dir.join(format!("{name}.bat"));
    let touch = marker.map_or_else(String::new, |m| {
        format!(
            "echo x > \"{}\"
",
            m.display()
        )
    });
    fs::write(
        &path,
        format!(
            "@echo off
{touch}echo {output}
"
        ),
    )
    .expect("write");
    path
}

#[cfg(unix)]
fn write_program(
    dir: &Path,
    name: &str,
    output: &str,
    marker: Option<&Path>,
) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    let touch = marker.map_or_else(String::new, |m| {
        format!(
            "touch '{}'
",
            m.display()
        )
    });
    fs::write(
        &path,
        format!(
            "#!/bin/sh
{touch}echo '{output}'
"
        ),
    )
    .expect("write");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

#[cfg(windows)]
fn write_fake_ffmpeg(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("fake-ffmpeg.bat");
    fs::write(
        &path,
        "@echo off\r\necho ffmpeg version 4.5.6-test Copyright\r\n",
    )
    .expect("write");
    path
}

#[cfg(unix)]
fn write_fake_ffmpeg(dir: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-ffmpeg");
    fs::write(
        &path,
        "#!/bin/sh\necho 'ffmpeg version 4.5.6-test Copyright'\n",
    )
    .expect("write");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

#[test]
fn a_library_override_reports_the_version_the_loader_saw() {
    let tools = tempfile::tempdir().expect("temp dir");
    let archive = zip_archive();
    let manager = manager(
        tools.path(),
        ffmpeg_manifest("https://example.test/ffmpeg.zip", &archive, "zip", "1.0"),
    );
    let mine = tempfile::tempdir().expect("temp dir");
    let library = mine.path().join(ToolId::MediaInfo.file_name());
    fs::write(&library, b"x").expect("write");
    manager
        .set_override(ToolId::MediaInfo, Some(&library))
        .expect("set");
    let version = |m: &ToolsManager| {
        m.status()
            .into_iter()
            .find(|s| s.id == ToolId::MediaInfo)
            .and_then(|s| s.version)
    };
    assert_eq!(version(&manager), None);
    manager.report_loaded_version(ToolId::MediaInfo, "25.04");
    assert_eq!(version(&manager).as_deref(), Some("25.04"));
}

#[test]
fn a_platform_without_downloads_is_reported_as_unsupported() {
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = ToolsManager::with_manifest(tools.path(), Manifest::embedded(), None);
    let status = manager.status();
    assert!(status.iter().all(|s| s.state == ToolState::Unsupported));
    assert!(matches!(
        install(&manager),
        Err(ToolError::UnsupportedPlatform(Package::Ffmpeg))
    ));
}

#[test]
fn an_announced_download_is_coming_until_its_install_finishes() {
    let server = TestServer::start();
    let archive = zip_archive();
    let url = server.route("/ffmpeg.zip", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "1.0"));
    assert!(!manager.is_coming(Package::Ffmpeg));

    manager.expect(Package::Ffmpeg);
    assert!(manager.is_coming(Package::Ffmpeg));
    assert!(!manager.is_coming(Package::MediaInfo));

    // While the download runs it is coming too, and afterwards it is settled.
    let mut during = Vec::new();
    manager
        .install(Package::Ffmpeg, &Cancel::new(), &mut |_| {
            during.push(manager.is_coming(Package::Ffmpeg));
        })
        .expect("install");
    assert!(during.iter().all(|coming| *coming) && !during.is_empty());
    assert!(!manager.is_coming(Package::Ffmpeg));
}

#[test]
fn a_failed_download_is_no_longer_coming() {
    let server = TestServer::start();
    let archive = zip_archive();
    let url = server.route("/ffmpeg.zip", Reply::Status(500));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), ffmpeg_manifest(&url, &archive, "zip", "1.0"));
    manager.expect(Package::Ffmpeg);
    install(&manager).expect_err("500");
    assert!(!manager.is_coming(Package::Ffmpeg));
}

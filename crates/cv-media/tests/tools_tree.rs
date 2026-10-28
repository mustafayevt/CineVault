//! Tree installs of the tool manager (a program that comes with many files of its own): the shape
//! below the root is kept, every file is recorded, and anything that could land elsewhere is
//! refused before the install is moved into place. No test touches the network.

mod common;

use std::fs;
use std::path::Path;

use common::{
    Reply, TarKind, TestServer, sha256, tar_bz2_of, tar_gz_of, tar_gz_raw, zip_of, zip_with_symlink,
};
use cv_media::tools::{
    Cancel, Manifest, Package, Platform, ToolError, ToolId, ToolSource, ToolState, ToolsManager,
};
use serde_json::json;

const PROGRAM_BYTES: &[u8] = b"pretend this is the whole program";

/// A tree package (the indexer-service package is a tree) served from `url`: everything below
/// `Pkg/`, with `bin/run.exe` as the program.
fn tree_manifest(url: &str, archive: &[u8], format: &str, max_unpacked: Option<u64>) -> Manifest {
    let mut artifact = json!({
        "url": url,
        "sha256": sha256(archive),
        "size": archive.len(),
        "format": format,
        "mode": "tree",
        "root": "Pkg/",
        "program": { "provides": "indexer-service", "path": "bin/run.exe" }
    });
    if let Some(limit) = max_unpacked {
        artifact["max_unpacked"] = json!(limit);
    }
    Manifest::parse(
        &json!({
            "schema": 1,
            "packages": { "indexer-service": {
                "display": "Tree test", "license": "test", "homepage": "https://example.test",
                "platforms": { "windows-x86_64": { "version": "1.0", "artifacts": [artifact] } }
            }}
        })
        .to_string(),
    )
    .expect("test manifest is valid")
}

fn manager(dir: &Path, manifest: Manifest) -> ToolsManager {
    ToolsManager::with_manifest(dir, manifest, Some(Platform::WindowsX64))
}

fn install_tree(manager: &ToolsManager) -> Result<(), ToolError> {
    manager.install(Package::IndexerService, &Cancel::new(), &mut |_| {})
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

fn tree_entries() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("Pkg/bin/run.exe", PROGRAM_BYTES),
        ("Pkg/bin/lib/helper.dll", b"a library"),
        ("Pkg/config/default.json", b"{}"),
        ("Pkg/README.txt", b"read me"),
    ]
}

/// Installs `archive` as a tree and returns the error it fails with, and the tools folder.
fn tree_failure(
    archive: &[u8],
    format: &str,
    limit: Option<u64>,
) -> (ToolError, tempfile::TempDir) {
    let server = TestServer::start();
    let url = server.route("/tree", Reply::Body(archive.to_vec()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), tree_manifest(&url, archive, format, limit));
    let error = install_tree(&manager).expect_err("the install is refused");
    (error, tools)
}

/// No install and no staging folder is left.
fn assert_nothing_installed(tools: &Path) {
    assert_eq!(
        names(tools),
        Vec::<String>::new(),
        "tools folder is not empty"
    );
}

fn assert_refused(error: &ToolError) {
    assert!(matches!(error, ToolError::Archive(_)), "{error:?}");
}

fn check_good_tree(archive: Vec<u8>, format: &str) {
    let server = TestServer::start();
    let url = server.route("/tree", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), tree_manifest(&url, &archive, format, None));
    install_tree(&manager).expect("install");

    let program = manager.resolve(ToolId::IndexerService).expect("program");
    assert_eq!(program.source, ToolSource::Managed);
    assert_eq!(fs::read(&program.path).expect("read"), PROGRAM_BYTES);
    let dir = tools.path().join("indexer-service").join("1.0");
    // The root folder is stripped and the shape below it is kept.
    assert_eq!(program.path, dir.join("bin").join("run.exe"));
    assert_eq!(
        fs::read(dir.join("bin").join("lib").join("helper.dll")).expect("helper"),
        b"a library"
    );
    assert_eq!(
        fs::read(dir.join("README.txt")).expect("readme"),
        b"read me"
    );
    assert_eq!(names(&dir), ["README.txt", "bin", "config", "install.json"]);
    assert_eq!(names(tools.path()), ["indexer-service"]);

    // Every file is in the record, so the Damaged check covers the whole tree.
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("install.json")).expect("record"))
            .expect("json");
    let mut listed: Vec<String> = record["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|f| f["name"].as_str().expect("name").to_owned())
        .collect();
    listed.sort();
    assert_eq!(
        listed,
        [
            "README.txt",
            "bin/lib/helper.dll",
            "bin/run.exe",
            "config/default.json"
        ]
    );
}

#[test]
fn a_tree_install_from_a_zip_keeps_the_shape_below_its_root() {
    check_good_tree(zip_of(&tree_entries()), "zip");
}

#[test]
fn a_tree_install_from_a_tar_gz_keeps_the_shape_below_its_root() {
    check_good_tree(tar_gz_of(&tree_entries()), "tar.gz");
}

#[test]
fn a_tree_install_from_a_tar_bz2_keeps_the_shape_below_its_root() {
    check_good_tree(tar_bz2_of(&tree_entries()), "tar.bz2");
}

#[test]
fn a_zip_entry_that_climbs_out_is_refused_and_nothing_is_installed() {
    let mut entries = tree_entries();
    entries.push(("Pkg/../../evil.txt", b"x"));
    let (error, tools) = tree_failure(&zip_of(&entries), "zip", None);
    assert_refused(&error);
    assert!(error.to_string().contains("evil.txt"), "{error}");
    assert_nothing_installed(tools.path());
    // And nothing was written beside the tools folder either.
    let beside = tools.path().parent().expect("parent").join("evil.txt");
    assert!(!beside.exists());
}

#[test]
fn a_tar_entry_that_climbs_out_or_is_absolute_is_refused() {
    for bad in [
        "Pkg/../evil.txt",
        "../evil.txt",
        "/evil.txt",
        "Pkg/a/../../../evil.txt",
    ] {
        let archive = tar_gz_raw(&[
            ("Pkg/bin/run.exe", TarKind::File, PROGRAM_BYTES, 0o755),
            (bad, TarKind::File, b"x", 0o644),
        ]);
        let (error, tools) = tree_failure(&archive, "tar.gz", None);
        assert_refused(&error);
        assert_nothing_installed(tools.path());
    }
}

#[test]
fn a_drive_letter_or_backslash_escape_is_refused() {
    for bad in ["C:/evil.txt", "Pkg/C:/evil.txt", "Pkg\\..\\..\\evil.txt"] {
        let archive = zip_of(&[("Pkg/bin/run.exe", PROGRAM_BYTES), (bad, b"x")]);
        let (error, tools) = tree_failure(&archive, "zip", None);
        assert_refused(&error);
        assert_nothing_installed(tools.path());
    }
}

#[test]
fn an_entry_outside_the_root_is_refused() {
    let mut entries = tree_entries();
    entries.push(("Elsewhere/file.txt", b"x"));
    let (error, tools) = tree_failure(&zip_of(&entries), "zip", None);
    assert_refused(&error);
    assert!(error.to_string().contains("outside"), "{error}");
    assert_nothing_installed(tools.path());
}

#[test]
fn a_symbolic_link_in_a_zip_is_refused() {
    let archive = zip_with_symlink(&tree_entries(), "Pkg/link", "/etc/passwd");
    let (error, tools) = tree_failure(&archive, "zip", None);
    assert_refused(&error);
    assert!(error.to_string().contains("link"), "{error}");
    assert_nothing_installed(tools.path());
}

#[test]
fn symbolic_and_hard_links_in_a_tar_are_refused() {
    for kind in [TarKind::Symlink, TarKind::Hardlink] {
        let archive = tar_gz_raw(&[
            ("Pkg/bin/run.exe", TarKind::File, PROGRAM_BYTES, 0o755),
            ("Pkg/link", kind, b"/etc/passwd", 0o777),
        ]);
        let (error, tools) = tree_failure(&archive, "tar.gz", None);
        assert_refused(&error);
        assert!(error.to_string().contains("link"), "{error}");
        assert_nothing_installed(tools.path());
    }
}

#[test]
fn a_tree_without_its_program_is_refused() {
    let archive = zip_of(&[("Pkg/README.txt", b"read me")]);
    let (error, tools) = tree_failure(&archive, "zip", None);
    assert_refused(&error);
    assert!(error.to_string().contains("bin/run.exe"), "{error}");
    assert_nothing_installed(tools.path());
}

#[test]
fn the_unpacked_size_is_capped() {
    let big = vec![7_u8; 600];
    let archive = zip_of(&[("Pkg/bin/run.exe", &big), ("Pkg/data.bin", &big)]);
    // Two files of 600 bytes do not fit in 1000.
    let (error, tools) = tree_failure(&archive, "zip", Some(1000));
    assert_refused(&error);
    assert!(error.to_string().contains("more than"), "{error}");
    assert_nothing_installed(tools.path());
    // The same archive fits when the limit allows it, so it is the limit that refused it.
    let server = TestServer::start();
    let url = server.route("/tree", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(
        tools.path(),
        tree_manifest(&url, &archive, "zip", Some(1200)),
    );
    install_tree(&manager).expect("fits exactly");
}

#[test]
fn the_cap_counts_the_bytes_really_written() {
    let big = vec![1_u8; 5000];
    let archive = tar_gz_of(&[("Pkg/bin/run.exe", &big)]);
    let (error, tools) = tree_failure(&archive, "tar.gz", Some(4000));
    assert_refused(&error);
    assert_nothing_installed(tools.path());
}

#[test]
fn a_tree_with_a_deleted_file_is_damaged_and_installing_again_repairs_it() {
    let server = TestServer::start();
    let archive = zip_of(&tree_entries());
    let url = server.route("/tree", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), tree_manifest(&url, &archive, "zip", None));
    install_tree(&manager).expect("install");
    let state = |m: &ToolsManager| Some(m.status_of(ToolId::IndexerService).state);
    assert_eq!(state(&manager), Some(ToolState::Ready));

    let dir = tools.path().join("indexer-service").join("1.0");
    let helper = dir.join("bin").join("lib").join("helper.dll");
    fs::remove_file(&helper).expect("delete one file deep in the tree");
    assert!(matches!(
        manager.resolve(ToolId::IndexerService),
        Err(ToolError::NotInstalled(_))
    ));
    assert_eq!(state(&manager), Some(ToolState::Damaged));

    install_tree(&manager).expect("repair");
    assert_eq!(state(&manager), Some(ToolState::Ready));
    assert_eq!(fs::read(helper).expect("helper"), b"a library");
    assert_eq!(names(tools.path()), ["indexer-service"]);
}

#[test]
fn a_failed_tree_install_leaves_the_previous_one_alone() {
    let server = TestServer::start();
    let good = zip_of(&tree_entries());
    let url = server.route("/tree", Reply::Body(good.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    install_tree(&manager(
        tools.path(),
        tree_manifest(&url, &good, "zip", None),
    ))
    .expect("first");

    let mut bad_entries = tree_entries();
    bad_entries.push(("Pkg/../../evil.txt", b"x"));
    let bad = zip_of(&bad_entries);
    let bad_url = server.route("/bad", Reply::Body(bad.clone()));
    let again = manager(tools.path(), tree_manifest(&bad_url, &bad, "zip", None));
    assert!(install_tree(&again).is_err());
    assert_eq!(names(tools.path()), ["indexer-service"]);
    let program = again
        .resolve(ToolId::IndexerService)
        .expect("still installed");
    assert_eq!(fs::read(program.path).expect("read"), PROGRAM_BYTES);
}

#[cfg(unix)]
#[test]
fn the_executable_files_of_a_tree_stay_executable() {
    use std::os::unix::fs::PermissionsExt;
    let archive = tar_gz_raw(&[
        ("Pkg/bin/run.exe", TarKind::File, PROGRAM_BYTES, 0o755),
        ("Pkg/bin/other", TarKind::File, b"x", 0o755),
        ("Pkg/data", TarKind::File, b"y", 0o644),
    ]);
    let server = TestServer::start();
    let url = server.route("/tree", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    // A Windows drive mounted into Linux (WSL's /mnt/c) answers every chmod with its own fixed
    // mode, so no install can keep a mode there; the check only means something where the folder
    // can hold Unix modes.
    let probe = tools.path().join("mode-probe");
    fs::write(&probe, b"x").expect("probe file");
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o751)).expect("chmod probe");
    let kept = fs::metadata(&probe)
        .expect("probe metadata")
        .permissions()
        .mode()
        & 0o777;
    fs::remove_file(&probe).expect("remove probe");
    if kept != 0o751 {
        eprintln!(
            "skipped: the temp folder's file system does not keep Unix modes (got {kept:o}); \
             point TMPDIR at a Linux file system"
        );
        return;
    }
    let manager = manager(tools.path(), tree_manifest(&url, &archive, "tar.gz", None));
    install_tree(&manager).expect("install");
    let program = manager
        .resolve(ToolId::IndexerService)
        .expect("program")
        .path;
    let mode = |p: &Path| fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(mode(&program), 0o755);
    assert_eq!(mode(&program.with_file_name("other")), 0o755);
    let data = program
        .parent()
        .expect("bin")
        .parent()
        .expect("root")
        .join("data");
    assert_eq!(mode(&data), 0o644);
}

#[test]
fn a_manifest_with_a_broken_tree_entry_is_rejected() {
    let parse = |artifact: serde_json::Value| {
        Manifest::parse(
            &json!({
                "schema": 1,
                "packages": { "indexer-service": {
                    "display": "x", "license": "x", "homepage": "https://example.test",
                    "platforms": { "windows-x86_64": { "version": "1", "artifacts": [artifact] } }
                }}
            })
            .to_string(),
        )
    };
    let base = json!({
        "url": "https://example.test/x.zip", "sha256": "a".repeat(64), "size": 1,
        "format": "zip", "mode": "tree", "root": "Pkg/",
        "program": { "provides": "indexer-service", "path": "run.exe" }
    });
    assert!(parse(base.clone()).is_ok());
    let with = |key: &str, value: serde_json::Value| {
        let mut artifact = base.clone();
        artifact[key] = value;
        artifact
    };
    for (key, value) in [
        (
            "program",
            json!({ "provides": "indexer-service", "path": "../run.exe" }),
        ),
        (
            "program",
            json!({ "provides": "indexer-service", "path": "/run.exe" }),
        ),
        ("program", json!({ "provides": "nope", "path": "run.exe" })),
        ("program", json!(null)),
        ("root", json!("../Pkg/")),
        ("format", json!("raw")),
        ("max_unpacked", json!(0)),
        (
            "files",
            json!([{ "provides": "indexer-service", "from": "a", "to": "a" }]),
        ),
    ] {
        assert!(parse(with(key, value.clone())).is_err(), "{key} = {value}");
    }
}

#[test]
fn the_tools_page_lists_two_rows_and_the_indexer_service_keeps_its_own_status() {
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = ToolsManager::with_manifest(
        tools.path(),
        Manifest::embedded(),
        Some(Platform::WindowsX64),
    );
    let ids: Vec<ToolId> = manager.status().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, [ToolId::MediaInfo, ToolId::Ffmpeg]);
    let status = manager.status_of(ToolId::IndexerService);
    assert_eq!(status.state, ToolState::NotInstalled);
    assert_eq!(status.name, "Indexer service (Jackett)");
    assert_eq!(status.pinned_version.as_deref(), Some("0.24.2903"));
    assert_eq!(status.download_size, Some(57_061_988));
    assert_eq!(status.license, "GPL-2.0");
}

// ---- a download that is not the pinned one ---------------------------------------------------

fn tree_spec(url: &str, archive: &[u8], sha: &str, version: &str) -> cv_media::tools::PlatformSpec {
    serde_json::from_value(json!({
        "version": version,
        "artifacts": [{
            "url": url, "sha256": sha, "size": archive.len(), "format": "zip",
            "mode": "tree", "root": "Pkg/",
            "program": { "provides": "indexer-service", "path": "bin/run.exe" }
        }]
    }))
    .expect("spec")
}

#[test]
fn a_newer_download_installs_by_its_own_spec_and_replaces_the_pinned_one() {
    let server = TestServer::start();
    let old = zip_of(&tree_entries());
    let old_url = server.route("/old", Reply::Body(old.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), tree_manifest(&old_url, &old, "zip", None));
    install_tree(&manager).expect("pinned");

    let mut newer = tree_entries();
    newer.push(("Pkg/NEWS.txt", b"what changed"));
    let new = zip_of(&newer);
    let new_url = server.route("/new", Reply::Body(new.clone()));
    let spec = tree_spec(&new_url, &new, &sha256(&new), "2.0");
    manager
        .install_spec(Package::IndexerService, &spec, &Cancel::new(), &mut |_| {})
        .expect("newer");
    assert_eq!(names(&tools.path().join("indexer-service")), ["2.0"]);
    let program = manager.resolve(ToolId::IndexerService).expect("program");
    assert!(
        program
            .path
            .starts_with(tools.path().join("indexer-service").join("2.0"))
    );
    assert_eq!(
        fs::read(
            tools
                .path()
                .join("indexer-service")
                .join("2.0")
                .join("NEWS.txt")
        )
        .expect("news"),
        b"what changed"
    );
    assert_eq!(
        manager
            .pinned_spec(Package::IndexerService)
            .map(|s| s.version),
        Some("1.0".into())
    );
}

#[test]
fn a_download_whose_digest_does_not_match_installs_nothing() {
    let server = TestServer::start();
    let archive = zip_of(&tree_entries());
    let url = server.route("/new", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), tree_manifest(&url, &archive, "zip", None));
    let spec = tree_spec(&url, &archive, &"0".repeat(64), "2.0");
    let error = manager
        .install_spec(Package::IndexerService, &spec, &Cancel::new(), &mut |_| {})
        .expect_err("wrong digest");
    assert!(
        matches!(error, ToolError::ChecksumMismatch { .. }),
        "{error:?}"
    );
    assert_nothing_installed(tools.path());
}

#[test]
fn a_spec_that_breaks_a_manifest_rule_is_refused_before_any_download() {
    let server = TestServer::start();
    let archive = zip_of(&tree_entries());
    let url = server.route("/new", Reply::Body(archive.clone()));
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = manager(tools.path(), tree_manifest(&url, &archive, "zip", None));
    for bad_sha in ["", "not-a-hash", "ABCDEF"] {
        let spec = tree_spec(&url, &archive, bad_sha, "2.0");
        let error = manager
            .install_spec(Package::IndexerService, &spec, &Cancel::new(), &mut |_| {})
            .expect_err("bad digest text");
        assert!(matches!(error, ToolError::Manifest(_)), "{error:?}");
    }
    assert!(server.hits().is_empty(), "nothing was downloaded");
    assert_nothing_installed(tools.path());
}

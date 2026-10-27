//! The `cinevault` command line, end to end: the spec scenarios, the exit codes, the
//! JSON output and the case where the CineVault app is running and executes the batch itself.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use cinevault_cli::{Cli, Env, execute};
use clap::Parser;
use cv_app::automation::ipc::IpcServer;
use cv_app::automation::{BatchEngine, EngineOptions};
use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::Database;
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::{ItemQuery, list_items};
use cv_domain::{ItemKind, SourceKind};
use cv_providers::catalogue::{API_KEY, key};
use cv_providers::{CredentialResolver, MemoryUserCredentials, ProviderRegistry};
use serde_json::Value;
use tempfile::TempDir;

const TMDB_KEY: &str = "dummy-user-tmdb-3f6a92c1d0b7";
const TRAKT_ID: &str = "dummy-user-trakt-id-55e0a1b9c8";

fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

/// An environment that opens no network and no credential store.
fn env(location: DataLocation) -> Env {
    Env::with_services(
        location,
        Arc::new(SecretStore::in_memory()),
        BootOptions::without_providers,
    )
}

struct Output {
    code: u8,
    out: String,
    err: String,
}

impl Output {
    fn json(&self) -> Value {
        serde_json::from_str(&self.out)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", self.out))
    }
}

async fn run(env: &Env, args: &[&str]) -> Output {
    let cli = Cli::try_parse_from(std::iter::once("cinevault").chain(args.iter().copied()))
        .unwrap_or_else(|e| panic!("{args:?} does not parse: {e}"));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = execute(cli, env, &mut out, &mut err).await;
    Output {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// A library with one movie data source holding two downloads. Returns the source folder.
fn library(root: &Path, location: &DataLocation) -> PathBuf {
    let movies = root.join("movies");
    for name in ["alien.1979.1080p.bluray", "heat.1995.720p"] {
        let dir = movies.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.mkv")), name).unwrap();
    }
    location.ensure_dirs().unwrap();
    let db = Database::open(location.library_db()).unwrap();
    let path = movies.to_string_lossy().into_owned();
    db.write(move |tx| insert_source(tx, SourceKind::Movie, &path))
        .unwrap();
    movies
}

fn folders(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn titles(services: &AppServices) -> Vec<String> {
    let mut titles: Vec<String> = services
        .db()
        .read(|c| list_items(c, &ItemQuery::new(ItemKind::Movie), None, 100))
        .unwrap()
        .items
        .into_iter()
        .map(|i| i.title)
        .collect();
    titles.sort();
    titles
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dry_run_rename_prints_the_planned_paths_as_json_and_changes_no_file() {
    let temp = TempDir::new().unwrap();
    let env = env(location(temp.path()));
    let movies = library(temp.path(), env.location());

    // Find the movies first (a real run), then plan their rename.
    let found = run(&env, &["movie", "--update-all", "--json"]).await;
    assert_eq!(found.code, 0, "{}{}", found.out, found.err);
    let before = folders(&movies);
    assert_eq!(before, ["alien.1979.1080p.bluray", "heat.1995.720p"]);

    // `cinevault movie --rename-all --dry-run --json`
    let planned = run(&env, &["movie", "--rename-all", "--dry-run", "--json"]).await;
    assert_eq!(planned.code, 0, "{}{}", planned.out, planned.err);
    let report = planned.json();
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["forwarded"], false);
    let rename = &report["steps"][0];
    assert_eq!(rename["step"], "rename");
    assert_eq!(rename["status"], "planned");
    let items = rename["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{rename}");
    for item in items {
        assert_eq!(item["status"], "rename");
        let (from, to) = (item["from"].as_str().unwrap(), item["to"].as_str().unwrap());
        assert_ne!(from, to);
        assert!(
            from.starts_with(movies.to_string_lossy().as_ref()),
            "{from}"
        );
        assert!(item["title"].as_str().is_some() && item["id"].as_i64().is_some());
    }
    // No file changed.
    assert_eq!(folders(&movies), before);
    assert!(planned.err.is_empty(), "{}", planned.err);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_text_output_shows_each_step_and_a_real_rename_moves_the_folders() {
    let temp = TempDir::new().unwrap();
    let env = env(location(temp.path()));
    let movies = library(temp.path(), env.location());

    let nightly = run(&env, &["movie", "--update-all", "--rename-new"]).await;
    assert_eq!(nightly.code, 0, "{}{}", nightly.out, nightly.err);
    assert!(nightly.out.contains("[update] done"), "{}", nightly.out);
    assert!(nightly.out.contains("[rename] done"), "{}", nightly.out);
    let after = folders(&movies);
    assert_eq!(after.len(), 2, "{after:?}");
    assert!(
        after
            .iter()
            .all(|f| f != "alien.1979.1080p.bluray" && f != "heat.1995.720p"),
        "{after:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn selection_flags_reach_the_batch() {
    let temp = TempDir::new().unwrap();
    let env = env(location(temp.path()));
    library(temp.path(), env.location());
    assert_eq!(run(&env, &["movie", "-u"]).await.code, 0);

    let planned = run(
        &env,
        &[
            "movie",
            "--rename-all",
            "--dry-run",
            "--json",
            "--query",
            "year:1990..2000",
            "--source",
            "1",
        ],
    )
    .await;
    assert_eq!(planned.code, 0, "{}{}", planned.out, planned.err);
    let report = planned.json();
    assert_eq!(report["selection"]["titles"], 1);
    assert_eq!(report["selection"]["narrowed"], true);
    assert_eq!(report["steps"][0]["items"].as_array().unwrap().len(), 1);

    let nothing = run(
        &env,
        &["movie", "--rename-all", "--json", "--query", "year:2090.."],
    )
    .await;
    assert_eq!(nothing.code, 0);
    assert_eq!(nothing.json()["steps"][0]["status"], "skipped");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_running_app_executes_the_batch_when_there_is_one() {
    let temp = TempDir::new().unwrap();
    let location = location(temp.path());
    let movies = library(temp.path(), &location);

    // The app: its own services over the library, and the command-line channel open.
    let services = AppServices::boot(
        BootOptions::new(location.clone(), Arc::new(SecretStore::in_memory())).without_providers(),
    )
    .unwrap();
    services.scan().set_after_scan(Arc::new(|_| {}));
    let engine = BatchEngine::new(Arc::clone(&services), EngineOptions::default());
    let server = IpcServer::start(engine, &location).unwrap();

    // A command line that must not open the library itself: building services is a failure.
    let forward_only =
        Env::with_services(location.clone(), Arc::new(SecretStore::in_memory()), |_| {
            panic!("the command line opened the library although the app is running")
        });
    let found = run(&forward_only, &["movie", "--update-all", "--json"]).await;
    assert_eq!(found.code, 0, "{}{}", found.out, found.err);
    let report = found.json();
    assert_eq!(report["forwarded"], true);
    assert_eq!(report["steps"][0]["status"], "done");
    // The titles are in the app's library: it did the work.
    assert_eq!(titles(&services).len(), 2);

    let text = run(&forward_only, &["movie", "--rename-all", "--dry-run"]).await;
    assert_eq!(text.code, 0, "{}{}", text.out, text.err);
    assert!(
        text.out.contains("run by the open CineVault app"),
        "{}",
        text.out
    );
    assert_eq!(folders(&movies).len(), 2);
    assert!(folders(&movies).contains(&"heat.1995.720p".to_owned()));

    // Real work through the app moves the folders and the app's own library follows.
    let renamed = run(&forward_only, &["movie", "--rename-all", "--json"]).await;
    assert_eq!(renamed.code, 0, "{}{}", renamed.out, renamed.err);
    assert_eq!(renamed.json()["steps"][0]["counts"]["renamed"], 2);
    assert!(!folders(&movies).contains(&"heat.1995.720p".to_owned()));
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn after_the_app_closes_the_command_line_runs_the_batch_itself() {
    let temp = TempDir::new().unwrap();
    let location = location(temp.path());
    library(temp.path(), &location);
    {
        let services = AppServices::boot(
            BootOptions::new(location.clone(), Arc::new(SecretStore::in_memory()))
                .without_providers(),
        )
        .unwrap();
        let engine = BatchEngine::new(Arc::clone(&services), EngineOptions::default());
        let server = IpcServer::start(engine, &location).unwrap();
        assert!(cv_app::automation::ipc::endpoint_file(&location).is_file());
        drop(server);
        assert!(!cv_app::automation::ipc::endpoint_file(&location).exists());
    }
    let env = env(location);
    let report = run(&env, &["movie", "--update-all", "--json"]).await;
    assert_eq!(report.code, 0, "{}{}", report.out, report.err);
    assert_eq!(report.json()["forwarded"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exit_codes_tell_success_failure_and_a_command_that_could_not_run() {
    let temp = TempDir::new().unwrap();
    let env = env(location(temp.path()));
    library(temp.path(), env.location());

    // No step asked for.
    let none = run(&env, &["movie", "--json"]).await;
    assert_eq!(none.code, 2);
    assert!(none.json()["error"].as_str().unwrap().contains("No step"));

    // A selection that cannot be made.
    let bad = run(&env, &["movie", "-r", "--query", "year:abc"]).await;
    assert_eq!(bad.code, 2);
    assert!(bad.out.contains("year:abc"), "{}", bad.out);
    let source = run(&env, &["movie", "-r", "--source", "9"]).await;
    assert_eq!(source.code, 2);

    // A step with nothing to work on is skipped, which is not a failure.
    let aspect = run(&env, &["movie", "-d", "--json"]).await;
    assert_eq!(aspect.code, 0);
    assert_eq!(aspect.json()["steps"][0]["status"], "skipped");

    // Success.
    assert_eq!(run(&env, &["movie", "-u"]).await.code, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_library_that_cannot_be_opened_is_a_clear_message_and_a_failure_code() {
    let temp = TempDir::new().unwrap();
    // The data folder is a file, so nothing can be created in it.
    let location = location(temp.path());
    std::fs::create_dir_all(location.root().parent().unwrap()).unwrap();
    std::fs::write(location.root(), "not a folder").unwrap();
    let env = env(location);
    let failed = run(&env, &["movie", "-u"]).await;
    assert_eq!(failed.code, 2);
    assert!(failed.out.contains("data folder"), "{}", failed.out);
    assert!(failed.out.contains("close it"), "{}", failed.out);
}

/// The header of a library file of schema version 17, from a version before the fresh start.
fn old_library_bytes() -> Vec<u8> {
    let mut bytes = b"SQLite format 3 ".to_vec();
    bytes.resize(100, 0);
    bytes[60..64].copy_from_slice(&17u32.to_be_bytes());
    bytes.extend_from_slice(b"pages of the old library");
    bytes
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_library_of_an_earlier_version_asks_for_the_app_and_moves_nothing() {
    let temp = TempDir::new().unwrap();
    let location = location(temp.path());
    location.ensure_dirs().unwrap();
    std::fs::write(location.library_db(), old_library_bytes()).unwrap();
    let old_settings = "{\"version\":1,\"settings\":{\"values\":{}}}";
    std::fs::write(location.settings_file(), old_settings).unwrap();
    let root = location.root().to_path_buf();
    let names = || {
        let mut names: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let before = names();
    let env = env(location.clone());

    let failed = run(&env, &["movie", "-u"]).await;

    assert_eq!(failed.code, 2);
    assert!(
        failed.out.contains("earlier version of CineVault"),
        "{}",
        failed.out
    );
    assert!(
        failed.out.contains("Start the CineVault app once"),
        "{}",
        failed.out
    );
    assert!(failed.out.contains("library.db"), "{}", failed.out);
    // Nothing was moved, opened or reset.
    assert_eq!(names(), before);
    assert_eq!(
        std::fs::read(location.library_db()).unwrap(),
        old_library_bytes()
    );
    assert_eq!(
        std::fs::read_to_string(location.settings_file()).unwrap(),
        old_settings
    );

    // Old settings alone (the app set the library aside but stopped before the settings step)
    // are refused the same way.
    std::fs::remove_file(location.library_db()).unwrap();
    let failed = run(&env, &["movie", "-u"]).await;
    assert_eq!(failed.code, 2);
    assert!(failed.out.contains("settings.json"), "{}", failed.out);
    assert_eq!(
        std::fs::read_to_string(location.settings_file()).unwrap(),
        old_settings
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn json_output_never_carries_a_users_key() {
    let temp = TempDir::new().unwrap();
    let location = location(temp.path());
    library(temp.path(), &location);
    let secrets = Arc::new(SecretStore::in_memory());
    secrets.set("tmdb.api_key", TMDB_KEY).unwrap();
    secrets.set("trakt.client_id", TRAKT_ID).unwrap();
    // A provider service whose user saved the dummy keys, with no provider to call.
    let make = move |options: BootOptions| {
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        resolver.set_user(key::TMDB, API_KEY, TMDB_KEY).unwrap();
        resolver
            .set_user(key::TRAKT, "client_id", TRAKT_ID)
            .unwrap();
        options.with_providers(ProviderCredentialsService::with_resolver(
            resolver,
            ProviderRegistry::new(),
        ))
    };
    let env = Env::with_services(location, secrets, make);

    let everything = [
        run(&env, &["providers", "--json"]).await,
        run(&env, &["providers"]).await,
        run(&env, &["movie", "-u", "-n", "-r", "--dry-run", "--json"]).await,
        run(&env, &["movie", "-n", "--dry-run"]).await,
        run(&env, &["movie", "-u", "-n", "--json"]).await,
    ];
    for output in &everything {
        assert!(!output.out.contains("builtin") && !output.out.contains("built-in"));
        for secret in [TMDB_KEY, TRAKT_ID] {
            assert!(
                !output.out.contains(secret),
                "stdout leaks a key: {}",
                output.out
            );
            assert!(
                !output.err.contains(secret),
                "stderr leaks a key: {}",
                output.err
            );
        }
    }
    // They do say how important each provider is and where it stands.
    let providers = everything[0].json();
    let tmdb = providers
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["provider"] == "tmdb")
        .expect("tmdb is listed");
    assert_eq!(tmdb["importance"], "required");
    assert_eq!(tmdb["state"], "active");
    assert_eq!(tmdb["active"], true);
    let tvdb = providers
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["provider"] == "tvdb")
        .expect("tvdb is listed");
    assert_eq!(tvdb["importance"], "recommended");
    assert_eq!(tvdb["state"], "missing");
    assert!(everything[1].out.contains("required"));
    let report = everything[2].json();
    let credentials = report["credentials"].as_array().unwrap();
    assert!(credentials.iter().any(|c| c["provider"] == "tmdb"
        && c["state"] == "active"
        && c["importance"] == "required"));
    // The scrape could not run (no provider is registered), and the report says so.
    assert_eq!(everything[4].json()["steps"][1]["status"], "unavailable");
    // Only these fields are ever reported for a provider.
    for credential in credentials {
        let mut names: Vec<_> = credential.as_object().unwrap().keys().cloned().collect();
        names.sort();
        assert_eq!(names, ["active", "importance", "name", "provider", "state"]);
    }
}

// ---- the real binary -----------------------------------------------------------------------

/// A copy of the built `cinevault` program in a portable install: its data lives beside it, so
/// the test never touches the user's own library.
struct Portable {
    exe: PathBuf,
    location: DataLocation,
    _temp: TempDir,
}

fn portable() -> Portable {
    let temp = TempDir::new().unwrap();
    let app = temp.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let name = if cfg!(windows) {
        "cinevault.exe"
    } else {
        "cinevault"
    };
    let exe = app.join(name);
    std::fs::copy(env!("CARGO_BIN_EXE_cinevault"), &exe).unwrap();
    std::fs::write(app.join("portable"), "").unwrap();
    let inputs = LocationInputs {
        exe_dir: app,
        user_data_base: temp.path().join("userdata"),
        user_config_base: temp.path().join("userconfig"),
    };
    let location = DataLocation::resolve(&inputs);
    Portable {
        exe,
        location,
        _temp: temp,
    }
}

async fn spawn(exe: &Path, args: &[&str]) -> std::process::Output {
    let (exe, args): (PathBuf, Vec<String>) = (
        exe.to_owned(),
        args.iter().map(|a| (*a).to_owned()).collect(),
    );
    tokio::task::spawn_blocking(move || {
        Command::new(exe)
            .args(args)
            .env_remove("CINEVAULT_LOG")
            .env_remove("RUST_LOG")
            .output()
            .expect("the program starts")
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_program_reports_its_version_and_refuses_nonsense_with_code_2() {
    let install = portable();
    let version = spawn(&install.exe, &["--version"]).await;
    assert_eq!(version.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("cinevault "));

    let help = spawn(&install.exe, &["movie", "--help"]).await;
    let text = String::from_utf8_lossy(&help.stdout).into_owned();
    for flag in [
        "--update-all",
        "--scrape-new",
        "--rename-new",
        "--dry-run",
        "--json",
        "--query",
    ] {
        assert!(text.contains(flag), "{flag} missing from:\n{text}");
    }

    let unknown = spawn(&install.exe, &["movie", "--no-such-flag"]).await;
    assert_eq!(unknown.status.code(), Some(2));
    // Export is gone: its flag is a usage error like any other unknown flag.
    assert!(!text.contains("--export"), "{text}");
    let export = spawn(&install.exe, &["movie", "--export"]).await;
    assert_eq!(export.status.code(), Some(2));
    let both = spawn(&install.exe, &["movie", "--rename-new", "--rename-all"]).await;
    assert_eq!(both.status.code(), Some(2));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_program_runs_a_dry_run_alone_and_through_the_running_app() {
    let install = portable();
    let library_root = install.location.root().parent().unwrap().to_owned();
    let movies = library(&library_root, &install.location);

    // Alone: the program opens the library in its own process.
    let alone = spawn(&install.exe, &["movie", "-u", "--json"]).await;
    assert_eq!(
        alone.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&alone.stderr)
    );
    let report: Value = serde_json::from_slice(&alone.stdout).unwrap();
    assert_eq!(report["forwarded"], false);
    let planned = spawn(
        &install.exe,
        &["movie", "--rename-all", "--dry-run", "--json"],
    )
    .await;
    assert_eq!(
        planned.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );
    let report: Value = serde_json::from_slice(&planned.stdout).unwrap();
    assert_eq!(report["steps"][0]["items"].as_array().unwrap().len(), 2);
    assert_eq!(folders(&movies).len(), 2);
    assert!(folders(&movies).contains(&"heat.1995.720p".to_owned()));

    // With the app running: it holds the library, so the program hands the batch over.
    let services = AppServices::boot(
        BootOptions::new(install.location.clone(), Arc::new(SecretStore::in_memory()))
            .without_providers(),
    )
    .unwrap();
    let engine = BatchEngine::new(Arc::clone(&services), EngineOptions::default());
    let server = IpcServer::start(engine, &install.location).unwrap();
    let forwarded = spawn(
        &install.exe,
        &["movie", "--rename-all", "--dry-run", "--json"],
    )
    .await;
    assert_eq!(
        forwarded.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&forwarded.stderr)
    );
    let report: Value = serde_json::from_slice(&forwarded.stdout).unwrap();
    assert_eq!(report["forwarded"], true);
    assert_eq!(report["steps"][0]["items"].as_array().unwrap().len(), 2);

    // The app really did the work: a rename through the program moves the folders, and the app's
    // library holds the new paths.
    let renamed = spawn(&install.exe, &["movie", "--rename-all", "--json"]).await;
    assert_eq!(
        renamed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&renamed.stderr)
    );
    let report: Value = serde_json::from_slice(&renamed.stdout).unwrap();
    assert_eq!(report["forwarded"], true);
    assert!(!folders(&movies).contains(&"heat.1995.720p".to_owned()));
    let moved = services
        .db()
        .read(|c| list_items(c, &ItemQuery::new(ItemKind::Movie), None, 10))
        .unwrap()
        .items;
    assert!(moved.iter().all(|i| !i.path.ends_with("heat.1995.720p")));

    // The app quits: the program works alone again.
    drop(server);
    let alone_again = spawn(
        &install.exe,
        &["movie", "--rename-all", "--dry-run", "--json"],
    )
    .await;
    assert_eq!(alone_again.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&alone_again.stdout).unwrap();
    assert_eq!(report["forwarded"], false);
}

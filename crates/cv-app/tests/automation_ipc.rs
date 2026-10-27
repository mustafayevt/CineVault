//! The local channel between the command line and a running app: a batch sent over
//! the named pipe (Windows) or Unix socket (macOS) runs on the app's own services and its report
//! comes back; a caller without the token, or with another protocol version, is refused; a left-over
//! endpoint file of a crashed app means "no app is running".

use std::path::Path;
use std::sync::{Arc, Mutex};

use cv_app::automation::ipc::{
    ENDPOINT_FILE, Endpoint, ForwardError, IpcServer, endpoint_file, forward, forward_to,
    read_endpoint,
};
use cv_app::automation::{
    BatchEngine, BatchRequest, EngineOptions, EntityKind, Progress, Scope, Step, StepStatus,
};
use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::{ItemQuery, list_items};
use cv_domain::{ItemKind, SourceKind};
use tempfile::TempDir;

fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

struct App {
    temp: TempDir,
    location: DataLocation,
    services: Arc<AppServices>,
    server: IpcServer,
}

fn app() -> App {
    let temp = TempDir::new().unwrap();
    let location = location(temp.path());
    let options =
        BootOptions::new(location.clone(), Arc::new(SecretStore::in_memory())).without_providers();
    let services = AppServices::boot(options).expect("boot");
    services.scan().set_after_scan(Arc::new(|_| {}));
    let engine = BatchEngine::new(Arc::clone(&services), EngineOptions::default());
    let server = IpcServer::start(engine, &location).expect("the channel opens");
    App {
        temp,
        location,
        services,
        server,
    }
}

impl App {
    fn movies(&self) -> Vec<String> {
        self.services
            .db()
            .read(|c| list_items(c, &ItemQuery::new(ItemKind::Movie), None, 100))
            .unwrap()
            .items
            .into_iter()
            .map(|i| i.title)
            .collect()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_batch_sent_over_the_channel_runs_on_the_apps_services() {
    let app = app();
    let movies = app.temp.path().join("movies");
    std::fs::create_dir_all(movies.join("heat.1995.720p")).unwrap();
    std::fs::write(movies.join("heat.1995.720p/heat.1995.720p.mkv"), "x").unwrap();
    let text = movies.to_string_lossy().into_owned();
    app.services
        .db()
        .write(move |tx| insert_source(tx, SourceKind::Movie, &text))
        .unwrap();

    let mut request = BatchRequest::new(EntityKind::Movie);
    request.steps.update = true;
    request.steps.rename = Some(Scope::New);
    request.dry_run = true;
    let seen = Mutex::new(Vec::<Progress>::new());
    let report = forward(&app.location, &request, |p| seen.lock().unwrap().push(p))
        .await
        .expect("forwarded");
    // A dry run through the channel changes nothing in the app's library.
    assert!(report.forwarded && report.dry_run && report.error.is_none());
    assert!(app.movies().is_empty());

    // The real run adds the title to the app's own library.
    request.dry_run = false;
    request.steps.rename = None;
    let report = forward(&app.location, &request, |p| seen.lock().unwrap().push(p))
        .await
        .expect("forwarded");
    assert!(report.forwarded);
    assert_eq!(report.steps[0].step, Step::Update);
    assert_eq!(report.steps[0].status, StepStatus::Done);
    assert_eq!(app.movies().len(), 1);

    // Progress came back while the batch ran.
    let progress = seen.lock().unwrap();
    assert!(progress.iter().any(|p| p.step == Some(Step::Update)));
    assert!(progress.iter().any(|p| p.step.is_none()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_caller_without_the_token_is_refused_before_anything_runs() {
    let app = app();
    let mut endpoint = read_endpoint(&endpoint_file(&app.location)).unwrap();
    endpoint.token = "0".repeat(endpoint.token.len());
    let mut request = BatchRequest::new(EntityKind::Movie);
    request.steps.update = true;
    match forward_to(&endpoint, &request, |_| {}).await {
        Err(ForwardError::Refused(message)) => {
            assert!(message.contains("not allowed"), "{message}")
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    endpoint.token = String::new();
    assert!(matches!(
        forward_to(&endpoint, &request, |_| {}).await,
        Err(ForwardError::Refused(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_protocol_version_is_refused_with_a_clear_message() {
    let app = app();
    let mut endpoint = read_endpoint(&endpoint_file(&app.location)).unwrap();
    endpoint.protocol += 1;
    let request = BatchRequest::new(EntityKind::Movie);
    match forward_to(&endpoint, &request, |_| {}).await {
        Err(ForwardError::Refused(message)) => assert!(message.contains("protocol"), "{message}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_left_over_endpoint_file_means_no_app_is_running() {
    let temp = TempDir::new().unwrap();
    let location = location(temp.path());
    location.ensure_dirs().unwrap();
    let request = BatchRequest::new(EntityKind::Movie);

    // No file at all.
    assert!(matches!(
        forward(&location, &request, |_| {}).await,
        Err(ForwardError::NoRunningApp)
    ));

    // A file from an app that crashed: nobody listens at its address.
    let stale = Endpoint {
        protocol: cv_app::automation::ipc::PROTOCOL,
        address: if cfg!(windows) {
            r"\\.\pipe\cinevault-cli-does-not-exist".to_owned()
        } else {
            temp.path().join("gone.sock").to_string_lossy().into_owned()
        },
        token: "abc".to_owned(),
        pid: 1,
    };
    std::fs::write(
        endpoint_file(&location),
        serde_json::to_vec(&stale).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        forward(&location, &request, |_| {}).await,
        Err(ForwardError::NoRunningApp)
    ));

    // A damaged file reads as no app too.
    std::fs::write(endpoint_file(&location), "{ nope").unwrap();
    assert!(matches!(
        forward(&location, &request, |_| {}).await,
        Err(ForwardError::NoRunningApp)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_the_app_removes_the_endpoint_and_a_new_app_replaces_it() {
    let app = app();
    let file = endpoint_file(&app.location);
    assert_eq!(file.file_name().unwrap(), ENDPOINT_FILE);
    assert!(file.is_file());
    let first = app.server.endpoint().clone();
    // The token is not printed by the endpoint's Debug output.
    assert!(!format!("{first:?}").contains(&first.token));

    app.server.stop();
    assert!(!file.exists());
    let request = BatchRequest::new(EntityKind::Movie);
    assert!(matches!(
        forward(&app.location, &request, |_| {}).await,
        Err(ForwardError::NoRunningApp)
    ));

    // The next launch writes a new address and a new token.
    let engine = BatchEngine::new(Arc::clone(&app.services), EngineOptions::default());
    let second = IpcServer::start(engine, &app.location).unwrap();
    assert_ne!(second.endpoint().token, first.token);
    // Windows opens a pipe with a fresh name each launch; elsewhere the socket always has the same
    // path in the data folder, and the token alone tells the launches apart.
    if cfg!(windows) {
        assert_ne!(second.endpoint().address, first.address);
    } else {
        assert_eq!(second.endpoint().address, first.address);
    }
    let mut dry = BatchRequest::new(EntityKind::Movie);
    dry.steps.nfo = true;
    dry.dry_run = true;
    assert!(
        forward(&app.location, &dry, |_| {})
            .await
            .unwrap()
            .forwarded
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_command_lines_at_once_both_get_their_report() {
    let app = app();
    let mut dry = BatchRequest::new(EntityKind::Movie);
    dry.steps.nfo = true;
    dry.dry_run = true;
    let (a, b) = tokio::join!(
        forward(&app.location, &dry, |_| {}),
        forward(&app.location, &dry, |_| {})
    );
    assert!(a.unwrap().forwarded && b.unwrap().forwarded);
}

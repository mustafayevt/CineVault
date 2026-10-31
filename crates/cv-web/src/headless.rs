//! The headless `cinevault-server`: boots its own services over a data folder, holds the
//! folder's owner file and serves until it is asked to stop.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use cv_app::data_location::DataLocation;
use cv_app::services::{AppServices, BootOptions};
use cv_ipc::image::ImageContext;

use crate::auth;
use crate::server;
use crate::state::{Borrowed, Commands, DeleteGuard, Streams, UiAssets, WebState};

/// What the command line chose.
pub struct Options {
    pub ui_dir: PathBuf,
    pub bind: String,
    pub port: u16,
}

/// How long a graceful stop may take after the signal before the process exits anyway.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(15);

/// Runs the server until SIGTERM or Ctrl+C.
///
/// # Errors
///
/// When the data folder cannot be found or is held by another process, the library cannot be
/// opened, or the address cannot be bound.
pub async fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let Options { ui_dir, bind, port } = options;
    let location = DataLocation::resolve_for_process()
        .ok_or("CineVault could not find a folder for its data.")?;
    let _owner = match cv_app::owner::acquire(location.root(), "server") {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            let current = cv_app::owner::read_owner(location.root());
            let why = current.map_or_else(
                || "Another CineVault process is using this library.".to_owned(),
                |owner| owner.sentence(),
            );
            return Err(format!("{why} Stop it, or choose another data folder.").into());
        }
        Err(error) => return Err(error.into()),
    };
    let data_root = location.root().to_path_buf();
    server::clean_uploads(&data_root);
    let auth = Arc::new(auth::Auth::open(location.root())?);
    let (secrets, keys_locked) =
        cv_app::server_secrets::server_store(&data_root).map_err(std::io::Error::other)?;
    let services = AppServices::boot(BootOptions::new(location, Arc::new(secrets)))?;
    services.start();
    let images = Arc::new(ImageContext::from_services(&services));
    let progress = Arc::new(cv_ipc::jobs::ProgressHub::default());
    let notices = cv_ipc::notices::NoticeHub::from_app(Arc::clone(services.notices()));
    // Every command's state object, built once from the one set of services. The desktop builds
    // the same objects lazily; the server has no window and one lifetime.
    let commands = Arc::new(Commands::build(&services, &images, Borrowed::default()));

    let state = WebState {
        services,
        images,
        auth,
        progress,
        notices,
        streams: Arc::new(Streams::default()),
        commands,
        delete_guard: Arc::new(DeleteGuard::default()),
        data_root,
        bind: bind.clone(),
        port,
        keys_locked,
        trust_proxy: server::parse_trust_proxy(),
        ui: UiAssets::Folder(ui_dir.clone()),
        lan_only: false,
    };
    let _feed = server::wire(&state);
    let app = server::router(state.clone());

    let addr: SocketAddr = format!("{bind}:{port}").parse()?;
    println!(
        "cinevault-server on http://{addr}/ (ui from {})",
        ui_dir.display()
    );
    if !state.auth.has_password() {
        println!(
            "No password yet: open the address from a browser on this computer or your home network to create one."
        );
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let stopping = Arc::new(tokio::sync::Notify::new());
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown(state, Arc::clone(&stopping)));
    // A client that refuses to close must not keep the process (and the library's owner file)
    // alive forever: once the stop signal arrived, the graceful shutdown gets 15 seconds. The cap
    // starts at the signal, not at start-up, or the server would stop itself after 15 seconds.
    tokio::select! {
        result = server => result?,
        () = async {
            stopping.notified().await;
            tokio::time::sleep(SHUTDOWN_GRACE).await;
        } => {}
    }
    Ok(())
}

async fn shutdown(state: WebState, stopping: Arc<tokio::sync::Notify>) {
    wait_for_signal().await;
    // `notify_one` stores a permit, so the grace timer starts even if it is polled later.
    stopping.notify_one();
    // Dropping every stream ends the SSE responses, so axum can finish its graceful shutdown.
    state.streams.close_all();
    state.services.shutdown();
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("a SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// `cinevault-server passwd` and `cinevault-server passwd --reset` on the machine that holds the
/// data folder. This is the recovery path when the browser password is lost and no browser on the
/// home network can make a reset PIN.
///
/// # Errors
///
/// When the data folder cannot be found, the passwords do not match or the file cannot be
/// written.
pub fn passwd(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let location = DataLocation::resolve_for_process()
        .ok_or("CineVault could not find a folder for its data.")?;
    let path = location.root().join(auth::AUTH_FILE);
    if arguments.iter().any(|arg| arg == "--reset") {
        match std::fs::remove_file(&path) {
            Ok(()) => println!(
                "The password was cleared. Start the server and set a new one in a browser on your home network."
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                println!("There is no password to clear.");
            }
            Err(error) => return Err(error.into()),
        }
        return Ok(());
    }
    let auth = auth::Auth::open(location.root())?;
    if auth.has_password() {
        let current = rpassword::prompt_password("Current password: ")?;
        if !auth.check_password(&current) {
            return Err("That password is not right.".into());
        }
    }
    let password = rpassword::prompt_password("New password (at least 12 characters): ")?;
    let confirm = rpassword::prompt_password("Confirm the new password: ")?;
    if password != confirm {
        return Err("The passwords do not match.".into());
    }
    auth.set_password_unattended(&password)?;
    // Run as root (`sudo`, or `docker exec` without `-u`), the new file would belong to root with
    // mode 0600 and the server, which runs as the data folder's owner, could not read it.
    auth::give_to_folder_owner(&path, location.root());
    println!("Password set. Open the server in a browser and sign in.");
    Ok(())
}

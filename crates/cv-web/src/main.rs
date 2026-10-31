//! CineVault's browser server (`cinevault-server`).
//!
//! Boots the same `AppServices` the desktop app and the command line use, serves the built
//! interface and answers the typed command surface over HTTP. Everything but the command line
//! parsing lives in the `cv_web` library, which the desktop app's Browser access shares.

use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The shared initialiser reads CINEVAULT_LOG/RUST_LOG and writes through the redacting
    // writer, so a provider key or a session token can never reach a log line.
    cv_app::logging::init_with_default("info");

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().is_some_and(|arg| arg == "passwd") {
        return cv_web::headless::passwd(&arguments[1..]);
    }

    let mut ui_dir = PathBuf::from("ui/dist");
    let mut bind = std::env::var("CINEVAULT_WEB_BIND").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let mut port = std::env::var("CINEVAULT_WEB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8787_u16);
    let mut args = arguments.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ui" => ui_dir = PathBuf::from(args.next().ok_or("--ui needs a folder")?),
            "--bind" => bind = args.next().ok_or("--bind needs an address")?,
            "--port" => port = args.next().ok_or("--port needs a number")?.parse()?,
            other => return Err(format!("unknown argument {other}").into()),
        }
    }
    cv_web::headless::run(cv_web::headless::Options { ui_dir, bind, port }).await
}

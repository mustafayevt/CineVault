//! `cinevault` command-line interface.

use std::io::Write as _;
use std::process::ExitCode;

use cinevault_cli::{Cli, Env, execute};
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    cv_app::logging::init_with_default("warn");
    let cli = Cli::parse();
    let env = Env::from_process()?;
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let code = execute(cli, &env, &mut stdout, &mut stderr).await;
    let _ = stdout.flush();
    Ok(ExitCode::from(code))
}

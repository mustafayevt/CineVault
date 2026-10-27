//! A stand-in for the app, used by the tests of `indexer_service`: starts the service program the
//! way the app does, prints `pid=<service pid>` and then waits. The test ends this process the hard
//! way and checks that the service goes with it.
//!
//! Usage: `indexer_parent_harness <program> <install folder> <data folder>`.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use cv_torrent::cancel::CancelToken;
use cv_torrent::indexer_service::{IndexerProcess, StartParams};
use cv_torrent::source::client_builder;

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(program), Some(install), Some(data)) = (args.next(), args.next(), args.next()) else {
        eprintln!("usage: indexer_parent_harness <program> <install folder> <data folder>");
        std::process::exit(2);
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let http = client_builder().build().expect("http client");
    let running = runtime.block_on(async {
        IndexerProcess::start(
            &StartParams {
                program: &program,
                install_dir: &install,
                data_dir: &data,
                solver_url: None,
                ready_timeout: Duration::from_secs(20),
            },
            &http,
            &CancelToken::new(),
        )
        .await
    });
    let running = running.expect("the service starts");
    println!("pid={}", running.pid());
    std::io::stdout().flush().expect("flush");
    // Held until this process is ended from outside: nothing here stops the service.
    std::thread::sleep(Duration::from_secs(3600));
    drop(running);
}

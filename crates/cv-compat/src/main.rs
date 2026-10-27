//! Prints the per-fixture golden-file report, or updates the golden files.
//!
//! `cargo run -p cv-compat --bin compat-report -- [--all] [--require-complete] [--update] [--root <dir>]`
//!
//! `--update` writes CineVault's current output over the expected files (review the diff before
//! committing). Exit code 1 when a fixture fails (or, with `--require-complete`, is pending); 2
//! when the fixture tree cannot be loaded or updated.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut show_pending = false;
    let mut require_complete = false;
    let mut update = false;
    let mut root = cv_compat::default_root();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--all" => show_pending = true,
            "--require-complete" => require_complete = true,
            "--update" => update = true,
            "--root" => match args.next() {
                Some(dir) => root = PathBuf::from(dir),
                None => {
                    eprintln!("--root needs a folder");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("unknown argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }

    let load = || cv_compat::load(&root);
    let mut suite = match load() {
        Ok(suite) => suite,
        Err(err) => {
            eprintln!("cannot load fixtures: {err}");
            return ExitCode::from(2);
        }
    };
    if update {
        match cv_compat::update_expected(&suite, &cv_compat::registered()) {
            Ok(changed) => println!("updated {changed} expected file(s)"),
            Err(err) => {
                eprintln!("cannot update the golden files: {err}");
                return ExitCode::from(2);
            }
        }
        suite = match load() {
            Ok(suite) => suite,
            Err(err) => {
                eprintln!("cannot reload fixtures: {err}");
                return ExitCode::from(2);
            }
        };
    }
    let report = cv_compat::run_suite(&suite, &cv_compat::registered());
    print!("{}", report.render(show_pending));
    let ok = if require_complete {
        report.is_complete()
    } else {
        report.is_green()
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

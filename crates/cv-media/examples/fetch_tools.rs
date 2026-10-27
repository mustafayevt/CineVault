//! Development helper: downloads the pinned external tools into a git-ignored cache.
//!
//! ```text
//! bun run tools:fetch                      # MediaInfo, FFmpeg and the indexer service, into <repo>/.dev-tools
//! bun run tools:fetch -- mediainfo ffmpeg  # only these packages
//! bun run tools:fetch -- --help           # the packages and options
//! bun run tools:fetch -- --dir D:\tools    # another folder
//! bun run tools:fetch -- --force           # download again even if current
//! ```
//!
//! It uses the same manager, manifest and checksum checks as the app, so the cache looks exactly
//! like the app's `tools` folder. Later media tests point `ToolsManager::new(dev_cache_dir())`
//! at it. Downloaded binaries stay out of git (`/.dev-tools/` is ignored).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use cv_media::tools::{Cancel, InstallPhase, Package, ToolState, ToolsManager, dev_cache_dir};

fn main() -> ExitCode {
    let mut dir = dev_cache_dir();
    let mut force = false;
    let mut packages: Vec<Package> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => match args.next() {
                Some(path) => dir = PathBuf::from(path),
                None => return usage("--dir needs a folder"),
            },
            "--force" => force = true,
            "--help" | "-h" => {
                println!("{}", usage_text());
                return ExitCode::SUCCESS;
            }
            name => match Package::ALL.into_iter().find(|p| p.as_str() == name) {
                Some(package) => packages.push(package),
                None => return usage(&format!("unknown package {name:?}")),
            },
        }
    }
    if packages.is_empty() {
        packages = Package::ALL.to_vec();
    }

    let manager = ToolsManager::new(&dir);
    println!("Tools folder: {}", dir.display());
    let mut failed = false;
    for package in packages {
        let current = manager.status().into_iter().any(|status| {
            status.id.package() == package
                && status.state == ToolState::Ready
                && !status.update_available
        });
        if current && !force {
            println!("{package}: already installed");
            continue;
        }
        let mut last_percent = u64::MAX;
        let result = manager.install(package, &Cancel::new(), &mut |progress| {
            if progress.phase == InstallPhase::Downloading && progress.total > 0 {
                let percent = progress.downloaded * 100 / progress.total;
                if percent != last_percent && percent % 10 == 0 {
                    last_percent = percent;
                    print!("\r{package}: downloading {percent}%");
                    let _ = std::io::stdout().flush();
                }
            }
        });
        match result {
            Ok(()) => println!("\r{package}: installed and verified"),
            Err(error) => {
                println!("\r{package}: FAILED: {error}");
                failed = true;
            }
        }
    }
    for status in manager.status() {
        println!(
            "{:<10} {:<10} {}",
            status.id,
            status.version.as_deref().unwrap_or("-"),
            status
                .path
                .as_ref()
                .map_or_else(String::new, |p| p.display().to_string()),
        );
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// The usage line, naming every package the manifest has.
fn usage_text() -> String {
    let names: Vec<&str> = Package::ALL.iter().map(|p| p.as_str()).collect();
    format!(
        "usage: fetch_tools [--dir <folder>] [--force] [{} ...]
Without a package name every package is fetched.",
        names.join("|")
    )
}

fn usage(problem: &str) -> ExitCode {
    eprintln!(
        "{problem}
{}",
        usage_text()
    );
    ExitCode::from(2)
}

//! Finding out which version of a user-chosen program is in use, by asking it.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::manifest::ToolId;

/// How long a program gets to print its version.
const ASK_TIMEOUT: Duration = Duration::from_secs(10);
/// A version banner is a few lines; anything longer is not one.
const MAX_OUTPUT: u64 = 64 * 1024;

/// The version in the text a tool prints for its version flag, or `None` when it is not
/// recognisable.
///
/// * FFmpeg prints `ffmpeg version 8.1.2-essentials_build-www.gyan.dev Copyright ...`; the first
///   word after `version` is the version.
///
/// A library has no output to parse and always gives `None`.
#[must_use]
pub fn parse_version(tool: ToolId, output: &str) -> Option<String> {
    match tool {
        ToolId::Ffmpeg => output.lines().find_map(|line| {
            let mut words = line.split_whitespace();
            let program = words.next()?;
            (program == "ffmpeg").then_some(())?;
            (words.next()? == "version").then_some(())?;
            words.next().map(str::to_owned)
        }),
        ToolId::MediaInfo | ToolId::IndexerService => None,
    }
}

/// The version of the program at `path`, found by running it.
pub(crate) fn ask_version(tool: ToolId, path: &Path) -> Option<String> {
    let flag = match tool {
        ToolId::Ffmpeg => "-version",
        ToolId::MediaInfo | ToolId::IndexerService => return None,
    };
    let output = run_capture(path, &[flag], ASK_TIMEOUT)?;
    parse_version(tool, &output)
}

/// Runs a program and returns what it printed, or `None` if it did not start or did not finish
/// in time.
fn run_capture(path: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    let mut command = Command::new(path);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashes up.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let mut bytes = Vec::new();
        let _ = (&mut stdout).take(MAX_OUTPUT).read_to_end(&mut bytes);
        text.push_str(&String::from_utf8_lossy(&bytes));
        text
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    reader.join().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg_banner_gives_the_word_after_version() {
        let banner = "ffmpeg version 8.1.2-essentials_build-www.gyan.dev Copyright (c) 2000-2026 the FFmpeg developers\nbuilt with gcc 15.2.0\n";
        assert_eq!(
            parse_version(ToolId::Ffmpeg, banner).as_deref(),
            Some("8.1.2-essentials_build-www.gyan.dev")
        );
    }

    #[test]
    fn ffmpeg_output_without_a_banner_gives_nothing() {
        assert_eq!(parse_version(ToolId::Ffmpeg, "Usage: something else"), None);
        assert_eq!(parse_version(ToolId::Ffmpeg, ""), None);
    }

    #[test]
    fn a_library_has_no_version_output() {
        assert_eq!(parse_version(ToolId::MediaInfo, "26.10"), None);
    }

    #[test]
    fn a_missing_program_gives_no_version() {
        let missing = std::env::temp_dir().join("cinevault-no-such-program-xyz");
        assert_eq!(ask_version(ToolId::Ffmpeg, &missing), None);
    }
}

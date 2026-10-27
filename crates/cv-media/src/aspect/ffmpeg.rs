//! Running FFmpeg for the dark level and the crop samples, and reading what it prints.

use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regex::Regex;

use super::AspectError;
use crate::tools::Cancel;

/// The longest one FFmpeg run may take (a sample is a few seconds of video, but a file on a
/// sleeping network drive can be slow to open).
const RUN_TIMEOUT: Duration = Duration::from_secs(120);
/// The most output kept from one run.
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;
/// FFmpeg's default black threshold for 8-bit video.
pub(super) const DEFAULT_DARK_LEVEL: u32 = 24;

/// A crop rectangle reported by FFmpeg's `cropdetect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Crop {
    pub width: u32,
    pub height: u32,
}

/// The black threshold for `cropdetect`, from the darkest luma of the first frame
/// (`lavfi.signalstats.YMIN`). Black bars sit at the darkest level of the file (16 for
/// limited-range video, 0 for full range), so the threshold is a little above it; it is kept
/// between FFmpeg's own default and 40 so a bright first frame cannot make dark scenes count as
/// bars.
pub(super) fn dark_level_from_output(output: &str) -> u32 {
    static YMIN: OnceLock<Regex> = OnceLock::new();
    let regex = YMIN.get_or_init(|| {
        Regex::new(r"lavfi\.signalstats\.YMIN=(\d+(?:\.\d+)?)").expect("valid pattern")
    });
    regex
        .captures(output)
        .and_then(|c| c[1].parse::<f64>().ok())
        .map_or(DEFAULT_DARK_LEVEL, |ymin| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let level = ymin.clamp(0.0, 255.0).round() as u32 + 8;
            level.clamp(DEFAULT_DARK_LEVEL, 40)
        })
}

/// The last `crop=W:H:X:Y` line of a `cropdetect` run. With `reset=0` FFmpeg keeps the largest
/// area seen so far, so the last line is the picture area of the whole sample. `None` when the
/// sample produced no usable rectangle (an all-black stretch).
pub(super) fn last_crop(output: &str) -> Option<Crop> {
    static CROP: OnceLock<Regex> = OnceLock::new();
    let regex =
        CROP.get_or_init(|| Regex::new(r"crop=(\d+):(\d+):(\d+):(\d+)").expect("valid pattern"));
    let last = regex.captures_iter(output).last()?;
    let width: u32 = last[1].parse().ok()?;
    let height: u32 = last[2].parse().ok()?;
    // Slivers are noise, not pictures.
    (width >= 16 && height >= 16).then_some(Crop { width, height })
}

/// Arguments that read the darkest luma of the first frame.
pub(super) fn dark_level_args(file: &Path) -> Vec<OsString> {
    let mut args = base_args();
    args.extend(["-ss", "0", "-i"].map(OsString::from));
    args.push(file.as_os_str().to_owned());
    args.extend(
        [
            "-vf",
            "signalstats,metadata=print",
            "-vframes",
            "1",
            "-f",
            "null",
            "-",
        ]
        .map(OsString::from),
    );
    args
}

/// Arguments that run `cropdetect` over `length` seconds from `start`. The seek lands on the
/// keyframe before `start` without decoding up to it, which is what keeps a sample fast on a
/// large file.
pub(super) fn crop_args(file: &Path, start: f64, length: f64, dark_level: u32) -> Vec<OsString> {
    let mut args = base_args();
    args.extend(
        [
            "-noaccurate_seek".to_owned(),
            "-t".to_owned(),
            format!("{length:.3}"),
            "-ss".to_owned(),
            format!("{start:.3}"),
            "-i".to_owned(),
        ]
        .map(OsString::from),
    );
    args.push(file.as_os_str().to_owned());
    args.extend(
        [
            "-vf".to_owned(),
            format!("cropdetect=limit={dark_level}:round=2:reset=0"),
            "-f".to_owned(),
            "null".to_owned(),
            "-".to_owned(),
        ]
        .map(OsString::from),
    );
    args
}

fn base_args() -> Vec<OsString> {
    ["-hide_banner", "-nostats", "-nostdin", "-an", "-dn", "-sn"]
        .map(OsString::from)
        .to_vec()
}

/// Runs FFmpeg and returns everything it printed (log and metadata). A run that fails or times
/// out still returns what it printed; only a program that cannot start, or a cancel, is an
/// error.
pub(crate) fn run(
    ffmpeg: &Path,
    args: &[OsString],
    cancel: &Cancel,
) -> Result<String, AspectError> {
    let mut command = Command::new(ffmpeg);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashes up.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|source| AspectError::Start {
        program: ffmpeg.to_path_buf(),
        source,
    })?;
    let readers: Vec<_> = [
        child.stdout.take().map(|s| spawn_reader(Box::new(s))),
        child.stderr.take().map(|s| spawn_reader(Box::new(s))),
    ]
    .into_iter()
    .flatten()
    .collect();

    let started = Instant::now();
    let mut cancelled = false;
    loop {
        if cancel.is_cancelled() {
            cancelled = true;
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < RUN_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                tracing::warn!("FFmpeg did not finish in time and was stopped");
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let mut output = String::new();
    for reader in readers {
        if let Ok(text) = reader.join() {
            output.push_str(&text);
            output.push('\n');
        }
    }
    if cancelled {
        Err(AspectError::Cancelled)
    } else {
        Ok(output)
    }
}

fn spawn_reader(mut pipe: Box<dyn Read + Send>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        // Keep draining after the cap so FFmpeg is never blocked on a full pipe.
        let _ = (&mut pipe).take(MAX_OUTPUT).read_to_end(&mut bytes);
        let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_crop_line_is_the_picture_area() {
        let output = "\
[Parsed_cropdetect_0 @ 000001] x1:0 x2:639 y1:46 y2:313 w:640 h:268 x:0 y:46 pts:0 t:0.000000 limit:0.094118 crop=640:268:0:46
[Parsed_cropdetect_0 @ 000001] x1:0 x2:639 y1:44 y2:315 w:640 h:272 x:0 y:44 pts:2 t:0.200000 limit:0.094118 crop=640:272:0:44
frame=10 fps=0.0 q=-0.0 Lsize=N/A time=00:00:00.40
";
        assert_eq!(
            last_crop(output),
            Some(Crop {
                width: 640,
                height: 272
            })
        );
    }

    #[test]
    fn no_crop_line_or_a_sliver_gives_nothing() {
        assert_eq!(last_crop("Input #0, matroska,webm\nVideo: h264"), None);
        assert_eq!(last_crop("crop=640:2:0:0"), None);
        assert_eq!(last_crop("crop=0:0:0:0"), None);
    }

    #[test]
    fn the_dark_level_follows_the_darkest_luma_within_bounds() {
        let print = |ymin: &str| {
            format!(
                "frame:0 pts:0 pts_time:0\nlavfi.signalstats.YLOW=16\nlavfi.signalstats.YMIN={ymin}\nlavfi.signalstats.YMAX=235\n"
            )
        };
        // Limited-range black bars: FFmpeg's default.
        assert_eq!(dark_level_from_output(&print("16")), 24);
        // Full-range black bars: the default is still the floor.
        assert_eq!(dark_level_from_output(&print("0")), 24);
        // A little above the darkest level.
        assert_eq!(dark_level_from_output(&print("20")), 28);
        // A bright first frame cannot push the threshold into the picture.
        assert_eq!(dark_level_from_output(&print("120")), 40);
        // Nothing printed: the default.
        assert_eq!(dark_level_from_output(""), 24);
    }

    #[test]
    fn crop_arguments_seek_without_decoding_and_limit_the_read() {
        let args: Vec<String> = crop_args(Path::new("movie.mkv"), 12.5, 2.0, 24)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let joined = args.join(" ");
        assert!(joined.contains("-noaccurate_seek -t 2.000 -ss 12.500 -i movie.mkv"));
        assert!(joined.contains("-vf cropdetect=limit=24:round=2:reset=0"));
        assert!(joined.ends_with("-f null -"));
        assert!(args.iter().any(|a| a == "-an") && args.iter().any(|a| a == "-sn"));
    }
}

//! Checksums of video files: CRC-32 and a perceptual hash.
//!
//! Spec: `media-info` (Checksums); the stored keys are `crc32` (8 uppercase hex
//! digits) and `phash`.
//!
//! * [`crc32_of_file`] streams the whole file, so it is as slow as reading the file.
//! * [`perceptual_hash_of_file`] asks FFmpeg for one 32x32 grey frame from the middle of the
//!   video (the middle of the whole timeline when the video is stacked) and hashes it with
//!   [`phash_of_gray`]; the hash is 16 lowercase hex digits.
//! * [`calculate_checksums`] runs the chosen kinds for one [`MediaFile`] and stores the results
//!   in its `checksums` map. A value already there is kept unless `force` is set.

mod crc32;
mod phash;

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cv_domain::MediaFile;

pub use crc32::{Crc32, crc32_hex, crc32_of_file};
pub use phash::{
    FRAME_BYTES, FRAME_SIDE, hamming_distance, parse_phash_hex, phash_hex, phash_of_gray,
};

use crate::disc::is_image_path;
use crate::tools::{Cancel, ToolError, ToolId, ToolsManager};

/// Key of the CRC-32 in [`MediaFile::checksums`].
pub const CRC32_KEY: &str = "crc32";
/// Key of the perceptual hash in [`MediaFile::checksums`].
pub const PHASH_KEY: &str = "phash";

const FFMPEG_TIMEOUT: Duration = Duration::from_secs(120);

/// Which checksums to calculate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumKind {
    Crc32,
    Phash,
    /// Both (the CLI's `all`).
    All,
}

impl ChecksumKind {
    /// Parses the CLI/API `type` value (`crc32`, `phash`, `all`, any case).
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "crc32" => Some(Self::Crc32),
            "phash" => Some(Self::Phash),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    const fn wants_crc32(self) -> bool {
        matches!(self, Self::Crc32 | Self::All)
    }

    const fn wants_phash(self) -> bool {
        matches!(self, Self::Phash | Self::All)
    }
}

/// Why a checksum was not calculated. Every message is ready to show.
#[derive(Debug, thiserror::Error)]
pub enum ChecksumError {
    #[error("The file {} could not be read: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// FFmpeg is not installed or its path is broken (perceptual hash only).
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error("FFmpeg could not be started ({}): {source}", .program.display())]
    Start {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("FFmpeg could not read a picture from {}.", .0.display())]
    NoFrame(PathBuf),
    #[error("Disc images have no single picture to hash, so {} was skipped.", .0.display())]
    DiscImage(PathBuf),
    #[error("The calculation was cancelled.")]
    Cancelled,
}

/// Where in the video the perceptual-hash frame is taken: `(part index, seconds into the part)`
/// for the middle of the combined timeline of `durations`. Parts without a length are ignored;
/// with no lengths at all the answer is the start of the first part.
#[must_use]
pub fn hash_frame_position(durations: &[f64]) -> (usize, f64) {
    let total: f64 = durations.iter().filter(|d| **d > 0.0).sum();
    let mut left = total / 2.0;
    for (index, &length) in durations.iter().enumerate() {
        if length > 0.0 {
            if left < length {
                return (index, left);
            }
            left -= length;
        }
    }
    (0, 0.0)
}

fn frame_args(file: &Path, at: f64) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostats", "-nostdin", "-an", "-dn", "-sn"]
        .map(OsString::from)
        .to_vec();
    args.extend(["-ss".to_owned(), format!("{at:.3}"), "-i".to_owned()].map(OsString::from));
    args.push(file.as_os_str().to_owned());
    args.extend(
        [
            "-frames:v",
            "1",
            "-vf",
            "scale=32:32:flags=area,format=gray",
            "-f",
            "rawvideo",
            "-",
        ]
        .map(OsString::from),
    );
    args
}

/// Runs FFmpeg and returns what it wrote to stdout.
fn run_for_frame(
    ffmpeg: &Path,
    args: &[OsString],
    cancel: &Cancel,
) -> Result<Vec<u8>, ChecksumError> {
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
    let mut child = command.spawn().map_err(|source| ChecksumError::Start {
        program: ffmpeg.to_path_buf(),
        source,
    })?;
    let stdout = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = (&mut pipe).take(1024 * 1024).read_to_end(&mut bytes);
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
            bytes
        })
    });
    let stderr = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        })
    });
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
            Ok(None) if started.elapsed() < FFMPEG_TIMEOUT => {
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
    let bytes = stdout.and_then(|h| h.join().ok()).unwrap_or_default();
    if let Some(handle) = stderr {
        let _ = handle.join();
    }
    if cancelled {
        Err(ChecksumError::Cancelled)
    } else {
        Ok(bytes)
    }
}

/// The perceptual hash of a video (16 lowercase hex digits). `parts` are `(path, seconds)` of
/// the video files in playing order; a plain movie has one.
///
/// # Errors
/// [`ChecksumError::DiscImage`] for a disc image, [`ChecksumError::Tool`] when FFmpeg is not
/// installed, [`ChecksumError::NoFrame`] when FFmpeg gave no picture,
/// [`ChecksumError::Cancelled`] when `cancel` is set, [`ChecksumError::Start`] when FFmpeg
/// cannot start.
pub fn perceptual_hash_of_file(
    tools: &ToolsManager,
    parts: &[(PathBuf, f64)],
    cancel: &Cancel,
) -> Result<String, ChecksumError> {
    let Some(first) = parts.first() else {
        return Err(ChecksumError::NoFrame(PathBuf::new()));
    };
    if let Some((image, _)) = parts.iter().find(|(p, _)| is_image_path(p)) {
        return Err(ChecksumError::DiscImage(image.clone()));
    }
    let durations: Vec<f64> = parts.iter().map(|(_, d)| *d).collect();
    let (index, at) = hash_frame_position(&durations);
    let path = parts.get(index).map_or(&first.0, |p| &p.0);
    let ffmpeg = tools.resolve(ToolId::Ffmpeg)?;
    let _reading = cv_fs::reading(path);
    let frame = run_for_frame(&ffmpeg.path, &frame_args(path, at), cancel)?;
    phash_of_gray(&frame)
        .map(phash_hex)
        .ok_or_else(|| ChecksumError::NoFrame(path.clone()))
}

/// Calculates the chosen checksums for one video file and stores them in `file.checksums`.
/// Values already stored are kept unless `force` is set. Returns the keys that were calculated.
///
/// A failed perceptual hash does not discard a CRC-32 calculated in the same call; the first
/// error is returned after both were attempted.
///
/// # Errors
/// The first [`ChecksumError`] met.
pub fn calculate_checksums(
    tools: &ToolsManager,
    file: &mut MediaFile,
    kind: ChecksumKind,
    force: bool,
    cancel: &Cancel,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Vec<&'static str>, ChecksumError> {
    let path = Path::new(&file.directory).join(&file.filename);
    let mut done = Vec::new();
    let mut first_error = None;
    if kind.wants_crc32() && (force || !file.checksums.contains_key(CRC32_KEY)) {
        match crc32_of_file(&path, cancel, progress) {
            Ok(value) => {
                file.checksums.insert(CRC32_KEY.to_owned(), value);
                done.push(CRC32_KEY);
            }
            Err(error) => first_error = Some(error),
        }
    }
    if kind.wants_phash()
        && (force || !file.checksums.contains_key(PHASH_KEY))
        && !matches!(first_error, Some(ChecksumError::Cancelled))
    {
        let parts = [(path, f64::from(file.duration_secs))];
        match perceptual_hash_of_file(tools, &parts, cancel) {
            Ok(value) => {
                file.checksums.insert(PHASH_KEY.to_owned(), value);
                done.push(PHASH_KEY);
            }
            Err(error) => first_error = first_error.or(Some(error)),
        }
    }
    first_error.map_or(Ok(done), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_published_check_values() {
        assert_eq!(crc32_hex(b"123456789"), "CBF43926");
        assert_eq!(crc32_hex(b""), "00000000");
        assert_eq!(
            crc32_hex(b"The quick brown fox jumps over the lazy dog"),
            "414FA339"
        );
    }

    #[test]
    fn crc32_is_eight_uppercase_digits() {
        let hex = crc32_hex(&[0x00]);
        assert_eq!(hex, "D202EF8D");
        let mut crc = Crc32::new();
        crc.update(b"a");
        assert_eq!(crc.hex(), "E8B7BE43");
        // A value with leading zeros keeps them.
        assert_eq!(crc32_hex(b""), "00000000");
    }

    #[test]
    fn crc32_in_pieces_equals_crc32_at_once() {
        let mut crc = Crc32::new();
        crc.update(b"1234");
        crc.update(b"56789");
        assert_eq!(crc.hex(), "CBF43926");
    }

    #[test]
    fn the_hash_frame_is_the_middle_of_the_combined_timeline() {
        assert_eq!(hash_frame_position(&[7200.0]), (0, 3600.0));
        // The middle (5760 s) is inside part 0.
        assert_eq!(hash_frame_position(&[6000.0, 5520.0]), (0, 5760.0));
        // 1000 s + 5000 s: the middle (3000 s) is 2000 s into part 1.
        assert_eq!(hash_frame_position(&[1000.0, 5000.0]), (1, 2000.0));
        assert_eq!(hash_frame_position(&[0.0, 0.0]), (0, 0.0));
        assert_eq!(hash_frame_position(&[]), (0, 0.0));
    }

    #[test]
    fn kinds_parse_like_the_cli_type_values() {
        assert_eq!(ChecksumKind::parse("CRC32"), Some(ChecksumKind::Crc32));
        assert_eq!(ChecksumKind::parse("phash"), Some(ChecksumKind::Phash));
        assert_eq!(ChecksumKind::parse(" all "), Some(ChecksumKind::All));
        assert_eq!(ChecksumKind::parse("md5"), None);
    }

    #[test]
    fn frame_arguments_ask_for_one_small_grey_frame_on_stdout() {
        let args: Vec<String> = frame_args(Path::new("m.mkv"), 61.5)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let joined = args.join(" ");
        assert!(joined.contains("-ss 61.500 -i m.mkv"));
        assert!(joined.contains("-frames:v 1"));
        assert!(joined.contains("scale=32:32:flags=area,format=gray"));
        assert!(joined.ends_with("-f rawvideo -"));
    }
}

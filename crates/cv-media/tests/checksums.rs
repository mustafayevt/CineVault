//! Checksums: CRC-32 of files, the perceptual hash of synthetic frames, and (when FFmpeg is in
//! the dev tools cache) the hash of a real clip. Without FFmpeg the clip test prints why and
//! returns.

use std::process::{Command, Stdio};

use cv_domain::{MediaFile, MediaFileType};
use cv_media::checksum::{
    ChecksumError, ChecksumKind, FRAME_BYTES, FRAME_SIDE, calculate_checksums, crc32_hex,
    crc32_of_file, hamming_distance, parse_phash_hex, perceptual_hash_of_file, phash_hex,
    phash_of_gray,
};
use cv_media::tools::{Cancel, ToolId, ToolsManager, dev_cache_dir};

/// A 32x32 frame from a pixel function.
fn frame(pixel: impl Fn(usize, usize) -> u8) -> Vec<u8> {
    (0..FRAME_BYTES)
        .map(|i| pixel(i % FRAME_SIDE, i / FRAME_SIDE))
        .collect()
}

#[allow(clippy::cast_possible_truncation)]
fn gradient(x: usize, y: usize) -> u8 {
    // A diagonal ramp with a bright blob; values stay within 40..=200 so brightening does not clip.
    let ramp = 40 + (x * 3 + y * 2);
    let blob = if (6..14).contains(&x) && (18..28).contains(&y) {
        60
    } else {
        0
    };
    (ramp + blob) as u8
}

fn blocks(x: usize, y: usize) -> u8 {
    if (x / 8 + y / 8).is_multiple_of(2) {
        230
    } else {
        20
    }
}

fn video_file(dir: &std::path::Path) -> MediaFile {
    MediaFile::new(MediaFileType::Video, dir.to_string_lossy(), "m.mkv")
}

#[test]
fn crc32_of_a_file_matches_the_check_value_and_reports_progress() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("movie.mkv");
    std::fs::write(&path, b"123456789").expect("write");
    let mut seen = Vec::new();
    let hex = crc32_of_file(&path, &Cancel::new(), &mut |done, total| {
        seen.push((done, total));
    })
    .expect("crc");
    assert_eq!(hex, "CBF43926");
    assert_eq!(seen.last(), Some(&(9, 9)));
}

#[test]
fn crc32_of_a_multi_chunk_file_equals_the_whole_buffer() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("big.bin");
    #[allow(clippy::cast_possible_truncation)]
    let data: Vec<u8> = (0..2_500_000u32).map(|i| (i * 31 + 7) as u8).collect();
    std::fs::write(&path, &data).expect("write");
    let got = crc32_of_file(&path, &Cancel::new(), &mut |_, _| {}).expect("crc");
    assert_eq!(got, crc32_hex(&data));
    assert_eq!(got.len(), 8);
}

#[test]
fn crc32_stops_when_cancelled_and_reports_missing_files() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("a.bin");
    std::fs::write(&path, b"x").expect("write");
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(matches!(
        crc32_of_file(&path, &cancel, &mut |_, _| {}),
        Err(ChecksumError::Cancelled)
    ));
    let missing = dir.path().join("missing.bin");
    assert!(matches!(
        crc32_of_file(&missing, &Cancel::new(), &mut |_, _| {}),
        Err(ChecksumError::Io { .. })
    ));
}

#[test]
fn the_perceptual_hash_is_stable_and_tolerates_brightness_and_noise() {
    let base = phash_of_gray(&frame(gradient)).expect("hash");
    assert_eq!(phash_of_gray(&frame(gradient)), Some(base));

    let brighter = phash_of_gray(&frame(|x, y| gradient(x, y).saturating_add(12))).expect("hash");
    #[allow(clippy::cast_possible_truncation)]
    let noisy = phash_of_gray(&frame(|x, y| {
        gradient(x, y).saturating_add(((x * 7 + y * 13) % 5) as u8)
    }))
    .expect("hash");
    assert!(hamming_distance(base, brighter) <= 6, "brighter");
    assert!(hamming_distance(base, noisy) <= 6, "noisy");
}

#[test]
fn different_pictures_have_distant_hashes() {
    let a = phash_of_gray(&frame(gradient)).expect("hash");
    let b = phash_of_gray(&frame(blocks)).expect("hash");
    let distance = hamming_distance(a, b);
    assert!(distance >= 12, "distance {distance}");
}

#[test]
fn a_frame_of_the_wrong_size_has_no_hash_and_hex_round_trips() {
    assert_eq!(phash_of_gray(&[0u8; 10]), None);
    let hash = phash_of_gray(&frame(blocks)).expect("hash");
    let hex = phash_hex(hash);
    assert_eq!(hex.len(), 16);
    assert_eq!(hex, hex.to_lowercase());
    assert_eq!(parse_phash_hex(&hex), Some(hash));
    assert_eq!(parse_phash_hex("xyz"), None);
}

#[test]
fn calculate_checksums_stores_crc32_and_keeps_existing_values() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("m.mkv"), b"123456789").expect("write");
    let mut file = video_file(dir.path());
    let tools = ToolsManager::new(dir.path().join("tools"));
    let run = |file: &mut MediaFile, force: bool| {
        calculate_checksums(
            &tools,
            file,
            ChecksumKind::Crc32,
            force,
            &Cancel::new(),
            &mut |_, _| {},
        )
        .expect("crc32")
    };
    assert_eq!(run(&mut file, false), ["crc32"]);
    assert_eq!(file.checksums["crc32"], "CBF43926");

    // A stored value is not recalculated without `force`.
    std::fs::write(dir.path().join("m.mkv"), b"changed").expect("write");
    assert!(run(&mut file, false).is_empty());
    assert_eq!(file.checksums["crc32"], "CBF43926");
    run(&mut file, true);
    assert_ne!(file.checksums["crc32"], "CBF43926");
}

#[test]
fn a_missing_ffmpeg_fails_the_phash_but_keeps_the_crc32() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("m.mkv"), b"123456789").expect("write");
    let mut file = video_file(dir.path());
    // An empty tools folder and no override: FFmpeg cannot be found there.
    let tools = ToolsManager::new(dir.path().join("empty-tools"));
    if tools.resolve(ToolId::Ffmpeg).is_ok() {
        eprintln!("skipped: FFmpeg was found on this machine");
        return;
    }
    let result = calculate_checksums(
        &tools,
        &mut file,
        ChecksumKind::All,
        false,
        &Cancel::new(),
        &mut |_, _| {},
    );
    assert!(matches!(result, Err(ChecksumError::Tool(_))));
    assert_eq!(file.checksums["crc32"], "CBF43926");
    assert!(!file.checksums.contains_key("phash"));
}

#[test]
fn disc_images_are_not_hashed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let tools = ToolsManager::new(dir.path());
    let parts = [(dir.path().join("movie.iso"), 7000.0)];
    assert!(matches!(
        perceptual_hash_of_file(&tools, &parts, &Cancel::new()),
        Err(ChecksumError::DiscImage(_))
    ));
}

#[test]
fn the_hash_of_a_real_clip_is_stable_across_re_encodes() {
    let tools = ToolsManager::new(dev_cache_dir());
    let Ok(ffmpeg) = tools.resolve(ToolId::Ffmpeg) else {
        eprintln!("skipped: FFmpeg is not in the dev tools cache; run `bun run tools:fetch`");
        return;
    };
    let dir = tempfile::tempdir().expect("temp dir");
    let encode = |name: &str, size: &str, crf: &str| {
        let out = dir.path().join(name);
        let status = Command::new(&ffmpeg.path)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("testsrc2=size={size}:rate=5:duration=6"))
            .args(["-c:v", "libx264", "-preset", "ultrafast", "-crf", crf])
            .args(["-pix_fmt", "yuv420p", "-an"])
            .arg(&out)
            .stdin(Stdio::null())
            .status()
            .expect("FFmpeg starts");
        assert!(status.success());
        out
    };
    let hash =
        |path| perceptual_hash_of_file(&tools, &[(path, 6.0)], &Cancel::new()).expect("hash");
    let big = hash(encode("big.mp4", "640x360", "18"));
    let small = hash(encode("small.mp4", "480x270", "28"));
    assert_eq!(big.len(), 16);
    let distance = hamming_distance(
        parse_phash_hex(&big).expect("hex"),
        parse_phash_hex(&small).expect("hex"),
    );
    assert!(distance <= 10, "distance {distance}");
}

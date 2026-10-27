//! The ISO 9660 and UDF reader against images built by the test builders: every flavour holds
//! the same tree, and every file must read back byte for byte.

mod disc_support;

use std::io::Cursor;

use cv_media::disc::{DiscFs, IsoImage};
use disc_support::images::{UdfFlavor, iso9660, udf};
use disc_support::pattern;

type Files = Vec<(&'static str, Vec<u8>)>;

/// A tree that exercises small (embedded in UDF), exact-block, multi-block and empty files,
/// nested folders and lookups by any case.
fn tree() -> Files {
    vec![
        ("README.TXT", b"hello disc".to_vec()),
        ("EMPTY.BIN", Vec::new()),
        ("VIDEO_TS/VIDEO_TS.IFO", pattern(300, 1)),
        ("VIDEO_TS/VTS_01_1.VOB", pattern(5000, 2)),
        ("VIDEO_TS/VTS_01_2.VOB", pattern(2048, 3)),
        ("VIDEO_TS/VTS_01_3.VOB", pattern(2049, 4)),
        ("BDMV/PLAYLIST/00001.MPLS", pattern(150, 5)),
        ("BDMV/STREAM/00001.M2TS", pattern(100_000, 6)),
        ("BDMV/STREAM/00002.M2TS", pattern(70_001, 7)),
    ]
}

fn images() -> Vec<(&'static str, Vec<u8>)> {
    let files = tree();
    vec![
        ("ISO 9660", iso9660(&files)),
        ("UDF plain", udf(&files, UdfFlavor::Plain)),
        ("UDF metadata partition", udf(&files, UdfFlavor::Metadata)),
    ]
}

fn read_all(fs: &dyn DiscFs, path: &str, chunk: usize) -> Vec<u8> {
    let mut data = Vec::new();
    let mut buffer = vec![0_u8; chunk];
    loop {
        let read = fs
            .read_at(path, data.len() as u64, &mut buffer)
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        if read == 0 {
            return data;
        }
        data.extend_from_slice(&buffer[..read]);
    }
}

fn names(fs: &dyn DiscFs, dir: &str) -> Vec<String> {
    let mut names: Vec<String> = fs
        .list(dir)
        .unwrap()
        .into_iter()
        .map(|entry| entry.name.to_ascii_uppercase())
        .collect();
    names.sort();
    names
}

#[test]
fn every_flavour_lists_the_same_tree() {
    for (label, image) in images() {
        let fs = IsoImage::open(Cursor::new(image)).unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(
            names(&fs, ""),
            ["BDMV", "EMPTY.BIN", "README.TXT", "VIDEO_TS"],
            "{label}"
        );
        assert_eq!(names(&fs, "BDMV"), ["PLAYLIST", "STREAM"], "{label}");
        assert_eq!(
            names(&fs, "BDMV/STREAM"),
            ["00001.M2TS", "00002.M2TS"],
            "{label}"
        );
        assert_eq!(
            names(&fs, "video_ts"),
            [
                "VIDEO_TS.IFO",
                "VTS_01_1.VOB",
                "VTS_01_2.VOB",
                "VTS_01_3.VOB"
            ],
            "{label}"
        );
        let streams = fs.list("BDMV/STREAM").unwrap();
        assert!(streams.iter().all(|entry| !entry.is_dir), "{label}");
        let root = fs.list("").unwrap();
        assert_eq!(
            root.iter().filter(|entry| entry.is_dir).count(),
            2,
            "{label}"
        );
    }
}

#[test]
fn every_file_reads_back_byte_for_byte() {
    for (label, image) in images() {
        let fs = IsoImage::open(Cursor::new(image)).unwrap();
        for (path, expected) in tree() {
            assert_eq!(
                fs.size(path).unwrap(),
                expected.len() as u64,
                "{label}: {path}"
            );
            for chunk in [4096, 777, 2048, 1] {
                if chunk == 1 && expected.len() > 3000 {
                    continue;
                }
                assert_eq!(
                    read_all(&fs, path, chunk),
                    expected,
                    "{label}: {path} in {chunk}s"
                );
            }
        }
    }
}

#[test]
fn lookups_ignore_case_and_reads_can_start_anywhere() {
    for (label, image) in images() {
        let fs = IsoImage::open(Cursor::new(image)).unwrap();
        let expected = pattern(100_000, 6);
        let mut buffer = [0_u8; 600];
        // Across the 2048-byte boundary where larger UDF files change extent, and near the end.
        for offset in [0_usize, 2000, 2048, 2047, 50_001, 99_700] {
            let read = fs
                .read_at("bdmv/stream/00001.m2ts", offset as u64, &mut buffer)
                .unwrap();
            let end = (offset + buffer.len()).min(expected.len());
            assert_eq!(
                &buffer[..read],
                &expected[offset..end],
                "{label} at {offset}"
            );
        }
        for beyond in [100_000, 999_999] {
            let read = fs
                .read_at("BDMV/STREAM/00001.M2TS", beyond, &mut buffer)
                .unwrap();
            assert_eq!(read, 0, "{label}");
        }
    }
}

#[test]
fn a_missing_path_is_not_found() {
    for (label, image) in images() {
        let fs = IsoImage::open(Cursor::new(image)).unwrap();
        let error = fs.size("BDMV/STREAM/99999.M2TS").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{label}");
        assert!(fs.list("NOPE").is_err(), "{label}");
        assert!(fs.list("README.TXT").is_err(), "{label}");
    }
}

#[test]
fn the_file_system_is_reported() {
    let kinds: Vec<_> = images()
        .into_iter()
        .map(|(_, image)| IsoImage::open(Cursor::new(image)).unwrap().file_system())
        .collect();
    assert_eq!(kinds, ["ISO 9660", "UDF", "UDF"]);
}

#[test]
fn udf_names_outside_ascii_are_decoded() {
    let files = vec![(
        "Caf\u{e9} \u{dc}n\u{ef}/na\u{ef}ve \u{65e5}\u{672c}.txt",
        b"x".repeat(500),
    )];
    for flavor in [UdfFlavor::Plain, UdfFlavor::Metadata] {
        let fs = IsoImage::open(Cursor::new(udf(&files, flavor))).unwrap();
        assert_eq!(fs.list("").unwrap()[0].name, "Caf\u{e9} \u{dc}n\u{ef}");
        assert_eq!(
            fs.size("Caf\u{e9} \u{dc}n\u{ef}/na\u{ef}ve \u{65e5}\u{672c}.txt")
                .unwrap(),
            500
        );
    }
}

#[test]
fn bytes_that_are_not_an_image_are_rejected_with_a_reason() {
    for bytes in [
        Vec::new(),
        vec![0_u8; 100],
        pattern(70_000, 9),
        vec![0_u8; 300 * 2048],
    ] {
        let error = IsoImage::open(Cursor::new(bytes)).err().expect("rejected");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        let message = error.to_string();
        assert!(
            message.contains("UDF") && message.contains("ISO 9660"),
            "{message}"
        );
    }
}

#[test]
fn a_truncated_image_fails_cleanly() {
    let image = udf(&tree(), UdfFlavor::Metadata);
    // Cut inside the partition: opening or reading may fail, but never panics.
    for keep in [150 * 2048, 270 * 2048, 289 * 2048, 292 * 2048] {
        let cut = image[..keep.min(image.len())].to_vec();
        if let Ok(fs) = IsoImage::open(Cursor::new(cut)) {
            let mut buffer = [0_u8; 100];
            let _ = fs.list("");
            let _ = fs.read_at("BDMV/STREAM/00001.M2TS", 0, &mut buffer);
        }
    }
}

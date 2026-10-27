//! A stand-in for FFmpeg used by the automation tests. It is compiled by the test with rustc and
//! reads no video.
//!
//! It answers the calls the app makes, by their arguments:
//!
//! * `-version`: prints an FFmpeg version banner (setting the path checks it);
//! * `rawvideo` (the perceptual-hash frame): writes a 32 x 32 grey frame on stdout;
//! * `signalstats` (the black level): prints the darkest luma of the first frame;
//! * `cropdetect` (an aspect-ratio sample): prints a 4:3 picture in a 16:9 frame, the way a
//!   pillar-boxed video would.

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let joined = args.join(" ");
    if args.iter().any(|a| a == "-version") {
        println!("ffmpeg version 0.0-mock Copyright (c) the CineVault tests");
        return;
    }
    if joined.contains("rawvideo") {
        let frame: Vec<u8> = (0..1024_u32)
            .map(|i| u8::try_from((i * 7 + i / 32) % 251).unwrap_or(0))
            .collect();
        std::io::stdout().write_all(&frame).expect("frame");
        return;
    }
    if joined.contains("signalstats") {
        eprintln!("[Parsed_metadata_1 @ 0x1] lavfi.signalstats.YMIN=16");
        return;
    }
    if joined.contains("cropdetect") {
        for t in 0..3 {
            eprintln!(
                "[Parsed_cropdetect_0 @ 0x1] x1:240 x2:1679 y1:0 y2:1079 w:1440 h:1080 x:240 y:0 pts:{t} t:{t}.0 limit:0.1 crop=1440:1080:240:0"
            );
        }
        return;
    }
    println!("mock ffmpeg: nothing to do for {joined}");
}

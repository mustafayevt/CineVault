//! Perceptual hash: a 64-bit fingerprint of one frame that survives re-encoding and scaling.
//!
//! The frame is reduced to 32x32 grey, a 2-D DCT keeps the 8x8 lowest frequencies, and each of
//! the 64 coefficients becomes one bit: set when it is above the median of the 63 AC
//! coefficients. Two frames of the same picture differ in few bits ([`hamming_distance`]).

use std::sync::OnceLock;

/// Side of the grey frame the hash is computed from.
pub const FRAME_SIDE: usize = 32;
/// Number of grey bytes in a frame.
pub const FRAME_BYTES: usize = FRAME_SIDE * FRAME_SIDE;
const LOW: usize = 8;

fn cosines() -> &'static [[f64; FRAME_SIDE]; LOW] {
    static TABLE: OnceLock<[[f64; FRAME_SIDE]; LOW]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [[0.0; FRAME_SIDE]; LOW];
        for (k, row) in table.iter_mut().enumerate() {
            for (n, cell) in row.iter_mut().enumerate() {
                *cell = (std::f64::consts::PI * (2 * n + 1) as f64 * k as f64
                    / (2 * FRAME_SIDE) as f64)
                    .cos();
            }
        }
        table
    })
}

/// The 64-bit hash of a 32x32 grey frame (row-major, one byte per pixel); `None` when `frame`
/// is not exactly [`FRAME_BYTES`] long.
#[must_use]
pub fn phash_of_gray(frame: &[u8]) -> Option<u64> {
    if frame.len() != FRAME_BYTES {
        return None;
    }
    let cos = cosines();
    // Rows first: for each image row, the 8 lowest horizontal frequencies.
    let mut rows = [[0.0f64; LOW]; FRAME_SIDE];
    for (y, out) in rows.iter_mut().enumerate() {
        for (u, cell) in out.iter_mut().enumerate() {
            *cell = (0..FRAME_SIDE)
                .map(|x| f64::from(frame[y * FRAME_SIDE + x]) * cos[u][x])
                .sum();
        }
    }
    // Then columns: the 8 lowest vertical frequencies of each.
    let mut coefficients = [0.0f64; LOW * LOW];
    for v in 0..LOW {
        for u in 0..LOW {
            coefficients[v * LOW + u] = (0..FRAME_SIDE).map(|y| rows[y][u] * cos[v][y]).sum();
        }
    }
    let mut ac: Vec<f64> = coefficients[1..].to_vec();
    ac.sort_by(f64::total_cmp);
    let median = ac[ac.len() / 2];
    let mut hash = 0u64;
    for (i, &value) in coefficients.iter().enumerate() {
        if value > median {
            hash |= 1 << i;
        }
    }
    Some(hash)
}

/// The hash as 16 lowercase hex digits.
#[must_use]
pub fn phash_hex(hash: u64) -> String {
    format!("{hash:016x}")
}

/// Parses [`phash_hex`] output.
#[must_use]
pub fn parse_phash_hex(text: &str) -> Option<u64> {
    (text.len() == 16).then(|| u64::from_str_radix(text, 16).ok())?
}

/// Number of differing bits; 0 is the same picture, above about 10 a different one.
#[must_use]
pub const fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

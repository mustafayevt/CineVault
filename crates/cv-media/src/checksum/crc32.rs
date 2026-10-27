//! CRC-32 (the IEEE 802.3 polynomial used by zip, PNG and release-name tags), streamed.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::ChecksumError;
use crate::tools::Cancel;

const POLY: u32 = 0xEDB8_8320;
const TABLE: [u32; 256] = build_table();
const CHUNK: usize = 1024 * 1024;

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        #[allow(clippy::cast_possible_truncation)]
        let mut value = i as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                (value >> 1) ^ POLY
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}

/// A running CRC-32.
#[derive(Debug, Clone, Copy)]
pub struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    #[must_use]
    pub const fn new() -> Self {
        Self(!0)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        let mut state = self.0;
        for &byte in bytes {
            state = TABLE[((state ^ u32::from(byte)) & 0xFF) as usize] ^ (state >> 8);
        }
        self.0 = state;
    }

    #[must_use]
    pub const fn value(self) -> u32 {
        !self.0
    }

    /// The checksum as 8 uppercase hex digits, zero padded.
    #[must_use]
    pub fn hex(self) -> String {
        format!("{:08X}", self.value())
    }
}

/// The CRC-32 of `bytes` as 8 uppercase hex digits.
#[must_use]
pub fn crc32_hex(bytes: &[u8]) -> String {
    let mut crc = Crc32::new();
    crc.update(bytes);
    crc.hex()
}

/// The CRC-32 of a whole file as 8 uppercase hex digits. `progress` gets
/// `(bytes read, file size)` after every chunk; `cancel` is checked between chunks.
///
/// # Errors
/// [`ChecksumError::Io`] when the file cannot be read, [`ChecksumError::Cancelled`] when
/// `cancel` is set.
pub fn crc32_of_file(
    path: &Path,
    cancel: &Cancel,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<String, ChecksumError> {
    let io = |source| ChecksumError::Io {
        path: path.to_path_buf(),
        source,
    };
    let _reading = cv_fs::reading(path);
    let mut file = File::open(path).map_err(io)?;
    let total = file.metadata().map_err(io)?.len();
    let mut buffer = vec![0u8; CHUNK];
    let mut crc = Crc32::new();
    let mut done = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(ChecksumError::Cancelled);
        }
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        crc.update(&buffer[..read]);
        done += read as u64;
        progress(done, total);
    }
    Ok(crc.hex())
}

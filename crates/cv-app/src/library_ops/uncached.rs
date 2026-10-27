//! Reading a copy back from the disk itself, not from the operating system's cache (the verify
//! of a move between drives, spec `source-moves` "Copy, verify, then commit").
//!
//! Right after a copy is written, most of it is still in the system's file cache: a normal read
//! would compare the cache with the original and could miss a write the disk got wrong. On
//! Windows the copy is opened with `FILE_FLAG_NO_BUFFERING`, on Linux with `O_DIRECT` (a safe
//! open option, through `rustix`'s flag constant); both read from the device. Such a handle
//! takes reads only of whole sectors, into memory aligned to the sector size, at aligned
//! offsets: [`AlignedReader`] reads whole aligned blocks into an aligned part of an
//! over-allocated buffer (found with `align_offset`, no `unsafe`) and hands the bytes out as a
//! normal reader. The last block of a file is read whole as well; the system returns only the
//! bytes the file has. Where the flag is refused at open (a share, a file system without direct
//! I/O such as some FUSE mounts), the file is opened normally; where a read is refused (a device
//! with larger sectors), the reader falls back to a normal read from the same offset. Elsewhere
//! (macOS) the read is a normal one: switching the cache off there needs a call this workspace
//! keeps out (`unsafe`).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The alignment of reads: a multiple of every common sector size (512 and 4096 bytes).
pub const ALIGN: usize = 4096;

/// A reader of whole aligned blocks.
pub struct AlignedReader {
    file: File,
    path: PathBuf,
    /// Over-allocated by [`ALIGN`]; the aligned window starts at `start`.
    buffer: Vec<u8>,
    start: usize,
    /// The window's length: a multiple of [`ALIGN`].
    window: usize,
    /// Bytes of the window that hold data, and how many of them were handed out.
    filled: usize,
    taken: usize,
    /// Bytes read from the file so far (always a multiple of [`ALIGN`] until the end).
    offset: u64,
    /// The file ended (a short block was read).
    ended: bool,
    /// The file is read normally from now on (the device refused aligned reads).
    plain: bool,
}

impl AlignedReader {
    /// A reader over `file` (opened without the cache where the system allows), reading `block`
    /// bytes at a time, rounded up to whole [`ALIGN`] blocks.
    #[must_use]
    pub fn new(file: File, path: &Path, block: usize) -> Self {
        let window = block.max(ALIGN).div_ceil(ALIGN) * ALIGN;
        let buffer = vec![0_u8; window + ALIGN];
        let start = buffer.as_ptr().align_offset(ALIGN);
        Self {
            file,
            path: path.to_path_buf(),
            buffer,
            start: if start > ALIGN { 0 } else { start },
            window,
            filled: 0,
            taken: 0,
            offset: 0,
            ended: false,
            plain: false,
        }
    }

    /// Reopens the file normally at the current offset.
    fn fall_back(&mut self) -> io::Result<()> {
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.offset))?;
        self.file = file;
        self.plain = true;
        Ok(())
    }

    /// Reads the next whole window (fewer bytes only at the end of the file).
    fn fill(&mut self) -> io::Result<()> {
        let (start, window) = (self.start, self.window);
        let mut filled = 0;
        while filled < window {
            let slice = &mut self.buffer[start + filled..start + window];
            match self.file.read(slice) {
                Ok(0) => {
                    self.ended = true;
                    break;
                }
                Ok(n) => {
                    filled += n;
                    self.offset += n as u64;
                    // An uncached handle returns a short read only at the end of the file;
                    // stop there, the next read would start at an unaligned offset.
                    if !self.plain && n % ALIGN != 0 {
                        self.ended = true;
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) if !self.plain && filled % ALIGN == 0 => {
                    // The device does not take uncached reads (or this alignment): read on
                    // normally from the same, aligned, place.
                    self.fall_back()?;
                }
                Err(e) => return Err(e),
            }
        }
        self.filled = filled;
        self.taken = 0;
        Ok(())
    }
}

impl Read for AlignedReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.taken == self.filled {
            if self.ended || out.is_empty() {
                return Ok(0);
            }
            self.fill()?;
            if self.filled == 0 {
                return Ok(0);
            }
        }
        let n = out.len().min(self.filled - self.taken);
        let from = self.start + self.taken;
        out[..n].copy_from_slice(&self.buffer[from..from + n]);
        self.taken += n;
        Ok(n)
    }
}

/// Opens `path` to be read back from the disk, past the system's cache where it can. `native`
/// is the path as the system takes it (long paths).
///
/// # Errors
/// The file cannot be opened at all.
pub fn open_uncached(native: &Path, block: usize) -> io::Result<Box<dyn Read + Send>> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        /// `FILE_FLAG_NO_BUFFERING`.
        const NO_BUFFERING: u32 = 0x2000_0000;
        /// `FILE_FLAG_SEQUENTIAL_SCAN`.
        const SEQUENTIAL: u32 = 0x0800_0000;
        if let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(NO_BUFFERING | SEQUENTIAL)
            .open(native)
        {
            return Ok(Box::new(AlignedReader::new(file, native, block)));
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // `O_DIRECT`: the same aligned reads, from the device. A file system that refuses it
        // at open falls through to the normal open below.
        if let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::DIRECT.bits() as i32)
            .open(native)
        {
            return Ok(Box::new(AlignedReader::new(file, native, block)));
        }
    }
    let file = File::open(native)?;
    let mut reader = AlignedReader::new(file, native, block);
    reader.plain = true;
    Ok(Box::new(reader))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| u8::try_from((i * 31 + i / 7) % 251).unwrap())
            .collect()
    }

    fn read_all(path: &Path, block: usize) -> Vec<u8> {
        let mut out = Vec::new();
        open_uncached(path, block)
            .unwrap()
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    #[test]
    fn the_final_partial_block_and_every_boundary_read_back_whole() {
        let dir = tempfile::tempdir().unwrap();
        for len in [
            0,
            1,
            ALIGN - 1,
            ALIGN,
            ALIGN + 1,
            2 * ALIGN,
            3 * 8192 + 5,
            5 * 8192,
        ] {
            let path = dir.path().join(format!("f{len}.bin"));
            let data = pattern(len);
            std::fs::write(&path, &data).unwrap();
            let back = read_all(&path, 8192);
            assert_eq!(back.len(), len, "length {len}");
            assert!(back == data, "content of length {len}");
        }
    }

    #[test]
    fn small_reads_from_the_caller_still_get_every_byte_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        let data = pattern(3 * ALIGN + 77);
        std::fs::write(&path, &data).unwrap();
        let mut reader = open_uncached(&path, ALIGN).unwrap();
        let mut back = Vec::new();
        let mut small = [0_u8; 100];
        loop {
            let n = reader.read(&mut small).unwrap();
            if n == 0 {
                break;
            }
            back.extend_from_slice(&small[..n]);
        }
        assert!(back == data);
    }

    #[test]
    fn the_window_is_aligned_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        std::fs::write(&path, b"x").unwrap();
        let reader = AlignedReader::new(File::open(&path).unwrap(), &path, 10_000);
        assert_eq!(reader.window % ALIGN, 0);
        assert!(reader.window >= 10_000);
        let at = reader.buffer[reader.start..].as_ptr() as usize;
        assert_eq!(at % ALIGN, 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_copy_on_tmpfs_still_reads_back_whole() {
        // `/dev/shm` is tmpfs, which older kernels refuse `O_DIRECT` on: the open then falls
        // back to a normal read. Either way every byte must come back, the last partial block
        // included. Without `/dev/shm` there is nothing to check.
        let Ok(dir) = tempfile::Builder::new()
            .prefix("cv-uncached-")
            .tempdir_in("/dev/shm")
        else {
            return;
        };
        for len in [1, 2 * ALIGN, 3 * 8192 + 5] {
            let path = dir.path().join(format!("f{len}.bin"));
            let data = pattern(len);
            std::fs::write(&path, &data).unwrap();
            let back = read_all(&path, 8192);
            assert!(back == data, "content of length {len} on tmpfs");
        }
    }
}

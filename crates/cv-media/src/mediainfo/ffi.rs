//! The MediaInfo C interface, loaded at run time.
//!
//! This is the only module in the workspace that uses `unsafe` (see `AGENTS.md`). Every unsafe
//! block says why it is sound. Nothing here leaks out: the rest of the crate sees [`Api`] (a
//! loaded library) and [`Handle`] (one open instance), both with safe methods.
//!
//! The library exports `MediaInfo_*` functions that take `wchar_t` strings: UTF-16 on Windows,
//! UTF-32 on macOS and Linux. Strings it returns point into the instance and stay valid only until
//! the next call on that instance, so they are copied immediately.

#![allow(unsafe_code)]

use std::ffi::{OsStr, c_int, c_void};
use std::path::Path;
use std::sync::Arc;

use libloading::Library;

use super::MediaInfoError;
use super::snapshot::{MediaSnapshot, StreamInfo, StreamKind};

#[cfg(windows)]
type WChar = u16;
#[cfg(not(windows))]
type WChar = u32;

/// `(size_t)-1`: "all streams" / "the count" in the library's calls.
const ALL: usize = usize::MAX;
/// `MediaInfo_Info_Name` and `MediaInfo_Info_Text` of `MediaInfo_info_C`.
const INFO_NAME: c_int = 0;
const INFO_TEXT: c_int = 1;

type NewFn = unsafe extern "C" fn() -> *mut c_void;
type DeleteFn = unsafe extern "C" fn(*mut c_void);
type OpenFn = unsafe extern "C" fn(*mut c_void, *const WChar) -> usize;
type CloseFn = unsafe extern "C" fn(*mut c_void);
type CountFn = unsafe extern "C" fn(*mut c_void, c_int, usize) -> usize;
type GetIFn = unsafe extern "C" fn(*mut c_void, c_int, usize, usize, c_int) -> *const WChar;
type BufferInitFn = unsafe extern "C" fn(*mut c_void, u64, u64) -> usize;
type BufferContinueFn = unsafe extern "C" fn(*mut c_void, *const u8, usize) -> usize;
type BufferGoToFn = unsafe extern "C" fn(*mut c_void) -> u64;
type BufferFinalizeFn = unsafe extern "C" fn(*mut c_void) -> usize;
type OptionFn = unsafe extern "C" fn(*mut c_void, *const WChar, *const WChar) -> *const WChar;

/// A loaded MediaInfo library: the function table, and the library itself so it stays mapped.
pub(super) struct Api {
    new: NewFn,
    delete: DeleteFn,
    open: OpenFn,
    close: CloseFn,
    count: CountFn,
    get_i: GetIFn,
    option: OptionFn,
    buffer_init: BufferInitFn,
    buffer_continue: BufferContinueFn,
    buffer_goto: BufferGoToFn,
    buffer_finalize: BufferFinalizeFn,
    // Declared last, so the function pointers above never outlive the mapping they point into.
    _library: Library,
}

impl Api {
    /// Loads the library at `path` and binds every function the app uses.
    pub(super) fn load(path: &Path) -> Result<Self, MediaInfoError> {
        // SAFETY: loading a shared library runs its initialisers. The path comes from the tools
        // manager (a verified download or the user's own choice of MediaInfo library).
        let library = unsafe { Library::new(path) }.map_err(|e| MediaInfoError::Load {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        let missing = |name: &str, e: libloading::Error| MediaInfoError::Unusable {
            path: path.to_path_buf(),
            reason: format!("it has no {name} function ({e})"),
        };
        // SAFETY (for every `symbol` call below): the type named for each symbol is the C
        // signature documented in MediaInfoDLL.h for that export, and the copied function
        // pointer is kept next to `_library`, which is dropped after it.
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let found = unsafe { library.get::<$ty>(concat!($name, "\0").as_bytes()) };
                match found {
                    Ok(symbol) => *symbol,
                    Err(e) => return Err(missing($name, e)),
                }
            }};
        }
        Ok(Self {
            new: symbol!("MediaInfo_New", NewFn),
            delete: symbol!("MediaInfo_Delete", DeleteFn),
            open: symbol!("MediaInfo_Open", OpenFn),
            close: symbol!("MediaInfo_Close", CloseFn),
            count: symbol!("MediaInfo_Count_Get", CountFn),
            get_i: symbol!("MediaInfo_GetI", GetIFn),
            option: symbol!("MediaInfo_Option", OptionFn),
            buffer_init: symbol!("MediaInfo_Open_Buffer_Init", BufferInitFn),
            buffer_continue: symbol!("MediaInfo_Open_Buffer_Continue", BufferContinueFn),
            buffer_goto: symbol!("MediaInfo_Open_Buffer_Continue_GoTo_Get", BufferGoToFn),
            buffer_finalize: symbol!("MediaInfo_Open_Buffer_Finalize", BufferFinalizeFn),
            _library: library,
        })
    }

    /// The library's own version text, e.g. `MediaInfoLib - v26.10`.
    pub(super) fn version(&self) -> String {
        self.static_option("Info_Version", "")
    }

    fn static_option(&self, name: &str, value: &str) -> String {
        let (name, value) = (to_wide(OsStr::new(name)), to_wide(OsStr::new(value)));
        // SAFETY: a null handle selects the library's global options. Both strings are
        // NUL-terminated and live for the whole call; the reply is copied before returning.
        unsafe {
            from_wide((self.option)(
                std::ptr::null_mut(),
                name.as_ptr(),
                value.as_ptr(),
            ))
        }
    }
}

/// The status bits `Open_Buffer_Continue` returns.
#[derive(Clone, Copy, Debug)]
pub(super) struct BufferStatus(usize);

impl BufferStatus {
    /// The library has everything it wants and needs no more bytes.
    pub(super) const fn filled(self) -> bool {
        self.0 & 0x02 != 0
    }

    /// The library has finished with the file.
    pub(super) const fn finalized(self) -> bool {
        self.0 & 0x08 != 0
    }
}

/// One MediaInfo instance, used by the thread that created it. It is closed and freed on drop.
///
/// Instances share no state (apart from the library's global options, which are not changed after
/// loading), so every file gets its own and several threads can read files at once. The type is
/// neither `Send` nor `Sync` because it holds a raw pointer; it never needs to cross threads.
pub(super) struct Handle {
    api: Arc<Api>,
    raw: *mut c_void,
}

impl Handle {
    pub(super) fn new(api: &Arc<Api>) -> Result<Self, MediaInfoError> {
        // SAFETY: `New` takes no arguments and returns an owned instance, or null on failure.
        let raw = unsafe { (api.new)() };
        if raw.is_null() {
            return Err(MediaInfoError::OutOfMemory);
        }
        let handle = Self {
            api: Arc::clone(api),
            raw,
        };
        // Without `Complete` the library reports only a short list of fields per stream.
        handle.option("Complete", "1");
        Ok(handle)
    }

    fn option(&self, name: &str, value: &str) {
        let (name, value) = (to_wide(OsStr::new(name)), to_wide(OsStr::new(value)));
        // SAFETY: `raw` is a live instance owned by `self`; the strings are NUL-terminated and
        // outlive the call; the returned text is ignored.
        unsafe {
            (self.api.option)(self.raw, name.as_ptr(), value.as_ptr());
        }
    }

    /// Opens `path` for reading. False when the library could not open it.
    pub(super) fn open(&self, path: &Path) -> bool {
        let wide = to_wide(path.as_os_str());
        // SAFETY: `raw` is a live instance owned by `self`; `wide` is NUL-terminated and outlives
        // the call.
        unsafe { (self.api.open)(self.raw, wide.as_ptr()) != 0 }
    }

    /// Starts reading from memory instead of a path: `size` is the whole file's length (0 when
    /// unknown) and `offset` where the first buffer starts in it.
    pub(super) fn buffer_init(&self, size: u64, offset: u64) {
        // SAFETY: `raw` is a live instance owned by `self`; the arguments are plain numbers.
        unsafe {
            (self.api.buffer_init)(self.raw, size, offset);
        }
    }

    /// Hands the next bytes to the library. The result is the library's status bit set.
    pub(super) fn buffer_continue(&self, bytes: &[u8]) -> BufferStatus {
        // SAFETY: `raw` is a live instance owned by `self`; `bytes` is readable for its whole
        // length during the call, and the library copies what it keeps before returning.
        let bits = unsafe { (self.api.buffer_continue)(self.raw, bytes.as_ptr(), bytes.len()) };
        BufferStatus(bits)
    }

    /// Where the library wants the next bytes to come from, if it wants a jump.
    pub(super) fn buffer_goto(&self) -> Option<u64> {
        // SAFETY: `raw` is a live instance owned by `self`; the call only reads library state.
        let position = unsafe { (self.api.buffer_goto)(self.raw) };
        (position != u64::MAX).then_some(position)
    }

    /// Ends buffer reading so the results can be read.
    pub(super) fn buffer_finalize(&self) {
        // SAFETY: `raw` is a live instance owned by `self`; the status result is not needed.
        unsafe {
            (self.api.buffer_finalize)(self.raw);
        }
    }

    fn count(&self, kind: StreamKind, stream: usize) -> usize {
        // SAFETY: `raw` is a live instance; the kind is one of the library's seven stream kinds.
        // `stream == ALL` asks for the number of streams, otherwise for the stream's field count.
        unsafe { (self.api.count)(self.raw, kind.index() as c_int, stream) }
    }

    fn field(&self, kind: StreamKind, stream: usize, index: usize, info: c_int) -> String {
        // SAFETY: `raw` is a live instance; an out-of-range index returns an empty string rather
        // than failing. The returned text belongs to the instance and is copied at once.
        unsafe {
            from_wide((self.api.get_i)(
                self.raw,
                kind.index() as c_int,
                stream,
                index,
                info,
            ))
        }
    }

    /// Reads every non-empty field of every stream.
    pub(super) fn snapshot(&self) -> MediaSnapshot {
        let mut snapshot = MediaSnapshot::new();
        for kind in StreamKind::ALL {
            for stream in 0..self.count(kind, ALL) {
                let mut info = StreamInfo::new();
                for index in 0..self.count(kind, stream) {
                    let name = self.field(kind, stream, index, INFO_NAME);
                    if !name.is_empty() {
                        info.push(name, self.field(kind, stream, index, INFO_TEXT));
                    }
                }
                snapshot.push(kind, info);
            }
        }
        snapshot
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: `raw` is the instance made in `new`, closed and freed exactly once here. The
        // `Arc<Api>` field keeps the library mapped until after this runs.
        unsafe {
            (self.api.close)(self.raw);
            (self.api.delete)(self.raw);
        }
    }
}

/// `text` as a NUL-terminated string in the library's `wchar_t` width.
fn to_wide(text: &OsStr) -> Vec<WChar> {
    #[cfg(windows)]
    let mut wide: Vec<WChar> = std::os::windows::ffi::OsStrExt::encode_wide(text).collect();
    #[cfg(not(windows))]
    let mut wide: Vec<WChar> = text.to_string_lossy().chars().map(u32::from).collect();
    wide.push(0);
    wide
}

/// Copies a NUL-terminated `wchar_t` string; null is the empty string.
///
/// # Safety
/// `ptr` must be null or point to a readable, NUL-terminated string.
unsafe fn from_wide(ptr: *const WChar) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut length = 0;
    // SAFETY: the caller promises a NUL-terminated string, so every offset up to and including
    // the terminator is readable.
    while unsafe { *ptr.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: the `length` units before the terminator were just read.
    let units = unsafe { std::slice::from_raw_parts(ptr, length) };
    #[cfg(windows)]
    return String::from_utf16_lossy(units);
    #[cfg(not(windows))]
    return units
        .iter()
        .map(|&unit| char::from_u32(unit).unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_strings_round_trip_including_non_ascii() {
        let text = "Amélie (2001) 日本語.mkv";
        let wide = to_wide(OsStr::new(text));
        assert_eq!(wide.last(), Some(&0));
        // SAFETY: `wide` is a NUL-terminated buffer that lives through the call.
        assert_eq!(unsafe { from_wide(wide.as_ptr()) }, text);
    }

    #[test]
    fn a_null_string_is_empty() {
        // SAFETY: null is explicitly allowed.
        assert_eq!(unsafe { from_wide(std::ptr::null()) }, "");
    }
}

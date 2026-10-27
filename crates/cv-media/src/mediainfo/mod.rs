//! MediaInfo: technical information about media files.
//!
//! The MediaInfo library is **loaded at run time** ([`MediaInfoLibrary::load`], or
//! [`MediaInfoLibrary::from_tools`] for the copy the tools manager found), never linked, so the
//! app starts without it and the Settings page can offer the download.
//!
//! * [`MediaInfoLibrary`] reads one file synchronously. Each call creates its own library
//!   instance, so calls from many threads never share state and need no global lock.
//! * [`MediaInfoPool`] runs many reads in parallel up to a limit, one at a time per drive, gives
//!   each file a time limit that grows with its size (120 s plus 3 s per gigabyte by default) and
//!   never lets one bad file stop a batch.
//! * [`MediaInfoLibrary::read_buffered`] feeds bytes from any [`ByteSource`] to the library's
//!   buffer interface, which is how disc images are analysed without mounting them (see
//!   [`crate::disc`]).
//! * [`read_sidecar`] and [`write_sidecar`] keep a snapshot next to the video as standard
//!   MediaInfo XML, a cache that spares reading the video (see [`sidecar_paths`]).
//! * [`MediaSnapshot`] is the raw result: every stream and its non-empty MediaInfo fields.
//! * [`apply_snapshot`] maps those fields to CineVault's stream model (`cv_domain::MediaFile`:
//!   codec variants, channels, HDR, 3D, languages and flags). The mapping is pure, so it also
//!   serves the XML sidecar cache and recorded fixtures.
//!
//! The FFI lives in one private module (`ffi`), the only place in the workspace allowed to use
//! `unsafe`.

mod ffi;
mod mapping;
mod pool;
mod sidecar;
mod snapshot;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::tools::{ToolError, ToolId, ToolsManager};

pub use mapping::{
    Halve, apply_snapshot, audio_codec, detect_3d, dolby_vision_profile, hdr_details, hdr_formats,
    language, map_audio_stream, map_subtitle_stream, video_3d_from_name, video_codec,
};
pub use pool::{
    Analyzer, DEFAULT_PER_GB, DEFAULT_TIMEOUT, MAX_TIMEOUT, MediaInfoPool, PoolOptions, volume_key,
};
pub use sidecar::{
    SidecarError, SidecarState, SidecarWrite, parse_sidecar, read_sidecar, render_sidecar,
    sidecar_paths, worth_writing, write_sidecar,
};
pub use snapshot::{BatchItem, MediaSnapshot, StreamInfo, StreamKind};

/// Why a file could not be read, or the library could not be loaded. Every message is ready to
/// show.
#[derive(Debug, thiserror::Error)]
pub enum MediaInfoError {
    #[error("The MediaInfo library at {} could not be loaded: {message}", .path.display())]
    Load { path: PathBuf, message: String },
    #[error("{} is not a usable MediaInfo library: {reason}", .path.display())]
    Unusable { path: PathBuf, reason: String },
    #[error("MediaInfo could not open {}.", .path.display())]
    Open { path: PathBuf },
    #[error("Reading {} took longer than {seconds} seconds, so it was skipped.", .path.display())]
    Timeout { path: PathBuf, seconds: u64 },
    #[error("MediaInfo stopped unexpectedly while reading {}.", .path.display())]
    Crashed { path: PathBuf },
    #[error("Reading {} was cancelled.", .path.display())]
    Cancelled { path: PathBuf },
    #[error("MediaInfo ran out of memory.")]
    OutOfMemory,
    #[error("The bytes of {name} could not be read: {message}")]
    Source { name: String, message: String },
    #[error("A reader thread could not be started: {0}")]
    Thread(#[source] std::io::Error),
    #[error(transparent)]
    Tool(#[from] ToolError),
}

impl MediaInfoError {
    /// A short stable name for the kind of failure, for the UI and job reports to branch on.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Load { .. } => "load",
            Self::Unusable { .. } => "unusable",
            Self::Open { .. } => "open",
            Self::Timeout { .. } => "timeout",
            Self::Crashed { .. } => "crashed",
            Self::Cancelled { .. } => "cancelled",
            Self::OutOfMemory => "memory",
            Self::Source { .. } => "source",
            Self::Thread(_) => "thread",
            Self::Tool(_) => "tool",
        }
    }
}

/// Random access to the bytes of one file that is not (or not directly) a file on disk: a file
/// inside a disc image, for example.
pub trait ByteSource {
    /// The whole length in bytes.
    fn len(&self) -> u64;

    /// True for an empty source.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Reads up to `buf.len()` bytes starting at `offset`; 0 means the end.
    ///
    /// # Errors
    /// Any I/O error of the underlying storage.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize>;
}

/// How much of a source [`MediaInfoLibrary::read_buffered`] hands to the library at most.
pub const DEFAULT_BUFFER_LIMIT: u64 = 64 * 1024 * 1024;

/// A loaded MediaInfo library. Cheap to clone; all clones share one mapping.
#[derive(Clone)]
pub struct MediaInfoLibrary {
    api: Arc<ffi::Api>,
    path: PathBuf,
    version: String,
}

impl std::fmt::Debug for MediaInfoLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaInfoLibrary")
            .field("path", &self.path)
            .field("version", &self.version)
            .finish()
    }
}

impl MediaInfoLibrary {
    /// Loads the MediaInfo library at `path` (`MediaInfo.dll`, `libmediainfo.0.dylib`).
    ///
    /// # Errors
    /// [`MediaInfoError::Load`] when the file cannot be loaded as a library,
    /// [`MediaInfoError::Unusable`] when it is a library but not MediaInfo.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, MediaInfoError> {
        let path = path.as_ref();
        // The library sets up its global options on the first call, which is not safe to race
        // with another thread doing the same; loading is rare, so it is serialised.
        static LOADING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOADING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let api = ffi::Api::load(path)?;
        let reported = api.version();
        let version = reported
            .strip_prefix("MediaInfoLib - v")
            .unwrap_or(&reported)
            .trim()
            .to_owned();
        if !reported.starts_with("MediaInfoLib") {
            return Err(MediaInfoError::Unusable {
                path: path.to_path_buf(),
                reason: "it does not report itself as MediaInfo".to_owned(),
            });
        }
        Ok(Self {
            api: Arc::new(api),
            path: path.to_path_buf(),
            version,
        })
    }

    /// Loads the library the tools manager resolves (the user's path, else the verified
    /// download) and tells the manager which version it is, for the Settings page.
    ///
    /// # Errors
    /// [`MediaInfoError::Tool`] when no library is installed or the user's path is gone; the
    /// loading errors of [`Self::load`] otherwise.
    pub fn from_tools(tools: &ToolsManager) -> Result<Self, MediaInfoError> {
        let resolved = tools.resolve(ToolId::MediaInfo)?;
        let library = Self::load(&resolved.path)?;
        tools.report_loaded_version(ToolId::MediaInfo, library.version());
        Ok(library)
    }

    /// The version the library reports, e.g. `26.10`.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Where the library was loaded from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads one file on the calling thread, with no time limit. Use [`MediaInfoPool`] for
    /// parallel reads with a limit.
    ///
    /// A file the library can open but finds nothing in (empty, corrupt) is not an error: the
    /// snapshot has a general section only ([`MediaSnapshot::has_media_streams`]).
    ///
    /// # Errors
    /// [`MediaInfoError::Open`] when the library cannot open the path (missing, unreadable).
    pub fn read(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
        // A rename of the folder waits for this read (see `cv_fs::reading`); the guard is dropped
        // after the handle, which closes the file.
        let _reading = cv_fs::reading(path);
        let handle = ffi::Handle::new(&self.api)?;
        if !handle.open(path) {
            return Err(MediaInfoError::Open {
                path: path.to_path_buf(),
            });
        }
        Ok(handle.snapshot())
    }
}

impl MediaInfoLibrary {
    /// Reads a file from `source` through the library's buffer interface.
    ///
    /// The bytes are fed in order from the start. When the library asks for another position
    /// (a transport stream's end, to find the duration) the read jumps there. It stops when the
    /// library says it has enough, or after `limit` bytes in total, and reports what it found.
    /// `name` is used in error messages only.
    ///
    /// # Errors
    /// [`MediaInfoError::Source`] when the source cannot be read,
    /// [`MediaInfoError::OutOfMemory`] when no instance can be made.
    pub fn read_buffered(
        &self,
        name: &str,
        source: &mut dyn ByteSource,
        limit: u64,
    ) -> Result<MediaSnapshot, MediaInfoError> {
        let handle = ffi::Handle::new(&self.api)?;
        let size = source.len();
        let mut buffer = vec![0_u8; 1 << 20];
        let mut offset = 0_u64;
        let mut fed = 0_u64;
        handle.buffer_init(size, 0);
        while fed < limit {
            let read = source
                .read_at(offset, &mut buffer)
                .map_err(|e| MediaInfoError::Source {
                    name: name.to_owned(),
                    message: e.to_string(),
                })?;
            if read == 0 {
                break;
            }
            let status = handle.buffer_continue(&buffer[..read]);
            offset += read as u64;
            fed += read as u64;
            if status.finalized() || status.filled() {
                break;
            }
            if let Some(position) = handle.buffer_goto() {
                if position >= size {
                    break;
                }
                offset = position;
                handle.buffer_init(size, position);
            }
        }
        handle.buffer_finalize();
        Ok(handle.snapshot())
    }
}

impl Analyzer for MediaInfoLibrary {
    fn analyze(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
        self.read(path)
    }
}

impl MediaInfoPool<MediaInfoLibrary> {
    /// A pool over the library the tools manager resolves, with default options.
    ///
    /// # Errors
    /// As [`MediaInfoLibrary::from_tools`].
    pub fn from_tools(tools: &ToolsManager, options: PoolOptions) -> Result<Self, MediaInfoError> {
        Ok(Self::new(MediaInfoLibrary::from_tools(tools)?, options))
    }
}

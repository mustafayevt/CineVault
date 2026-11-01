//! Network shares: connecting an SMB share with a login of its own, so a library folder can live
//! on a NAS whose shares need a user and password other than the computer's sign-in.
//!
//! * **The system's own tools, no FFI.** Each operating system has a [`ShareConnector`] that
//!   starts a system tool with a fixed argument list. Arguments and environment carry only the
//!   server, share, user and paths; the password goes to the tool's standard input (or its
//!   terminal), only after the tool asked for it, and is never logged. [`Password`] never shows in
//!   `Debug` or `Display`, and tool output is scrubbed of it before it becomes an error message.
//! * **One Address field.** [`Address::parse`] reads `\\nas\movies\Films`, `//nas/movies/Films`
//!   and `smb://nas/movies/Films`, and every part is checked before a tool starts.
//! * **Plain reasons.** A failure has a [`ShareErrorKind`] (wrong login, unreachable, share not
//!   found, another user connected, …) and keeps the system's own message for anything else.
//! * **Nothing on the share is touched.** Connecting and disconnecting write, move and delete no
//!   file on a share.
//!
//! [`fake::FakeConnector`] stands in for a real connector in the tests of the services above.

mod address;
mod connector;
mod error;
pub mod fake;
#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(any(target_os = "macos", test))]
mod macos;
mod process;
#[cfg(any(windows, test))]
mod windows;

use std::sync::Arc;

pub use address::{Address, InputError, ShareAddress, check_user, folder_in_share};
pub use connector::{Cancel, Connected, Login, Password, ShareConnector, Support};
pub use error::{ShareError, ShareErrorKind};

/// The connector of this operating system: `WNetAddConnection2W` on Windows, `mount_smbfs` on macOS,
/// `gio` on a Linux desktop.
#[must_use]
pub fn system_connector() -> Arc<dyn ShareConnector> {
    #[cfg(windows)]
    {
        Arc::new(windows::WindowsConnector::new())
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::MacConnector::new())
    }
    #[cfg(target_os = "linux")]
    {
        Arc::new(linux::LinuxConnector::new())
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Arc::new(Unsupported)
    }
}

/// The mount folder below the home folder on macOS: `~/CineVault Shares/<server>/<share>`.
pub const MAC_MOUNT_FOLDER: &str = "CineVault Shares";

/// Any other system: shares are connected with the system's own tools, then added as folders.
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
struct Unsupported;

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl ShareConnector for Unsupported {
    fn support(&self) -> Support {
        Support::NoUserMounting
    }

    fn connect(
        &self,
        _share: &ShareAddress,
        _login: &Login,
        _cancel: &Cancel,
    ) -> Result<Connected, ShareError> {
        Err(ShareError::new(
            ShareErrorKind::NoUserMounting,
            "this system cannot connect network shares for apps",
        ))
    }

    fn is_connected(&self, _share: &ShareAddress, _cancel: &Cancel) -> Result<bool, ShareError> {
        Ok(false)
    }

    fn disconnect(&self, _share: &ShareAddress, _cancel: &Cancel) -> Result<(), ShareError> {
        Ok(())
    }

    fn local_root(
        &self,
        _share: &ShareAddress,
        _cancel: &Cancel,
    ) -> Result<Option<std::path::PathBuf>, ShareError> {
        Ok(None)
    }
}

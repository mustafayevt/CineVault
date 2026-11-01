//! Why a share could not be connected, checked or disconnected.

use std::fmt;

/// The kinds of failure the Network share page and the folder rows tell apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShareErrorKind {
    /// The server refused the user name or password.
    WrongLogin,
    /// The server could not be reached, or did not answer in time.
    Unreachable,
    /// The server has no share by that name.
    ShareNotFound,
    /// The share is connected, but the folder inside it is not there.
    FolderNotFound,
    /// Windows is already connected to this server as another user (error 1219).
    OtherUserConnected,
    /// Windows still holds its sign-in to this server from an earlier connection, with the
    /// earlier password (error 1219 with no connection to the server in sight): a restart or a
    /// Windows sign-out ends it.
    SessionInUse,
    /// The server locked the account.
    AccountLocked,
    /// Files on the share are open, so it cannot be disconnected now.
    InUse,
    /// This computer offers no way for an app to connect a share (a Linux desktop without
    /// user-level share mounting).
    NoUserMounting,
    /// The user stopped the attempt.
    Cancelled,
    /// Anything else; the error carries the system's message.
    Other,
}

impl ShareErrorKind {
    /// The code stored for a failed connect (`network_share.last_error`). It never holds user input.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::WrongLogin => "wrong_login",
            Self::Unreachable => "unreachable",
            Self::ShareNotFound => "share_not_found",
            Self::FolderNotFound => "folder_not_found",
            Self::OtherUserConnected => "other_user_connected",
            Self::SessionInUse => "session_in_use",
            Self::AccountLocked => "account_locked",
            Self::InUse => "in_use",
            Self::NoUserMounting => "no_user_mounting",
            Self::Cancelled => "cancelled",
            Self::Other => "other",
        }
    }

    /// The kind stored as `code`.
    #[must_use]
    pub fn parse(code: &str) -> Option<Self> {
        [
            Self::WrongLogin,
            Self::Unreachable,
            Self::ShareNotFound,
            Self::FolderNotFound,
            Self::OtherUserConnected,
            Self::SessionInUse,
            Self::AccountLocked,
            Self::InUse,
            Self::NoUserMounting,
            Self::Cancelled,
            Self::Other,
        ]
        .into_iter()
        .find(|kind| kind.code() == code)
    }
}

/// A failed share operation: its kind, the system's error number when there is one, and the
/// system's own message. The message comes from the tool's output with the password removed;
/// callers still pass it through the log redaction before writing it anywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareError {
    kind: ShareErrorKind,
    code: Option<i64>,
    message: String,
    /// The tool was stopped before it ended: by the user, or because it ran out of time.
    interrupted: bool,
}

impl ShareError {
    pub fn new(kind: ShareErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            code: None,
            message: message.into(),
            interrupted: false,
        }
    }

    /// The same error with the system's error number.
    #[must_use]
    pub fn with_code(mut self, code: i64) -> Self {
        self.code = Some(code);
        self
    }

    #[must_use]
    pub fn kind(&self) -> ShareErrorKind {
        self.kind
    }

    /// The system's error number (a Win32 code on Windows, an exit status elsewhere).
    #[must_use]
    pub fn code(&self) -> Option<i64> {
        self.code
    }

    /// The system's message, for "Other" and for the log.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether the tool was stopped before it ended (cancelled, or out of time), so the system
    /// may have connected the share after all.
    #[must_use]
    pub fn is_interrupted(&self) -> bool {
        self.interrupted
    }

    pub(crate) fn cancelled() -> Self {
        Self {
            interrupted: true,
            ..Self::new(ShareErrorKind::Cancelled, "stopped")
        }
    }

    /// A tool that did not end in time: the server counts as not reachable.
    pub(crate) fn timed_out() -> Self {
        Self {
            interrupted: true,
            ..Self::new(
                ShareErrorKind::Unreachable,
                "the server did not answer in time",
            )
        }
    }

    pub(crate) fn no_user_mounting() -> Self {
        Self::new(
            ShareErrorKind::NoUserMounting,
            "this desktop cannot connect network shares for apps",
        )
    }
}

impl fmt::Display for ShareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self.kind {
            ShareErrorKind::WrongLogin => "Wrong user name or password.",
            ShareErrorKind::Unreachable => "The server could not be reached.",
            ShareErrorKind::ShareNotFound => "There is no share by that name on the server.",
            ShareErrorKind::FolderNotFound => "The folder was not found on the share.",
            ShareErrorKind::OtherUserConnected => {
                "Windows is already connected to this server as another user."
            }
            ShareErrorKind::SessionInUse => {
                "Windows is still signed in to this server with the earlier password. Restart CineVault (or sign out of Windows and back in), then connect again."
            }
            ShareErrorKind::AccountLocked => "The server has locked this account.",
            ShareErrorKind::InUse => "Files on the share are open.",
            ShareErrorKind::NoUserMounting => {
                "This desktop cannot connect network shares for apps."
            }
            ShareErrorKind::Cancelled => "Connecting was stopped.",
            ShareErrorKind::Other => {
                return match self.code {
                    Some(code) => write!(f, "{} (error {code})", self.message),
                    None => f.write_str(&self.message),
                };
            }
        };
        f.write_str(text)
    }
}

impl std::error::Error for ShareError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_reads_back_from_its_code() {
        for kind in [
            ShareErrorKind::WrongLogin,
            ShareErrorKind::Unreachable,
            ShareErrorKind::ShareNotFound,
            ShareErrorKind::FolderNotFound,
            ShareErrorKind::OtherUserConnected,
            ShareErrorKind::SessionInUse,
            ShareErrorKind::AccountLocked,
            ShareErrorKind::InUse,
            ShareErrorKind::NoUserMounting,
            ShareErrorKind::Cancelled,
            ShareErrorKind::Other,
        ] {
            assert_eq!(ShareErrorKind::parse(kind.code()), Some(kind));
        }
        assert_eq!(ShareErrorKind::parse("nonsense"), None);
    }

    #[test]
    fn other_errors_show_the_system_message_and_code() {
        let error = ShareError::new(ShareErrorKind::Other, "Access is denied.").with_code(5);
        assert_eq!(error.to_string(), "Access is denied. (error 5)");
        let plain = ShareError::new(ShareErrorKind::WrongLogin, "logon failure");
        assert_eq!(plain.to_string(), "Wrong user name or password.");
        assert_eq!(plain.message(), "logon failure");
    }
}

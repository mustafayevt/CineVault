//! The connector interface, the login it takes and the stop signal of a running attempt.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::address::{InputError, ShareAddress, check_user};
use crate::error::ShareError;

/// Whether this computer can connect shares for CineVault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Supported,
    /// A Linux desktop without user-level share mounting: the page explains the system's own way.
    NoUserMounting,
}

/// Connects SMB shares with a login through the operating system's own tools.
///
/// Every implementation passes the password on the tool's standard input (or its terminal), never
/// in an argument or the environment, and never logs it.
pub trait ShareConnector: Send + Sync {
    /// Whether shares can be connected here at all.
    fn support(&self) -> Support;

    /// Connects `share` with `login` and returns the folder its files appear in, and whether this
    /// call made the connection. A share that is already connected (by the user, or earlier) is
    /// not connected again and comes back with [`Connected::made`] false: `login` is not checked
    /// then.
    ///
    /// # Errors
    /// A [`ShareError`] whose kind says why; also when the system cannot say whether the share is
    /// connected already.
    fn connect(
        &self,
        share: &ShareAddress,
        login: &Login,
        cancel: &Cancel,
    ) -> Result<Connected, ShareError>;

    /// Whether `share` is connected now.
    ///
    /// # Errors
    /// When the system cannot be asked.
    fn is_connected(&self, share: &ShareAddress, cancel: &Cancel) -> Result<bool, ShareError>;

    /// Ends the connection to `share`. A share that is not connected is not an error; open files
    /// give [`crate::ShareErrorKind::InUse`].
    ///
    /// # Errors
    /// A [`ShareError`] whose kind says why.
    fn disconnect(&self, share: &ShareAddress, cancel: &Cancel) -> Result<(), ShareError>;

    /// The folder the files of `share` appear in, when that is known now: always on Windows and
    /// macOS, only while it is mounted on Linux.
    ///
    /// # Errors
    /// When the system cannot be asked.
    fn local_root(
        &self,
        share: &ShareAddress,
        cancel: &Cancel,
    ) -> Result<Option<PathBuf>, ShareError>;
}

/// A share that is connected: the folder its files appear in, and whether the connect that
/// returned it made the connection (`false`: it was there already, perhaps made by the user).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connected {
    pub root: PathBuf,
    pub made: bool,
}

/// A share password. `Debug` and `Display` never show it.
#[derive(Clone, PartialEq, Eq)]
pub struct Password(String);

impl Password {
    /// Checks and keeps a password: anything but a line break or a NUL, as it is sent as one line.
    ///
    /// # Errors
    /// [`InputError::BadPassword`].
    pub fn new(value: impl Into<String>) -> Result<Self, InputError> {
        let value = value.into();
        if value.contains(['\n', '\r', '\0']) {
            return Err(InputError::BadPassword);
        }
        Ok(Self(value))
    }

    /// The password itself. Call this only where it is written to a tool's input.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// `text` with every occurrence of the password replaced by `[REDACTED]`. Passwords shorter
    /// than four characters are not searched for, as they would mask ordinary words; tool output
    /// never holds them anyway, since the tools do not echo what they read.
    pub(crate) fn scrub(&self, text: &str) -> String {
        if self.0.len() < 4 {
            text.to_owned()
        } else {
            text.replace(&self.0, "[REDACTED]")
        }
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password([REDACTED])")
    }
}

impl fmt::Display for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// A user and password for a share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    user: String,
    password: Password,
}

impl Login {
    /// Checks the user name (see [`check_user`]) and keeps both.
    ///
    /// # Errors
    /// [`InputError::EmptyUser`] or [`InputError::BadUser`].
    pub fn new(user: &str, password: Password) -> Result<Self, InputError> {
        Ok(Self {
            user: check_user(user)?,
            password,
        })
    }

    /// The user as typed, with its `DOMAIN\` part.
    #[must_use]
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The `DOMAIN` of `DOMAIN\name`.
    #[must_use]
    pub fn domain(&self) -> Option<&str> {
        self.user.split_once('\\').map(|(domain, _)| domain)
    }

    /// The name without its domain.
    #[must_use]
    pub fn name(&self) -> &str {
        self.user
            .split_once('\\')
            .map_or(self.user.as_str(), |(_, name)| name)
    }

    #[must_use]
    pub fn password(&self) -> &Password {
        &self.password
    }
}

/// Stops a running attempt: when [`Cancel::cancel`] is called, or when its time limit passes.
/// Clones share the signal.
#[derive(Debug, Clone, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
    deadline: Option<Instant>,
}

impl Cancel {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A signal that also fires `limit` from now (a reconnect at start allows 15 seconds).
    #[must_use]
    pub fn with_limit(limit: Duration) -> Self {
        Self {
            flag: Arc::default(),
            deadline: Some(Instant::now() + limit),
        }
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Whether [`Cancel::cancel`] was called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Whether the time limit has passed.
    #[must_use]
    pub fn timed_out(&self) -> bool {
        self.deadline.is_some_and(|at| Instant::now() >= at)
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_never_shows_in_formatting() {
        let login = Login::new("alex", Password::new("Example-Pass-42").unwrap()).unwrap();
        for text in [
            format!("{login:?}"),
            format!("{}", login.password()),
            format!("{:?}", login.password()),
        ] {
            assert!(!text.contains("Example-Pass-42"), "{text}");
        }
        let short = Password::new("abc").unwrap();
        assert!(!format!("{short:?} {short}").contains("abc"));
    }

    #[test]
    fn a_password_with_a_line_break_is_refused() {
        for bad in ["a\nb", "a\rb", "a\0b"] {
            assert_eq!(Password::new(bad), Err(InputError::BadPassword));
        }
        assert!(Password::new("spaces and ünïcode ok").is_ok());
        assert!(Password::new("").is_ok());
    }

    #[test]
    fn scrubbing_removes_the_password_from_tool_output() {
        let password = Password::new("Example-Pass-42").unwrap();
        assert_eq!(
            password.scrub("bad Example-Pass-42 here"),
            "bad [REDACTED] here"
        );
        assert_eq!(Password::new("ab").unwrap().scrub("about"), "about");
    }

    #[test]
    fn a_login_splits_its_domain() {
        let login = Login::new(r"HOME\alex", Password::new("x").unwrap()).unwrap();
        assert_eq!((login.domain(), login.name()), (Some("HOME"), "alex"));
        let plain = Login::new("alex", Password::new("x").unwrap()).unwrap();
        assert_eq!((plain.domain(), plain.name()), (None, "alex"));
        assert_eq!(
            Login::new("", Password::new("x").unwrap()),
            Err(InputError::EmptyUser)
        );
    }

    #[test]
    fn a_cancel_fires_by_hand_or_by_time() {
        let cancel = Cancel::new();
        let clone = cancel.clone();
        assert!(!cancel.is_cancelled() && !cancel.timed_out());
        clone.cancel();
        assert!(cancel.is_cancelled());
        assert!(Cancel::with_limit(Duration::ZERO).timed_out());
        assert!(!Cancel::with_limit(Duration::from_secs(60)).timed_out());
    }
}

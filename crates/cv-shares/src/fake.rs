//! A connector for tests of the services above this crate: no tool runs, shares "appear" as
//! folders below a test folder, and every call is recorded.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::address::ShareAddress;
use crate::connector::{Cancel, Connected, Login, Password, ShareConnector, Support};
use crate::error::{ShareError, ShareErrorKind};

/// A call the fake received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeCall {
    Connect {
        share: ShareAddress,
        user: String,
        password: Password,
    },
    IsConnected(ShareAddress),
    Disconnect(ShareAddress),
    LocalRoot(ShareAddress),
}

struct State {
    support: Support,
    connected: HashSet<ShareAddress>,
    connect_errors: HashMap<ShareAddress, ShareError>,
    check_errors: HashMap<ShareAddress, ShareError>,
    disconnect_errors: HashMap<ShareAddress, ShareError>,
    delay: Duration,
    calls: Vec<FakeCall>,
}

/// Connects shares by making their folder `<root>/<server>/<share>`.
pub struct FakeConnector {
    root: PathBuf,
    state: Mutex<State>,
}

impl FakeConnector {
    /// A fake whose shares appear below `root` (a temporary folder of the test).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            state: Mutex::new(State {
                support: Support::Supported,
                connected: HashSet::new(),
                connect_errors: HashMap::new(),
                check_errors: HashMap::new(),
                disconnect_errors: HashMap::new(),
                delay: Duration::ZERO,
                calls: Vec::new(),
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Where `share` appears: `<root>/<server>/<share>`.
    #[must_use]
    pub fn root_of(&self, share: &ShareAddress) -> PathBuf {
        self.root.join(share.server()).join(share.share())
    }

    pub fn set_support(&self, support: Support) {
        self.state().support = support;
    }

    /// Makes every connect of `share` fail with `error` (`None` lets it connect again).
    pub fn fail_connect(&self, share: &ShareAddress, error: Option<ShareError>) {
        let mut state = self.state();
        match error {
            Some(error) => state.connect_errors.insert(share.clone(), error),
            None => state.connect_errors.remove(share),
        };
    }

    /// Makes every check whether `share` is connected fail with `error`, the one a connect makes
    /// first included (`None` answers again).
    pub fn fail_check(&self, share: &ShareAddress, error: Option<ShareError>) {
        let mut state = self.state();
        match error {
            Some(error) => state.check_errors.insert(share.clone(), error),
            None => state.check_errors.remove(share),
        };
    }

    /// Makes every disconnect of `share` fail with `error` (`None` lets it disconnect again).
    pub fn fail_disconnect(&self, share: &ShareAddress, error: Option<ShareError>) {
        let mut state = self.state();
        match error {
            Some(error) => state.disconnect_errors.insert(share.clone(), error),
            None => state.disconnect_errors.remove(share),
        };
    }

    /// Makes every connect take `delay` (stopped early by its [`Cancel`]).
    pub fn set_delay(&self, delay: Duration) {
        self.state().delay = delay;
    }

    /// Marks `share` as connected already, as if by the system, and makes its folder.
    pub fn mark_connected(&self, share: &ShareAddress) {
        let _ = std::fs::create_dir_all(self.root_of(share));
        self.state().connected.insert(share.clone());
    }

    /// The calls so far, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<FakeCall> {
        self.state().calls.clone()
    }

    fn record(&self, call: FakeCall) {
        self.state().calls.push(call);
    }

    fn wait(&self, cancel: &Cancel) -> Result<(), ShareError> {
        let delay = self.state().delay;
        let start = Instant::now();
        while start.elapsed() < delay {
            if cancel.is_cancelled() {
                return Err(ShareError::cancelled());
            }
            if cancel.timed_out() {
                return Err(ShareError::new(
                    ShareErrorKind::Unreachable,
                    "the server did not answer in time",
                ));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }
}

impl ShareConnector for FakeConnector {
    fn support(&self) -> Support {
        self.state().support
    }

    fn connect(
        &self,
        share: &ShareAddress,
        login: &Login,
        cancel: &Cancel,
    ) -> Result<Connected, ShareError> {
        self.record(FakeCall::Connect {
            share: share.clone(),
            user: login.user().to_owned(),
            password: login.password().clone(),
        });
        if self.support() == Support::NoUserMounting {
            return Err(ShareError::no_user_mounting());
        }
        if let Some(error) = self.state().check_errors.get(share) {
            return Err(error.clone());
        }
        if self.state().connected.contains(share) {
            // As the real connectors do: an existing connection is used as it is, login unchecked.
            return Ok(Connected {
                root: self.root_of(share),
                made: false,
            });
        }
        self.wait(cancel)?;
        if let Some(error) = self.state().connect_errors.get(share) {
            return Err(error.clone());
        }
        let root = self.root_of(share);
        std::fs::create_dir_all(&root)
            .map_err(|error| ShareError::new(ShareErrorKind::Other, error.to_string()))?;
        self.state().connected.insert(share.clone());
        Ok(Connected { root, made: true })
    }

    fn is_connected(&self, share: &ShareAddress, _cancel: &Cancel) -> Result<bool, ShareError> {
        self.record(FakeCall::IsConnected(share.clone()));
        let state = self.state();
        if let Some(error) = state.check_errors.get(share) {
            return Err(error.clone());
        }
        Ok(state.connected.contains(share))
    }

    fn disconnect(&self, share: &ShareAddress, _cancel: &Cancel) -> Result<(), ShareError> {
        self.record(FakeCall::Disconnect(share.clone()));
        let mut state = self.state();
        if let Some(error) = state.disconnect_errors.get(share) {
            return Err(error.clone());
        }
        state.connected.remove(share);
        Ok(())
    }

    fn local_root(
        &self,
        share: &ShareAddress,
        _cancel: &Cancel,
    ) -> Result<Option<PathBuf>, ShareError> {
        self.record(FakeCall::LocalRoot(share.clone()));
        Ok(Some(self.root_of(share)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn share() -> ShareAddress {
        ShareAddress::new("nas", "movies").unwrap()
    }

    fn login() -> Login {
        Login::new("alex", Password::new("Example-Pass-42").unwrap()).unwrap()
    }

    #[test]
    fn a_connect_makes_the_folder_and_is_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeConnector::new(dir.path());
        let connected = fake.connect(&share(), &login(), &Cancel::new()).unwrap();
        assert!(connected.made);
        let root = connected.root;
        assert!(root.is_dir());
        let again = fake.connect(&share(), &login(), &Cancel::new()).unwrap();
        assert_eq!((again.root.as_path(), again.made), (root.as_path(), false));
        assert!(fake.is_connected(&share(), &Cancel::new()).unwrap());
        fake.disconnect(&share(), &Cancel::new()).unwrap();
        assert!(!fake.is_connected(&share(), &Cancel::new()).unwrap());
        assert!(root.is_dir(), "disconnecting touches no files");
        assert!(matches!(
            &fake.calls()[0],
            FakeCall::Connect { user, .. } if user == "alex"
        ));
        assert!(!format!("{:?}", fake.calls()).contains("Example-Pass-42"));
    }

    #[test]
    fn failures_delays_and_missing_support_are_played_back() {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeConnector::new(dir.path());
        fake.fail_connect(
            &share(),
            Some(ShareError::new(ShareErrorKind::WrongLogin, "no")),
        );
        let error = fake
            .connect(&share(), &login(), &Cancel::new())
            .unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::WrongLogin);
        fake.fail_connect(&share(), None);

        fake.set_delay(Duration::from_secs(30));
        let slow = fake
            .connect(
                &share(),
                &login(),
                &Cancel::with_limit(Duration::from_millis(20)),
            )
            .unwrap_err();
        assert_eq!(slow.kind(), ShareErrorKind::Unreachable);
        fake.set_delay(Duration::ZERO);

        fake.fail_disconnect(
            &share(),
            Some(ShareError::new(ShareErrorKind::InUse, "open files")),
        );
        assert_eq!(
            fake.disconnect(&share(), &Cancel::new())
                .unwrap_err()
                .kind(),
            ShareErrorKind::InUse
        );

        fake.set_support(Support::NoUserMounting);
        let error = fake
            .connect(&share(), &login(), &Cancel::new())
            .unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::NoUserMounting);
    }
}

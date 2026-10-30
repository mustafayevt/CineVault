//! Sign-in for the browser server: one user, one password, server-side sessions.
//!
//! On the headless server, the first start has no password. The welcome page lets the first
//! browser on the home network set one, for as long as none exists; a browser from a public
//! address is told to finish setup from home. A forgotten password is reset with a one-time PIN
//! written to a file in the data folder, or with `cinevault-server passwd` on the server.
//!
//! In the desktop app's Browser access the password is set in the app's Settings, so there is no
//! welcome page and no PIN; the sign-in page says where to change it.
//!
//! The password is argon2id with a random salt, stored in `server-auth.json` written atomically
//! with mode `0600`. Sessions are random ids in memory, never in a file, so a restart (or turning
//! Browser access off) signs everyone out.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use argon2::Argon2;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

/// The file the password hash lives in, inside the data folder.
pub const AUTH_FILE: &str = "server-auth.json";
/// The file a password-reset PIN is written to, inside the data folder.
pub const PIN_FILE: &str = "password-reset-pin.txt";
const VERSION: u32 = 1;
/// A setup claim that is not used for this long lapses, so an abandoned tab cannot hold setup.
const CLAIM_LAPSE: Duration = Duration::from_secs(30 * 60);
const SESSION_IDLE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const SESSION_MAX: Duration = Duration::from_secs(90 * 24 * 60 * 60);
const MAX_SESSIONS: usize = 32;
const MAX_FAILURES: usize = 5;
const MAX_GLOBAL_FAILURES: usize = 100;
const FAILURE_WINDOW: Duration = Duration::from_secs(60);
/// How long a reset PIN is valid.
pub const PIN_LIFETIME: Duration = Duration::from_secs(15 * 60);
/// Wrong PINs before the PIN is void.
const PIN_ATTEMPTS: u32 = 5;
/// How long after a PIN ends before another can be made.
const PIN_COOLDOWN: Duration = Duration::from_secs(60);
/// How many digits a PIN has.
const PIN_DIGITS: u32 = 8;
/// The longest browser description kept for a session.
const AGENT_MAX: usize = 200;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AuthFile {
    version: u32,
    /// The PHC string, which already carries the salt and the parameters.
    hash: String,
}

/// Where the sign-in runs: the headless server, or the desktop app's Browser access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostMode {
    Server,
    /// The desktop app on the named computer; the password is set in its Settings.
    Desktop {
        computer: String,
    },
}

#[derive(Clone)]
pub struct Session {
    pub csrf: String,
    /// A public id for the session list; the cookie value never leaves the server.
    pub handle: String,
    pub address: String,
    pub agent: String,
    pub signed_in: SystemTime,
    pub last_used: SystemTime,
    created: Instant,
    seen: Instant,
}

/// One signed-in browser, as the desktop's Browser access section lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub handle: String,
    pub address: String,
    pub agent: String,
    /// Milliseconds since the epoch.
    pub signed_in_ms: u64,
    pub last_used_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("The password must be at least 12 characters.")]
    TooShort,
    #[error("The passwords do not match.")]
    Mismatch,
    #[error("That password is not right.")]
    WrongPassword,
    #[error("Too many attempts. Wait a minute and try again.")]
    TooManyAttempts,
    #[error("No password is set yet. Open CineVault from your home network to create one.")]
    NoPassword,
    #[error("A password is already set. Sign in instead.")]
    AlreadySet,
    #[error("Setup is in progress in another browser.")]
    SetupClaimed,
    #[error("Finish setup from your home network.")]
    NotHomeNetwork,
    #[error("The server's sign-in file could not be read: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "The server's sign-in file is damaged. Move it aside and restart to set a new password."
    )]
    Damaged,
    #[error("The password could not be hashed: {0}")]
    Hash(String),
    #[error("Change it in CineVault's Settings on {0}.")]
    DesktopReset(String),
    #[error("A PIN was made less than a minute ago. Wait a minute and ask again.")]
    ResetTooSoon,
    #[error("There is no reset PIN, or it expired. Ask for a new one.")]
    NoPin,
    #[error("That PIN is not right. {0} tries left.")]
    WrongPin(u32),
    #[error("That PIN is not right. It is no longer valid; ask for a new one in a minute.")]
    PinVoid,
}

/// A reset PIN that is live.
struct ResetPin {
    pin: String,
    expires: Instant,
    attempts: u32,
}

#[derive(Default)]
struct ResetState {
    active: Option<ResetPin>,
    ended: Option<Instant>,
}

/// The answer to "Forgot password?": where the PIN is, and whether this request made it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResetStarted {
    pub path: PathBuf,
    pub created: bool,
}

/// The password, the live sessions, the welcome claim and the reset PIN.
pub struct Auth {
    path: PathBuf,
    data_dir: PathBuf,
    host: HostMode,
    password: Mutex<Option<AuthFile>>,
    sessions: Mutex<HashMap<String, Session>>,
    setup_claim: Mutex<Option<(String, Instant)>>,
    failures: Mutex<Failures>,
    reset: Mutex<ResetState>,
}

#[derive(Default)]
struct Failures {
    per_client: HashMap<String, Vec<Instant>>,
    global: Vec<Instant>,
}

impl Failures {
    fn prune(&mut self, now: Instant) {
        self.per_client.retain(|_, attempts| {
            attempts.retain(|at| now.duration_since(*at) <= FAILURE_WINDOW);
            !attempts.is_empty()
        });
        self.global
            .retain(|at| now.duration_since(*at) <= FAILURE_WINDOW);
    }

    fn blocked(&self, client: &str) -> bool {
        self.global.len() >= MAX_GLOBAL_FAILURES
            || self
                .per_client
                .get(client)
                .is_some_and(|attempts| attempts.len() >= MAX_FAILURES)
    }

    fn record(&mut self, client: &str, now: Instant) {
        self.per_client
            .entry(client.to_owned())
            .or_default()
            .push(now);
        self.global.push(now);
    }

    fn clear(&mut self, client: &str) {
        self.per_client.remove(client);
    }
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

fn hash_password(password: &str) -> Result<AuthFile, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| AuthError::Hash(error.to_string()))?
        .to_string();
    Ok(AuthFile {
        version: VERSION,
        hash,
    })
}

/// Checks the length and the confirmation of a new password.
pub fn check_new_password(password: &str, confirm: &str) -> Result<(), AuthError> {
    if password.chars().count() < 12 {
        return Err(AuthError::TooShort);
    }
    if password != confirm {
        return Err(AuthError::Mismatch);
    }
    Ok(())
}

impl Auth {
    /// Opens the sign-in state of a data folder for the headless server. A damaged file is
    /// refused rather than silently treated as "no password".
    pub fn open(data_dir: &Path) -> Result<Self, AuthError> {
        Self::open_as(data_dir, HostMode::Server)
    }

    /// Opens the sign-in state for the given host.
    pub fn open_as(data_dir: &Path, host: HostMode) -> Result<Self, AuthError> {
        let path = data_dir.join(AUTH_FILE);
        let password = match std::fs::read(&path) {
            Ok(bytes) => {
                let file: AuthFile =
                    serde_json::from_slice(&bytes).map_err(|_| AuthError::Damaged)?;
                if file.version != VERSION {
                    return Err(AuthError::Damaged);
                }
                Some(file)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(AuthError::Io(error)),
        };
        // A PIN lives in memory; one left in a file by an earlier run can no longer be used.
        let _ = std::fs::remove_file(data_dir.join(PIN_FILE));
        Ok(Self {
            path,
            data_dir: data_dir.to_path_buf(),
            host,
            password: Mutex::new(password),
            sessions: Mutex::new(HashMap::new()),
            setup_claim: Mutex::new(None),
            failures: Mutex::new(Failures::default()),
            reset: Mutex::new(ResetState::default()),
        })
    }

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Where this sign-in runs.
    pub fn host(&self) -> &HostMode {
        &self.host
    }

    pub fn has_password(&self) -> bool {
        self.current().is_some()
    }

    /// The password as it is on disk now. `cinevault-server passwd` and the desktop's Settings
    /// write the file while the server runs, and the new password must work without a restart. A
    /// file that cannot be read or parsed keeps the password already in memory.
    fn current(&self) -> Option<AuthFile> {
        let mut guard = Self::lock(&self.password);
        if let Ok(bytes) = std::fs::read(&self.path)
            && let Ok(file) = serde_json::from_slice::<AuthFile>(&bytes)
            && file.version == VERSION
        {
            *guard = Some(file);
        }
        (*guard).clone()
    }

    /// True on the headless server while no password exists. The desktop never shows the
    /// welcome page: its password is set in the app.
    pub fn needs_setup(&self) -> bool {
        self.host == HostMode::Server && !self.has_password()
    }

    /// Claims the setup for this browser, or checks the claim it already holds. A claim that was
    /// not used for 30 minutes lapses and can be taken by another browser.
    pub fn claim_setup(&self, existing: Option<&str>) -> Result<String, AuthError> {
        self.claim_setup_at(existing, Instant::now())
    }

    fn claim_setup_at(&self, existing: Option<&str>, now: Instant) -> Result<String, AuthError> {
        if !self.needs_setup() {
            return Err(AuthError::AlreadySet);
        }
        let mut claim = Self::lock(&self.setup_claim);
        if let Some((current, used)) = claim.as_mut() {
            if existing == Some(current.as_str()) {
                *used = now;
                return Ok(current.clone());
            }
            if now.duration_since(*used) < CLAIM_LAPSE {
                return Err(AuthError::SetupClaimed);
            }
        }
        let id = random_hex(16);
        *claim = Some((id.clone(), now));
        Ok(id)
    }

    /// Sets the first password from the welcome page and signs the browser in.
    pub fn set_password(
        &self,
        password: &str,
        confirm: &str,
        claim: &str,
        address: &str,
        agent: &str,
    ) -> Result<(String, Session), AuthError> {
        if !self.needs_setup() {
            return Err(AuthError::AlreadySet);
        }
        if Self::lock(&self.setup_claim)
            .as_ref()
            .map(|(id, _)| id.as_str())
            != Some(claim)
        {
            return Err(AuthError::SetupClaimed);
        }
        check_new_password(password, confirm)?;
        self.write(hash_password(password)?)?;
        *Self::lock(&self.setup_claim) = None;
        Ok(self.create_session(address, agent))
    }

    fn write(&self, file: AuthFile) -> Result<(), AuthError> {
        let bytes =
            serde_json::to_vec(&file).map_err(|error| AuthError::Hash(error.to_string()))?;
        cv_fs::write_atomic_with_mode(&self.path, &bytes, 0o600)?;
        *Self::lock(&self.password) = Some(file);
        Ok(())
    }

    /// Sets the password without the setup claim or a session: `cinevault-server passwd` on the
    /// machine that holds the data folder, and the desktop's Settings. Being at the machine is
    /// the trust boundary. The caller ends the sessions that should end.
    pub fn set_password_unattended(&self, password: &str) -> Result<(), AuthError> {
        if password.chars().count() < 12 {
            return Err(AuthError::TooShort);
        }
        self.write(hash_password(password)?)?;
        *Self::lock(&self.setup_claim) = None;
        Ok(())
    }

    /// Changes the password from a signed-in session. The current password is required; the
    /// caller ends every other session.
    pub fn change_password(
        &self,
        current: &str,
        password: &str,
        confirm: &str,
    ) -> Result<(), AuthError> {
        check_new_password(password, confirm)?;
        if self.current().is_none() {
            return Err(AuthError::NoPassword);
        }
        if !self.check_password(current) {
            return Err(AuthError::WrongPassword);
        }
        self.write(hash_password(password)?)
    }

    /// Checks a password without starting a session (for a sensitive confirmation).
    pub fn check_password(&self, password: &str) -> bool {
        let file = self.current();
        let Some(file) = file else {
            return false;
        };
        let Ok(parsed) = PasswordHash::new(&file.hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }

    /// Checks a password and starts a session, with the five-a-minute limit and a global
    /// ceiling. The check, the verification and the record happen under one lock, so concurrent
    /// attempts cannot slip past the limit together.
    pub fn login(
        &self,
        password: &str,
        client: &str,
        agent: &str,
    ) -> Result<(String, Session), AuthError> {
        let file = self.current();
        let Some(file) = file else {
            return Err(AuthError::NoPassword);
        };
        let parsed = PasswordHash::new(&file.hash).map_err(|_| AuthError::Damaged)?;
        let now = Instant::now();
        let mut failures = Self::lock(&self.failures);
        failures.prune(now);
        if failures.blocked(client) {
            return Err(AuthError::TooManyAttempts);
        }
        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_err()
        {
            failures.record(client, now);
            return Err(AuthError::WrongPassword);
        }
        failures.clear(client);
        drop(failures);
        Ok(self.create_session(client, agent))
    }

    /// The live session, if the id is valid and neither the idle nor the absolute lifetime passed.
    pub fn session(&self, id: &str) -> Option<Session> {
        let now = Instant::now();
        let mut sessions = Self::lock(&self.sessions);
        let session = sessions.get_mut(id)?;
        if now.duration_since(session.seen) > SESSION_IDLE
            || now.duration_since(session.created) > SESSION_MAX
        {
            sessions.remove(id);
            return None;
        }
        session.seen = now;
        session.last_used = SystemTime::now();
        Some(session.clone())
    }

    /// Whether the session is still live, without counting as a use (a media stream checks this
    /// once per chunk).
    pub fn is_live(&self, id: &str) -> bool {
        Self::lock(&self.sessions).contains_key(id)
    }

    pub fn logout(&self, id: &str) {
        Self::lock(&self.sessions).remove(id);
    }

    /// Ends every session except `keep`, and returns the ids that ended.
    pub fn logout_all_except(&self, keep: Option<&str>) -> Vec<String> {
        let mut sessions = Self::lock(&self.sessions);
        let ended: Vec<String> = sessions
            .keys()
            .filter(|id| Some(id.as_str()) != keep)
            .cloned()
            .collect();
        sessions.retain(|id, _| Some(id.as_str()) == keep);
        ended
    }

    /// Ends the session with this public handle, and returns its id.
    pub fn logout_handle(&self, handle: &str) -> Option<String> {
        let mut sessions = Self::lock(&self.sessions);
        let id = sessions
            .iter()
            .find(|(_, session)| session.handle == handle)
            .map(|(id, _)| id.clone())?;
        sessions.remove(&id);
        Some(id)
    }

    pub fn live_sessions(&self) -> usize {
        Self::lock(&self.sessions).len()
    }

    /// Every live session, the most recently used first.
    pub fn sessions(&self) -> Vec<SessionInfo> {
        let sessions = Self::lock(&self.sessions);
        let mut list: Vec<SessionInfo> = sessions
            .values()
            .map(|session| SessionInfo {
                handle: session.handle.clone(),
                address: session.address.clone(),
                agent: session.agent.clone(),
                signed_in_ms: millis(session.signed_in),
                last_used_ms: millis(session.last_used),
            })
            .collect();
        list.sort_by_key(|session| std::cmp::Reverse(session.last_used_ms));
        list
    }

    fn create_session(&self, address: &str, agent: &str) -> (String, Session) {
        let now = Instant::now();
        let mut sessions = Self::lock(&self.sessions);
        sessions.retain(|_, session| {
            now.duration_since(session.seen) <= SESSION_IDLE
                && now.duration_since(session.created) <= SESSION_MAX
        });
        if sessions.len() >= MAX_SESSIONS
            && let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, session)| session.seen)
                .map(|(id, _)| id.clone())
        {
            sessions.remove(&oldest);
        }
        let id = random_hex(32);
        let wall = SystemTime::now();
        let session = Session {
            csrf: random_hex(32),
            handle: random_hex(16),
            address: address.to_owned(),
            agent: agent.chars().take(AGENT_MAX).collect(),
            signed_in: wall,
            last_used: wall,
            created: now,
            seen: now,
        };
        sessions.insert(id.clone(), session.clone());
        (id, session)
    }

    /// The reset PIN file.
    pub fn pin_path(&self) -> PathBuf {
        self.data_dir.join(PIN_FILE)
    }

    /// "Forgot password?" on the headless server: makes a PIN and writes it to the PIN file, or
    /// points at the one that is still live. The caller has checked that the browser is on the
    /// home network.
    pub fn start_reset(&self) -> Result<ResetStarted, AuthError> {
        self.start_reset_at(Instant::now())
    }

    fn start_reset_at(&self, now: Instant) -> Result<ResetStarted, AuthError> {
        if let HostMode::Desktop { computer } = &self.host {
            return Err(AuthError::DesktopReset(computer.clone()));
        }
        let mut reset = Self::lock(&self.reset);
        self.expire(&mut reset, now);
        if reset.active.is_some() {
            return Ok(ResetStarted {
                path: self.pin_path(),
                created: false,
            });
        }
        if reset
            .ended
            .is_some_and(|ended| now.duration_since(ended) < PIN_COOLDOWN)
        {
            return Err(AuthError::ResetTooSoon);
        }
        let pin = random_pin();
        let expires_at = SystemTime::now() + PIN_LIFETIME;
        let text = format!(
            "CineVault password reset PIN: {pin}\n\nEnter it on the sign-in page with a new password.\nIt works once and expires at {} UTC (15 minutes after it was made).\n",
            clock_utc(expires_at)
        );
        cv_fs::write_atomic_with_mode(&self.pin_path(), text.as_bytes(), 0o600)?;
        reset.active = Some(ResetPin {
            pin,
            expires: now + PIN_LIFETIME,
            attempts: 0,
        });
        tracing::warn!(
            file = %self.pin_path().display(),
            expires_utc = %clock_utc(expires_at),
            "a password reset PIN was written to the data folder (docker: docker exec <container> cat /data/{PIN_FILE})"
        );
        Ok(ResetStarted {
            path: self.pin_path(),
            created: true,
        })
    }

    /// Ends an expired PIN and deletes its file.
    fn expire(&self, reset: &mut ResetState, now: Instant) {
        if reset.active.as_ref().is_some_and(|pin| now >= pin.expires) {
            reset.active = None;
            reset.ended = Some(now);
            let _ = std::fs::remove_file(self.pin_path());
        }
    }

    /// Sets a new password with the PIN, ends every session and signs this browser in. The PIN
    /// is compared in constant time; five wrong ones void it. A right PIN with an unusable new
    /// password is refused without spending the PIN.
    pub fn finish_reset(
        &self,
        pin: &str,
        password: &str,
        confirm: &str,
        address: &str,
        agent: &str,
    ) -> Result<(String, Session), AuthError> {
        self.finish_reset_at(pin, password, confirm, address, agent, Instant::now())
    }

    fn finish_reset_at(
        &self,
        pin: &str,
        password: &str,
        confirm: &str,
        address: &str,
        agent: &str,
        now: Instant,
    ) -> Result<(String, Session), AuthError> {
        if let HostMode::Desktop { computer } = &self.host {
            return Err(AuthError::DesktopReset(computer.clone()));
        }
        let mut reset = Self::lock(&self.reset);
        self.expire(&mut reset, now);
        let Some(active) = reset.active.as_mut() else {
            return Err(AuthError::NoPin);
        };
        let given = pin.trim().as_bytes();
        let right =
            given.len() == active.pin.len() && bool::from(given.ct_eq(active.pin.as_bytes()));
        if !right {
            active.attempts += 1;
            if active.attempts >= PIN_ATTEMPTS {
                reset.active = None;
                reset.ended = Some(now);
                let _ = std::fs::remove_file(self.pin_path());
                return Err(AuthError::PinVoid);
            }
            return Err(AuthError::WrongPin(PIN_ATTEMPTS - active.attempts));
        }
        check_new_password(password, confirm)?;
        self.write(hash_password(password)?)?;
        reset.active = None;
        reset.ended = Some(now);
        let _ = std::fs::remove_file(self.pin_path());
        drop(reset);
        *Self::lock(&self.setup_claim) = None;
        self.logout_all_except(None);
        Ok(self.create_session(address, agent))
    }
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buffer);
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// An 8-digit PIN, uniform over every value (rejection sampling, no modulo bias).
fn random_pin() -> String {
    let limit = 10_u32.pow(PIN_DIGITS);
    // The largest multiple of `limit` that fits in a u32.
    let zone = u32::MAX - (u32::MAX % limit);
    loop {
        let value = OsRng.next_u32();
        if value < zone {
            return format!("{:0width$}", value % limit, width = PIN_DIGITS as usize);
        }
    }
}

/// `HH:MM` in UTC, for the PIN file and the log.
fn clock_utc(time: SystemTime) -> String {
    let seconds = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let day = seconds % 86_400;
    format!("{:02}:{:02}", day / 3600, (day % 3600) / 60)
}

/// One cookie's value from a request.
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_owned())
}

/// The sign-in cookie. `Secure` is added when the request arrived over HTTPS.
pub fn session_cookie(id: &str, secure: bool) -> String {
    format!(
        "cv_session={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        30 * 24 * 60 * 60
    ) + if secure { "; Secure" } else { "" }
}

/// The cookie that ties the welcome page to one browser.
pub fn setup_cookie(id: &str, secure: bool) -> String {
    format!("cv_setup={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age=1800")
        + if secure { "; Secure" } else { "" }
}

pub fn clear_cookie(name: &str, secure: bool) -> String {
    format!("{name}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
        + if secure { "; Secure" } else { "" }
}

/// Text for an HTML page: the five characters that matter escaped.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// The "Forgot password?" part of the sign-in page.
fn recovery(host: &HostMode) -> String {
    match host {
        HostMode::Desktop { computer } => format!(
            r#"<details><summary>Forgot the password?</summary><p>Change it in CineVault's Settings on {}.</p></details>"#,
            escape(computer)
        ),
        HostMode::Server => r#"<details id="forgot"><summary>Forgot the password?</summary>
<p>CineVault can write a one-time PIN to a file on the server. This works from your home network.</p>
<button type="button" id="pin-start">Make a reset PIN</button>
<p id="pin-where" role="status"></p>
<form id="pin-form" hidden>
<label for="pin">PIN</label>
<input id="pin" name="pin" inputmode="numeric" autocomplete="one-time-code" required>
<label for="pin-password">New password</label>
<input id="pin-password" type="password" autocomplete="new-password" minlength="12" required>
<label for="pin-confirm">Confirm new password</label>
<input id="pin-confirm" type="password" autocomplete="new-password" minlength="12" required>
<p id="pin-error" role="alert"></p>
<button class="primary" type="submit">Set new password</button>
</form>
<p>Or run <code>cinevault-server passwd</code> on the server. Your library, settings and provider keys are kept.</p>
</details>"#
            .to_owned(),
    }
}

/// The script behind the PIN reset on the headless server's sign-in page.
const RESET_SCRIPT: &str = r#"
const start = document.getElementById('pin-start');
if (start) {
  const where = document.getElementById('pin-where');
  const pinForm = document.getElementById('pin-form');
  start.onclick = async () => {
    where.textContent = '';
    const answer = await fetch('/api/reset/start', { method: 'POST' });
    const body = await answer.json().catch(() => ({ message: 'Could not make a PIN.' }));
    if (!answer.ok) { where.textContent = body.message; return; }
    where.textContent = 'Open ' + body.path + ' on the server and enter the PIN below. It works once and expires in 15 minutes. In Docker: docker exec <container> cat /data/password-reset-pin.txt';
    pinForm.hidden = false;
    document.getElementById('pin').focus();
  };
  pinForm.onsubmit = async (event) => {
    event.preventDefault();
    const pinError = document.getElementById('pin-error');
    pinError.textContent = '';
    const password = document.getElementById('pin-password').value;
    const confirm = document.getElementById('pin-confirm').value;
    if (password !== confirm) { pinError.textContent = 'The passwords do not match.'; return; }
    const answer = await fetch('/api/reset/finish', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ pin: document.getElementById('pin').value, password, confirm }) });
    if (answer.ok) { location.href = '/'; return; }
    const body = await answer.json().catch(() => ({ message: 'Could not set the password.' }));
    pinError.textContent = body.message;
  };
}
"#;

/// The sign-in page. One password field, a show/hide control, Enter submits, and the way back
/// in when the password is forgotten.
#[must_use]
pub fn login_page(plain_http: bool, host: &HostMode) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Sign in to CineVault</title>
<style>{style}</style></head>
<body><main>
<div class="mark">CineVault</div>
<h1>Sign in</h1>
{warning}
<form id="form">
<label for="password">Password</label>
<div class="field"><input id="password" name="password" type="password" autocomplete="current-password" autofocus required>
<button type="button" id="show" aria-label="Show password">Show</button></div>
<p id="error" role="alert"></p>
<button class="primary" type="submit">Sign in</button>
</form>
{recovery}
</main>
<script>
const form = document.getElementById('form');
const password = document.getElementById('password');
const error = document.getElementById('error');
const show = document.getElementById('show');
show.onclick = () => {{ const shown = password.type === 'text'; password.type = shown ? 'password' : 'text'; show.textContent = shown ? 'Show' : 'Hide'; }};
form.onsubmit = async (event) => {{
  event.preventDefault();
  error.textContent = '';
  const answer = await fetch('/api/login', {{ method: 'POST', headers: {{ 'content-type': 'application/json' }}, body: JSON.stringify({{ password: password.value }}) }});
  if (answer.ok) {{ location.href = {safe_next}; return; }}
  const body = await answer.json().catch(() => ({{ message: 'Could not sign in.' }}));
  error.textContent = body.message;
  password.select();
}};
{reset_script}
</script>
</body></html>"#,
        style = PAGE_STYLE,
        warning = match host {
            HostMode::Desktop { .. } if plain_http => DESKTOP_PLAIN_HTTP,
            _ => plain_http_warning(plain_http),
        },
        safe_next = SAFE_NEXT,
        recovery = recovery(host),
        reset_script = if *host == HostMode::Server {
            RESET_SCRIPT
        } else {
            ""
        },
    )
}

/// The desktop app has no reverse proxy in front of it; the advice is where to use it.
const DESKTOP_PLAIN_HTTP: &str = r#"<p class="warn">This connection is not encrypted. Use Browser access on your home network, not on public Wi-Fi.</p>"#;

/// The page a browser from outside the home network gets while no password exists.
#[must_use]
pub fn away_page() -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Finish setup from your home network</title>
<style>{style}</style></head>
<body><main>
<div class="mark">CineVault</div>
<h1>Finish setup from your home network</h1>
<p class="lead">This server has no password yet. For safety, the password can only be created from a browser on the same computer or the same home network as the server.</p>
<p class="hint">Or run <code>cinevault-server passwd</code> on the server.</p>
</main></body></html>"#,
        style = PAGE_STYLE
    )
}

pub fn setup_page(plain_http: bool) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Welcome to CineVault</title>
<style>{style}</style></head>
<body><main>
<div class="mark">CineVault</div>
<h1>Welcome</h1>
<p class="lead">Create the password you will use to open this server from a browser.</p>
<form id="form">
<label for="password">Password</label>
<div class="field"><input id="password" name="password" type="password" autocomplete="new-password" minlength="12" autofocus required>
<button type="button" id="show" aria-label="Show password">Show</button></div>
<label for="confirm">Confirm password</label>
<input id="confirm" name="confirm" type="password" autocomplete="new-password" minlength="12" required>
<p class="hint">At least 12 characters. A password manager is a good idea.</p>
<p id="error" role="alert"></p>
{warning}
<button class="primary" type="submit">Create password</button>
</form>
<p class="warn">Finish setup before making this server reachable from the internet.</p>
</main>
<script>
const form = document.getElementById('form');
const password = document.getElementById('password');
const confirm = document.getElementById('confirm');
const error = document.getElementById('error');
const show = document.getElementById('show');
show.onclick = () => {{ const shown = password.type === 'text'; password.type = shown ? 'password' : 'text'; confirm.type = password.type; show.textContent = shown ? 'Show' : 'Hide'; }};
form.onsubmit = async (event) => {{
  event.preventDefault();
  error.textContent = '';
  if (password.value !== confirm.value) {{ error.textContent = 'The passwords do not match.'; confirm.select(); return; }}
  const answer = await fetch('/api/setup', {{ method: 'POST', headers: {{ 'content-type': 'application/json' }}, body: JSON.stringify({{ password: password.value, confirm: confirm.value }}) }});
  if (answer.ok) {{ location.href = {safe_next}; return; }}
  const body = await answer.json().catch(() => ({{ message: 'Could not set the password.' }}));
  error.textContent = body.message;
}};
</script>
</body></html>"#,
        style = PAGE_STYLE,
        warning = plain_http_warning(plain_http),
        safe_next = SAFE_NEXT
    )
}

/// Where the page goes after signing in: the `next` address only when it resolves to this
/// server. A plain prefix check is not enough, because a browser reads `/\host` as `//host`.
const SAFE_NEXT: &str = "(() => { try { const next = new URLSearchParams(location.search).get('next'); if (!next) return '/'; const target = new URL(next, location.origin); return target.origin === location.origin ? target.pathname + target.search + target.hash : '/'; } catch { return '/'; } })()";

fn plain_http_warning(plain_http: bool) -> &'static str {
    if plain_http {
        r#"<p class="warn">This connection is not encrypted. On a network you do not fully trust, put CineVault behind a reverse proxy with HTTPS before signing in.</p>"#
    } else {
        ""
    }
}

const PAGE_STYLE: &str = r#"
:root { color-scheme: dark; --bg: #0c0c0e; --surface: #16161a; --line: #2a2a30; --text: #f2f0ea; --muted: #a8a49b; --gold: #d8b45a; --on-gold: #1a1405; --error: #ff8a7a; }
* { box-sizing: border-box; }
body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: var(--bg); color: var(--text); font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; padding: 24px; }
main { width: min(400px, 100%); background: var(--surface); border: 1px solid var(--line); border-radius: 16px; padding: 32px; }
.mark { color: var(--gold); font-weight: 600; letter-spacing: .08em; text-transform: uppercase; font-size: 12px; }
h1 { font-size: 28px; margin: 12px 0 4px; }
p.lead { color: var(--muted); margin: 0 0 20px; }
label { display: block; font-size: 13px; color: var(--muted); margin: 16px 0 6px; }
input { width: 100%; padding: 11px 12px; border-radius: 10px; border: 1px solid var(--line); background: var(--bg); color: var(--text); font: inherit; }
input:focus { outline: 2px solid var(--gold); outline-offset: 1px; }
.field { position: relative; }
.field input { padding-right: 64px; }
.field button { position: absolute; right: 6px; top: 6px; height: 32px; border: 0; background: transparent; color: var(--muted); cursor: pointer; font: inherit; }
button.primary { width: 100%; margin-top: 20px; padding: 12px; border: 0; border-radius: 10px; background: var(--gold); color: var(--on-gold); font: inherit; font-weight: 600; cursor: pointer; }
button.primary:hover { filter: brightness(1.08); }
p.hint, details, p.warn { color: var(--muted); font-size: 13px; }
p.warn { border-top: 1px solid var(--line); margin-top: 24px; padding-top: 16px; }
#error { color: var(--error); min-height: 1.5em; margin: 10px 0 0; }
details { margin-top: 20px; }
code { background: var(--bg); padding: 1px 5px; border-radius: 4px; }
@media (max-width: 420px) { main { padding: 22px; border: 0; background: transparent; } }
@media (prefers-color-scheme: light) {
  :root { color-scheme: light; --bg: #f2f1ee; --surface: #fbfaf8; --line: rgba(0, 0, 0, 0.08); --text: #1a1a19; --muted: #666561; --gold: #7a6438; --on-gold: #f2f1ee; --error: #a82533; }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "a long enough password";

    fn auth() -> (tempfile::TempDir, Auth) {
        let dir = tempfile::tempdir().unwrap();
        let auth = Auth::open(dir.path()).unwrap();
        (dir, auth)
    }

    fn with_password() -> (tempfile::TempDir, Auth) {
        let (dir, auth) = auth();
        let claim = auth.claim_setup(None).unwrap();
        auth.set_password(GOOD, GOOD, &claim, "192.168.1.30", "Test")
            .unwrap();
        (dir, auth)
    }

    fn read_pin(dir: &tempfile::TempDir) -> String {
        let text = std::fs::read_to_string(dir.path().join(PIN_FILE)).unwrap();
        text.split("PIN: ")
            .nth(1)
            .unwrap()
            .chars()
            .take(8)
            .collect()
    }

    #[test]
    fn a_new_folder_opens_the_welcome_page() {
        let (_dir, auth) = auth();
        assert!(auth.needs_setup());
        assert!(!auth.has_password());
    }

    #[test]
    fn the_welcome_page_has_no_time_limit() {
        let (_dir, auth) = auth();
        let later = Instant::now() + Duration::from_secs(2 * 60 * 60);
        assert!(auth.needs_setup());
        assert!(auth.claim_setup_at(None, later).is_ok());
    }

    #[test]
    fn a_second_browser_is_refused_until_the_claim_lapses() {
        let (_dir, auth) = auth();
        let now = Instant::now();
        let first = auth.claim_setup_at(None, now).unwrap();
        assert!(matches!(
            auth.claim_setup_at(None, now + Duration::from_secs(60)),
            Err(AuthError::SetupClaimed)
        ));
        // The first browser keeps its claim alive by using it.
        assert_eq!(
            auth.claim_setup_at(Some(&first), now + Duration::from_secs(20 * 60))
                .unwrap(),
            first
        );
        assert!(matches!(
            auth.claim_setup_at(None, now + Duration::from_secs(40 * 60)),
            Err(AuthError::SetupClaimed)
        ));
        let second = auth
            .claim_setup_at(None, now + Duration::from_secs(51 * 60))
            .unwrap();
        assert_ne!(first, second);
        assert!(matches!(
            auth.set_password(GOOD, GOOD, &first, "a", "b"),
            Err(AuthError::SetupClaimed)
        ));
        assert!(auth.set_password(GOOD, GOOD, &second, "a", "b").is_ok());
    }

    #[test]
    fn the_desktop_never_shows_the_welcome_page() {
        let dir = tempfile::tempdir().unwrap();
        let auth = Auth::open_as(
            dir.path(),
            HostMode::Desktop {
                computer: "STUDY-PC".into(),
            },
        )
        .unwrap();
        assert!(!auth.needs_setup());
        assert!(matches!(auth.claim_setup(None), Err(AuthError::AlreadySet)));
        assert!(matches!(
            auth.start_reset(),
            Err(AuthError::DesktopReset(name)) if name == "STUDY-PC"
        ));
        assert!(!dir.path().join(PIN_FILE).exists());
        let page = login_page(false, auth.host());
        assert!(page.contains("Change it in CineVault's Settings on STUDY-PC."));
        assert!(!page.contains("/api/reset/start"));
    }

    #[test]
    fn the_computer_name_is_escaped_on_the_sign_in_page() {
        let page = login_page(
            false,
            &HostMode::Desktop {
                computer: "<script>x</script>".into(),
            },
        );
        assert!(!page.contains("<script>x</script>"));
        assert!(page.contains("&lt;script&gt;"));
    }

    #[test]
    fn a_short_password_is_refused() {
        let (_dir, auth) = auth();
        let claim = auth.claim_setup(None).unwrap();
        assert!(matches!(
            auth.set_password("short", "short", &claim, "a", "b"),
            Err(AuthError::TooShort)
        ));
    }

    #[test]
    fn a_mismatched_confirmation_is_refused() {
        let (_dir, auth) = auth();
        let claim = auth.claim_setup(None).unwrap();
        assert!(matches!(
            auth.set_password(GOOD, "another long password", &claim, "a", "b"),
            Err(AuthError::Mismatch)
        ));
    }

    #[test]
    fn the_password_is_hashed_and_signs_in() {
        let (dir, auth) = with_password();
        assert!(!auth.needs_setup());
        let saved = std::fs::read_to_string(dir.path().join(AUTH_FILE)).unwrap();
        assert!(saved.contains("$argon2"));
        assert!(!saved.contains(GOOD));
        let (again, session) = auth.login(GOOD, "127.0.0.1", "Edge").unwrap();
        assert!(auth.session(&again).is_some());
        assert!(!session.csrf.is_empty());
        assert_ne!(session.handle, again);
    }

    #[test]
    fn a_wrong_password_is_refused_and_rate_limited() {
        let (_dir, auth) = with_password();
        for _ in 0..5 {
            assert!(matches!(
                auth.login("wrong password", "127.0.0.1", ""),
                Err(AuthError::WrongPassword)
            ));
        }
        assert!(matches!(
            auth.login(GOOD, "127.0.0.1", ""),
            Err(AuthError::TooManyAttempts)
        ));
    }

    /// `cinevault-server passwd` writes the file while the server runs; the running server
    /// accepts the new password at once and refuses the old one.
    #[test]
    fn a_password_set_on_the_command_line_works_without_a_restart() {
        let (dir, auth) = with_password();
        let command_line = Auth::open(dir.path()).unwrap();
        command_line
            .set_password_unattended("the second long password")
            .unwrap();
        assert!(auth.login("the second long password", "a", "").is_ok());
        assert!(auth.login(GOOD, "b", "").is_err());
    }

    #[test]
    fn the_sign_in_pages_only_return_to_this_server() {
        for page in [login_page(false, &HostMode::Server), setup_page(false)] {
            assert!(page.contains("target.origin === location.origin"));
            assert!(!page.contains("startsWith('//')"));
        }
    }

    #[test]
    fn sign_out_ends_the_session() {
        let (_dir, auth) = with_password();
        let (id, _) = auth.login(GOOD, "a", "").unwrap();
        auth.logout(&id);
        assert!(auth.session(&id).is_none());
    }

    #[test]
    fn sessions_are_listed_and_signed_out_by_handle() {
        let (_dir, auth) = with_password();
        auth.logout_all_except(None);
        let (first, one) = auth.login(GOOD, "192.168.1.30", "Phone browser").unwrap();
        let (second, _) = auth.login(GOOD, "192.168.1.31", "TV browser").unwrap();
        let list = auth.sessions();
        assert_eq!(list.len(), 2);
        assert!(
            list.iter()
                .any(|s| s.address == "192.168.1.30" && s.agent == "Phone browser")
        );
        assert!(list.iter().all(|s| s.handle != first && s.handle != second));
        assert_eq!(auth.logout_handle(&one.handle), Some(first.clone()));
        assert!(auth.session(&first).is_none());
        assert!(auth.session(&second).is_some());
        assert_eq!(auth.logout_handle("unknown"), None);
    }

    #[test]
    fn the_session_cap_evicts_the_oldest() {
        let (_dir, auth) = with_password();
        auth.logout_all_except(None);
        let ids: Vec<String> = (0..33)
            .map(|n| auth.create_session(&format!("10.0.0.{n}"), "").0)
            .collect();
        assert_eq!(auth.live_sessions(), 32);
        assert!(!auth.is_live(&ids[0]));
        assert!(auth.is_live(&ids[32]));
    }

    #[test]
    fn a_damaged_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(AUTH_FILE), "not json").unwrap();
        assert!(matches!(Auth::open(dir.path()), Err(AuthError::Damaged)));
    }

    #[test]
    fn a_reset_pin_is_written_to_a_file_and_signs_everyone_else_out() {
        let (dir, auth) = with_password();
        let (old, _) = auth.login(GOOD, "a", "").unwrap();
        let started = auth.start_reset().unwrap();
        assert!(started.created);
        assert_eq!(started.path, dir.path().join(PIN_FILE));
        let pin = read_pin(&dir);
        assert_eq!(pin.len(), 8);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
        let (id, _) = auth
            .finish_reset(
                &pin,
                "a brand new password",
                "a brand new password",
                "b",
                "",
            )
            .unwrap();
        assert!(auth.session(&id).is_some());
        assert!(auth.session(&old).is_none());
        assert!(!dir.path().join(PIN_FILE).exists());
        assert!(auth.login("a brand new password", "c", "").is_ok());
        assert!(auth.login(GOOD, "d", "").is_err());
        // The PIN works once.
        assert!(matches!(
            auth.finish_reset(
                &pin,
                "a third long password",
                "a third long password",
                "e",
                ""
            ),
            Err(AuthError::NoPin)
        ));
    }

    #[test]
    fn asking_again_keeps_the_same_pin() {
        let (dir, auth) = with_password();
        assert!(auth.start_reset().unwrap().created);
        let pin = read_pin(&dir);
        let again = auth.start_reset().unwrap();
        assert!(!again.created);
        assert_eq!(read_pin(&dir), pin);
    }

    #[test]
    fn five_wrong_pins_void_it() {
        let (dir, auth) = with_password();
        auth.start_reset().unwrap();
        let pin = read_pin(&dir);
        let wrong = if pin == "00000000" {
            "00000001"
        } else {
            "00000000"
        };
        for left in (1..5).rev() {
            assert!(matches!(
                auth.finish_reset(wrong, GOOD, GOOD, "a", ""),
                Err(AuthError::WrongPin(n)) if n == left
            ));
        }
        assert!(matches!(
            auth.finish_reset(wrong, GOOD, GOOD, "a", ""),
            Err(AuthError::PinVoid)
        ));
        assert!(!dir.path().join(PIN_FILE).exists());
        assert!(matches!(
            auth.finish_reset(
                &pin,
                "a brand new password",
                "a brand new password",
                "a",
                ""
            ),
            Err(AuthError::NoPin)
        ));
        // A new PIN only after a minute.
        assert!(matches!(auth.start_reset(), Err(AuthError::ResetTooSoon)));
        let later = Instant::now() + Duration::from_secs(61);
        assert!(auth.start_reset_at(later).unwrap().created);
    }

    #[test]
    fn an_expired_pin_is_refused_and_its_file_deleted() {
        let (dir, auth) = with_password();
        let now = Instant::now();
        auth.start_reset_at(now).unwrap();
        let pin = read_pin(&dir);
        let later = now + PIN_LIFETIME + Duration::from_secs(60);
        assert!(matches!(
            auth.finish_reset_at(
                &pin,
                "a brand new password",
                "a brand new password",
                "a",
                "",
                later
            ),
            Err(AuthError::NoPin)
        ));
        assert!(!dir.path().join(PIN_FILE).exists());
        assert!(auth.login(GOOD, "a", "").is_ok());
    }

    #[test]
    fn a_short_new_password_does_not_spend_the_pin() {
        let (dir, auth) = with_password();
        auth.start_reset().unwrap();
        let pin = read_pin(&dir);
        assert!(matches!(
            auth.finish_reset(&pin, "short", "short", "a", ""),
            Err(AuthError::TooShort)
        ));
        assert!(matches!(
            auth.finish_reset(
                &pin,
                "a brand new password",
                "another new password",
                "a",
                ""
            ),
            Err(AuthError::Mismatch)
        ));
        assert!(
            auth.finish_reset(
                &pin,
                "a brand new password",
                "a brand new password",
                "a",
                ""
            )
            .is_ok()
        );
    }

    #[test]
    fn a_pin_left_by_an_earlier_run_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(PIN_FILE),
            "CineVault password reset PIN: 12345678",
        )
        .unwrap();
        let _auth = Auth::open(dir.path()).unwrap();
        assert!(!dir.path().join(PIN_FILE).exists());
    }

    #[test]
    fn pins_are_eight_digits() {
        for _ in 0..200 {
            let pin = random_pin();
            assert_eq!(pin.len(), 8);
            assert!(pin.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_password_and_pin_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, auth) = with_password();
        auth.start_reset().unwrap();
        for name in [AUTH_FILE, PIN_FILE] {
            let mode = std::fs::metadata(dir.path().join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{name}");
        }
    }
}

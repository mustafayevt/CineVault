//! Sign-in for the browser server: one user, one password, server-side sessions.
//!
//! The first start has no password. A welcome window of 30 minutes lets the first browser set one;
//! after that, only `cinevault-server passwd` on the server can. The password is argon2id with a
//! random salt, stored in `server-auth.json` written atomically with mode `0600`. Sessions are
//! random ids in memory, never in a file, so a restart signs everyone out.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use argon2::Argon2;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

/// The file the password hash lives in, inside the data folder.
pub const AUTH_FILE: &str = "server-auth.json";
const VERSION: u32 = 1;
const SETUP_WINDOW: Duration = Duration::from_secs(30 * 60);
const SESSION_IDLE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const SESSION_MAX: Duration = Duration::from_secs(90 * 24 * 60 * 60);
const MAX_SESSIONS: usize = 32;
const MAX_FAILURES: usize = 5;
const MAX_GLOBAL_FAILURES: usize = 100;
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AuthFile {
    version: u32,
    /// The PHC string, which already carries the salt and the parameters.
    hash: String,
}

#[derive(Clone)]
pub struct Session {
    pub csrf: String,
    created: Instant,
    seen: Instant,
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
    #[error("The setup window has closed. Run `cinevault-server passwd` on the server.")]
    SetupClosed,
    #[error("Setup is in progress in another browser.")]
    SetupClaimed,
    #[error("The server's sign-in file could not be read: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "The server's sign-in file is damaged. Move it aside and restart to set a new password."
    )]
    Damaged,
    #[error("The password could not be hashed: {0}")]
    Hash(String),
}

/// The password, the live sessions and the first-run window.
pub struct Auth {
    path: PathBuf,
    password: Mutex<Option<AuthFile>>,
    sessions: Mutex<HashMap<String, Session>>,
    setup_deadline: Mutex<Option<Instant>>,
    setup_claim: Mutex<Option<String>>,
    failures: Mutex<Failures>,
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

impl Auth {
    /// Opens the sign-in state of a data folder. A missing file starts the welcome window; a
    /// damaged file is refused rather than silently treated as "no password".
    pub fn open(data_dir: &Path) -> Result<Self, AuthError> {
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
        let setup_deadline = password.is_none().then(|| Instant::now() + SETUP_WINDOW);
        Ok(Self {
            path,
            password: Mutex::new(password),
            sessions: Mutex::new(HashMap::new()),
            setup_deadline: Mutex::new(setup_deadline),
            setup_claim: Mutex::new(None),
            failures: Mutex::new(Failures::default()),
        })
    }

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn has_password(&self) -> bool {
        Self::lock(&self.password).is_some()
    }

    /// True while the welcome window is open and no password exists.
    pub fn needs_setup(&self) -> bool {
        if self.has_password() {
            return false;
        }
        Self::lock(&self.setup_deadline).is_some_and(|deadline| Instant::now() < deadline)
    }

    /// The welcome window has closed without a password.
    pub fn setup_closed(&self) -> bool {
        !self.has_password() && !self.needs_setup()
    }

    /// Claims the setup for this browser, or checks the claim it already holds.
    pub fn claim_setup(&self, existing: Option<&str>) -> Result<String, AuthError> {
        if !self.needs_setup() {
            return Err(AuthError::SetupClosed);
        }
        let mut claim = Self::lock(&self.setup_claim);
        match claim.as_deref() {
            Some(current) if existing == Some(current) => Ok(current.to_owned()),
            Some(_) => Err(AuthError::SetupClaimed),
            None => {
                let id = random_hex(16);
                *claim = Some(id.clone());
                Ok(id)
            }
        }
    }

    /// Sets the first password and signs the browser in.
    pub fn set_password(
        &self,
        password: &str,
        confirm: &str,
        claim: &str,
    ) -> Result<(String, Session), AuthError> {
        if !self.needs_setup() {
            return Err(AuthError::SetupClosed);
        }
        if Self::lock(&self.setup_claim).as_deref() != Some(claim) {
            return Err(AuthError::SetupClaimed);
        }
        if password.chars().count() < 12 {
            return Err(AuthError::TooShort);
        }
        if password != confirm {
            return Err(AuthError::Mismatch);
        }
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|error| AuthError::Hash(error.to_string()))?
            .to_string();
        let file = AuthFile {
            version: VERSION,
            hash,
        };
        let bytes =
            serde_json::to_vec(&file).map_err(|error| AuthError::Hash(error.to_string()))?;
        cv_fs::write_atomic_with_mode(&self.path, &bytes, 0o600)?;
        *Self::lock(&self.password) = Some(file);
        *Self::lock(&self.setup_deadline) = None;
        *Self::lock(&self.setup_claim) = None;
        Ok(self.create_session())
    }

    /// Sets the password without the setup claim or a session, for `cinevault-server passwd` on
    /// the machine that holds the data folder. Shell access is the trust boundary.
    pub fn set_password_unattended(&self, password: &str) -> Result<(), AuthError> {
        if password.chars().count() < 12 {
            return Err(AuthError::TooShort);
        }
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|error| AuthError::Hash(error.to_string()))?
            .to_string();
        let next = AuthFile {
            version: VERSION,
            hash,
        };
        let bytes =
            serde_json::to_vec(&next).map_err(|error| AuthError::Hash(error.to_string()))?;
        cv_fs::write_atomic_with_mode(&self.path, &bytes, 0o600)?;
        *Self::lock(&self.password) = Some(next);
        *Self::lock(&self.setup_deadline) = None;
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
        if password.chars().count() < 12 {
            return Err(AuthError::TooShort);
        }
        if password != confirm {
            return Err(AuthError::Mismatch);
        }
        let file = {
            let guard = Self::lock(&self.password);
            (*guard).clone()
        };
        let Some(file) = file else {
            return Err(AuthError::SetupClosed);
        };
        let parsed = PasswordHash::new(&file.hash).map_err(|_| AuthError::Damaged)?;
        if Argon2::default()
            .verify_password(current.as_bytes(), &parsed)
            .is_err()
        {
            return Err(AuthError::WrongPassword);
        }
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|error| AuthError::Hash(error.to_string()))?
            .to_string();
        let next = AuthFile {
            version: VERSION,
            hash,
        };
        let bytes =
            serde_json::to_vec(&next).map_err(|error| AuthError::Hash(error.to_string()))?;
        cv_fs::write_atomic_with_mode(&self.path, &bytes, 0o600)?;
        *Self::lock(&self.password) = Some(next);
        Ok(())
    }

    /// Checks a password without starting a session (for a sensitive confirmation).
    pub fn check_password(&self, password: &str) -> bool {
        let file = {
            let guard = Self::lock(&self.password);
            (*guard).clone()
        };
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
    pub fn login(&self, password: &str, client: &str) -> Result<(String, Session), AuthError> {
        let file = {
            let guard = Self::lock(&self.password);
            (*guard).clone()
        };
        let Some(file) = file else {
            return Err(AuthError::SetupClosed);
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
        Ok(self.create_session())
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
        Some(session.clone())
    }

    pub fn logout(&self, id: &str) {
        Self::lock(&self.sessions).remove(id);
    }

    #[allow(dead_code)] // Used by the Server settings section (change password).
    pub fn logout_all_except(&self, keep: Option<&str>) {
        let mut sessions = Self::lock(&self.sessions);
        sessions.retain(|id, _| Some(id.as_str()) == keep);
    }

    #[allow(dead_code)] // Shown by the Server settings section.
    pub fn live_sessions(&self) -> usize {
        Self::lock(&self.sessions).len()
    }

    fn create_session(&self) -> (String, Session) {
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
        let session = Session {
            csrf: random_hex(32),
            created: now,
            seen: now,
        };
        sessions.insert(id.clone(), session.clone());
        (id, session)
    }
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buffer);
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
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

/// The cookie that ties the welcome window to one browser.
pub fn setup_cookie(id: &str, secure: bool) -> String {
    format!("cv_setup={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age=1800")
        + if secure { "; Secure" } else { "" }
}

pub fn clear_cookie(name: &str, secure: bool) -> String {
    format!("{name}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
        + if secure { "; Secure" } else { "" }
}

/// The sign-in page. One password field, a show/hide control, Enter submits, and a link that
/// explains the recovery command.
#[must_use]
pub fn login_page() -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Sign in to CineVault</title>
<style>{style}</style></head>
<body><main>
<div class="mark">CineVault</div>
<h1>Sign in</h1>
<form id="form">
<label for="password">Password</label>
<div class="field"><input id="password" name="password" type="password" autocomplete="current-password" autofocus required>
<button type="button" id="show" aria-label="Show password">Show</button></div>
<p id="error" role="alert"></p>
<button class="primary" type="submit">Sign in</button>
</form>
<details><summary>Forgot the password?</summary><p>On the server, run <code>cinevault-server passwd</code> and enter a new one. Your library, settings and provider keys are kept.</p></details>
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
  if (answer.ok) {{ location.href = '/'; return; }}
  const body = await answer.json().catch(() => ({{ message: 'Could not sign in.' }}));
  error.textContent = body.message;
  password.select();
}};
</script>
</body></html>"#,
        style = PAGE_STYLE
    )
}

/// The welcome page shown once, before a password exists.
#[must_use]
pub fn setup_page() -> String {
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
  if (answer.ok) {{ location.href = '/'; return; }}
  const body = await answer.json().catch(() => ({{ message: 'Could not set the password.' }}));
  error.textContent = body.message;
}};
</script>
</body></html>"#,
        style = PAGE_STYLE
    )
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

    fn auth() -> (tempfile::TempDir, Auth) {
        let dir = tempfile::tempdir().unwrap();
        let auth = Auth::open(dir.path()).unwrap();
        (dir, auth)
    }

    #[test]
    fn a_new_folder_opens_the_welcome_window() {
        let (_dir, auth) = auth();
        assert!(auth.needs_setup());
        assert!(!auth.setup_closed());
        assert!(!auth.has_password());
    }

    #[test]
    fn a_short_password_is_refused() {
        let (_dir, auth) = auth();
        let claim = auth.claim_setup(None).unwrap();
        assert!(matches!(
            auth.set_password("short", "short", &claim),
            Err(AuthError::TooShort)
        ));
    }

    #[test]
    fn a_mismatched_confirmation_is_refused() {
        let (_dir, auth) = auth();
        let claim = auth.claim_setup(None).unwrap();
        assert!(matches!(
            auth.set_password("a long enough password", "another long password", &claim),
            Err(AuthError::Mismatch)
        ));
    }

    #[test]
    fn the_password_is_hashed_and_signs_in() {
        let (dir, auth) = auth();
        let claim = auth.claim_setup(None).unwrap();
        let (id, session) = auth
            .set_password("a long enough password", "a long enough password", &claim)
            .unwrap();
        assert!(!auth.needs_setup());
        assert!(auth.session(&id).is_some());
        assert!(!session.csrf.is_empty());
        let saved = std::fs::read_to_string(dir.path().join(AUTH_FILE)).unwrap();
        assert!(saved.contains("$argon2"));
        assert!(!saved.contains("a long enough password"));

        let (again, _) = auth.login("a long enough password", "127.0.0.1").unwrap();
        assert!(auth.session(&again).is_some());
    }

    #[test]
    fn a_wrong_password_is_refused_and_rate_limited() {
        let (_dir, auth) = auth();
        let claim = auth.claim_setup(None).unwrap();
        auth.set_password("a long enough password", "a long enough password", &claim)
            .unwrap();
        for _ in 0..5 {
            assert!(matches!(
                auth.login("wrong password", "127.0.0.1"),
                Err(AuthError::WrongPassword)
            ));
        }
        assert!(matches!(
            auth.login("a long enough password", "127.0.0.1"),
            Err(AuthError::TooManyAttempts)
        ));
    }

    #[test]
    fn sign_out_ends_the_session() {
        let (_dir, auth) = auth();
        let claim = auth.claim_setup(None).unwrap();
        let (id, _) = auth
            .set_password("a long enough password", "a long enough password", &claim)
            .unwrap();
        auth.logout(&id);
        assert!(auth.session(&id).is_none());
    }

    #[test]
    fn a_damaged_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(AUTH_FILE), "not json").unwrap();
        assert!(matches!(Auth::open(dir.path()), Err(AuthError::Damaged)));
    }

    #[cfg(unix)]
    #[test]
    fn the_password_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, auth) = auth();
        let claim = auth.claim_setup(None).unwrap();
        auth.set_password("a long enough password", "a long enough password", &claim)
            .unwrap();
        let mode = std::fs::metadata(dir.path().join(AUTH_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

//! The update service: settings, channel requests and the installer download.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use semver::Version;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::install::VerifiedInstaller;
use super::manifest::{
    BUILT_IN_PUBLIC_KEY, MAX_MANIFEST_BYTES, ManifestArtifact, parse_public_key, platform_id,
    verify_manifest,
};
use crate::app_settings::{AppSettingsError, SettingsService};

/// Setting: whether the app looks for updates by itself (default on).
pub const CHECK_KEY: &str = "general.check-updates";
/// Setting: the address of the signed manifest. Empty (the default) means the built-in channel,
/// [`BUILT_IN_CHANNEL`]; an address here overrides it.
pub const CHANNEL_KEY: &str = "general.update-channel";
/// The channel this build reads when `general.update-channel` is empty: the signed manifest of
/// the newest release in the public releases repository. The address redirects to the file of
/// the latest release, so it never changes.
pub const BUILT_IN_CHANNEL: &str =
    "https://github.com/mustafayevt/CineVault-releases/releases/latest/download/manifest.json";
/// Setting: how often to look, `daily` (default), `weekly` or `monthly`.
pub const INTERVAL_KEY: &str = "general.update-interval";
/// Setting: the public key the manifest must be signed with, when it is not the built-in one.
pub const PUBLIC_KEY_KEY: &str = "general.update-public-key";
/// State: when the channel was last read successfully, in seconds since the Unix epoch.
pub const LAST_CHECK_KEY: &str = "updates.last-check";
/// Setting: install a downloaded, verified update when the app closes, without asking (default
/// off).
pub const INSTALL_ON_CLOSE_KEY: &str = "updates.install-on-close";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const CHUNK: usize = 64 * 1024;
/// The most redirects followed for one request (release downloads redirect twice).
const MAX_REDIRECTS: usize = 10;

/// How often the channel is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateInterval {
    Daily,
    Weekly,
    Monthly,
}

impl UpdateInterval {
    /// Reads the stored value; anything unknown is the default.
    #[must_use]
    pub fn from_setting(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_str) {
            Some("weekly") => Self::Weekly,
            Some("monthly") => Self::Monthly,
            _ => Self::Daily,
        }
    }

    #[must_use]
    pub fn seconds(self) -> i64 {
        match self {
            Self::Daily => 24 * 3600,
            Self::Weekly => 7 * 24 * 3600,
            Self::Monthly => 30 * 24 * 3600,
        }
    }
}

/// The update settings as they stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateConfig {
    /// The toggle in Settings.
    pub enabled: bool,
    /// The manifest address, when one is set.
    pub channel: Option<String>,
    pub interval: UpdateInterval,
}

impl UpdateConfig {
    /// Whether the app checks by itself: the toggle is on and a channel is configured.
    #[must_use]
    pub fn active(&self) -> bool {
        self.enabled && self.channel.is_some()
    }
}

/// A newer release the user can install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOffer {
    pub version: String,
    /// The version that is running now.
    pub current: String,
    pub published: Option<String>,
    /// The release notes from the signed manifest.
    pub notes: String,
    pub size_bytes: u64,
}

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// Updates are off, or no channel is configured: nothing was requested.
    Disabled,
    /// The last check is younger than the interval: nothing was requested.
    NotDue,
    UpToDate,
    Available(UpdateOffer),
}

/// Why an update could not be checked or installed. Every message is ready to show.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("No update channel is set. Add the address of an update channel in Settings first.")]
    NoChannel,
    #[error("The update channel address {0} is not https, so nothing was requested from it.")]
    InsecureUrl(String),
    #[error("The update server redirected to {0}, which is not https, so it was not followed.")]
    InsecureRedirect(String),
    #[error(
        "The update channel {0} is not a web address. Enter one that starts with https:// in Settings."
    )]
    BadUrl(String),
    #[error(
        "This version of CineVault has no built-in update signing key. Enter the key the update channel is signed with under Update signing key in Settings."
    )]
    NoKey,
    #[error("The update signing key is not valid. It should be 32 bytes written as base64.")]
    BadKey,
    #[error("The update channel could not be read: {0}.")]
    Network(String),
    #[error(
        "The update information is not signed by the expected key, so it was ignored. Nothing was installed."
    )]
    BadSignature,
    #[error("The update information was ignored because {0}.")]
    BadManifest(String),
    #[error("Version {0} has no build for this computer.")]
    NoBuild(String),
    #[error("There is no update to install. Check for updates first.")]
    NothingOffered,
    #[error("The update changed since you looked at it. Check for updates again.")]
    OfferChanged,
    #[error("The download was cancelled.")]
    Cancelled,
    #[error("The update download is damaged (its checksum does not match), so it was deleted.")]
    ChecksumMismatch,
    #[error("The update download has the wrong size, so it was deleted.")]
    WrongSize,
    #[error("The update could not be downloaded: {0}.")]
    Download(String),
    #[error("The update could not be saved: {0}.")]
    Io(String),
    #[error(
        "The update was downloaded but its setup could not be started: {0}. CineVault keeps running."
    )]
    Launch(String),
    #[error("{0}")]
    Settings(#[from] AppSettingsError),
}

impl UpdateError {
    /// A stable code for the UI: `no_channel`, `insecure`, `bad_url`, `no_key`, `bad_key`, `network`,
    /// `bad_signature`, `bad_manifest`, `no_build`, `nothing_offered`, `offer_changed`,
    /// `cancelled`, `checksum`, `size`, `download`, `storage`, `launch` or `settings`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoChannel => "no_channel",
            Self::InsecureUrl(_) | Self::InsecureRedirect(_) => "insecure",
            Self::BadUrl(_) => "bad_url",
            Self::NoKey => "no_key",
            Self::BadKey => "bad_key",
            Self::Network(_) => "network",
            Self::BadSignature => "bad_signature",
            Self::BadManifest(_) => "bad_manifest",
            Self::NoBuild(_) => "no_build",
            Self::NothingOffered => "nothing_offered",
            Self::OfferChanged => "offer_changed",
            Self::Cancelled => "cancelled",
            Self::ChecksumMismatch => "checksum",
            Self::WrongSize => "size",
            Self::Download(_) => "download",
            Self::Io(_) => "storage",
            Self::Launch(_) => "launch",
            Self::Settings(_) => "settings",
        }
    }
}

/// Whether `url` may be requested: `https`, or plain `http` to this computer (the test server).
/// Returns whether the request must be https only.
fn check_url(url: &str) -> Result<bool, UpdateError> {
    let Some((scheme, rest)) = url.split_once("://") else {
        return Err(UpdateError::BadUrl(url.to_owned()));
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let named = !scheme.is_empty()
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && !host.is_empty()
        && !host.contains(char::is_whitespace);
    if !named {
        return Err(UpdateError::BadUrl(url.to_owned()));
    }
    if url.starts_with("https://") {
        return Ok(true);
    }
    if let Some(rest) = url.strip_prefix("http://") {
        let host = rest.split(['/', ':', '?', '#']).next().unwrap_or_default();
        if host == "127.0.0.1" || host == "localhost" || rest.starts_with("[::1]") {
            return Ok(false);
        }
    }
    Err(UpdateError::InsecureUrl(url.to_owned()))
}

fn agent(https_only: bool, total: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(https_only)
        // Redirects are followed by hand in `fetch`, so every hop is checked like the first.
        .max_redirects(0)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(total))
        .user_agent(concat!("CineVault/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn describe(error: &ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(code) => format!("the server answered {code}"),
        ureq::Error::HostNotFound => "the server could not be found".to_owned(),
        ureq::Error::ConnectionFailed => "the connection failed".to_owned(),
        ureq::Error::Timeout(_) => "the server took too long to answer".to_owned(),
        other => other.to_string(),
    }
}

/// Resolves a `Location` header against the address that answered with it.
pub(super) fn resolve_location(base: &str, location: &str) -> String {
    let location = location.trim();
    let absolute = location.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if absolute {
        return location.to_owned();
    }
    let (scheme, rest) = base.split_once("://").unwrap_or(("https", base));
    if let Some(network_path) = location.strip_prefix("//") {
        return format!("{scheme}://{network_path}");
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let origin = format!("{scheme}://{}", &rest[..authority_end]);
    if location.starts_with('/') {
        return format!("{origin}{location}");
    }
    let path = rest[authority_end..]
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    let dir = path.rfind('/').map_or("/", |end| &path[..=end]);
    format!("{origin}{dir}{location}")
}

/// GETs `url`, following up to [`MAX_REDIRECTS`] redirects. Every hop must pass the same address
/// check as the first, and a chain that started on https never drops to plain http (not even to
/// this computer). `wrap` turns a transport failure into the caller's error.
fn fetch(
    url: &str,
    total: Duration,
    wrap: fn(String) -> UpdateError,
) -> Result<ureq::http::Response<ureq::Body>, UpdateError> {
    let mut https_only = check_url(url)?;
    let mut current = url.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        let response = agent(https_only, total)
            .get(&current)
            .call()
            .map_err(|error| wrap(describe(&error)))?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        let location = response
            .headers()
            .get(ureq::http::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                wrap(format!(
                    "the server answered {} without saying where to go",
                    response.status().as_u16()
                ))
            })?;
        let next = resolve_location(&current, location);
        let next_https_only = match check_url(&next) {
            Ok(secure) => secure,
            Err(UpdateError::InsecureUrl(_)) => return Err(UpdateError::InsecureRedirect(next)),
            Err(other) => return Err(other),
        };
        if https_only && !next_https_only {
            return Err(UpdateError::InsecureRedirect(next));
        }
        https_only = next_https_only;
        current = next;
    }
    Err(wrap("the server redirected too many times".to_owned()))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// A file name for the installer: the last part of the address, reduced to safe characters.
pub(super) fn installer_name(url: &str, version: &Version) -> String {
    let last = url
        .split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit('/').next())
        .unwrap_or_default();
    let safe: String = last
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    let safe = safe.trim_start_matches('.');
    if safe.is_empty() {
        format!("CineVault-{version}-update")
    } else {
        safe.to_owned()
    }
}

/// A verified offer and the build it names.
#[derive(Debug, Clone)]
struct Offered {
    view: UpdateOffer,
    version: Version,
    artifact: ManifestArtifact,
}

/// Checks the update channel and downloads installers. Cheap to share behind an `Arc`.
pub struct UpdateService {
    settings: Arc<SettingsService>,
    current: Version,
    platform: String,
    downloads: PathBuf,
    /// The channel used when the setting is empty.
    default_channel: Option<String>,
    /// The key used when the setting is empty.
    built_in_key: Option<String>,
    offered: Mutex<Option<Offered>>,
    /// The installer of the last download that passed its checks, for "install when I close".
    ready: Mutex<Option<VerifiedInstaller>>,
    /// One download at a time, so the confirmed install and the download in the background do
    /// not write the same file.
    downloading: Mutex<()>,
}

impl UpdateService {
    /// `current_version` is the running build's version; installers are downloaded under
    /// `downloads`. A version that does not parse counts as `0.0.0`.
    #[must_use]
    pub fn new(settings: Arc<SettingsService>, current_version: &str, downloads: PathBuf) -> Self {
        Self {
            settings,
            current: Version::parse(current_version).unwrap_or_else(|_| Version::new(0, 0, 0)),
            platform: platform_id(),
            downloads,
            default_channel: Some(BUILT_IN_CHANNEL.to_owned()),
            built_in_key: BUILT_IN_PUBLIC_KEY.map(str::to_owned),
            offered: Mutex::new(None),
            ready: Mutex::new(None),
            downloading: Mutex::new(()),
        }
    }

    /// Replaces the built-in channel used when the setting is empty (`None`: no channel). For
    /// tests, which must never reach the real channel.
    #[must_use]
    pub fn with_default_channel(mut self, channel: Option<&str>) -> Self {
        self.default_channel = channel.map(str::to_owned);
        self
    }

    /// Replaces the built-in public key used when the setting is empty. For tests.
    #[must_use]
    pub fn with_built_in_key(mut self, key: Option<&str>) -> Self {
        self.built_in_key = key.map(str::to_owned);
        self
    }

    /// Looks for builds of another platform id. For tests.
    #[must_use]
    pub fn with_platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = platform.into();
        self
    }

    fn text_setting(&self, key: &str) -> Option<String> {
        self.settings
            .get(key)
            .and_then(|value| value.as_str().map(|text| text.trim().to_owned()))
            .filter(|text| !text.is_empty())
    }

    /// The update settings now.
    #[must_use]
    pub fn config(&self) -> UpdateConfig {
        UpdateConfig {
            enabled: self
                .settings
                .get(CHECK_KEY)
                .and_then(|value| value.as_bool())
                .unwrap_or(true),
            // An empty setting (the default, and what older installs have) is the built-in
            // channel, never "no channel".
            channel: self
                .text_setting(CHANNEL_KEY)
                .or_else(|| self.default_channel.clone()),
            interval: UpdateInterval::from_setting(self.settings.get(INTERVAL_KEY).as_ref()),
        }
    }

    /// Whether a downloaded, verified update installs when the app closes (Settings, Updates,
    /// "Install updates when I close the app"). Off unless the user turned it on.
    #[must_use]
    pub fn install_on_close(&self) -> bool {
        self.settings
            .get(INSTALL_ON_CLOSE_KEY)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    /// The version of the downloaded, verified installer that waits, if there is one.
    #[must_use]
    pub fn ready_version(&self) -> Option<String> {
        self.ready
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|installer| installer.version().to_owned())
    }

    /// The installer to start when the app closes: the one a download verified, and only while
    /// the setting is on. Whoever starts it checks it once more
    /// ([`VerifiedInstaller::reverify`]), exactly as for a confirmed install.
    #[must_use]
    pub fn installer_for_close(&self) -> Option<VerifiedInstaller> {
        if !self.install_on_close() {
            return None;
        }
        self.ready
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The offer found by the last check, if the user has not installed it.
    #[must_use]
    pub fn offer(&self) -> Option<UpdateOffer> {
        self.offered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|offered| offered.view.clone())
    }

    fn last_check(&self) -> Option<i64> {
        self.settings.get(LAST_CHECK_KEY).and_then(|v| v.as_i64())
    }

    /// The check the app runs by itself: it does nothing unless updates are on, a channel is
    /// set and the interval has passed since the last successful check. `now` is seconds since
    /// the Unix epoch.
    ///
    /// # Errors
    /// See [`UpdateError`]. A failed check does not count as done, so the next start tries
    /// again.
    pub fn check_if_due(&self, now: i64) -> Result<CheckOutcome, UpdateError> {
        let config = self.config();
        if !config.active() {
            return Ok(CheckOutcome::Disabled);
        }
        if let Some(last) = self.last_check()
            && last <= now
            && now - last < config.interval.seconds()
        {
            return Ok(CheckOutcome::NotDue);
        }
        self.run_check(&config, now)
    }

    /// The check the user asks for: it needs a channel but ignores the toggle and the interval.
    ///
    /// # Errors
    /// [`UpdateError::NoChannel`] without a channel, otherwise as [`Self::check_if_due`].
    pub fn check_now(&self, now: i64) -> Result<CheckOutcome, UpdateError> {
        let config = self.config();
        if config.channel.is_none() {
            return Err(UpdateError::NoChannel);
        }
        self.run_check(&config, now)
    }

    fn verifying_key(&self) -> Result<VerifyingKey, UpdateError> {
        let own = self.text_setting(PUBLIC_KEY_KEY);
        match own.as_deref().or(self.built_in_key.as_deref()) {
            Some(text) => parse_public_key(text),
            None => Err(UpdateError::NoKey),
        }
    }

    fn run_check(&self, config: &UpdateConfig, now: i64) -> Result<CheckOutcome, UpdateError> {
        let url = config.channel.as_deref().ok_or(UpdateError::NoChannel)?;
        // The address is the first thing that can be wrong, so it is checked before the key.
        check_url(url)?;
        let key = self.verifying_key()?;
        let body = fetch(url, MANIFEST_TIMEOUT, UpdateError::Network)?
            .body_mut()
            .with_config()
            .limit(MAX_MANIFEST_BYTES)
            .read_to_vec()
            .map_err(|error| UpdateError::Network(error.to_string()))?;
        let manifest = verify_manifest(&body, &key)?;
        self.settings.set(LAST_CHECK_KEY, Value::from(now))?;

        let mut offered = self.offered.lock().unwrap_or_else(PoisonError::into_inner);
        if manifest.version <= self.current {
            *offered = None;
            self.forget_ready_except(None);
            return Ok(CheckOutcome::UpToDate);
        }
        let artifact = manifest
            .platforms
            .get(&self.platform)
            .cloned()
            .ok_or_else(|| UpdateError::NoBuild(manifest.version.to_string()))?;
        let view = UpdateOffer {
            version: manifest.version.to_string(),
            current: self.current.to_string(),
            published: manifest.published,
            notes: manifest.notes,
            size_bytes: artifact.size,
        };
        self.forget_ready_except(Some(&view.version));
        *offered = Some(Offered {
            view: view.clone(),
            version: manifest.version,
            artifact,
        });
        Ok(CheckOutcome::Available(view))
    }

    /// Drops a ready installer that is not of `keep` (the version now on offer).
    fn forget_ready_except(&self, keep: Option<&str>) {
        let mut ready = self.ready.lock().unwrap_or_else(PoisonError::into_inner);
        if ready
            .as_ref()
            .is_some_and(|installer| Some(installer.version()) != keep)
        {
            *ready = None;
        }
    }

    /// Downloads the installer of the offered `version` after checking the size and SHA-256 the
    /// signed manifest gave, and returns it as the only kind of file an update may start.
    /// `on_bytes` receives the running total. The caller starts the installer; this is only
    /// called after the user confirmed.
    ///
    /// # Errors
    /// [`UpdateError::NothingOffered`] and [`UpdateError::OfferChanged`] when `version` is not
    /// the verified offer, and the download errors. A file that fails a check is deleted.
    pub fn download(
        &self,
        version: &str,
        cancel: &AtomicBool,
        on_bytes: &mut dyn FnMut(u64),
    ) -> Result<VerifiedInstaller, UpdateError> {
        let offered = self
            .offered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(UpdateError::NothingOffered)?;
        if offered.view.version != version {
            return Err(UpdateError::OfferChanged);
        }
        let _one_at_a_time = self
            .downloading
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Downloaded already (in the background, for "install when I close"): checked again,
        // and used when it still matches.
        let waiting = self
            .ready
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .filter(|installer| installer.version() == version);
        if let Some(installer) = waiting
            && installer.reverify().is_ok()
        {
            return Ok(installer);
        }
        let dir = self.downloads.join(offered.version.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| UpdateError::Io(e.to_string()))?;
        let dest = dir.join(installer_name(&offered.artifact.url, &offered.version));
        let _ = std::fs::remove_file(&dest);
        let result = fetch_installer(&offered.artifact, &dest, cancel, on_bytes);
        if result.is_err() {
            let _ = std::fs::remove_file(&dest);
        }
        result?;
        tracing::info!(version, path = %dest.display(), "update downloaded and verified");
        let installer = VerifiedInstaller::new(
            dest,
            self.downloads.clone(),
            offered.view.version.clone(),
            offered.artifact.size,
            offered.artifact.sha256.clone(),
        );
        *self.ready.lock().unwrap_or_else(PoisonError::into_inner) = Some(installer.clone());
        Ok(installer)
    }

    /// The folder installers are downloaded into.
    #[must_use]
    pub fn downloads(&self) -> &Path {
        &self.downloads
    }

    /// Removes installers left by earlier updates. Best effort: a setup that is still running
    /// keeps its file, and the next start tries again.
    pub fn sweep_downloads(&self) {
        let Ok(entries) = std::fs::read_dir(&self.downloads) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let removed = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if let Err(error) = removed {
                tracing::debug!(%error, path = %path.display(), "an old update download stays");
            }
        }
    }
}

fn fetch_installer(
    artifact: &ManifestArtifact,
    dest: &Path,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<(), UpdateError> {
    let mut response = fetch(&artifact.url, DOWNLOAD_TIMEOUT, UpdateError::Download)?;
    // One byte past the signed size is enough to tell the body is too long.
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(artifact.size.saturating_add(1))
        .reader();
    let io = |error: std::io::Error| UpdateError::Io(error.to_string());
    let mut file = File::create_new(dest).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    let mut total = 0_u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(UpdateError::Cancelled);
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| UpdateError::Download(error.to_string()))?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > artifact.size {
            return Err(UpdateError::WrongSize);
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(io)?;
        on_bytes(total);
    }
    file.flush().map_err(io)?;
    file.sync_all().map_err(io)?;
    if total != artifact.size {
        return Err(UpdateError::WrongSize);
    }
    if !hex(hasher.finalize().as_slice()).eq_ignore_ascii_case(artifact.sha256.trim()) {
        return Err(UpdateError::ChecksumMismatch);
    }
    Ok(())
}

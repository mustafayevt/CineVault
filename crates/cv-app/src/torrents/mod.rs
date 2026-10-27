//! Torrent search in the app: the service that builds the sources from the
//! settings, finds titles on TMDB, and reads what the library holds of a title.
//!
//! Torrent search is always on, behind one responsibility notice. Until `torrents.notice-accepted`
//! is set every method returns [`TorrentError::NoticeNotAccepted`] and nothing is contacted: no
//! TMDB request, no source, no process.
//! The searching itself is `cv_torrent::search`; the Tauri commands drive it through
//! [`TorrentService::sources`] and [`TorrentService::build_query`].
//!
//! Sources use their own HTTP client (`cv_torrent::source::client_builder`: 20 second timeout, no
//! retry, the CineVault User-Agent, an 8 MiB answer limit) that shares the app's redaction.

pub mod indexer;
pub mod indexer_update;
mod landing;
mod library;
mod title;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use cv_db::Database;
use cv_providers::HttpClient;
use cv_torrent::model::{SourceError, SourceKind, SourceOutcome};
use cv_torrent::source::{SourceHttp, TorrentSource, check_status, looks_like_html};
use cv_torrent::sources::apibay::ApibaySource;
use cv_torrent::sources::eztv::EztvSource;
use cv_torrent::sources::knaben::KnabenSource;
use cv_torrent::sources::nyaa::NyaaSource;
use cv_torrent::sources::torrentio::TorrentioSource;
use cv_torrent::sources::torznab::TorznabSource;
use cv_torrent::sources::torznab::{
    CapsSummary, JACKETT_DEFAULT_ADDRESS, detect_jackett, jackett_all_feed, test_connection,
};
use cv_torrent::sources::yts::YtsSource;
use cv_torrent::{TorrentError, model::TorrentQuery};

pub use landing::{Landing, LandingTitle, STRIP_LENGTH, landing_of};
pub use library::{
    LibraryHolding, LibraryLink, MovieCopy, copy_of, hdr_of, movie_copy_of, resolution_of,
};
pub use title::{EpisodeInfo, SeasonInfo, TitleDetails, TitleHit};

use crate::app_settings::SettingsService;
use crate::provider_credentials::ProviderCredentialsService;
use crate::secrets::SecretStore;
use crate::torrent_settings::{
    INDEXER_ENABLED_KEY, SOURCE_IDS, TORZNAB_KEY_ACCOUNT, TORZNAB_URL_KEY, TorrentSecrets,
    TorrentSettings, clean_base_url, source_key,
};

/// Why a Torrent search call failed.
#[derive(Debug, thiserror::Error)]
pub enum TorrentServiceError {
    /// Torrent search is off, or another failure of the torrent code itself.
    #[error("{0}")]
    Torrent(#[from] TorrentError),
    /// TMDB (the title search) needs a key the user has not set.
    #[error("TMDB needs an API key. Add yours in Settings, Scraping and providers.")]
    NeedsKey,
    /// The saved TMDB key was rejected.
    #[error("TMDB rejected the saved key.")]
    KeyRejected,
    /// This build has no TMDB provider.
    #[error("There is no TMDB provider to look titles up with.")]
    NoMetadataProvider,
    /// The title is not on TMDB.
    #[error("That title was not found.")]
    NotFound,
    /// TMDB could not answer.
    #[error("{message}")]
    Provider { message: String, retryable: bool },
    /// The library could not be read.
    #[error("CineVault could not read its library: {0}")]
    Library(String),
}

/// A Jackett the person runs themselves, found by [`TorrentService::detect_jackett`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnJackett {
    /// `http://127.0.0.1:9117`
    pub base_url: String,
    /// Its all-indexers Torznab feed, the address the own-instance setting takes.
    pub feed_url: String,
    /// Whether the own-instance setting already points at that feed.
    pub in_use: bool,
}

/// The base address of a Jackett feed (`http://host:port` of
/// `http://host:port/api/v2.0/indexers/...`), or `None` when the address is no Jackett feed.
fn jackett_base_of(feed: &str) -> Option<String> {
    let clean = clean_base_url(feed)?;
    let at = clean.find("/api/v2.0/indexers/")?;
    Some(clean[..at].to_owned())
}

/// Whether `base` is this computer at `port`.
fn is_local_port(base: &str, port: u16) -> bool {
    let rest = base
        .strip_prefix("http://")
        .or_else(|| base.strip_prefix("https://"))
        .unwrap_or(base);
    let authority = rest.split('/').next().unwrap_or_default();
    ["127.0.0.1", "localhost", "[::1]"]
        .iter()
        .any(|host| authority.eq_ignore_ascii_case(&format!("{host}:{port}")))
}

/// Where to look for the person's own Jackett: the address of their saved Torznab instance when
/// it is a Jackett feed, then the default address; never the managed service's port.
#[must_use]
pub fn jackett_candidates(saved_feed: &str, managed_port: Option<u16>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for base in jackett_base_of(saved_feed)
        .into_iter()
        .chain(std::iter::once(JACKETT_DEFAULT_ADDRESS.to_owned()))
    {
        let ours = managed_port.is_some_and(|port| is_local_port(&base, port));
        if !ours && !out.contains(&base) {
            out.push(base);
        }
    }
    out
}

/// Torrent search for the app.
pub struct TorrentService {
    settings: Arc<SettingsService>,
    secrets: Arc<SecretStore>,
    db: Arc<Database>,
    providers: Arc<ProviderCredentialsService>,
    client: OnceLock<Option<HttpClient>>,
    episodes: title::EpisodeCache,
    /// Sources added by other parts of the app (the managed indexer service).
    extra: Mutex<HashMap<String, Arc<dyn TorrentSource>>>,
    /// What the last search found out about each source.
    health: Mutex<HashMap<String, SourceHealth>>,
}

/// How a source is doing, as Settings shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Answered its last search.
    Working,
    /// Behind a browser check the indexer service could not pass.
    NeedsSolver,
    /// Nothing answered: connection, timeout, a server error.
    Unreachable,
    /// Answered with something unusable, was rate limited or blocked.
    Error,
    /// Not asked yet.
    Untested,
    /// Switched off.
    Off,
    /// A check is running.
    Checking,
}

/// What the last search learned about one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHealth {
    pub health: Health,
    /// Plain words for a source that is not working.
    pub reason: Option<String>,
    /// Epoch milliseconds.
    pub checked_at_ms: i64,
}

/// A source as the Torrent sources settings list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStatus {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    pub on: bool,
    pub health: Health,
    pub reason: Option<String>,
    pub checked_at_ms: Option<i64>,
}

/// What to do with the saved Torznab key.
#[derive(Clone, PartialEq, Eq, Default)]
pub enum KeyChange {
    /// Leave it as it is.
    #[default]
    Keep,
    /// Save this one.
    Set(String),
    /// Remove it.
    Clear,
}

impl std::fmt::Debug for KeyChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Keep => "Keep",
            Self::Set(_) => "Set(<redacted>)",
            Self::Clear => "Clear",
        })
    }
}

const BUILT_IN_NAMES: [(&str, &str); 6] = [
    ("torrentio", "Torrentio"),
    ("yts", "YTS"),
    ("eztv", "EZTV"),
    ("apibay", "The Pirate Bay"),
    ("knaben", "Knaben"),
    ("nyaa", "Nyaa"),
];

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl std::fmt::Debug for TorrentService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TorrentService").finish_non_exhaustive()
    }
}

impl TorrentService {
    /// A service over the app's settings, secrets, library and providers.
    #[must_use]
    pub fn new(
        settings: Arc<SettingsService>,
        secrets: Arc<SecretStore>,
        db: Arc<Database>,
        providers: Arc<ProviderCredentialsService>,
    ) -> Self {
        Self {
            settings,
            secrets,
            db,
            providers,
            client: OnceLock::new(),
            episodes: Mutex::default(),
            extra: Mutex::default(),
            health: Mutex::default(),
        }
    }

    /// Uses `client` for the sources instead of building one (tests, or a client set up by the
    /// composition root).
    #[must_use]
    pub fn with_client(self, client: HttpClient) -> Self {
        let _ = self.client.set(Some(client));
        self
    }

    /// Whether the responsibility notice was accepted, which is what lets Torrent search contact
    /// anything.
    #[must_use]
    pub fn notice_accepted(&self) -> bool {
        self.settings_now().notice_accepted
    }

    /// Fails with [`TorrentError::NoticeNotAccepted`] until the notice was accepted.
    ///
    /// # Errors
    /// `NoticeNotAccepted`.
    pub fn ensure_accepted(&self) -> Result<(), TorrentError> {
        if self.notice_accepted() {
            Ok(())
        } else {
            Err(TorrentError::NoticeNotAccepted)
        }
    }

    /// The settings now.
    #[must_use]
    pub fn settings_now(&self) -> TorrentSettings {
        TorrentSettings::read(&self.settings)
    }

    /// The HTTP client of the sources, built on first use. The managed indexer service uses it
    /// too, for its readiness checks, its indexers and the check for a newer version.
    #[must_use]
    pub fn http_client(&self) -> Option<HttpClient> {
        self.http()
    }

    fn http(&self) -> Option<HttpClient> {
        self.client
            .get_or_init(|| {
                cv_torrent::source::client_builder()
                    .redactor(Arc::new(|text: &str| {
                        crate::redact::redact(text).into_owned()
                    }))
                    .build()
                    .map_err(
                        |error| tracing::warn!(%error, "the torrent HTTP client did not start"),
                    )
                    .ok()
            })
            .clone()
    }

    /// Adds (or replaces) a source that another part of the app provides, such as one indexer
    /// of the managed service. It is searched with the built-in ones while the feature is on.
    pub fn set_extra_source(&self, source: Arc<dyn TorrentSource>) {
        self.extra
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(source.id().0.clone(), source);
    }

    /// Removes a source added with [`Self::set_extra_source`].
    pub fn remove_extra_source(&self, id: &str) {
        self.extra
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }

    /// The sources to search for this query: the built-in ones that are switched on, the
    /// sources other parts of the app added, and the user's own Torznab instance when its
    /// address and key are set. A source that does not apply to the query is still listed (the
    /// search reports it as skipped, with the reason).
    ///
    /// # Errors
    /// `NoticeNotAccepted` before the notice is accepted.
    pub fn sources(
        &self,
        _query: &TorrentQuery,
    ) -> Result<Vec<Arc<dyn TorrentSource>>, TorrentError> {
        self.ensure_accepted()?;
        let settings = self.settings_now();
        let Some(client) = self.http() else {
            return Err(TorrentError::Invalid(
                "The torrent sources could not start.".to_owned(),
            ));
        };
        let mut sources: Vec<Arc<dyn TorrentSource>> = Vec::new();
        let url = |id: &str| settings.base_url(id).unwrap_or_default().to_owned();
        if settings.source_on("torrentio") {
            sources.push(Arc::new(TorrentioSource::new(
                client.clone(),
                &url("torrentio"),
            )));
        }
        if settings.source_on("yts") {
            sources.push(Arc::new(YtsSource::new(client.clone(), &url("yts"))));
        }
        if settings.source_on("eztv") {
            sources.push(Arc::new(EztvSource::new(client.clone(), &url("eztv"))));
        }
        if settings.source_on("apibay") {
            sources.push(Arc::new(ApibaySource::new(client.clone(), &url("apibay"))));
        }
        if settings.source_on("knaben") {
            sources.push(Arc::new(KnabenSource::new(client.clone(), &url("knaben"))));
        }
        if settings.source_on("nyaa") {
            sources.push(Arc::new(NyaaSource::new(client.clone(), &url("nyaa"))));
        }
        let mut extra: Vec<_> = self
            .extra
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        extra.sort_by(|a, b| a.id().cmp(b.id()));
        sources.extend(extra);
        if !settings.torznab_url.is_empty()
            && let Ok(secrets) = TorrentSecrets::read(&self.secrets)
            && let Some(key) = secrets.torznab_api_key
        {
            sources.push(Arc::new(TorznabSource::user_instance(
                client,
                &settings.torznab_url,
                &key,
            )));
        }
        Ok(sources)
    }

    /// Records what a search found out about a source, for the health column of Settings.
    pub fn note_outcome(&self, id: &str, outcome: &SourceOutcome) {
        let (health, reason) = match outcome {
            SourceOutcome::Done { .. } => (Health::Working, None),
            // A source that did not apply says nothing about how it is doing.
            SourceOutcome::Skipped(_) => return,
            SourceOutcome::Failed(error) => {
                let health = match error {
                    SourceError::Unreachable(_) | SourceError::Timeout => Health::Unreachable,
                    SourceError::RateLimited
                    | SourceError::Blocked
                    | SourceError::BadAnswer(_)
                    | SourceError::KeyRejected => Health::Error,
                };
                (health, Some(failure_words(error)))
            }
        };
        self.health
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                id.to_owned(),
                SourceHealth {
                    health,
                    reason,
                    checked_at_ms: now_ms(),
                },
            );
    }

    /// Every source with its switch and health: the six built-in ones, the sources other parts
    /// of the app added, and the user's own instance when it is set up.
    ///
    /// # Errors
    /// `NoticeNotAccepted` before the notice is accepted.
    pub fn source_statuses(&self) -> Result<Vec<SourceStatus>, TorrentError> {
        self.ensure_accepted()?;
        let settings = self.settings_now();
        let health = self
            .health
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let status = |id: &str, name: &str, kind: SourceKind, on: bool| {
            let known = health.get(id);
            SourceStatus {
                id: id.to_owned(),
                name: name.to_owned(),
                kind,
                on,
                health: if on {
                    known.map_or(Health::Untested, |h| h.health)
                } else {
                    Health::Off
                },
                reason: if on {
                    known.and_then(|h| h.reason.clone())
                } else {
                    None
                },
                checked_at_ms: known.map(|h| h.checked_at_ms),
            }
        };
        let mut out: Vec<SourceStatus> = BUILT_IN_NAMES
            .iter()
            .map(|(id, name)| status(id, name, SourceKind::BuiltIn, settings.source_on(id)))
            .collect();
        let mut extra: Vec<(String, String, SourceKind)> = self
            .extra
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|s| (s.id().0.clone(), s.name().to_owned(), s.kind()))
            .collect();
        extra.sort_by(|a, b| a.0.cmp(&b.0));
        out.extend(
            extra
                .iter()
                .map(|(id, name, kind)| status(id, name, *kind, true)),
        );
        if !settings.torznab_url.is_empty() {
            out.push(status(
                "torznab",
                cv_torrent::sources::torznab::USER_INSTANCE_NAME,
                SourceKind::Torznab,
                self.has_torznab_key(),
            ));
        }
        Ok(out)
    }

    /// Switches a source on or off. `indexer-service` switches the managed indexer service.
    ///
    /// # Errors
    /// `NoticeNotAccepted` before the notice is accepted; an unknown source is refused.
    pub fn set_source(&self, id: &str, on: bool) -> Result<(), TorrentServiceError> {
        self.ensure_accepted()?;
        let key = if id == "indexer-service" {
            INDEXER_ENABLED_KEY.to_owned()
        } else if SOURCE_IDS.contains(&id) {
            source_key(id)
        } else {
            return Err(TorrentError::Invalid(format!("There is no source called {id:?}.")).into());
        };
        self.settings
            .set(&key, serde_json::Value::Bool(on))
            .map_err(|e| TorrentError::Invalid(format!("The switch could not be saved: {e}")))?;
        Ok(())
    }

    /// Whether a Torznab key is saved.
    #[must_use]
    pub fn has_torznab_key(&self) -> bool {
        self.secrets.contains(TORZNAB_KEY_ACCOUNT).unwrap_or(false)
    }

    /// Saves the address of the user's own Torznab instance and changes its key: kept, replaced
    /// or removed. An empty address removes the instance (and its key). The key goes to the
    /// operating system's credential store, never to the settings file.
    ///
    /// # Errors
    /// `NoticeNotAccepted` before the notice is accepted; an address that is not `http://` or `https://`, or
    /// a credential store that cannot be written, is refused.
    pub fn set_torznab(&self, url: &str, key: KeyChange) -> Result<(), TorrentServiceError> {
        self.ensure_accepted()?;
        let url = url.trim();
        let stored_url = if url.is_empty() {
            String::new()
        } else {
            clean_base_url(url).ok_or_else(|| {
                TorrentError::Invalid(
                    "Enter an address that starts with http:// or https://, with no user name or query."
                        .to_owned(),
                )
            })?
        };
        let key = if stored_url.is_empty() {
            KeyChange::Clear
        } else {
            key
        };
        match key {
            KeyChange::Keep => {}
            KeyChange::Set(value) if !value.trim().is_empty() => {
                self.secrets
                    .set(TORZNAB_KEY_ACCOUNT, value.trim())
                    .map_err(|e| {
                        TorrentError::Invalid(format!("The key could not be saved: {e}"))
                    })?;
            }
            KeyChange::Set(_) | KeyChange::Clear => {
                self.secrets.delete(TORZNAB_KEY_ACCOUNT).map_err(|e| {
                    TorrentError::Invalid(format!("The key could not be removed: {e}"))
                })?;
            }
        }
        self.settings
            .set(TORZNAB_URL_KEY, serde_json::Value::String(stored_url))
            .map_err(|e| TorrentError::Invalid(format!("The address could not be saved: {e}")))?;
        self.health
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove("torznab");
        Ok(())
    }

    /// Looks for a Jackett the person runs themselves: at the address of their own Torznab
    /// instance when one is set, then at [`JACKETT_DEFAULT_ADDRESS`]. The managed indexer service
    /// (listening on `managed_port` of this computer) is not theirs and is never offered. Only
    /// the feed is asked, without a key; no other program's files are read.
    ///
    /// # Errors
    /// `NoticeNotAccepted` before the notice is accepted.
    pub async fn detect_jackett(
        &self,
        managed_port: Option<u16>,
    ) -> Result<Option<OwnJackett>, TorrentServiceError> {
        self.ensure_accepted()?;
        let Some(client) = self.http() else {
            return Ok(None);
        };
        let saved = self.settings_now().torznab_url;
        for base in jackett_candidates(&saved, managed_port) {
            if detect_jackett(client.clone(), &base).await {
                let feed_url = jackett_all_feed(&base);
                let in_use = clean_base_url(&saved).is_some_and(|saved| saved == feed_url);
                return Ok(Some(OwnJackett {
                    base_url: base,
                    feed_url,
                    in_use,
                }));
            }
        }
        Ok(None)
    }

    /// Tests an instance with `key`, or with the saved key when none is given.
    ///
    /// # Errors
    /// The failure as a [`SourceError`]: `KeyRejected`, `Unreachable`, `Timeout`, `Blocked` or
    /// `BadAnswer("This is not a Torznab feed")`. A missing address or key is a `BadAnswer`
    /// that says so.
    pub async fn test_torznab(
        &self,
        url: &str,
        key: Option<&str>,
    ) -> Result<CapsSummary, SourceError> {
        self.ensure_accepted()
            .map_err(|e| SourceError::BadAnswer(e.to_string()))?;
        let Some(address) = clean_base_url(url) else {
            return Err(SourceError::BadAnswer(
                "Enter an address that starts with http:// or https://".to_owned(),
            ));
        };
        let saved;
        let key = match key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(key) => key,
            None => {
                saved = TorrentSecrets::read(&self.secrets)
                    .ok()
                    .and_then(|s| s.torznab_api_key)
                    .ok_or_else(|| SourceError::BadAnswer("Enter the API key.".to_owned()))?;
                &saved
            }
        };
        let client = self.http().ok_or_else(|| {
            SourceError::Unreachable("the torrent client did not start".to_owned())
        })?;
        test_connection(client, &address, key).await
    }

    /// Downloads a `.torrent` file the person asked for (at most 8 MiB).
    ///
    /// # Errors
    /// A bad answer before the notice is accepted; otherwise the failure of the request.
    pub async fn fetch_torrent_file(&self, url: &str) -> Result<TorrentDownload, SourceError> {
        self.ensure_accepted()
            .map_err(|e| SourceError::BadAnswer(e.to_string()))?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(SourceError::BadAnswer(
                "That address is not usable.".to_owned(),
            ));
        }
        let client = self.http().ok_or_else(|| {
            SourceError::Unreachable("the torrent client did not start".to_owned())
        })?;
        let response = SourceHttp::new(client, "download")
            .get_raw(url, &[])
            .await?;
        // The indexer service's download address answers a magnet-only site with a redirect to
        // the magnet link; there is no file, so the link is the answer.
        if (300..400).contains(&response.status)
            && let Some(location) = response.location.as_deref()
            && location.starts_with("magnet:")
        {
            return Ok(TorrentDownload::Magnet(location.to_owned()));
        }
        check_status(&response)?;
        if looks_like_html(&response.body) {
            return Err(SourceError::Blocked);
        }
        Ok(TorrentDownload::File(response.body))
    }
}

/// What a release's download address gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorrentDownload {
    /// The bytes of a `.torrent` file (not yet checked to be one).
    File(Vec<u8>),
    /// A redirect to a magnet link: the site has no file for this release.
    Magnet(String),
}

/// The longest site message shown for an unreadable answer.
pub const SITE_MESSAGE_MAX: usize = 80;

/// A site's own message about an unreadable answer, on one line and at most
/// [`SITE_MESSAGE_MAX`] characters (cut with an ellipsis). `None` when it says nothing.
#[must_use]
pub fn site_message(message: &str) -> Option<String> {
    let line = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= SITE_MESSAGE_MAX {
        return Some(line);
    }
    let cut: String = line.chars().take(SITE_MESSAGE_MAX - 1).collect();
    Some(format!("{}…", cut.trim_end()))
}

/// A failed source in plain words.
#[must_use]
pub fn failure_words(error: &SourceError) -> String {
    match error {
        SourceError::Unreachable(_) => "Nothing answered".to_owned(),
        SourceError::RateLimited => "Rate limited".to_owned(),
        SourceError::Blocked => "Blocked by a challenge page".to_owned(),
        SourceError::BadAnswer(message) => {
            site_message(message).unwrap_or_else(|| "The answer could not be read".to_owned())
        }
        SourceError::Timeout => "Timed out".to_owned(),
        SourceError::KeyRejected => "The key was rejected".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_own_jackett_is_looked_for_at_the_saved_feed_then_the_default_address() {
        assert_eq!(
            jackett_candidates("", None),
            ["http://127.0.0.1:9117".to_owned()]
        );
        assert_eq!(
            jackett_candidates(
                "http://192.168.1.5:9117/api/v2.0/indexers/all/results/torznab/api",
                None
            ),
            [
                "http://192.168.1.5:9117".to_owned(),
                "http://127.0.0.1:9117".to_owned()
            ]
        );
        // The same address once; a feed that is not a Jackett one adds nothing.
        assert_eq!(
            jackett_candidates(
                "http://127.0.0.1:9117/api/v2.0/indexers/all/results/torznab/api/",
                None
            ),
            ["http://127.0.0.1:9117".to_owned()]
        );
        assert_eq!(
            jackett_candidates("http://nas.local:9696/1/api", None),
            ["http://127.0.0.1:9117".to_owned()]
        );
    }

    #[test]
    fn the_managed_service_is_never_offered_as_the_persons_own() {
        assert!(jackett_candidates("", Some(9117)).is_empty());
        assert_eq!(
            jackett_candidates(
                "http://localhost:51234/api/v2.0/indexers/all/results/torznab/api",
                Some(51234)
            ),
            ["http://127.0.0.1:9117".to_owned()]
        );
        assert!(is_local_port("http://[::1]:9117", 9117));
        assert!(!is_local_port("http://192.168.1.5:9117", 9117));
    }
}

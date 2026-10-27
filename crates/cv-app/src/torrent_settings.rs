//! The Torrent search settings, read from the settings map with their defaults.
//!
//! | Key | Default | Meaning |
//! |---|---|---|
//! | `torrents.notice-accepted` | off | the one-line responsibility notice was accepted |
//! | `torrents.source.<id>` | on | whether a built-in source is searched (`torrentio`, `yts`, `eztv`, `apibay`, `knaben`, `nyaa`) |
//! | `torrents.indexer-service.enabled` | on | whether the managed indexer service is downloaded and run |
//! | `torrents.indexer-service.solver-url` | empty | a challenge solver the indexer service may use |
//! | `torrents.torznab.url` | empty | the feed URL of the user's own Torznab instance |
//! | `torrents.url.<id>` | the site's address | the base address of a built-in source |
//! | `torrents.indexer-service.languages` | English and every language set elsewhere (metadata, its fallback and NFO language) | the tracker languages: only indexers in one of them are set up |
//! | `torrents.recent` | empty | the titles last opened on the Torrent search page (at most ten, newest first; written by the page) |
//!
//! Two secrets live in the OS credential store and never in the settings file: the Torznab API
//! key ([`TORZNAB_KEY_ACCOUNT`]) and the indexer service's admin password
//! ([`ADMIN_PASSWORD_ACCOUNT`]). Reading them through [`TorrentSecrets::read`] registers them
//! with the log redactor ([`SecretStore::get`] does that for every value it hands out).
//!
//! Every reader falls back to the default for a missing or unusable value, so a hand-edited or
//! newer settings file can never stop a search. The settings only say what the user chose. The
//! responsibility notice (`torrents.notice-accepted`) gates every contact: no source and no
//! indexer service download or start before it was accepted.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::app_settings::SettingsService;
use crate::secrets::{SecretError, SecretStore};

/// Setting: the responsibility notice was accepted.
pub const NOTICE_ACCEPTED_KEY: &str = "torrents.notice-accepted";
/// Setting: the managed indexer service is downloaded and run.
pub const INDEXER_ENABLED_KEY: &str = "torrents.indexer-service.enabled";
/// Setting: the challenge solver address handed to the indexer service.
pub const SOLVER_URL_KEY: &str = "torrents.indexer-service.solver-url";
/// Setting: the user's own Torznab feed URL.
pub const TORZNAB_URL_KEY: &str = "torrents.torznab.url";
/// Setting: the tracker languages (`["en", "ru"]`); absent means [`default_tracker_languages`].
pub const TRACKER_LANGUAGES_KEY: &str = "torrents.indexer-service.languages";
/// Setting: the recent searches of the Torrent search page, kept by the page itself.
pub const RECENT_KEY: &str = "torrents.recent";
/// The most recent searches the page keeps.
pub const MAX_RECENT: usize = 10;

/// Account of the Torznab API key in the credential store.
pub const TORZNAB_KEY_ACCOUNT: &str = "torrents.torznab.api-key";
/// Account of the indexer service's admin password in the credential store.
pub const ADMIN_PASSWORD_ACCOUNT: &str = "torrents.indexer-service.admin-password";

/// The ids of the built-in direct sources, in the order the interface lists them.
pub const SOURCE_IDS: [&str; 6] = ["torrentio", "yts", "eztv", "apibay", "knaben", "nyaa"];

/// The default base address of each built-in source, by id.
pub const DEFAULT_URLS: [(&str, &str); 6] = [
    ("torrentio", "https://torrentio.strem.fun"),
    ("yts", "https://movies-api.accel.li"),
    ("eztv", "https://eztvx.to"),
    ("apibay", "https://apibay.org"),
    ("knaben", "https://api.knaben.org"),
    ("nyaa", "https://nyaa.si"),
];

/// The setting that switches a built-in source: `torrents.source.<id>`.
#[must_use]
pub fn source_key(id: &str) -> String {
    format!("torrents.source.{id}")
}

/// The setting that holds a built-in source's base address: `torrents.url.<id>`.
#[must_use]
pub fn url_key(id: &str) -> String {
    format!("torrents.url.{id}")
}

/// The default base address of a built-in source, or `None` for an unknown id.
#[must_use]
pub fn default_url(id: &str) -> Option<&'static str> {
    DEFAULT_URLS
        .iter()
        .find(|(source, _)| *source == id)
        .map(|(_, url)| *url)
}

/// Every key [`TorrentSettings`] reads, for a settings listener that reacts to changes.
#[must_use]
pub fn all_keys() -> Vec<String> {
    let mut keys = vec![
        NOTICE_ACCEPTED_KEY.to_owned(),
        INDEXER_ENABLED_KEY.to_owned(),
        SOLVER_URL_KEY.to_owned(),
        TORZNAB_URL_KEY.to_owned(),
    ];
    for id in SOURCE_IDS {
        keys.push(source_key(id));
        keys.push(url_key(id));
    }
    keys
}

/// Whether `key` is one of the Torrent search settings.
#[must_use]
pub fn is_torrent_key(key: &str) -> bool {
    key.starts_with("torrents.")
}

/// A web address with only its scheme and host checked: `http` or `https`, a host, no
/// credentials, no query or fragment. The trailing slash is removed so paths can be appended.
///
/// Returns `None` for anything else, including the empty text.
#[must_use]
pub fn clean_base_url(text: &str) -> Option<String> {
    let text = text.trim();
    let (scheme, rest) = text.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty()
        || authority.contains('@')
        || authority.chars().any(|c| c.is_whitespace() || c == '\\')
    {
        return None;
    }
    let tail = &rest[authority_end..];
    if tail.contains('?') || tail.contains('#') {
        return None;
    }
    let tail = tail.trim_end_matches('/');
    Some(format!(
        "{}://{authority}{tail}",
        scheme.to_ascii_lowercase()
    ))
}

/// The language part of a code, as the indexer service names a site's language: `pt` for `pt-BR`
/// and for `por`, lower case; empty for anything that is not a language.
fn language_part(code: &str) -> String {
    let code = code.trim();
    let part = code.split(['-', '_']).next().unwrap_or_default();
    if !(2..=3).contains(&part.len()) || !part.chars().all(|c| c.is_ascii_alphabetic()) {
        return String::new();
    }
    // Three-letter codes (`eng`, `ger`, `por`) become the two-letter ones the sites use.
    let canonical = cv_media::external::normalize_language(code).unwrap_or_else(|| code.to_owned());
    canonical
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// The settings whose languages are tracker languages by default: what the person reads
/// metadata and NFO files in. Each is one code or a list of them (a JSON list
/// or text such as "en, tr").
pub const LANGUAGE_SOURCES: [&str; 3] = [
    "metadata.language",
    "metadata.fallback-language",
    "metadata.nfo-language",
];

/// The codes in a setting value: one text ("pt-BR", "en, tr; de") or a list of texts.
fn codes_in(value: &Value) -> Vec<String> {
    let texts: Vec<&str> = match value {
        Value::String(text) => vec![text.as_str()],
        Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    texts
        .into_iter()
        .flat_map(|text| text.split([',', ' ', ';']))
        .map(language_part)
        .filter(|code| !code.is_empty())
        .collect()
}

/// The tracker languages when none are chosen: English with every language the person set
/// anywhere else ([`LANGUAGE_SOURCES`]), English first and the others by code.
#[must_use]
pub fn default_tracker_languages(values: &BTreeMap<String, Value>) -> Vec<String> {
    let mut others: std::collections::BTreeSet<String> = LANGUAGE_SOURCES
        .iter()
        .filter_map(|key| values.get(*key))
        .flat_map(codes_in)
        .collect();
    others.remove("en");
    std::iter::once("en".to_owned()).chain(others).collect()
}

/// The tracker languages: the ones chosen in [`TRACKER_LANGUAGES_KEY`], else
/// [`default_tracker_languages`].
#[must_use]
pub fn tracker_languages(values: &BTreeMap<String, Value>) -> std::collections::HashSet<String> {
    if let Some(chosen) = values.get(TRACKER_LANGUAGES_KEY).and_then(Value::as_array) {
        let set: std::collections::HashSet<String> = chosen
            .iter()
            .filter_map(Value::as_str)
            .map(language_part)
            .filter(|code| !code.is_empty())
            .collect();
        if !set.is_empty() {
            return set;
        }
    }
    default_tracker_languages(values).into_iter().collect()
}

fn read_bool(values: &BTreeMap<String, Value>, key: &str, default: bool) -> bool {
    values.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn read_text(values: &BTreeMap<String, Value>, key: &str) -> String {
    values
        .get(key)
        .and_then(Value::as_str)
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

/// What the settings say about Torrent search (never the secrets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentSettings {
    /// The responsibility notice was accepted.
    pub notice_accepted: bool,
    /// The managed indexer service is downloaded and run.
    pub indexer_service_enabled: bool,
    /// The challenge solver address for the indexer service; empty for none.
    pub solver_url: String,
    /// The user's own Torznab feed address; empty for none.
    pub torznab_url: String,
    sources: BTreeMap<String, bool>,
    urls: BTreeMap<String, String>,
}

impl Default for TorrentSettings {
    fn default() -> Self {
        Self::from_values(&BTreeMap::new())
    }
}

impl TorrentSettings {
    /// The settings now. Missing and unusable values take their defaults.
    #[must_use]
    pub fn read(settings: &SettingsService) -> Self {
        Self::from_values(&settings.values())
    }

    /// The settings of a plain map of values.
    #[must_use]
    pub fn from_values(values: &BTreeMap<String, Value>) -> Self {
        let mut sources = BTreeMap::new();
        let mut urls = BTreeMap::new();
        for (id, default) in DEFAULT_URLS {
            sources.insert(id.to_owned(), read_bool(values, &source_key(id), true));
            let url = clean_base_url(&read_text(values, &url_key(id)))
                .unwrap_or_else(|| default.to_owned());
            urls.insert(id.to_owned(), url);
        }
        Self {
            notice_accepted: read_bool(values, NOTICE_ACCEPTED_KEY, false),
            indexer_service_enabled: read_bool(values, INDEXER_ENABLED_KEY, true),
            solver_url: clean_base_url(&read_text(values, SOLVER_URL_KEY)).unwrap_or_default(),
            torznab_url: read_text(values, TORZNAB_URL_KEY),
            sources,
            urls,
        }
    }

    /// Whether the built-in source `id` is switched on. An unknown id is off.
    #[must_use]
    pub fn source_on(&self, id: &str) -> bool {
        self.sources.get(id).copied().unwrap_or(false)
    }

    /// The base address of the built-in source `id`, without a trailing slash.
    #[must_use]
    pub fn base_url(&self, id: &str) -> Option<&str> {
        self.urls.get(id).map(String::as_str)
    }
}

/// The two secrets of Torrent search, read from the credential store.
#[derive(Clone, Default)]
pub struct TorrentSecrets {
    /// The API key of the user's own Torznab instance.
    pub torznab_api_key: Option<String>,
    /// The admin password of the managed indexer service.
    pub admin_password: Option<String>,
}

impl std::fmt::Debug for TorrentSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TorrentSecrets")
            .field(
                "torznab_api_key",
                &self.torznab_api_key.as_ref().map(|_| "<set>"),
            )
            .field(
                "admin_password",
                &self.admin_password.as_ref().map(|_| "<set>"),
            )
            .finish()
    }
}

impl TorrentSecrets {
    /// Reads both secrets. [`SecretStore::get`] registers each value it returns with the log
    /// redactor, so neither can reach a log line unmasked.
    ///
    /// # Errors
    /// Fails when the credential store cannot be reached.
    pub fn read(store: &SecretStore) -> Result<Self, SecretError> {
        Ok(Self {
            torznab_api_key: store
                .get(TORZNAB_KEY_ACCOUNT)?
                .map(|secret| secret.expose().to_owned()),
            admin_password: store
                .get(ADMIN_PASSWORD_ACCOUNT)?
                .map(|secret| secret.expose().to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn values(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn every_language_set_anywhere_is_a_tracker_language_by_default() {
        let all = values(&[
            ("metadata.language", json!("de-DE")),
            ("metadata.nfo-language", json!("FR")),
            // A list with three-letter codes and "all", as a hand-edited file may hold.
            (
                "metadata.fallback-language",
                json!(["en-US", "tur", "pt-BR", "*"]),
            ),
        ]);
        assert_eq!(
            default_tracker_languages(&all),
            ["en", "de", "fr", "pt", "tr"]
        );
        assert_eq!(default_tracker_languages(&values(&[])), ["en"]);
        let set = tracker_languages(&all);
        assert_eq!(set.len(), 5);
        assert!(set.contains("fr") && set.contains("tr"));
    }

    #[test]
    fn tracker_languages_default_to_english_and_the_users_languages() {
        let set = |codes: &[&str]| {
            codes
                .iter()
                .map(|c| (*c).to_owned())
                .collect::<std::collections::HashSet<_>>()
        };
        assert_eq!(tracker_languages(&values(&[])), set(&["en"]));
        assert_eq!(
            tracker_languages(&values(&[
                ("metadata.language", json!("pt-BR")),
                ("metadata.nfo-language", json!("en, tr;de")),
            ])),
            set(&["en", "pt", "tr", "de"])
        );
        assert_eq!(
            tracker_languages(&values(&[(
                TRACKER_LANGUAGES_KEY,
                json!(["ru", "EN", "x"])
            )])),
            set(&["ru", "en"]),
            "the chosen ones, cleaned"
        );
        assert_eq!(
            tracker_languages(&values(&[(TRACKER_LANGUAGES_KEY, json!([]))])),
            set(&["en"]),
            "an empty choice falls back"
        );
    }

    #[test]
    fn a_fresh_install_has_every_source_on_and_the_documented_addresses() {
        let settings = TorrentSettings::default();
        for id in SOURCE_IDS {
            assert!(settings.source_on(id), "{id} is on by default");
        }
        assert_eq!(
            settings.base_url("torrentio"),
            Some("https://torrentio.strem.fun")
        );
        assert_eq!(
            settings.base_url("yts"),
            Some("https://movies-api.accel.li")
        );
        assert_eq!(settings.base_url("eztv"), Some("https://eztvx.to"));
        assert_eq!(settings.base_url("apibay"), Some("https://apibay.org"));
        assert_eq!(settings.base_url("knaben"), Some("https://api.knaben.org"));
        assert_eq!(settings.base_url("nyaa"), Some("https://nyaa.si"));
        assert!(settings.indexer_service_enabled);
        assert!(!settings.notice_accepted);
        assert_eq!(settings.solver_url, "");
        assert_eq!(settings.torznab_url, "");
    }

    #[test]
    fn an_unknown_source_is_off_and_has_no_address() {
        let settings = TorrentSettings::default();
        assert!(!settings.source_on("indexer:foo"));
        assert_eq!(settings.base_url("nope"), None);
    }

    #[test]
    fn a_switched_off_source_and_a_custom_address_are_read() {
        let settings = TorrentSettings::from_values(&values(&[
            ("torrents.source.yts", json!(false)),
            ("torrents.url.eztv", json!("https://eztv.example/")),
            ("torrents.indexer-service.enabled", json!(false)),
            ("torrents.notice-accepted", json!(true)),
            (
                "torrents.torznab.url",
                json!(" http://192.168.1.5:9117/api "),
            ),
        ]));
        assert!(!settings.source_on("yts"));
        assert!(settings.source_on("torrentio"));
        assert_eq!(settings.base_url("eztv"), Some("https://eztv.example"));
        assert!(!settings.indexer_service_enabled);
        assert!(settings.notice_accepted);
        assert_eq!(settings.torznab_url, "http://192.168.1.5:9117/api");
    }

    #[test]
    fn unusable_values_fall_back_to_the_defaults() {
        let settings = TorrentSettings::from_values(&values(&[
            ("torrents.source.yts", json!("yes")),
            ("torrents.url.knaben", json!("ftp://x")),
            ("torrents.url.nyaa", json!("not a url")),
            ("torrents.url.apibay", json!(7)),
            (
                "torrents.indexer-service.solver-url",
                json!("javascript:alert(1)"),
            ),
        ]));
        assert!(settings.source_on("yts"));
        assert_eq!(settings.base_url("knaben"), Some("https://api.knaben.org"));
        assert_eq!(settings.base_url("nyaa"), Some("https://nyaa.si"));
        assert_eq!(settings.base_url("apibay"), Some("https://apibay.org"));
        assert_eq!(settings.solver_url, "");
    }

    #[test]
    fn base_urls_are_cleaned_or_refused() {
        assert_eq!(clean_base_url("https://a.b/"), Some("https://a.b".into()));
        assert_eq!(
            clean_base_url("HTTPS://A.b/x/"),
            Some("https://A.b/x".into())
        );
        assert_eq!(
            clean_base_url("http://127.0.0.1:8191"),
            Some("http://127.0.0.1:8191".into())
        );
        for bad in [
            "",
            "   ",
            "a.b",
            "ftp://a.b",
            "https://",
            "https:///x",
            "https://user:pw@a.b",
            "https://a.b/?x=1",
            "https://a.b/#frag",
            "https://a b",
        ] {
            assert_eq!(clean_base_url(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn read_follows_the_settings_service() {
        let service = SettingsService::in_memory("test");
        assert!(TorrentSettings::read(&service).source_on("nyaa"));
        service
            .set("torrents.source.nyaa", json!(false))
            .expect("saved");
        assert!(!TorrentSettings::read(&service).source_on("nyaa"));
    }

    #[test]
    fn every_key_is_a_valid_setting_key_and_listed_once() {
        let keys = all_keys();
        let unique: std::collections::BTreeSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
        assert_eq!(keys.len(), 4 + 2 * SOURCE_IDS.len());
        for key in &keys {
            assert!(crate::app_settings::is_valid_key(key), "{key}");
            assert!(is_torrent_key(key));
        }
    }

    #[test]
    fn the_secrets_are_read_from_the_store_and_never_shown_by_debug() {
        let store = SecretStore::in_memory();
        assert_eq!(
            TorrentSecrets::read(&store).expect("read").torznab_api_key,
            None
        );
        store
            .set(TORZNAB_KEY_ACCOUNT, "key-1234567890")
            .expect("set");
        store
            .set(ADMIN_PASSWORD_ACCOUNT, "pw-abcdefghijkl")
            .expect("set");
        let secrets = TorrentSecrets::read(&store).expect("read");
        assert_eq!(secrets.torznab_api_key.as_deref(), Some("key-1234567890"));
        assert_eq!(secrets.admin_password.as_deref(), Some("pw-abcdefghijkl"));
        let shown = format!("{secrets:?}");
        assert!(!shown.contains("key-1234567890") && !shown.contains("pw-abcdefghijkl"));
    }

    #[test]
    fn reading_the_secrets_registers_them_for_log_redaction() {
        let store = SecretStore::in_memory();
        store
            .set(TORZNAB_KEY_ACCOUNT, "torznab-secret-value-1")
            .expect("set");
        let _ = TorrentSecrets::read(&store).expect("read");
        let masked = crate::redact::redact("GET /feed/torznab-secret-value-1/x");
        assert!(!masked.contains("torznab-secret-value-1"), "{masked}");
    }
}

//! Provisioning: choosing which of the service's public indexers to use.
//!
//! The service knows hundreds of indexers. Provisioning picks the public ones that carry movies or
//! TV (or index everything without saying what, as the general search engines do) and are not
//! mostly adult, sets each one up, tests it, and deletes the ones that fail so a
//! search never waits for a broken site. What it found is written to the store row by row, as each
//! indexer finishes, so a search can use the first indexers that passed while the others are still
//! being tested. A person's "off" for an indexer is kept: it is skipped and removed from the
//! service. Only sites in the person's tracker languages are set up, and a site set up by an
//! earlier check that no longer qualifies (another language, adult, a site searched directly) is
//! removed from the service.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::admin::{AdminClient, AdminError, IndexerInfo};
use super::config::generate_api_key;
use crate::cancel::CancelToken;

/// Indexers set up at the same time.
pub const ENABLE_PARALLEL: usize = 6;
/// Indexers tested at the same time.
pub const TEST_PARALLEL: usize = 8;
/// How long one connection test may take.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(60);

/// How an indexer stands after a check. The words match the `torrent_indexers` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexerState {
    Working,
    NeedsSolver,
    Unreachable,
    Error,
    Off,
}

impl IndexerState {
    /// The text stored in the table.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::NeedsSolver => "needs_solver",
            Self::Unreachable => "unreachable",
            Self::Error => "error",
            Self::Off => "off",
        }
    }

    /// The state stored as `text`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::Working,
            Self::NeedsSolver,
            Self::Unreachable,
            Self::Error,
            Self::Off,
        ]
        .into_iter()
        .find(|state| state.as_str() == text)
    }
}

/// One indexer and how its last check went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexerRow {
    pub id: String,
    pub name: String,
    pub state: IndexerState,
    /// Plain words for a state that is not `working`.
    pub reason: Option<String>,
    /// When it was checked, epoch milliseconds.
    pub checked_at: i64,
    /// Switched off by the person.
    pub user_off: bool,
}

/// Where provisioning writes what it finds.
pub trait IndexerStore: Send + Sync {
    /// Writes one row, replacing the row with the same id.
    ///
    /// # Errors
    /// A message when the row could not be saved; provisioning goes on without it.
    fn upsert(&self, row: &IndexerRow) -> Result<(), String>;
}

/// Where the admin password is kept (the operating system's credential store in the app).
pub trait PasswordStore: Send + Sync {
    /// The saved password, if there is one.
    fn get(&self) -> Option<String>;
    /// Saves the password.
    ///
    /// # Errors
    /// A message when it could not be saved.
    fn set(&self, password: &str) -> Result<(), String>;
}

/// A site that qualifies but is not set up because it is in none of the tracker languages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherLanguageIndexer {
    pub id: String,
    pub name: String,
    /// The language part of the site's language (`ru` for `ru-RU`).
    pub language: String,
}

/// What a provisioning run found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProvisionReport {
    pub rows: Vec<IndexerRow>,
    /// Indexers an earlier check had set up that no longer qualify, removed from the service.
    pub removed: Vec<String>,
    /// Sites left out only for their language, by name: what turning a language on would add.
    pub other_languages: Vec<OtherLanguageIndexer>,
}

impl ProvisionReport {
    /// How many indexers ended in `state`.
    #[must_use]
    pub fn count(&self, state: IndexerState) -> usize {
        self.rows.iter().filter(|row| row.state == state).count()
    }

    /// How many sites each other language would add (`ru` -> 7), most first, then by code.
    #[must_use]
    pub fn other_language_counts(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for site in &self.other_languages {
            *counts.entry(site.language.as_str()).or_default() += 1;
        }
        let mut out: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(code, count)| (code.to_owned(), count))
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }
}

/// Limits of a run; the defaults are the app's, tests shorten them.
#[derive(Debug, Clone, Copy)]
pub struct ProvisionOptions {
    pub enable_parallel: usize,
    pub test_parallel: usize,
    pub test_timeout: Duration,
}

impl Default for ProvisionOptions {
    fn default() -> Self {
        Self {
            enable_parallel: ENABLE_PARALLEL,
            test_parallel: TEST_PARALLEL,
            test_timeout: TEST_TIMEOUT,
        }
    }
}

/// Why provisioning could not run.
#[derive(Debug, thiserror::Error)]
pub enum ProvisionError {
    #[error(transparent)]
    Admin(#[from] AdminError),
    #[error("The indexer service has an admin password that CineVault does not know.")]
    PasswordUnknown,
    #[error("The admin password could not be saved: {0}")]
    PasswordNotSaved(String),
    #[error("Checking the torrent sources was cancelled.")]
    Cancelled,
}

/// Gets a session with the service. When no admin password is set yet (a fresh service, or one that
/// lost its data), sets one at once, saving it before anything else is done, then signs in with it.
///
/// # Errors
/// [`ProvisionError::PasswordUnknown`] when the service wants a password and none is saved;
/// [`ProvisionError::PasswordNotSaved`] when a new one cannot be kept (then it is not set).
pub async fn open_session(
    admin: &AdminClient,
    passwords: &dyn PasswordStore,
) -> Result<(), ProvisionError> {
    match admin.server_config().await {
        // No sign-in asked for: nothing protects the service yet.
        Ok(_) => {}
        Err(AdminError::Unauthorized) => {
            // The real service answers every admin call with a redirect to its sign-in page until
            // a session exists, even with no password set: it hands that session to anyone who
            // asks like a browser. If it does, no password is set yet.
            if admin.begin_session().await.is_err() {
                let saved = passwords.get().ok_or(ProvisionError::PasswordUnknown)?;
                return admin
                    .login(&saved)
                    .await
                    .map_err(|_| ProvisionError::PasswordUnknown);
            }
        }
        Err(other) => return Err(other.into()),
    }
    // Keep a password first, so it can never be set and lost. A password we already hold is set
    // again (the service lost its data); otherwise a random one is made.
    let password = match passwords.get() {
        Some(saved) => saved,
        None => {
            let fresh = generate_api_key()
                .map_err(|error| ProvisionError::PasswordNotSaved(error.to_string()))?;
            passwords
                .set(&fresh)
                .map_err(ProvisionError::PasswordNotSaved)?;
            fresh
        }
    };
    admin.set_admin_password(&password).await?;
    admin.login(&password).await?;
    Ok(())
}

/// Words that mark a site as adult in what the service says about it. Some adult-only sites file
/// their pages under the TV or anime categories, so the categories alone do not tell.
const ADULT_WORDS: [&str; 6] = ["hentai", "porn", "xxx", "erotic", "doujin", "adult"];

/// Sites CineVault already searches itself, with their own API: running them through the service
/// as well would only list every release twice and show two chips with the same name.
const SEARCHED_DIRECTLY: [&str; 5] = ["thepiratebay", "knaben", "yts", "eztv", "nyaasi"];

/// The first of the sites' own category numbers; below it are the standard Torznab categories.
const CUSTOM_CATEGORIES: u32 = 100_000;

/// Whether a site files everything under "Other" (8000-8999), as the general search engines and
/// DHT crawlers do: they hold movies and TV without saying so.
fn is_general(info: &IndexerInfo) -> bool {
    let mut standard = info
        .categories
        .iter()
        .filter(|id| **id < CUSTOM_CATEGORIES)
        .peekable();
    standard.peek().is_some() && standard.all(|id| (8000..9000).contains(id))
}

/// Whether an indexer is worth setting up: a public one that returns movies or TV (or indexes
/// everything under "Other"), is not mostly adult and is not a site the app searches directly.
#[must_use]
pub fn is_candidate(info: &IndexerInfo) -> bool {
    if info.kind != "public" || SEARCHED_DIRECTLY.contains(&info.id.as_str()) {
        return false;
    }
    let words = format!("{} {}", info.name, info.description).to_lowercase();
    if ADULT_WORDS.iter().any(|word| words.contains(word)) {
        return false;
    }
    let wanted = is_general(info)
        || info
            .categories
            .iter()
            .any(|id| (2000..3000).contains(id) || (5000..6000).contains(id));
    let adult = info
        .categories
        .iter()
        .filter(|id| (6000..7000).contains(*id))
        .count();
    wanted && adult * 2 < info.categories.len()
}

/// The language part of a language tag: `en` for `en-US`, lower case.
#[must_use]
pub fn language_of(tag: &str) -> String {
    tag.trim()
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Whether a site is in one of the tracker languages (`en`, `ru`, ...). A site that names no
/// language passes, and so does every site when no language is chosen.
#[must_use]
pub fn speaks(info: &IndexerInfo, languages: &HashSet<String>) -> bool {
    let own = language_of(&info.language);
    languages.is_empty() || own.is_empty() || languages.contains(&own)
}

/// The state and words for a failure message the service gave.
#[must_use]
pub fn classify(message: &str) -> (IndexerState, String) {
    let lower = message.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|word| lower.contains(word));
    if any(&[
        "challenge detected",
        "cloudflare",
        "flaresolverr",
        "captcha",
        "ddos-guard",
    ]) {
        return (
            IndexerState::NeedsSolver,
            "A browser challenge blocks it. A challenge solver address would let it through."
                .to_owned(),
        );
    }
    if any(&[
        "timed out",
        "timeout",
        "task was canceled",
        "request was canceled",
        "unable to connect",
        "no such host",
        "name or service not known",
        "nodename nor servname",
        "could not be resolved",
        "name does not resolve",
        "connection refused",
        "actively refused",
        "connection reset",
        "ssl connection",
        "ssl/tls",
        "tls handshake",
        "secure channel",
        "certificate",
    ]) {
        return (
            IndexerState::Unreachable,
            "The site did not answer.".to_owned(),
        );
    }
    let reason: String = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let reason: String = reason.chars().take(160).collect();
    (
        IndexerState::Error,
        if reason.is_empty() {
            "The test failed.".to_owned()
        } else {
            reason
        },
    )
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Sets up, tests and keeps or deletes the service's public indexers. `user_off` holds the ids a
/// person switched off; `languages` the tracker languages (`en`, `ru`), only sites in one of them
/// are set up. Sites set up earlier that no longer qualify are deleted from the service and listed
/// in [`ProvisionReport::removed`]. Each row is written to `store` and passed to `on_row` as soon
/// as that indexer is done.
///
/// # Errors
/// [`ProvisionError`] when the list of indexers cannot be read or the run is cancelled; a failing
/// indexer is not an error, it is a row.
pub async fn provision(
    admin: &AdminClient,
    store: Arc<dyn IndexerStore>,
    user_off: &HashSet<String>,
    languages: &HashSet<String>,
    options: ProvisionOptions,
    cancel: &CancelToken,
    on_row: &(dyn Fn(&IndexerRow) + Send + Sync),
) -> Result<ProvisionReport, ProvisionError> {
    let all = admin.list_indexers().await?;
    let mut report = ProvisionReport::default();
    let record = |row: IndexerRow, report: &mut ProvisionReport| {
        if let Err(error) = store.upsert(&row) {
            tracing::warn!(id = %row.id, %error, "an indexer row could not be saved");
        }
        on_row(&row);
        report.rows.push(row);
    };

    // A person's off: not set up, and removed from the service if it was.
    for info in all.iter().filter(|info| user_off.contains(&info.id)) {
        if info.configured {
            let _ = admin.delete(&info.id).await;
        }
        record(
            IndexerRow {
                id: info.id.clone(),
                name: info.name.clone(),
                state: IndexerState::Off,
                reason: None,
                checked_at: now_ms(),
                user_off: true,
            },
            &mut report,
        );
    }

    // Set up by an earlier check but no longer wanted: another language, adult, searched directly.
    let wanted = |info: &IndexerInfo| is_candidate(info) && speaks(info, languages);
    report.other_languages = all
        .iter()
        .filter(|info| {
            is_candidate(info) && !speaks(info, languages) && !user_off.contains(&info.id)
        })
        .map(|info| OtherLanguageIndexer {
            id: info.id.clone(),
            name: info.name.clone(),
            language: language_of(&info.language),
        })
        .collect();
    report
        .other_languages
        .sort_by_key(|site| site.name.to_lowercase());
    for info in all
        .iter()
        .filter(|info| info.configured && !user_off.contains(&info.id) && !wanted(info))
    {
        if let Err(error) = admin.delete(&info.id).await {
            tracing::warn!(id = %info.id, error = %error.text(), "a stale indexer was not removed");
        }
        report.removed.push(info.id.clone());
    }

    let enable = Arc::new(Semaphore::new(options.enable_parallel.max(1)));
    let test = Arc::new(Semaphore::new(options.test_parallel.max(1)));
    let mut tasks = JoinSet::new();
    for info in all
        .into_iter()
        .filter(|info| wanted(info) && !user_off.contains(&info.id))
    {
        let admin = admin.clone();
        let (enable, test) = (Arc::clone(&enable), Arc::clone(&test));
        tasks.spawn(
            async move { check_one(&admin, info, &enable, &test, options.test_timeout).await },
        );
    }
    loop {
        let finished = tokio::select! {
            () = cancel.cancelled() => {
                tasks.abort_all();
                return Err(ProvisionError::Cancelled);
            }
            finished = tasks.join_next() => finished,
        };
        match finished {
            None => break,
            Some(Ok(row)) => record(row, &mut report),
            Some(Err(error)) => tracing::warn!(%error, "an indexer check ended unexpectedly"),
        }
    }
    report.rows.sort_by_key(|row| row.name.to_lowercase());
    Ok(report)
}

/// Sets one indexer up, tests it, and deletes it when it fails.
async fn check_one(
    admin: &AdminClient,
    info: IndexerInfo,
    enable: &Semaphore,
    test: &Semaphore,
    test_timeout: Duration,
) -> IndexerRow {
    let finish = |state, reason: Option<String>| IndexerRow {
        id: info.id.clone(),
        name: info.name.clone(),
        state,
        reason,
        checked_at: now_ms(),
        user_off: false,
    };
    let failed = |message: &str| {
        let (state, reason) = classify(message);
        finish(state, Some(reason))
    };

    let enabled = {
        let _permit = enable.acquire().await;
        match admin.get_config(&info.id).await {
            Ok(config) => admin.post_config(&info.id, &config).await,
            Err(error) => Err(error),
        }
    };
    if let Err(error) = enabled {
        let _ = admin.delete(&info.id).await;
        return failed(&error.text());
    }

    let tested = {
        let _permit = test.acquire().await;
        tokio::time::timeout(test_timeout, admin.test(&info.id)).await
    };
    match tested {
        Ok(Ok(())) => finish(IndexerState::Working, None),
        Ok(Err(error)) => {
            let _ = admin.delete(&info.id).await;
            failed(&error.text())
        }
        Err(_) => {
            let _ = admin.delete(&info.id).await;
            finish(
                IndexerState::Unreachable,
                Some(format!(
                    "The site did not answer within {} seconds.",
                    test_timeout.as_secs()
                )),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(kind: &str, categories: &[u32]) -> IndexerInfo {
        IndexerInfo {
            id: "x".into(),
            name: "X".into(),
            description: String::new(),
            kind: kind.into(),
            configured: false,
            categories: categories.to_vec(),
            language: String::new(),
        }
    }

    #[test]
    fn only_sites_in_a_tracker_language_are_wanted() {
        let set = |codes: &[&str]| {
            codes
                .iter()
                .map(|c| (*c).to_owned())
                .collect::<HashSet<_>>()
        };
        let site = |language: &str| IndexerInfo {
            language: language.into(),
            ..info("public", &[2000])
        };
        assert!(speaks(&site("en-US"), &set(&["en"])));
        assert!(speaks(&site("EN"), &set(&["en", "de"])));
        assert!(!speaks(&site("ru-RU"), &set(&["en"])));
        assert!(!speaks(&site("zh-CN"), &set(&["en", "tr"])));
        assert!(speaks(&site("ru-RU"), &set(&["en", "ru"])));
        assert!(speaks(&site(""), &set(&["en"])), "no language named");
        assert!(speaks(&site("bg-BG"), &set(&[])), "nothing chosen");
        assert_eq!(language_of("pt_BR"), "pt");
    }

    #[test]
    fn only_public_movie_or_tv_indexers_that_are_not_mostly_adult_are_used() {
        assert!(is_candidate(&info("public", &[2000, 5000])));
        assert!(is_candidate(&info("public", &[2040])));
        assert!(is_candidate(&info("public", &[5070, 8000])));
        // Adult in under half of the categories is still fine; half or more is not.
        assert!(is_candidate(&info("public", &[2000, 5000, 6000])));
        assert!(!is_candidate(&info("public", &[2000, 6000])));
        assert!(!is_candidate(&info("public", &[2000, 6000, 6010, 6020])));
        assert!(!is_candidate(&info("private", &[2000])));
        assert!(!is_candidate(&info("semi-public", &[2000])));
        assert!(!is_candidate(&info("public", &[3000, 4000, 7000])));
        assert!(!is_candidate(&info("public", &[])));
    }

    #[test]
    fn general_search_engines_that_file_everything_under_other_are_used() {
        // As the service lists them (2026-10-26): a meta-search engine and a DHT crawler with
        // "Other" and the site's own categories only; a music tracker and a games tracker are not.
        assert!(is_candidate(&info("public", &[8000])));
        assert!(is_candidate(&info("public", &[8000, 127_246])));
        assert!(is_candidate(&info("public", &[8000, 8010])));
        assert!(!is_candidate(&info("public", &[3000, 3040])));
        assert!(!is_candidate(&info("public", &[4000, 4050, 100_001])));
        assert!(
            !is_candidate(&info("public", &[127_246])),
            "own categories only"
        );
        assert!(!is_candidate(&info("public", &[6000, 8000])), "half adult");
        assert!(!is_candidate(&info("private", &[8000])));
        let mut adult = info("public", &[8000]);
        adult.description = "A Public DHT crawler for porn".into();
        assert!(!is_candidate(&adult));
    }

    #[test]
    fn other_languages_are_counted_most_first() {
        let site = |id: &str, language: &str| OtherLanguageIndexer {
            id: id.into(),
            name: id.into(),
            language: language.into(),
        };
        let report = ProvisionReport {
            other_languages: vec![
                site("rutor", "ru"),
                site("torrent9", "fr"),
                site("rutracker", "ru"),
                site("dmhy", "zh"),
                site("mikan", "zh"),
                site("megapeer", "ru"),
            ],
            ..ProvisionReport::default()
        };
        assert_eq!(
            report.other_language_counts(),
            vec![("ru".into(), 3), ("zh".into(), 2), ("fr".into(), 1)]
        );
    }

    #[test]
    fn sites_the_app_searches_directly_are_not_set_up_a_second_time() {
        for id in ["thepiratebay", "knaben", "yts", "eztv", "nyaasi"] {
            let mut direct = info("public", &[2000, 5000]);
            direct.id = id.into();
            assert!(!is_candidate(&direct), "{id}");
        }
        assert!(is_candidate(&info("public", &[2000, 5000])));
    }

    #[test]
    fn a_site_that_calls_itself_adult_is_left_out_whatever_its_categories() {
        let mut hentai = info("public", &[5000, 5070]);
        hentai.name = "E-Hentai".into();
        hentai.description = "E-Hentai is a Public site for Hentai doujinshi, manga.".into();
        assert!(!is_candidate(&hentai));
        let mut porn = info("public", &[5000]);
        porn.description = "A Public tracker for 3X, mostly porn".into();
        assert!(!is_candidate(&porn));
        let mut plain = info("public", &[5000]);
        plain.description = "A Public site for MOVIES / TV / GENERAL".into();
        assert!(is_candidate(&plain));
    }

    #[test]
    fn failures_are_told_apart_by_what_the_service_said() {
        let challenge = classify("Exception (1337x): Challenge detected (CloudFlare)");
        assert_eq!(challenge.0, IndexerState::NeedsSolver);
        for text in [
            "The operation has timed out",
            "A task was canceled.",
            "Unable to connect to the remote server",
            "No such host is known",
            "The SSL connection could not be established",
            "Connection refused",
        ] {
            assert_eq!(classify(text).0, IndexerState::Unreachable, "{text}");
        }
        let other = classify("  Parse error:\n unexpected   token ");
        assert_eq!(
            other,
            (
                IndexerState::Error,
                "Parse error: unexpected token".to_owned()
            )
        );
        assert_eq!(classify("").1, "The test failed.");
        assert_eq!(classify(&"x".repeat(500)).1.len(), 160);
    }

    #[test]
    fn states_round_trip_through_their_text() {
        for state in [
            IndexerState::Working,
            IndexerState::NeedsSolver,
            IndexerState::Unreachable,
            IndexerState::Error,
            IndexerState::Off,
        ] {
            assert_eq!(IndexerState::parse(state.as_str()), Some(state));
        }
    }
}

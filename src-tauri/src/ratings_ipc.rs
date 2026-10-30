//! Typed commands for ratings: fetching the IMDb Top 250 on request, the library-wide "Fetch
//! missing ratings" sweep, and opening a title's page on a rating source's site.
//!
//! The rules live in `cv_app::top250` and `cv_db::repo::top250`; this module opens the shared
//! library, builds the IMDb source and turns the outcome into plain sentences for the UI.

use cv_app::data_location::DataLocation;
use cv_app::top250::{Top250Error, refresh_top250};
use cv_db::Database;
use cv_db::repo::Top250Report;
use cv_providers::imdb::ImdbProvider;
use cv_providers::model::RequestOptions;
use cv_providers::{HttpClient, ProviderError, Top250List, Top250Source};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri::ipc::Channel;
use tauri_plugin_opener::OpenerExt;

use std::sync::Arc;

use cv_app::rating_status::{SweepScope, source_for_key};
use cv_app::ratings_sweep::{SweepSummary, submit_sweep};
use cv_providers::RatingSource;

use crate::boot::Boot;
use crate::sources_ipc::LibraryState;

/// Which chart to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Top250ListDto {
    Movies,
    Shows,
}

impl From<Top250ListDto> for Top250List {
    fn from(list: Top250ListDto) -> Self {
        match list {
            Top250ListDto::Movies => Self::Movies,
            Top250ListDto::Shows => Self::Shows,
        }
    }
}

/// What storing a chart did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Top250ReportDto {
    /// Titles of the library that are on the chart now.
    pub ranked: u32,
    /// Of those, how many got a new position.
    pub changed: u32,
    /// Titles that were on the chart before and are not any more.
    pub cleared: u32,
    /// Titles left alone because they are locked.
    pub locked: u32,
    /// Chart entries the library does not have.
    pub not_in_library: u32,
}

fn small(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl From<Top250Report> for Top250ReportDto {
    fn from(report: Top250Report) -> Self {
        Self {
            ranked: small(report.ranked),
            changed: small(report.changed),
            cleared: small(report.cleared),
            locked: small(report.locked),
            not_in_library: small(report.not_in_library),
        }
    }
}

/// Why a ratings command did not finish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RatingsCommandError {
    /// Stable code: `storage`, `format_changed` or `request`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<Top250Error> for RatingsCommandError {
    fn from(error: Top250Error) -> Self {
        match error {
            Top250Error::Source(ProviderError::FormatChanged(_)) => Self {
                code: "format_changed".to_owned(),
                message: "IMDb changed its Top 250 page, so CineVault cannot read it. \
                          No positions were changed."
                    .to_owned(),
            },
            Top250Error::Source(other) => Self {
                code: "request".to_owned(),
                message: format!("{}. No positions were changed.", source_reason(&other)),
            },
            Top250Error::Storage(other) => Self {
                code: "storage".to_owned(),
                message: format!("The library could not be updated: {other}"),
            },
        }
    }
}

/// Why IMDb gave no chart, once: the Activity card's title already says the Top 250 was not
/// updated, so the reason neither repeats it nor names IMDb twice, and a refusal (HTTP 403, a
/// human check) is not called a silence.
fn source_reason(error: &ProviderError) -> String {
    match error {
        ProviderError::Request { message, .. } => {
            format!(
                "The request to IMDb failed: {}",
                message.trim_end_matches('.')
            )
        }
        other => other.to_string().trim_end_matches('.').to_owned(),
    }
}

/// Fetches a chart from `source` and stores the positions. The command's logic without the Tauri
/// plumbing.
async fn run(
    db: &Database,
    source: &dyn Top250Source,
    list: Top250ListDto,
) -> Result<Top250ReportDto, RatingsCommandError> {
    let report = refresh_top250(db, source, list.into(), &RequestOptions::default()).await?;
    Ok(report.into())
}

fn imdb_source() -> Result<ImdbProvider, RatingsCommandError> {
    let cache_dir = DataLocation::resolve_for_process().map(|l| l.cache_dir().join("providers"));
    let mut builder = HttpClient::builder();
    if let Some(dir) = cache_dir {
        builder = builder.cache_dir(dir);
    }
    let http = builder.build().map_err(|error| RatingsCommandError {
        code: "request".to_owned(),
        message: format!("CineVault could not start its network client: {error}"),
    })?;
    Ok(ImdbProvider::new(http))
}

/// Fetches the IMDb Top 250 chart and stores each title's position.
#[tauri::command]
#[specta::specta]
pub async fn fetch_imdb_top250(
    library: State<'_, LibraryState>,
    list: Top250ListDto,
) -> Result<Top250ReportDto, RatingsCommandError> {
    let db = library.database().map_err(|message| RatingsCommandError {
        code: "storage".to_owned(),
        message,
    })?;
    let source = imdb_source()?;
    run(&db, &source, list).await
}

/// Which titles the ratings sweep looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RatingSweepScopeDto {
    /// Scraped titles that lack the ratings of a source that can be asked now.
    Missing,
    /// Every scraped title: all ratings are fetched again.
    All,
}

impl From<RatingSweepScopeDto> for SweepScope {
    fn from(scope: RatingSweepScopeDto) -> Self {
        match scope {
            RatingSweepScopeDto::Missing => Self::Missing,
            RatingSweepScopeDto::All => Self::All,
        }
    }
}

/// The sweep was queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct RatingSweepStartedDto {
    /// The job the Activity drawer shows for it.
    pub job_id: u32,
    pub titles: u32,
}

/// What a finished (or cancelled, or limited) sweep did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RatingSweepSummaryDto {
    pub job_id: u32,
    /// Titles whose ratings were stored or changed.
    pub updated: u32,
    pub unchanged: u32,
    pub skipped: u32,
    pub failed: u32,
    /// Titles left for later because a source's request limit was reached.
    pub left: u32,
    /// The source whose limit stopped the sweep (`OMDb`, `MDBList`), if one did.
    pub limited_by: Option<String>,
    pub cancelled: bool,
    /// The whole summary as one plain sentence.
    pub sentence: String,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl RatingSweepSummaryDto {
    fn new(job_id: u32, summary: &SweepSummary) -> Self {
        Self {
            job_id,
            updated: count(summary.updated),
            unchanged: count(summary.unchanged),
            skipped: count(summary.skipped),
            failed: count(summary.failed),
            left: count(summary.left),
            limited_by: summary.limited_by.clone(),
            cancelled: summary.cancelled,
            sentence: summary.sentence(),
        }
    }
}

fn starting() -> RatingsCommandError {
    RatingsCommandError {
        code: "storage".to_owned(),
        message: "CineVault is still starting. Try again in a moment.".to_owned(),
    }
}

fn unreadable(error: impl std::fmt::Display) -> RatingsCommandError {
    RatingsCommandError {
        code: "storage".to_owned(),
        message: format!("The library could not be read: {error}"),
    }
}

/// The rating source a provider key names (`omdb`, `mdblist`); `None` for no key, or for a key
/// that unlocks no rating source.
fn only_source(provider: Option<&str>) -> Option<RatingSource> {
    provider.and_then(source_for_key)
}

/// How many titles the ratings sweep of `scope` would fetch. With `provider` (`omdb` or
/// `mdblist`) only the titles that lack that source's ratings count: what a new key unlocks.
#[tauri::command]
#[specta::specta]
pub async fn count_rating_sweep(
    boot: State<'_, Boot>,
    scope: RatingSweepScopeDto,
    provider: Option<String>,
) -> Result<u32, RatingsCommandError> {
    let services = Arc::clone(boot.services().ok_or_else(starting)?);
    let only = only_source(provider.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        services
            .rating_status()
            .sweep_titles(scope.into(), only)
            .map(|ids| count(ids.len()))
            .map_err(unreadable)
    })
    .await
    .map_err(unreadable)?
}

/// Fetches the ratings of the sweep's titles as one job in Activity, one title at a time,
/// stopping calmly at a source's request limit. `on_done` receives the summary once, when the job
/// is over. Answers `not_found` when there is nothing to fetch.
#[tauri::command]
#[specta::specta]
pub async fn start_rating_sweep(
    boot: State<'_, Boot>,
    scope: RatingSweepScopeDto,
    provider: Option<String>,
    on_done: Channel<RatingSweepSummaryDto>,
) -> Result<RatingSweepStartedDto, RatingsCommandError> {
    let services = Arc::clone(boot.services().ok_or_else(starting)?);
    let only = only_source(provider.as_deref());
    let on_done = crate::ipc_sink::sink(on_done);
    let queued = tauri::async_runtime::spawn_blocking(move || {
        submit_sweep(
            services.jobs(),
            services.actions(),
            services.rating_status(),
            services.db(),
            scope.into(),
            only,
        )
        .map_err(unreadable)
    })
    .await
    .map_err(unreadable)??;
    let Some((job, work, titles)) = queued else {
        return Err(RatingsCommandError {
            code: "not_found".to_owned(),
            message: "Nothing to fetch: every scraped title has the ratings its sources can give."
                .to_owned(),
        });
    };
    let job_id = u32::try_from(job.id().0).unwrap_or(u32::MAX);
    tauri::async_runtime::spawn(async move {
        job.wait().await;
        on_done(RatingSweepSummaryDto::new(job_id, &work.summary()));
    });
    Ok(RatingSweepStartedDto {
        job_id,
        titles: count(titles),
    })
}

/// Opens `address` with `open` when it is a web page address (`http` or `https` with a host);
/// anything else (a file path, another scheme) is refused before it reaches the system.
fn open_web_page(
    address: &str,
    open: &dyn Fn(&str) -> Result<(), String>,
) -> Result<(), RatingsCommandError> {
    let Some(address) = cv_domain::web_address(address) else {
        return Err(RatingsCommandError {
            code: "invalid_address".to_owned(),
            message: "That is not a web page address.".to_owned(),
        });
    };
    open(&address).map_err(|message| {
        tracing::warn!(%message, "the system could not open a rating page");
        RatingsCommandError {
            code: "failed".to_owned(),
            message: "The page could not be opened.".to_owned(),
        }
    })
}

/// Opens the title's page on a rating source's site (a click on a rating of the ratings strip)
/// in the default browser. Only `http` and `https` addresses are opened.
#[tauri::command]
#[specta::specta]
pub async fn open_rating_page(
    app: tauri::AppHandle,
    address: String,
) -> Result<(), RatingsCommandError> {
    open_web_page(&address, &|url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use cv_db::repo::{get_item, insert_item};
    use cv_domain::{Item, ItemDetail, id_key};
    use cv_providers::Top250Entry;

    use super::*;

    struct Fake(Result<Vec<Top250Entry>, fn() -> ProviderError>);

    #[async_trait]
    impl Top250Source for Fake {
        async fn top250(
            &self,
            _list: Top250List,
            _options: &RequestOptions,
        ) -> Result<Vec<Top250Entry>, ProviderError> {
            self.0.clone().map_err(|make| make())
        }
    }

    fn library() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = Database::open(dir.path().join("library.db")).expect("open");
        (dir, db)
    }

    #[tokio::test]
    async fn the_report_counts_what_changed_and_the_position_is_stored() {
        let (_dir, db) = library();
        let mut item = Item::movie("Heat");
        item.core.ids.set(id_key::IMDB, "tt0113277");
        let id = db
            .write(move |tx| insert_item(tx, &item))
            .expect("insert movie");
        let chart = Fake(Ok(vec![
            Top250Entry {
                rank: 1,
                imdb_id: "tt0113277".into(),
                title: "Heat".into(),
            },
            Top250Entry {
                rank: 2,
                imdb_id: "tt0000009".into(),
                title: "Elsewhere".into(),
            },
        ]));

        let report = run(&db, &chart, Top250ListDto::Movies).await.expect("run");

        assert_eq!(
            report,
            Top250ReportDto {
                ranked: 1,
                changed: 1,
                cleared: 0,
                locked: 0,
                not_in_library: 1,
            }
        );
        let stored = db.read(|c| get_item(c, id)).unwrap().unwrap();
        let ItemDetail::Movie(movie) = stored.detail else {
            panic!("not a movie");
        };
        assert_eq!(movie.top250, 1);
    }

    #[tokio::test]
    async fn a_changed_page_is_explained_and_a_network_failure_is_a_request_error() {
        let (_dir, db) = library();
        let changed = Fake(Err(|| ProviderError::FormatChanged("IMDb".into())));
        let error = run(&db, &changed, Top250ListDto::Movies).await.unwrap_err();
        assert_eq!(error.code, "format_changed");
        assert!(error.message.contains("No positions were changed"));

        let down = Fake(Err(|| ProviderError::Request {
            provider: "IMDb".into(),
            message: "HTTP 503".into(),
        }));
        let error = run(&db, &down, Top250ListDto::Shows).await.unwrap_err();
        assert_eq!(error.code, "request");
        assert!(error.message.contains("503"), "{}", error.message);

        // reg1-21: a refusal is told once, in plain words, without naming IMDb twice.
        let refused = Fake(Err(|| ProviderError::Request {
            provider: "IMDb".into(),
            message: "the site asked for a human check (HTTP 403); try again later".into(),
        }));
        let error = run(&db, &refused, Top250ListDto::Movies).await.unwrap_err();
        assert_eq!(
            error.message,
            "The request to IMDb failed: the site asked for a human check (HTTP 403); try again \
             later. No positions were changed."
        );
        assert_eq!(error.message.matches("IMDb").count(), 1);
        assert!(!error.message.contains("did not answer"));
    }

    #[test]
    fn only_web_pages_reach_the_system() {
        let opened = std::cell::RefCell::new(Vec::<String>::new());
        let open = |url: &str| {
            opened.borrow_mut().push(url.to_owned());
            Ok(())
        };
        open_web_page(" https://www.imdb.com/title/tt0083658/ ", &open).expect("a web page");
        for bad in [
            "file:///C:/Windows/notepad.exe",
            r"C:\Windows\notepad.exe",
            "javascript:alert(1)",
            "mailto:a@b.c",
            "",
        ] {
            let error = open_web_page(bad, &open).unwrap_err();
            assert_eq!(error.code, "invalid_address", "{bad}");
        }
        assert_eq!(
            opened.into_inner(),
            ["https://www.imdb.com/title/tt0083658/"]
        );
        let failing = |_: &str| Err("no browser".to_owned());
        let error = open_web_page("https://letterboxd.com/", &failing).unwrap_err();
        assert_eq!(error.code, "failed");
    }
}

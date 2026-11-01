//! Typed commands for ratings: the library-wide "Fetch missing ratings" sweep, and opening a
//! title's page on a rating source's site.

use serde::{Deserialize, Serialize};
use specta::Type;

use std::sync::Arc;

use cv_app::rating_status::{SweepScope, source_for_key};
use cv_app::ratings_sweep::{SweepSummary, submit_sweep};
use cv_providers::RatingSource;

use cv_app::services::AppServices;

/// Why a ratings command did not finish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RatingsCommandError {
    /// Stable code: `storage`, `not_found`, `invalid_address` or `failed`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
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
pub async fn count_rating_sweep(
    services: Option<&Arc<AppServices>>,
    scope: RatingSweepScopeDto,
    provider: Option<String>,
) -> Result<u32, RatingsCommandError> {
    let services = Arc::clone(services.ok_or_else(starting)?);
    let only = only_source(provider.as_deref());
    cv_jobs::client::spawn_blocking(move || {
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
pub async fn start_rating_sweep(
    services: Option<&Arc<AppServices>>,
    scope: RatingSweepScopeDto,
    provider: Option<String>,
    on_done: crate::sink::Sink<RatingSweepSummaryDto>,
) -> Result<RatingSweepStartedDto, RatingsCommandError> {
    let services = Arc::clone(services.ok_or_else(starting)?);
    let only = only_source(provider.as_deref());
    let queued = cv_jobs::client::spawn_blocking(move || {
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
    tokio::spawn(async move {
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
pub fn open_web_page(
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

#[cfg(test)]
mod tests {
    use super::*;

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

//! The title lifecycle over IPC: the stage, flags and explanation lines every screen shows, and
//! the pipeline counts.
//!
//! The rules live in `cv_app::lifecycle`. The DTOs here travel on the library row, the movie,
//! show and episode detail and the Inspector's [`get_lifecycles`], so the stage meter reads the
//! same words everywhere. [`lifecycle_counts`] is the one source of the numbers on the sidebar,
//! Home, Needs review, Organize and Activity.

use std::sync::Arc;

use cv_app::lifecycle::{
    FlagKind, Lifecycle, LifecycleCounts, LifecycleService, Line, LineLink, LinePart, LineStatus,
    LineStep, Stage,
};
use cv_domain::{ItemId, ItemKind};
use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::detail_ipc::parse_id;
use crate::library_ipc::{LibraryCommandError, LibraryKindDto, blocking, storage};
use crate::sources_ipc::LibraryState;

/// The stage of a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum StageDto {
    New,
    Scraped,
    Renamed,
}

impl From<Stage> for StageDto {
    fn from(stage: Stage) -> Self {
        match stage {
            Stage::New => Self::New,
            Stage::Scraped => Self::Scraped,
            Stage::Renamed => Self::Renamed,
        }
    }
}

/// A flag on top of the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum FlagKindDto {
    Working,
    NeedsReview,
    NotMatched,
    Missing,
    Locked,
    RenamePending,
}

impl From<FlagKind> for FlagKindDto {
    fn from(kind: FlagKind) -> Self {
        match kind {
            FlagKind::Working => Self::Working,
            FlagKind::NeedsReview => Self::NeedsReview,
            FlagKind::NotMatched => Self::NotMatched,
            FlagKind::Missing => Self::Missing,
            FlagKind::Locked => Self::Locked,
            FlagKind::RenamePending => Self::RenamePending,
        }
    }
}

/// A flag with its reason (empty when there is nothing to add).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct LifecycleFlagDto {
    pub kind: FlagKindDto,
    pub reason: String,
}

/// The step an explanation line is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum LineStepDto {
    Scan,
    Scrape,
    Rename,
}

impl From<LineStep> for LineStepDto {
    fn from(step: LineStep) -> Self {
        match step {
            LineStep::Scan => Self::Scan,
            LineStep::Scrape => Self::Scrape,
            LineStep::Rename => Self::Rename,
        }
    }
}

/// The shared status vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum LineStatusDto {
    Done,
    Waiting,
    Missing,
    NeedsKey,
    InProgress,
    Off,
}

impl From<LineStatus> for LineStatusDto {
    fn from(status: LineStatus) -> Self {
        match status {
            LineStatus::Done => Self::Done,
            LineStatus::Waiting => Self::Waiting,
            LineStatus::Missing => Self::Missing,
            LineStatus::NeedsKey => Self::NeedsKey,
            LineStatus::InProgress => Self::InProgress,
            LineStatus::Off => Self::Off,
        }
    }
}

/// Where an explanation line sends the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum LineLinkDto {
    OrganizePreview,
    OrganizeResolve,
}

impl From<LineLink> for LineLinkDto {
    fn from(link: LineLink) -> Self {
        match link {
            LineLink::OrganizePreview => Self::OrganizePreview,
            LineLink::OrganizeResolve => Self::OrganizeResolve,
        }
    }
}

/// One part of an explanation line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct LifecycleLinePartDto {
    pub text: String,
    pub status: Option<LineStatusDto>,
}

impl From<&LinePart> for LifecycleLinePartDto {
    fn from(part: &LinePart) -> Self {
        Self {
            text: part.text.clone(),
            status: part.status.map(Into::into),
        }
    }
}

/// One line of the stage explanation: what happened and when, or what happens next.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct LifecycleLineDto {
    pub step: LineStepDto,
    pub status: LineStatusDto,
    /// `Scanned`, `Scraped`, `Renamed: no`. The screen puts the date after it.
    pub label: String,
    /// Epoch milliseconds.
    pub at_ms: Option<f64>,
    pub parts: Vec<LifecycleLinePartDto>,
    pub link: Option<LineLinkDto>,
}

impl From<&Line> for LifecycleLineDto {
    fn from(line: &Line) -> Self {
        Self {
            step: line.step.into(),
            status: line.status.into(),
            label: line.label.clone(),
            at_ms: line.at_ms.map(millis),
            parts: line.parts.iter().map(Into::into).collect(),
            link: line.link.map(Into::into),
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn millis(ms: i64) -> f64 {
    ms as f64
}

/// The stage, flags and explanation of a title.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct LifecycleDto {
    pub stage: StageDto,
    /// The first flag is the main one, shown beside the stage word.
    pub flags: Vec<LifecycleFlagDto>,
    /// One line per step: Scan, Scrape and Rename.
    pub lines: Vec<LifecycleLineDto>,
    /// Needs review counts the title (an uncertain match, a search that found nothing or an
    /// automatic-rename conflict): what the inbox lists and the REVIEW marker shows.
    pub in_review: bool,
}

impl From<&Lifecycle> for LifecycleDto {
    fn from(lifecycle: &Lifecycle) -> Self {
        Self {
            stage: lifecycle.stage.into(),
            flags: lifecycle
                .flags
                .iter()
                .map(|flag| LifecycleFlagDto {
                    kind: flag.kind.into(),
                    reason: flag.reason.clone(),
                })
                .collect(),
            lines: lifecycle.lines.iter().map(Into::into).collect(),
            in_review: lifecycle.in_review,
        }
    }
}

/// The lifecycle of one title.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ItemLifecycleDto {
    /// Item id as text.
    pub id: String,
    pub lifecycle: LifecycleDto,
}

/// A reason and how many titles share it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ReasonCountDto {
    pub reason: String,
    pub count: u32,
}

/// The pipeline counts of one library type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct LifecycleCountsDto {
    pub total: u32,
    pub new: u32,
    pub scraped: u32,
    pub renamed: u32,
    /// Uncertain matches, titles whose search found nothing and automatic-rename conflicts.
    pub needs_review: u32,
    pub uncertain: u32,
    /// Titles a scrape was tried for that found no confident match (the Not matched flag). A
    /// title no scrape was tried for is not one of them.
    pub not_matched: u32,
    /// New titles waiting for their first scrape: no match, no flag, nothing in Needs review.
    pub not_scraped: u32,
    /// The titles of `not_matched` whose search found nothing (Needs review counts these).
    pub could_not_match: u32,
    /// The most common reason among the titles that are not matched.
    pub not_matched_top_reason: Option<ReasonCountDto>,
    pub working: u32,
    pub rename_pending: u32,
    pub locked: u32,
    pub missing: u32,
    /// Titles whose names are still being checked against the naming style.
    pub checking: u32,
}

impl From<LifecycleCounts> for LifecycleCountsDto {
    fn from(counts: LifecycleCounts) -> Self {
        Self {
            total: counts.total,
            new: counts.new,
            scraped: counts.scraped,
            renamed: counts.renamed,
            needs_review: counts.needs_review,
            uncertain: counts.uncertain,
            not_matched: counts.not_matched,
            not_scraped: counts.not_scraped,
            could_not_match: counts.could_not_match,
            not_matched_top_reason: counts
                .not_matched_top_reason
                .map(|(reason, count)| ReasonCountDto { reason, count }),
            working: counts.working,
            rename_pending: counts.rename_pending,
            locked: counts.locked,
            missing: counts.missing,
            checking: counts.checking,
        }
    }
}

/// The lifecycles of `ids`, by id (`None` without a lifecycle service or for a title that has
/// none). Blocking.
pub(crate) fn lifecycles_of(
    lifecycle: Option<&Arc<LifecycleService>>,
    ids: &[ItemId],
) -> std::collections::BTreeMap<ItemId, LifecycleDto> {
    let Some(service) = lifecycle else {
        return std::collections::BTreeMap::new();
    };
    match service.read(ids) {
        Ok(read) => read
            .iter()
            .map(|(id, lifecycle)| (*id, LifecycleDto::from(lifecycle)))
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "the lifecycle of titles could not be read");
            std::collections::BTreeMap::new()
        }
    }
}

/// The lifecycle of one title, when it has one. Blocking.
pub(crate) fn lifecycle_of(
    lifecycle: Option<&Arc<LifecycleService>>,
    id: ItemId,
) -> Option<LifecycleDto> {
    lifecycles_of(lifecycle, &[id]).remove(&id)
}

fn service(
    library: &State<'_, LibraryState>,
) -> Result<Arc<LifecycleService>, LibraryCommandError> {
    library
        .lifecycle()
        .cloned()
        .ok_or_else(|| storage("CineVault could not open the library.".to_owned()))
}

/// The pipeline counts of the movies or the shows: titles per stage, and Needs review, Not
/// matched, Working, Rename pending, Locked and Missing. Every screen that shows these numbers
/// reads this command.
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_counts(
    library: State<'_, LibraryState>,
    kind: LibraryKindDto,
) -> Result<LifecycleCountsDto, LibraryCommandError> {
    let item_kind = ItemKind::from(kind);
    if !matches!(item_kind, ItemKind::Movie | ItemKind::Show) {
        return Err(LibraryCommandError {
            code: "bad_filter".to_owned(),
            message: "Collections have no pipeline.".to_owned(),
        });
    }
    let service = service(&library)?;
    blocking(move || {
        service
            .counts(item_kind)
            .map(LifecycleCountsDto::from)
            .map_err(LibraryCommandError::from)
    })
    .await
}

/// The stage, flags and explanation lines of the titles `ids` (movies, shows, seasons and
/// episodes). A title without a lifecycle, such as a collection, is left out.
#[tauri::command]
#[specta::specta]
pub async fn get_lifecycles(
    library: State<'_, LibraryState>,
    ids: Vec<String>,
) -> Result<Vec<ItemLifecycleDto>, LibraryCommandError> {
    let ids = ids
        .iter()
        .map(|id| parse_id(id))
        .collect::<Result<Vec<_>, _>>()?;
    let service = service(&library)?;
    blocking(move || {
        let read = service.read(&ids).map_err(LibraryCommandError::from)?;
        Ok(ids
            .iter()
            .filter_map(|id| {
                read.get(id).map(|lifecycle| ItemLifecycleDto {
                    id: id.0.to_string(),
                    lifecycle: lifecycle.into(),
                })
            })
            .collect())
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use cv_app::data_location::{DataLocation, LocationInputs};
    use cv_app::secrets::SecretStore;
    use cv_app::services::{AppServices, BootOptions};
    use cv_artwork::library_source::LibrarySources;
    use cv_domain::{Item, SourceKind};

    use super::*;
    use crate::detail_ipc::DetailReader;
    use crate::tv_detail_ipc::TvReader;

    fn services(root: &Path) -> Arc<AppServices> {
        let inputs = LocationInputs {
            exe_dir: root.join("app"),
            user_data_base: root.join("userdata"),
            user_config_base: root.join("userconfig"),
        };
        std::fs::create_dir_all(&inputs.exe_dir).unwrap();
        let options = BootOptions::new(
            DataLocation::resolve(&inputs),
            Arc::new(SecretStore::in_memory()),
        )
        .without_providers();
        AppServices::boot(options).expect("boot")
    }

    /// The movie, show and episode pages (and so the Inspector, which reads
    /// them) carry the stage and flags of the title and the label of its library folder.
    #[test]
    fn the_detail_and_tv_pages_carry_the_lifecycle_and_the_source_label() {
        let root = tempfile::tempdir().unwrap();
        let services = services(root.path());
        let (movies, shows) = (r"D:\Movies 4K", r"D:\Series");
        let (movie, show, episode) = services
            .db()
            .write(move |conn| {
                let source =
                    cv_db::repo::data_sources::insert_source(conn, SourceKind::Movie, movies)?;
                cv_db::repo::data_sources::set_source_label(conn, source.id, "NAS 4K")?;
                let source = cv_db::repo::data_sources::insert_source(conn, SourceKind::Tv, shows)?;
                cv_db::repo::data_sources::set_source_label(conn, source.id, "NAS TV")?;
                let mut heat = Item::movie("Heat");
                heat.core.data_source = movies.to_owned();
                heat.core.path = format!(r"{movies}\Heat (1995)");
                heat.core.ids.set("tmdb", "949");
                heat.core.last_scraper_id = "tmdb".to_owned();
                let movie = cv_db::repo::insert_item(conn, &heat)?;
                let mut dark = Item::show("Dark");
                dark.core.data_source = shows.to_owned();
                dark.core.path = format!(r"{shows}\Dark");
                let show = cv_db::repo::insert_item(conn, &dark)?;
                let mut secrets = Item::episode(show, "Secrets");
                secrets.core.data_source = shows.to_owned();
                let episode = cv_db::repo::insert_item(conn, &secrets)?;
                Ok((movie, show, episode))
            })
            .unwrap();
        let lifecycle = Some(Arc::clone(services.lifecycle()));
        let sources = Arc::new(LibrarySources::new());

        let page = DetailReader::new(Arc::clone(services.db()), Arc::clone(&sources), None)
            .with_lifecycle(lifecycle.clone())
            .movie(movie)
            .unwrap();
        assert_eq!(page.source_label, "NAS 4K");
        let stage = page.lifecycle.expect("the movie page has a lifecycle");
        assert_eq!(stage.stage, StageDto::Scraped);
        assert_eq!(stage.lines.len(), 3);

        let tv = TvReader::new(Arc::clone(services.db()), sources, None).with_lifecycle(lifecycle);
        let page = tv.show(show).unwrap();
        assert_eq!(page.source_label, "NAS TV");
        let stage = page.lifecycle.expect("the show page has a lifecycle");
        assert_eq!(stage.stage, StageDto::New);
        // No scrape was tried for it: just New, not Not matched (QA final-07).
        assert!(stage.flags.is_empty(), "{:?}", stage.flags);
        let page = tv.episode(episode).unwrap();
        assert_eq!(page.source_label, "NAS TV");
        assert_eq!(
            page.lifecycle.map(|l| l.stage),
            Some(StageDto::New),
            "an episode takes the lifecycle of its show"
        );
    }
}

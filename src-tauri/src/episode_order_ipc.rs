//! Typed commands for the per-show episode ordering: which orderings a show can use,
//! and switching it.
//!
//! The rules live in `cv_app::episode_order`. Switching asks the provider for the episode list in
//! the new ordering, matches the library's episodes with it and stores the show's choice; a
//! provider that lacks the ordering answers `ordering_not_available` and nothing changes.

use std::sync::Arc;

use cv_app::episode_order::{
    EpisodeOrderError, EpisodeOrderService, OrderingReport, ordering_name,
};
use cv_app::match_service::MatchService;
use cv_domain::{EpisodeGroup, EpisodeOrdering, ItemId};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::boot::Boot;

/// Held in Tauri state. When the app services could not start, every command answers with a
/// `storage` error that says so.
pub struct EpisodeOrderState {
    matching: Option<Arc<MatchService>>,
}

impl EpisodeOrderState {
    pub fn from_boot(boot: &Boot) -> Self {
        Self {
            matching: boot
                .services()
                .map(|services| Arc::clone(services.matching())),
        }
    }

    fn service(&self) -> Result<EpisodeOrderService, EpisodeOrderCommandError> {
        self.matching
            .clone()
            .map(EpisodeOrderService::new)
            .ok_or_else(|| EpisodeOrderCommandError {
                code: "storage".to_owned(),
                message:
                    "CineVault could not open the library, so episode orderings are not available."
                        .to_owned(),
            })
    }
}

/// Why a command did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct EpisodeOrderCommandError {
    /// Stable code: `storage`, `not_found`, `unsupported`, `invalid`, `ordering_not_available`,
    /// `no_provider`, `key_needed` (a provider key is missing), `not_identified` (the show has
    /// no ID of a provider with episode lists) or `provider`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<EpisodeOrderError> for EpisodeOrderCommandError {
    fn from(error: EpisodeOrderError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

type CommandResult<T> = Result<T, EpisodeOrderCommandError>;

fn invalid(message: String) -> EpisodeOrderCommandError {
    EpisodeOrderCommandError {
        code: "invalid".to_owned(),
        message,
    }
}

fn parse_show(text: &str) -> CommandResult<ItemId> {
    text.trim()
        .parse::<i64>()
        .map(ItemId)
        .map_err(|_| invalid(format!("{text:?} is not a library item id.")))
}

/// An episode ordering: its kind and the provider's name for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EpisodeOrderingDto {
    /// `aired`, `dvd`, `absolute`, `alternate` or `display`.
    pub ordering: String,
    /// The provider's name for an alternate order; empty for the standard ones.
    pub name: String,
    /// The ordering in words, for the picker. Ignored when sent to the backend.
    #[serde(default)]
    pub label: String,
}

impl From<&EpisodeGroup> for EpisodeOrderingDto {
    fn from(group: &EpisodeGroup) -> Self {
        Self {
            ordering: kind_name(group.ordering).to_owned(),
            name: group.name.clone(),
            label: ordering_name(group),
        }
    }
}

fn kind_name(ordering: EpisodeOrdering) -> &'static str {
    match ordering {
        EpisodeOrdering::Aired => "aired",
        EpisodeOrdering::Dvd => "dvd",
        EpisodeOrdering::Absolute => "absolute",
        EpisodeOrdering::Alternate => "alternate",
        EpisodeOrdering::Display => "display",
    }
}

fn group_of(dto: &EpisodeOrderingDto) -> CommandResult<EpisodeGroup> {
    let ordering = match dto.ordering.as_str() {
        "aired" => EpisodeOrdering::Aired,
        "dvd" => EpisodeOrdering::Dvd,
        "absolute" => EpisodeOrdering::Absolute,
        "alternate" => EpisodeOrdering::Alternate,
        other => return Err(invalid(format!("{other:?} is not an episode ordering."))),
    };
    Ok(EpisodeGroup::new(ordering, dto.name.trim()))
}

/// The orderings of a show: the one in use and the ones it can be switched to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ShowOrderingsDto {
    pub active: EpisodeOrderingDto,
    pub options: Vec<EpisodeOrderingDto>,
}

/// What switching a show to an ordering did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct OrderingReportDto {
    pub ordering: EpisodeOrderingDto,
    /// Episodes of the library found in the provider's list.
    pub matched: u32,
    /// Episodes of the library the list has no counterpart for; they keep their numbers.
    pub unmatched: u32,
    /// Locked episodes that were left unchanged.
    pub locked: u32,
    /// Episodes of the list that the library lacks.
    pub missing: u32,
    /// The show's season numbers in the ordering; `0` is the specials.
    pub seasons: Vec<u32>,
}

fn small(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

impl From<OrderingReport> for OrderingReportDto {
    fn from(report: OrderingReport) -> Self {
        Self {
            ordering: (&report.group).into(),
            matched: small(report.matched),
            unmatched: small(report.unmatched.len()),
            locked: small(report.locked),
            missing: small(report.missing),
            seasons: report.seasons,
        }
    }
}

/// The orderings a show can use and the one it uses now.
#[tauri::command]
#[specta::specta]
pub async fn get_show_orderings(
    state: State<'_, EpisodeOrderState>,
    show_id: String,
) -> CommandResult<ShowOrderingsDto> {
    let id = parse_show(&show_id)?;
    let service = state.service()?;
    let (active, options) = tauri::async_runtime::spawn_blocking(move || service.options(id))
        .await
        .map_err(|error| EpisodeOrderCommandError {
            code: "storage".to_owned(),
            message: error.to_string(),
        })??;
    Ok(ShowOrderingsDto {
        active: (&active).into(),
        options: options.iter().map(Into::into).collect(),
    })
}

/// Switches the show to an ordering and rematches its episodes. Answers `ordering_not_available`
/// when the provider has no episodes for the show in that ordering; the show is then unchanged.
#[tauri::command]
#[specta::specta]
pub async fn set_show_ordering(
    state: State<'_, EpisodeOrderState>,
    show_id: String,
    ordering: EpisodeOrderingDto,
) -> CommandResult<OrderingReportDto> {
    let id = parse_show(&show_id)?;
    let group = group_of(&ordering)?;
    let service = state.service()?;
    Ok(service.set_ordering(id, &group).await?.into())
}

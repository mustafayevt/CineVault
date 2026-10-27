//! Typed commands for episode scraping: scraping the episodes of a show or one
//! episode with the user's field selection, and finding an unnumbered episode by title.
//!
//! The rules live in `cv_app::episode_scrape`. A search returns entries of the provider's episode
//! list for the show's ordering; assigning one gives the episode its numbers and IDs and scrapes
//! the selected fields (locks and do-not-overwrite are honoured, as for every scrape).

use std::sync::Arc;

use cv_app::episode_scrape::{
    EpisodeHit, EpisodePick, EpisodeScrapeError, EpisodeScrapeReport, EpisodeScrapeService,
    ScrapeOverride,
};
use cv_app::match_service::MatchService;
use cv_domain::{FieldSet, ItemId, ScrapeBlock, ScrapeOutcome};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::boot::Boot;

/// Held in Tauri state. When the app services could not start, every command answers with a
/// `storage` error that says so.
pub struct EpisodeScrapeState {
    matching: Option<Arc<MatchService>>,
}

impl EpisodeScrapeState {
    pub fn from_boot(boot: &Boot) -> Self {
        Self {
            matching: boot
                .services()
                .map(|services| Arc::clone(services.matching())),
        }
    }

    fn service(&self) -> Result<EpisodeScrapeService, EpisodeScrapeCommandError> {
        self.matching
            .clone()
            .map(EpisodeScrapeService::new)
            .ok_or_else(|| EpisodeScrapeCommandError {
                code: "storage".to_owned(),
                message: "CineVault could not open the library, so episodes cannot be scraped."
                    .to_owned(),
            })
    }
}

/// Why a command did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct EpisodeScrapeCommandError {
    /// Stable code: `storage`, `not_found`, `unsupported`, `invalid`, `not_available`,
    /// `no_provider`, `key_needed` (a provider key is missing), `not_identified` (the show has
    /// no ID of a provider with episode lists), `no_match`, `stale_pick` or `provider`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<EpisodeScrapeError> for EpisodeScrapeCommandError {
    fn from(error: EpisodeScrapeError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

type CommandResult<T> = Result<T, EpisodeScrapeCommandError>;

fn invalid(message: String) -> EpisodeScrapeCommandError {
    EpisodeScrapeCommandError {
        code: "invalid".to_owned(),
        message,
    }
}

fn parse_id(text: &str) -> CommandResult<ItemId> {
    text.trim()
        .parse::<i64>()
        .map(ItemId)
        .map_err(|_| invalid(format!("{text:?} is not a library item id.")))
}

fn small(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// One provider ID of an episode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EpisodeIdDto {
    pub provider: String,
    pub value: String,
}

/// What identifies an entry of the provider's list when it is assigned. The UI returns it
/// unchanged from a search hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EpisodePickDto {
    pub ids: Vec<EpisodeIdDto>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

impl From<&EpisodePick> for EpisodePickDto {
    fn from(pick: &EpisodePick) -> Self {
        Self {
            ids: pick
                .ids
                .iter()
                .map(|(provider, value)| EpisodeIdDto {
                    provider: provider.to_owned(),
                    value: value.to_owned(),
                })
                .collect(),
            season: pick.number.map(|n| n.0),
            episode: pick.number.map(|n| n.1),
        }
    }
}

impl EpisodePickDto {
    fn to_pick(&self) -> EpisodePick {
        let mut ids = cv_domain::ProviderIds::default();
        for id in &self.ids {
            if !id.provider.trim().is_empty() && !id.value.trim().is_empty() {
                ids.set(id.provider.trim(), id.value.trim());
            }
        }
        EpisodePick {
            ids,
            number: self.season.zip(self.episode),
        }
    }
}

/// One entry of the provider's episode list that fits the typed title.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct EpisodeHitDto {
    pub title: String,
    /// Season and episode in the show's ordering, when the list gives them.
    pub season: Option<u32>,
    pub episode: Option<u32>,
    /// The aired date as `YYYY-MM-DD`.
    pub first_aired: Option<String>,
    /// The start of the plot.
    pub plot: String,
    /// 0 to 1, higher is closer.
    pub score: f64,
    /// Sent back to assign this entry.
    pub pick: EpisodePickDto,
}

impl From<&EpisodeHit> for EpisodeHitDto {
    fn from(hit: &EpisodeHit) -> Self {
        Self {
            title: hit.title.clone(),
            season: hit.number.map(|n| n.0),
            episode: hit.number.map(|n| n.1),
            first_aired: hit.first_aired.map(|d| d.to_string()),
            plot: hit.plot.clone(),
            score: hit.score,
            pick: (&hit.pick()).into(),
        }
    }
}

/// What a scrape did to one episode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct EpisodeWriteDto {
    /// Keys of the fields whose stored value changed.
    pub written: Vec<String>,
    /// Keys of selected fields a field lock kept.
    pub locked_fields: Vec<String>,
    /// Keys of selected fields kept because they were filled (do not overwrite).
    pub kept: Vec<String>,
    /// The episode itself is locked, so nothing was written.
    pub item_locked: bool,
}

impl From<&ScrapeOutcome> for EpisodeWriteDto {
    fn from(outcome: &ScrapeOutcome) -> Self {
        let keys =
            |fields: &[cv_domain::MetaField]| fields.iter().map(|f| f.key().to_owned()).collect();
        Self {
            written: keys(&outcome.written),
            locked_fields: keys(&outcome.locked),
            kept: keys(&outcome.kept),
            item_locked: outcome.blocked == Some(ScrapeBlock::ItemLocked),
        }
    }
}

/// What assigning a list entry to an episode did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AssignEpisodeDto {
    /// The season and episode the episode has now; empty when it is locked.
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub write: EpisodeWriteDto,
}

/// What scraping the episodes of a show did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct EpisodeScrapeReportDto {
    /// Episodes of the library found in the provider's list.
    pub matched: u32,
    /// Matched episodes whose stored values changed.
    pub changed: u32,
    /// Episodes of the library the list has no counterpart for: search them by title.
    pub unmatched: u32,
    /// Matched episodes left alone because they are locked.
    pub locked: u32,
    /// Matched episodes where a field lock kept a selected field.
    pub field_locked: u32,
    /// Episodes of the list that the library lacks.
    pub missing: Option<u32>,
}

impl From<&EpisodeScrapeReport> for EpisodeScrapeReportDto {
    fn from(report: &EpisodeScrapeReport) -> Self {
        Self {
            matched: small(report.matched),
            changed: small(report.changed),
            unmatched: small(report.unmatched.len()),
            locked: small(report.locked),
            field_locked: small(report.field_locked),
            missing: report.missing.map(small),
        }
    }
}

/// The run's choices: `None` takes the setting.
fn choice(fields: Option<Vec<String>>, do_not_overwrite: Option<bool>) -> ScrapeOverride {
    ScrapeOverride {
        fields: fields.map(|keys| FieldSet::from_keys(&keys.join(","))),
        do_not_overwrite,
    }
}

/// Searches the episode list of a show by title. Fails with `not_available` when the provider has
/// no list for the show, and `no_provider` when no provider is ready.
#[tauri::command]
#[specta::specta]
pub async fn search_show_episodes(
    state: State<'_, EpisodeScrapeState>,
    show_id: String,
    query: String,
) -> CommandResult<Vec<EpisodeHitDto>> {
    let show = parse_id(&show_id)?;
    let service = state.service()?;
    let hits = service.search(show, &query).await?;
    Ok(hits.iter().map(Into::into).collect())
}

/// Gives an episode the numbers and IDs of a searched list entry and scrapes the selected fields
/// into it. `fields` (field keys) and `do_not_overwrite` override the settings for this call.
#[tauri::command]
#[specta::specta]
pub async fn assign_episode(
    state: State<'_, EpisodeScrapeState>,
    episode_id: String,
    pick: EpisodePickDto,
    fields: Option<Vec<String>>,
    do_not_overwrite: Option<bool>,
) -> CommandResult<AssignEpisodeDto> {
    let episode = parse_id(&episode_id)?;
    let service = state.service()?;
    let report = service
        .assign(episode, &pick.to_pick(), &choice(fields, do_not_overwrite))
        .await?;
    Ok(AssignEpisodeDto {
        season: report.number.map(|n| n.0),
        episode: report.number.map(|n| n.1),
        write: (&report.outcome).into(),
    })
}

/// Scrapes the episodes of a show by number in the show's ordering.
#[tauri::command]
#[specta::specta]
pub async fn scrape_show_episodes(
    state: State<'_, EpisodeScrapeState>,
    show_id: String,
    fields: Option<Vec<String>>,
    do_not_overwrite: Option<bool>,
) -> CommandResult<EpisodeScrapeReportDto> {
    let show = parse_id(&show_id)?;
    let service = state.service()?;
    let report = service
        .scrape_show(show, &choice(fields, do_not_overwrite))
        .await?;
    Ok((&report).into())
}

/// Scrapes one episode. Fails with `no_match` when the list has nothing for its number, which is
/// the cue to search it by title.
#[tauri::command]
#[specta::specta]
pub async fn scrape_episode(
    state: State<'_, EpisodeScrapeState>,
    episode_id: String,
    fields: Option<Vec<String>>,
    do_not_overwrite: Option<bool>,
) -> CommandResult<EpisodeWriteDto> {
    let episode = parse_id(&episode_id)?;
    let service = state.service()?;
    let outcome = service
        .scrape_episode(episode, &choice(fields, do_not_overwrite))
        .await?;
    Ok((&outcome.outcome).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_survives_the_trip_through_the_dto() {
        let mut ids = cv_domain::ProviderIds::default();
        ids.set("tvdb", "e103");
        let pick = EpisodePick {
            ids,
            number: Some((1, 3)),
        };
        let dto = EpisodePickDto::from(&pick);
        assert_eq!(dto.to_pick(), pick);
        let blank = EpisodePickDto {
            ids: vec![EpisodeIdDto {
                provider: " ".into(),
                value: "x".into(),
            }],
            season: Some(2),
            episode: None,
        };
        let back = blank.to_pick();
        assert!(back.ids.iter().next().is_none() && back.number.is_none());
    }

    #[test]
    fn the_runs_choices_are_field_keys_and_a_mode() {
        let c = choice(
            Some(vec!["title".into(), "plot".into(), "bogus".into()]),
            Some(true),
        );
        assert_eq!(c.fields.unwrap().to_keys(), "title,plot");
        assert_eq!(c.do_not_overwrite, Some(true));
        let none = choice(None, None);
        assert!(none.fields.is_none() && none.do_not_overwrite.is_none());
    }

    #[test]
    fn codes_and_messages_reach_the_ui() {
        let error: EpisodeScrapeCommandError = EpisodeScrapeError::NoCounterpart.into();
        assert_eq!(error.code, "no_match");
        assert!(error.message.contains("Search it by title"));
        assert_eq!(parse_id("x").unwrap_err().code, "invalid");
        assert_eq!(parse_id(" 12 ").unwrap(), ItemId(12));
    }
}

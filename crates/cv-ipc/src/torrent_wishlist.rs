//! The torrent Wishlist over IPC: the entries with their matches, adding, editing and removing
//! them, the check, and the state of each match.
//!
//! The rules live in `cv_app::torrents::wishlist`; this module adapts them to DTOs. A match's
//! quality words are read from its name again with the release search's parser, so they read
//! like the search's.

use cv_app::torrents::wishlist::{
    CheckNote, MinResolution, WishCriteria, WishEntry, WishMatch, WishRequest, WishScope,
    WishSource,
};
use cv_db::repo::torrent_wishlist::MatchState;
use cv_torrent::release::parse_release;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::matching::ScrapeLinkDto;
use crate::matching::{POSTER_WAIT, poster_keys_of};
use crate::torrent_stream::{
    CommandResult, TorrentCommandError, TorrentEpisodeRefDto, TorrentState, audio_text, codec_text,
    hdr_text, kind_text, link_dto, parse_kind, resolution_text, source_text,
};

/// What kind of release an entry is after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WishCriteriaDto {
    /// `720p`, `1080p` or `2160p`; none takes any.
    pub min_resolution: Option<String>,
    /// `Remux`, `BluRay`, `WEB-DL` or `WEBRip`; none takes any.
    pub sources: Vec<String>,
    /// Only releases with HDR.
    pub hdr: bool,
    /// The largest size in bytes.
    pub max_size: Option<f64>,
    /// The fewest seeders.
    pub min_seeders: Option<u32>,
}

/// What an entry looks for within its title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WishScopeDto {
    /// `movie`, `season`, `episode` or `next` (the next new episode).
    pub kind: String,
    pub season: Option<u16>,
    pub episode: Option<u16>,
}

/// A release a check kept for an entry.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WishMatchDto {
    /// The release's key within its entry.
    pub key: String,
    pub name: String,
    pub size: Option<f64>,
    pub seeders: Option<u32>,
    pub magnet: Option<String>,
    pub torrent_url: Option<String>,
    /// The ids of the sources that returned it.
    pub sources: Vec<String>,
    /// Epoch milliseconds.
    pub found: f64,
    /// `new`, `seen` or `sent`.
    pub state: String,
    /// `2160p`, `1080p`, `720p`, `480p` or `unknown`.
    pub resolution: String,
    pub source: Option<String>,
    pub codec: Option<String>,
    pub hdr: Option<String>,
    pub audio: Option<String>,
}

/// An entry of the Wishlist.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WishEntryDto {
    pub id: u32,
    /// `movie` or `show`.
    pub kind: String,
    pub tmdb_id: u32,
    pub title: String,
    pub year: Option<u16>,
    pub poster_key: Option<String>,
    pub scope: WishScopeDto,
    pub criteria: WishCriteriaDto,
    /// Epoch milliseconds.
    pub added: f64,
    pub checked: Option<f64>,
    /// What the last check could not do: `no-source`, `airs`, `no-new-episode` or `failed`.
    pub note: Option<String>,
    /// The day an awaited episode airs (`airs`), or the failure's words (`failed`).
    pub note_detail: Option<String>,
    /// When the library was found to hold a copy that meets the criteria.
    pub done: Option<f64>,
    /// The episode a "next new episode" entry looked for at its last check.
    pub target: Option<TorrentEpisodeRefDto>,
    pub in_library: Option<ScrapeLinkDto>,
    /// The matches that are not dismissed, most seeders first.
    pub matches: Vec<WishMatchDto>,
}

/// A title to add to the Wishlist.
#[derive(Debug, Clone, PartialEq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WishAddDto {
    /// `movie` or `show`.
    pub kind: String,
    pub tmdb_id: u32,
    pub scope: WishScopeDto,
    pub criteria: WishCriteriaDto,
}

/// What a check did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WishCheckDto {
    /// How many entries were checked.
    pub checked: u32,
    /// How many releases were found that were not known before.
    pub new_matches: u32,
    /// Another check was running, so this one did nothing.
    pub busy: bool,
}

fn invalid(message: &str) -> TorrentCommandError {
    TorrentCommandError::new("invalid", message)
}

#[allow(clippy::cast_precision_loss)]
fn millis(value: i64) -> f64 {
    value as f64
}

pub fn criteria_dto(criteria: &WishCriteria) -> WishCriteriaDto {
    #[allow(clippy::cast_precision_loss)]
    WishCriteriaDto {
        min_resolution: criteria.min_resolution.map(|r| r.as_str().to_owned()),
        sources: criteria
            .sources
            .iter()
            .map(|s| s.as_str().to_owned())
            .collect(),
        hdr: criteria.hdr,
        max_size: criteria.max_size.map(|size| size as f64),
        min_seeders: criteria.min_seeders,
    }
}

/// The criteria a DTO names; an unknown resolution or source is refused.
///
/// # Errors
/// `invalid` for an unknown resolution or source, or a negative size.
pub fn criteria_of(dto: &WishCriteriaDto) -> CommandResult<WishCriteria> {
    let min_resolution = match dto.min_resolution.as_deref() {
        None | Some("") => None,
        Some(text) => Some(
            MinResolution::parse(text).ok_or_else(|| invalid("That resolution is not offered."))?,
        ),
    };
    let mut sources = Vec::new();
    for text in &dto.sources {
        let source =
            WishSource::parse(text).ok_or_else(|| invalid("That source is not offered."))?;
        if !sources.contains(&source) {
            sources.push(source);
        }
    }
    let max_size = match dto.max_size {
        None => None,
        Some(size) if size.is_finite() && size >= 0.0 => {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let bytes = size.round() as u64;
            Some(bytes)
        }
        Some(_) => return Err(invalid("The size limit must be a positive number.")),
    };
    Ok(WishCriteria {
        min_resolution,
        sources,
        hdr: dto.hdr,
        max_size,
        min_seeders: dto.min_seeders,
    })
}

pub fn scope_dto(scope: WishScope) -> WishScopeDto {
    let (kind, season, episode) = match scope {
        WishScope::Movie => ("movie", None, None),
        WishScope::Season(season) => ("season", Some(season), None),
        WishScope::Episode(season, episode) => ("episode", Some(season), Some(episode)),
        WishScope::Next => ("next", None, None),
    };
    WishScopeDto {
        kind: kind.to_owned(),
        season,
        episode,
    }
}

/// The scope a DTO names.
///
/// # Errors
/// `invalid` for an unknown kind, or a season or episode that is missing.
pub fn scope_of(dto: &WishScopeDto) -> CommandResult<WishScope> {
    let missing = || invalid("A season or episode is missing.");
    match dto.kind.as_str() {
        "movie" => Ok(WishScope::Movie),
        "next" => Ok(WishScope::Next),
        "season" => Ok(WishScope::Season(dto.season.ok_or_else(missing)?)),
        "episode" => Ok(WishScope::Episode(
            dto.season.ok_or_else(missing)?,
            dto.episode.ok_or_else(missing)?,
        )),
        _ => Err(invalid("That scope is not offered.")),
    }
}

fn state_text(state: MatchState) -> &'static str {
    match state {
        MatchState::New => "new",
        MatchState::Seen | MatchState::Dismissed => "seen",
        MatchState::Sent => "sent",
    }
}

pub fn match_dto(found: WishMatch) -> WishMatchDto {
    let parsed = parse_release(&found.name, &[]);
    #[allow(clippy::cast_precision_loss)]
    WishMatchDto {
        key: found.key,
        size: found.size.map(|size| size as f64),
        seeders: found.seeders,
        magnet: found.magnet,
        torrent_url: found.torrent_url,
        sources: found.sources,
        found: millis(found.found_ms),
        state: state_text(found.state).to_owned(),
        resolution: resolution_text(parsed.resolution).to_owned(),
        source: source_text(&parsed),
        codec: codec_text(parsed.codec).map(str::to_owned),
        hdr: hdr_text(parsed.hdr).map(str::to_owned),
        audio: audio_text(parsed.audio, parsed.channels.as_deref()),
        name: found.name,
    }
}

fn note_dto(note: Option<CheckNote>) -> (Option<String>, Option<String>) {
    match note {
        None => (None, None),
        Some(CheckNote::NoSource) => (Some("no-source".to_owned()), None),
        Some(CheckNote::NoNewEpisode) => (Some("no-new-episode".to_owned()), None),
        Some(CheckNote::Airs(date)) => (Some("airs".to_owned()), Some(date.to_string())),
        Some(CheckNote::Failed(words)) => (Some("failed".to_owned()), Some(words)),
    }
}

pub fn entry_dto(entry: WishEntry, poster_key: Option<String>) -> WishEntryDto {
    let (note, note_detail) = note_dto(entry.note);
    WishEntryDto {
        id: u32::try_from(entry.id).unwrap_or_default(),
        kind: kind_text(entry.kind).to_owned(),
        tmdb_id: u32::try_from(entry.tmdb_id).unwrap_or_default(),
        title: entry.title,
        year: entry.year,
        poster_key,
        scope: scope_dto(entry.scope),
        criteria: criteria_dto(&entry.criteria),
        added: millis(entry.added_ms),
        checked: entry.checked_ms.map(millis),
        note,
        note_detail,
        done: entry.done_ms.map(millis),
        target: entry
            .target
            .map(|(season, episode)| TorrentEpisodeRefDto { season, episode }),
        in_library: link_dto(entry.library.as_ref()),
        matches: entry.matches.into_iter().map(match_dto).collect(),
    }
}

/// Every entry of the Wishlist, the newest first, with its matches and poster. Entries whose
/// criteria the library now meets are marked done. Only the library is read (and the posters,
/// from the thumbnail cache).
pub async fn torrent_wishlist(state: &TorrentState) -> CommandResult<Vec<WishEntryDto>> {
    let service = state.get()?;
    let entries = service.wishlist()?;
    let posters = poster_keys_of(
        state.artwork.as_ref(),
        entries
            .iter()
            .filter_map(|entry| entry.poster_url.as_deref()),
        POSTER_WAIT,
    )
    .await;
    Ok(entries
        .into_iter()
        .map(|entry| {
            let key = entry
                .poster_url
                .as_deref()
                .and_then(|url| posters.get(url))
                .cloned();
            entry_dto(entry, key)
        })
        .collect())
}

/// Adds a title to the Wishlist, or replaces the criteria of the entry with this title and
/// scope. Returns the entry's id.
pub async fn torrent_wishlist_add(state: &TorrentState, request: WishAddDto) -> CommandResult<u32> {
    let service = state.get()?;
    let id = service
        .wishlist_add(WishRequest {
            kind: parse_kind(&request.kind)?,
            tmdb_id: u64::from(request.tmdb_id),
            scope: scope_of(&request.scope)?,
            criteria: criteria_of(&request.criteria)?,
        })
        .await?;
    Ok(u32::try_from(id).unwrap_or_default())
}

/// Changes an entry's scope and criteria; the entry is judged and checked again.
pub fn torrent_wishlist_update(
    state: &TorrentState,
    id: u32,
    scope: WishScopeDto,
    criteria: WishCriteriaDto,
) -> CommandResult<()> {
    let service = state.get()?;
    service.wishlist_update(i64::from(id), scope_of(&scope)?, &criteria_of(&criteria)?)?;
    Ok(())
}

/// Removes an entry and its matches.
pub fn torrent_wishlist_remove(state: &TorrentState, id: u32) -> CommandResult<()> {
    state.get()?.wishlist_remove(i64::from(id))?;
    Ok(())
}

/// Checks the Wishlist: the entries that are due (`due_only`) or every entry that is not done.
/// One check runs at a time; a second answers at once with `busy`.
pub async fn torrent_wishlist_check(
    state: &TorrentState,
    due_only: bool,
) -> CommandResult<WishCheckDto> {
    let report = state.get()?.wishlist_check(due_only).await?;
    Ok(WishCheckDto {
        checked: u32::try_from(report.checked).unwrap_or(u32::MAX),
        new_matches: u32::try_from(report.new_matches).unwrap_or(u32::MAX),
        busy: report.busy,
    })
}

/// Marks a match `sent` or `dismissed` (or `seen` again).
pub fn torrent_wishlist_match_state(
    state: &TorrentState,
    id: u32,
    key: String,
    match_state: String,
) -> CommandResult<()> {
    let wanted = match match_state.as_str() {
        "sent" => MatchState::Sent,
        "dismissed" => MatchState::Dismissed,
        "seen" => MatchState::Seen,
        _ => return Err(invalid("That state is not offered.")),
    };
    state
        .get()?
        .wishlist_set_match(i64::from(id), &key, wanted)?;
    Ok(())
}

/// Marks every new match seen (the Wishlist was opened). Returns how many there were.
pub fn torrent_wishlist_seen(state: &TorrentState) -> CommandResult<u32> {
    let count = state.get()?.wishlist_seen()?;
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}

/// How many matches have not been looked at yet, for the sidebar.
pub fn torrent_wishlist_new_count(state: &TorrentState) -> CommandResult<u32> {
    let count = state.get()?.wishlist_new_count()?;
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn criteria_dto_of(min: Option<&str>, sources: &[&str]) -> WishCriteriaDto {
        WishCriteriaDto {
            min_resolution: min.map(str::to_owned),
            sources: sources.iter().map(|s| (*s).to_owned()).collect(),
            hdr: true,
            max_size: Some(80_000_000_000.0),
            min_seeders: Some(5),
        }
    }

    #[test]
    fn criteria_read_back_and_unknown_words_are_refused() {
        let dto = criteria_dto_of(Some("2160p"), &["Remux", "WEB-DL", "Remux"]);
        let criteria = criteria_of(&dto).unwrap();
        assert_eq!(criteria.min_resolution, Some(MinResolution::R2160));
        assert_eq!(criteria.sources, [WishSource::Remux, WishSource::WebDl]);
        assert_eq!(criteria.max_size, Some(80_000_000_000));
        assert_eq!(
            criteria_dto(&criteria),
            criteria_dto_of(Some("2160p"), &["Remux", "WEB-DL"])
        );
        assert!(criteria_of(&criteria_dto_of(Some("4320p"), &[])).is_err());
        assert!(criteria_of(&criteria_dto_of(None, &["CAM"])).is_err());
        let mut negative = criteria_dto_of(None, &[]);
        negative.max_size = Some(-1.0);
        assert_eq!(criteria_of(&negative).unwrap_err().code, "invalid");
    }

    #[test]
    fn scopes_read_back_and_need_their_numbers() {
        for scope in [
            WishScope::Movie,
            WishScope::Season(2),
            WishScope::Episode(3, 6),
            WishScope::Next,
        ] {
            assert_eq!(scope_of(&scope_dto(scope)).unwrap(), scope);
        }
        let season = WishScopeDto {
            kind: "season".to_owned(),
            season: None,
            episode: None,
        };
        assert!(scope_of(&season).is_err());
        let other = WishScopeDto {
            kind: "show".to_owned(),
            ..season
        };
        assert!(scope_of(&other).is_err());
    }

    #[test]
    fn a_match_reads_its_quality_from_its_name() {
        let dto = match_dto(WishMatch {
            key: "k".to_owned(),
            name: "Example.Movie.2020.2160p.UHD.BluRay.REMUX.DV.HEVC.TrueHD.Atmos.7.1-GRP"
                .to_owned(),
            size: Some(58_000_000_000),
            seeders: Some(40),
            magnet: None,
            torrent_url: None,
            sources: vec!["yts".to_owned()],
            found_ms: 5,
            state: MatchState::New,
        });
        assert_eq!(dto.resolution, "2160p");
        assert_eq!(dto.source.as_deref(), Some("Remux"));
        assert_eq!(dto.hdr.as_deref(), Some("DV"));
        assert_eq!(dto.codec.as_deref(), Some("HEVC"));
        assert_eq!(dto.state, "new");
        assert!((dto.found - 5.0).abs() < f64::EPSILON);
    }
}

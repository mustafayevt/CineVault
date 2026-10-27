//! Typed commands for the artwork picker: the candidates of every provider for one artwork type,
//! putting a chosen image (or a set of extras) in place, and filling the missing artwork of a
//! selection.
//!
//! The rules live in `cv_app::artwork` (and `cv_artwork::download` below it). This module opens
//! the service from the app services, converts to the DTOs the interface sees and keeps images
//! off IPC: a candidate carries a `cv://img` key, because the service has already downloaded it
//! into the thumbnail cache.

use std::sync::Arc;

use cv_app::artwork::{ArtworkError, ArtworkOutcome, ArtworkService, PickerCandidate, source_url};
use cv_artwork::thumb::ThumbCache;
use cv_domain::{ItemId, MediaFileType};
use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::boot::Boot;
use crate::library_ipc::{PosterDto, poster_dto};

/// Held in Tauri state. When the app services could not start, every command answers with a
/// `storage` error that says so.
pub struct ArtworkState {
    pub(crate) service: Option<Arc<ArtworkService>>,
    pub(crate) thumbs: Option<Arc<ThumbCache>>,
}

impl ArtworkState {
    pub fn from_boot(boot: &Boot) -> Self {
        let services = boot.services();
        Self {
            service: services.map(|services| Arc::clone(services.artwork())),
            thumbs: services.map(|services| Arc::clone(services.thumbs())),
        }
    }

    pub(crate) fn get(&self) -> Result<Arc<ArtworkService>, ArtworkCommandError> {
        self.service.clone().ok_or_else(|| {
            ArtworkCommandError::new(
                "storage",
                "CineVault could not open the library, so artwork cannot be changed.",
            )
        })
    }
}

/// Why a command did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ArtworkCommandError {
    /// Stable code: `storage`, `not_found`, `unsupported`, `no_folder` or `invalid`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl ArtworkCommandError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

impl From<ArtworkError> for ArtworkCommandError {
    fn from(error: ArtworkError) -> Self {
        let code = match &error {
            ArtworkError::ItemNotFound => "not_found",
            ArtworkError::Unsupported | ArtworkError::UnknownSlot => "unsupported",
            ArtworkError::NoFolder => "no_folder",
            ArtworkError::BadSource(_) => "invalid",
            ArtworkError::UndoGone | ArtworkError::UndoStale(_) => "undo_gone",
            ArtworkError::Db(_) | ArtworkError::Worker => "storage",
            ArtworkError::Busy(_) => "busy",
        };
        Self::new(code, error.to_string())
    }
}

pub(crate) type CommandResult<T> = Result<T, ArtworkCommandError>;

pub(crate) fn parse_item(text: &str) -> CommandResult<ItemId> {
    text.trim().parse::<i64>().map(ItemId).map_err(|_| {
        ArtworkCommandError::new("invalid", format!("{text:?} is not a library item id."))
    })
}

/// The artwork types as the interface names them.
const SLOTS: [(&str, MediaFileType); 12] = [
    ("poster", MediaFileType::Poster),
    ("fanart", MediaFileType::Fanart),
    ("banner", MediaFileType::Banner),
    ("clearart", MediaFileType::Clearart),
    ("clearlogo", MediaFileType::Clearlogo),
    ("disc", MediaFileType::Disc),
    ("thumb", MediaFileType::Thumb),
    ("keyart", MediaFileType::Keyart),
    ("squareart", MediaFileType::Squareart),
    ("characterart", MediaFileType::Characterart),
    ("extrafanart", MediaFileType::Extrafanart),
    ("extrathumb", MediaFileType::Extrathumb),
];

pub(crate) fn parse_slot(text: &str) -> CommandResult<MediaFileType> {
    SLOTS
        .iter()
        .find(|(name, _)| *name == text)
        .map(|(_, kind)| *kind)
        .ok_or_else(|| {
            ArtworkCommandError::new("unsupported", format!("{text:?} is not a kind of artwork."))
        })
}

pub(crate) fn slot_name(kind: MediaFileType) -> Option<&'static str> {
    SLOTS
        .iter()
        .find(|(_, candidate)| *candidate == kind)
        .map(|(name, _)| *name)
}

fn is_extra(kind: MediaFileType) -> bool {
    matches!(kind, MediaFileType::Extrafanart | MediaFileType::Extrathumb)
}

// ---- DTOs ------------------------------------------------------------------------------------

/// A provider that was asked for images.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ArtworkProviderDto {
    pub key: String,
    pub name: String,
}

/// One image a provider offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ArtworkCandidateDto {
    /// Where it comes from; pass it back to `place_artwork` or `place_extra_artwork`.
    pub id: String,
    pub provider: String,
    /// Primary language of the text on the image, `None` for an image without text.
    pub language: Option<String>,
    pub width: u32,
    pub height: u32,
    /// How well it fits the artwork settings, 0 to 90.
    pub score: u32,
    /// The "Best" order of the picker, higher first: the fit, then the language, the size and
    /// how much people like the image. Comparable across providers.
    pub best: u32,
    /// How many people voted on the image at the provider (TMDB); `0` when unknown.
    pub votes: u32,
    /// The average of those votes times ten (`56` for 5.6); `0` without votes.
    pub rating: u32,
    /// How many people liked the image at the provider (Fanart.tv); `0` when unknown.
    pub likes: u32,
    /// Too small or the wrong shape for its type: listed behind "Show N more".
    pub low_quality: bool,
    /// Its thumbnail in the image cache.
    pub image: PosterDto,
}

/// What putting images in place did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ArtworkPlacedDto {
    /// Images written.
    pub images: u32,
    /// What could not be done, as `what: reason`.
    pub problems: Vec<String>,
    /// The History entry of the change: pass it to `history_undo` to put the displaced files
    /// back. `None` when nothing was written.
    pub undo: Option<String>,
}

/// What filling the missing artwork of a selection did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ArtworkRunDto {
    pub images: u32,
    pub portraits: u32,
    /// Titles that could not be worked on.
    pub skipped: u32,
}

fn small(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

pub(crate) fn candidate_dto(
    candidate: PickerCandidate,
    thumbs: Option<&ThumbCache>,
) -> ArtworkCandidateDto {
    ArtworkCandidateDto {
        image: poster_dto(thumbs, &candidate.key),
        id: candidate.url,
        provider: candidate.provider,
        language: candidate.language,
        width: candidate.width,
        height: candidate.height,
        score: candidate.score,
        best: candidate.best,
        votes: candidate.votes,
        rating: candidate.rating,
        likes: candidate.likes,
        low_quality: candidate.low_quality,
    }
}

fn placed_dto(outcome: &ArtworkOutcome) -> ArtworkPlacedDto {
    ArtworkPlacedDto {
        images: small(outcome.images),
        problems: outcome.problems.clone(),
        undo: outcome.undo.clone(),
    }
}

// ---- commands --------------------------------------------------------------------------------

/// Sets one image of an item from a candidate's address, a pasted web address or the path of a
/// picture on disk. The old image goes to the trash folder of the data source.
#[tauri::command]
#[specta::specta]
pub async fn place_artwork(
    state: State<'_, ArtworkState>,
    item_id: String,
    slot: String,
    source: String,
) -> CommandResult<ArtworkPlacedDto> {
    let service = state.get()?;
    let item = parse_item(&item_id)?;
    let kind = parse_slot(&slot)?;
    if is_extra(kind) {
        return Err(ArtworkCommandError::new(
            "unsupported",
            "Extras are set together; choose them in the list.",
        ));
    }
    let url = source_url(&source)?;
    let outcome = service.place(item, kind, &url, true).await?;
    Ok(placed_dto(&outcome))
}

/// Replaces the extra fanart or the extra thumbs of an item with the chosen images. The old
/// extras go to the trash folder of the data source.
#[tauri::command]
#[specta::specta]
pub async fn place_extra_artwork(
    state: State<'_, ArtworkState>,
    item_id: String,
    slot: String,
    sources: Vec<String>,
) -> CommandResult<ArtworkPlacedDto> {
    let service = state.get()?;
    let item = parse_item(&item_id)?;
    let kind = parse_slot(&slot)?;
    if !is_extra(kind) {
        return Err(ArtworkCommandError::new(
            "unsupported",
            "Only extra fanart and extra thumbs take several images.",
        ));
    }
    let urls = sources
        .iter()
        .map(|source| source_url(source))
        .collect::<Result<Vec<_>, _>>()?;
    let outcome = service.place_extras(item, kind, &urls).await?;
    Ok(placed_dto(&outcome))
}

/// Fills only the empty artwork types of the items from the providers (and the extras and
/// portraits the settings ask for).
#[tauri::command]
#[specta::specta]
pub async fn download_missing_artwork(
    state: State<'_, ArtworkState>,
    item_ids: Vec<String>,
) -> CommandResult<ArtworkRunDto> {
    let service = state.get()?;
    let items = item_ids
        .iter()
        .map(|text| parse_item(text))
        .collect::<CommandResult<Vec<_>>>()?;
    let summary = service.download_missing(&items, false).await;
    Ok(ArtworkRunDto {
        images: small(summary.images()),
        portraits: small(summary.portraits()),
        skipped: small(summary.skipped.len()),
    })
}

/// The step after episodes were scraped: fills the missing season images and episode thumbs of
/// the show of `item_id`, when the after-scrape artwork setting is on. `season` limits it to one
/// season; without it a season or an episode stands for its own season and the show for all of
/// them. Nothing that is there is replaced.
#[tauri::command]
#[specta::specta]
pub async fn fill_season_artwork(
    state: State<'_, ArtworkState>,
    item_id: String,
    season: Option<u32>,
) -> CommandResult<ArtworkRunDto> {
    let service = state.get()?;
    let item = parse_item(&item_id)?;
    let outcome = service.fill_tv_after_scrape(item, season).await?;
    Ok(ArtworkRunDto {
        images: small(outcome.images),
        portraits: small(outcome.portraits),
        skipped: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_artwork::thumb::ThumbKey;

    #[test]
    fn slots_round_trip_by_name() {
        for (name, kind) in SLOTS {
            assert_eq!(parse_slot(name).unwrap(), kind);
            assert_eq!(slot_name(kind), Some(name));
        }
        assert_eq!(parse_slot("trailer").unwrap_err().code, "unsupported");
        assert!(is_extra(parse_slot("extrafanart").unwrap()));
        assert!(!is_extra(parse_slot("poster").unwrap()));
    }

    #[test]
    fn item_ids_are_numbers_as_text() {
        assert_eq!(parse_item(" 42 ").unwrap(), ItemId(42));
        assert_eq!(parse_item("forty-two").unwrap_err().code, "invalid");
    }

    #[test]
    fn errors_carry_a_stable_code_and_a_sentence() {
        let cases = [
            (ArtworkError::ItemNotFound, "not_found"),
            (ArtworkError::Unsupported, "unsupported"),
            (ArtworkError::UnknownSlot, "unsupported"),
            (ArtworkError::NoFolder, "no_folder"),
            (ArtworkError::BadSource("nope".to_owned()), "invalid"),
            (ArtworkError::Worker, "storage"),
        ];
        for (error, code) in cases {
            let message = error.to_string();
            let dto = ArtworkCommandError::from(error);
            assert_eq!(dto.code, code);
            assert_eq!(dto.message, message);
        }
    }

    #[test]
    fn a_candidate_becomes_a_dto_with_a_cache_key_and_never_an_address_for_the_image() {
        let key = ThumbKey::of_bytes(b"picture");
        let candidate = PickerCandidate {
            provider: "tmdb".to_owned(),
            url: "https://img.test/p.jpg".to_owned(),
            language: Some("en".to_owned()),
            width: 2000,
            height: 3000,
            score: 90,
            best: 9_480_000,
            votes: 12,
            rating: 56,
            likes: 0,
            low_quality: false,
            key: key.clone(),
        };

        let dto = candidate_dto(candidate, None);

        assert_eq!(dto.id, "https://img.test/p.jpg");
        assert_eq!(dto.image.key, key.to_string());
        assert_eq!(dto.image.thumbhash, None, "no cache, no preview");
        assert_eq!(dto.score, 90);
        assert_eq!((dto.best, dto.votes, dto.rating), (9_480_000, 12, 56));
        assert!(!dto.low_quality);
    }

    #[test]
    fn what_was_placed_is_counted() {
        let outcome = ArtworkOutcome {
            item: ItemId(1),
            images: 2,
            portraits: 0,
            trashed: 3,
            problems: vec!["extra fanart 3: the server answered 404".to_owned()],
            undo: Some("place-1".to_owned()),
        };
        assert_eq!(
            placed_dto(&outcome),
            ArtworkPlacedDto {
                images: 2,
                problems: vec!["extra fanart 3: the server answered 404".to_owned()],
                undo: Some("place-1".to_owned()),
            }
        );
    }
}

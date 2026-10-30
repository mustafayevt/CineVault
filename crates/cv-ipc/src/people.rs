//! The people of a title, beyond what the detail page stores for them: the portraits of the cast
//! and the IMDb page of one person.
//!
//! Portraits are TMDB profile pictures. They are fetched lazily into the app's thumbnail cache
//! (never into the media folders) when a title's page is opened, and served over `cv://img` like
//! every other image: the portrait files in `.actors`, which the artwork download writes when
//! `artwork.actor-images` is on, are used first (the detail command already returns them). The
//! IMDb ID of a person is read from TMDB when the Cast & crew panel opens and stored with them.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use cv_app::artwork::ArtworkService;
use cv_app::match_service::MatchService;
use cv_artwork::download::actors::portrait_file;
use cv_artwork::thumb::ThumbCache;
use cv_db::Database;
use cv_db::repo::get_item;
use cv_domain::{CreditKind, ItemId};
use serde::Serialize;
use specta::Type;

use crate::library::{LibraryCommandError, PosterDto, blocking, poster_dto};

/// How long opening a page waits for portraits that are not cached yet. The rest keep
/// downloading, and asking again finds them.
const PORTRAIT_WAIT: Duration = Duration::from_millis(2500);

/// The most people of one title whose portraits are fetched.
const MAX_PORTRAITS: usize = 60;

/// A portrait fetched for a person of the credits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct CastPortraitDto {
    /// The person's name as the credits spell it.
    pub name: String,
    pub portrait: PosterDto,
}

/// The portraits of the cast of a title that no file in `.actors` covers, fetched into the
/// thumbnail cache. Empty when the title has no TMDB portraits, or the artwork service is down.
pub async fn get_cast_portraits(
    db: Arc<Database>,
    cache: Option<Arc<ThumbCache>>,
    artwork: Option<Arc<ArtworkService>>,
    id: ItemId,
) -> Result<Vec<CastPortraitDto>, LibraryCommandError> {
    let wanted: Vec<(String, String)> = blocking(move || {
        let Some(item) = db.read(|c| get_item(c, id))? else {
            return Ok(Vec::new());
        };
        let folder = item.core.path.clone();
        let mut seen = std::collections::HashSet::new();
        Ok(item
            .core
            .credits
            .iter()
            .filter(|credit| matches!(credit.kind, CreditKind::Actor | CreditKind::Guest))
            .filter(|credit| !credit.person.thumb_url.trim().is_empty())
            .filter(|credit| seen.insert(credit.person.name.to_lowercase()))
            .filter(|credit| {
                folder.trim().is_empty()
                    || portrait_file(Path::new(&folder), &credit.person.name).is_none()
            })
            .take(MAX_PORTRAITS)
            .map(|credit| (credit.person.name.clone(), credit.person.thumb_url.clone()))
            .collect())
    })
    .await?;
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let keys: HashMap<String, cv_artwork::thumb::ThumbKey> = match artwork {
        Some(artwork) => {
            artwork
                .remote_thumbs(
                    wanted.iter().map(|(_, url)| url.clone()).collect(),
                    PORTRAIT_WAIT,
                )
                .await
        }
        None => HashMap::new(),
    };
    // The portrait files in `.actors` were already checked: every remaining person wants the
    // provider's picture, whose key the artwork service fetched.
    Ok(wanted
        .into_iter()
        .filter_map(|(name, url)| {
            let key = keys.get(&url)?;
            Some(CastPortraitDto {
                name,
                portrait: poster_dto(cache.as_deref(), key),
            })
        })
        .collect())
}

/// The IMDb ID (`nm0000244`) of a person known by their TMDB ID, read from TMDB and stored with
/// the person. `None` when TMDB has none or cannot be asked now; the cast list then opens the
/// person's TMDB page instead.
pub async fn get_person_imdb_id(
    service: Arc<MatchService>,
    tmdb_id: String,
) -> Result<Option<String>, String> {
    service
        .person_imdb_id(&tmdb_id)
        .await
        .map_err(|error| error.to_string())
}

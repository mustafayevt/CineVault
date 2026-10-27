//! Season and episode artwork of a show, written when its artwork is downloaded.
//!
//! Spec: `artwork` (Download missing artwork), `tv-library` (season and episode artwork).
//!
//! # What is written
//!
//! * **Season images.** For every season of the show (the specials included) a poster, fanart,
//!   banner and thumb that is missing is fetched and written to the names of the artwork
//!   settings (`SEASON_POSTER`, `SEASON_FOLDER`, ...; see [`crate::tv_info::season_targets`]),
//!   relative to the show folder. The image comes from the artwork providers that serve the
//!   show (TheTVDB, Fanart.tv and TMDB answer per season), chosen like every other image by the
//!   best-image selection; without a provider answer, or with automatic selection off, the URL
//!   the season remembers from its scrape is used.
//! * **Episode thumbs.** An episode that has a thumb URL (stored when the episode was scraped)
//!   and no thumb file gets the file written to the thumb names of the settings, next to its
//!   video.
//!
//! Every file follows the rules of the other downloads: an image that is there, under any name
//! of its type or extension, is never replaced unless the caller asks, replaced files go to the
//! trash folder, and nothing is written into a folder that is gone. The outcome of each image is
//! recorded on the season or episode itself (its file and its source URL), so a Re-download
//! fetches missing files again from the remembered URLs.
//!
//! # When it runs
//!
//! * with the show's artwork (Download missing artwork, and the step after a show's scrape);
//! * after the episodes of a show or of one season were scraped
//!   ([`ArtworkService::fill_tv_after_scrape`]), so the season page's Scrape fills its season's
//!   images too. Before, only the show's own scrape did, and a season scraped from its page
//!   never got its poster.
//!
//! # A season image chosen by hand
//!
//! The picker and the quick poster overlay work on a season item like on a movie
//! ([`ArtworkService::place`] with the season's ID): the image is written to the season names of
//! the settings, the season's other files of that type move to the trash folder (they are the
//! old image under another name, such as `Season 1/folder.jpg`), and Undo puts them back. The
//! automatic runs only fill gaps, so a chosen image is never replaced by them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_artwork::download::files::{Trash, any_variant_exists};
use cv_artwork::download::{NamedImage, download_named_image};
use cv_artwork::naming::TvNames;
use cv_artwork::select::rank;
use cv_db::repo::artwork_refs::{ArtworkRef, artwork_refs};
use cv_db::repo::{children_of, get_item};
use cv_domain::{Item, ItemDetail, ItemId, MediaFileType};
use cv_providers::artwork::gather_season_artwork;
use cv_providers::model::{MediaKind, RequestOptions};
use cv_providers::{ArtworkProvider, SourcedArtwork};
use cv_rename::naming::SeasonArt;

use super::{ArtworkError, ArtworkOutcome, ArtworkService, Loaded, Run, undo};
use crate::matching::MatchSettings;
use crate::nfo::model::{aired_number, main_video};
use crate::nfo::paths::video_base_name;
use crate::tv_info::season_targets;

/// The season image types, with the artwork type each is recorded under.
const SEASON_TYPES: [(SeasonArt, MediaFileType); 4] = [
    (SeasonArt::Poster, MediaFileType::SeasonPoster),
    (SeasonArt::Fanart, MediaFileType::SeasonFanart),
    (SeasonArt::Banner, MediaFileType::SeasonBanner),
    (SeasonArt::Thumb, MediaFileType::SeasonThumb),
];

/// The season type a picker slot stands for: the plain names (`Poster`) and the season ones
/// (`SeasonPoster`) both work. `None` for a type a season does not have.
#[must_use]
pub(crate) fn season_kind(slot: MediaFileType) -> Option<MediaFileType> {
    Some(match slot {
        MediaFileType::Poster | MediaFileType::SeasonPoster => MediaFileType::SeasonPoster,
        MediaFileType::Fanart | MediaFileType::SeasonFanart => MediaFileType::SeasonFanart,
        MediaFileType::Banner | MediaFileType::SeasonBanner => MediaFileType::SeasonBanner,
        MediaFileType::Thumb | MediaFileType::SeasonThumb => MediaFileType::SeasonThumb,
        _ => return None,
    })
}

fn season_art(kind: MediaFileType) -> Option<SeasonArt> {
    SEASON_TYPES
        .iter()
        .find(|(_, k)| *k == kind)
        .map(|(art, _)| *art)
}

/// The types the picker offers for a season, by their plain names (`Poster` for the season
/// poster): the ones the naming settings write somewhere.
#[must_use]
pub(crate) fn season_slots(names: &TvNames) -> Vec<MediaFileType> {
    [
        (SeasonArt::Poster, MediaFileType::Poster),
        (SeasonArt::Fanart, MediaFileType::Fanart),
        (SeasonArt::Banner, MediaFileType::Banner),
        (SeasonArt::Thumb, MediaFileType::Thumb),
    ]
    .into_iter()
    .filter(|(art, _)| !names.season_choices(*art).is_empty())
    .map(|(_, slot)| slot)
    .collect()
}

/// How the picker names a season: `Severance · Season 2`, `Severance · Specials`.
#[must_use]
pub(crate) fn season_title(show: &Item, number: u32) -> String {
    let season = if number == 0 {
        "Specials".to_owned()
    } else {
        format!("Season {number}")
    };
    format!("{} · {season}", show.core.title)
}

/// A season with what writing its images needs: its show (with the show's folder) and the
/// folder its episodes sit in.
pub(super) struct SeasonContext {
    pub(super) id: ItemId,
    pub(super) item: Item,
    pub(super) number: u32,
    pub(super) show: Loaded,
    /// The season folder relative to the show folder, empty when the episodes sit in the show
    /// folder.
    pub(super) folder: String,
    /// The season's remembered image sources.
    pub(super) recorded: Vec<ArtworkRef>,
}

impl SeasonContext {
    /// The season as an item whose images live in its show's folder, for Undo.
    pub(super) fn as_loaded(&self) -> Loaded {
        Loaded {
            item: self.item.clone(),
            media: MediaKind::Show,
            target: self.show.target.clone(),
            recorded: self.recorded.clone(),
        }
    }
}

/// Which parts of a show a run works on.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct TvParts<'a> {
    /// The artwork providers to ask, by key; empty asks every ready one.
    pub(super) only: &'a [String],
    /// One season (and its episodes) only; `None` for all of them.
    pub(super) season: Option<u32>,
}

/// The seasons and episodes of a show, as one read of the database gives them.
struct ShowTree {
    seasons: Vec<(ItemId, Item)>,
    episodes: Vec<(ItemId, Item)>,
}

/// The name of the season folder, relative to the show folder with `/`; empty when the season's
/// episodes sit in the show folder (or somewhere that is not below it).
fn season_folder(show_dir: &Path, episodes: &[&Item]) -> String {
    let mut dirs: Vec<PathBuf> = episodes
        .iter()
        .filter_map(|e| main_video(e))
        .map(|v| PathBuf::from(&v.directory))
        .collect();
    dirs.sort();
    let Some(dir) = dirs.into_iter().next() else {
        return String::new();
    };
    below_show(show_dir, &dir)
}

/// `dir` relative to the show folder with `/`; empty when it is the show folder or not below it.
/// Case counts only where the platform's file names keep it (Linux).
fn below_show(show_dir: &Path, dir: &Path) -> String {
    if !cv_fs::paths::is_within(dir, show_dir) || cv_fs::paths::same_path(dir, show_dir) {
        return String::new();
    }
    // The original spelling, not the comparison key.
    let skip = show_dir.components().count();
    dir.components()
        .skip(skip)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether the item has a file of `kind` on disk.
fn has_file(item: &Item, kind: MediaFileType) -> bool {
    item.core
        .media_files
        .iter()
        .any(|f| f.kind == kind && f.path().is_file())
}

impl ArtworkService {
    /// Writes the missing season images and episode thumbs of a show. `replace` writes every
    /// image again. Locked seasons and episodes are left alone when `skip_locked` is set. Returns
    /// what was written and what could not be.
    pub(super) async fn run_tv_parts(
        &self,
        id: ItemId,
        loaded: &Loaded,
        run: Run,
        replace: bool,
        skip_locked: bool,
        parts: TvParts<'_>,
    ) -> Result<ArtworkOutcome, ArtworkError> {
        let (only, only_season) = (parts.only, parts.season);
        let mut outcome = ArtworkOutcome::nothing(id);
        let tree = self.load_tree(id).await?;
        let config = self.config();
        let names: &TvNames = &config.names.tv;
        let target = &loaded.target;
        let show_dir = target.dir.as_path();
        let data_source = target.data_source.as_deref();
        let providers = if run == Run::Missing && config.download.auto_select {
            self.ready_providers(MediaKind::Show, only)
        } else {
            Vec::new()
        };
        let options: RequestOptions =
            MatchSettings::from_values(&self.settings.snapshot().values).request;

        for (season_id, season) in &tree.seasons {
            let ItemDetail::Season(fields) = &season.detail else {
                continue;
            };
            if skip_locked && season.core.locked {
                continue;
            }
            let number = fields.season;
            if only_season.is_some_and(|wanted| wanted != number) {
                continue;
            }
            let episodes: Vec<&Item> = tree
                .episodes
                .iter()
                .filter(|(_, e)| aired_number(e).is_some_and(|(s, _)| s == number))
                .map(|(_, e)| e)
                .collect();
            let folder = season_folder(show_dir, &episodes);
            let mut wanted: Vec<(SeasonArt, MediaFileType)> = Vec::new();
            for (art, kind) in SEASON_TYPES {
                if names.season_choices(art).is_empty() {
                    continue;
                }
                let probe = season_targets(
                    names.season_choices(art),
                    art,
                    i64::from(number),
                    &folder,
                    "jpg",
                );
                let on_disk = !probe.is_empty()
                    && probe.iter().all(|n| {
                        let mut path = show_dir.to_path_buf();
                        path.extend(n.split('/'));
                        any_variant_exists(&path)
                    });
                let present = has_file(season, kind) || on_disk;
                if replace || !present {
                    wanted.push((art, kind));
                }
            }
            if wanted.is_empty() {
                continue;
            }

            // One round of provider answers serves every type of the season.
            let candidates = self
                .season_candidates(&providers, &loaded.item, number, &options)
                .await;
            for (art, kind) in wanted {
                let url = rank(&candidates, kind, &config.selection)
                    .first()
                    .map(|c| c.candidate.url.clone())
                    .or_else(|| season.core.artwork_urls.get(&kind).cloned())
                    .filter(|u| !u.trim().is_empty());
                let Some(url) = url else {
                    continue;
                };
                let what = format!("season {number} {}", kind_word(kind));
                let list = |ext: &str| {
                    season_targets(
                        names.season_choices(art),
                        art,
                        i64::from(number),
                        &folder,
                        ext,
                    )
                };
                let report = download_named_image(
                    self.fetcher.as_ref(),
                    &NamedImage {
                        what: &what,
                        kind,
                        url: &url,
                        base: show_dir,
                        names: &list,
                        data_source,
                        replace,
                    },
                )
                .await;
                self.record(*season_id, &report).await?;
                add(&mut outcome, &report);
            }
        }

        // Episode thumbs come from the URL the episode scrape stored.
        for (episode_id, episode) in &tree.episodes {
            if skip_locked && episode.core.locked {
                continue;
            }
            if let Some(wanted) = only_season
                && aired_number(episode).is_none_or(|(s, _)| s != wanted)
            {
                continue;
            }
            let Some(url) = episode
                .core
                .artwork_urls
                .get(&MediaFileType::Thumb)
                .filter(|u| !u.trim().is_empty())
            else {
                continue;
            };
            let Some(video) = main_video(episode) else {
                continue;
            };
            let base = video_base_name(&video.filename);
            if base.is_empty() {
                continue;
            }
            let dir = PathBuf::from(&video.directory);
            let list = |ext: &str| names.episode_thumb_targets(&base, ext);
            let present = has_file(episode, MediaFileType::Thumb)
                || list("jpg")
                    .iter()
                    .any(|name| any_variant_exists(&dir.join(name)));
            if present && !replace {
                continue;
            }
            let what = format!("{base} thumb");
            let report = download_named_image(
                self.fetcher.as_ref(),
                &NamedImage {
                    what: &what,
                    kind: MediaFileType::Thumb,
                    url,
                    base: &dir,
                    names: &list,
                    data_source,
                    replace,
                },
            )
            .await;
            self.record(*episode_id, &report).await?;
            add(&mut outcome, &report);
        }
        Ok(outcome)
    }

    /// The step after the episodes of a show were scraped (the show page's Scrape episodes, the
    /// season page's Scrape, one episode): when the after-scrape setting is on, fills the missing
    /// season images and episode thumbs of the show, of season `season` only when it is given.
    /// `item` is the show or any of its seasons or episodes; without `season`, a season or an
    /// episode stands for its own season and the show for all of them. A locked show or season is
    /// left alone, as the show's own after-scrape step leaves it.
    ///
    /// # Errors
    /// The show cannot be used for artwork (no folder, gone).
    pub async fn fill_tv_after_scrape(
        &self,
        item: ItemId,
        season: Option<u32>,
    ) -> Result<ArtworkOutcome, ArtworkError> {
        if !self
            .after_scrape_runs
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Ok(ArtworkOutcome::nothing(item));
        }
        let db = Arc::clone(&self.db);
        let (show, own_season) = tokio::task::spawn_blocking(move || {
            let show = crate::title_guard::title_of(&db, item)?;
            let own = db
                .read(|c| get_item(c, item))?
                .and_then(|found| match &found.detail {
                    ItemDetail::Season(fields) => Some(fields.season),
                    ItemDetail::Episode(_) => aired_number(&found).map(|(s, _)| s),
                    _ => None,
                });
            Ok::<_, cv_db::DbError>((show, own))
        })
        .await
        .map_err(|_| ArtworkError::Worker)??;
        let season = season.or(own_season);
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| ArtworkError::Worker)?;
        let _title = self.hold(show).await?;
        let loaded = self.load(show).await?;
        if loaded.media != MediaKind::Show {
            return Err(ArtworkError::Unsupported);
        }
        if loaded.item.core.locked {
            return Ok(ArtworkOutcome::nothing(show));
        }
        let outcome = self
            .run_tv_parts(
                show,
                &loaded,
                Run::Missing,
                false,
                true,
                TvParts { only: &[], season },
            )
            .await?;
        for problem in &outcome.problems {
            tracing::debug!(show = show.0, %problem, "season artwork problem");
        }
        Ok(outcome)
    }

    /// The season `id` with its show and folders, or `None` when `id` is not a season.
    ///
    /// # Errors
    /// The season's show is gone or has no folder.
    pub(super) async fn season_context(
        &self,
        id: ItemId,
    ) -> Result<Option<SeasonContext>, ArtworkError> {
        let db = Arc::clone(&self.db);
        let found = tokio::task::spawn_blocking(move || {
            db.read(|c| {
                let Some(item) = get_item(c, id)? else {
                    return Ok(None);
                };
                let recorded = artwork_refs(c, id)?;
                Ok(Some((item, recorded)))
            })
        })
        .await
        .map_err(|_| ArtworkError::Worker)??;
        let Some((item, recorded)) = found else {
            return Ok(None);
        };
        let ItemDetail::Season(fields) = &item.detail else {
            return Ok(None);
        };
        let number = fields.season;
        let show_id = item.parent_id.ok_or(ArtworkError::ItemNotFound)?;
        let show = self.load(show_id).await?;
        let tree = self.load_tree(show_id).await?;
        let episodes: Vec<&Item> = tree
            .episodes
            .iter()
            .filter(|(_, e)| aired_number(e).is_some_and(|(s, _)| s == number))
            .map(|(_, e)| e)
            .collect();
        let folder = season_folder(&show.target.dir, &episodes);
        Ok(Some(SeasonContext {
            id,
            item,
            number,
            show,
            folder,
            recorded,
        }))
    }

    /// Sets one image of a season from `url` (a candidate, a pasted address, or a dropped file
    /// as a `file:` URL), written to the season names of the settings. With `replace`, the files
    /// at those names and the season's other files of the type go to the trash folder, once the
    /// new image is written; Undo puts them back. The caller holds the title.
    pub(super) async fn place_season(
        &self,
        season: SeasonContext,
        slot: MediaFileType,
        url: &str,
        replace: bool,
    ) -> Result<ArtworkOutcome, ArtworkError> {
        let kind = season_kind(slot).ok_or(ArtworkError::UnknownSlot)?;
        let art = season_art(kind).ok_or(ArtworkError::UnknownSlot)?;
        let config = self.config();
        let choices = config.names.tv.season_choices(art);
        if choices.is_empty() {
            return Err(ArtworkError::UnknownSlot);
        }
        let show_dir = season.show.target.dir.clone();
        let data_source = season.show.target.data_source.clone();
        let number = season.number;
        let folder = season.folder.clone();
        let list = |ext: &str| season_targets(choices, art, i64::from(number), &folder, ext);
        let what = format!("season {number} {}", kind_word(kind));
        let mut report = download_named_image(
            self.fetcher.as_ref(),
            &NamedImage {
                what: &what,
                kind,
                url,
                base: &show_dir,
                names: &list,
                data_source: data_source.as_deref(),
                replace,
            },
        )
        .await;

        // The season's image under a name the settings do not write is the old image too: it
        // gives way once the new one is in place, so pages and media centres show the new one.
        let targets: Vec<String> = list("jpg")
            .iter()
            .map(|name| without_extension(&join(&show_dir, name)))
            .collect();
        let others: Vec<PathBuf> = season
            .item
            .core
            .media_files
            .iter()
            .filter(|f| f.kind == kind)
            .map(cv_domain::MediaFile::path)
            .filter(|p| p.is_file() && !targets.contains(&without_extension(p)))
            .collect();
        if replace && !report.images.is_empty() && !others.is_empty() {
            let trash = Trash::new(data_source.as_deref(), &show_dir);
            let (moved, moves) = tokio::task::spawn_blocking(move || {
                let mut moved = Vec::new();
                for path in others {
                    match trash.put(&path) {
                        Ok(_) => moved.push(path),
                        Err(error) => tracing::warn!(
                            path = %path.display(), %error, "old season image not moved"
                        ),
                    }
                }
                (moved, trash.moves())
            })
            .await
            .map_err(|_| ArtworkError::Worker)?;
            report.trashed.extend(moved);
            report.trash_moves.extend(moves);
        }
        self.record(season.id, &report).await?;
        let mut outcome = ArtworkOutcome::of(season.id, &report);
        outcome.undo = undo::remember(
            &self.db,
            season.id,
            &season.as_loaded(),
            kind,
            &report,
            &self.deleted_files(),
        );
        Ok(outcome)
    }

    /// What the providers offer for one season of the show; nothing when none is asked.
    async fn season_candidates(
        &self,
        providers: &[Arc<dyn ArtworkProvider>],
        show: &Item,
        season: u32,
        options: &RequestOptions,
    ) -> Vec<SourcedArtwork> {
        if providers.is_empty() || show.core.ids.is_empty() {
            return Vec::new();
        }
        let gathered = gather_season_artwork(providers, &show.core.ids, season, options).await;
        for (provider, error) in &gathered.failures {
            tracing::debug!(provider, season, %error, "season artwork provider failed");
        }
        gathered.candidates
    }

    async fn load_tree(&self, show: ItemId) -> Result<ShowTree, ArtworkError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            db.read(|conn| {
                let mut tree = ShowTree {
                    seasons: Vec::new(),
                    episodes: Vec::new(),
                };
                for child in children_of(conn, show)? {
                    let Some(item) = get_item(conn, child)? else {
                        continue;
                    };
                    match item.detail {
                        ItemDetail::Season(_) => tree.seasons.push((child, item)),
                        ItemDetail::Episode(_) => tree.episodes.push((child, item)),
                        _ => {}
                    }
                }
                tree.seasons.sort_by_key(|(_, s)| match &s.detail {
                    ItemDetail::Season(f) => f.season,
                    _ => 0,
                });
                Ok(tree)
            })
            .map_err(ArtworkError::from)
        })
        .await
        .map_err(|_| ArtworkError::Worker)?
    }
}

fn join(base: &Path, relative: &str) -> PathBuf {
    let mut path = base.to_path_buf();
    path.extend(relative.split('/').filter(|part| !part.is_empty()));
    path
}

/// The path without its extension and with `/`, for comparing file names: lower case where the
/// platform's file names ignore case, as written on Linux.
fn without_extension(path: &Path) -> String {
    cv_fs::paths::fold_name(&path.with_extension("").to_string_lossy().replace('\\', "/"))
}

fn kind_word(kind: MediaFileType) -> &'static str {
    match kind {
        MediaFileType::SeasonPoster => "poster",
        MediaFileType::SeasonFanart => "fanart",
        MediaFileType::SeasonBanner => "banner",
        _ => "thumb",
    }
}

/// Adds what a report says to the show's outcome.
fn add(outcome: &mut ArtworkOutcome, report: &cv_artwork::download::DownloadReport) {
    outcome.images += report.images.len();
    outcome.trashed += report.trashed.len();
    outcome.problems.extend(
        report
            .failures
            .iter()
            .map(|f| format!("{}: {}", f.what, f.reason)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_season_folder_is_found_below_the_show_folder_by_the_platforms_case_rules() {
        let show = Path::new("/tv/Show");
        assert_eq!(
            below_show(show, Path::new("/tv/Show/Season 01")),
            "Season 01"
        );
        assert_eq!(below_show(show, Path::new("/tv/Show")), "");
        assert_eq!(below_show(show, Path::new("/tv/Show2/Season 01")), "");
        // `/tv/show` is the same folder where case is folded, and another one on Linux.
        let expected = if cv_fs::paths::CASE_SENSITIVE {
            ""
        } else {
            "Season 01"
        };
        assert_eq!(below_show(show, Path::new("/tv/show/Season 01")), expected);
    }
}

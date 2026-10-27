//! NFO preview: the text a Rewrite NFO would write for a movie, without writing anything.
//!
//! The detail page shows it next to a note on whether the NFO on disk already says the same. The
//! text is rendered exactly as Rewrite NFO renders it (same model, same carried-over values from
//! the old file, same format), so what the user reads is what a rewrite would produce.

use std::fs;
use std::path::PathBuf;

use cv_db::repo::{children_of, get_item};
use cv_domain::{FieldSet, ItemDetail, ItemId};
use cv_nfo::{
    MovieConnector, NfoOwnership, TvConnector, apply_old_episode_nfo, apply_old_movie_nfo,
    apply_old_show_nfo, classify_nfo_file, content_differs, decode_nfo_bytes, render_episode_nfo,
    render_movie_nfo_for, render_show_nfo,
};

use super::model::{ShowTree, episode_model, main_video, movie_model, show_model, shown_number};
use super::paths::{NfoTargets, episode_targets, movie_targets, show_targets};
use super::service::{NfoError, NfoService, old_text};
use super::settings::NfoSettings;

/// How the NFO on disk compares with the rendered text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NfoSync {
    /// The file holds the same content (the header comment is ignored).
    InSync,
    /// The file is CineVault's but says something else.
    Differs,
    /// There is no NFO file yet.
    Missing,
    /// A file with the NFO's name exists but is not CineVault's; a rewrite would leave it alone.
    NotManaged,
    /// The movie has no place for an NFO (no video file and no folder).
    NoTarget,
}

/// The NFO a movie would get.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NfoPreview {
    /// The rendered NFO, with LF line ends.
    pub text: String,
    /// The format's name: `Kodi`, `XBMC`, `Emby`, `Jellyfin` or `MediaPortal`.
    pub format: &'static str,
    /// The first file a rewrite would write, when there is one.
    pub path: Option<PathBuf>,
    pub sync: NfoSync,
    /// What the file on disk says now (LF line ends), when the file is CineVault's and could be
    /// read. The page shows its difference from `text` ("Show diff").
    pub file_text: Option<String>,
}

/// The display name of a movie NFO format.
pub fn format_name(connector: MovieConnector) -> &'static str {
    match connector {
        MovieConnector::Kodi => "Kodi",
        MovieConnector::Xbmc => "XBMC",
        MovieConnector::Emby => "Emby",
        MovieConnector::Jellyfin => "Jellyfin",
        MovieConnector::MediaPortal(_) => "MediaPortal",
    }
}

/// The display name of a TV NFO format.
pub fn tv_format_name(connector: TvConnector) -> &'static str {
    match connector {
        TvConnector::Kodi => "Kodi",
        TvConnector::Xbmc => "XBMC",
        TvConnector::Emby => "Emby",
        TvConnector::Jellyfin => "Jellyfin",
        TvConnector::MediaPortal => "MediaPortal",
    }
}

/// How the NFO file at `path` compares with the rendered text, and what the file says when it is
/// CineVault's and readable.
fn sync_of(path: Option<&std::path::Path>, rendered: &str) -> (NfoSync, Option<String>) {
    let Some(path) = path else {
        return (NfoSync::NoTarget, None);
    };
    match classify_nfo_file(path) {
        Ok(NfoOwnership::Missing) => (NfoSync::Missing, None),
        Ok(NfoOwnership::Managed) => match fs::read(path) {
            Ok(bytes) => {
                let existing = decode_nfo_bytes(&bytes);
                let sync = if content_differs(&existing, rendered) {
                    NfoSync::Differs
                } else {
                    NfoSync::InSync
                };
                (sync, Some(existing.replace("\r\n", "\n")))
            }
            Err(_) => (NfoSync::Differs, None),
        },
        Ok(_) => (NfoSync::NotManaged, None),
        Err(_) => (NfoSync::Differs, None),
    }
}

/// The preview of a rendered TV NFO.
fn tv_preview(connector: TvConnector, targets: Option<&NfoTargets>, rendered: &str) -> NfoPreview {
    let path = targets.and_then(|t| t.write.first().cloned());
    let (sync, file_text) = sync_of(path.as_deref(), rendered);
    NfoPreview {
        text: rendered.replace("\r\n", "\n"),
        format: tv_format_name(connector),
        sync,
        file_text,
        path,
    }
}

impl NfoService {
    /// Renders the NFO of a movie as Rewrite NFO would and compares it with the file on disk.
    /// Returns `None` when `id` is not a movie. Nothing is written.
    ///
    /// # Errors
    /// Fails when the library cannot be read.
    pub fn preview_movie(
        &self,
        id: ItemId,
        settings: &NfoSettings,
    ) -> Result<Option<NfoPreview>, NfoError> {
        let Some((item, set)) = self.load_movie(id)? else {
            return Ok(None);
        };
        let options = settings.movie.clone();
        let Some(mut movie) = movie_model(&item, set.as_ref(), &options) else {
            return Ok(None);
        };
        movie.locked_fields = self
            .locks_of(&[id])?
            .get(&id)
            .map(FieldSet::keys)
            .unwrap_or_default();
        let targets = movie_targets(&item, settings);
        if let Some(old) = targets.as_ref().and_then(old_text) {
            apply_old_movie_nfo(&mut movie, &old, &options);
        }
        let rendered = render_movie_nfo_for(settings.movie_connector, &movie, &options);
        let path = targets.as_ref().and_then(|t| t.write.first().cloned());
        let (sync, file_text) = sync_of(path.as_deref(), &rendered);
        Ok(Some(NfoPreview {
            text: rendered.replace("\r\n", "\n"),
            format: format_name(settings.movie_connector),
            path,
            sync,
            file_text,
        }))
    }

    /// Renders the `tvshow.nfo` a rewrite would write for a show and compares it with the file on
    /// disk. Returns `None` when `id` is not a show. Nothing is written.
    ///
    /// # Errors
    /// Fails when the library cannot be read.
    pub fn preview_show(
        &self,
        id: ItemId,
        settings: &NfoSettings,
    ) -> Result<Option<NfoPreview>, NfoError> {
        let Some(data) = self.load_show(id)? else {
            return Ok(None);
        };
        let tv = &settings.tv;
        let tree = ShowTree {
            show: &data.show,
            seasons: &data.seasons,
            episodes: &data.episodes,
        };
        let Some(mut show) = show_model(&tree, &tv.common) else {
            return Ok(None);
        };
        show.locked_fields = self
            .locks_of(&[id])?
            .get(&id)
            .map(FieldSet::keys)
            .unwrap_or_default();
        let targets = show_targets(&data.show, settings);
        if let Some(old) = targets.as_ref().and_then(old_text) {
            apply_old_show_nfo(&mut show, &old, &tv.common);
        }
        let rendered = render_show_nfo(settings.tv_connector, &show, tv);
        Ok(Some(tv_preview(
            settings.tv_connector,
            targets.as_ref(),
            &rendered,
        )))
    }

    /// Renders the NFO a rewrite would write for the video file an episode sits in (the episodes
    /// sharing that file are in the same document) and compares it with the file on disk. Returns
    /// `None` when `id` is not an episode of a show. Nothing is written.
    ///
    /// # Errors
    /// Fails when the library cannot be read.
    pub fn preview_episode(
        &self,
        id: ItemId,
        settings: &NfoSettings,
    ) -> Result<Option<NfoPreview>, NfoError> {
        let loaded = self.db.read(|c| {
            let Some(episode) = get_item(c, id)? else {
                return Ok(None);
            };
            if !matches!(episode.detail, ItemDetail::Episode(_)) {
                return Ok(None);
            }
            let Some(show) = episode
                .parent_id
                .map(|p| get_item(c, p))
                .transpose()?
                .flatten()
            else {
                return Ok(None);
            };
            if !matches!(show.detail, ItemDetail::Show(_)) {
                return Ok(None);
            }
            let file = main_video(&episode).map(|v| v.path());
            let mut group = vec![episode];
            if let (Some(file), Some(show_id)) = (file, show.id) {
                for sibling in children_of(c, show_id)? {
                    if sibling == id {
                        continue;
                    }
                    if let Some(other) = get_item(c, sibling)?
                        && matches!(other.detail, ItemDetail::Episode(_))
                        && main_video(&other).is_some_and(|v| v.path() == file)
                    {
                        group.push(other);
                    }
                }
            }
            Ok(Some((show, group)))
        })?;
        let Some((show, mut group)) = loaded else {
            return Ok(None);
        };
        let ItemDetail::Show(fields) = &show.detail else {
            return Ok(None);
        };
        group.sort_by_key(|e| shown_number(e, &fields.episode_group));
        let tv = &settings.tv;
        let mut models: Vec<_> = group
            .iter()
            .filter_map(|e| episode_model(&show, e, &tv.common))
            .collect();
        if models.is_empty() {
            return Ok(None);
        }
        let targets = group.first().and_then(episode_targets);
        if let Some(old) = targets.as_ref().and_then(old_text) {
            apply_old_episode_nfo(&mut models, &old, &tv.common);
        }
        let rendered = render_episode_nfo(settings.tv_connector, &models, tv);
        Ok(Some(tv_preview(
            settings.tv_connector,
            targets.as_ref(),
            &rendered,
        )))
    }
}

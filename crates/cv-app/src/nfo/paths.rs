//! Where the NFO files of an item go, and which files could be an item's own.
//!
//! Every function returns [`NfoTargets`]: the paths to write (the configured naming choices) and
//! the **candidates**, every path the item could have written under any naming choice or before a
//! rename. Candidates are what a rewrite reads the old NFO from, and the only files cleanup may
//! remove (and only when they are managed, see `cv_nfo::ownership`). A file that is not a
//! candidate, such as a release group's `info.nfo`, is never looked at.

use std::path::{Path, PathBuf};

use cv_domain::{Item, ItemDetail, MediaFile};
use cv_nfo::{
    MovieNameContext, SeasonNfoNaming, movie_nfo_file_names, movie_set_file_name_with,
    movie_set_folder_name_with, movie_set_nfo_paths_for, season_nfo_paths, show_nfo_file_names,
};
use cv_scan::stacking::parse_file_name;

use super::model::main_video;
use super::settings::NfoSettings;

/// The paths of one NFO group (a movie, a show, a season, a video file with episodes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NfoTargets {
    /// Where the configured naming choices put the NFO, all with the same content.
    pub write: Vec<PathBuf>,
    /// Every path that could be this item's own NFO; contains all of `write`.
    pub candidates: Vec<PathBuf>,
}

impl NfoTargets {
    fn new(write: Vec<PathBuf>, extra: Vec<PathBuf>) -> Self {
        let mut candidates: Vec<PathBuf> = Vec::new();
        for path in write.iter().chain(extra.iter()) {
            push_unique(&mut candidates, path.clone());
        }
        let mut unique_write: Vec<PathBuf> = Vec::new();
        for path in write {
            push_unique(&mut unique_write, path);
        }
        Self {
            write: unique_write,
            candidates,
        }
    }

    /// The candidates that are not written any more.
    pub fn others(&self) -> impl Iterator<Item = &PathBuf> {
        self.candidates
            .iter()
            .filter(|c| !self.write.iter().any(|w| same(w, c)))
    }
}

fn same(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

fn push_unique(list: &mut Vec<PathBuf>, path: PathBuf) {
    if !list.iter().any(|p| same(p, &path)) {
        list.push(path);
    }
}

/// A file name without its extension.
fn stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() && !ext.contains(' ') && ext.len() <= 5 => base,
        _ => name,
    }
}

/// The video's name without extension and without a stacking marker (`Heat cd1.mkv` is `Heat`).
pub fn video_base_name(filename: &str) -> String {
    let cleaned = parse_file_name(filename).cleaned;
    stem(&cleaned).trim().to_owned()
}

/// The names before the renamer last renamed this item's files, as base names.
fn old_base_names(item: &Item) -> Vec<String> {
    item.core
        .rename_history
        .iter()
        .flat_map(|h| h.files.iter())
        .map(|step| video_base_name(&step.old_filename))
        .filter(|n| !n.is_empty())
        .collect()
}

fn item_folder(item: &Item, video: Option<&MediaFile>) -> Option<PathBuf> {
    let own = item.core.path.trim();
    if own.is_empty() {
        video.map(|v| PathBuf::from(&v.directory))
    } else {
        Some(PathBuf::from(own))
    }
}

/// The NFO paths of a movie. `None` when the movie has no video file and no folder.
pub fn movie_targets(item: &Item, settings: &NfoSettings) -> Option<NfoTargets> {
    let ItemDetail::Movie(m) = &item.detail else {
        return None;
    };
    let video = main_video(item);
    let root = item_folder(item, video)?;
    let video_dir = video.map_or_else(|| root.clone(), |v| PathBuf::from(&v.directory));
    let filename = video.map_or("", |v| v.filename.as_str());
    let base = video_base_name(filename);
    let disc_base = stem(filename).to_owned();
    if base.is_empty() && !m.is_disc {
        return None;
    }
    let context = MovieNameContext {
        video_base_name: base.clone(),
        is_disc: m.is_disc,
        disc_base_name: disc_base.clone(),
        multi_movie_dir: m.multi_movie_dir,
        nfo_in_disc_folder: settings.nfo_in_disc_folder,
    };
    let disc_file = format!("{disc_base}.nfo");
    let place = |name: &str| -> PathBuf {
        // A disc's own NFO sits in the disc folder when asked to; `movie.nfo` stays at the top.
        if m.is_disc && !(settings.nfo_in_disc_folder && name == disc_file) {
            root.join(name)
        } else {
            video_dir.join(name)
        }
    };
    let write: Vec<PathBuf> = movie_nfo_file_names(&settings.movie_namings, &context)
        .iter()
        .map(|name| place(name))
        .collect();

    let mut extra: Vec<PathBuf> = Vec::new();
    if !base.is_empty() {
        extra.push(video_dir.join(format!("{base}.nfo")));
    }
    if !m.multi_movie_dir {
        extra.push(video_dir.join("movie.nfo"));
        extra.push(root.join("movie.nfo"));
    }
    if m.is_disc {
        extra.push(video_dir.join(&disc_file));
    }
    for old in old_base_names(item) {
        extra.push(video_dir.join(format!("{old}.nfo")));
    }
    Some(NfoTargets::new(write, extra))
}

/// The NFO paths of a movie set. `None` without a collection data folder.
pub fn set_targets(item: &Item, settings: &NfoSettings) -> Option<NfoTargets> {
    if !matches!(item.detail, ItemDetail::Set(_)) {
        return None;
    }
    let root = settings.collection_folder.as_ref()?;
    let title = item.core.title.as_str();
    let replacement = settings.set_name_replacement.as_str();
    let file = movie_set_file_name_with(title, replacement);
    if file.is_empty() {
        return None;
    }
    let tmdb = item.core.ids.get("tmdb");
    let join = |relative: &str| {
        let mut path = root.clone();
        path.extend(relative.split('/'));
        path
    };
    let folder = movie_set_folder_name_with(
        title,
        tmdb,
        settings.append_tmdb_id_to_set_folder,
        replacement,
    );
    let write: Vec<PathBuf> = movie_set_nfo_paths_for(&settings.set_namings, &file, &folder)
        .iter()
        .map(|p| join(p))
        .collect();

    // Every naming choice, with and without the TMDB ID in the folder name.
    use cv_nfo::MovieSetNfoNaming::{Automator, Emby, Kodi, Kodi2};
    let all = [Kodi, Kodi2, Automator, Emby];
    let mut extra: Vec<PathBuf> = Vec::new();
    for append in [false, true] {
        let folder = movie_set_folder_name_with(title, tmdb, append, replacement);
        extra.extend(
            movie_set_nfo_paths_for(&all, &file, &folder)
                .iter()
                .map(|p| join(p)),
        );
    }
    Some(NfoTargets::new(write, extra))
}

/// The NFO paths of a show: `tvshow.nfo` in the show folder. `None` without a folder.
pub fn show_targets(item: &Item, settings: &NfoSettings) -> Option<NfoTargets> {
    if !matches!(item.detail, ItemDetail::Show(_)) {
        return None;
    }
    let root = PathBuf::from(item.core.path.trim());
    if root.as_os_str().is_empty() {
        return None;
    }
    let write: Vec<PathBuf> = show_nfo_file_names(&settings.show_namings)
        .iter()
        .map(|n| root.join(n))
        .collect();
    Some(NfoTargets::new(write, Vec::new()))
}

/// The NFO paths of one season of a show.
///
/// `season_folder` is the folder holding the season's episodes, relative to the show folder (empty
/// when they sit in it).
pub fn season_targets(
    show_folder: &Path,
    season: u32,
    season_folder: Option<&str>,
    settings: &NfoSettings,
) -> NfoTargets {
    let join = |relative: String| {
        let mut path = show_folder.to_path_buf();
        path.extend(relative.split('/'));
        path
    };
    let write: Vec<PathBuf> =
        season_nfo_paths(&settings.season_namings, season, season_folder, true)
            .into_iter()
            .map(join)
            .collect();
    let mut extra: Vec<PathBuf> = Vec::new();
    for special_name in [true, false] {
        extra.extend(
            season_nfo_paths(
                &[SeasonNfoNaming::Season, SeasonNfoNaming::SeasonFolder],
                season,
                season_folder,
                special_name,
            )
            .into_iter()
            .map(join),
        );
    }
    NfoTargets::new(write, extra)
}

/// The NFO path of the video file an episode (or several) sits in, plus names from before a
/// rename. `None` when the episode has no video file.
pub fn episode_targets(episode: &Item) -> Option<NfoTargets> {
    let video = main_video(episode)?;
    let dir = PathBuf::from(&video.directory);
    let base = video_base_name(&video.filename);
    if base.is_empty() {
        return None;
    }
    let write = vec![dir.join(format!("{base}.nfo"))];
    let extra = old_base_names(episode)
        .into_iter()
        .map(|old| dir.join(format!("{old}.nfo")))
        .collect();
    Some(NfoTargets::new(write, extra))
}

/// The folder of a season's episodes relative to the show folder, with `/` separators. `None`
/// when they sit in the show folder itself or outside it.
pub fn relative_folder(show_folder: &Path, folder: &Path) -> Option<String> {
    let relative = folder.strip_prefix(show_folder).ok()?;
    let text: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!text.is_empty()).then(|| text.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_domain::{MediaFileType, MovieFields, RenameHistory, RenameStep};
    use cv_nfo::MovieNfoNaming;

    fn movie(dir: &str, file: &str) -> Item {
        let mut item = Item::movie("Heat");
        item.core.path = dir.to_owned();
        item.core
            .media_files
            .push(MediaFile::new(MediaFileType::Video, dir, file));
        item
    }

    fn s(names: &[MovieNfoNaming]) -> NfoSettings {
        NfoSettings {
            movie_namings: names.to_vec(),
            ..NfoSettings::default()
        }
    }

    #[test]
    fn base_names_drop_the_extension_and_the_stacking_marker() {
        assert_eq!(video_base_name("Heat (1995).mkv"), "Heat (1995)");
        assert_eq!(video_base_name("Heat (1995) cd1.avi"), "Heat (1995)");
        assert_eq!(video_base_name("Heat (1995)-part2.mkv"), "Heat (1995)");
        assert_eq!(video_base_name("v1.2 release"), "v1.2 release");
    }

    #[test]
    fn movies_write_the_chosen_names_and_know_every_candidate() {
        let item = movie("D:/Movies/Heat (1995)", "Heat (1995).mkv");
        let t = movie_targets(&item, &s(&[MovieNfoNaming::MovieNfo])).unwrap();
        assert_eq!(t.write, [PathBuf::from("D:/Movies/Heat (1995)/movie.nfo")]);
        assert_eq!(
            t.others().collect::<Vec<_>>(),
            [&PathBuf::from("D:/Movies/Heat (1995)/Heat (1995).nfo")]
        );
        let both = movie_targets(
            &item,
            &s(&[MovieNfoNaming::VideoBase, MovieNfoNaming::MovieNfo]),
        )
        .unwrap();
        assert_eq!(both.write.len(), 2);
        assert_eq!(both.others().count(), 0);
        // A release note next to the video is never a candidate.
        assert!(
            !both
                .candidates
                .iter()
                .any(|p| p.file_name().unwrap() == "info.nfo")
        );
    }

    #[test]
    fn a_shared_folder_has_no_movie_nfo_candidate() {
        let mut item = movie("D:/Movies/Pile", "Heat (1995).mkv");
        if let ItemDetail::Movie(m) = &mut item.detail {
            m.multi_movie_dir = true;
        }
        let t = movie_targets(&item, &s(&[MovieNfoNaming::MovieNfo])).unwrap();
        assert_eq!(t.write, [PathBuf::from("D:/Movies/Pile/Heat (1995).nfo")]);
        assert!(!t.candidates.iter().any(|p| p.ends_with("movie.nfo")));
    }

    #[test]
    fn a_renamed_movie_still_finds_the_nfo_of_its_old_name() {
        let mut item = movie("D:/Movies/Heat (1995)", "Heat (1995).mkv");
        item.core.rename_history = Some(RenameHistory {
            old_path: "D:/Movies/heat".into(),
            new_path: "D:/Movies/Heat (1995)".into(),
            files: vec![RenameStep {
                old_filename: "heat.1995.mkv".into(),
                new_filename: "Heat (1995).mkv".into(),
            }],
        });
        let t = movie_targets(&item, &s(&[MovieNfoNaming::VideoBase])).unwrap();
        assert!(
            t.others()
                .any(|p| p == &PathBuf::from("D:/Movies/Heat (1995)/heat.1995.nfo"))
        );
    }

    #[test]
    fn discs_use_movie_nfo_or_the_disc_file_inside_the_folder() {
        let mut item = movie("D:/Movies/Heat (1995)", "BDMV.mkv");
        if let ItemDetail::Movie(m) = &mut item.detail {
            *m = MovieFields {
                is_disc: true,
                ..m.clone()
            };
        }
        item.core.media_files[0].directory = "D:/Movies/Heat (1995)/VIDEO_TS".into();
        item.core.media_files[0].filename = "VIDEO_TS.IFO".into();
        let top = movie_targets(&item, &s(&[MovieNfoNaming::VideoBase])).unwrap();
        assert_eq!(
            top.write,
            [PathBuf::from("D:/Movies/Heat (1995)/movie.nfo")]
        );
        let mut inside = s(&[MovieNfoNaming::VideoBase]);
        inside.nfo_in_disc_folder = true;
        let t = movie_targets(&item, &inside).unwrap();
        assert_eq!(
            t.write,
            [PathBuf::from("D:/Movies/Heat (1995)/VIDEO_TS/VIDEO_TS.nfo")]
        );
    }

    #[test]
    fn a_movie_without_files_has_no_targets() {
        let mut item = Item::movie("Heat");
        item.core.path = String::new();
        assert_eq!(movie_targets(&item, &NfoSettings::default()), None);
    }

    #[test]
    fn sets_need_a_collection_folder() {
        let mut item = Item::set("Alien Collection");
        item.core.ids.set("tmdb", "8091");
        assert_eq!(set_targets(&item, &NfoSettings::default()), None);
        let settings = NfoSettings {
            collection_folder: Some(PathBuf::from("D:/Collections")),
            ..NfoSettings::default()
        };
        let t = set_targets(&item, &settings).unwrap();
        assert_eq!(
            t.write,
            [PathBuf::from(
                "D:/Collections/Alien Collection/Alien Collection.nfo"
            )]
        );
        assert!(t.candidates.contains(&PathBuf::from(
            "D:/Collections/Alien Collection-tmdb-8091/collection.nfo"
        )));
    }

    #[test]
    fn the_set_name_replacement_reaches_the_nfo_paths() {
        let mut item = Item::set("Alien: Saga");
        item.core.ids.set("tmdb", "8091");
        let mut settings = NfoSettings {
            collection_folder: Some(PathBuf::from("D:/Collections")),
            append_tmdb_id_to_set_folder: true,
            ..NfoSettings::default()
        };
        let t = set_targets(&item, &settings).unwrap();
        assert_eq!(
            t.write,
            [PathBuf::from(
                "D:/Collections/Alien_ Saga-tmdb-8091/Alien_ Saga.nfo"
            )]
        );
        settings.set_name_replacement = " -".to_owned();
        let t = set_targets(&item, &settings).unwrap();
        assert_eq!(
            t.write,
            [PathBuf::from(
                "D:/Collections/Alien - Saga-tmdb-8091/Alien - Saga.nfo"
            )]
        );
        // A name with nothing left has no folder.
        let nothing = Item::set("???");
        settings.set_name_replacement = String::new();
        assert_eq!(set_targets(&nothing, &settings), None);
    }

    #[test]
    fn show_season_and_episode_paths() {
        let mut show = Item::show("Severance");
        show.core.path = "D:/TV/Severance".into();
        let t = show_targets(&show, &NfoSettings::default()).unwrap();
        assert_eq!(t.write, [PathBuf::from("D:/TV/Severance/tvshow.nfo")]);

        let season = season_targets(
            Path::new("D:/TV/Severance"),
            0,
            Some("Season 0"),
            &NfoSettings::default(),
        );
        assert_eq!(
            season.write,
            [PathBuf::from("D:/TV/Severance/season-specials.nfo")]
        );
        assert!(
            season
                .candidates
                .contains(&PathBuf::from("D:/TV/Severance/Season 0/season.nfo"))
        );

        let mut episode = Item::episode(cv_domain::ItemId(1), "Pilot");
        episode.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            "D:/TV/Severance/Season 1",
            "Severance - S01E01.mkv",
        ));
        let e = episode_targets(&episode).unwrap();
        assert_eq!(
            e.write,
            [PathBuf::from(
                "D:/TV/Severance/Season 1/Severance - S01E01.nfo"
            )]
        );
    }

    #[test]
    fn relative_folders_use_forward_slashes() {
        let show = Path::new("D:/TV/Severance");
        assert_eq!(
            relative_folder(show, Path::new("D:/TV/Severance/Season 1")).as_deref(),
            Some("Season 1")
        );
        assert_eq!(relative_folder(show, show), None);
        assert_eq!(relative_folder(show, Path::new("E:/Other")), None);
    }
}

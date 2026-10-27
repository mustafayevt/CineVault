//! NFO file names for TV shows, seasons and episodes.
//!
//! * show: `tvshow.nfo` in the show folder;
//! * season: `seasonNN.nfo` (`season-specials.nfo` for season 0) in the show folder, or
//!   `<season folder>/season.nfo`; a season without a folder of its own falls back to the first
//!   form;
//! * episode: `<video base>.nfo` next to the video file.
//!
//! All paths are relative to the show folder and use `/`.

/// A show NFO naming choice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ShowNfoNaming {
    /// `tvshow.nfo`.
    #[default]
    TvShow,
}

impl ShowNfoNaming {
    /// Parses the setting name (`TV_SHOW`), ignoring case.
    pub fn parse(name: &str) -> Option<Self> {
        name.trim()
            .eq_ignore_ascii_case("TV_SHOW")
            .then_some(Self::TvShow)
    }
}

/// A season NFO naming choice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SeasonNfoNaming {
    /// `seasonNN.nfo` in the show folder.
    #[default]
    Season,
    /// `<season folder>/season.nfo`.
    SeasonFolder,
}

impl SeasonNfoNaming {
    /// Parses the setting name (`SEASON` or `SEASON_FOLDER`), ignoring case.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_uppercase().as_str() {
            "SEASON" => Some(Self::Season),
            "SEASON_FOLDER" => Some(Self::SeasonFolder),
            _ => None,
        }
    }
}

/// An episode NFO naming choice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum EpisodeNfoNaming {
    /// `<video base>.nfo`.
    #[default]
    Filename,
}

impl EpisodeNfoNaming {
    /// Parses the setting name (`FILENAME`), ignoring case.
    pub fn parse(name: &str) -> Option<Self> {
        name.trim()
            .eq_ignore_ascii_case("FILENAME")
            .then_some(Self::Filename)
    }
}

/// The show NFO file names, without duplicates, in the order of the choices.
pub fn show_nfo_file_names(namings: &[ShowNfoNaming]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for naming in namings {
        let name = match naming {
            ShowNfoNaming::TvShow => "tvshow.nfo".to_owned(),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Where the season NFO of `season` goes, relative to the show folder.
///
/// `season_folder` is the folder (relative to the show folder) that holds the season's episodes,
/// `None` or empty when they sit in the show folder. `special_season_name` selects
/// `season-specials.nfo` for season 0 (the renamer's "special season" option).
pub fn season_nfo_path(
    naming: SeasonNfoNaming,
    season: u32,
    season_folder: Option<&str>,
    special_season_name: bool,
) -> String {
    let plain = if season == 0 && special_season_name {
        "season-specials.nfo".to_owned()
    } else {
        format!("season{season:02}.nfo")
    };
    match (naming, season_folder.map(|f| f.trim_matches('/'))) {
        (SeasonNfoNaming::SeasonFolder, Some(folder)) if !folder.is_empty() => {
            format!("{folder}/season.nfo")
        }
        _ => plain,
    }
}

/// The season NFO paths, without duplicates, in the order of the choices.
pub fn season_nfo_paths(
    namings: &[SeasonNfoNaming],
    season: u32,
    season_folder: Option<&str>,
    special_season_name: bool,
) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for naming in namings {
        let path = season_nfo_path(*naming, season, season_folder, special_season_name);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// The episode NFO file name for a video file, `None` when the base name is blank.
pub fn episode_nfo_file_name(video_base_name: &str) -> Option<String> {
    let base = video_base_name.trim();
    (!base.is_empty()).then(|| format!("{base}.nfo"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn season_names_follow_the_number() {
        let path = |season, folder, special| {
            season_nfo_path(SeasonNfoNaming::Season, season, folder, special)
        };
        assert_eq!(path(1, None, true), "season01.nfo");
        assert_eq!(path(12, Some("Season 12"), true), "season12.nfo");
        assert_eq!(path(0, None, true), "season-specials.nfo");
        assert_eq!(path(0, None, false), "season00.nfo");
    }

    #[test]
    fn season_folder_names_use_the_folder_or_fall_back() {
        let path = |folder| season_nfo_path(SeasonNfoNaming::SeasonFolder, 2, folder, true);
        assert_eq!(path(Some("Season 2")), "Season 2/season.nfo");
        assert_eq!(path(Some("Season 2/")), "Season 2/season.nfo");
        assert_eq!(path(Some("")), "season02.nfo");
        assert_eq!(path(None), "season02.nfo");
    }

    #[test]
    fn both_choices_give_two_paths_and_no_duplicates() {
        let both = [SeasonNfoNaming::Season, SeasonNfoNaming::SeasonFolder];
        assert_eq!(
            season_nfo_paths(&both, 1, Some("Season 1"), true),
            ["season01.nfo", "Season 1/season.nfo"]
        );
        assert_eq!(season_nfo_paths(&both, 1, None, true), ["season01.nfo"]);
        assert!(season_nfo_paths(&[], 1, None, true).is_empty());
    }

    #[test]
    fn episode_and_show_names() {
        assert_eq!(
            episode_nfo_file_name("A - S01E01").as_deref(),
            Some("A - S01E01.nfo")
        );
        assert_eq!(episode_nfo_file_name("  "), None);
        assert_eq!(
            show_nfo_file_names(&[ShowNfoNaming::TvShow, ShowNfoNaming::TvShow]),
            ["tvshow.nfo"]
        );
        assert_eq!(
            SeasonNfoNaming::parse("season_folder"),
            Some(SeasonNfoNaming::SeasonFolder)
        );
        assert_eq!(SeasonNfoNaming::parse("x"), None);
    }
}

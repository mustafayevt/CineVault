//! Renamer settings: the patterns, the clean-up options and the file-naming choices.
//!
//! Two structs hold everything the planner reads: [`MovieSettings`] and [`TvSettings`]. They
//! start from the defaults of a fresh install (`Default`) and are filled from the user's active
//! profile. Names of choices (`FILENAME_POSTER`, `ISO3T`, ...) are the setting names of the
//! naming tables; they are kept as text so a profile can carry choices a newer version
//! understands, and a name this version does not know is ignored.

use std::path::PathBuf;

use crate::exec::ExecOptions;
use crate::lang::SubtitleLanguageStyle;

/// What the space of a name becomes when "space substitution" is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceSubstitution {
    pub enabled: bool,
    /// The replacement text; empty removes the spaces.
    pub replacement: String,
}

impl Default for SpaceSubstitution {
    fn default() -> Self {
        SpaceSubstitution {
            enabled: false,
            replacement: "_".to_owned(),
        }
    }
}

impl SpaceSubstitution {
    /// Substitution on, with `replacement` for each space.
    pub fn with(replacement: impl Into<String>) -> Self {
        SpaceSubstitution {
            enabled: true,
            replacement: replacement.into(),
        }
    }
}

/// The clean-up options both renamers share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameOptions {
    /// What a colon becomes: `-` (`Dune: Part Two` is `Dune - Part Two`), `_`, or nothing.
    pub colon_replacement: String,
    /// What `${title;first}` prints for a title that starts with a digit.
    pub first_character_number_replacement: String,
    /// Fold names to ASCII (`Amélie` is `Amelie`, `Ü` is `Ue`).
    pub ascii_replacement: bool,
    /// Write the characters a file name cannot hold as Unicode look-alikes.
    pub unicode_replacement: bool,
    /// The app data folder, where `replace(file.csv)` finds its tables.
    pub data_dir: Option<PathBuf>,
}

impl Default for NameOptions {
    fn default() -> Self {
        NameOptions {
            colon_replacement: "-".to_owned(),
            first_character_number_replacement: "#".to_owned(),
            ascii_replacement: false,
            unicode_replacement: false,
            data_dir: None,
        }
    }
}

/// The default movie folder pattern.
pub const DEFAULT_MOVIE_FOLDER: &str = "${title}${ - ,edition,} (${year})";
/// The default movie file pattern.
pub const DEFAULT_MOVIE_FILE: &str =
    "${title}${ - ,edition,} (${year}) ${videoFormat} ${audioCodec}";
/// The default TV show folder pattern.
pub const DEFAULT_SHOW_FOLDER: &str = "${showTitle} (${showYear})";
/// The default TV season folder pattern.
pub const DEFAULT_SEASON_FOLDER: &str = "Season ${seasonNr}";
/// The default TV episode file pattern.
pub const DEFAULT_EPISODE_FILE: &str = "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}";

/// File extensions the unwanted-file clean-up removes by default.
pub const DEFAULT_UNWANTED: &[&str] = &[".txt", ".url", ".html", ".sfv"];

/// Everything the movie planner reads from a naming profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieSettings {
    pub folder_pattern: String,
    pub file_pattern: String,
    /// When off, the movie stays in its folder.
    pub rename_folder: bool,
    /// Keep sub-folders: a folder pattern of one segment renders inside the folder that holds the
    /// movie now (`Movies/Amadeus` stays in `Movies`) instead of the data source root. A pattern
    /// with `/` always renders under the root. On in a fresh install; off renders under the root.
    pub keep_subfolders: bool,
    /// When off, the files keep their names.
    pub rename_files: bool,
    pub folder_space: SpaceSubstitution,
    pub file_space: SpaceSubstitution,
    pub name: NameOptions,
    /// Move NFOs of other writers to the trash instead of leaving them where they are.
    pub nfo_cleanup: bool,
    /// Move files matching `unwanted_patterns` to the trash.
    pub cleanup_unwanted: bool,
    /// Regular expressions (default: the extensions of [`DEFAULT_UNWANTED`]) of unwanted files.
    pub unwanted_patterns: Vec<String>,
    /// Offer `${movieSet.*}` tokens for a set of one movie.
    pub create_movieset_for_single_movie: bool,
    /// The NFO file names (`FILENAME_NFO`, `MOVIE_NFO`).
    pub nfo: Vec<String>,
    pub poster: Vec<String>,
    pub fanart: Vec<String>,
    pub extra_fanart: Vec<String>,
    pub banner: Vec<String>,
    pub clearart: Vec<String>,
    pub thumb: Vec<String>,
    pub clearlogo: Vec<String>,
    pub discart: Vec<String>,
    pub keyart: Vec<String>,
    pub squareart: Vec<String>,
    pub trailer: Vec<String>,
    pub subtitle_language_style: SubtitleLanguageStyle,
    /// Accepted but not used: a subtitle whose language is unknown never gets a language tag.
    /// Kept so profiles round-trip.
    pub subtitle_without_language_tag: bool,
    /// A disc movie's NFO is named after the main disc file (`VIDEO_TS.nfo`, `index.nfo`); the
    /// renamer forces this whatever the NFO option says.
    pub disc_nfo_in_folder: bool,
}

impl Default for MovieSettings {
    fn default() -> Self {
        let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect();
        MovieSettings {
            folder_pattern: DEFAULT_MOVIE_FOLDER.to_owned(),
            file_pattern: DEFAULT_MOVIE_FILE.to_owned(),
            rename_folder: true,
            keep_subfolders: true,
            rename_files: true,
            folder_space: SpaceSubstitution::default(),
            file_space: SpaceSubstitution::default(),
            name: NameOptions::default(),
            nfo_cleanup: false,
            cleanup_unwanted: false,
            unwanted_patterns: DEFAULT_UNWANTED
                .iter()
                .map(|ext| format!("{}$", regex::escape(ext)))
                .collect(),
            create_movieset_for_single_movie: false,
            nfo: names(&["FILENAME_NFO"]),
            poster: names(&["FILENAME_POSTER"]),
            fanart: names(&["FILENAME_FANART"]),
            extra_fanart: names(&["FILENAME_EXTRAFANART"]),
            banner: names(&["FILENAME_BANNER"]),
            clearart: names(&["FILENAME_CLEARART"]),
            thumb: names(&["FILENAME_LANDSCAPE"]),
            clearlogo: names(&["FILENAME_CLEARLOGO"]),
            discart: names(&["FILENAME_DISCART"]),
            keyart: names(&["FILENAME_KEYART"]),
            squareart: names(&["FILENAME_SQUAREART"]),
            trailer: names(&["FILENAME_TRAILER"]),
            subtitle_language_style: SubtitleLanguageStyle::Iso3T,
            subtitle_without_language_tag: false,
            disc_nfo_in_folder: true,
        }
    }
}

impl MovieSettings {
    /// The executor options of a batch under this profile: the unwanted-file clean-up is on only
    /// when the profile asks for it (it is off in a fresh install).
    pub fn exec_options(&self, batch_id: impl Into<String>) -> ExecOptions {
        let mut options = ExecOptions::new(batch_id);
        if self.cleanup_unwanted {
            options
                .unwanted_patterns
                .clone_from(&self.unwanted_patterns);
        }
        options
    }
}

/// How a file that holds several episodes is named.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MultiEpisodeStyle {
    /// `S01E01 S01E02`: the season and episode part once per episode.
    #[default]
    Repeat,
    /// `S01E01-E02` for consecutive episodes of one season (`1x01-02` with an `x`).
    Range,
}

impl MultiEpisodeStyle {
    /// Parses the setting name (`REPEAT`, `RANGE`), ignoring case.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_uppercase().as_str() {
            "REPEAT" => Some(Self::Repeat),
            "RANGE" => Some(Self::Range),
            _ => None,
        }
    }

    /// The setting name [`MultiEpisodeStyle::parse`] reads back.
    pub fn name(self) -> &'static str {
        match self {
            Self::Repeat => "REPEAT",
            Self::Range => "RANGE",
        }
    }
}

/// Everything the TV planner reads from a naming profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TvSettings {
    pub show_pattern: String,
    pub season_pattern: String,
    pub file_pattern: String,
    pub rename_show_folder: bool,
    /// Keep sub-folders for the show folder; see [`MovieSettings::keep_subfolders`].
    pub keep_subfolders: bool,
    pub rename_season_folder: bool,
    pub rename_files: bool,
    pub show_space: SpaceSubstitution,
    pub season_space: SpaceSubstitution,
    pub file_space: SpaceSubstitution,
    pub name: NameOptions,
    pub multi_episode_style: MultiEpisodeStyle,
    /// Season 0 is the folder `Specials`.
    pub special_season: bool,
    pub cleanup_unwanted: bool,
    pub unwanted_patterns: Vec<String>,
    /// Names of the show NFO (`TV_SHOW`).
    pub show_nfo: Vec<String>,
    /// Names of the season NFO (`SEASON`, `SEASON_FOLDER`).
    pub season_nfo: Vec<String>,
    /// Names of the episode NFO (`FILENAME`).
    pub episode_nfo: Vec<String>,
    pub poster: Vec<String>,
    pub fanart: Vec<String>,
    pub banner: Vec<String>,
    pub clearart: Vec<String>,
    pub clearlogo: Vec<String>,
    pub discart: Vec<String>,
    pub characterart: Vec<String>,
    pub keyart: Vec<String>,
    pub thumb: Vec<String>,
    pub squareart: Vec<String>,
    pub season_poster: Vec<String>,
    pub season_fanart: Vec<String>,
    pub season_banner: Vec<String>,
    pub season_thumb: Vec<String>,
    /// Names of the episode thumbnail (`FILENAME`, `FILENAME_THUMB`, `FILENAME_TBN`).
    pub episode_thumb: Vec<String>,
    pub subtitle_language_style: SubtitleLanguageStyle,
}

impl Default for TvSettings {
    fn default() -> Self {
        let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect();
        TvSettings {
            show_pattern: DEFAULT_SHOW_FOLDER.to_owned(),
            season_pattern: DEFAULT_SEASON_FOLDER.to_owned(),
            file_pattern: DEFAULT_EPISODE_FILE.to_owned(),
            rename_show_folder: true,
            keep_subfolders: true,
            rename_season_folder: true,
            rename_files: true,
            show_space: SpaceSubstitution::default(),
            season_space: SpaceSubstitution::default(),
            file_space: SpaceSubstitution::default(),
            name: NameOptions {
                colon_replacement: String::new(),
                ..NameOptions::default()
            },
            multi_episode_style: MultiEpisodeStyle::Repeat,
            special_season: true,
            cleanup_unwanted: false,
            unwanted_patterns: DEFAULT_UNWANTED
                .iter()
                .map(|ext| format!("{}$", regex::escape(ext)))
                .collect(),
            show_nfo: names(&["TV_SHOW"]),
            season_nfo: names(&["SEASON"]),
            episode_nfo: names(&["FILENAME"]),
            poster: names(&["POSTER"]),
            fanart: names(&["FANART"]),
            banner: names(&["BANNER"]),
            clearart: names(&["CLEARART"]),
            clearlogo: names(&["CLEARLOGO"]),
            discart: names(&["DISCART"]),
            characterart: names(&["CHARACTERART"]),
            keyart: names(&["KEYART"]),
            thumb: names(&["LANDSCAPE"]),
            squareart: names(&["SQUAREART"]),
            season_poster: names(&["SEASON_POSTER"]),
            season_fanart: names(&["SEASON_FANART"]),
            season_banner: names(&["SEASON_BANNER"]),
            season_thumb: names(&["SEASON_THUMB"]),
            episode_thumb: names(&["FILENAME_THUMB"]),
            subtitle_language_style: SubtitleLanguageStyle::Iso3T,
        }
    }
}

impl TvSettings {
    /// The executor options of a batch under this profile; see [`MovieSettings::exec_options`].
    pub fn exec_options(&self, batch_id: impl Into<String>) -> ExecOptions {
        let mut options = ExecOptions::new(batch_id);
        if self.cleanup_unwanted {
            options
                .unwanted_patterns
                .clone_from(&self.unwanted_patterns);
        }
        options
    }
}

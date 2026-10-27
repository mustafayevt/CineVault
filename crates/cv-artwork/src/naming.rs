//! Media-centre artwork file names: which files an image is written to.
//!
//! Spec: `artwork` (Media-centre artwork filenames). The name tables are the renamer's (`cv_rename::naming`), so the writer and the
//! renamer always plan the same files; this module adds the settings, the presets and the
//! per-folder restrictions around them.
//!
//! # Choices and names
//!
//! Every artwork type has a list of **choices** (`FILENAME_POSTER`, `POSTER`, `FOLDER`, ...). Each
//! choice gives one file name and every name of the list receives the same image, so a library
//! can be read by more than one media centre at once. Choices this version does not know are
//! ignored, duplicates collapse, and an empty list means the type is not written.
//!
//! # Folder restrictions
//!
//! * [`FolderKind::MultiMovie`]: several movies share the folder. Only choices built on the video
//!   name (`<base>-poster`) are kept, because `poster.jpg` would be shared by all of them; when a
//!   type has only fixed choices, its `FILENAME_*` default is used instead. Extra fanart and
//!   extra thumbs are not written.
//! * [`FolderKind::Disc`]: a disc structure (`VIDEO_TS`, `BDMV`) prefers fixed names, so
//!   `<base>-poster` becomes `poster`.
//!
//! # Presets
//!
//! [`ArtworkPreset`] fills all lists at once: Kodi (every `FILENAME_*` choice, the default),
//! Xbmc (the same with the `extrafanart/` folder), Emby and Plex (fixed names such as
//! `poster.jpg`, `fanart.jpg`) and Jellyfin (`backdrop`). The setting `metadata.artwork-names`
//! picks one: `prefixed` (Kodi, default), `short` (Emby), `both` (the names of the two), or a
//! preset id (`kodi`, `xbmc`, `emby`, `plex`, `jellyfin`).
//!
//! # Collections
//!
//! The files of a collection (movie set) are planned by the [`set`] module: in each movie folder
//! as `movieset-<type>`, or in the collection data folder in Kodi or Automator style, with
//! name-character replacement and an optional TMDB ID suffix.
//!
//! # Settings
//!
//! | Key | Values | Meaning |
//! |---|---|---|
//! | `metadata.artwork-names` | `prefixed` (default), `short`, `both`, a preset id | the starting lists |
//! | `artwork.names.movie.<type>` | array of choices, or a comma separated string | replaces one movie list |
//! | `artwork.names.tv.<type>` | the same | replaces one TV list |
//!
//! Movie types: `poster`, `fanart`, `extra-fanart`, `banner`, `clearart`, `thumb`, `clearlogo`,
//! `discart`, `keyart`, `squareart`. TV types: `poster`, `fanart`, `banner`, `clearart`,
//! `clearlogo`, `discart`, `characterart`, `keyart`, `thumb`, `squareart`, `season-poster`,
//! `season-fanart`, `season-banner`, `season-thumb`, `episode-thumb`. A value with no valid
//! choice is ignored, and so is an empty one: every artwork type is written, so the settings
//! write refuses an empty list (`cv_app::app_settings`) and a hand-edited empty list gives the
//! starting names.

use std::collections::BTreeMap;

pub mod set;

use cv_domain::MediaFileType;
use cv_rename::naming::{
    ArtKind, NameRule, SeasonArt, ShowArt, disc_rules, episode_thumb_name, movie_rule, movie_rules,
    season_art_path, shared_folder_rules, show_stem,
};
use serde_json::Value;

type Values = BTreeMap<String, Value>;

/// The key of the starting lists.
pub const STYLE_KEY: &str = "metadata.artwork-names";

/// The folder a movie's files are written to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum FolderKind {
    /// The movie has its folder to itself.
    #[default]
    Single,
    /// Several movies share the folder (root files, A-Z, year or genre folders).
    MultiMovie,
    /// A disc structure (`VIDEO_TS`, `BDMV`).
    Disc,
}

/// The single-image artwork types of a movie. Extra fanart and extra thumbs have their own
/// methods because they carry an index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MovieArt {
    Poster,
    Fanart,
    Banner,
    Clearart,
    Thumb,
    Clearlogo,
    Discart,
    Keyart,
    Squareart,
}

impl MovieArt {
    /// Every type, in the order the settings list them.
    pub const ALL: [MovieArt; 9] = [
        MovieArt::Poster,
        MovieArt::Fanart,
        MovieArt::Banner,
        MovieArt::Clearart,
        MovieArt::Thumb,
        MovieArt::Clearlogo,
        MovieArt::Discart,
        MovieArt::Keyart,
        MovieArt::Squareart,
    ];

    /// The name used in setting keys.
    pub fn key(self) -> &'static str {
        match self {
            MovieArt::Poster => "poster",
            MovieArt::Fanart => "fanart",
            MovieArt::Banner => "banner",
            MovieArt::Clearart => "clearart",
            MovieArt::Thumb => "thumb",
            MovieArt::Clearlogo => "clearlogo",
            MovieArt::Discart => "discart",
            MovieArt::Keyart => "keyart",
            MovieArt::Squareart => "squareart",
        }
    }

    /// Parses [`MovieArt::key`].
    pub fn parse(key: &str) -> Option<MovieArt> {
        MovieArt::ALL.into_iter().find(|a| a.key() == key)
    }

    /// The renamer's kind of the same type.
    pub fn kind(self) -> ArtKind {
        match self {
            MovieArt::Poster => ArtKind::Poster,
            MovieArt::Fanart => ArtKind::Fanart,
            MovieArt::Banner => ArtKind::Banner,
            MovieArt::Clearart => ArtKind::Clearart,
            MovieArt::Thumb => ArtKind::Thumb,
            MovieArt::Clearlogo => ArtKind::Clearlogo,
            MovieArt::Discart => ArtKind::Discart,
            MovieArt::Keyart => ArtKind::Keyart,
            MovieArt::Squareart => ArtKind::Squareart,
        }
    }

    /// The type of a scanned file, or `None` for anything that is not single-image movie artwork.
    pub fn from_file_type(kind: MediaFileType) -> Option<MovieArt> {
        Some(match kind {
            MediaFileType::Poster => MovieArt::Poster,
            MediaFileType::Fanart => MovieArt::Fanart,
            MediaFileType::Banner => MovieArt::Banner,
            MediaFileType::Clearart => MovieArt::Clearart,
            MediaFileType::Thumb => MovieArt::Thumb,
            MediaFileType::Clearlogo => MovieArt::Clearlogo,
            MediaFileType::Disc => MovieArt::Discart,
            MediaFileType::Keyart => MovieArt::Keyart,
            MediaFileType::Squareart => MovieArt::Squareart,
            _ => return None,
        })
    }

    /// The extension of a fresh download when the source says nothing: `png` for the types that
    /// are mostly transparent, `jpg` for the rest.
    pub fn default_ext(self) -> &'static str {
        match self {
            MovieArt::Clearart | MovieArt::Clearlogo | MovieArt::Discart => "png",
            _ => "jpg",
        }
    }
}

/// A ready-made set of lists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ArtworkPreset {
    /// `<name>-poster.jpg`, `<name>-fanart.jpg`, ... for every type.
    #[default]
    Kodi,
    /// The Kodi names, with extra fanart in the `extrafanart/` folder.
    Xbmc,
    /// `poster.jpg`, `fanart.jpg`, `landscape.jpg`, ...
    Emby,
    /// The same fixed movie names as Emby; episode thumbnails are `<name>.jpg`.
    Plex,
    /// `poster.jpg`, `backdrop.jpg`, `backdrop1.jpg`, ...
    Jellyfin,
}

impl ArtworkPreset {
    pub const ALL: [ArtworkPreset; 5] = [
        ArtworkPreset::Kodi,
        ArtworkPreset::Xbmc,
        ArtworkPreset::Emby,
        ArtworkPreset::Plex,
        ArtworkPreset::Jellyfin,
    ];

    /// The id used in settings and fixtures.
    pub fn id(self) -> &'static str {
        match self {
            ArtworkPreset::Kodi => "kodi",
            ArtworkPreset::Xbmc => "xbmc",
            ArtworkPreset::Emby => "emby",
            ArtworkPreset::Plex => "plex",
            ArtworkPreset::Jellyfin => "jellyfin",
        }
    }

    /// Parses [`ArtworkPreset::id`], ignoring case.
    pub fn parse(id: &str) -> Option<ArtworkPreset> {
        let id = id.trim().to_ascii_lowercase();
        ArtworkPreset::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The movie lists of the preset.
    pub fn movie_names(self) -> MovieNames {
        let list =
            |names: &[&str]| -> Vec<String> { names.iter().map(|n| (*n).to_owned()).collect() };
        let kodi = MovieNames {
            poster: list(&["FILENAME_POSTER"]),
            fanart: list(&["FILENAME_FANART"]),
            extra_fanart: list(&["FILENAME_EXTRAFANART"]),
            banner: list(&["FILENAME_BANNER"]),
            clearart: list(&["FILENAME_CLEARART"]),
            thumb: list(&["FILENAME_LANDSCAPE"]),
            clearlogo: list(&["FILENAME_CLEARLOGO"]),
            discart: list(&["FILENAME_DISCART"]),
            keyart: list(&["FILENAME_KEYART"]),
            squareart: list(&["FILENAME_SQUAREART"]),
        };
        match self {
            ArtworkPreset::Kodi => kodi,
            ArtworkPreset::Xbmc => MovieNames {
                extra_fanart: list(&["FOLDER_EXTRAFANART"]),
                ..kodi
            },
            ArtworkPreset::Emby | ArtworkPreset::Plex => MovieNames {
                poster: list(&["POSTER"]),
                fanart: list(&["FANART"]),
                extra_fanart: list(&["FOLDER_EXTRAFANART"]),
                banner: list(&["BANNER"]),
                clearart: list(&["CLEARART"]),
                thumb: list(&["LANDSCAPE"]),
                clearlogo: list(&["CLEARLOGO"]),
                discart: list(&["DISCART"]),
                keyart: list(&["KEYART"]),
                squareart: list(&["SQUAREART"]),
            },
            ArtworkPreset::Jellyfin => MovieNames {
                poster: list(&["POSTER"]),
                fanart: list(&["BACKDROP"]),
                extra_fanart: list(&["EXTRABACKDROP"]),
                banner: list(&["BANNER"]),
                clearart: list(&["CLEARART"]),
                thumb: list(&["LANDSCAPE"]),
                clearlogo: list(&["CLEARLOGO"]),
                discart: list(&["DISCART"]),
                keyart: list(&["KEYART"]),
                squareart: list(&["SQUAREART"]),
            },
        }
    }

    /// The TV lists of the preset.
    pub fn tv_names(self) -> TvNames {
        let list =
            |names: &[&str]| -> Vec<String> { names.iter().map(|n| (*n).to_owned()).collect() };
        let base = TvNames {
            poster: list(&["POSTER"]),
            fanart: list(&["FANART"]),
            banner: list(&["BANNER"]),
            clearart: list(&["CLEARART"]),
            clearlogo: list(&["CLEARLOGO"]),
            discart: list(&["DISCART"]),
            characterart: list(&["CHARACTERART"]),
            keyart: list(&["KEYART"]),
            thumb: list(&["LANDSCAPE"]),
            squareart: list(&["SQUAREART"]),
            season_poster: list(&["SEASON_POSTER"]),
            season_fanart: list(&["SEASON_FANART"]),
            season_banner: list(&["SEASON_BANNER"]),
            season_thumb: list(&["SEASON_THUMB"]),
            episode_thumb: list(&["FILENAME_THUMB"]),
        };
        match self {
            ArtworkPreset::Kodi | ArtworkPreset::Xbmc | ArtworkPreset::Emby => base,
            ArtworkPreset::Plex => TvNames {
                episode_thumb: list(&["FILENAME"]),
                ..base
            },
            ArtworkPreset::Jellyfin => TvNames {
                fanart: list(&["BACKDROP"]),
                ..base
            },
        }
    }
}

/// The lists of choices for movie artwork.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieNames {
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
}

impl Default for MovieNames {
    fn default() -> Self {
        ArtworkPreset::Kodi.movie_names()
    }
}

/// `<base>.<ext>` style names for a list of rules, without duplicates and without the rules that
/// need a video name when there is none.
fn rule_names(rules: &[NameRule], base: &str, ext: &str, index: Option<u32>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for rule in rules {
        if base.is_empty() && rule.is_per_movie() {
            continue;
        }
        let name = rule.file_name(base, ext, index);
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

fn clean_ext(ext: &str, default: &str) -> String {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    if ext.is_empty() {
        default.to_owned()
    } else {
        ext
    }
}

impl MovieNames {
    /// The choices of a type.
    pub fn choices(&self, art: MovieArt) -> &[String] {
        match art {
            MovieArt::Poster => &self.poster,
            MovieArt::Fanart => &self.fanart,
            MovieArt::Banner => &self.banner,
            MovieArt::Clearart => &self.clearart,
            MovieArt::Thumb => &self.thumb,
            MovieArt::Clearlogo => &self.clearlogo,
            MovieArt::Discart => &self.discart,
            MovieArt::Keyart => &self.keyart,
            MovieArt::Squareart => &self.squareart,
        }
    }

    fn choices_mut(&mut self, art: MovieArt) -> &mut Vec<String> {
        match art {
            MovieArt::Poster => &mut self.poster,
            MovieArt::Fanart => &mut self.fanart,
            MovieArt::Banner => &mut self.banner,
            MovieArt::Clearart => &mut self.clearart,
            MovieArt::Thumb => &mut self.thumb,
            MovieArt::Clearlogo => &mut self.clearlogo,
            MovieArt::Discart => &mut self.discart,
            MovieArt::Keyart => &mut self.keyart,
            MovieArt::Squareart => &mut self.squareart,
        }
    }

    fn rules(kind: ArtKind, choices: &[String], folder: FolderKind) -> Vec<NameRule> {
        let rules = movie_rules(kind, choices);
        match folder {
            FolderKind::Single => rules,
            FolderKind::MultiMovie => shared_folder_rules(kind, &rules),
            FolderKind::Disc => disc_rules(&rules),
        }
    }

    /// The files a movie's image of this type is written to, relative to the movie folder, in the
    /// order of the list. `base` is the video file name without extension and stacking marker;
    /// `ext` is the extension of the image (without the dot, empty for the type's default).
    pub fn targets(&self, art: MovieArt, base: &str, ext: &str, folder: FolderKind) -> Vec<String> {
        let ext = clean_ext(ext, art.default_ext());
        let rules = Self::rules(art.kind(), self.choices(art), folder);
        rule_names(&rules, base, &ext, None)
    }

    /// The files extra fanart number `1..=count` is written to: the **first** choice of the list
    /// only, each file named with its number. Nothing for a multi-movie folder.
    pub fn extra_fanart_targets(
        &self,
        base: &str,
        ext: &str,
        folder: FolderKind,
        count: u32,
    ) -> Vec<String> {
        let Some(rule) = self.extra_fanart_rule(folder) else {
            return Vec::new();
        };
        let ext = clean_ext(ext, "jpg");
        (1..=count)
            .filter_map(|i| {
                if base.is_empty() && rule.is_per_movie() {
                    None
                } else {
                    Some(rule.file_name(base, &ext, Some(i)))
                }
            })
            .collect()
    }

    /// The folder that holds the extra fanart when the first choice uses one (`extrafanart`):
    /// the folder to empty before new extras are written. `None` for a multi-movie folder or
    /// when the extras sit beside the video.
    pub fn extra_fanart_folder(&self, folder: FolderKind) -> Option<&'static str> {
        match self.extra_fanart_rule(folder)? {
            NameRule::InFolder { dir, .. } | NameRule::InFolderBase { dir, .. } => Some(dir),
            NameRule::Base(_) | NameRule::Fixed(_) => None,
        }
    }

    fn extra_fanart_rule(&self, folder: FolderKind) -> Option<NameRule> {
        if folder == FolderKind::MultiMovie {
            return None;
        }
        Self::rules(ArtKind::ExtraFanart, &self.extra_fanart, folder)
            .into_iter()
            .next()
    }

    /// The files extra thumbs number `1..=count` are written to: `extrathumbs/thumb1.jpg`, ...
    /// Nothing for a multi-movie folder.
    pub fn extra_thumb_targets(&self, folder: FolderKind, ext: &str, count: u32) -> Vec<String> {
        if folder == FolderKind::MultiMovie {
            return Vec::new();
        }
        let ext = clean_ext(ext, "jpg");
        (1..=count)
            .map(|i| format!("{EXTRA_THUMB_FOLDER}/thumb{i}.{ext}"))
            .collect()
    }

    /// True when the choice is one this version knows for the type.
    pub fn is_valid_choice(art: MovieArt, choice: &str) -> bool {
        movie_rule(art.kind(), choice).is_some()
    }

    /// Adds the names of `other` after the ones already in the lists, without duplicates.
    fn merge(&mut self, other: &MovieNames) {
        for art in MovieArt::ALL {
            merge_list(self.choices_mut(art), other.choices(art));
        }
        merge_list(&mut self.extra_fanart, &other.extra_fanart);
    }
}

/// The folder of the extra thumbs.
pub const EXTRA_THUMB_FOLDER: &str = "extrathumbs";

fn merge_list(into: &mut Vec<String>, from: &[String]) {
    for name in from {
        if !into.contains(name) {
            into.push(name.clone());
        }
    }
}

/// The lists of choices for TV artwork.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TvNames {
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
    pub episode_thumb: Vec<String>,
}

impl Default for TvNames {
    fn default() -> Self {
        ArtworkPreset::Kodi.tv_names()
    }
}

/// The TV types in the order of the settings, with the key of each.
const SHOW_KEYS: [(&str, ShowArt); 10] = [
    ("poster", ShowArt::Poster),
    ("fanart", ShowArt::Fanart),
    ("banner", ShowArt::Banner),
    ("clearart", ShowArt::Clearart),
    ("clearlogo", ShowArt::Clearlogo),
    ("discart", ShowArt::Discart),
    ("characterart", ShowArt::Characterart),
    ("keyart", ShowArt::Keyart),
    ("thumb", ShowArt::Thumb),
    ("squareart", ShowArt::Squareart),
];

const SEASON_KEYS: [(&str, SeasonArt); 4] = [
    ("season-poster", SeasonArt::Poster),
    ("season-fanart", SeasonArt::Fanart),
    ("season-banner", SeasonArt::Banner),
    ("season-thumb", SeasonArt::Thumb),
];

fn unique(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

impl TvNames {
    /// The choices of a show type.
    pub fn show_choices(&self, art: ShowArt) -> &[String] {
        match art {
            ShowArt::Poster => &self.poster,
            ShowArt::Fanart => &self.fanart,
            ShowArt::Banner => &self.banner,
            ShowArt::Clearart => &self.clearart,
            ShowArt::Clearlogo => &self.clearlogo,
            ShowArt::Discart => &self.discart,
            ShowArt::Characterart => &self.characterart,
            ShowArt::Keyart => &self.keyart,
            ShowArt::Thumb => &self.thumb,
            ShowArt::Squareart => &self.squareart,
        }
    }

    fn show_choices_mut(&mut self, art: ShowArt) -> &mut Vec<String> {
        match art {
            ShowArt::Poster => &mut self.poster,
            ShowArt::Fanart => &mut self.fanart,
            ShowArt::Banner => &mut self.banner,
            ShowArt::Clearart => &mut self.clearart,
            ShowArt::Clearlogo => &mut self.clearlogo,
            ShowArt::Discart => &mut self.discart,
            ShowArt::Characterart => &mut self.characterart,
            ShowArt::Keyart => &mut self.keyart,
            ShowArt::Thumb => &mut self.thumb,
            ShowArt::Squareart => &mut self.squareart,
        }
    }

    /// The choices of a season type.
    pub fn season_choices(&self, art: SeasonArt) -> &[String] {
        match art {
            SeasonArt::Poster => &self.season_poster,
            SeasonArt::Fanart => &self.season_fanart,
            SeasonArt::Banner => &self.season_banner,
            SeasonArt::Thumb => &self.season_thumb,
        }
    }

    fn season_choices_mut(&mut self, art: SeasonArt) -> &mut Vec<String> {
        match art {
            SeasonArt::Poster => &mut self.season_poster,
            SeasonArt::Fanart => &mut self.season_fanart,
            SeasonArt::Banner => &mut self.season_banner,
            SeasonArt::Thumb => &mut self.season_thumb,
        }
    }

    /// The files a show's image is written to, relative to the show folder: `poster.jpg`.
    pub fn show_targets(&self, art: ShowArt, ext: &str) -> Vec<String> {
        let ext = clean_ext(ext, "jpg");
        unique(
            self.show_choices(art)
                .iter()
                .filter_map(|c| show_stem(art, c))
                .map(|stem| format!("{stem}.{ext}")),
        )
    }

    /// The files a season's image is written to, relative to the show folder. `season` is `0`
    /// for specials and `-1` for the all-seasons image; `season_folder` is the season's folder
    /// name (empty when the episodes sit in the show folder).
    pub fn season_targets(
        &self,
        art: SeasonArt,
        season: i64,
        season_folder: &str,
        ext: &str,
    ) -> Vec<String> {
        let ext = clean_ext(ext, "jpg");
        unique(
            self.season_choices(art)
                .iter()
                .filter_map(|c| season_art_path(art, c, season, season_folder, &ext)),
        )
    }

    /// The files an episode thumbnail is written to, relative to the episode's folder. `base` is
    /// the episode video's file name without extension.
    pub fn episode_thumb_targets(&self, base: &str, ext: &str) -> Vec<String> {
        if base.is_empty() {
            return Vec::new();
        }
        let ext = clean_ext(ext, "jpg");
        unique(
            self.episode_thumb
                .iter()
                .filter_map(|c| episode_thumb_name(c, base, &ext)),
        )
    }

    fn merge(&mut self, other: &TvNames) {
        for (_, art) in SHOW_KEYS {
            merge_list(self.show_choices_mut(art), other.show_choices(art));
        }
        for (_, art) in SEASON_KEYS {
            merge_list(self.season_choices_mut(art), other.season_choices(art));
        }
        merge_list(&mut self.episode_thumb, &other.episode_thumb);
    }
}

/// The artwork names of a library: movies, TV and collections.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtworkNames {
    pub movie: MovieNames,
    pub tv: TvNames,
    /// Collections (movie sets); see [`set`].
    pub set: set::SetNames,
}

/// The upper-case, trimmed choices of a settings value (an array or a comma separated string).
fn choices_of(value: &Value) -> Option<Vec<String>> {
    let items: Vec<String> = match value {
        Value::Array(list) => list
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Value::String(text) => text.split(',').map(str::to_owned).collect(),
        _ => return None,
    };
    Some(
        items
            .iter()
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

/// The list a settings value gives, or `None` when the value does not apply. A list with no
/// valid choice, an empty one included, is ignored: every artwork type keeps at least one name.
fn override_list(value: &Value, valid: impl Fn(&str) -> bool) -> Option<Vec<String>> {
    let wanted = choices_of(value)?;
    let mut kept: Vec<String> = Vec::new();
    for choice in wanted {
        if valid(&choice) && !kept.contains(&choice) {
            kept.push(choice);
        }
    }
    (!kept.is_empty()).then_some(kept)
}

impl ArtworkNames {
    /// The lists of a preset.
    pub fn preset(preset: ArtworkPreset) -> Self {
        ArtworkNames {
            movie: preset.movie_names(),
            tv: preset.tv_names(),
            set: set::SetNames::default(),
        }
    }

    /// The lists of a `metadata.artwork-names` value (`prefixed`, `short`, `both` or a preset
    /// id); anything else gives the default (Kodi).
    pub fn from_style(style: &str) -> Self {
        let style = style.trim().to_ascii_lowercase();
        match style.as_str() {
            "short" => Self::preset(ArtworkPreset::Emby),
            "both" => {
                let mut names = Self::preset(ArtworkPreset::Kodi);
                let short = Self::preset(ArtworkPreset::Emby);
                names.movie.merge(&short.movie);
                names.tv.merge(&short.tv);
                names
            }
            other => Self::preset(ArtworkPreset::parse(other).unwrap_or_default()),
        }
    }

    /// Reads the settings; anything missing or unknown takes its default, so a hand-edited
    /// settings file can never stop artwork from being written.
    pub fn from_values(values: &Values) -> Self {
        let style = values
            .get(STYLE_KEY)
            .and_then(Value::as_str)
            .unwrap_or("prefixed");
        let mut names = Self::from_style(style);

        for art in MovieArt::ALL {
            if let Some(list) = values
                .get(&format!("artwork.names.movie.{}", art.key()))
                .and_then(|v| override_list(v, |c| MovieNames::is_valid_choice(art, c)))
            {
                *names.movie.choices_mut(art) = list;
            }
        }
        if let Some(list) = values
            .get("artwork.names.movie.extra-fanart")
            .and_then(|v| override_list(v, |c| movie_rule(ArtKind::ExtraFanart, c).is_some()))
        {
            names.movie.extra_fanart = list;
        }
        for (key, art) in SHOW_KEYS {
            if let Some(list) = values
                .get(&format!("artwork.names.tv.{key}"))
                .and_then(|v| override_list(v, |c| show_stem(art, c).is_some()))
            {
                *names.tv.show_choices_mut(art) = list;
            }
        }
        for (key, art) in SEASON_KEYS {
            if let Some(list) = values
                .get(&format!("artwork.names.tv.{key}"))
                .and_then(|v| {
                    override_list(v, |c| {
                        season_art_path(art, c, 1, "Season 1", "jpg").is_some()
                    })
                })
            {
                *names.tv.season_choices_mut(art) = list;
            }
        }
        if let Some(list) = values
            .get("artwork.names.tv.episode-thumb")
            .and_then(|v| override_list(v, |c| episode_thumb_name(c, "e", "jpg").is_some()))
        {
            names.tv.episode_thumb = list;
        }
        names.set = set::SetNames::from_values(values);
        names
    }
}

#[cfg(test)]
mod tests;

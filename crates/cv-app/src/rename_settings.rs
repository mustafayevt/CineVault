//! The naming profiles, read from and written to the settings map.
//!
//! | Key | Values | Meaning |
//! |---|---|---|
//! | `renamer.active-profile` | profile id (default `clean`) | the profile Organize uses |
//! | `renamer.profiles` | array of custom profiles (default none) | the user's custom profiles |
//! | `renamer.cleanup-unwanted` | bool (default off) | move unwanted files to the trash folder after a rename, and allow the clean-up action |
//! | `renamer.unwanted-patterns` | text (default `.txt, .url, .html, .sfv`) | the unwanted files: extensions or regular expressions on the file name, separated by commas |
//! | `naming.colon` | `remove`, `dash` or `lookalike` (unset follows the profile, which writes `dash` unless a custom profile says otherwise) | how a colon in a title is written in names (Settings, Naming): dropped, ` - `, or the look-alike character U+A789 |
//! | `renamer.keep-subfolders` | bool (default on) | "Keep sub-folders": a folder pattern of one segment renders inside the folder that holds the title now (`Movies/Amadeus` stays in `Movies`); a pattern with `/` still renders under the library folder |
//!
//! The three presets (`clean`, `detailed`, `media-server-safe`) are built into `cv-rename` and never
//! stored. A custom profile is stored as an object with camelCase keys; a missing key takes the value
//! of the Clean preset, an unknown choice name is ignored, so a hand-edited or newer file can never
//! stop renaming.
//!
//! # Where the file names come from
//!
//! A profile holds the **naming** (patterns, clean-up, space and colon options, unwanted-file
//! clean-up, multi-episode style, subtitle language style). The names of the NFO files come from the
//! NFO settings, so the renamer plans exactly the files the NFO writer writes
//! ([`NfoSettings`]): [`apply_nfo_choices`] copies them into the profile's settings. That includes
//! the disc option: `metadata.nfo-disc-folder` (default off) decides whether a disc movie's NFO is
//! named after the main disc file. It replaces the renamer's own default of `true`, so a rename
//! never moves a disc NFO to a name the writer would not use.
//!
//! The names of the artwork files come from the artwork settings (`metadata.artwork-names` and the
//! per-type lists, see `cv_artwork::naming`): [`apply_artwork_choices`] copies them into the
//! profile, so the renamer moves artwork to exactly the names the artwork writer uses.
//!
//! The unwanted-file clean-up stays off until it is turned on: by the library-wide switch
//! `renamer.cleanup-unwanted` (Organize and Settings, Files), which [`apply_cleanup_choices`]
//! copies into whatever profile is used with its file types, or by a custom profile of its own
//! ([`cv_rename::MovieSettings::exec_options`]).

use std::collections::BTreeMap;

use cv_artwork::naming::ArtworkNames;
use cv_nfo::{MovieNfoNaming, SeasonNfoNaming};
use cv_rename::{
    MovieSettings, MultiEpisodeStyle, Preset, ProfileBook, RenameProfile, SpaceSubstitution,
    SubtitleLanguageStyle, TvSettings,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::nfo::NfoSettings;

type Values = BTreeMap<String, Value>;

/// The key of the active profile id.
pub const ACTIVE_PROFILE_KEY: &str = "renamer.active-profile";
/// The key of the custom profiles.
pub const PROFILES_KEY: &str = "renamer.profiles";

/// The key of the library-wide unwanted-file clean-up switch.
pub const CLEANUP_KEY: &str = "renamer.cleanup-unwanted";
/// The key of the unwanted file types.
pub const UNWANTED_KEY: &str = "renamer.unwanted-patterns";
/// The unwanted file types of a fresh install, as the setting writes them.
pub const DEFAULT_UNWANTED_TEXT: &str = ".txt, .url, .html, .sfv";
/// The key of how a colon in a title is written in names.
pub const COLON_KEY: &str = "naming.colon";
/// The key of the library-wide "Keep sub-folders" switch.
pub const KEEP_SUBFOLDERS_KEY: &str = "renamer.keep-subfolders";
/// The key of how the language is written in subtitle file names (`ISO2`, `ISO3T`, `ISO3B`,
/// `LANG_EN`, `LANG_LOCALIZED`, `ORIGINAL`; `profile` or unset follows the naming profile). It
/// was a subtitle download setting; it stays as the choice for the names the renamer gives
/// subtitle files.
pub const SUBTITLE_LANGUAGE_STYLE_KEY: &str = "subtitles.language-style";

/// How many custom profiles are kept; the settings file limits a value to 64 KiB.
pub const MAX_CUSTOM_PROFILES: usize = 24;

/// The naming profiles.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenameSettings {
    pub book: ProfileBook,
}

impl RenameSettings {
    /// Reads the settings; anything missing or malformed takes its default.
    pub fn from_values(values: &Values) -> Self {
        let custom: Vec<RenameProfile> = values
            .get(PROFILES_KEY)
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|v| serde_json::from_value::<StoredProfile>(v.clone()).ok())
                    .map(StoredProfile::into_profile)
                    .take(MAX_CUSTOM_PROFILES)
                    .collect()
            })
            .unwrap_or_default();
        let active = values
            .get(ACTIVE_PROFILE_KEY)
            .and_then(Value::as_str)
            .unwrap_or(cv_rename::profile::DEFAULT_PROFILE_ID);
        RenameSettings {
            book: ProfileBook::new(custom, active),
        }
    }

    /// The active profile with the NFO file names of `nfo` applied: what the planners read.
    pub fn active_profile(&self, nfo: &NfoSettings) -> RenameProfile {
        let mut profile = self.book.active();
        apply_nfo_choices(&mut profile, nfo);
        profile
    }

    /// [`RenameSettings::active_profile`] with the artwork file names of `artwork` applied too.
    pub fn active_profile_with_artwork(
        &self,
        nfo: &NfoSettings,
        artwork: &ArtworkNames,
    ) -> RenameProfile {
        let mut profile = self.active_profile(nfo);
        apply_artwork_choices(&mut profile, artwork);
        profile
    }

    /// The values to store for the profiles: `(key, value)` pairs for the active id and the custom
    /// profiles. Write them with `SettingsService::set` (or reset a key whose value is the default).
    pub fn to_values(&self) -> Vec<(&'static str, Value)> {
        let stored: Vec<Value> = self
            .book
            .custom()
            .iter()
            .filter_map(|p| serde_json::to_value(StoredProfile::from_profile(p)).ok())
            .collect();
        vec![
            (ACTIVE_PROFILE_KEY, Value::from(self.book.active_id())),
            (PROFILES_KEY, Value::Array(stored)),
        ]
    }
}

/// The unwanted-file clean-up as the settings hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupChoices {
    /// The clean-up is on for every profile.
    pub enabled: bool,
    /// The file types as patterns the renamer and executor read (regular expressions).
    pub patterns: Vec<String>,
}

impl CleanupChoices {
    /// Reads the switch and the file types; anything missing or malformed takes its default.
    pub fn from_values(values: &Values) -> Self {
        let text = values
            .get(UNWANTED_KEY)
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_UNWANTED_TEXT);
        let mut patterns = unwanted_patterns(text);
        if patterns.is_empty() {
            patterns = unwanted_patterns(DEFAULT_UNWANTED_TEXT);
        }
        CleanupChoices {
            enabled: values
                .get(CLEANUP_KEY)
                .and_then(Value::as_bool)
                .unwrap_or(false),
            patterns,
        }
    }
}

/// The unwanted-file patterns of the text a person types: entries separated by commas or
/// semicolons; an extension (`.txt`, `*.nfo.bak` or a bare `sfv`) matches the end of the file
/// name, anything else is a regular expression on the file name (ignoring case). An expression
/// that is not valid is left out.
pub fn unwanted_patterns(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in text.split([',', ';', '\n']).map(str::trim) {
        if entry.is_empty() {
            continue;
        }
        let plain = entry.trim_start_matches('*');
        let is_extension = plain
            .trim_start_matches('.')
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
            && !plain.trim_start_matches('.').is_empty()
            && (plain.starts_with('.') || !plain.contains('.'));
        let pattern = if is_extension {
            format!(
                "{}$",
                regex::escape(&format!(".{}", plain.trim_start_matches('.')))
            )
        } else if regex::Regex::new(entry).is_ok() {
            entry.to_owned()
        } else {
            continue;
        };
        if !out.contains(&pattern) {
            out.push(pattern);
        }
    }
    out
}

/// The profile Organize plans with: the active profile with the NFO and artwork file names of the
/// settings and the library-wide clean-up switch applied. The preview, the rename after a scrape
/// and the organize-state cache all plan with this one.
pub fn active_profile_from_values(values: &Values) -> RenameProfile {
    let mut profile = RenameSettings::from_values(values).active_profile_with_artwork(
        &NfoSettings::from_values(values),
        &ArtworkNames::from_values(values),
    );
    apply_cleanup_choices(&mut profile, &CleanupChoices::from_values(values));
    apply_subfolder_choice(&mut profile, keep_subfolders(values));
    apply_colon_choice(&mut profile, values);
    profile
}

/// Whether renames keep the sub-folders titles sit in: on unless the switch is turned off, for
/// new installs and for installs from before the switch existed alike.
#[must_use]
pub fn keep_subfolders(values: &Values) -> bool {
    values
        .get(KEEP_SUBFOLDERS_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// What the colon is written as for the `naming.colon` choice: nothing (`remove`), `-` (`dash`,
/// which also turns `: ` into ` - `) or the look-alike colon U+A789 (`lookalike`). `None` for
/// anything else, so an unset or hand-edited value leaves the profile's own choice.
#[must_use]
pub fn colon_replacement(values: &Values) -> Option<&'static str> {
    match values.get(COLON_KEY).and_then(Value::as_str)? {
        "remove" => Some(""),
        "dash" => Some("-"),
        "lookalike" => Some("\u{a789}"),
        _ => None,
    }
}

/// Sets how a colon is written on a profile's movie and TV settings from the `naming.colon`
/// setting, when it has one. Every caller that plans a rename with the user's settings applies
/// it, next to the sub-folder choice.
pub fn apply_colon_choice(profile: &mut RenameProfile, values: &Values) {
    if let Some(replacement) = colon_replacement(values) {
        replacement.clone_into(&mut profile.movie.name.colon_replacement);
        replacement.clone_into(&mut profile.tv.name.colon_replacement);
    }
}

/// Sets "Keep sub-folders" on a profile's movie and TV settings. Every caller that plans a rename
/// with the user's settings applies it, so Organize, its preview, rename after scrape and the
/// automation steps place folders alike.
pub fn apply_subfolder_choice(profile: &mut RenameProfile, keep: bool) {
    profile.movie.keep_subfolders = keep;
    profile.tv.keep_subfolders = keep;
}

/// Turns the unwanted-file clean-up on in a profile when the library-wide switch is on, with the
/// file types of the settings. When it is off the profile keeps its own choice.
pub fn apply_cleanup_choices(profile: &mut RenameProfile, cleanup: &CleanupChoices) {
    if !cleanup.enabled {
        return;
    }
    profile.movie.cleanup_unwanted = true;
    profile
        .movie
        .unwanted_patterns
        .clone_from(&cleanup.patterns);
    profile.tv.cleanup_unwanted = true;
    profile.tv.unwanted_patterns.clone_from(&cleanup.patterns);
}

/// Copies the NFO file names of the NFO settings into a profile's movie and TV settings.
pub fn apply_nfo_choices(profile: &mut RenameProfile, nfo: &NfoSettings) {
    profile.movie.nfo = nfo
        .movie_namings
        .iter()
        .map(|n| {
            match n {
                MovieNfoNaming::VideoBase => "FILENAME_NFO",
                MovieNfoNaming::MovieNfo => "MOVIE_NFO",
            }
            .to_owned()
        })
        .collect();
    profile.movie.disc_nfo_in_folder = nfo.nfo_in_disc_folder;
    profile.tv.season_nfo = nfo
        .season_namings
        .iter()
        .map(|n| {
            match n {
                SeasonNfoNaming::Season => "SEASON",
                SeasonNfoNaming::SeasonFolder => "SEASON_FOLDER",
            }
            .to_owned()
        })
        .collect();
}

/// Copies the artwork file names of the artwork settings into a profile's movie and TV settings.
/// Trailer names are not artwork and stay as the profile has them.
pub fn apply_artwork_choices(profile: &mut RenameProfile, artwork: &ArtworkNames) {
    let m = &artwork.movie;
    profile.movie.poster.clone_from(&m.poster);
    profile.movie.fanart.clone_from(&m.fanart);
    profile.movie.extra_fanart.clone_from(&m.extra_fanart);
    profile.movie.banner.clone_from(&m.banner);
    profile.movie.clearart.clone_from(&m.clearart);
    profile.movie.thumb.clone_from(&m.thumb);
    profile.movie.clearlogo.clone_from(&m.clearlogo);
    profile.movie.discart.clone_from(&m.discart);
    profile.movie.keyart.clone_from(&m.keyart);
    profile.movie.squareart.clone_from(&m.squareart);

    let t = &artwork.tv;
    profile.tv.poster.clone_from(&t.poster);
    profile.tv.fanart.clone_from(&t.fanart);
    profile.tv.banner.clone_from(&t.banner);
    profile.tv.clearart.clone_from(&t.clearart);
    profile.tv.clearlogo.clone_from(&t.clearlogo);
    profile.tv.discart.clone_from(&t.discart);
    profile.tv.characterart.clone_from(&t.characterart);
    profile.tv.keyart.clone_from(&t.keyart);
    profile.tv.thumb.clone_from(&t.thumb);
    profile.tv.squareart.clone_from(&t.squareart);
    profile.tv.season_poster.clone_from(&t.season_poster);
    profile.tv.season_fanart.clone_from(&t.season_fanart);
    profile.tv.season_banner.clone_from(&t.season_banner);
    profile.tv.season_thumb.clone_from(&t.season_thumb);
    profile.tv.episode_thumb.clone_from(&t.episode_thumb);
}

// ---- the stored form -----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StoredSpace {
    enabled: bool,
    replacement: String,
}

impl Default for StoredSpace {
    fn default() -> Self {
        StoredSpace::from(&SpaceSubstitution::default())
    }
}

impl From<&SpaceSubstitution> for StoredSpace {
    fn from(s: &SpaceSubstitution) -> Self {
        StoredSpace {
            enabled: s.enabled,
            replacement: s.replacement.clone(),
        }
    }
}

impl From<StoredSpace> for SpaceSubstitution {
    fn from(s: StoredSpace) -> Self {
        SpaceSubstitution {
            enabled: s.enabled,
            replacement: s.replacement,
        }
    }
}

/// The naming options of a movie profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StoredMovie {
    folder_pattern: String,
    file_pattern: String,
    rename_folder: bool,
    rename_files: bool,
    folder_space: StoredSpace,
    file_space: StoredSpace,
    colon_replacement: String,
    first_character_number_replacement: String,
    ascii_replacement: bool,
    unicode_replacement: bool,
    nfo_cleanup: bool,
    cleanup_unwanted: bool,
    unwanted_patterns: Vec<String>,
    create_movieset_for_single_movie: bool,
    subtitle_language_style: String,
}

impl Default for StoredMovie {
    fn default() -> Self {
        StoredMovie::from(&Preset::Clean.movie_settings())
    }
}

impl From<&MovieSettings> for StoredMovie {
    fn from(s: &MovieSettings) -> Self {
        StoredMovie {
            folder_pattern: s.folder_pattern.clone(),
            file_pattern: s.file_pattern.clone(),
            rename_folder: s.rename_folder,
            rename_files: s.rename_files,
            folder_space: (&s.folder_space).into(),
            file_space: (&s.file_space).into(),
            colon_replacement: s.name.colon_replacement.clone(),
            first_character_number_replacement: s.name.first_character_number_replacement.clone(),
            ascii_replacement: s.name.ascii_replacement,
            unicode_replacement: s.name.unicode_replacement,
            nfo_cleanup: s.nfo_cleanup,
            cleanup_unwanted: s.cleanup_unwanted,
            unwanted_patterns: s.unwanted_patterns.clone(),
            create_movieset_for_single_movie: s.create_movieset_for_single_movie,
            subtitle_language_style: s.subtitle_language_style.name().to_owned(),
        }
    }
}

impl StoredMovie {
    fn into_settings(self) -> MovieSettings {
        let mut s = Preset::Clean.movie_settings();
        s.folder_pattern = self.folder_pattern;
        s.file_pattern = self.file_pattern;
        s.rename_folder = self.rename_folder;
        s.rename_files = self.rename_files;
        s.folder_space = self.folder_space.into();
        s.file_space = self.file_space.into();
        s.name.colon_replacement = self.colon_replacement;
        s.name.first_character_number_replacement = self.first_character_number_replacement;
        s.name.ascii_replacement = self.ascii_replacement;
        s.name.unicode_replacement = self.unicode_replacement;
        s.nfo_cleanup = self.nfo_cleanup;
        s.cleanup_unwanted = self.cleanup_unwanted;
        s.unwanted_patterns = self.unwanted_patterns;
        s.create_movieset_for_single_movie = self.create_movieset_for_single_movie;
        if let Some(style) = SubtitleLanguageStyle::parse(&self.subtitle_language_style) {
            s.subtitle_language_style = style;
        }
        s
    }
}

/// The naming options of a TV profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StoredTv {
    show_pattern: String,
    season_pattern: String,
    file_pattern: String,
    rename_show_folder: bool,
    rename_season_folder: bool,
    rename_files: bool,
    show_space: StoredSpace,
    season_space: StoredSpace,
    file_space: StoredSpace,
    colon_replacement: String,
    first_character_number_replacement: String,
    ascii_replacement: bool,
    unicode_replacement: bool,
    multi_episode_style: String,
    special_season: bool,
    cleanup_unwanted: bool,
    unwanted_patterns: Vec<String>,
    subtitle_language_style: String,
}

impl Default for StoredTv {
    fn default() -> Self {
        StoredTv::from(&Preset::Clean.tv_settings())
    }
}

impl From<&TvSettings> for StoredTv {
    fn from(s: &TvSettings) -> Self {
        StoredTv {
            show_pattern: s.show_pattern.clone(),
            season_pattern: s.season_pattern.clone(),
            file_pattern: s.file_pattern.clone(),
            rename_show_folder: s.rename_show_folder,
            rename_season_folder: s.rename_season_folder,
            rename_files: s.rename_files,
            show_space: (&s.show_space).into(),
            season_space: (&s.season_space).into(),
            file_space: (&s.file_space).into(),
            colon_replacement: s.name.colon_replacement.clone(),
            first_character_number_replacement: s.name.first_character_number_replacement.clone(),
            ascii_replacement: s.name.ascii_replacement,
            unicode_replacement: s.name.unicode_replacement,
            multi_episode_style: s.multi_episode_style.name().to_owned(),
            special_season: s.special_season,
            cleanup_unwanted: s.cleanup_unwanted,
            unwanted_patterns: s.unwanted_patterns.clone(),
            subtitle_language_style: s.subtitle_language_style.name().to_owned(),
        }
    }
}

impl StoredTv {
    fn into_settings(self) -> TvSettings {
        let mut s = Preset::Clean.tv_settings();
        s.show_pattern = self.show_pattern;
        s.season_pattern = self.season_pattern;
        s.file_pattern = self.file_pattern;
        s.rename_show_folder = self.rename_show_folder;
        s.rename_season_folder = self.rename_season_folder;
        s.rename_files = self.rename_files;
        s.show_space = self.show_space.into();
        s.season_space = self.season_space.into();
        s.file_space = self.file_space.into();
        s.name.colon_replacement = self.colon_replacement;
        s.name.first_character_number_replacement = self.first_character_number_replacement;
        s.name.ascii_replacement = self.ascii_replacement;
        s.name.unicode_replacement = self.unicode_replacement;
        if let Some(style) = MultiEpisodeStyle::parse(&self.multi_episode_style) {
            s.multi_episode_style = style;
        }
        s.special_season = self.special_season;
        s.cleanup_unwanted = self.cleanup_unwanted;
        s.unwanted_patterns = self.unwanted_patterns;
        if let Some(style) = SubtitleLanguageStyle::parse(&self.subtitle_language_style) {
            s.subtitle_language_style = style;
        }
        s
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StoredProfile {
    id: String,
    name: String,
    movie: StoredMovie,
    tv: StoredTv,
}

impl StoredProfile {
    fn from_profile(p: &RenameProfile) -> Self {
        StoredProfile {
            id: p.id.clone(),
            name: p.name.clone(),
            movie: (&p.movie).into(),
            tv: (&p.tv).into(),
        }
    }

    fn into_profile(self) -> RenameProfile {
        RenameProfile {
            id: self.id,
            name: self.name,
            preset: None,
            movie: self.movie.into_settings(),
            tv: self.tv.into_settings(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_nfo::MovieNfoNaming;
    use serde_json::json;

    #[test]
    fn keep_subfolders_is_on_unless_turned_off() {
        let fresh = active_profile_from_values(&Values::new());
        assert!(fresh.movie.keep_subfolders && fresh.tv.keep_subfolders);
        let mut values = Values::new();
        values.insert(KEEP_SUBFOLDERS_KEY.to_owned(), json!(false));
        let off = active_profile_from_values(&values);
        assert!(!off.movie.keep_subfolders && !off.tv.keep_subfolders);
        // A custom profile follows the switch too.
        values.insert(ACTIVE_PROFILE_KEY.to_owned(), json!("detailed"));
        assert!(!active_profile_from_values(&values).movie.keep_subfolders);
        values.insert(KEEP_SUBFOLDERS_KEY.to_owned(), json!("garbage"));
        assert!(active_profile_from_values(&values).movie.keep_subfolders);
    }

    #[test]
    fn the_colon_choice_sets_how_a_colon_is_written() {
        let name = |values: &Values| {
            let profile = active_profile_from_values(values);
            (
                cv_rename::clean::replace_invalid_characters(
                    "Dune: Part Two",
                    &profile.movie.name.colon_replacement,
                    false,
                ),
                profile.tv.name.colon_replacement,
            )
        };
        // Unset keeps today's names: a colon and a space become " - ".
        assert_eq!(name(&Values::new()).0, "Dune - Part Two");
        let with = |choice: &str| values(&[(COLON_KEY, json!(choice))]);
        assert_eq!(name(&with("dash")).0, "Dune - Part Two");
        assert_eq!(name(&with("remove")).0, "Dune Part Two");
        assert_eq!(
            name(&with("lookalike")),
            ("Dune\u{a789} Part Two".to_owned(), "\u{a789}".to_owned())
        );
        // Anything else leaves the profile's own choice.
        assert_eq!(name(&with("garbage")).0, "Dune - Part Two");
    }

    fn values(pairs: &[(&str, Value)]) -> Values {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn the_clean_up_switch_turns_unwanted_files_on_with_the_typed_file_types() {
        let mut profile = RenameSettings::default().book.active();
        apply_cleanup_choices(&mut profile, &CleanupChoices::from_values(&Values::new()));
        assert!(!profile.movie.cleanup_unwanted, "off on a fresh install");
        assert!(
            profile
                .movie
                .exec_options("b1")
                .unwanted_patterns
                .is_empty()
        );

        let cleanup = CleanupChoices::from_values(&values(&[
            (CLEANUP_KEY, json!(true)),
            (UNWANTED_KEY, json!(".txt, url ; *.nfo.bak, ^RARBG, [bad")),
        ]));
        apply_cleanup_choices(&mut profile, &cleanup);
        assert!(profile.movie.cleanup_unwanted && profile.tv.cleanup_unwanted);
        let patterns = profile.movie.exec_options("b1").unwanted_patterns;
        assert_eq!(patterns.len(), 4, "{patterns:?}");
        let matches = |name: &str| {
            patterns.iter().any(|p| {
                regex::RegexBuilder::new(p)
                    .case_insensitive(true)
                    .build()
                    .unwrap()
                    .is_match(name)
            })
        };
        assert!(matches("RARBG.txt"));
        assert!(matches("Visit us.URL"));
        assert!(matches("Fargo (1996).nfo.bak"));
        assert!(matches("RARBG_DO_NOT_MIRROR.exe"));
        assert!(!matches("Fargo (1996).nfo"));
        assert!(!matches("Fargo (1996).mkv"));

        // Empty text falls back to the default file types.
        let fallback = CleanupChoices::from_values(&values(&[
            (CLEANUP_KEY, json!(true)),
            (UNWANTED_KEY, json!(" , ")),
        ]));
        assert_eq!(fallback.patterns.len(), 4);
    }

    #[test]
    fn a_fresh_install_uses_the_clean_profile() {
        let s = RenameSettings::from_values(&Values::new());
        assert_eq!(s.book.active_id(), "clean");
        assert!(s.book.custom().is_empty());
        let profile = s.active_profile(&NfoSettings::default());
        assert_eq!(profile.preset, Some(Preset::Clean));
        assert_eq!(profile.movie.folder_pattern, "${title} (${year})");
        assert!(!profile.movie.cleanup_unwanted);
    }

    #[test]
    fn custom_profiles_survive_a_save_and_load() {
        let mut book = ProfileBook::default();
        let made = book.create("Archive", "detailed").unwrap();
        let mut movie = made.movie.clone();
        movie.folder_space = SpaceSubstitution::with(".");
        movie.cleanup_unwanted = true;
        movie.name.colon_replacement = "_".into();
        movie.subtitle_language_style = SubtitleLanguageStyle::Iso2;
        let mut tv = made.tv.clone();
        tv.multi_episode_style = MultiEpisodeStyle::Range;
        tv.special_season = false;
        book.update(&made.id, movie, tv).unwrap();
        book.set_active(&made.id).unwrap();
        let before = RenameSettings { book };

        let stored: Values = before
            .to_values()
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect();
        // Through the JSON text the settings file holds.
        let text = serde_json::to_string(&stored).unwrap();
        let after = RenameSettings::from_values(&serde_json::from_str(&text).unwrap());
        assert_eq!(after, before);
        assert_eq!(after.book.active_id(), "custom-archive");
    }

    #[test]
    fn missing_keys_take_the_clean_values_and_bad_choices_are_ignored() {
        let s = RenameSettings::from_values(&values(&[
            (
                PROFILES_KEY,
                json!([
                    { "id": "custom-x", "name": "X",
                      "movie": { "folderPattern": "${title}", "subtitleLanguageStyle": "KLINGON" },
                      "tv": { "multiEpisodeStyle": "WHATEVER" } },
                    "not a profile",
                    { "id": "custom-y" }
                ]),
            ),
            (ACTIVE_PROFILE_KEY, json!("custom-x")),
        ]));
        let x = s.book.get("custom-x").unwrap();
        assert_eq!(x.movie.folder_pattern, "${title}");
        assert_eq!(
            x.movie.file_pattern,
            Preset::Clean.movie_settings().file_pattern
        );
        assert_eq!(
            x.movie.subtitle_language_style,
            SubtitleLanguageStyle::Iso3T
        );
        assert_eq!(x.tv.multi_episode_style, MultiEpisodeStyle::Repeat);
        // `custom-y` has no name, so it is dropped.
        assert!(s.book.get("custom-y").is_none());
        assert_eq!(s.book.active_id(), "custom-x");
    }

    #[test]
    fn an_unknown_active_profile_falls_back_to_clean() {
        let s = RenameSettings::from_values(&values(&[(ACTIVE_PROFILE_KEY, json!("deleted"))]));
        assert_eq!(s.book.active_id(), "clean");
        let s = RenameSettings::from_values(&values(&[(ACTIVE_PROFILE_KEY, json!(7))]));
        assert_eq!(s.book.active_id(), "clean");
    }

    #[test]
    fn the_nfo_settings_decide_the_nfo_names_and_the_disc_option() {
        let settings = RenameSettings::default();
        let defaults = settings.active_profile(&NfoSettings::default());
        assert_eq!(defaults.movie.nfo, ["FILENAME_NFO"]);
        assert_eq!(defaults.tv.season_nfo, ["SEASON"]);
        // The renamer's own default is on; the NFO option (off by default) wins.
        assert!(!defaults.movie.disc_nfo_in_folder);

        let nfo = NfoSettings::from_values(&values(&[
            ("metadata.nfo-movie-names", json!("both")),
            ("metadata.nfo-season-names", json!("season-folder")),
            ("metadata.nfo-disc-folder", json!(true)),
        ]));
        assert_eq!(
            nfo.movie_namings,
            [MovieNfoNaming::VideoBase, MovieNfoNaming::MovieNfo]
        );
        let profile = settings.active_profile(&nfo);
        assert_eq!(profile.movie.nfo, ["FILENAME_NFO", "MOVIE_NFO"]);
        assert_eq!(profile.tv.season_nfo, ["SEASON_FOLDER"]);
        assert!(profile.movie.disc_nfo_in_folder);
    }

    #[test]
    fn the_artwork_settings_decide_the_artwork_names() {
        let settings = RenameSettings::default();
        let nfo = NfoSettings::default();
        // The defaults of the artwork settings are the renamer's own defaults.
        let defaults = settings.active_profile_with_artwork(&nfo, &ArtworkNames::default());
        let plain = settings.active_profile(&nfo);
        assert_eq!(defaults.movie.poster, plain.movie.poster);
        assert_eq!(defaults.movie.thumb, plain.movie.thumb);
        assert_eq!(defaults.movie.discart, plain.movie.discart);
        assert_eq!(defaults.movie.extra_fanart, plain.movie.extra_fanart);
        assert_eq!(defaults.tv.poster, plain.tv.poster);
        assert_eq!(defaults.tv.season_poster, plain.tv.season_poster);
        assert_eq!(defaults.tv.episode_thumb, plain.tv.episode_thumb);

        let artwork = ArtworkNames::from_values(&values(&[
            ("metadata.artwork-names", json!("both")),
            (
                "artwork.names.movie.extra-fanart",
                json!(["folder_extrafanart"]),
            ),
            ("artwork.names.tv.season-poster", json!(["season_folder"])),
        ]));
        let profile = settings.active_profile_with_artwork(&nfo, &artwork);
        assert_eq!(profile.movie.poster, ["FILENAME_POSTER", "POSTER"]);
        assert_eq!(profile.movie.fanart, ["FILENAME_FANART", "FANART"]);
        assert_eq!(profile.movie.extra_fanart, ["FOLDER_EXTRAFANART"]);
        assert_eq!(profile.tv.season_poster, ["SEASON_FOLDER"]);
        // The NFO names and the trailer names are not touched.
        assert_eq!(profile.movie.nfo, plain.movie.nfo);
        assert_eq!(profile.movie.trailer, plain.movie.trailer);
    }

    #[test]
    fn the_profile_option_for_unwanted_files_reaches_the_executor() {
        let mut book = ProfileBook::default();
        let made = book.create("Tidy", "clean").unwrap();
        let mut movie = made.movie.clone();
        movie.cleanup_unwanted = true;
        book.update(&made.id, movie, made.tv.clone()).unwrap();
        book.set_active(&made.id).unwrap();
        let settings = RenameSettings { book };
        let profile = settings.active_profile(&NfoSettings::default());
        let options = profile.movie.exec_options("b1");
        assert!(!options.unwanted_patterns.is_empty());
        // Off in a fresh install: nothing is cleaned.
        let fresh = RenameSettings::default().active_profile(&NfoSettings::default());
        assert!(fresh.movie.exec_options("b1").unwanted_patterns.is_empty());
        assert!(fresh.tv.exec_options("b1").unwanted_patterns.is_empty());
    }
}

//! The naming presets: Clean, Detailed, Media-server safe and Release info, for movies and TV shows.
//!
//! A preset is a set of patterns with the option values that go with them. It builds the
//! [`MovieSettings`] and [`TvSettings`] a planner reads (`Preset::movie_settings`,
//! `Preset::tv_settings`); a custom profile starts as a copy of one (see [`crate::profile`]).
//!
//! | Preset | Movie folder | Movie file |
//! |---|---|---|
//! | Clean (default) | `Title (Year)` | `Title - Edition (Year)` |
//! | Detailed | `Title (Year) [Resolution] [Source] [HDR] [Audio Channels]` | the same tags |
//! | Media-server safe | `Title (Year) {tmdb-ID}` | `Title (Year) {edition-Edition}` |
//! | Release info | `Title (Year)` | `Title - Edition (Year) [Release tags]-Group` |
//!
//! The TV presets are the equivalents for the show folder, the season folder and the episode
//! file. A tag that has no value (no HDR, no edition, no TMDB id) leaves nothing behind: empty
//! brackets are removed by the name clean-up, and the `{...}` tags of the media-server preset are
//! only written when the value exists.
//!
//! The tag values are the ones of the compatibility fixtures: the source is `Web-DL` or
//! `Blu-ray`, the audio codec `DTSHD-MA` or `TrueHD Atmos`, the channels `7.1`.

use crate::options::{MovieSettings, TvSettings};

/// A naming preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Preset {
    /// `Title (Year)`: short names that every media center reads.
    Clean,
    /// Resolution, source, HDR and audio tags in the name.
    Detailed,
    /// An id tag in the folder and an edition tag in the file, for servers that match by id.
    MediaServerSafe,
    /// Clean folders, and file names that keep the release tags and group of the download name,
    /// for libraries a download manager also reads.
    ReleaseInfo,
}

/// The movie folder of [`Preset::Clean`].
pub const CLEAN_MOVIE_FOLDER: &str = "${title} (${year})";
/// The movie file of [`Preset::Clean`].
pub const CLEAN_MOVIE_FILE: &str = "${title}${ - ,edition,} (${year})";
/// The movie folder of [`Preset::Detailed`].
pub const DETAILED_MOVIE_FOLDER: &str = "${title} (${year}) [${videoFormat}] [${mediaSource}] [${hdrformat}] [${audioCodec} ${audioChannelsDot}]";
/// The movie file of [`Preset::Detailed`]: the folder's tags.
pub const DETAILED_MOVIE_FILE: &str = DETAILED_MOVIE_FOLDER;
/// The movie folder of [`Preset::MediaServerSafe`]; the id tag is left out when there is no id.
pub const SAFE_MOVIE_FOLDER: &str = "${title} (${year})${if tmdb > 0} {tmdb-${tmdb}}${end}";
/// The movie file of [`Preset::MediaServerSafe`]; the edition tag is left out without an edition.
pub const SAFE_MOVIE_FILE: &str = "${title} (${year})${if edition} {edition-${edition}}${end}";

/// The movie file of [`Preset::ReleaseInfo`]: the Clean name with the release tags and group of
/// the original file name; a name without them gets the Clean name.
pub const RELEASE_MOVIE_FILE: &str =
    "${title}${ - ,edition,} (${year}) [${releaseTags}]${-,releaseGroup,}";

/// The show folder of [`Preset::Clean`], [`Preset::Detailed`] and [`Preset::ReleaseInfo`].
pub const CLEAN_SHOW_FOLDER: &str = "${showTitle} (${showYear})";
/// The season folder of every preset.
pub const SEASON_FOLDER: &str = "Season ${seasonNr2}";
/// The episode file of [`Preset::Clean`].
pub const CLEAN_EPISODE_FILE: &str = "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}";
/// The episode file of [`Preset::Detailed`].
pub const DETAILED_EPISODE_FILE: &str = "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title} [${videoFormat}] [${mediaSource}] [${hdrformat}] [${audioCodec} ${audioChannelsDot}]";
/// The show folder of [`Preset::MediaServerSafe`]; the id tag is left out when there is no id.
pub const SAFE_SHOW_FOLDER: &str =
    "${showTitle} (${showYear})${if showTmdb > 0} {tmdb-${showTmdb}}${end}";
/// The episode file of [`Preset::MediaServerSafe`]: no title, so a retitled episode keeps its name.
pub const SAFE_EPISODE_FILE: &str = "${showTitle} - S${seasonNr2}E${episodeNr2}";
/// The episode file of [`Preset::ReleaseInfo`].
pub const RELEASE_EPISODE_FILE: &str =
    "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title} [${releaseTags}]${-,releaseGroup,}";

impl Preset {
    /// The presets in the order the app lists them.
    pub const ALL: [Preset; 4] = [
        Preset::Clean,
        Preset::Detailed,
        Preset::MediaServerSafe,
        Preset::ReleaseInfo,
    ];

    /// The stable id stored in the settings.
    pub fn id(self) -> &'static str {
        match self {
            Preset::Clean => "clean",
            Preset::Detailed => "detailed",
            Preset::MediaServerSafe => "media-server-safe",
            Preset::ReleaseInfo => "release-info",
        }
    }

    /// The name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Preset::Clean => "Clean",
            Preset::Detailed => "Detailed",
            Preset::MediaServerSafe => "Media-server safe",
            Preset::ReleaseInfo => "Release info",
        }
    }

    /// The preset with this id, ignoring case.
    pub fn parse(id: &str) -> Option<Preset> {
        let id = id.trim();
        Preset::ALL
            .into_iter()
            .find(|p| p.id().eq_ignore_ascii_case(id))
    }

    /// The movie folder and file patterns.
    pub fn movie_patterns(self) -> (&'static str, &'static str) {
        match self {
            Preset::Clean => (CLEAN_MOVIE_FOLDER, CLEAN_MOVIE_FILE),
            Preset::Detailed => (DETAILED_MOVIE_FOLDER, DETAILED_MOVIE_FILE),
            Preset::MediaServerSafe => (SAFE_MOVIE_FOLDER, SAFE_MOVIE_FILE),
            Preset::ReleaseInfo => (CLEAN_MOVIE_FOLDER, RELEASE_MOVIE_FILE),
        }
    }

    /// The show folder, season folder and episode file patterns.
    pub fn tv_patterns(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Preset::Clean => (CLEAN_SHOW_FOLDER, SEASON_FOLDER, CLEAN_EPISODE_FILE),
            Preset::Detailed => (CLEAN_SHOW_FOLDER, SEASON_FOLDER, DETAILED_EPISODE_FILE),
            Preset::MediaServerSafe => (SAFE_SHOW_FOLDER, SEASON_FOLDER, SAFE_EPISODE_FILE),
            Preset::ReleaseInfo => (CLEAN_SHOW_FOLDER, SEASON_FOLDER, RELEASE_EPISODE_FILE),
        }
    }

    /// The movie profile of the preset: its patterns on top of a fresh install's options.
    pub fn movie_settings(self) -> MovieSettings {
        let (folder, file) = self.movie_patterns();
        MovieSettings {
            folder_pattern: folder.to_owned(),
            file_pattern: file.to_owned(),
            ..MovieSettings::default()
        }
    }

    /// The TV profile of the preset: its patterns on top of a fresh install's options, with the
    /// name clean-up of the movie profile. A colon followed by a space becomes ` - ` in episode
    /// and show names as it does in movie names (`The Accountants: Part 1` is
    /// `The Accountants - Part 1`), where the bare TV defaults drop it.
    pub fn tv_settings(self) -> TvSettings {
        let (show, season, file) = self.tv_patterns();
        let defaults = TvSettings::default();
        TvSettings {
            show_pattern: show.to_owned(),
            season_pattern: season.to_owned(),
            file_pattern: file.to_owned(),
            name: crate::options::NameOptions {
                colon_replacement: MovieSettings::default().name.colon_replacement,
                ..defaults.name.clone()
            },
            ..defaults
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for preset in Preset::ALL {
            assert_eq!(Preset::parse(preset.id()), Some(preset));
        }
        assert_eq!(Preset::parse(" CLEAN "), Some(Preset::Clean));
        assert_eq!(Preset::parse("custom"), None);
    }

    #[test]
    fn detailed_file_carries_the_tags_of_the_folder() {
        let (folder, file) = Preset::Detailed.movie_patterns();
        assert_eq!(folder, file);
        assert!(folder.ends_with("[${audioCodec} ${audioChannelsDot}]"));
    }

    #[test]
    fn a_colon_in_an_episode_title_becomes_a_dash_as_in_a_movie_title() {
        use cv_domain::{EpisodeNumber, Item, ItemDetail, ItemId};

        use crate::TvRenamer;

        let mut show = Item::show("The Office (US)");
        show.id = Some(ItemId(1));
        show.core.year = 2005;
        let mut special = Item::episode(ItemId(1), "The Accountants: The Books Don't Balance");
        if let ItemDetail::Episode(fields) = &mut special.detail {
            fields.numbers.set(EpisodeNumber::aired(0, 1));
        }
        for preset in Preset::ALL {
            let name = TvRenamer::new(preset.tv_settings())
                .episode_file_name_for(CLEAN_EPISODE_FILE, &show, &[&special])
                .unwrap();
            assert_eq!(
                name, "The Office (US) - S00E01 - The Accountants - The Books Don't Balance",
                "{preset:?}"
            );
        }
    }

    #[test]
    fn presets_build_settings_with_their_patterns() {
        let movie = Preset::MediaServerSafe.movie_settings();
        assert_eq!(movie.folder_pattern, SAFE_MOVIE_FOLDER);
        assert_eq!(movie.file_pattern, SAFE_MOVIE_FILE);
        let tv = Preset::Detailed.tv_settings();
        assert_eq!(tv.file_pattern, DETAILED_EPISODE_FILE);
        assert_eq!(tv.season_pattern, SEASON_FOLDER);
        assert!(tv.rename_show_folder && tv.rename_season_folder && tv.rename_files);
    }

    #[test]
    fn release_info_keeps_the_release_of_the_original_file_name() {
        use cv_domain::{EpisodeNumber, Item, ItemDetail, ItemId};

        use crate::{MovieRenamer, TvRenamer};

        let movies = MovieRenamer::new(Preset::ReleaseInfo.movie_settings());
        let mut movie = Item::movie("Example Movie");
        movie.core.year = 2020;
        movie.core.original_filename = "Example.Movie.2020.1080p.WEB-DL.x264-GRP.mkv".into();
        assert_eq!(
            movies.folder_name(&movie, None).unwrap(),
            "Example Movie (2020)"
        );
        assert_eq!(
            movies.file_name(&movie, None).unwrap(),
            "Example Movie (2020) [1080p.WEB-DL.x264]-GRP"
        );
        movie.core.original_filename = "Example Movie (2020).mkv".into();
        assert_eq!(
            movies.file_name(&movie, None).unwrap(),
            "Example Movie (2020)"
        );

        let mut show = Item::show("Example Show");
        show.id = Some(ItemId(1));
        show.core.year = 2017;
        let mut episode = Item::episode(ItemId(1), "Pilot");
        episode.core.original_filename =
            "Example.Show.2017.S01E01.Episode.1.Bluray-1080p.German.DL.x265-GRP.mkv".into();
        if let ItemDetail::Episode(fields) = &mut episode.detail {
            fields.numbers.set(EpisodeNumber::aired(1, 1));
        }
        let name = TvRenamer::new(Preset::ReleaseInfo.tv_settings())
            .episode_file_name(&show, &[&episode])
            .unwrap();
        assert_eq!(
            name,
            "Example Show - S01E01 - Pilot [Bluray-1080p.German.DL.x265]-GRP"
        );
    }
}

//! The media-info guard: a title whose names use media-info tokens waits for its video details.
//!
//! A pattern such as `${title} (${year}) [${videoFormat}] [${hdrFormat}]` renders the quality tags
//! from the video's stream details. When those details were never read (the read is pending,
//! timed out or failed), the tokens render empty and the rename would strip the tags from a name
//! that has them, `Heat (1995) [2160p] [HDR10]` becoming `Heat (1995)`. So the planner callers
//! ask [`MovieRenamer::waits_for_media_info`] and [`TvRenamer::waits_for_media_info`] first and
//! hold such a title back with [`MEDIA_HELD_REASON`] until its details are read. Patterns without
//! media tokens never wait.

use std::sync::LazyLock;

use cv_domain::{Item, MediaFile, MediaFileType};
use cv_template::tokens::{AliasMap, movie_aliases, tv_aliases};
use regex::Regex;

use crate::movie::MovieRenamer;
use crate::tv::{TvInput, TvRenamer};

/// Why a title is held back, ready to show.
pub const MEDIA_HELD_REASON: &str =
    "Video details not read yet: rename waits so the name keeps its quality tags";

/// A model field read from the video stream: codec, resolution, aspect ratio, bit depth, frame
/// rate, HDR and 3D.
static VIDEO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:movie|episode|tvShow)\.(?:mediaInfoVideo|mediaInfoAspectRatio|mediaInfoFrameRate|videoHDR|video3DFormat)",
    )
    .expect("valid regex")
});
/// A model field read from the audio streams.
static AUDIO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:movie|episode|tvShow)\.mediaInfoAudio").expect("valid regex")
});
/// A model field read from the embedded subtitle streams.
static SUBTITLES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:movie|episode|tvShow)\.mediaInfoSubtitle").expect("valid regex")
});

/// Which stream details a set of naming patterns reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MediaNeeds {
    pub video: bool,
    pub audio: bool,
    pub subtitles: bool,
}

impl MediaNeeds {
    /// What `patterns` read, with their aliases (`${videoFormat}`) written as model paths.
    #[must_use]
    pub fn of<'a>(patterns: impl IntoIterator<Item = &'a str>, aliases: &AliasMap) -> Self {
        let mut needs = Self::default();
        for pattern in patterns {
            if !pattern.contains("${") {
                continue;
            }
            let morphed = aliases.morph(pattern);
            needs.video |= VIDEO.is_match(&morphed);
            needs.audio |= AUDIO.is_match(&morphed);
            needs.subtitles |= SUBTITLES.is_match(&morphed);
        }
        needs
    }

    /// True when any media-info token is used.
    #[must_use]
    pub const fn any(self) -> bool {
        self.video || self.audio || self.subtitles
    }

    /// True when the video files of one title have what the tokens need: every video was read,
    /// and the first one (which the tokens render from) has a picture size when video tokens are
    /// used and at least one audio stream when audio tokens are used. A title without any video
    /// file has nothing to wait for.
    #[must_use]
    pub fn met_by(self, files: &[MediaFile]) -> bool {
        if !self.any() {
            return true;
        }
        // A disc's marker file next to its disc folder is not a video of its own.
        let mut videos = files
            .iter()
            .filter(|f| f.kind == MediaFileType::Video && !f.is_covered_disc_marker(files));
        let Some(main) = videos.next() else {
            return true;
        };
        if !main.has_media_info() || videos.any(|f| !f.has_media_info()) {
            return false;
        }
        if self.video && (main.width == 0 || main.height == 0) {
            return false;
        }
        !(self.audio && main.audio_streams.is_empty())
    }
}

impl MovieRenamer {
    /// What the folder and file patterns this profile renders read from the video.
    #[must_use]
    pub fn media_needs(&self) -> MediaNeeds {
        let s = self.settings();
        let mut patterns = Vec::new();
        if s.rename_folder {
            patterns.push(s.folder_pattern.as_str());
        }
        if s.rename_files {
            patterns.push(s.file_pattern.as_str());
        }
        MediaNeeds::of(patterns, &movie_aliases())
    }

    /// True when the movie must not be renamed yet: its names use media-info tokens and its video
    /// details are not read (see the module docs).
    #[must_use]
    pub fn waits_for_media_info(&self, movie: &Item) -> bool {
        !self.media_needs().met_by(&movie.core.media_files)
    }
}

impl TvRenamer {
    /// What the show, season and episode patterns this profile renders read from the videos.
    #[must_use]
    pub fn media_needs(&self) -> MediaNeeds {
        let s = self.settings();
        let mut patterns = Vec::new();
        if s.rename_show_folder {
            patterns.push(s.show_pattern.as_str());
        }
        if s.rename_season_folder {
            patterns.push(s.season_pattern.as_str());
        }
        if s.rename_files {
            patterns.push(s.file_pattern.as_str());
        }
        MediaNeeds::of(patterns, &tv_aliases())
    }

    /// True when the show must not be renamed yet: its names use media-info tokens and an
    /// episode's video details are not read. The show is planned as one, so one unread episode
    /// holds it all back.
    #[must_use]
    pub fn waits_for_media_info(&self, input: &TvInput<'_>) -> bool {
        let needs = self.media_needs();
        needs.any()
            && input
                .episodes
                .iter()
                .any(|episode| !needs.met_by(&episode.item.core.media_files))
    }
}

#[cfg(test)]
mod tests {
    use cv_domain::AudioStream;

    use super::*;
    use crate::MovieSettings;

    fn needs(pattern: &str) -> MediaNeeds {
        MediaNeeds::of([pattern], &movie_aliases())
    }

    fn read_video() -> MediaFile {
        let mut file = MediaFile::new(MediaFileType::Video, "D:/Movies/Heat", "Heat.mkv");
        file.container_format = "Matroska".into();
        file.video_codec = "HEVC".into();
        file.width = 3840;
        file.height = 2160;
        file.audio_streams.push(AudioStream {
            codec: "DTS-HD MA".into(),
            channels: 6,
            ..AudioStream::default()
        });
        file
    }

    #[test]
    fn media_tokens_are_found_by_alias_and_by_model_path() {
        assert_eq!(needs("${title} (${year})"), MediaNeeds::default());
        assert!(needs("${title} [${videoFormat}]").video);
        assert!(needs("${title} [${hdrFormat}]").video);
        assert!(needs("${title}${ [,hdr,]}").video);
        assert!(needs("${title} ${movie.mediaInfoVideoCodec}").video);
        assert!(needs("${if videoFormat}x${end}").video);
        let audio = needs("${title} [${audioCodec} ${audioChannelsDot}]");
        assert!(audio.audio && !audio.video);
        assert!(needs("${subtitleLanguagesAsString}").subtitles);
        // The file size and the checksum come from the scan, not from MediaInfo.
        assert!(!needs("${title} ${filesize} ${crc32}").any());
        // Text outside a token is not a token.
        assert!(!needs("videoFormat").any());
    }

    #[test]
    fn an_unread_video_does_not_meet_media_needs_and_a_read_one_does() {
        let video = needs("[${videoFormat}] [${audioCodec}]");
        let mut unread = read_video();
        unread.container_format.clear();
        unread.width = 0;
        unread.height = 0;
        unread.audio_streams.clear();
        assert!(!video.met_by(std::slice::from_ref(&unread)));
        assert!(video.met_by(&[read_video()]));
        // A file that was read but lacks the picture size the token needs still waits.
        let mut no_size = read_video();
        no_size.width = 0;
        assert!(!video.met_by(&[no_size]));
        // A second part that is not read holds the movie back too.
        assert!(!video.met_by(&[read_video(), unread.clone()]));
        // Nothing is needed: nothing waits.
        assert!(MediaNeeds::default().met_by(&[unread]));
        // No video at all: nothing to wait for.
        assert!(video.met_by(&[]));
        // A Blu-ray folder that was read, next to its unread `index.bdmv` marker.
        let mut folder = read_video();
        folder.directory = "D:/Movies/Spies".into();
        folder.filename = "BDMV".into();
        let marker = MediaFile::new(MediaFileType::Video, "D:/Movies/Spies/BDMV", "index.bdmv");
        assert!(video.met_by(&[folder, marker]));
    }

    #[test]
    fn a_movie_waits_only_when_its_patterns_use_media_tokens() {
        let mut movie = Item::movie("Killers of the Flower Moon");
        movie.core.year = 2023;
        let mut unread = read_video();
        unread.container_format.clear();
        movie.core.media_files.push(unread);
        let tagged = MovieRenamer::new(MovieSettings {
            folder_pattern: "${title} (${year}) [${videoFormat}] [${hdrFormat}]".into(),
            ..MovieSettings::default()
        });
        assert!(tagged.waits_for_media_info(&movie));
        let plain = MovieRenamer::new(MovieSettings {
            folder_pattern: "${title} (${year})".into(),
            file_pattern: "${title} (${year})".into(),
            ..MovieSettings::default()
        });
        assert!(!plain.waits_for_media_info(&movie));
        // A folder pattern that is not used does not count.
        let folder_off = MovieRenamer::new(MovieSettings {
            folder_pattern: "${title} [${videoFormat}]".into(),
            file_pattern: "${title} (${year})".into(),
            rename_folder: false,
            ..MovieSettings::default()
        });
        assert!(!folder_off.waits_for_media_info(&movie));
        movie.core.media_files[0] = read_video();
        assert!(!tagged.waits_for_media_info(&movie));
    }
}

//! The alias tables: short renamer tokens and the model paths they stand for.

use std::collections::HashMap;
use std::sync::Arc;

/// Short token names mapped to model paths, optionally followed by a renderer (`;bitrate`).
#[derive(Debug, Clone)]
pub struct AliasMap {
    entries: &'static [(&'static str, &'static str)],
    index: Arc<HashMap<&'static str, &'static str>>,
}

impl AliasMap {
    fn new(entries: &'static [(&'static str, &'static str)]) -> AliasMap {
        AliasMap {
            entries,
            index: Arc::new(entries.iter().copied().collect()),
        }
    }

    /// The model path (and renderer) an alias stands for.
    pub fn target(&self, alias: &str) -> Option<&'static str> {
        self.index.get(alias).copied()
    }

    pub fn contains(&self, alias: &str) -> bool {
        self.index.contains_key(alias)
    }

    /// Every `(alias, target)` pair, in table order.
    pub fn entries(&self) -> &'static [(&'static str, &'static str)] {
        self.entries
    }

    /// The alias names, in table order.
    pub fn names(&self) -> impl Iterator<Item = &'static str> {
        self.entries.iter().map(|(name, _)| *name)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The movie aliases. Model roots: `movie`, and `movieSet` for a movie in a collection.
pub fn movie_aliases() -> AliasMap {
    AliasMap::new(MOVIE)
}

/// The TV aliases. Model roots: `tvShow`, `season` and `episode`.
pub fn tv_aliases() -> AliasMap {
    AliasMap::new(TV)
}

static MOVIE: &[(&str, &str)] = &[
    ("title", "movie.title"),
    ("originalTitle", "movie.originalTitle"),
    ("englishTitle", "movie.englishTitle"),
    ("originalFilename", "movie.originalFilename"),
    ("originalBasename", "movie.originalBasename"),
    ("sorttitle", "movie.sortTitle"),
    ("year", "movie.year"),
    ("releaseDate", "movie.releaseDate;date(yyyy-MM-dd)"),
    ("titleSortable", "movie.titleSortable"),
    ("rating", "movie.rating.rating"),
    ("top250", "movie.top250"),
    ("imdb", "movie.imdbId"),
    ("tmdb", "movie.tmdbId"),
    ("certification", "movie.certification"),
    ("language", "movie.spokenLanguages"),
    ("genres", "movie.genres"),
    ("genresAsString", "movie.genresAsString"),
    ("tags", "movie.tags"),
    ("tagsAsString", "movie.tagsAsString"),
    ("actors", "movie.actors"),
    ("producers", "movie.producers"),
    ("directors", "movie.directors"),
    ("writers", "movie.writers"),
    ("productionCompany", "movie.productionCompany"),
    ("productionCompanyAsArray", "movie.productionCompanyAsArray"),
    ("mediaSource", "movie.mediaSource"),
    ("edition", "movie.edition"),
    ("parent", "movie.parent"),
    ("decadeLong", "movie.decadeLong"),
    ("decadeShort", "movie.decadeShort"),
    ("crc32", "movie.CRC32"),
    ("videoCodec", "movie.mediaInfoVideoCodec"),
    ("videoFormat", "movie.mediaInfoVideoFormat"),
    ("aspectRatio", "movie.mediaInfoAspectRatioAsString"),
    ("aspectRatio2", "movie.mediaInfoAspectRatio2AsString"),
    ("videoResolution", "movie.mediaInfoVideoResolution"),
    ("videoBitDepth", "movie.mediaInfoVideoBitDepth"),
    ("videoBitRate", "movie.mediaInfoVideoBitrate;bitrate"),
    ("framerate", "movie.mediaInfoFrameRate;framerate"),
    ("audioCodec", "movie.mediaInfoAudioCodec"),
    ("audioCodecList", "movie.mediaInfoAudioCodecList"),
    ("audioCodecsAsString", "movie.mediaInfoAudioCodecList;array"),
    ("audioChannels", "movie.mediaInfoAudioChannels"),
    ("audioChannelList", "movie.mediaInfoAudioChannelList"),
    (
        "audioChannelsAsString",
        "movie.mediaInfoAudioChannelList;array",
    ),
    ("audioChannelsDot", "movie.mediaInfoAudioChannelsDot"),
    ("audioChannelDotList", "movie.mediaInfoAudioChannelDotList"),
    (
        "audioChannelsDotAsString",
        "movie.mediaInfoAudioChannelDotList;array",
    ),
    ("audioLanguage", "movie.mediaInfoAudioLanguage"),
    ("audioLanguageList", "movie.mediaInfoAudioLanguageList"),
    (
        "audioLanguagesAsString",
        "movie.mediaInfoAudioLanguageList;array",
    ),
    (
        "subtitleLanguageList",
        "movie.mediaInfoSubtitleLanguageList",
    ),
    (
        "subtitleLanguagesAsString",
        "movie.mediaInfoSubtitleLanguageList;array",
    ),
    ("3Dformat", "movie.video3DFormat"),
    ("3Dformat2", "movie.video3DFormat2"),
    ("hdr", "movie.videoHDR"),
    ("hdrformat", "movie.videoHDRFormat"),
    // The spelling the naming presets and the template-engine spec use.
    ("hdrFormat", "movie.videoHDRFormat"),
    ("filesize", "movie.videoFilesize;filesize"),
    ("note", "movie.note"),
    ("movieSetIndex", "movie;indexOfMovieSet"),
    ("movieSetIndex2", "movie;indexOfMovieSetWithDummy"),
];

static TV: &[(&str, &str)] = &[
    // Show
    ("showTitle", "tvShow.title"),
    ("showOriginalTitle", "tvShow.originalTitle"),
    ("showEnglishTitle", "tvShow.englishTitle"),
    ("showTitleSortable", "tvShow.titleSortable"),
    ("showTagline", "tvShow.tagline"),
    ("showYear", "tvShow.year"),
    ("showYearEnd", "tvShow.yearEnd"),
    ("parent", "tvShow.parent"),
    ("showNote", "tvShow.note"),
    ("showStatus", "tvShow.status"),
    ("showRating", "tvShow.rating.rating"),
    ("showTop250", "tvShow.top250"),
    ("showImdb", "tvShow.imdbId"),
    ("showTmdb", "tvShow.tmdbId"),
    ("showTvdb", "tvShow.tvdbId"),
    ("showCertification", "tvShow.certification"),
    ("showTags", "tvShow.tagsAsString"),
    ("showGenres", "tvShow.genres"),
    ("showGenresAsString", "tvShow.genresAsString"),
    ("showProductionCompany", "tvShow.productionCompany"),
    (
        "showProductionCompanyAsArray",
        "tvShow.productionCompanyAsArray",
    ),
    // Season
    ("seasonName", "season.title"),
    // Numbers. A number that is missing in an ordering is -1.
    ("episodeNr", "episode.episode"),
    ("episodeNr2", "episode.episode;number(%02d)"),
    ("episodeNrAired", "episode.airedEpisode"),
    ("episodeNrAired2", "episode.airedEpisode;number(%02d)"),
    ("episodeNrDvd", "episode.dvdEpisode"),
    ("episodeNrDvd2", "episode.dvdEpisode;number(%02d)"),
    ("seasonNr", "episode.season;number(%d)"),
    ("seasonNr2", "episode.season;number(%02d)"),
    ("seasonNrAired", "episode.airedSeason;number(%d)"),
    ("seasonNrAired2", "episode.airedSeason;number(%02d)"),
    ("seasonNrDvd", "episode.dvdSeason;number(%d)"),
    ("seasonNrDvd2", "episode.dvdSeason;number(%02d)"),
    ("absoluteNr", "episode.absoluteNumber"),
    ("absoluteNr2", "episode.absoluteNumber;number(%02d)"),
    // Episode
    ("title", "episode.title"),
    ("originalTitle", "episode.originalTitle"),
    ("englishTitle", "episode.englishTitle"),
    ("originalFilename", "episode.originalFilename"),
    ("originalBasename", "episode.originalBasename"),
    ("folderName", "episode.folderName"),
    ("titleSortable", "episode.titleSortable"),
    ("year", "episode.year"),
    ("airedDate", "episode.firstAired;date(yyyy-MM-dd)"),
    ("episodeRating", "episode.rating.rating"),
    ("episodeImdb", "episode.imdbId"),
    ("episodeTmdb", "episode.tmdbId"),
    ("episodeTvdb", "episode.tvdbId"),
    ("episodeTags", "episode.tagsAsString"),
    ("note", "episode.note"),
    ("crc32", "episode.CRC32"),
    ("mediaSource", "episode.mediaSource"),
    // Media info: the movie names, read from the episode's files.
    ("videoCodec", "episode.mediaInfoVideoCodec"),
    ("videoFormat", "episode.mediaInfoVideoFormat"),
    ("aspectRatio", "episode.mediaInfoAspectRatioAsString"),
    ("aspectRatio2", "episode.mediaInfoAspectRatio2AsString"),
    ("videoResolution", "episode.mediaInfoVideoResolution"),
    ("videoBitDepth", "episode.mediaInfoVideoBitDepth"),
    ("videoBitRate", "episode.mediaInfoVideoBitrate;bitrate"),
    ("framerate", "episode.mediaInfoFrameRate;framerate"),
    ("audioCodec", "episode.mediaInfoAudioCodec"),
    ("audioCodecList", "episode.mediaInfoAudioCodecList"),
    (
        "audioCodecsAsString",
        "episode.mediaInfoAudioCodecList;array",
    ),
    ("audioChannels", "episode.mediaInfoAudioChannels"),
    ("audioChannelList", "episode.mediaInfoAudioChannelList"),
    (
        "audioChannelsAsString",
        "episode.mediaInfoAudioChannelList;array",
    ),
    ("audioChannelsDot", "episode.mediaInfoAudioChannelsDot"),
    (
        "audioChannelDotList",
        "episode.mediaInfoAudioChannelDotList",
    ),
    (
        "audioChannelsDotAsString",
        "episode.mediaInfoAudioChannelDotList;array",
    ),
    ("audioLanguage", "episode.mediaInfoAudioLanguage"),
    ("audioLanguageList", "episode.mediaInfoAudioLanguageList"),
    (
        "audioLanguagesAsString",
        "episode.mediaInfoAudioLanguageList;array",
    ),
    (
        "subtitleLanguageList",
        "episode.mediaInfoSubtitleLanguageList",
    ),
    (
        "subtitleLanguagesAsString",
        "episode.mediaInfoSubtitleLanguageList;array",
    ),
    ("3Dformat", "episode.video3DFormat"),
    ("hdr", "episode.videoHDR"),
    ("hdrformat", "episode.videoHDRFormat"),
    ("hdrFormat", "episode.videoHDRFormat"),
    ("filesize", "episode.videoFilesize;filesize"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_targets_have_a_root() {
        for (map, roots) in [
            (movie_aliases(), &["movie"][..]),
            (tv_aliases(), &["tvShow", "season", "episode"][..]),
        ] {
            let mut seen = std::collections::HashSet::new();
            for (name, target) in map.entries() {
                assert!(seen.insert(*name), "duplicate alias {name}");
                let root = target.split(['.', ';']).next().unwrap();
                assert!(
                    roots.contains(&root),
                    "{name} -> {target} has no known root"
                );
            }
        }
    }

    #[test]
    fn lookups() {
        let movies = movie_aliases();
        assert_eq!(movies.target("title"), Some("movie.title"));
        assert!(movies.contains("videoBitRate"));
        assert!(!movies.contains("seasonNr"));
        assert!(tv_aliases().contains("seasonNr2"));
        assert_eq!(tv_aliases().target("title"), Some("episode.title"));
    }
}

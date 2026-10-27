//! The NFO settings, read from the settings map.
//!
//! The keys are the ids of the settings page (`ui/src/features/settings/schema.ts`); a value that
//! is missing or not one of the page's choices falls back to the default, so a hand-edited
//! settings file can never stop NFO writing. The defaults here and in `schema.ts` are the same
//! (a test pins the Rust side to the writers' own defaults).
//!
//! | Key | Values | Meaning |
//! |---|---|---|
//! | `metadata.nfo-format` | `kodi` (default), `xbmc`, `jellyfin`, `emby`, `plex`, `mediaportal` | movie, set and TV format |
//! | `metadata.nfo-mediaportal-style` | `legacy`, `moving-pictures`, `myvideo` | MediaPortal movie layout |
//! | `metadata.nfo-movie-names` | `video-base` (default), `movie`, `both` | `<video base>.nfo`, `movie.nfo` |
//! | `metadata.nfo-set-names` | `set-name` (default), `set`, `collection`, `all` | `<set>.nfo`, `set.nfo`, `collection.nfo` |
//! | `metadata.nfo-season-names` | `season` (default), `season-folder`, `both` | `seasonNN.nfo`, `<season folder>/season.nfo` |
//! | `metadata.nfo-disc-folder` | bool | NFO inside disc folders |
//! | `metadata.nfo-collection-folder` | path | where set NFOs go; sets are skipped without it |
//! | `metadata.nfo-collection-tmdb-id` | bool | append `-tmdb-<id>` to the set folder |
//! | `metadata.collection-name-replacement` | text (default `_`) | what replaces a character a file name cannot hold in the set folder and file names; empty removes it |
//! | `metadata.nfo-language` | language code | genres and spoken languages |
//! | `metadata.nfo-certification` | `short` (default) ... `technical2` | certification format |
//! | `metadata.nfo-create-outline`, `metadata.nfo-outline-first-sentence` | bool | outline from the plot |
//! | `metadata.nfo-single-studio` | bool | first studio only |
//! | `metadata.nfo-date-added`, `metadata.nfo-date-added-source` | bool, `added`/`file-created`/`file-modified`/`release` | `<dateadded>` |
//! | `metadata.nfo-artwork-urls`, `metadata.nfo-trailer`, `metadata.nfo-fileinfo`, `metadata.nfo-external-audio`, `metadata.nfo-lockdata` | bool | what to write |
//! | `metadata.nfo-all-actors` | bool | all actors in the show NFO (TV) |
//! | `metadata.nfo-episodeguide` | `json` (default), `url`, `off` | `<episodeguide>` style |
//! | `metadata.nfo-end-date` | bool | `<enddate>` of an ended show |
//! | `metadata.nfo-clean` | bool | write clean NFO (keep nothing from the old file) |
//! | `metadata.language` | language code | the scraper language of old-style episode guide URLs |
//! | `ratings.preferred-source` | rating source key | the default rating of movie and show NFOs; unset keeps each writer's own order |

use std::collections::BTreeMap;
use std::path::PathBuf;

use cv_domain::RatingPreference;
use cv_nfo::{
    CertificationStyle, DEFAULT_NAME_REPLACEMENT, DateAddedField, MediaPortalStyle, MovieConnector,
    MovieNfoNaming, MovieNfoOptions, MovieSetConnector, MovieSetNfoNaming, SeasonNfoNaming,
    ShowNfoNaming, TvConnector, TvNfoOptions,
};
use serde_json::Value;

type Values = BTreeMap<String, Value>;

/// Everything the NFO actions read from the settings.
#[derive(Debug, Clone, PartialEq)]
pub struct NfoSettings {
    pub movie_connector: MovieConnector,
    pub set_connector: MovieSetConnector,
    pub tv_connector: TvConnector,
    pub movie_namings: Vec<MovieNfoNaming>,
    pub set_namings: Vec<MovieSetNfoNaming>,
    pub show_namings: Vec<ShowNfoNaming>,
    pub season_namings: Vec<SeasonNfoNaming>,
    /// NFO inside disc folders.
    pub nfo_in_disc_folder: bool,
    /// The collection data folder; set NFOs are written under it.
    pub collection_folder: Option<PathBuf>,
    pub append_tmdb_id_to_set_folder: bool,
    /// What replaces each character a file name cannot hold in a collection name.
    pub set_name_replacement: String,
    pub movie: MovieNfoOptions,
    pub tv: TvNfoOptions,
}

impl Default for NfoSettings {
    fn default() -> Self {
        Self::from_values(&Values::new())
    }
}

fn text<'a>(values: &'a Values, key: &str) -> Option<&'a str> {
    values
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn flag(values: &Values, key: &str, default: bool) -> bool {
    values.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn certification_style(name: &str) -> Option<CertificationStyle> {
    CertificationStyle::parse(&name.replace('-', "_"))
}

fn date_added_field(name: &str) -> Option<DateAddedField> {
    Some(match name {
        "added" => DateAddedField::DateAdded,
        "file-created" => DateAddedField::FileCreationDate,
        "file-modified" => DateAddedField::FileLastModifiedDate,
        "release" => DateAddedField::ReleaseDate,
        _ => return None,
    })
}

impl NfoSettings {
    /// Reads the settings; anything missing or unknown takes its default.
    pub fn from_values(values: &Values) -> Self {
        let format = text(values, "metadata.nfo-format")
            .unwrap_or("kodi")
            .to_ascii_lowercase();
        let style = match text(values, "metadata.nfo-mediaportal-style") {
            Some("moving-pictures") => MediaPortalStyle::MovingPictures,
            Some("myvideo") => MediaPortalStyle::MyVideo,
            _ => MediaPortalStyle::Legacy,
        };
        let movie_connector = match format.as_str() {
            "xbmc" => MovieConnector::Xbmc,
            "jellyfin" => MovieConnector::Jellyfin,
            "emby" => MovieConnector::Emby,
            "mediaportal" => MovieConnector::MediaPortal(style),
            _ => MovieConnector::Kodi,
        };
        let tv_connector = TvConnector::parse(&format).unwrap_or_default();
        let set_connector = if format == "emby" {
            MovieSetConnector::Emby
        } else {
            MovieSetConnector::Kodi
        };

        let movie_namings = match text(values, "metadata.nfo-movie-names") {
            Some("movie") => vec![MovieNfoNaming::MovieNfo],
            Some("both") => vec![MovieNfoNaming::VideoBase, MovieNfoNaming::MovieNfo],
            _ => vec![MovieNfoNaming::VideoBase],
        };
        let set_namings = match text(values, "metadata.nfo-set-names") {
            Some("set") => vec![MovieSetNfoNaming::Kodi2],
            Some("collection") => vec![MovieSetNfoNaming::Emby],
            Some("all") => vec![
                MovieSetNfoNaming::Kodi,
                MovieSetNfoNaming::Kodi2,
                MovieSetNfoNaming::Emby,
            ],
            _ => vec![MovieSetNfoNaming::Kodi],
        };
        let season_namings = match text(values, "metadata.nfo-season-names") {
            Some("season-folder") => vec![SeasonNfoNaming::SeasonFolder],
            Some("both") => vec![SeasonNfoNaming::Season, SeasonNfoNaming::SeasonFolder],
            _ => vec![SeasonNfoNaming::Season],
        };

        let defaults = MovieNfoOptions::default();
        let mut movie = MovieNfoOptions {
            write_date_added: flag(values, "metadata.nfo-date-added", defaults.write_date_added),
            single_studio: flag(values, "metadata.nfo-single-studio", defaults.single_studio),
            lockdata: flag(values, "metadata.nfo-lockdata", defaults.lockdata),
            trailer: flag(values, "metadata.nfo-trailer", defaults.trailer),
            fileinfo: flag(values, "metadata.nfo-fileinfo", defaults.fileinfo),
            artwork_urls: flag(values, "metadata.nfo-artwork-urls", defaults.artwork_urls),
            create_outline: flag(
                values,
                "metadata.nfo-create-outline",
                defaults.create_outline,
            ),
            outline_first_sentence: flag(
                values,
                "metadata.nfo-outline-first-sentence",
                defaults.outline_first_sentence,
            ),
            include_external_audio: flag(
                values,
                "metadata.nfo-external-audio",
                defaults.include_external_audio,
            ),
            clean_nfo: flag(values, "metadata.nfo-clean", defaults.clean_nfo),
            ..defaults
        };
        if let Some(field) =
            text(values, "metadata.nfo-date-added-source").and_then(date_added_field)
        {
            movie.date_added_field = field;
        }
        if let Some(language) = text(values, "metadata.nfo-language") {
            movie.language = language.to_ascii_lowercase();
        }
        if let Some(style) =
            text(values, "metadata.nfo-certification").and_then(certification_style)
        {
            movie.certification_style = style;
        }

        let mut tv = TvNfoOptions::default();
        // TV shares the common options but ranks its own rating sources.
        tv.common = MovieNfoOptions {
            rating_sources: tv.common.rating_sources.clone(),
            ..movie.clone()
        };
        let guide = text(values, "metadata.nfo-episodeguide").unwrap_or("json");
        tv.write_episodeguide = guide != "off";
        tv.new_episodeguide_style = guide != "url";
        tv.write_date_ended = flag(values, "metadata.nfo-end-date", tv.write_date_ended);
        tv.write_all_actors = flag(values, "metadata.nfo-all-actors", tv.write_all_actors);
        if let Some(language) = text(values, "metadata.language") {
            tv.scraper_language = language.to_ascii_lowercase();
        }

        // Choosing a preferred rating source makes it the NFO's default rating, with the fallback
        // order of the library behind it. Until one is chosen each writer keeps its own order.
        if let Some(source) = text(values, "ratings.preferred-source") {
            let order: Vec<String> = RatingPreference::new(source)
                .order()
                .into_iter()
                .map(str::to_owned)
                .collect();
            movie.rating_sources.clone_from(&order);
            tv.common.rating_sources = order;
        }

        Self {
            movie_connector,
            set_connector,
            tv_connector,
            movie_namings,
            set_namings,
            show_namings: vec![ShowNfoNaming::TvShow],
            season_namings,
            nfo_in_disc_folder: flag(values, "metadata.nfo-disc-folder", false),
            collection_folder: text(values, "metadata.nfo-collection-folder").map(PathBuf::from),
            append_tmdb_id_to_set_folder: flag(values, "metadata.nfo-collection-tmdb-id", false),
            set_name_replacement: values
                .get("metadata.collection-name-replacement")
                .and_then(Value::as_str)
                .map_or_else(|| DEFAULT_NAME_REPLACEMENT.to_owned(), str::to_owned),
            movie,
            tv,
        }
    }

    /// The options with the header-comment time set, so the files of one action share it.
    #[must_use]
    pub fn stamped(&self, created_on: &str) -> Self {
        let mut stamped = self.clone();
        stamped.movie.created_on = created_on.to_owned();
        stamped.tv.common.created_on = created_on.to_owned();
        stamped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(pairs: &[(&str, Value)]) -> Values {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn nothing_stored_gives_the_writer_defaults() {
        let s = NfoSettings::default();
        assert_eq!(s.movie, MovieNfoOptions::default());
        assert_eq!(s.movie_connector, MovieConnector::Kodi);
        assert_eq!(s.tv_connector, TvConnector::Kodi);
        assert_eq!(s.movie_namings, [MovieNfoNaming::VideoBase]);
        assert_eq!(s.set_namings, [MovieSetNfoNaming::Kodi]);
        assert_eq!(s.season_namings, [SeasonNfoNaming::Season]);
        assert!(!s.nfo_in_disc_folder);
        assert_eq!(s.collection_folder, None);
        let tv = TvNfoOptions::default();
        assert_eq!(s.tv.common.rating_sources, tv.common.rating_sources);
        assert!(s.tv.write_episodeguide && s.tv.new_episodeguide_style);
        assert!(s.tv.write_date_ended && !s.tv.write_all_actors);
    }

    #[test]
    fn every_option_is_read() {
        let s = NfoSettings::from_values(&values(&[
            ("metadata.nfo-format", json!("emby")),
            ("metadata.nfo-movie-names", json!("both")),
            ("metadata.nfo-set-names", json!("all")),
            ("metadata.nfo-season-names", json!("both")),
            ("metadata.nfo-disc-folder", json!(true)),
            ("metadata.nfo-collection-folder", json!("D:/Collections")),
            ("metadata.nfo-collection-tmdb-id", json!(true)),
            ("metadata.nfo-language", json!("DE")),
            ("metadata.nfo-certification", json!("medium-full")),
            ("metadata.nfo-create-outline", json!(true)),
            ("metadata.nfo-outline-first-sentence", json!(false)),
            ("metadata.nfo-single-studio", json!(true)),
            ("metadata.nfo-date-added", json!(false)),
            ("metadata.nfo-date-added-source", json!("file-created")),
            ("metadata.nfo-artwork-urls", json!(false)),
            ("metadata.nfo-trailer", json!(false)),
            ("metadata.nfo-fileinfo", json!(false)),
            ("metadata.nfo-external-audio", json!(true)),
            ("metadata.nfo-lockdata", json!(true)),
            ("metadata.nfo-all-actors", json!(true)),
            ("metadata.nfo-episodeguide", json!("url")),
            ("metadata.nfo-end-date", json!(false)),
            ("metadata.nfo-clean", json!(true)),
            ("metadata.language", json!("fr")),
        ]));
        assert_eq!(s.movie_connector, MovieConnector::Emby);
        assert_eq!(s.set_connector, MovieSetConnector::Emby);
        assert_eq!(s.tv_connector, TvConnector::Emby);
        assert_eq!(
            s.movie_namings,
            [MovieNfoNaming::VideoBase, MovieNfoNaming::MovieNfo]
        );
        assert_eq!(s.set_namings.len(), 3);
        assert_eq!(s.season_namings.len(), 2);
        assert!(s.nfo_in_disc_folder);
        assert_eq!(s.collection_folder, Some(PathBuf::from("D:/Collections")));
        assert!(s.append_tmdb_id_to_set_folder);
        let m = &s.movie;
        assert_eq!(m.language, "de");
        assert_eq!(m.certification_style, CertificationStyle::MediumFull);
        assert!(m.create_outline && !m.outline_first_sentence && m.single_studio);
        assert!(!m.write_date_added && m.date_added_field == DateAddedField::FileCreationDate);
        assert!(!m.artwork_urls && !m.trailer && !m.fileinfo);
        assert!(m.include_external_audio && m.lockdata && m.clean_nfo);
        assert_eq!(s.tv.common.language, "de");
        assert!(s.tv.common.clean_nfo && s.tv.common.lockdata);
        assert!(s.tv.write_all_actors && !s.tv.write_date_ended);
        assert!(s.tv.write_episodeguide && !s.tv.new_episodeguide_style);
        assert_eq!(s.tv.scraper_language, "fr");
        let off = NfoSettings::from_values(&values(&[("metadata.nfo-episodeguide", json!("off"))]));
        assert!(!off.tv.write_episodeguide);
    }

    #[test]
    fn mediaportal_uses_its_style_and_plex_the_kodi_format() {
        let mp = NfoSettings::from_values(&values(&[
            ("metadata.nfo-format", json!("mediaportal")),
            ("metadata.nfo-mediaportal-style", json!("myvideo")),
        ]));
        assert_eq!(
            mp.movie_connector,
            MovieConnector::MediaPortal(MediaPortalStyle::MyVideo)
        );
        assert_eq!(mp.tv_connector, TvConnector::MediaPortal);
        let plex = NfoSettings::from_values(&values(&[("metadata.nfo-format", json!("plex"))]));
        assert_eq!(plex.movie_connector, MovieConnector::Kodi);
        assert_eq!(plex.tv_connector, TvConnector::Kodi);
    }

    #[test]
    fn the_preferred_rating_source_becomes_the_nfo_default_rating() {
        let s = NfoSettings::from_values(&values(&[(
            "ratings.preferred-source",
            json!("Letterboxd"),
        )]));
        assert_eq!(s.movie.rating_sources[0], "letterboxd");
        assert_eq!(s.tv.common.rating_sources[0], "letterboxd");
        // The rest follows the library's fallback order.
        assert_eq!(s.movie.rating_sources[1], "imdb");
        // Without a choice (or with an unusable one) the writers keep their own order.
        let none = NfoSettings::from_values(&values(&[("ratings.preferred-source", json!(4))]));
        assert_eq!(none, NfoSettings::default());
    }

    #[test]
    fn unknown_values_fall_back_to_the_defaults() {
        let s = NfoSettings::from_values(&values(&[
            ("metadata.nfo-format", json!(7)),
            ("metadata.nfo-movie-names", json!("nonsense")),
            ("metadata.nfo-certification", json!("huge")),
            ("metadata.nfo-date-added-source", json!("never")),
            ("metadata.nfo-trailer", json!("yes")),
            ("metadata.nfo-collection-folder", json!("   ")),
        ]));
        assert_eq!(s, NfoSettings::default());
    }
}

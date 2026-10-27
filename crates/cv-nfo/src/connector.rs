//! The movie NFO formats and the dispatch to their writers.
//!
//! The setting names are the ones the library setting stores: `KODI` (the default), `XBMC`,
//! `EMBY`, `JELLYFIN`, `PLEX`, `MP`, `MP_MP`, `MP_MV` and `DVR_3`. Plex and DVR 3 use the Kodi
//! format.

use crate::kodi_movie::{KodiFlavor, kodi_family_document};
use crate::legacy_movie::{MediaPortalStyle, media_portal_document, xbmc_document};
use crate::movie::{MovieNfo, MovieNfoOptions};
use crate::xml::Document;

/// The format a movie NFO is written in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MovieConnector {
    /// Kodi v17 and later, also used for Plex and DVR 3.
    #[default]
    Kodi,
    /// The older XBMC layout (Kodi before v17).
    Xbmc,
    Emby,
    Jellyfin,
    /// MediaPortal, in one of its three styles.
    MediaPortal(MediaPortalStyle),
}

impl MovieConnector {
    /// Parses the setting name, ignoring case. `PLEX` and `DVR_3` give the Kodi format.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_uppercase().as_str() {
            "KODI" | "PLEX" | "DVR_3" => Self::Kodi,
            "XBMC" => Self::Xbmc,
            "EMBY" => Self::Emby,
            "JELLYFIN" => Self::Jellyfin,
            "MP" => Self::MediaPortal(MediaPortalStyle::Legacy),
            "MP_MP" => Self::MediaPortal(MediaPortalStyle::MovingPictures),
            "MP_MV" => Self::MediaPortal(MediaPortalStyle::MyVideo),
            _ => return None,
        })
    }
}

/// Builds the NFO document of a movie in the given format.
pub fn movie_document_for(
    connector: MovieConnector,
    movie: &MovieNfo,
    options: &MovieNfoOptions,
) -> Document {
    match connector {
        MovieConnector::Kodi => kodi_family_document(KodiFlavor::Kodi, movie, options),
        MovieConnector::Emby => kodi_family_document(KodiFlavor::Emby, movie, options),
        MovieConnector::Jellyfin => kodi_family_document(KodiFlavor::Jellyfin, movie, options),
        MovieConnector::Xbmc => xbmc_document(movie, options),
        MovieConnector::MediaPortal(style) => media_portal_document(style, movie, options),
    }
}

/// Renders the NFO of a movie in the given format: UTF-8 text, two-space indentation, CRLF line
/// ends.
pub fn render_movie_nfo_for(
    connector: MovieConnector,
    movie: &MovieNfo,
    options: &MovieNfoOptions,
) -> String {
    movie_document_for(connector, movie, options).render()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_names_parse() {
        assert_eq!(MovieConnector::parse("kodi"), Some(MovieConnector::Kodi));
        assert_eq!(MovieConnector::parse("PLEX"), Some(MovieConnector::Kodi));
        assert_eq!(MovieConnector::parse("DVR_3"), Some(MovieConnector::Kodi));
        assert_eq!(MovieConnector::parse("XBMC"), Some(MovieConnector::Xbmc));
        assert_eq!(MovieConnector::parse(" Emby "), Some(MovieConnector::Emby));
        assert_eq!(
            MovieConnector::parse("JELLYFIN"),
            Some(MovieConnector::Jellyfin)
        );
        assert_eq!(
            MovieConnector::parse("MP_MV"),
            Some(MovieConnector::MediaPortal(MediaPortalStyle::MyVideo))
        );
        assert_eq!(MovieConnector::parse("nope"), None);
    }
}

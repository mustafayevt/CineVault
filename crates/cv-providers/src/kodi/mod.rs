//! Kodi XML scrapers: an interpreter of the Kodi ScraperXML language and the
//! discovery of installed metadata add-ons.
//!
//! * [`addon`] finds add-ons (`addon.xml` folders) in CineVault's own `kodi_scraper` folder, the
//!   Kodi user-data add-ons folder and the Kodi install's add-ons folder. Add-ons whose scraper
//!   is Python are listed as unsupported with the reason; only XML scrapers can run.
//! * [`ScraperDef`] reads a scraper file, [`KodiSettings`] holds the add-on's settings.
//! * `engine` (private) runs scraper functions: numbered buffers and nested `<RegExp>` rules, see
//!   the module for the rules it implements. Regular expressions are compiled by `pattern`,
//!   which makes PCRE-style expressions work on the Rust engine.
//! * [`KodiProvider`] is a [`crate::MetadataProvider`] for one scraper: it builds the search URL
//!   with `CreateSearchUrl`, fetches pages through the shared [`crate::HttpClient`] (lane =
//!   the add-on ID), reads `GetSearchResults`, `GetDetails`, `GetEpisodeList` and
//!   `GetEpisodeDetails`, and resolves `<chain>` hand-overs. `NfoUrl` is offered as
//!   [`KodiProvider::nfo_url`] and is how an IMDb ID is resolved to a title.
//! * [`KodiCatalogue`] turns a discovery into providers plus the list of add-ons that cannot be
//!   used and why.
//!
//! The ID a Kodi provider stores for a title is a [`Reference`] (the scraper's own ID and page
//! URLs), so a stored title can be fetched again without searching. Scrapers can only reach
//! `http` and `https` addresses.

pub mod addon;
mod engine;
mod pattern;
mod provider;
mod results;
mod scraper;
mod text;
mod xml;

use std::collections::HashMap;
use std::path::PathBuf;

pub use addon::{KodiAddon, ScraperEntry, Support, default_roots, discover};
pub use provider::{KodiConfig, KodiProvider, default_limits};
pub use results::Reference;
pub use scraper::{KodiContent, KodiError, KodiSettings, ScraperDef, ScraperUrl};

use crate::http::HttpClient;

/// A scraper that is listed but cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedScraper {
    pub addon_id: String,
    pub addon_name: String,
    pub content: KodiContent,
    /// Why, in words for the user.
    pub reason: String,
}

/// What a discovery found: the providers to offer and the scrapers that cannot be used.
#[derive(Default)]
pub struct KodiCatalogue {
    pub providers: Vec<KodiProvider>,
    pub unsupported: Vec<UnsupportedScraper>,
}

impl KodiCatalogue {
    /// Discovers add-ons under `roots` and builds a provider for each XML scraper.
    /// `settings` holds the user's setting values per add-on ID.
    pub fn load(
        http: &HttpClient,
        roots: &[PathBuf],
        settings: &HashMap<String, KodiSettings>,
    ) -> Self {
        Self::from_addons(http, &discover(roots), settings)
    }

    /// Builds the catalogue for add-ons already discovered.
    pub fn from_addons(
        http: &HttpClient,
        addons: &[KodiAddon],
        settings: &HashMap<String, KodiSettings>,
    ) -> Self {
        let mut catalogue = Self::default();
        let no_settings = KodiSettings::new();
        for addon in addons {
            let runnable: Vec<&ScraperEntry> = addon.supported().collect();
            let kinds = |content| runnable.iter().filter(|e| e.content == content).count();
            let both = kinds(KodiContent::Movies) > 0 && kinds(KodiContent::TvShows) > 0;
            for entry in &addon.scrapers {
                let unsupported = |reason: String| UnsupportedScraper {
                    addon_id: addon.id.clone(),
                    addon_name: addon.name.clone(),
                    content: entry.content,
                    reason,
                };
                if let Support::Unsupported(reason) = &entry.support {
                    catalogue.unsupported.push(unsupported(reason.clone()));
                    continue;
                }
                let key = match (both, entry.content) {
                    (false, _) => addon.id.clone(),
                    (true, KodiContent::Movies) => format!("{}.movies", addon.id),
                    (true, KodiContent::TvShows) => format!("{}.tvshows", addon.id),
                };
                let overrides = settings.get(&addon.id).unwrap_or(&no_settings);
                match KodiProvider::from_addon(http.clone(), addon, entry, &key, overrides) {
                    Ok(provider) => catalogue.providers.push(provider),
                    Err(error) => catalogue.unsupported.push(unsupported(error.to_string())),
                }
            }
        }
        catalogue
    }
}

//! The credit each provider's terms ask an app to show.
//!
//! The About screen lists these. The wording is the provider's own where the provider prescribes
//! one (TMDB), and a plain credit otherwise.

use crate::catalogue::key;

/// One provider's credit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attribution {
    /// Provider key, for example `tmdb`. Matches the credential schema key where there is one.
    pub provider: &'static str,
    /// Name as the provider writes it.
    pub name: &'static str,
    /// The notice to show.
    pub notice: &'static str,
    /// Where the credit links to.
    pub url: &'static str,
    /// The provider's terms require the notice wherever its data is shown. The About screen lists
    /// these whether or not a key is set.
    pub required: bool,
}

const fn credit(
    provider: &'static str,
    name: &'static str,
    notice: &'static str,
    url: &'static str,
    required: bool,
) -> Attribution {
    Attribution {
        provider,
        name,
        notice,
        url,
        required,
    }
}

/// Every provider the app can use, in the order About lists them.
pub const ATTRIBUTIONS: &[Attribution] = &[
    credit(
        key::TMDB,
        "TMDB",
        "This product uses the TMDB API but is not endorsed or certified by TMDB.",
        "https://www.themoviedb.org",
        true,
    ),
    credit(
        key::TVDB,
        "TheTVDB",
        "Metadata provided by TheTVDB. Please consider adding missing information or subscribing.",
        "https://thetvdb.com",
        true,
    ),
    credit(
        key::FANART,
        "Fanart.tv",
        "Artwork provided by Fanart.tv.",
        "https://fanart.tv",
        true,
    ),
    credit(
        "imdb",
        "IMDb",
        "Information courtesy of IMDb. Used for personal, non-commercial purposes.",
        "https://www.imdb.com",
        true,
    ),
    credit(
        "tvmaze",
        "TVmaze",
        "TV data from TVmaze.com, licensed under CC BY-SA 4.0.",
        "https://www.tvmaze.com",
        true,
    ),
    credit(
        key::OMDB,
        "OMDb",
        "Ratings and details from the OMDb API.",
        "https://www.omdbapi.com",
        false,
    ),
    credit(
        key::TRAKT,
        "Trakt",
        "Ratings and details from Trakt.",
        "https://trakt.tv",
        false,
    ),
    credit(
        key::MDBLIST,
        "MDBList",
        "Aggregated ratings from MDBList.",
        "https://mdblist.com",
        false,
    ),
    credit(
        key::MOVIEMETER,
        "MovieMeter",
        "Dutch titles and ratings from MovieMeter.",
        "https://www.moviemeter.nl",
        false,
    ),
    credit(
        key::ANIDB,
        "AniDB",
        "Anime data provided by AniDB.",
        "https://anidb.net",
        true,
    ),
    credit(
        key::THESPORTSDB,
        "TheSportsDB",
        "Sports data and artwork provided by TheSportsDB.",
        "https://www.thesportsdb.com",
        true,
    ),
    credit(
        "ofdb",
        "OFDb",
        "Film data from OFDb, the Online-Filmdatenbank.",
        "https://www.ofdb.de",
        false,
    ),
    credit(
        "fernsehserien",
        "fernsehserien.de",
        "TV data from fernsehserien.de.",
        "https://www.fernsehserien.de",
        false,
    ),
    credit(
        key::MPDB,
        "MPDb.tv",
        "Metadata and artwork from MPDb.tv.",
        "https://mpdb.tv",
        false,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::CREDENTIAL_SCHEMAS;
    use std::collections::HashSet;

    #[test]
    fn the_providers_the_spec_names_are_credited_and_required() {
        for name in ["TMDB", "TheTVDB", "Fanart.tv"] {
            let credit = ATTRIBUTIONS
                .iter()
                .find(|a| a.name == name)
                .unwrap_or_else(|| panic!("{name} is credited"));
            assert!(credit.required, "{name}");
        }
        let tmdb = ATTRIBUTIONS.iter().find(|a| a.provider == "tmdb").unwrap();
        assert_eq!(
            tmdb.notice,
            "This product uses the TMDB API but is not endorsed or certified by TMDB."
        );
    }

    #[test]
    fn every_provider_with_credentials_has_a_credit() {
        for schema in CREDENTIAL_SCHEMAS {
            let credit = ATTRIBUTIONS
                .iter()
                .find(|a| a.provider == schema.provider)
                .unwrap_or_else(|| panic!("{} has no credit", schema.provider));
            assert_eq!(credit.name, schema.display_name);
        }
    }

    #[test]
    fn credits_are_unique_and_link_over_https() {
        let mut seen = HashSet::new();
        for credit in ATTRIBUTIONS {
            assert!(
                seen.insert(credit.provider),
                "duplicate {}",
                credit.provider
            );
            assert!(credit.url.starts_with("https://"));
            assert!(!credit.notice.is_empty());
        }
    }
}

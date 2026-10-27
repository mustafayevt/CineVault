//! Open-ended values: a built-in set of known values plus free text.
//!
//! Unknown input is kept verbatim as `Other(text)` and never discarded. Known values serialise as
//! their stable `UPPER_SNAKE` key; `Other` serialises as the free text itself, so a value always
//! reads back as it was written.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Maps free text to the key form used for known values: `"Science Fiction"` and
/// `"science-fiction"` both become `SCIENCE_FICTION`.
fn canonical_key(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_sep = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if pending_sep && !out.is_empty() {
                out.push('_');
            }
            pending_sep = false;
            out.extend(ch.to_uppercase());
        } else {
            pending_sep = true;
        }
    }
    out
}

macro_rules! open_enum {
    (
        $(#[$meta:meta])*
        $name:ident, blank = $blank:expr, { $($var:ident => $key:literal),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $($var,)+
            /// A value outside the built-in set, kept as the user's text.
            Other(String),
        }

        impl $name {
            /// Stable keys of the built-in values, in declaration order.
            pub const KNOWN_KEYS: &'static [&'static str] = &[$($key),+];

            /// All built-in values, in declaration order.
            pub fn known() -> Vec<Self> {
                vec![$(Self::$var),+]
            }

            /// Parses free text. Known values (matched ignoring case, spaces, `-` and `_`)
            /// map to their variant; anything else becomes `Other` with the trimmed text.
            /// Blank input maps to the type's blank value, if it has one.
            pub fn parse(text: &str) -> Option<Self> {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return $blank;
                }
                Some(match canonical_key(trimmed).as_str() {
                    $($key => Self::$var,)+
                    _ => Self::Other(trimmed.to_owned()),
                })
            }

            /// The stable key for known values, or the free text for `Other`.
            pub fn name(&self) -> &str {
                match self {
                    $(Self::$var => $key,)+
                    Self::Other(text) => text,
                }
            }

            /// True for built-in values.
            pub fn is_known(&self) -> bool {
                !matches!(self, Self::Other(_))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.name())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.name())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = String::deserialize(d)?;
                Self::parse(&text).ok_or_else(|| {
                    serde::de::Error::custom(concat!("blank ", stringify!($name), " value"))
                })
            }
        }
    };
}

open_enum! {
    /// A genre. Unknown genres are kept as free text.
    Genre, blank = None, {
        Action => "ACTION",
        Adventure => "ADVENTURE",
        Animation => "ANIMATION",
        Anime => "ANIME",
        Animal => "ANIMAL",
        Biography => "BIOGRAPHY",
        Comedy => "COMEDY",
        Crime => "CRIME",
        Disaster => "DISASTER",
        Documentary => "DOCUMENTARY",
        Drama => "DRAMA",
        Eastern => "EASTERN",
        Erotic => "EROTIC",
        Family => "FAMILY",
        FanFilm => "FAN_FILM",
        Fantasy => "FANTASY",
        FilmNoir => "FILM_NOIR",
        Foreign => "FOREIGN",
        GameShow => "GAME_SHOW",
        History => "HISTORY",
        Holiday => "HOLIDAY",
        Horror => "HORROR",
        Indie => "INDIE",
        MiniSeries => "MINI_SERIES",
        Music => "MUSIC",
        Musical => "MUSICAL",
        Mystery => "MYSTERY",
        NeoNoir => "NEO_NOIR",
        News => "NEWS",
        RealityTv => "REALITY_TV",
        RoadMovie => "ROAD_MOVIE",
        Romance => "ROMANCE",
        ScienceFiction => "SCIENCE_FICTION",
        Series => "SERIES",
        Short => "SHORT",
        SilentMovie => "SILENT_MOVIE",
        Soap => "SOAP",
        Sport => "SPORT",
        SportingEvent => "SPORTING_EVENT",
        SportsFilm => "SPORTS_FILM",
        Suspense => "SUSPENSE",
        TalkShow => "TALK_SHOW",
        TvMovie => "TV_MOVIE",
        Thriller => "THRILLER",
        War => "WAR",
        Western => "WESTERN",
    }
}

open_enum! {
    /// Where a video came from. Blank input means [`MediaSource::Unknown`].
    MediaSource, blank = Some(MediaSource::Unknown), {
        UhdBluray => "UHD_BLURAY",
        Bluray => "BLURAY",
        Dvd => "DVD",
        HdDvd => "HDDVD",
        Tv => "TV",
        Vhs => "VHS",
        Laserdisc => "LASERDISC",
        DVhs => "D_VHS",
        HdRip => "HDRIP",
        Cam => "CAM",
        Ts => "TS",
        Tc => "TC",
        DvdScr => "DVDSCR",
        R5 => "R5",
        WebRip => "WEBRIP",
        WebDl => "WEB_DL",
        Stream => "STREAM",
        Unknown => "UNKNOWN",
    }
}

open_enum! {
    /// A movie or episode edition. Blank input means [`Edition::None`].
    Edition, blank = Some(Edition::None), {
        None => "NONE",
        DirectorsCut => "DIRECTORS_CUT",
        ExtendedEdition => "EXTENDED_EDITION",
        TheatricalEdition => "THEATRICAL_EDITION",
        Unrated => "UNRATED",
        Uncut => "UNCUT",
        Imax => "IMAX",
        Remastered => "REMASTERED",
        CollectorsEdition => "COLLECTORS_EDITION",
        UltimateEdition => "ULTIMATE_EDITION",
        FinalCut => "FINAL_CUT",
        SpecialEdition => "SPECIAL_EDITION",
        CriterionCollection => "CRITERION_COLLECTION",
        OpenMatte => "OPEN_MATTE",
    }
}

#[allow(clippy::derivable_impls)] // the enum is macro-generated, so it cannot carry `#[default]`
impl Default for MediaSource {
    fn default() -> Self {
        Self::Unknown
    }
}

#[allow(clippy::derivable_impls)]
impl Default for Edition {
    fn default() -> Self {
        Self::None
    }
}

/// Edition names in file and folder names: the edition key and the lower-case words of one
/// spelling (apostrophes removed). The scanner matches them word by word (the longest spelling
/// wins); the library filter "Edition only in the file name" uses the same list.
pub const EDITION_SPELLINGS: &[(&str, &str)] = &[
    ("DIRECTORS_CUT", "directors cut"),
    ("DIRECTORS_CUT", "director cut"),
    ("DIRECTORS_CUT", "directors edition"),
    ("DIRECTORS_CUT", "directors version"),
    ("EXTENDED_EDITION", "extended edition"),
    ("EXTENDED_EDITION", "extended cut"),
    ("EXTENDED_EDITION", "extended version"),
    ("EXTENDED_EDITION", "extended"),
    ("THEATRICAL_EDITION", "theatrical edition"),
    ("THEATRICAL_EDITION", "theatrical cut"),
    ("THEATRICAL_EDITION", "theatrical version"),
    ("THEATRICAL_EDITION", "theatrical"),
    ("UNRATED", "unrated edition"),
    ("UNRATED", "unrated cut"),
    ("UNRATED", "unrated"),
    ("UNCUT", "uncut"),
    ("IMAX", "imax edition"),
    ("IMAX", "imax"),
    ("REMASTERED", "remastered"),
    ("COLLECTORS_EDITION", "collectors edition"),
    ("COLLECTORS_EDITION", "collectors cut"),
    ("ULTIMATE_EDITION", "ultimate edition"),
    ("ULTIMATE_EDITION", "ultimate cut"),
    ("FINAL_CUT", "final cut"),
    ("SPECIAL_EDITION", "special edition"),
    ("CRITERION_COLLECTION", "criterion collection"),
    ("OPEN_MATTE", "open matte"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T>(value: &T) -> T
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let json = serde_json::to_string(value).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn every_known_genre_round_trips() {
        for g in Genre::known() {
            assert!(g.is_known());
            assert_eq!(round_trip(&g), g);
            assert_eq!(Genre::parse(g.name()), Some(g));
        }
        assert_eq!(Genre::known().len(), Genre::KNOWN_KEYS.len());
    }

    #[test]
    fn every_known_source_and_edition_round_trips() {
        for s in MediaSource::known() {
            assert_eq!(round_trip(&s), s);
        }
        for e in Edition::known() {
            assert_eq!(round_trip(&e), e);
        }
    }

    #[test]
    fn custom_genre_is_kept_as_free_text() {
        let g = Genre::parse("  Cosmic Horror ").unwrap();
        assert_eq!(g, Genre::Other("Cosmic Horror".into()));
        assert!(!g.is_known());
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(json, "\"Cosmic Horror\"");
        assert_eq!(round_trip(&g), g);
    }

    #[test]
    fn custom_edition_from_folder_tag_is_preserved() {
        let e = Edition::parse("Ultimate Archive").unwrap();
        assert_eq!(e, Edition::Other("Ultimate Archive".into()));
        assert_eq!(e.name(), "Ultimate Archive");
        assert_eq!(round_trip(&e), e);
    }

    #[test]
    fn custom_source_is_preserved() {
        let s = MediaSource::parse("Super 8 Scan").unwrap();
        assert_eq!(s, MediaSource::Other("Super 8 Scan".into()));
        assert_eq!(round_trip(&s), s);
    }

    #[test]
    fn known_values_match_loosely() {
        assert_eq!(Genre::parse("science fiction"), Some(Genre::ScienceFiction));
        assert_eq!(Genre::parse("Sci-Fi"), Some(Genre::Other("Sci-Fi".into())));
        assert_eq!(Edition::parse("directors-cut"), Some(Edition::DirectorsCut));
        assert_eq!(MediaSource::parse("web dl"), Some(MediaSource::WebDl));
        assert_eq!(
            MediaSource::parse("uhd_bluray"),
            Some(MediaSource::UhdBluray)
        );
    }

    #[test]
    fn blank_values() {
        assert_eq!(Genre::parse("   "), None);
        assert!(serde_json::from_str::<Genre>("\"\"").is_err());
        assert_eq!(
            serde_json::from_str::<MediaSource>("\"\"").unwrap(),
            MediaSource::Unknown
        );
        assert_eq!(
            serde_json::from_str::<Edition>("\"\"").unwrap(),
            Edition::None
        );
        assert_eq!(MediaSource::default(), MediaSource::Unknown);
        assert_eq!(Edition::default(), Edition::None);
    }

    #[test]
    fn unicode_free_text_survives() {
        let g = Genre::parse("Научная фантастика").unwrap();
        assert_eq!(round_trip(&g), g);
    }
}

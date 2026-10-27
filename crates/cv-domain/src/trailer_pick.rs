//! Choosing the best trailer: quality aliases, the preferred (NFO) trailer and the download
//! candidates.
//!
//! These are pure functions over [`Trailer`] lists, so the rules are table-tested without any
//! provider or disk.
//!
//! **Quality.** The setting is one of SD, HD 720, HD 1080 and HD 2160. A trailer's quality label
//! passes when it is one of the setting's aliases ([`TrailerQuality::accepts`]). YouTube trailers
//! always pass, because YouTube offers every quality ([`TrailerPreferences::accepts_quality`]).
//!
//! **The preferred trailer** ([`preferred_index`], [`mark_preferred`]) is the first trailer that
//!
//! 1. matches the preferred language and has an acceptable quality; otherwise
//! 2. has an acceptable quality; otherwise
//! 3. is a web trailer.
//!
//! **Download candidates** ([`download_candidates`]) never include local files. They are the
//! first non-empty group of: language and quality; trailers with no language and quality; quality
//! alone; the single trailer, when there is exactly one.

use serde::{Deserialize, Serialize};

use crate::Trailer;

/// The quality setting for trailers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrailerQuality {
    /// 480p and below.
    #[serde(rename = "sd")]
    Sd,
    /// 720p.
    #[default]
    #[serde(rename = "hd_720")]
    Hd720,
    /// 1080p.
    #[serde(rename = "hd_1080")]
    Hd1080,
    /// 2160p.
    #[serde(rename = "hd_2160")]
    Hd2160,
}

impl TrailerQuality {
    /// Every setting, lowest first.
    pub const ALL: [TrailerQuality; 4] = [Self::Sd, Self::Hd720, Self::Hd1080, Self::Hd2160];

    /// The stored name of the setting.
    pub fn key(self) -> &'static str {
        match self {
            Self::Sd => "sd",
            Self::Hd720 => "hd_720",
            Self::Hd1080 => "hd_1080",
            Self::Hd2160 => "hd_2160",
        }
    }

    /// The setting stored under `key`.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|q| q.key() == key.trim())
    }

    /// The quality labels the setting accepts, as the table in the spec lists them. SD accepts
    /// any height up to 480 as well, so `540p` is not an alias but `360p` is.
    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Sd => &["sd", "480p", "360p", "240p", "144p"],
            Self::Hd720 => &["hd", "720p", "720"],
            Self::Hd1080 => &["hd", "1080p", "1080"],
            Self::Hd2160 => &["hd", "2160p", "2160"],
        }
    }

    /// Whether a trailer labelled `label` (`1080p`, `HD`, `480`) has an acceptable quality.
    /// Matching ignores case and surrounding blanks. A blank label never matches.
    pub fn accepts(self, label: &str) -> bool {
        let label = label.trim().to_ascii_lowercase();
        if label.is_empty() {
            return false;
        }
        if self.aliases().contains(&label.as_str()) {
            return true;
        }
        match self {
            Self::Sd => height_of(&label).is_some_and(|h| (1..=480).contains(&h)),
            _ => false,
        }
    }
}

/// The height in a label such as `720p` or `480`.
fn height_of(label: &str) -> Option<u32> {
    label.strip_suffix('p').unwrap_or(label).parse().ok()
}

/// What the user prefers: a trailer language and a quality.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TrailerPreferences {
    /// Preferred language as a two-letter (`de`), three-letter (`deu`) or regional (`de-DE`)
    /// code. Empty means no language preference.
    pub language: String,
    pub quality: TrailerQuality,
}

impl TrailerPreferences {
    /// Whether the trailer's quality passes: always for YouTube, otherwise by alias.
    pub fn accepts_quality(&self, trailer: &Trailer) -> bool {
        trailer.is_youtube() || self.quality.accepts(&trailer.quality)
    }

    /// Whether a language preference is set.
    pub fn has_language(&self) -> bool {
        !language_code(&self.language).is_empty()
    }

    /// Whether the trailer is in the preferred language. Only the language counts, not the
    /// region, and two- and three-letter codes are the same language. False when no language
    /// is preferred or the trailer has none.
    pub fn matches_language(&self, trailer: &Trailer) -> bool {
        let wanted = language_code(&self.language);
        !wanted.is_empty() && wanted == language_code(&trailer.language)
    }
}

/// The two-letter code of a language tag (`pt-BR`, `por`, `PT` all give `pt`). Codes the table
/// does not know are returned lower-cased as they are.
pub fn language_code(tag: &str) -> String {
    let tag = tag.trim().to_ascii_lowercase();
    let primary = tag.split(['-', '_']).next().unwrap_or("");
    if primary.len() == 3
        && let Some((two, _)) = LANGUAGES.iter().find(|(_, three)| three.contains(&primary))
    {
        return (*two).to_owned();
    }
    primary.to_owned()
}

/// Whether two language tags name the same language: `en` and `eng`, `fr` and `fre-CA`. False
/// when either is empty.
pub fn same_language(a: &str, b: &str) -> bool {
    let (a, b) = (language_code(a), language_code(b));
    !a.is_empty() && a == b
}

/// `(two-letter code, its three-letter codes)`, bibliographic variants included.
const LANGUAGES: &[(&str, &[&str])] = &[
    ("af", &["afr"]),
    ("ar", &["ara"]),
    ("az", &["aze"]),
    ("be", &["bel"]),
    ("bg", &["bul"]),
    ("bn", &["ben"]),
    ("bo", &["bod", "tib"]),
    ("bs", &["bos"]),
    ("ca", &["cat"]),
    ("cs", &["ces", "cze"]),
    ("cy", &["cym", "wel"]),
    ("da", &["dan"]),
    ("de", &["deu", "ger"]),
    ("el", &["ell", "gre"]),
    ("en", &["eng"]),
    ("eo", &["epo"]),
    ("es", &["spa"]),
    ("et", &["est"]),
    ("eu", &["eus", "baq"]),
    ("fa", &["fas", "per"]),
    ("fi", &["fin"]),
    ("fr", &["fra", "fre"]),
    ("ga", &["gle"]),
    ("gl", &["glg"]),
    ("gu", &["guj"]),
    ("he", &["heb"]),
    ("hi", &["hin"]),
    ("hr", &["hrv"]),
    ("hu", &["hun"]),
    ("hy", &["hye", "arm"]),
    ("id", &["ind"]),
    ("is", &["isl", "ice"]),
    ("it", &["ita"]),
    ("ja", &["jpn"]),
    ("ka", &["kat", "geo"]),
    ("kk", &["kaz"]),
    ("km", &["khm"]),
    ("kn", &["kan"]),
    ("ko", &["kor"]),
    ("ku", &["kur"]),
    ("la", &["lat"]),
    ("lo", &["lao"]),
    ("lt", &["lit"]),
    ("lv", &["lav"]),
    ("mk", &["mkd", "mac"]),
    ("ml", &["mal"]),
    ("mn", &["mon"]),
    ("mr", &["mar"]),
    ("ms", &["msa", "may"]),
    ("mt", &["mlt"]),
    ("my", &["mya", "bur"]),
    ("nb", &["nob"]),
    ("ne", &["nep"]),
    ("nl", &["nld", "dut"]),
    ("nn", &["nno"]),
    ("no", &["nor"]),
    ("pa", &["pan"]),
    ("pl", &["pol"]),
    ("pt", &["por"]),
    ("ro", &["ron", "rum"]),
    ("ru", &["rus"]),
    ("si", &["sin"]),
    ("sk", &["slk", "slo"]),
    ("sl", &["slv"]),
    ("sq", &["sqi", "alb"]),
    ("sr", &["srp"]),
    ("sv", &["swe"]),
    ("sw", &["swa"]),
    ("ta", &["tam"]),
    ("te", &["tel"]),
    ("th", &["tha"]),
    ("tl", &["fil", "tgl"]),
    ("tr", &["tur"]),
    ("uk", &["ukr"]),
    ("ur", &["urd"]),
    ("uz", &["uzb"]),
    ("vi", &["vie"]),
    ("yi", &["yid"]),
    ("zh", &["zho", "chi"]),
    ("zu", &["zul"]),
];

/// The position of the preferred trailer, or `None` when `trailers` has no web trailer and
/// nothing else passes.
pub fn preferred_index(trailers: &[Trailer], prefs: &TrailerPreferences) -> Option<usize> {
    let passes_language = |t: &Trailer| prefs.matches_language(t) && prefs.accepts_quality(t);
    let first = |test: &dyn Fn(&Trailer) -> bool| trailers.iter().position(test);
    if prefs.has_language()
        && let Some(index) = first(&passes_language)
    {
        return Some(index);
    }
    first(&|t| prefs.accepts_quality(t)).or_else(|| first(&|t| t.is_web()))
}

/// Marks the preferred trailer as the one written to the NFO and moves it to the front. Every
/// other trailer is unmarked, so exactly one trailer is marked afterwards (none for an empty
/// list or when nothing qualifies). The rest keep their order. Returns whether a trailer was
/// marked.
pub fn mark_preferred(trailers: &mut [Trailer], prefs: &TrailerPreferences) -> bool {
    let chosen = preferred_index(trailers, prefs);
    for trailer in trailers.iter_mut() {
        trailer.in_nfo = false;
    }
    let Some(index) = chosen else {
        return false;
    };
    trailers[index].in_nfo = true;
    trailers[..=index].rotate_right(1);
    true
}

/// The trailers to try downloading, in order. Local files are never candidates. The result is
/// the first non-empty group of: preferred language and quality; no language and quality;
/// quality alone; the only trailer. When the user has no language preference, the first group
/// is every trailer of acceptable quality.
pub fn download_candidates<'a>(
    trailers: &'a [Trailer],
    prefs: &TrailerPreferences,
) -> Vec<&'a Trailer> {
    let remote: Vec<&Trailer> = trailers
        .iter()
        .filter(|t| !t.is_local() && !t.url.trim().is_empty())
        .collect();
    let group = |test: &dyn Fn(&Trailer) -> bool| -> Vec<&'a Trailer> {
        remote.iter().copied().filter(|t| test(t)).collect()
    };
    let language_and_quality = group(&|t| {
        (!prefs.has_language() || prefs.matches_language(t)) && prefs.accepts_quality(t)
    });
    if !language_and_quality.is_empty() {
        return language_and_quality;
    }
    let no_language = group(&|t| language_code(&t.language).is_empty() && prefs.accepts_quality(t));
    if !no_language.is_empty() {
        return no_language;
    }
    let quality = group(&|t| prefs.accepts_quality(t));
    if !quality.is_empty() {
        return quality;
    }
    if remote.len() == 1 {
        return remote;
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const YOUTUBE: &str = "https://www.youtube.com/watch?v=";

    fn trailer(url: &str, quality: &str, language: &str) -> Trailer {
        Trailer {
            id: url.to_owned(),
            url: url.to_owned(),
            quality: quality.to_owned(),
            language: language.to_owned(),
            ..Trailer::default()
        }
    }

    fn web(name: &str, quality: &str, language: &str) -> Trailer {
        trailer(
            &format!("https://cdn.example.test/{name}.mp4"),
            quality,
            language,
        )
    }

    fn youtube(id: &str, quality: &str, language: &str) -> Trailer {
        trailer(&format!("{YOUTUBE}{id}"), quality, language)
    }

    fn prefs(language: &str, quality: TrailerQuality) -> TrailerPreferences {
        TrailerPreferences {
            language: language.to_owned(),
            quality,
        }
    }

    fn ids(list: &[&Trailer]) -> Vec<String> {
        list.iter().map(|t| t.id.clone()).collect()
    }

    #[test]
    fn quality_aliases() {
        use TrailerQuality::{Hd720, Hd1080, Hd2160, Sd};
        // (setting, label, accepted)
        let table = [
            (Sd, "SD", true),
            (Sd, "sd", true),
            (Sd, "480p", true),
            (Sd, "360p", true),
            (Sd, "240p", true),
            (Sd, "144p", true),
            (Sd, "480", true),
            (Sd, " 480P ", true),
            (Sd, "HD", false),
            (Sd, "540p", false),
            (Sd, "720p", false),
            (Sd, "0p", false),
            (Sd, "", false),
            (Hd720, "HD", true),
            (Hd720, "hd", true),
            (Hd720, "720p", true),
            (Hd720, "720", true),
            (Hd720, "720P", true),
            (Hd720, "1080p", false),
            (Hd720, "480p", false),
            (Hd720, "SD", false),
            (Hd720, "", false),
            (Hd1080, "HD", true),
            (Hd1080, "1080p", true),
            (Hd1080, "1080", true),
            (Hd1080, "720p", false),
            (Hd1080, "2160p", false),
            (Hd2160, "HD", true),
            (Hd2160, "2160p", true),
            (Hd2160, "2160", true),
            (Hd2160, "1080p", false),
            (Hd2160, "4k", false),
        ];
        for (setting, label, accepted) in table {
            assert_eq!(setting.accepts(label), accepted, "{setting:?} {label:?}");
        }
    }

    #[test]
    fn quality_settings_round_trip_through_their_keys() {
        for quality in TrailerQuality::ALL {
            assert_eq!(TrailerQuality::from_key(quality.key()), Some(quality));
            let json = serde_json::to_string(&quality).unwrap();
            assert_eq!(json, format!("\"{}\"", quality.key()));
            assert_eq!(
                serde_json::from_str::<TrailerQuality>(&json).unwrap(),
                quality
            );
        }
        assert_eq!(TrailerQuality::from_key("other"), None);
        assert_eq!(TrailerQuality::default(), TrailerQuality::Hd720);
    }

    #[test]
    fn language_codes() {
        let table = [
            ("de", "de"),
            ("DE", "de"),
            ("de-DE", "de"),
            ("de_AT", "de"),
            ("deu", "de"),
            ("ger", "de"),
            ("pt-BR", "pt"),
            ("por", "pt"),
            ("eng-US", "en"),
            ("  fr ", "fr"),
            ("xx", "xx"),
            ("", ""),
        ];
        for (tag, code) in table {
            assert_eq!(language_code(tag), code, "{tag:?}");
        }
    }

    #[test]
    fn language_matching_ignores_region_and_code_length() {
        let german = youtube("g", "1080p", "de-DE");
        for wanted in ["de", "deu", "ger", "de-AT", "DE"] {
            assert!(
                prefs(wanted, TrailerQuality::Hd720).matches_language(&german),
                "{wanted}"
            );
        }
        assert!(!prefs("en", TrailerQuality::Hd720).matches_language(&german));
        assert!(!prefs("", TrailerQuality::Hd720).matches_language(&german));
        assert!(!prefs("de", TrailerQuality::Hd720).matches_language(&youtube("n", "1080p", "")));
    }

    #[test]
    fn youtube_always_has_acceptable_quality() {
        let p = prefs("", TrailerQuality::Hd2160);
        assert!(p.accepts_quality(&youtube("a", "", "")));
        assert!(p.accepts_quality(&youtube("a", "480p", "")));
        assert!(!p.accepts_quality(&web("a", "480p", "")));
        assert!(p.accepts_quality(&web("a", "2160p", "")));
        assert!(!p.accepts_quality(&web("a", "", "")));
    }

    /// One row of the preferred-trailer table: the trailers, the preferences and the id of the
    /// trailer that must win (`None` when no trailer qualifies).
    struct Case {
        name: &'static str,
        trailers: Vec<Trailer>,
        prefs: TrailerPreferences,
        winner: Option<&'static str>,
    }

    fn named(id: &str, mut t: Trailer) -> Trailer {
        t.id = id.to_owned();
        t
    }

    #[test]
    fn preferred_trailer_table() {
        use TrailerQuality::{Hd720, Hd1080};
        let cases = vec![
            Case {
                name: "language beats an earlier trailer in another language",
                trailers: vec![
                    named("en", web("en", "1080p", "en-US")),
                    named("de", web("de", "1080p", "de-DE")),
                ],
                prefs: prefs("de", Hd1080),
                winner: Some("de"),
            },
            Case {
                name: "german 1080p wins over english 1080p (spec scenario)",
                trailers: vec![
                    named("en", youtube("en", "1080p", "en-US")),
                    named("de", youtube("de", "1080p", "de-DE")),
                ],
                prefs: prefs("deu", Hd1080),
                winner: Some("de"),
            },
            Case {
                name: "a three-letter preference matches a regional tag",
                trailers: vec![named("de", web("de", "720p", "de-DE"))],
                prefs: prefs("ger", Hd720),
                winner: Some("de"),
            },
            Case {
                name: "right language but wrong quality falls back to quality alone",
                trailers: vec![
                    named("de", web("de", "480p", "de-DE")),
                    named("en", web("en", "1080p", "en-US")),
                ],
                prefs: prefs("de", Hd1080),
                winner: Some("en"),
            },
            Case {
                name: "youtube passes the quality test in the right language",
                trailers: vec![
                    named("web", web("web", "1080p", "de-DE")),
                    named("yt", youtube("yt", "", "de-DE")),
                ],
                prefs: prefs("de", Hd1080),
                winner: Some("web"),
            },
            Case {
                name: "youtube with no quality label beats a poor web trailer",
                trailers: vec![
                    named("web", web("web", "480p", "de-DE")),
                    named("yt", youtube("yt", "", "de-DE")),
                ],
                prefs: prefs("de", Hd1080),
                winner: Some("yt"),
            },
            Case {
                name: "no language preference takes the first acceptable quality",
                trailers: vec![
                    named("low", web("low", "480p", "en-US")),
                    named("hd", web("hd", "720p", "fr-FR")),
                    named("hd2", web("hd2", "720p", "de-DE")),
                ],
                prefs: prefs("", Hd720),
                winner: Some("hd"),
            },
            Case {
                name: "no acceptable quality falls back to the first web trailer",
                trailers: vec![
                    named("local", trailer("file:///C:/m/heat-trailer.mkv", "", "")),
                    named("first", web("first", "480p", "en-US")),
                    named("second", web("second", "360p", "en-US")),
                ],
                prefs: prefs("de", Hd1080),
                winner: Some("first"),
            },
            Case {
                name: "unlabelled web trailer is still a web trailer",
                trailers: vec![named(
                    "imdb",
                    trailer("https://www.imdb.com/video/vi1/", "", ""),
                )],
                prefs: prefs("", Hd1080),
                winner: Some("imdb"),
            },
            Case {
                name: "only a local file leaves nothing to choose",
                trailers: vec![named("local", trailer("file:///x.mkv", "", ""))],
                prefs: prefs("", Hd1080),
                winner: None,
            },
            Case {
                name: "the alias HD counts for every HD setting",
                trailers: vec![
                    named("sd", web("sd", "SD", "en")),
                    named("hd", web("hd", "HD", "en")),
                ],
                prefs: prefs("en", Hd1080),
                winner: Some("hd"),
            },
            Case {
                name: "no trailers",
                trailers: vec![],
                prefs: prefs("en", Hd720),
                winner: None,
            },
        ];
        for case in cases {
            let index = preferred_index(&case.trailers, &case.prefs);
            let winner = index.map(|i| case.trailers[i].id.as_str());
            assert_eq!(winner, case.winner, "{}", case.name);

            let mut marked = case.trailers.clone();
            let did_mark = mark_preferred(&mut marked, &case.prefs);
            assert_eq!(did_mark, case.winner.is_some(), "{}", case.name);
            assert_eq!(marked.len(), case.trailers.len(), "{}", case.name);
            assert_eq!(
                marked.iter().filter(|t| t.in_nfo).count(),
                usize::from(case.winner.is_some()),
                "{}",
                case.name
            );
            if let Some(id) = case.winner {
                assert_eq!(marked[0].id, id, "{}: moved first", case.name);
                assert!(marked[0].in_nfo, "{}", case.name);
            }
        }
    }

    #[test]
    fn marking_moves_the_winner_first_and_keeps_the_order_of_the_rest() {
        let mut list = vec![
            named("a", web("a", "480p", "en")),
            named("b", web("b", "480p", "en")),
            named("c", web("c", "1080p", "en")),
            named("d", web("d", "1080p", "en")),
        ];
        assert!(mark_preferred(
            &mut list,
            &prefs("en", TrailerQuality::Hd1080)
        ));
        let order: Vec<&str> = list.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(order, ["c", "a", "b", "d"]);
        assert!(list[0].in_nfo);
    }

    #[test]
    fn marking_again_replaces_an_earlier_mark() {
        let mut list = vec![
            named("a", web("a", "720p", "en")),
            named("b", web("b", "1080p", "en")),
        ];
        list[0].in_nfo = true;
        assert!(mark_preferred(
            &mut list,
            &prefs("en", TrailerQuality::Hd1080)
        ));
        assert_eq!(list[0].id, "b");
        assert!(list[0].in_nfo);
        assert!(!list[1].in_nfo);

        let mut local = vec![named("l", trailer("file:///x.mkv", "", ""))];
        local[0].in_nfo = true;
        assert!(!mark_preferred(
            &mut local,
            &prefs("", TrailerQuality::Hd720)
        ));
        assert!(
            !local[0].in_nfo,
            "nothing qualifies, so nothing stays marked"
        );
    }

    #[test]
    fn download_candidate_table() {
        use TrailerQuality::{Hd720, Hd1080};
        struct Row {
            name: &'static str,
            trailers: Vec<Trailer>,
            prefs: TrailerPreferences,
            expected: Vec<&'static str>,
        }
        let rows = vec![
            Row {
                name: "language and quality come first, in source order",
                trailers: vec![
                    named("en", web("en", "1080p", "en-US")),
                    named("de1", web("de1", "1080p", "de-DE")),
                    named("de2", youtube("de2", "", "de-DE")),
                    named("de-low", web("de-low", "480p", "de-DE")),
                ],
                prefs: prefs("de", Hd1080),
                expected: vec!["de1", "de2"],
            },
            Row {
                name: "then trailers with no language",
                trailers: vec![
                    named("en", web("en", "1080p", "en-US")),
                    named("none", web("none", "1080p", "")),
                    named("de-low", web("de-low", "480p", "de-DE")),
                ],
                prefs: prefs("de", Hd1080),
                expected: vec!["none"],
            },
            Row {
                name: "then quality alone",
                trailers: vec![
                    named("en", web("en", "1080p", "en-US")),
                    named("fr", web("fr", "1080p", "fr-FR")),
                    named("de-low", web("de-low", "480p", "de-DE")),
                ],
                prefs: prefs("de", Hd1080),
                expected: vec!["en", "fr"],
            },
            Row {
                name: "then the only trailer, whatever its quality",
                trailers: vec![named("one", web("one", "480p", "en-US"))],
                prefs: prefs("de", Hd1080),
                expected: vec!["one"],
            },
            Row {
                name: "several poor trailers are not guessed at",
                trailers: vec![
                    named("a", web("a", "480p", "en-US")),
                    named("b", web("b", "360p", "en-US")),
                ],
                prefs: prefs("de", Hd1080),
                expected: vec![],
            },
            Row {
                name: "local files are never candidates",
                trailers: vec![
                    named(
                        "local",
                        trailer("file:///C:/m/heat-trailer.mkv", "1080p", "de"),
                    ),
                    named("web", web("web", "1080p", "de")),
                ],
                prefs: prefs("de", Hd1080),
                expected: vec!["web"],
            },
            Row {
                name: "a lone local file leaves nothing",
                trailers: vec![named("local", trailer("file:///x.mkv", "", ""))],
                prefs: prefs("", Hd720),
                expected: vec![],
            },
            Row {
                name: "without a language preference every acceptable trailer qualifies",
                trailers: vec![
                    named("fr", web("fr", "720p", "fr-FR")),
                    named("en", web("en", "720p", "en-US")),
                    named("low", web("low", "480p", "en-US")),
                ],
                prefs: prefs("", Hd720),
                expected: vec!["fr", "en"],
            },
            Row {
                name: "no trailers",
                trailers: vec![],
                prefs: prefs("de", Hd720),
                expected: vec![],
            },
        ];
        for row in rows {
            let list = download_candidates(&row.trailers, &row.prefs);
            assert_eq!(ids(&list), row.expected, "{}", row.name);
        }
    }
}

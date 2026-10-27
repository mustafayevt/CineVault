//! Languages and countries as TheTVDB spells them.
//!
//! TheTVDB v4 uses three-letter language codes (`eng`, `deu`, `tur`) and three-letter country
//! codes (`usa`, `gbr`). The app uses two-letter codes (`en`, `US`), so both directions are
//! mapped here.

/// `(app code, TheTVDB code)` for the languages TheTVDB serves. The first entry for a TheTVDB
/// code is the one mapped back.
const LANGUAGES: &[(&str, &str)] = &[
    ("ar", "ara"),
    ("bg", "bul"),
    ("ca", "cat"),
    ("cs", "ces"),
    ("da", "dan"),
    ("de", "deu"),
    ("el", "ell"),
    ("en", "eng"),
    ("es", "spa"),
    ("et", "est"),
    ("eu", "eus"),
    ("fa", "fas"),
    ("fi", "fin"),
    ("fr", "fra"),
    ("he", "heb"),
    ("hi", "hin"),
    ("hr", "hrv"),
    ("hu", "hun"),
    ("id", "ind"),
    ("is", "isl"),
    ("it", "ita"),
    ("ja", "jpn"),
    ("ko", "kor"),
    ("lt", "lit"),
    ("lv", "lav"),
    ("nb", "nor"),
    ("nl", "nld"),
    ("no", "nor"),
    ("pl", "pol"),
    ("pt", "por"),
    ("ro", "ron"),
    ("ru", "rus"),
    ("sk", "slk"),
    ("sl", "slv"),
    ("sr", "srp"),
    ("sv", "swe"),
    ("th", "tha"),
    ("tr", "tur"),
    ("uk", "ukr"),
    ("vi", "vie"),
    ("zh", "zho"),
];

/// Regional variants TheTVDB lists as languages of their own: Brazilian Portuguese is `pt`
/// (European Portuguese is `por`), traditional Chinese is `zhtw`, Canadian French is `frc`.
const REGIONAL: &[(&str, &str, &str)] = &[
    ("pt", "BR", "pt"),
    ("zh", "TW", "zhtw"),
    ("zh", "HK", "zhtw"),
    ("fr", "CA", "frc"),
];

/// The TheTVDB language code for an app language such as `tr`, `pt-BR` or `de_DE`. A code that
/// is already three letters (or one of TheTVDB's own regional codes) is used as given. Blank
/// input gives `None`.
pub(super) fn tvdb_language(code: &str) -> Option<String> {
    let code = code.trim();
    if code.is_empty() {
        return None;
    }
    let mut parts = code.split(['-', '_']);
    let language = parts.next()?.trim().to_ascii_lowercase();
    let region = parts.next().map(|r| r.trim().to_ascii_uppercase());
    if language.is_empty() {
        return None;
    }
    if let Some(region) = &region
        && let Some((_, _, tvdb)) = REGIONAL
            .iter()
            .find(|(l, r, _)| *l == language && r == region)
    {
        return Some((*tvdb).to_owned());
    }
    if let Some((_, tvdb)) = LANGUAGES.iter().find(|(app, _)| *app == language) {
        return Some((*tvdb).to_owned());
    }
    // Already a TheTVDB code (`eng`, `pt`, `zhtw`).
    Some(language)
}

/// The app's language code for a TheTVDB code, or the code itself when unknown.
pub(super) fn app_language(tvdb: &str) -> String {
    let tvdb = tvdb.trim().to_ascii_lowercase();
    if let Some((app, _, _)) = REGIONAL.iter().find(|(_, _, t)| *t == tvdb) {
        return (*app).to_owned();
    }
    LANGUAGES
        .iter()
        .find(|(_, t)| *t == tvdb)
        .map_or(tvdb, |(app, _)| (*app).to_owned())
}

/// The order in which languages are tried: the preferred one, the fallback, then English.
pub(super) fn language_chain(preferred: &str, fallback: &str) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    for code in [
        tvdb_language(preferred),
        tvdb_language(fallback),
        Some("eng".to_owned()),
    ]
    .into_iter()
    .flatten()
    {
        if !chain.contains(&code) {
            chain.push(code);
        }
    }
    chain
}

/// `(alpha-2, alpha-3)` for the countries TheTVDB lists most often.
const COUNTRIES: &[(&str, &str)] = &[
    ("AR", "arg"),
    ("AT", "aut"),
    ("AU", "aus"),
    ("BE", "bel"),
    ("BR", "bra"),
    ("CA", "can"),
    ("CH", "che"),
    ("CN", "chn"),
    ("CZ", "cze"),
    ("DE", "deu"),
    ("DK", "dnk"),
    ("ES", "esp"),
    ("FI", "fin"),
    ("FR", "fra"),
    ("GB", "gbr"),
    ("GR", "grc"),
    ("HK", "hkg"),
    ("HU", "hun"),
    ("ID", "idn"),
    ("IE", "irl"),
    ("IL", "isr"),
    ("IN", "ind"),
    ("IT", "ita"),
    ("JP", "jpn"),
    ("KR", "kor"),
    ("MX", "mex"),
    ("NL", "nld"),
    ("NO", "nor"),
    ("NZ", "nzl"),
    ("PL", "pol"),
    ("PT", "prt"),
    ("RU", "rus"),
    ("SE", "swe"),
    ("TH", "tha"),
    ("TR", "tur"),
    ("TW", "twn"),
    ("UA", "ukr"),
    ("US", "usa"),
    ("ZA", "zaf"),
];

/// The two-letter country code for a TheTVDB country (`usa` is `US`). Unknown three-letter
/// codes are upper-cased and kept; two-letter codes pass through.
pub(super) fn country_alpha2(code: &str) -> String {
    let code = code.trim();
    if code.len() == 2 {
        return code.to_ascii_uppercase();
    }
    COUNTRIES
        .iter()
        .find(|(_, three)| three.eq_ignore_ascii_case(code))
        .map_or_else(|| code.to_ascii_uppercase(), |(two, _)| (*two).to_owned())
}

/// Whether a TheTVDB country (`usa`) is the app country (`US`, or `usa`).
pub(super) fn same_country(tvdb: &str, app: &str) -> bool {
    let (tvdb, app) = (tvdb.trim(), app.trim());
    !tvdb.is_empty() && !app.is_empty() && country_alpha2(tvdb) == country_alpha2(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_languages_become_tvdb_codes() {
        assert_eq!(tvdb_language("en").as_deref(), Some("eng"));
        assert_eq!(tvdb_language("tr-TR").as_deref(), Some("tur"));
        assert_eq!(tvdb_language("de_DE").as_deref(), Some("deu"));
        assert_eq!(tvdb_language("ja").as_deref(), Some("jpn"));
        assert_eq!(tvdb_language(" "), None);
    }

    #[test]
    fn brazilian_portuguese_is_not_european_portuguese() {
        assert_eq!(tvdb_language("pt-BR").as_deref(), Some("pt"));
        assert_eq!(tvdb_language("pt-PT").as_deref(), Some("por"));
        assert_eq!(tvdb_language("pt").as_deref(), Some("por"));
        assert_eq!(tvdb_language("zh-TW").as_deref(), Some("zhtw"));
        assert_eq!(tvdb_language("zh").as_deref(), Some("zho"));
    }

    #[test]
    fn tvdb_codes_pass_through_and_map_back() {
        assert_eq!(tvdb_language("fra").as_deref(), Some("fra"));
        assert_eq!(app_language("fra"), "fr");
        assert_eq!(app_language("pt"), "pt");
        assert_eq!(app_language("por"), "pt");
        assert_eq!(app_language("zhtw"), "zh");
        assert_eq!(app_language("xyz"), "xyz");
    }

    #[test]
    fn the_chain_is_preferred_fallback_then_english_without_repeats() {
        assert_eq!(language_chain("tr", "de"), vec!["tur", "deu", "eng"]);
        assert_eq!(language_chain("en", "en"), vec!["eng"]);
        assert_eq!(language_chain("tr", "en"), vec!["tur", "eng"]);
        assert_eq!(language_chain("", ""), vec!["eng"]);
    }

    #[test]
    fn countries_map_both_ways() {
        assert_eq!(country_alpha2("usa"), "US");
        assert_eq!(country_alpha2("GBR"), "GB");
        assert_eq!(country_alpha2("de"), "DE");
        assert_eq!(country_alpha2("zzz"), "ZZZ");
        assert!(same_country("usa", "US"));
        assert!(same_country("deu", "de"));
        assert!(!same_country("usa", "GB"));
        assert!(!same_country("", "US"));
    }
}

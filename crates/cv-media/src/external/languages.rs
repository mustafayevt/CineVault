//! Language and region lookup tables for file-name detection.

/// One language: its canonical code (ISO 639-1 where one exists, else the ISO 639-2 terminology
/// code), the bibliographic ISO 639-2 code when it differs, and lower-case names.
struct Language {
    code: &'static str,
    /// ISO 639-2 terminology code (`deu`), empty when the canonical code is already three letters.
    t: &'static str,
    /// ISO 639-2 bibliographic code (`ger`), empty when equal to `t`.
    b: &'static str,
    names: &'static [&'static str],
}

const fn lang(
    code: &'static str,
    t: &'static str,
    b: &'static str,
    names: &'static [&'static str],
) -> Language {
    Language { code, t, b, names }
}

const LANGUAGES: &[Language] = &[
    lang("af", "afr", "", &["afrikaans"]),
    lang("sq", "sqi", "alb", &["albanian", "shqip"]),
    lang("ar", "ara", "", &["arabic"]),
    lang("hy", "hye", "arm", &["armenian"]),
    lang("az", "aze", "", &["azerbaijani", "azeri"]),
    lang("eu", "eus", "baq", &["basque", "euskara"]),
    lang("be", "bel", "", &["belarusian", "belarussian"]),
    lang("bn", "ben", "", &["bengali", "bangla"]),
    lang("bs", "bos", "", &["bosnian"]),
    lang("bg", "bul", "", &["bulgarian"]),
    lang("my", "mya", "bur", &["burmese"]),
    lang("ca", "cat", "", &["catalan", "catala", "català"]),
    lang("zh", "zho", "chi", &["chinese", "mandarin", "cantonese"]),
    lang("hr", "hrv", "", &["croatian", "hrvatski"]),
    lang("cs", "ces", "cze", &["czech", "čeština", "cesky"]),
    lang("da", "dan", "", &["danish", "dansk"]),
    lang("nl", "nld", "dut", &["dutch", "nederlands", "flemish"]),
    lang("en", "eng", "", &["english"]),
    lang("eo", "epo", "", &["esperanto"]),
    lang("et", "est", "", &["estonian", "eesti"]),
    lang("fil", "", "", &["filipino", "tagalog"]),
    lang("fi", "fin", "", &["finnish", "suomi"]),
    lang("fr", "fra", "fre", &["french", "français", "francais"]),
    lang("gl", "glg", "", &["galician", "galego"]),
    lang("ka", "kat", "geo", &["georgian"]),
    lang("de", "deu", "ger", &["german", "deutsch"]),
    lang("el", "ell", "gre", &["greek"]),
    lang("gu", "guj", "", &["gujarati"]),
    lang("he", "heb", "", &["hebrew", "iw"]),
    lang("hi", "hin", "", &["hindi"]),
    lang("hu", "hun", "", &["hungarian", "magyar"]),
    lang("is", "isl", "ice", &["icelandic"]),
    lang("id", "ind", "", &["indonesian"]),
    lang("ga", "gle", "", &["irish", "gaeilge"]),
    lang("it", "ita", "", &["italian", "italiano"]),
    lang("ja", "jpn", "", &["japanese", "jp"]),
    lang("kn", "kan", "", &["kannada"]),
    lang("kk", "kaz", "", &["kazakh"]),
    lang("km", "khm", "", &["khmer"]),
    lang("ko", "kor", "", &["korean"]),
    lang("ku", "kur", "", &["kurdish"]),
    lang("lo", "lao", "", &["lao"]),
    lang("la", "lat", "", &["latin"]),
    lang("lv", "lav", "", &["latvian"]),
    lang("lt", "lit", "", &["lithuanian"]),
    lang("mk", "mkd", "mac", &["macedonian"]),
    lang("ms", "msa", "may", &["malay"]),
    lang("ml", "mal", "", &["malayalam"]),
    lang("mt", "mlt", "", &["maltese"]),
    lang("mr", "mar", "", &["marathi"]),
    lang("mn", "mon", "", &["mongolian"]),
    lang("ne", "nep", "", &["nepali"]),
    lang("no", "nor", "", &["norwegian", "norsk"]),
    lang("nb", "nob", "", &["bokmal", "bokmål"]),
    lang("nn", "nno", "", &["nynorsk"]),
    lang("fa", "fas", "per", &["persian", "farsi"]),
    lang("pl", "pol", "", &["polish", "polski"]),
    lang("pt", "por", "", &["portuguese", "português", "portugues"]),
    lang("pa", "pan", "", &["punjabi", "panjabi"]),
    lang("ro", "ron", "rum", &["romanian"]),
    lang("ru", "rus", "", &["russian"]),
    lang("sr", "srp", "", &["serbian"]),
    lang("si", "sin", "", &["sinhala", "sinhalese"]),
    lang("sk", "slk", "slo", &["slovak"]),
    lang("sl", "slv", "", &["slovenian", "slovene"]),
    lang(
        "es",
        "spa",
        "",
        &["spanish", "español", "espanol", "castilian"],
    ),
    lang("sw", "swa", "", &["swahili"]),
    lang("sv", "swe", "", &["swedish", "svenska"]),
    lang("ta", "tam", "", &["tamil"]),
    lang("te", "tel", "", &["telugu"]),
    lang("th", "tha", "", &["thai"]),
    lang("bo", "bod", "tib", &["tibetan"]),
    lang("tr", "tur", "", &["turkish", "türkçe", "turkce"]),
    lang("uk", "ukr", "", &["ukrainian"]),
    lang("ur", "urd", "", &["urdu"]),
    lang("uz", "uzb", "", &["uzbek"]),
    lang("vi", "vie", "", &["vietnamese"]),
    lang("cy", "cym", "wel", &["welsh"]),
    lang("yi", "yid", "", &["yiddish"]),
    lang("zu", "zul", "", &["zulu"]),
];

/// The canonical code of a language token: an ISO 639-1 code, an ISO 639-2 code (terminology or
/// bibliographic) or an English or native name, in any case. `None` when the token is not a
/// known language.
pub(super) fn canonical_code(token: &str) -> Option<&'static str> {
    let lower = token.to_lowercase();
    let lower = lower.as_str();
    LANGUAGES
        .iter()
        .find(|l| {
            l.code == lower
                || (!l.t.is_empty() && l.t == lower)
                || (!l.b.is_empty() && l.b == lower)
                || l.names.contains(&lower)
        })
        .map(|l| l.code)
}

/// A regional language written as one token: `pob` (the code CineVault writes for Brazilian
/// Portuguese subtitles) and `pb` give `pt-BR`. `None` for any other token.
pub(super) fn regional_alias(token: &str) -> Option<&'static str> {
    match token.to_lowercase().as_str() {
        "pob" | "pb" => Some("pt-BR"),
        _ => None,
    }
}

/// ISO 3166-1 alpha-2 region codes accepted after a language (`pt-BR`, `es_MX`).
const REGIONS: &str = "AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ \
BL BM BN BO BQ BR BS BT BV BW BY BZ CA CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK \
DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU \
GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ \
LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY \
MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU \
RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO \
TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT ZA ZM ZW";

pub(super) fn is_region(token: &str) -> bool {
    token.len() == 2
        && token.chars().all(|c| c.is_ascii_alphabetic())
        && REGIONS
            .split_ascii_whitespace()
            .any(|r| r.eq_ignore_ascii_case(token))
}

/// A script subtag that appears in subtitle names (`Hans`, `Hant`, `Latn` ...).
pub(super) fn is_script(token: &str) -> bool {
    [
        "hans", "hant", "latn", "cyrl", "arab", "hebr", "deva", "jpan", "kore",
    ]
    .iter()
    .any(|s| s.eq_ignore_ascii_case(token))
}

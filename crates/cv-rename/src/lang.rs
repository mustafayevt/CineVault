//! The language part of a subtitle file name.
//!
//! A subtitle next to a video is named `<base>.<language>[.forced][.sdh][.<title>].<ext>`. The
//! language is written in the style of the profile: the two-letter code (`en`), the three-letter
//! terminology code (`eng`), the bibliographic code (`ger`), the English name (`English`), or
//! exactly as the stream has it.

/// How a subtitle language is written in a file name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SubtitleLanguageStyle {
    /// ISO 639-1: `en`, `de`.
    Iso2,
    /// ISO 639-2 terminology: `eng`, `deu`.
    #[default]
    Iso3T,
    /// ISO 639-2 bibliographic: `eng`, `ger`.
    Iso3B,
    /// The English name: `English`, `German`.
    LangEn,
    /// The name in the interface language. Only English names exist so far, so it is the same as
    /// [`SubtitleLanguageStyle::LangEn`].
    LangLocalized,
    /// The language exactly as the stream stores it: `pt-BR`.
    Original,
}

impl SubtitleLanguageStyle {
    /// Parses the setting name (`ISO2`, `ISO3T`, `ISO3B`, `LANG_EN`, `LANG_LOCALIZED`,
    /// `ORIGINAL`), ignoring case.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_uppercase().as_str() {
            "ISO2" => Some(Self::Iso2),
            "ISO3T" => Some(Self::Iso3T),
            "ISO3B" => Some(Self::Iso3B),
            "LANG_EN" => Some(Self::LangEn),
            "LANG_LOCALIZED" => Some(Self::LangLocalized),
            "ORIGINAL" => Some(Self::Original),
            _ => None,
        }
    }

    /// The setting name [`SubtitleLanguageStyle::parse`] reads back.
    pub fn name(self) -> &'static str {
        match self {
            Self::Iso2 => "ISO2",
            Self::Iso3T => "ISO3T",
            Self::Iso3B => "ISO3B",
            Self::LangEn => "LANG_EN",
            Self::LangLocalized => "LANG_LOCALIZED",
            Self::Original => "ORIGINAL",
        }
    }
}

/// `(ISO 639-1 or 639-2 code, 639-2 terminology, 639-2 bibliographic, English name)`.
const LANGUAGES: &[(&str, &str, &str, &str)] = &[
    ("af", "afr", "afr", "Afrikaans"),
    ("sq", "sqi", "alb", "Albanian"),
    ("ar", "ara", "ara", "Arabic"),
    ("hy", "hye", "arm", "Armenian"),
    ("az", "aze", "aze", "Azerbaijani"),
    ("eu", "eus", "baq", "Basque"),
    ("be", "bel", "bel", "Belarusian"),
    ("bn", "ben", "ben", "Bengali"),
    ("bs", "bos", "bos", "Bosnian"),
    ("bg", "bul", "bul", "Bulgarian"),
    ("my", "mya", "bur", "Burmese"),
    ("ca", "cat", "cat", "Catalan"),
    ("zh", "zho", "chi", "Chinese"),
    ("hr", "hrv", "hrv", "Croatian"),
    ("cs", "ces", "cze", "Czech"),
    ("da", "dan", "dan", "Danish"),
    ("nl", "nld", "dut", "Dutch"),
    ("en", "eng", "eng", "English"),
    ("eo", "epo", "epo", "Esperanto"),
    ("et", "est", "est", "Estonian"),
    ("fil", "fil", "fil", "Filipino"),
    ("fi", "fin", "fin", "Finnish"),
    ("fr", "fra", "fre", "French"),
    ("gl", "glg", "glg", "Galician"),
    ("ka", "kat", "geo", "Georgian"),
    ("de", "deu", "ger", "German"),
    ("el", "ell", "gre", "Greek"),
    ("gu", "guj", "guj", "Gujarati"),
    ("he", "heb", "heb", "Hebrew"),
    ("hi", "hin", "hin", "Hindi"),
    ("hu", "hun", "hun", "Hungarian"),
    ("is", "isl", "ice", "Icelandic"),
    ("id", "ind", "ind", "Indonesian"),
    ("ga", "gle", "gle", "Irish"),
    ("it", "ita", "ita", "Italian"),
    ("ja", "jpn", "jpn", "Japanese"),
    ("kn", "kan", "kan", "Kannada"),
    ("kk", "kaz", "kaz", "Kazakh"),
    ("km", "khm", "khm", "Khmer"),
    ("ko", "kor", "kor", "Korean"),
    ("ku", "kur", "kur", "Kurdish"),
    ("lo", "lao", "lao", "Lao"),
    ("la", "lat", "lat", "Latin"),
    ("lv", "lav", "lav", "Latvian"),
    ("lt", "lit", "lit", "Lithuanian"),
    ("mk", "mkd", "mac", "Macedonian"),
    ("ms", "msa", "may", "Malay"),
    ("ml", "mal", "mal", "Malayalam"),
    ("mt", "mlt", "mlt", "Maltese"),
    ("mr", "mar", "mar", "Marathi"),
    ("mn", "mon", "mon", "Mongolian"),
    ("ne", "nep", "nep", "Nepali"),
    ("no", "nor", "nor", "Norwegian"),
    ("nb", "nob", "nob", "Norwegian Bokmål"),
    ("nn", "nno", "nno", "Norwegian Nynorsk"),
    ("fa", "fas", "per", "Persian"),
    ("pl", "pol", "pol", "Polish"),
    ("pt", "por", "por", "Portuguese"),
    ("pa", "pan", "pan", "Punjabi"),
    ("ro", "ron", "rum", "Romanian"),
    ("ru", "rus", "rus", "Russian"),
    ("sr", "srp", "srp", "Serbian"),
    ("si", "sin", "sin", "Sinhala"),
    ("sk", "slk", "slo", "Slovak"),
    ("sl", "slv", "slv", "Slovenian"),
    ("es", "spa", "spa", "Spanish"),
    ("sw", "swa", "swa", "Swahili"),
    ("sv", "swe", "swe", "Swedish"),
    ("ta", "tam", "tam", "Tamil"),
    ("te", "tel", "tel", "Telugu"),
    ("th", "tha", "tha", "Thai"),
    ("bo", "bod", "tib", "Tibetan"),
    ("tr", "tur", "tur", "Turkish"),
    ("uk", "ukr", "ukr", "Ukrainian"),
    ("ur", "urd", "urd", "Urdu"),
    ("uz", "uzb", "uzb", "Uzbek"),
    ("vi", "vie", "vie", "Vietnamese"),
    ("cy", "cym", "wel", "Welsh"),
    ("yi", "yid", "yid", "Yiddish"),
    ("zu", "zul", "zul", "Zulu"),
];

/// Brazilian Portuguese has a code of its own in file names.
fn is_brazilian(normalized: &str) -> bool {
    matches!(normalized, "pt-br" | "por-br" | "pob" | "pb")
}

fn find(
    primary: &str,
) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    LANGUAGES
        .iter()
        .find(|(code, t, b, _)| *code == primary || *t == primary || *b == primary)
}

/// The language as written in a file name. A language this table does not know (and an empty
/// one) is written as the stream has it.
pub fn language_tag(language: &str, style: SubtitleLanguageStyle) -> String {
    let language = language.trim();
    if language.is_empty() || style == SubtitleLanguageStyle::Original {
        return language.to_owned();
    }
    let normalized = language.replace('_', "-").to_lowercase();
    if is_brazilian(&normalized) {
        return match style {
            SubtitleLanguageStyle::Iso2 => "pb",
            SubtitleLanguageStyle::Iso3T | SubtitleLanguageStyle::Iso3B => "pob",
            _ => "Portuguese",
        }
        .to_owned();
    }
    let primary = normalized.split('-').next().unwrap_or_default();
    let Some((code, t, b, name)) = find(primary) else {
        return language.to_owned();
    };
    match style {
        SubtitleLanguageStyle::Iso2 => (*code).to_owned(),
        SubtitleLanguageStyle::Iso3T => (*t).to_owned(),
        SubtitleLanguageStyle::Iso3B => (*b).to_owned(),
        SubtitleLanguageStyle::LangEn | SubtitleLanguageStyle::LangLocalized => (*name).to_owned(),
        SubtitleLanguageStyle::Original => language.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SubtitleLanguageStyle::*;

    #[test]
    fn styles_write_the_language_as_the_profile_wants() {
        let cases = [
            ("en", Iso2, "en"),
            ("en", Iso3T, "eng"),
            ("de", Iso3T, "deu"),
            ("de", Iso3B, "ger"),
            ("fr", Iso3B, "fre"),
            ("zh", Iso3B, "chi"),
            ("zh", Iso3T, "zho"),
            ("es", LangEn, "Spanish"),
            ("es", LangLocalized, "Spanish"),
            ("pt-BR", Original, "pt-BR"),
        ];
        for (language, style, want) in cases {
            assert_eq!(language_tag(language, style), want, "{language} {style:?}");
        }
    }

    #[test]
    fn brazilian_portuguese_has_its_own_code() {
        assert_eq!(language_tag("pt-BR", Iso2), "pb");
        assert_eq!(language_tag("pt-BR", Iso3T), "pob");
        assert_eq!(language_tag("pt-BR", Iso3B), "pob");
        assert_eq!(language_tag("pt-BR", LangEn), "Portuguese");
        assert_eq!(language_tag("pt", Iso3T), "por");
    }

    #[test]
    fn three_letter_input_and_regions_are_understood() {
        assert_eq!(language_tag("ger", Iso2), "de");
        assert_eq!(language_tag("deu", Iso3B), "ger");
        assert_eq!(language_tag("en-US", Iso3T), "eng");
        assert_eq!(language_tag("EN", Iso3T), "eng");
    }

    #[test]
    fn unknown_and_empty_languages_stay_as_written() {
        assert_eq!(language_tag("xx", Iso3T), "xx");
        assert_eq!(language_tag("", Iso3T), "");
        assert_eq!(language_tag("  ", Iso2), "");
    }

    #[test]
    fn setting_names_parse() {
        assert_eq!(SubtitleLanguageStyle::parse("iso3b"), Some(Iso3B));
        assert_eq!(
            SubtitleLanguageStyle::parse("LANG_LOCALIZED"),
            Some(LangLocalized)
        );
        assert_eq!(SubtitleLanguageStyle::parse("nope"), None);
    }
}

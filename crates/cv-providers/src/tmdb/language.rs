//! Request languages and translation lookup.

use super::api::{Text, Translation};

/// The region TMDB's own clients pair with a bare language code.
const DEFAULT_REGIONS: &[(&str, &str)] = &[
    ("ar", "SA"),
    ("bg", "BG"),
    ("ca", "ES"),
    ("cs", "CZ"),
    ("da", "DK"),
    ("de", "DE"),
    ("el", "GR"),
    ("en", "US"),
    ("es", "ES"),
    ("et", "EE"),
    ("eu", "ES"),
    ("fa", "IR"),
    ("fi", "FI"),
    ("fr", "FR"),
    ("he", "IL"),
    ("hi", "IN"),
    ("hr", "HR"),
    ("hu", "HU"),
    ("id", "ID"),
    ("it", "IT"),
    ("ja", "JP"),
    ("ko", "KR"),
    ("lt", "LT"),
    ("lv", "LV"),
    ("nb", "NO"),
    ("nl", "NL"),
    ("no", "NO"),
    ("pl", "PL"),
    ("pt", "PT"),
    ("ro", "RO"),
    ("ru", "RU"),
    ("sk", "SK"),
    ("sl", "SI"),
    ("sr", "RS"),
    ("sv", "SE"),
    ("th", "TH"),
    ("tr", "TR"),
    ("uk", "UA"),
    ("vi", "VN"),
    ("zh", "CN"),
];

/// Regional variants whose translations differ enough that a sibling region is not accepted
/// in their place (Brazilian and European Portuguese, Canadian French, Latin American Spanish).
const STRICT_LOCALES: &[(&str, &str)] = &[("es", "MX"), ("pt", "BR"), ("fr", "CA")];

/// A language and an optional country, normalised: lower-case language, upper-case country.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Locale {
    pub language: String,
    pub country: Option<String>,
}

impl Locale {
    /// Parses `tr`, `tr-TR`, `pt_BR` and similar. Returns `None` for blank input.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let mut parts = text.split(['-', '_']);
        let language = parts.next()?.trim().to_ascii_lowercase();
        if language.is_empty() {
            return None;
        }
        let country = parts
            .next()
            .map(|c| c.trim().to_ascii_uppercase())
            .filter(|c| !c.is_empty());
        Some(Self { language, country })
    }

    /// The `language` query parameter: a bare code gets its usual region (`tr` becomes
    /// `tr-TR`), a code with a region is kept.
    pub fn request_code(&self) -> String {
        let country = self.country.clone().or_else(|| {
            DEFAULT_REGIONS
                .iter()
                .find(|(l, _)| *l == self.language)
                .map(|(_, c)| (*c).to_owned())
        });
        match country {
            Some(c) => format!("{}-{c}", self.language),
            None => self.language.clone(),
        }
    }

    fn is_strict(&self) -> bool {
        self.country.as_deref().is_some_and(|c| {
            STRICT_LOCALES
                .iter()
                .any(|(l, region)| *l == self.language && *region == c)
        })
    }
}

/// The `language` parameter for a requested language, or `None` when it is blank.
pub(super) fn request_language(language: &str) -> Option<String> {
    Locale::parse(language).map(|l| l.request_code())
}

/// The translation entry for `wanted` for which `has_value` is true.
///
/// A translation for the same language and country wins. Otherwise a translation in the same
/// language is accepted, except for the regional variants in [`STRICT_LOCALES`], which need an
/// exact match. A bare language such as `tr` matches any region of that language.
pub(super) fn find_translation<'a>(
    translations: &'a [Translation],
    wanted: &Locale,
    has_value: impl Fn(&Translation) -> bool,
) -> Option<&'a Translation> {
    let usable = |t: &&Translation| has_value(t);
    let same_language = |t: &Translation| t.iso_639_1.text().eq_ignore_ascii_case(&wanted.language);
    if let Some(country) = &wanted.country
        && let Some(exact) = translations
            .iter()
            .filter(usable)
            .find(|t| same_language(t) && t.iso_3166_1.text().eq_ignore_ascii_case(country))
    {
        return Some(exact);
    }
    if wanted.is_strict() {
        return None;
    }
    translations
        .iter()
        .filter(usable)
        .find(|t| same_language(t))
}

/// The title, plot and tagline of an entry once the fallback language has filled the gaps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Texts {
    pub title: String,
    pub overview: String,
    pub tagline: String,
    /// The English title, when TMDB has one (or the original language is English).
    pub english_title: String,
}

/// What the main response carried, in the requested language.
pub(super) struct MainTexts<'a> {
    pub title: &'a str,
    pub original_title: &'a str,
    pub overview: &'a str,
    pub tagline: &'a str,
    pub original_language: &'a str,
}

/// Fills the title, plot and tagline from the translations list.
///
/// The preferred language wins; an empty plot or tagline then comes from the fallback
/// language. A title is only taken from the fallback when the preferred language has none (a
/// Turkish film with a Turkish title keeps it even if the Turkish plot is missing).
pub(super) fn resolve_texts(
    main: &MainTexts<'_>,
    translations: &[Translation],
    preferred: &str,
    fallback: &str,
) -> Texts {
    let preferred = Locale::parse(preferred);
    let fallback = Locale::parse(fallback);
    let title_of = |t: &Translation| -> String {
        let value = if t.data.title.text().trim().is_empty() {
            t.data.name.text()
        } else {
            t.data.title.text()
        };
        value.to_owned()
    };

    let in_language = |wanted: &Option<Locale>, field: &dyn Fn(&Translation) -> String| {
        wanted.as_ref().and_then(|locale| {
            find_translation(translations, locale, |t| !field(t).trim().is_empty()).map(field)
        })
    };

    // Title.
    let preferred_title = in_language(&preferred, &|t| title_of(t));
    let original_is_preferred = preferred
        .as_ref()
        .is_some_and(|p| p.language.eq_ignore_ascii_case(main.original_language));
    let main_title = main.title.trim();
    let title = match preferred_title {
        Some(title) => title,
        None if original_is_preferred || translations.is_empty() => main_title.to_owned(),
        None => in_language(&fallback, &|t| title_of(t))
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| main_title.to_owned()),
    };

    let fill = |main_value: &str, field: &dyn Fn(&Translation) -> String| -> String {
        if !main_value.trim().is_empty() {
            return main_value.to_owned();
        }
        in_language(&preferred, field)
            .or_else(|| in_language(&fallback, field))
            .unwrap_or_default()
    };
    let overview = fill(main.overview, &|t| t.data.overview.text().to_owned());
    let tagline = fill(main.tagline, &|t| t.data.tagline.text().to_owned());

    let english = Locale {
        language: "en".to_owned(),
        country: None,
    };
    let english_title = in_language(&Some(english), &|t| title_of(t)).unwrap_or_else(|| {
        if main.original_language.eq_ignore_ascii_case("en") {
            main.original_title.trim().to_owned()
        } else {
            String::new()
        }
    });

    Texts {
        title,
        overview,
        tagline,
        english_title,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmdb::api::TranslationData;

    fn translation(language: &str, country: &str, title: &str, overview: &str) -> Translation {
        Translation {
            iso_639_1: Some(language.into()),
            iso_3166_1: Some(country.into()),
            data: TranslationData {
                title: Some(title.into()),
                name: None,
                overview: Some(overview.into()),
                tagline: None,
            },
        }
    }

    #[test]
    fn a_bare_language_gets_its_usual_region() {
        assert_eq!(request_language("tr").as_deref(), Some("tr-TR"));
        assert_eq!(request_language("en").as_deref(), Some("en-US"));
        assert_eq!(request_language("de").as_deref(), Some("de-DE"));
        assert_eq!(request_language("ja").as_deref(), Some("ja-JP"));
    }

    #[test]
    fn a_language_with_a_region_is_kept_and_normalised() {
        assert_eq!(request_language("pt-br").as_deref(), Some("pt-BR"));
        assert_eq!(request_language("pt_BR").as_deref(), Some("pt-BR"));
        assert_eq!(request_language("en-GB").as_deref(), Some("en-GB"));
    }

    #[test]
    fn unknown_languages_pass_through_and_blank_is_none() {
        assert_eq!(request_language("xx").as_deref(), Some("xx"));
        assert_eq!(request_language("  "), None);
    }

    #[test]
    fn exact_region_beats_the_same_language() {
        let list = vec![
            translation("de", "AT", "Hitze AT", "x"),
            translation("de", "DE", "Heat DE", "x"),
        ];
        let wanted = Locale::parse("de-DE").unwrap();
        let found = find_translation(&list, &wanted, |t| !t.data.title.text().is_empty()).unwrap();
        assert_eq!(found.data.title.text(), "Heat DE");
    }

    #[test]
    fn the_same_language_is_accepted_for_ordinary_locales() {
        let list = vec![translation("de", "AT", "Hitze AT", "x")];
        let wanted = Locale::parse("de-DE").unwrap();
        assert!(find_translation(&list, &wanted, |t| !t.data.title.text().is_empty()).is_some());
        let bare = Locale::parse("de").unwrap();
        assert!(find_translation(&list, &bare, |t| !t.data.title.text().is_empty()).is_some());
    }

    #[test]
    fn brazilian_portuguese_needs_an_exact_match() {
        let list = vec![translation("pt", "PT", "Fogo Cruzado", "x")];
        let wanted = Locale::parse("pt-BR").unwrap();
        assert!(find_translation(&list, &wanted, |t| !t.data.title.text().is_empty()).is_none());
        let exact = vec![translation("pt", "BR", "Fogo Brasil", "x")];
        assert!(find_translation(&exact, &wanted, |t| !t.data.title.text().is_empty()).is_some());
        // The same applies to Canadian French and Mexican Spanish.
        let fr = vec![translation("fr", "FR", "Chaleur", "x")];
        assert!(
            find_translation(&fr, &Locale::parse("fr-CA").unwrap(), |t| !t
                .data
                .title
                .text()
                .is_empty())
            .is_none()
        );
    }

    #[test]
    fn empty_values_are_skipped() {
        let list = vec![
            translation("tr", "TR", "", "Bos"),
            translation("tr", "TR", "Ates", "x"),
        ];
        let wanted = Locale::parse("tr").unwrap();
        let found = find_translation(&list, &wanted, |t| !t.data.title.text().is_empty()).unwrap();
        assert_eq!(found.data.title.text(), "Ates");
    }

    fn main_texts<'a>(title: &'a str, overview: &'a str) -> MainTexts<'a> {
        MainTexts {
            title,
            original_title: "Heat",
            overview,
            tagline: "",
            original_language: "en",
        }
    }

    #[test]
    fn a_missing_preferred_plot_comes_from_the_fallback_but_the_title_stays() {
        let list = vec![
            translation("tr", "TR", "Ates", ""),
            translation("en", "US", "Heat", "A group of professional bank robbers."),
        ];
        let texts = resolve_texts(&main_texts("Ates", ""), &list, "tr", "en");
        assert_eq!(texts.title, "Ates");
        assert_eq!(texts.overview, "A group of professional bank robbers.");
        assert_eq!(texts.english_title, "Heat");
    }

    #[test]
    fn a_missing_preferred_title_comes_from_the_fallback() {
        let list = vec![translation("en", "US", "Heat", "Plot")];
        let texts = resolve_texts(&main_texts("Original", ""), &list, "tr", "en");
        assert_eq!(texts.title, "Heat");
        assert_eq!(texts.overview, "Plot");
    }

    #[test]
    fn the_response_is_used_when_there_are_no_translations() {
        let texts = resolve_texts(&main_texts("Heat", "Plot"), &[], "tr", "en");
        assert_eq!(texts.title, "Heat");
        assert_eq!(texts.overview, "Plot");
    }

    #[test]
    fn the_original_language_keeps_the_original_title() {
        let list = vec![translation("fr", "FR", "Chaleur", "x")];
        let mut main = main_texts("Heat", "Plot");
        main.original_language = "en";
        let texts = resolve_texts(&main, &list, "en", "fr");
        assert_eq!(texts.title, "Heat");
    }
}

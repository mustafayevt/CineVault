//! Genre and language names in the NFO language, and ISO 639-2 codes for stream languages.
//!
//! Genres and spoken languages are written in the language chosen in the NFO options. English,
//! German and French are translated here; any other NFO language falls back to English names.
//! Stream languages in `<fileinfo>` are always the three-letter ISO 639-2 terminology code.

use cv_domain::Genre;

/// One language: ISO 639-1 code (or the 639-2 code when there is no two-letter one), 639-2
/// terminology code, English name, German name and French name. An empty German or French name
/// means the English name is used.
struct Language {
    code: &'static str,
    t: &'static str,
    en: &'static str,
    de: &'static str,
    fr: &'static str,
}

const fn l(
    code: &'static str,
    t: &'static str,
    en: &'static str,
    de: &'static str,
    fr: &'static str,
) -> Language {
    Language {
        code,
        t,
        en,
        de,
        fr,
    }
}

const LANGUAGES: &[Language] = &[
    l("af", "afr", "Afrikaans", "Afrikaans", "afrikaans"),
    l("sq", "sqi", "Albanian", "Albanisch", "albanais"),
    l("ar", "ara", "Arabic", "Arabisch", "arabe"),
    l("hy", "hye", "Armenian", "Armenisch", "arménien"),
    l("az", "aze", "Azerbaijani", "Aserbaidschanisch", "azéri"),
    l("eu", "eus", "Basque", "Baskisch", "basque"),
    l("be", "bel", "Belarusian", "Belarussisch", "biélorusse"),
    l("bn", "ben", "Bengali", "Bengalisch", "bengali"),
    l("bs", "bos", "Bosnian", "Bosnisch", "bosniaque"),
    l("bg", "bul", "Bulgarian", "Bulgarisch", "bulgare"),
    l("my", "mya", "Burmese", "Birmanisch", "birman"),
    l("ca", "cat", "Catalan", "Katalanisch", "catalan"),
    l("zh", "zho", "Chinese", "Chinesisch", "chinois"),
    l("hr", "hrv", "Croatian", "Kroatisch", "croate"),
    l("cs", "ces", "Czech", "Tschechisch", "tchèque"),
    l("da", "dan", "Danish", "Dänisch", "danois"),
    l("nl", "nld", "Dutch", "Niederländisch", "néerlandais"),
    l("en", "eng", "English", "Englisch", "anglais"),
    l("eo", "epo", "Esperanto", "Esperanto", "espéranto"),
    l("et", "est", "Estonian", "Estnisch", "estonien"),
    l("fil", "fil", "Filipino", "Filipino", "filipino"),
    l("fi", "fin", "Finnish", "Finnisch", "finnois"),
    l("fr", "fra", "French", "Französisch", "français"),
    l("gl", "glg", "Galician", "Galicisch", "galicien"),
    l("ka", "kat", "Georgian", "Georgisch", "géorgien"),
    l("de", "deu", "German", "Deutsch", "allemand"),
    l("el", "ell", "Greek", "Griechisch", "grec"),
    l("gu", "guj", "Gujarati", "Gujarati", "goudjarati"),
    l("he", "heb", "Hebrew", "Hebräisch", "hébreu"),
    l("hi", "hin", "Hindi", "Hindi", "hindi"),
    l("hu", "hun", "Hungarian", "Ungarisch", "hongrois"),
    l("is", "isl", "Icelandic", "Isländisch", "islandais"),
    l("id", "ind", "Indonesian", "Indonesisch", "indonésien"),
    l("ga", "gle", "Irish", "Irisch", "irlandais"),
    l("it", "ita", "Italian", "Italienisch", "italien"),
    l("ja", "jpn", "Japanese", "Japanisch", "japonais"),
    l("kn", "kan", "Kannada", "Kannada", "kannada"),
    l("kk", "kaz", "Kazakh", "Kasachisch", "kazakh"),
    l("km", "khm", "Khmer", "Khmer", "khmer"),
    l("ko", "kor", "Korean", "Koreanisch", "coréen"),
    l("ku", "kur", "Kurdish", "Kurdisch", "kurde"),
    l("lo", "lao", "Lao", "Laotisch", "lao"),
    l("la", "lat", "Latin", "Latein", "latin"),
    l("lv", "lav", "Latvian", "Lettisch", "letton"),
    l("lt", "lit", "Lithuanian", "Litauisch", "lituanien"),
    l("mk", "mkd", "Macedonian", "Mazedonisch", "macédonien"),
    l("ms", "msa", "Malay", "Malaiisch", "malais"),
    l("ml", "mal", "Malayalam", "Malayalam", "malayalam"),
    l("mt", "mlt", "Maltese", "Maltesisch", "maltais"),
    l("mr", "mar", "Marathi", "Marathi", "marathi"),
    l("mn", "mon", "Mongolian", "Mongolisch", "mongol"),
    l("ne", "nep", "Nepali", "Nepalesisch", "népalais"),
    l("no", "nor", "Norwegian", "Norwegisch", "norvégien"),
    l(
        "nb",
        "nob",
        "Norwegian Bokmål",
        "Norwegisch Bokmål",
        "norvégien bokmål",
    ),
    l(
        "nn",
        "nno",
        "Norwegian Nynorsk",
        "Norwegisch Nynorsk",
        "norvégien nynorsk",
    ),
    l("fa", "fas", "Persian", "Persisch", "persan"),
    l("pl", "pol", "Polish", "Polnisch", "polonais"),
    l("pt", "por", "Portuguese", "Portugiesisch", "portugais"),
    l("pa", "pan", "Punjabi", "Pandschabi", "pendjabi"),
    l("ro", "ron", "Romanian", "Rumänisch", "roumain"),
    l("ru", "rus", "Russian", "Russisch", "russe"),
    l("sr", "srp", "Serbian", "Serbisch", "serbe"),
    l("si", "sin", "Sinhala", "Singhalesisch", "cingalais"),
    l("sk", "slk", "Slovak", "Slowakisch", "slovaque"),
    l("sl", "slv", "Slovenian", "Slowenisch", "slovène"),
    l("es", "spa", "Spanish", "Spanisch", "espagnol"),
    l("sw", "swa", "Swahili", "Suaheli", "swahili"),
    l("sv", "swe", "Swedish", "Schwedisch", "suédois"),
    l("ta", "tam", "Tamil", "Tamil", "tamoul"),
    l("te", "tel", "Telugu", "Telugu", "télougou"),
    l("th", "tha", "Thai", "Thailändisch", "thaï"),
    l("bo", "bod", "Tibetan", "Tibetisch", "tibétain"),
    l("tr", "tur", "Turkish", "Türkisch", "turc"),
    l("uk", "ukr", "Ukrainian", "Ukrainisch", "ukrainien"),
    l("ur", "urd", "Urdu", "Urdu", "ourdou"),
    l("uz", "uzb", "Uzbek", "Usbekisch", "ouzbek"),
    l("vi", "vie", "Vietnamese", "Vietnamesisch", "vietnamien"),
    l("cy", "cym", "Welsh", "Walisisch", "gallois"),
    l("yi", "yid", "Yiddish", "Jiddisch", "yiddish"),
    l("zu", "zul", "Zulu", "Zulu", "zoulou"),
];

/// ISO 639-2 bibliographic codes that differ from the terminology code, mapped to the
/// two-letter code.
const BIBLIOGRAPHIC: &[(&str, &str)] = &[
    ("alb", "sq"),
    ("arm", "hy"),
    ("baq", "eu"),
    ("bur", "my"),
    ("chi", "zh"),
    ("cze", "cs"),
    ("dut", "nl"),
    ("fre", "fr"),
    ("geo", "ka"),
    ("ger", "de"),
    ("gre", "el"),
    ("ice", "is"),
    ("mac", "mk"),
    ("may", "ms"),
    ("per", "fa"),
    ("rum", "ro"),
    ("slo", "sk"),
    ("tib", "bo"),
    ("wel", "cy"),
];

/// Finds a language by two-letter code, three-letter code (either form) or English name, with
/// any region or script suffix (`pt-BR`) ignored.
fn lookup(token: &str) -> Option<&'static Language> {
    let lower = token.trim().to_lowercase();
    let primary = lower.split(['-', '_']).next().unwrap_or_default();
    let primary = BIBLIOGRAPHIC
        .iter()
        .find(|(b, _)| *b == primary)
        .map_or(primary, |(_, code)| *code);
    LANGUAGES
        .iter()
        .find(|l| l.code == primary || l.t == primary)
        .or_else(|| LANGUAGES.iter().find(|l| l.en.to_lowercase() == lower))
}

/// The three-letter ISO 639-2 code written for a stream language (`en` gives `eng`). An unknown
/// language keeps its text in lower case with any region removed.
pub fn stream_language_code(language: &str) -> String {
    // Kodi spells Brazilian Portuguese `pob`.
    let normalized = language.trim().replace('_', "-").to_lowercase();
    if normalized == "pt-br" || normalized == "por-br" {
        return "pob".to_owned();
    }
    match lookup(language) {
        Some(found) => found.t.to_owned(),
        None => language
            .trim()
            .to_lowercase()
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_owned(),
    }
}

/// The name of a spoken language in the NFO language. An unknown language prints as given.
pub fn language_name(language: &str, nfo_language: &str) -> String {
    let Some(found) = lookup(language) else {
        return language.trim().to_owned();
    };
    let name = match primary_language(nfo_language).as_str() {
        "de" => found.de,
        "fr" => found.fr,
        _ => found.en,
    };
    if name.is_empty() { found.en } else { name }.to_owned()
}

fn primary_language(nfo_language: &str) -> String {
    nfo_language
        .trim()
        .to_lowercase()
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// English, German and French name of each built-in genre, in the order of [`Genre::known`].
const GENRES: &[(&str, &str, &str, &str)] = &[
    ("ACTION", "Action", "Action", "Action"),
    ("ADVENTURE", "Adventure", "Abenteuer", "Aventure"),
    ("ANIMATION", "Animation", "Animation", "Animation"),
    ("ANIME", "Anime", "Anime", "Anime"),
    ("ANIMAL", "Animal", "Tierfilm", "Animalier"),
    ("BIOGRAPHY", "Biography", "Biografie", "Biographie"),
    ("COMEDY", "Comedy", "Komödie", "Comédie"),
    ("CRIME", "Crime", "Krimi", "Crime"),
    ("DISASTER", "Disaster", "Katastrophenfilm", "Catastrophe"),
    (
        "DOCUMENTARY",
        "Documentary",
        "Dokumentation",
        "Documentaire",
    ),
    ("DRAMA", "Drama", "Drama", "Drame"),
    ("EASTERN", "Eastern", "Eastern", "Eastern"),
    ("EROTIC", "Erotic", "Erotik", "Érotique"),
    ("FAMILY", "Family", "Familie", "Familial"),
    ("FAN_FILM", "Fan Film", "Fan-Film", "Film de fans"),
    ("FANTASY", "Fantasy", "Fantasy", "Fantastique"),
    ("FILM_NOIR", "Film Noir", "Film Noir", "Film noir"),
    ("FOREIGN", "Foreign", "Ausländisch", "Étranger"),
    ("GAME_SHOW", "Game Show", "Spielshow", "Jeu télévisé"),
    ("HISTORY", "History", "Historie", "Histoire"),
    ("HOLIDAY", "Holiday", "Feiertag", "Fêtes"),
    ("HORROR", "Horror", "Horror", "Horreur"),
    ("INDIE", "Indie", "Indie", "Indépendant"),
    ("MINI_SERIES", "Mini-Series", "Miniserie", "Mini-série"),
    ("MUSIC", "Music", "Musik", "Musique"),
    ("MUSICAL", "Musical", "Musical", "Comédie musicale"),
    ("MYSTERY", "Mystery", "Mystery", "Mystère"),
    ("NEO_NOIR", "Neo-noir", "Neo-Noir", "Néo-noir"),
    ("NEWS", "News", "Nachrichten", "Actualités"),
    ("REALITY_TV", "Reality-TV", "Reality-TV", "Télé-réalité"),
    ("ROAD_MOVIE", "Road Movie", "Roadmovie", "Road movie"),
    ("ROMANCE", "Romance", "Liebesfilm", "Romance"),
    (
        "SCIENCE_FICTION",
        "Science Fiction",
        "Science Fiction",
        "Science-fiction",
    ),
    ("SERIES", "Series", "Serie", "Série"),
    ("SHORT", "Short", "Kurzfilm", "Court métrage"),
    ("SILENT_MOVIE", "Silent Movie", "Stummfilm", "Film muet"),
    ("SOAP", "Soap", "Seifenoper", "Feuilleton"),
    ("SPORT", "Sport", "Sport", "Sport"),
    (
        "SPORTING_EVENT",
        "Sporting Event",
        "Sportereignis",
        "Événement sportif",
    ),
    ("SPORTS_FILM", "Sports Film", "Sportfilm", "Film de sport"),
    ("SUSPENSE", "Suspense", "Suspense", "Suspense"),
    ("TALK_SHOW", "Talk Show", "Talkshow", "Talk-show"),
    ("TV_MOVIE", "TV Movie", "TV-Film", "Téléfilm"),
    ("THRILLER", "Thriller", "Thriller", "Thriller"),
    ("WAR", "War", "Kriegsfilm", "Guerre"),
    ("WESTERN", "Western", "Western", "Western"),
];

/// The name of a genre in the NFO language. A genre outside the built-in set prints as the user
/// wrote it.
pub fn genre_name(genre: &Genre, nfo_language: &str) -> String {
    if let Genre::Other(text) = genre {
        return text.clone();
    }
    let key = genre.name();
    let Some((_, en, de, fr)) = GENRES.iter().find(|(k, ..)| *k == key) else {
        return key.to_owned();
    };
    match primary_language(nfo_language).as_str() {
        "de" => de,
        "fr" => fr,
        _ => en,
    }
    .to_string()
}

/// Reads a genre written in English, German or French (the names [`genre_name`] writes) or by a
/// common alias (`Sci-Fi`). Other text is kept as a free-text genre. Blank text gives `None`.
pub fn parse_genre(text: &str) -> Option<Genre> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let lower = text.to_lowercase();
    if matches!(
        lower.as_str(),
        "sci-fi" | "scifi" | "sci fi" | "science-fiction"
    ) {
        return Genre::parse("SCIENCE_FICTION");
    }
    GENRES
        .iter()
        .find(|(_, en, de, fr)| [en, de, fr].iter().any(|n| n.to_lowercase() == lower))
        .and_then(|(key, ..)| Genre::parse(key))
        .or_else(|| Genre::parse(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genres_are_read_in_any_nfo_language() {
        assert_eq!(parse_genre("Komödie"), Some(Genre::Comedy));
        assert_eq!(parse_genre(" aventure "), Some(Genre::Adventure));
        assert_eq!(parse_genre("Sci-Fi"), Some(Genre::ScienceFiction));
        assert_eq!(parse_genre("Science-Fiction"), Some(Genre::ScienceFiction));
        assert_eq!(parse_genre("TV Movie"), Some(Genre::TvMovie));
        assert_eq!(
            parse_genre("Mockumentary"),
            Some(Genre::Other("Mockumentary".to_owned()))
        );
        assert_eq!(parse_genre("  "), None);
    }

    #[test]
    fn every_built_in_genre_has_names() {
        for genre in Genre::known() {
            assert!(
                GENRES.iter().any(|(k, ..)| *k == genre.name()),
                "no names for {genre}"
            );
        }
        assert_eq!(GENRES.len(), Genre::known().len());
    }

    #[test]
    fn genres_follow_the_nfo_language() {
        assert_eq!(genre_name(&Genre::Comedy, "en"), "Comedy");
        assert_eq!(genre_name(&Genre::Comedy, "de"), "Komödie");
        assert_eq!(genre_name(&Genre::Crime, "de"), "Krimi");
        assert_eq!(genre_name(&Genre::Adventure, "fr"), "Aventure");
        assert_eq!(genre_name(&Genre::Family, "fr"), "Familial");
        assert_eq!(genre_name(&Genre::Family, "ja"), "Family");
        assert_eq!(
            genre_name(&Genre::Other("Mockumentary".into()), "de"),
            "Mockumentary"
        );
    }

    #[test]
    fn language_names_follow_the_nfo_language() {
        assert_eq!(language_name("en", "en"), "English");
        assert_eq!(language_name("de", "en"), "German");
        assert_eq!(language_name("de", "de"), "Deutsch");
        assert_eq!(language_name("fr", "de"), "Französisch");
        assert_eq!(language_name("en", "fr"), "anglais");
        assert_eq!(language_name("deu", "en"), "German");
        assert_eq!(language_name("Klingon", "en"), "Klingon");
    }

    #[test]
    fn stream_languages_are_three_letter_codes() {
        assert_eq!(stream_language_code("en"), "eng");
        assert_eq!(stream_language_code("de"), "deu");
        assert_eq!(stream_language_code("pt-BR"), "pob");
        assert_eq!(stream_language_code("pt-PT"), "por");
        assert_eq!(stream_language_code("pt"), "por");
        assert_eq!(stream_language_code("ger"), "deu");
        assert_eq!(stream_language_code("fil"), "fil");
        assert_eq!(stream_language_code(""), "");
        assert_eq!(stream_language_code("xx"), "xx");
    }
}

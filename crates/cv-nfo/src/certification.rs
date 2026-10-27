//! Certification text for `<mpaa>` and `<certification>`.
//!
//! An item stores a certification token such as `US_PG13` or `DE_FSK16`: the ISO 3166 country,
//! an underscore and the rating key. The NFO option "certification format" decides how the token
//! is written:
//!
//! | Style | `US_G` | `DE_FSK16` |
//! |---|---|---|
//! | `Short` | `G` | `FSK 16` |
//! | `ShortMpaa` | `Rated G` | `FSK 16` |
//! | `Medium` | `US: G` | `DE: FSK 16` |
//! | `MediumFull` | `United States: G` | `Germany: FSK 16` |
//! | `Large` | `US:G / US:Rated G` | `DE:FSK 16 / DE:FSK-16 / DE:FSK16 / DE:16 / DE:16+ / DE:ab 16` |
//! | `LargeFull` | `United States:G / United States:Rated G` | `Germany:FSK 16 / ...` |
//! | `Technical` | `US_G` | `DE_FSK16` |
//! | `Technical2` | `US-G` | `DE-FSK16` |
//!
//! The catalogue below lists the ratings of the common systems with their written forms. A token
//! that is not listed still works: its name is the rating key with underscores as spaces and its
//! only written form is that name.

/// How a certification is written in an NFO.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum CertificationStyle {
    /// `G`, `FSK 16`.
    #[default]
    Short,
    /// `Rated G` for US ratings, otherwise as `Short`.
    ShortMpaa,
    /// `US: G`.
    Medium,
    /// `United States: G`.
    MediumFull,
    /// `US:G / US:Rated G`.
    Large,
    /// `United States:G / United States:Rated G`.
    LargeFull,
    /// The token, `US_G`.
    Technical,
    /// The token with a dash, `US-G`.
    Technical2,
}

impl CertificationStyle {
    /// Parses the setting name (`SHORT`, `SHORT_MPAA`, `MEDIUM`, `MEDIUM_FULL`, `LARGE`,
    /// `LARGE_FULL`, `TECHNICAL`, `TECHNICAL2`), ignoring case.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_uppercase().as_str() {
            "SHORT" => Self::Short,
            "SHORT_MPAA" => Self::ShortMpaa,
            "MEDIUM" => Self::Medium,
            "MEDIUM_FULL" => Self::MediumFull,
            "LARGE" => Self::Large,
            "LARGE_FULL" => Self::LargeFull,
            "TECHNICAL" => Self::Technical,
            "TECHNICAL2" => Self::Technical2,
            _ => return None,
        })
    }
}

/// A rating of a system: its token key, written name and every other written form.
struct Rating {
    key: String,
    name: String,
    notations: Vec<String>,
}

fn rating(key: &str, name: &str, notations: &[&str]) -> Rating {
    Rating {
        key: key.to_owned(),
        name: name.to_owned(),
        notations: notations.iter().map(|s| (*s).to_owned()).collect(),
    }
}

/// The US (and Canadian) TV parental guidelines. The token keys have no separator inside the
/// rating (`US_TVPG`, `US_TV14`), as the providers and the fixtures spell them.
fn tv_ratings() -> Vec<Rating> {
    vec![
        rating("TVY", "TV-Y", &["TV-Y"]),
        rating("TVY7", "TV-Y7", &["TV-Y7"]),
        rating("TVG", "TV-G", &["TV-G"]),
        rating("TVPG", "TV-PG", &["TV-PG"]),
        rating("TV14", "TV-14", &["TV-14"]),
        rating("TVMA", "TV-MA", &["TV-MA"]),
    ]
}

/// The ratings of one country, or an empty list.
fn catalogue(country: &str) -> Vec<Rating> {
    match country {
        "US" => vec![
            rating("G", "G", &["G", "Rated G"]),
            rating("PG", "PG", &["PG", "Rated PG"]),
            rating("PG13", "PG-13", &["PG-13", "Rated PG-13"]),
            rating("R", "R", &["R", "Rated R"]),
            rating("NC17", "NC-17", &["NC-17", "Rated NC-17"]),
        ]
        .into_iter()
        .chain(tv_ratings())
        .collect(),
        "CA" => tv_ratings(),
        "DE" => [0u8, 6, 12, 16, 18]
            .into_iter()
            .map(|n| {
                let key = format!("FSK{n}");
                let forms = [
                    format!("FSK {n}"),
                    format!("FSK-{n}"),
                    format!("FSK{n}"),
                    n.to_string(),
                    format!("{n}+"),
                    format!("ab {n}"),
                ];
                Rating {
                    key,
                    name: format!("FSK {n}"),
                    notations: forms.to_vec(),
                }
            })
            .collect(),
        "GB" => vec![
            rating("U", "U", &["U"]),
            rating("PG", "PG", &["PG"]),
            rating("12", "12", &["12"]),
            rating("12A", "12A", &["12A"]),
            rating("15", "15", &["15"]),
            rating("18", "18", &["18"]),
            rating("R18", "R18", &["R18"]),
        ],
        "AT" => [0u8, 6, 10, 12, 14, 16, 18]
            .into_iter()
            .map(|n| {
                rating(
                    &format!("FSK{n}"),
                    &format!("FSK {n}"),
                    &[&format!("FSK {n}")],
                )
            })
            .collect(),
        "FR" => vec![
            rating("0", "U", &["U", "Tous publics"]),
            rating("10", "10", &["10", "-10"]),
            rating("12", "12", &["12", "-12"]),
            rating("16", "16", &["16", "-16"]),
            rating("18", "18", &["18", "-18"]),
        ],
        _ => Vec::new(),
    }
}

/// English names of the countries whose ratings are common.
const COUNTRIES: &[(&str, &str)] = &[
    ("AR", "Argentina"),
    ("AT", "Austria"),
    ("AU", "Australia"),
    ("BE", "Belgium"),
    ("BG", "Bulgaria"),
    ("BR", "Brazil"),
    ("CA", "Canada"),
    ("CH", "Switzerland"),
    ("CL", "Chile"),
    ("CN", "China"),
    ("CZ", "Czech Republic"),
    ("DE", "Germany"),
    ("DK", "Denmark"),
    ("EE", "Estonia"),
    ("ES", "Spain"),
    ("FI", "Finland"),
    ("FR", "France"),
    ("GB", "United Kingdom"),
    ("GR", "Greece"),
    ("HK", "Hong Kong"),
    ("HR", "Croatia"),
    ("HU", "Hungary"),
    ("IE", "Ireland"),
    ("IL", "Israel"),
    ("IN", "India"),
    ("IS", "Iceland"),
    ("IT", "Italy"),
    ("JP", "Japan"),
    ("KR", "South Korea"),
    ("LT", "Lithuania"),
    ("LV", "Latvia"),
    ("MX", "Mexico"),
    ("MY", "Malaysia"),
    ("NL", "Netherlands"),
    ("NO", "Norway"),
    ("NZ", "New Zealand"),
    ("PH", "Philippines"),
    ("PL", "Poland"),
    ("PT", "Portugal"),
    ("RO", "Romania"),
    ("RU", "Russia"),
    ("SE", "Sweden"),
    ("SG", "Singapore"),
    ("SI", "Slovenia"),
    ("SK", "Slovakia"),
    ("TH", "Thailand"),
    ("TR", "Turkey"),
    ("TW", "Taiwan"),
    ("UA", "Ukraine"),
    ("US", "United States"),
    ("ZA", "South Africa"),
];

fn country_name(code: &str) -> &str {
    COUNTRIES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or(code, |(_, name)| name)
}

/// A token split into its country and rating key. `NOT_RATED`, `UNRATED` and `UNKNOWN` have no
/// country.
fn split_token(token: &str) -> Option<(&str, &str)> {
    let (country, key) = token.split_once('_')?;
    (country.len() == 2 && country.bytes().all(|b| b.is_ascii_uppercase()) && !key.is_empty())
        .then_some((country, key))
}

/// Writes a certification token in a style. An empty or unknown token (`UNKNOWN`) gives an empty
/// string.
pub fn format_certification(token: &str, style: CertificationStyle) -> String {
    let token = token.trim();
    match token {
        "" | "UNKNOWN" => return String::new(),
        "NOT_RATED" => return plain("Not Rated", token, style),
        "UNRATED" => return plain("Unrated", token, style),
        _ => {}
    }
    let Some((country, key)) = split_token(token) else {
        return token.to_owned();
    };
    // Providers spell the TV ratings with a separator inside (`US_TV_MA`), the catalogue without
    // (`US_TVMA`): both name the same rating.
    let compact = key.replace('_', "");
    let entry = catalogue(country)
        .into_iter()
        .find(|r| r.key == key || r.key == compact)
        .unwrap_or_else(|| {
            let name = key.replace('_', " ");
            Rating {
                key: key.to_owned(),
                notations: vec![name.clone()],
                name,
            }
        });
    match style {
        CertificationStyle::Short => entry.name,
        CertificationStyle::ShortMpaa if country == "US" && !entry.name.starts_with("TV-") => {
            format!("Rated {}", entry.name)
        }
        CertificationStyle::ShortMpaa => entry.name,
        CertificationStyle::Medium => format!("{country}: {}", entry.name),
        CertificationStyle::MediumFull => format!("{}: {}", country_name(country), entry.name),
        CertificationStyle::Large => joined(country, &entry),
        CertificationStyle::LargeFull => joined(country_name(country), &entry),
        CertificationStyle::Technical => token.to_owned(),
        CertificationStyle::Technical2 => format!("{country}-{key}"),
    }
}

fn plain(name: &str, token: &str, style: CertificationStyle) -> String {
    match style {
        CertificationStyle::Technical | CertificationStyle::Technical2 => token.to_owned(),
        _ => name.to_owned(),
    }
}

fn joined(prefix: &str, entry: &Rating) -> String {
    entry
        .notations
        .iter()
        .map(|n| format!("{prefix}:{n}"))
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Countries whose rating written without a country prefix is recognised, in the order they are
/// tried.
const SEARCH_ORDER: &[&str] = &["US", "DE", "GB", "AT", "FR", "CA"];

fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric() || *c == '+')
        .flat_map(char::to_lowercase)
        .collect()
}

fn country_code(text: &str) -> Option<&'static str> {
    let text = text.trim();
    COUNTRIES
        .iter()
        .find(|(code, name)| code.eq_ignore_ascii_case(text) || name.eq_ignore_ascii_case(text))
        .map(|(code, _)| *code)
        .or_else(|| text.eq_ignore_ascii_case("UK").then_some("GB"))
}

fn find_in(country: &str, text: &str, allow_numbers: bool) -> Option<String> {
    let wanted = squash(text);
    if wanted.is_empty() || (!allow_numbers && wanted.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    catalogue(country)
        .into_iter()
        .find(|r| {
            squash(&r.name) == wanted
                || squash(&r.key) == wanted
                || r.notations.iter().any(|n| squash(n) == wanted)
        })
        .map(|r| format!("{country}_{}", r.key))
}

/// Reads certification text into a token (the reverse of [`format_certification`]). It accepts
/// every style: `PG-13`, `Rated PG-13`, `US: PG-13`, `United States:PG-13`, `US_PG13`, `US-PG13`,
/// `DE:FSK 16`, `FSK 16`, `TV-MA`, and for a list written in the `Large` style (`a / b`) the first
/// entry. A bare number is not guessed. Text that matches no known rating is kept as written
/// (with the country prefix as a token when it has one), so nothing is lost.
pub fn parse_certification(text: &str) -> String {
    let first = text.split(" / ").next().unwrap_or_default().trim();
    if first.is_empty() {
        return String::new();
    }
    let first = first.strip_prefix("Rated ").unwrap_or(first).trim();
    match squash(first).as_str() {
        "notrated" | "nr" => return "NOT_RATED".to_owned(),
        "unrated" => return "UNRATED".to_owned(),
        _ => {}
    }
    // A token: `US_PG13` or `US-PG13`.
    if let Some((country, key)) = first.split_once(['_', '-'])
        && country.len() == 2
        && country.bytes().all(|b| b.is_ascii_uppercase())
        && let Some(code) = country_code(country)
    {
        if let Some(token) = find_in(code, key, true) {
            return token;
        }
        if !key.contains(' ') && !catalogue(code).is_empty() {
            return format!("{code}_{key}");
        }
    }
    // A country prefix: `US: PG-13`, `United States:PG-13`.
    if let Some((prefix, rest)) = first.split_once(':')
        && let Some(code) = country_code(prefix)
    {
        let rest = rest.trim().strip_prefix("Rated ").unwrap_or(rest.trim());
        return find_in(code, rest, true)
            .unwrap_or_else(|| format!("{code}_{}", rest.replace([' ', '-'], "_")));
    }
    for country in SEARCH_ORDER {
        if let Some(token) = find_in(country, first, false) {
            return token;
        }
    }
    first.to_owned()
}

#[cfg(test)]
mod tests {
    use super::CertificationStyle::*;
    use super::*;

    #[test]
    fn us_styles_match_the_fixtures() {
        let f = |s| format_certification("US_G", s);
        assert_eq!(f(Short), "G");
        assert_eq!(f(ShortMpaa), "Rated G");
        assert_eq!(f(Medium), "US: G");
        assert_eq!(f(MediumFull), "United States: G");
        assert_eq!(f(Large), "US:G / US:Rated G");
        assert_eq!(f(LargeFull), "United States:G / United States:Rated G");
        assert_eq!(f(Technical), "US_G");
        assert_eq!(f(Technical2), "US-G");
    }

    #[test]
    fn tv_ratings_are_not_rated_prefixed() {
        let f = |s| format_certification("US_TVPG", s);
        assert_eq!(f(Short), "TV-PG");
        assert_eq!(f(ShortMpaa), "TV-PG");
        assert_eq!(f(MediumFull), "United States: TV-PG");
        assert_eq!(f(Technical2), "US-TVPG");
        assert_eq!(format_certification("US_TVMA", Short), "TV-MA");
        // The spelling the providers write, with a separator inside, reads the same.
        assert_eq!(format_certification("US_TV_MA", Short), "TV-MA");
        assert_eq!(format_certification("US_TV_14", Short), "TV-14");
        assert_eq!(format_certification("US_TV_MA", Technical), "US_TV_MA");
        assert_eq!(format_certification("CA_TV14", Medium), "CA: TV-14");
    }

    #[test]
    fn german_styles_match_the_fixtures() {
        let f = |s| format_certification("DE_FSK16", s);
        assert_eq!(f(Short), "FSK 16");
        assert_eq!(f(ShortMpaa), "FSK 16");
        assert_eq!(f(Medium), "DE: FSK 16");
        assert_eq!(f(MediumFull), "Germany: FSK 16");
        assert_eq!(
            f(Large),
            "DE:FSK 16 / DE:FSK-16 / DE:FSK16 / DE:16 / DE:16+ / DE:ab 16"
        );
        assert_eq!(f(Technical), "DE_FSK16");
        assert_eq!(f(Technical2), "DE-FSK16");
    }

    #[test]
    fn unlisted_ratings_still_format() {
        assert_eq!(format_certification("SE_BTL", Short), "BTL");
        assert_eq!(format_certification("SE_BTL", MediumFull), "Sweden: BTL");
        assert_eq!(format_certification("XX_AB_CD", Medium), "XX: AB CD");
        assert_eq!(format_certification("XX_AB_CD", Technical2), "XX-AB_CD");
    }

    #[test]
    fn special_tokens_have_no_country() {
        assert_eq!(format_certification("NOT_RATED", Medium), "Not Rated");
        assert_eq!(format_certification("UNRATED", Short), "Unrated");
        assert_eq!(format_certification("UNKNOWN", Short), "");
        assert_eq!(format_certification("", Technical), "");
        assert_eq!(format_certification("NOT_RATED", Technical), "NOT_RATED");
    }

    #[test]
    fn style_names_parse() {
        assert_eq!(CertificationStyle::parse("short_mpaa"), Some(ShortMpaa));
        assert_eq!(CertificationStyle::parse("TECHNICAL2"), Some(Technical2));
        assert_eq!(CertificationStyle::parse("nope"), None);
    }

    #[test]
    fn every_style_reads_back_to_the_token() {
        let tokens = [
            "US_G", "US_PG13", "US_NC17", "US_TVMA", "CA_TV14", "DE_FSK16", "DE_FSK0", "GB_12A",
            "FR_16",
        ];
        let styles = [
            Short, ShortMpaa, Medium, MediumFull, Large, LargeFull, Technical, Technical2,
        ];
        for token in tokens {
            for style in styles {
                // A bare number or a rating shared by several countries is not tied to a country,
                // so the short styles lose it.
                if matches!(token, "FR_16" | "CA_TV14") && matches!(style, Short | ShortMpaa) {
                    continue;
                }
                let written = format_certification(token, style);
                assert_eq!(parse_certification(&written), token, "{token} as {style:?}");
            }
        }
    }

    #[test]
    fn plain_and_unknown_text() {
        assert_eq!(parse_certification("PG-13"), "US_PG13");
        assert_eq!(parse_certification("Rated R"), "US_R");
        assert_eq!(parse_certification("TV-MA"), "US_TVMA");
        assert_eq!(parse_certification("FSK 16"), "DE_FSK16");
        assert_eq!(parse_certification("Not Rated"), "NOT_RATED");
        assert_eq!(parse_certification("Unrated"), "UNRATED");
        assert_eq!(parse_certification("SE: Btl"), "SE_Btl");
        assert_eq!(parse_certification("16"), "16");
        assert_eq!(parse_certification("  "), "");
        assert_eq!(parse_certification("Tous publics"), "FR_0");
    }
}

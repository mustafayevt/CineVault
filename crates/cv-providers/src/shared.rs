//! Small conversion helpers the metadata providers share: dates, years, IDs, HTML text, and the
//! country and language codes CineVault stores.

use cv_domain::{CalendarDate, ProviderIds, id_key};

/// Parses `yyyy-MM-dd`, ignoring a time part. Blank and invalid dates are `None`.
pub(crate) fn parse_date(text: &str) -> Option<CalendarDate> {
    text.trim().get(..10)?.parse().ok()
}

/// Parses `15 Dec 1995`.
pub(crate) fn parse_day_month_year(text: &str) -> Option<CalendarDate> {
    let mut parts = text.split_whitespace();
    let day: u8 = parts.next()?.parse().ok()?;
    let month = match parts.next()?.to_ascii_lowercase().get(..3)? {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    };
    let year: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    CalendarDate::new(year, month, day)
}

/// The year of a date, `0` when there is none.
pub(crate) fn year_of(date: Option<CalendarDate>) -> u16 {
    date.and_then(|d| u16::try_from(d.year()).ok()).unwrap_or(0)
}

/// The four-digit year a text starts with (`1995`, `2008–2013`, `1999-04-30`), `0` otherwise.
pub(crate) fn year_prefix(text: &str) -> u16 {
    let text = text.trim();
    let digits: String = text.chars().take(4).collect();
    if digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit()) {
        digits.parse().unwrap_or(0)
    } else {
        0
    }
}

/// Stores an ID unless it is blank or `0`.
pub(crate) fn set_id(ids: &mut ProviderIds, key: &str, value: impl AsRef<str>) {
    let value = value.as_ref().trim();
    if !value.is_empty() && value != "0" {
        ids.set(key, value);
    }
}

/// An IMDb title ID (`tt` and at least six digits) normalised to lower case.
pub(crate) fn imdb_title_id(text: &str) -> Option<String> {
    let id = text.trim().to_ascii_lowercase();
    id.strip_prefix("tt")
        .is_some_and(|d| d.len() >= 6 && d.bytes().all(|b| b.is_ascii_digit()))
        .then_some(id)
}

/// The IMDb title ID in `ids`.
pub(crate) fn imdb_id_of(ids: &ProviderIds) -> Option<String> {
    ids.get(id_key::IMDB).and_then(imdb_title_id)
}

/// Seconds until the next UTC midnight (daily quotas reset then); at least one.
pub(crate) fn seconds_to_utc_midnight() -> u32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    u32::try_from(86_400 - now % 86_400)
        .unwrap_or(86_400)
        .max(1)
}

/// Splits `A, B, C` into trimmed, non-empty, distinct entries.
pub(crate) fn split_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if !part.is_empty() && !out.iter().any(|p| p == part) {
            out.push(part.to_owned());
        }
    }
    out
}

/// Plain text from the HTML fragments providers send for plots: tags removed, entities decoded,
/// paragraphs separated by a blank line and line breaks kept.
pub(crate) fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(end) = after.find('>') else {
            // Not a tag: keep the text.
            out.push_str(after);
            rest = "";
            break;
        };
        let tag = after[1..end].trim().trim_end_matches('/').trim();
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let closing = tag.starts_with('/');
        match name.as_str() {
            "br" => out.push('\n'),
            "p" if closing => out.push_str("\n\n"),
            "li" if closing => out.push('\n'),
            _ => {}
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    let decoded = decode_entities(&out);
    let mut lines: Vec<&str> = decoded.lines().map(str::trim).collect();
    // Collapse runs of blank lines to one.
    lines.dedup_by(|a, b| a.is_empty() && b.is_empty());
    lines.join("\n").trim().to_owned()
}

pub(crate) fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        let decoded = after.find(';').filter(|end| *end <= 10).and_then(|end| {
            let entity = &after[1..end];
            let ch = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                "ndash" => Some('–'),
                "mdash" => Some('—'),
                "hellip" => Some('…'),
                "rsquo" | "lsquo" => Some('\''),
                "rdquo" | "ldquo" => Some('"'),
                _ => entity.strip_prefix('#').and_then(|number| {
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse().ok()?,
                    };
                    char::from_u32(code)
                }),
            };
            ch.map(|c| (c, end))
        });
        match decoded {
            Some((ch, end)) => {
                out.push(ch);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

const COUNTRIES: &[(&str, &str)] = &[
    ("argentina", "AR"),
    ("australia", "AU"),
    ("austria", "AT"),
    ("belgium", "BE"),
    ("brazil", "BR"),
    ("bulgaria", "BG"),
    ("canada", "CA"),
    ("chile", "CL"),
    ("china", "CN"),
    ("colombia", "CO"),
    ("croatia", "HR"),
    ("czech republic", "CZ"),
    ("czechia", "CZ"),
    ("denmark", "DK"),
    ("egypt", "EG"),
    ("finland", "FI"),
    ("france", "FR"),
    ("germany", "DE"),
    ("west germany", "DE"),
    ("greece", "GR"),
    ("hong kong", "HK"),
    ("hungary", "HU"),
    ("iceland", "IS"),
    ("india", "IN"),
    ("indonesia", "ID"),
    ("iran", "IR"),
    ("ireland", "IE"),
    ("israel", "IL"),
    ("italy", "IT"),
    ("japan", "JP"),
    ("luxembourg", "LU"),
    ("malaysia", "MY"),
    ("mexico", "MX"),
    ("netherlands", "NL"),
    ("new zealand", "NZ"),
    ("nigeria", "NG"),
    ("norway", "NO"),
    ("pakistan", "PK"),
    ("peru", "PE"),
    ("philippines", "PH"),
    ("poland", "PL"),
    ("portugal", "PT"),
    ("romania", "RO"),
    ("russia", "RU"),
    ("soviet union", "SU"),
    ("saudi arabia", "SA"),
    ("serbia", "RS"),
    ("singapore", "SG"),
    ("slovakia", "SK"),
    ("slovenia", "SI"),
    ("south africa", "ZA"),
    ("south korea", "KR"),
    ("spain", "ES"),
    ("sweden", "SE"),
    ("switzerland", "CH"),
    ("taiwan", "TW"),
    ("thailand", "TH"),
    ("turkey", "TR"),
    ("türkiye", "TR"),
    ("ukraine", "UA"),
    ("united arab emirates", "AE"),
    ("united kingdom", "GB"),
    ("uk", "GB"),
    ("united states", "US"),
    ("usa", "US"),
    ("venezuela", "VE"),
    ("vietnam", "VN"),
];

const LANGUAGES: &[(&str, &str)] = &[
    ("arabic", "ar"),
    ("bengali", "bn"),
    ("bulgarian", "bg"),
    ("cantonese", "yue"),
    ("catalan", "ca"),
    ("chinese", "zh"),
    ("croatian", "hr"),
    ("czech", "cs"),
    ("danish", "da"),
    ("dutch", "nl"),
    ("english", "en"),
    ("filipino", "fil"),
    ("finnish", "fi"),
    ("french", "fr"),
    ("german", "de"),
    ("greek", "el"),
    ("hebrew", "he"),
    ("hindi", "hi"),
    ("hungarian", "hu"),
    ("icelandic", "is"),
    ("indonesian", "id"),
    ("italian", "it"),
    ("japanese", "ja"),
    ("korean", "ko"),
    ("latin", "la"),
    ("mandarin", "cmn"),
    ("norwegian", "no"),
    ("persian", "fa"),
    ("polish", "pl"),
    ("portuguese", "pt"),
    ("romanian", "ro"),
    ("russian", "ru"),
    ("serbian", "sr"),
    ("slovak", "sk"),
    ("slovenian", "sl"),
    ("spanish", "es"),
    ("swedish", "sv"),
    ("tamil", "ta"),
    ("thai", "th"),
    ("turkish", "tr"),
    ("ukrainian", "uk"),
    ("urdu", "ur"),
    ("vietnamese", "vi"),
];

fn lookup(table: &[(&str, &str)], name: &str) -> Option<String> {
    let name = name.trim();
    table
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, code)| (*code).to_owned())
}

/// Country names to the two-letter codes CineVault stores. A two-letter code stays (upper
/// case), a name outside the table stays as written, so nothing a provider says is lost.
pub(crate) fn country_codes(names: &[String]) -> Vec<String> {
    distinct(names.iter().map(|name| {
        if name.trim().len() == 2 {
            name.trim().to_ascii_uppercase()
        } else {
            lookup(COUNTRIES, name).unwrap_or_else(|| name.trim().to_owned())
        }
    }))
}

/// Language names to the ISO codes CineVault stores. Codes stay (lower case); a name outside
/// the table stays as written.
pub(crate) fn language_codes(names: &[String]) -> Vec<String> {
    distinct(names.iter().map(|name| {
        let name = name.trim();
        if (2..=3).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphabetic()) {
            name.to_ascii_lowercase()
        } else {
            lookup(LANGUAGES, name).unwrap_or_else(|| name.to_owned())
        }
    }))
}

/// The original language of a production as the lower-case ISO 639-1 code CineVault stores
/// (`en`, `fr`, `ja`): a two- or three-letter provider code goes through the ISO 639 tables, a
/// code the tables do not know stays as written, and "no linguistic content" codes (`xx`, `zxx`,
/// `und`, `mul`) give nothing.
pub(crate) fn original_language_code(code: &str) -> String {
    let code = cv_domain::language_code(code);
    match code.as_str() {
        "xx" | "zxx" | "und" | "mul" | "mis" => String::new(),
        _ => code,
    }
}

fn distinct(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values.filter(|v| !v.is_empty()) {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_years() {
        assert_eq!(
            parse_date("2008-01-20T02:00:00+00:00"),
            CalendarDate::new(2008, 1, 20)
        );
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("2008"), None);
        assert_eq!(
            parse_day_month_year("15 Dec 1995"),
            CalendarDate::new(1995, 12, 15)
        );
        assert_eq!(parse_day_month_year("N/A"), None);
        assert_eq!(parse_day_month_year("31 Feb 1995"), None);
        assert_eq!(year_prefix("2008–2013"), 2008);
        assert_eq!(year_prefix("1999-04-30"), 1999);
        assert_eq!(year_prefix("n/a"), 0);
        assert_eq!(year_prefix(""), 0);
    }

    #[test]
    fn imdb_ids_need_tt_and_digits() {
        assert_eq!(imdb_title_id(" TT0113277 ").as_deref(), Some("tt0113277"));
        assert_eq!(imdb_title_id("tt123"), None);
        assert_eq!(imdb_title_id("nm0000093"), None);
        let mut ids = ProviderIds::new();
        ids.set(id_key::IMDB, "tt0113277");
        assert_eq!(imdb_id_of(&ids).as_deref(), Some("tt0113277"));
        assert_eq!(imdb_id_of(&ProviderIds::new()), None);
    }

    #[test]
    fn original_languages_are_two_letter_codes() {
        assert_eq!(original_language_code("fr"), "fr");
        assert_eq!(original_language_code("fra"), "fr");
        assert_eq!(original_language_code("JPN"), "ja");
        assert_eq!(original_language_code("pt-BR"), "pt");
        assert_eq!(original_language_code("hi"), "hi");
        // Codes the tables do not know stay; "no language" gives nothing.
        assert_eq!(original_language_code("yue"), "yue");
        assert_eq!(original_language_code("xx"), "");
        assert_eq!(original_language_code("zxx"), "");
        assert_eq!(original_language_code(""), "");
    }

    #[test]
    fn lists_split_and_dedupe() {
        assert_eq!(split_list("A, B ,,A, C"), vec!["A", "B", "C"]);
        assert!(split_list("  ").is_empty());
    }

    #[test]
    fn html_becomes_text() {
        assert_eq!(
            html_to_text(
                "<p><b>Heat</b> &amp; more &#8211; it&#39;s &quot;good&quot;.</p><p>Second<br>line</p>"
            ),
            "Heat & more – it's \"good\".\n\nSecond\nline"
        );
        assert_eq!(html_to_text("plain & simple"), "plain & simple");
        assert_eq!(html_to_text("a < b"), "a < b");
        assert_eq!(html_to_text(""), "");
    }

    #[test]
    fn names_become_codes() {
        let names = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            country_codes(&names(&["United States", "Italy", "us", "Narnia"])),
            vec!["US", "IT", "Narnia"]
        );
        assert_eq!(
            language_codes(&names(&["English", "Turkish", "fr", "Klingon"])),
            vec!["en", "tr", "fr", "Klingon"]
        );
    }

    #[test]
    fn midnight_is_within_a_day() {
        let seconds = seconds_to_utc_midnight();
        assert!((1..=86_400).contains(&seconds));
    }
}

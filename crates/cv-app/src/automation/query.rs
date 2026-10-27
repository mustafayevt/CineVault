//! The quick-search language, for the CLI.
//!
//! The library pages parse quick search in the UI (`ui/src/features/search`); this is the same
//! grammar in Rust, so `cinevault movie --query "hdr:dv year:2010..2019 -watched heat"` selects
//! exactly what typing it into the search box shows.
//!
//! ```text
//! hdr:dv year:2010..2019 -watched actor:"mel gibson" heat
//! ```
//!
//! A term is a word, a `field:value` pair, or either with a leading `-` (not). Terms that name a
//! field become [`Chip`]s; every other word is title text. A known field with a value that makes
//! no sense (`year:abc`, `hdr:xyz`) is *rejected*: the UI keeps it in the box, but a batch must
//! never run on a selection it misread, so [`ParsedQuery::rejected`] lets the caller refuse.
//!
//! - **Numbers** (`year`, `runtime`, `rating` ...): `2015`, `2010..2019`, `2010..`, `..2000`,
//!   `>2010`, `>=2010`, `<2000`, `<=2000`.
//! - **Lists** (`hdr`, `genre`, `actor` ...): one value or several separated by commas, any of
//!   which matches. A value with spaces goes in double quotes.
//! - **Yes or no** (`watched`, `new`, `review`, `locked`): `-watched`, `is:new`, `watched:no`. A
//!   bare `watched` stays a title word.

use cv_db::repo::filter::{Filter, IntRange, ItemFlag, MatchMode};
use cv_domain::{CreditKind, RatingPreference};

/// The field a chip tests; the names the library queries of the app use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipField {
    Watched,
    New,
    Review,
    Locked,
    Year,
    Runtime,
    Top250,
    Rating,
    ImdbRating,
    TmdbRating,
    RottenTomatoesRating,
    MetacriticRating,
    UserRating,
    Resolution,
    Hdr,
    Codec,
    AudioLanguage,
    SubtitleLanguage,
    Genre,
    Actor,
    Director,
    Writer,
    Certification,
    Edition,
    Source,
}

/// One filter chip: a field, its value(s) and whether it is negated.
#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    pub field: ChipField,
    pub negated: bool,
    /// The values of a list field; any one matches. Empty for flags and ranges.
    pub values: Vec<String>,
    /// Lower bound of a range field, inclusive; `None` is open.
    pub min: Option<f64>,
    /// Upper bound of a range field, inclusive; `None` is open.
    pub max: Option<f64>,
}

/// What the quick-search text means.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedQuery {
    /// The recognised terms, in order.
    pub chips: Vec<Chip>,
    /// Title words joined by spaces; what remains for the text search.
    pub text: String,
    /// Terms of a known field whose value was refused, as typed.
    pub rejected: Vec<String>,
    /// The input ends inside an unclosed quote.
    pub open_quote: bool,
}

impl ParsedQuery {
    /// The filters the query stands for: the title search, then one per chip. A chip that
    /// restricts nothing (a range without bounds) is left out.
    #[must_use]
    pub fn filters(&self, preference: &RatingPreference) -> Vec<Filter> {
        let mut filters = Vec::new();
        if !self.text.trim().is_empty() {
            filters.push(Filter::Search {
                text: self.text.clone(),
            });
        }
        filters.extend(self.chips.iter().filter_map(|c| c.to_filter(preference)));
        filters
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Flag,
    Integer,
    Decimal,
    List,
}

struct FieldDef {
    id: ChipField,
    names: &'static [&'static str],
    kind: Kind,
    bounds: Option<(f64, f64)>,
    normalize: Option<fn(&str) -> Option<String>>,
}

const fn flag(id: ChipField, names: &'static [&'static str]) -> FieldDef {
    FieldDef {
        id,
        names,
        kind: Kind::Flag,
        bounds: None,
        normalize: None,
    }
}

const fn number(
    id: ChipField,
    names: &'static [&'static str],
    kind: Kind,
    min: f64,
    max: f64,
) -> FieldDef {
    FieldDef {
        id,
        names,
        kind,
        bounds: Some((min, max)),
        normalize: None,
    }
}

const fn list(
    id: ChipField,
    names: &'static [&'static str],
    normalize: fn(&str) -> Option<String>,
) -> FieldDef {
    FieldDef {
        id,
        names,
        kind: Kind::List,
        bounds: None,
        normalize: Some(normalize),
    }
}

fn not_blank(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn normalize_hdr(text: &str) -> Option<String> {
    let key: String = text
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .collect();
    let name = match key.as_str() {
        "dv" | "dovi" | "dolbyvision" | "dolby" => "Dolby Vision",
        "hdr10" => "HDR10",
        "hdr10+" | "hdr10plus" => "HDR10+",
        "hlg" => "HLG",
        _ => return None,
    };
    Some(name.to_owned())
}

fn normalize_resolution(text: &str) -> Option<String> {
    let name = match text.to_lowercase().trim() {
        "4k" | "uhd" | "2160" | "2160p" => "2160p",
        "fhd" | "1080" | "1080p" => "1080p",
        "hd" | "720" | "720p" => "720p",
        "576" | "576p" => "576p",
        "sd" | "480" | "480p" => "480p",
        _ => return None,
    };
    Some(name.to_owned())
}

const FIELDS: &[FieldDef] = &[
    flag(ChipField::Watched, &["watched", "seen"]),
    flag(ChipField::New, &["new"]),
    flag(ChipField::Review, &["review", "needsreview"]),
    flag(ChipField::Locked, &["locked"]),
    number(ChipField::Year, &["year"], Kind::Integer, 1000.0, 9999.0),
    number(
        ChipField::Runtime,
        &["runtime", "minutes"],
        Kind::Integer,
        0.0,
        100_000.0,
    ),
    number(
        ChipField::Top250,
        &["top250", "top"],
        Kind::Integer,
        1.0,
        250.0,
    ),
    number(ChipField::Rating, &["rating"], Kind::Decimal, 0.0, 10.0),
    number(ChipField::ImdbRating, &["imdb"], Kind::Decimal, 0.0, 10.0),
    number(ChipField::TmdbRating, &["tmdb"], Kind::Decimal, 0.0, 10.0),
    number(
        ChipField::RottenTomatoesRating,
        &["rottentomatoes", "rt", "tomatoes"],
        Kind::Decimal,
        0.0,
        10.0,
    ),
    number(
        ChipField::MetacriticRating,
        &["metacritic", "mc"],
        Kind::Decimal,
        0.0,
        10.0,
    ),
    number(
        ChipField::UserRating,
        &["myrating", "userrating"],
        Kind::Decimal,
        0.0,
        10.0,
    ),
    list(
        ChipField::Resolution,
        &["resolution", "res"],
        normalize_resolution,
    ),
    list(ChipField::Hdr, &["hdr"], normalize_hdr),
    list(ChipField::Codec, &["codec", "videocodec"], not_blank),
    list(
        ChipField::AudioLanguage,
        &["audio", "audiolanguage"],
        not_blank,
    ),
    list(
        ChipField::SubtitleLanguage,
        &["subs", "subtitles", "subtitle", "sub"],
        not_blank,
    ),
    list(ChipField::Genre, &["genre", "genres"], not_blank),
    list(ChipField::Actor, &["actor", "actors", "cast"], not_blank),
    list(ChipField::Director, &["director", "directors"], not_blank),
    list(ChipField::Writer, &["writer", "writers"], not_blank),
    list(
        ChipField::Certification,
        &["certification", "cert", "rated"],
        not_blank,
    ),
    list(ChipField::Edition, &["edition"], not_blank),
    list(ChipField::Source, &["source", "mediasource"], not_blank),
];

fn field_by_name(name: &str) -> Option<&'static FieldDef> {
    let name = name.to_lowercase();
    FIELDS.iter().find(|def| def.names.contains(&name.as_str()))
}

/// Splits on whitespace outside double quotes.
fn tokenize(input: &str) -> (Vec<String>, bool) {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    for ch in input.chars() {
        if ch == '"' {
            in_quote = !in_quote;
        }
        if !in_quote && ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    (tokens, in_quote)
}

/// Removes the quotes around a value; an unclosed opening quote is tolerated.
fn unquote(value: &str) -> &str {
    let Some(rest) = value.strip_prefix('"') else {
        return value;
    };
    match rest.find('"') {
        Some(close) => &rest[..close],
        None => rest,
    }
}

/// Splits a list value on commas outside quotes and removes the quotes.
fn split_list(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    for ch in value.chars() {
        match ch {
            '"' => in_quote = !in_quote,
            ',' if !in_quote => parts.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    parts.push(current);
    parts
        .into_iter()
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .collect()
}

fn to_number(text: &str) -> Option<f64> {
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (text, None),
    };
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if !digits(whole) || fraction.is_some_and(|f| !digits(f)) {
        return None;
    }
    text.parse().ok()
}

fn one_decimal(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[derive(Clone, Copy)]
struct Range {
    min: Option<f64>,
    max: Option<f64>,
}

/// Reads `5`, `5..9`, `5..`, `..9`, `>5`, `>=5`, `<9`, `<=9`.
fn parse_range(text: &str, def: &FieldDef) -> Option<Range> {
    let step = if def.kind == Kind::Decimal { 0.1 } else { 1.0 };
    if let Some(rest) = text.strip_prefix(">=") {
        return Some(Range {
            min: Some(to_number(rest.trim())?),
            max: None,
        });
    }
    if let Some(rest) = text.strip_prefix("<=") {
        return Some(Range {
            min: None,
            max: Some(to_number(rest.trim())?),
        });
    }
    if let Some(rest) = text.strip_prefix('>') {
        return Some(Range {
            min: Some(one_decimal(to_number(rest.trim())? + step)),
            max: None,
        });
    }
    if let Some(rest) = text.strip_prefix('<') {
        return Some(Range {
            min: None,
            max: Some(one_decimal(to_number(rest.trim())? - step)),
        });
    }
    if let Some((left, right)) = text.split_once("..") {
        let (left, right) = (left.trim(), right.trim());
        if left.is_empty() && right.is_empty() {
            return None;
        }
        let min = if left.is_empty() {
            None
        } else {
            Some(to_number(left)?)
        };
        let max = if right.is_empty() {
            None
        } else {
            Some(to_number(right)?)
        };
        return Some(Range { min, max });
    }
    let n = to_number(text)?;
    Some(Range {
        min: Some(n),
        max: Some(n),
    })
}

fn range_is_valid(range: Range, def: &FieldDef) -> bool {
    if let (Some(min), Some(max)) = (range.min, range.max)
        && min > max
    {
        return false;
    }
    if let Some((low, high)) = def.bounds {
        for value in [range.min, range.max].into_iter().flatten() {
            if value < low || value > high {
                return false;
            }
        }
    }
    if def.kind == Kind::Integer {
        return [range.min, range.max]
            .into_iter()
            .flatten()
            .all(|v| v.fract() == 0.0);
    }
    true
}

fn chip(def: &FieldDef, negated: bool) -> Chip {
    Chip {
        field: def.id,
        negated,
        values: Vec::new(),
        min: None,
        max: None,
    }
}

/// The chip for `field:value`, or `None` when the value is not valid for the field.
fn chip_from_value(def: &FieldDef, negated: bool, value: &str) -> Option<Chip> {
    match def.kind {
        Kind::Flag => {
            let word = unquote(value).to_lowercase();
            match word.as_str() {
                "yes" | "true" | "1" | "y" => Some(chip(def, negated)),
                "no" | "false" | "0" | "n" => Some(chip(def, !negated)),
                _ => None,
            }
        }
        Kind::Integer | Kind::Decimal => {
            let range = parse_range(unquote(value).trim(), def)?;
            if !range_is_valid(range, def) {
                return None;
            }
            Some(Chip {
                min: range.min,
                max: range.max,
                ..chip(def, negated)
            })
        }
        Kind::List => {
            let mut values: Vec<String> = Vec::new();
            for part in split_list(value) {
                let normal = match def.normalize {
                    Some(normalize) => normalize(&part)?,
                    None => part,
                };
                if !values.contains(&normal) {
                    values.push(normal);
                }
            }
            if values.is_empty() {
                return None;
            }
            Some(Chip {
                values,
                ..chip(def, negated)
            })
        }
    }
}

enum Term {
    Chip(Chip),
    Word(String),
    Rejected(String),
}

fn classify(raw: &str) -> Term {
    let free = || Term::Word(unquote(raw).to_owned());
    let mut body = raw;
    let mut negated = false;
    if body.chars().count() > 1
        && let Some(rest) = body.strip_prefix('-')
    {
        negated = true;
        body = rest;
    }
    let colon = body.find(':');
    let quote = body.find('"');
    let Some(colon) = colon.filter(|c| quote.is_none_or(|q| q > *c)) else {
        // `-watched`: a bare yes/no word is a chip only with its minus.
        if negated
            && let Some(def) = field_by_name(body)
            && def.kind == Kind::Flag
        {
            return Term::Chip(chip(def, true));
        }
        return free();
    };
    let name = body[..colon].to_lowercase();
    let value = &body[colon + 1..];
    if name == "is" {
        return match field_by_name(unquote(value)) {
            Some(def) if def.kind == Kind::Flag => Term::Chip(chip(def, negated)),
            _ => Term::Rejected(raw.to_owned()),
        };
    }
    let Some(def) = field_by_name(&name) else {
        return free();
    };
    let parsed = if value.is_empty() {
        None
    } else {
        chip_from_value(def, negated, value)
    };
    match parsed {
        Some(chip) => Term::Chip(chip),
        None => Term::Rejected(raw.to_owned()),
    }
}

/// Understands the quick-search text.
#[must_use]
pub fn parse_quick_search(input: &str) -> ParsedQuery {
    let (tokens, open_quote) = tokenize(input);
    let mut parsed = ParsedQuery {
        open_quote,
        ..ParsedQuery::default()
    };
    let mut words: Vec<String> = Vec::new();
    for token in &tokens {
        match classify(token) {
            Term::Chip(chip) => parsed.chips.push(chip),
            Term::Word(word) => {
                if !word.is_empty() {
                    words.push(word);
                }
            }
            Term::Rejected(raw) => parsed.rejected.push(raw),
        }
    }
    parsed.text = words.join(" ");
    parsed
}

// ---- chips to filters ----------------------------------------------------------------------

/// The frame heights behind the resolution labels on a card.
fn resolution_band(label: &str) -> Option<IntRange> {
    match label.trim().to_ascii_lowercase().as_str() {
        "2160p" => Some(IntRange::at_least(1500)),
        "1080p" => Some(IntRange::between(760, 1499)),
        "720p" => Some(IntRange::between(600, 759)),
        "576p" => Some(IntRange::between(500, 599)),
        "480p" => Some(IntRange::between(1, 499)),
        _ => None,
    }
}

/// A whole-number range from typed bounds: a fractional bound rounds inwards.
#[allow(clippy::cast_possible_truncation)]
fn whole_range(min: Option<f64>, max: Option<f64>) -> IntRange {
    IntRange {
        min: min.map(|v| v.ceil() as i64),
        max: max.map(|v| v.floor() as i64),
    }
}

fn any_of(mut filters: Vec<Filter>) -> Option<Filter> {
    match filters.len() {
        0 => None,
        1 => filters.pop(),
        _ => Some(Filter::or(filters)),
    }
}

impl Chip {
    fn has_bound(&self) -> bool {
        self.min.is_some() || self.max.is_some()
    }

    fn list(&self, build: impl FnOnce(Vec<String>) -> Filter) -> Option<Filter> {
        let values: Vec<String> = self
            .values
            .iter()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .collect();
        (!values.is_empty()).then(|| build(values))
    }

    fn person(&self, kind: CreditKind) -> Option<Filter> {
        self.list(|names| {
            let people = names
                .into_iter()
                .map(|name| Filter::Person {
                    name,
                    kind: Some(kind),
                })
                .collect();
            any_of(people).unwrap_or_default()
        })
    }

    fn rating(&self, source: &str) -> Option<Filter> {
        self.has_bound().then(|| Filter::Rating {
            source: source.to_owned(),
            min: self.min,
            max: self.max,
        })
    }

    fn range(&self, build: impl FnOnce(IntRange) -> Filter) -> Option<Filter> {
        self.has_bound()
            .then(|| build(whole_range(self.min, self.max)))
    }

    /// The condition of the chip, or `None` when it restricts nothing.
    #[must_use]
    pub fn to_filter(&self, preference: &RatingPreference) -> Option<Filter> {
        use ChipField as F;
        let flag = match self.field {
            F::Watched => Some(ItemFlag::Watched),
            F::New => Some(ItemFlag::New),
            F::Review => Some(ItemFlag::NeedsReview),
            F::Locked => Some(ItemFlag::Locked),
            _ => None,
        };
        if let Some(flag) = flag {
            // A negated flag is the flag turned off, so items without a value still match.
            return Some(Filter::Flag {
                flag,
                value: !self.negated,
            });
        }
        let positive = match self.field {
            F::Year => self.range(|range| Filter::Year { range }),
            F::Runtime => self.range(|range| Filter::RuntimeMinutes { range }),
            F::Top250 => self.range(|range| Filter::Top250 { range }),
            F::Rating => self.has_bound().then(|| Filter::PreferredRating {
                preference: preference.clone(),
                min: self.min,
                max: self.max,
            }),
            F::ImdbRating => self.rating("imdb"),
            F::TmdbRating => self.rating("tmdb"),
            F::RottenTomatoesRating => self.rating("tomatometerallcritics"),
            F::MetacriticRating => self.rating("metacritic"),
            F::UserRating => self.rating("user"),
            F::Resolution => any_of(
                self.values
                    .iter()
                    .filter_map(|label| resolution_band(label))
                    .map(|range| Filter::VideoHeight { range })
                    .collect(),
            ),
            F::Hdr => self.list(|values| Filter::Hdr {
                values,
                mode: MatchMode::Any,
            }),
            F::Codec => self.list(|values| Filter::VideoCodec { values }),
            F::AudioLanguage => self.list(|values| Filter::AudioLanguage { values }),
            F::SubtitleLanguage => self.list(|values| Filter::SubtitleLanguage { values }),
            F::Genre => self.list(|names| Filter::Genre {
                names,
                mode: MatchMode::Any,
            }),
            F::Actor => self.person(CreditKind::Actor),
            F::Director => self.person(CreditKind::Director),
            F::Writer => self.person(CreditKind::Writer),
            F::Certification => self.list(|values| Filter::Certification { values }),
            F::Edition => self.list(|values| Filter::Edition { values }),
            F::Source => self.list(|values| Filter::MediaSource { values }),
            F::Watched | F::New | F::Review | F::Locked => None,
        }?;
        Some(if self.negated {
            Filter::not(positive)
        } else {
            positive
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chips(input: &str) -> Vec<Chip> {
        parse_quick_search(input).chips
    }

    #[test]
    fn the_documented_example_splits_into_chips_and_title_text() {
        let parsed =
            parse_quick_search(r#"hdr:dv year:2010..2019 -watched actor:"mel gibson" heat"#);
        assert_eq!(parsed.text, "heat");
        assert!(parsed.rejected.is_empty());
        assert_eq!(parsed.chips.len(), 4);
        assert_eq!(parsed.chips[0].field, ChipField::Hdr);
        assert_eq!(parsed.chips[0].values, ["Dolby Vision"]);
        assert_eq!(parsed.chips[1].min, Some(2010.0));
        assert_eq!(parsed.chips[1].max, Some(2019.0));
        assert!(parsed.chips[2].negated && parsed.chips[2].field == ChipField::Watched);
        assert_eq!(parsed.chips[3].values, ["mel gibson"]);
    }

    #[test]
    fn number_forms_become_ranges() {
        let range = |text: &str| {
            let chip = &chips(text)[0];
            (chip.min, chip.max)
        };
        assert_eq!(range("year:2015"), (Some(2015.0), Some(2015.0)));
        assert_eq!(range("year:2010.."), (Some(2010.0), None));
        assert_eq!(range("year:..2000"), (None, Some(2000.0)));
        assert_eq!(range("year:>2010"), (Some(2011.0), None));
        assert_eq!(range("year:<=2000"), (None, Some(2000.0)));
        assert_eq!(range("rating:>7"), (Some(7.1), None));
        assert_eq!(range("rating:7.5.."), (Some(7.5), None));
    }

    #[test]
    fn a_bare_flag_word_is_a_title_word_but_a_minus_or_is_makes_it_a_chip() {
        assert_eq!(parse_quick_search("new york").text, "new york");
        assert!(parse_quick_search("new york").chips.is_empty());
        let negated = &chips("-new")[0];
        assert!(negated.negated && negated.field == ChipField::New);
        let positive = &chips("is:watched")[0];
        assert!(!positive.negated && positive.field == ChipField::Watched);
        assert!(chips("watched:no")[0].negated);
    }

    #[test]
    fn a_known_field_with_a_nonsense_value_is_rejected_not_searched() {
        let parsed = parse_quick_search("year:abc hdr:xyz heat");
        assert_eq!(parsed.rejected, ["year:abc", "hdr:xyz"]);
        assert_eq!(parsed.text, "heat");
        assert!(parsed.chips.is_empty());
        assert!(!parse_quick_search("year:2020..2010").rejected.is_empty());
        assert!(!parse_quick_search("year:99").rejected.is_empty());
    }

    #[test]
    fn unknown_fields_are_title_words_and_quotes_group_words() {
        let parsed = parse_quick_search(r#"foo:bar "two words" genre:drama,"sci-fi""#);
        assert_eq!(parsed.text, "foo:bar two words");
        assert_eq!(parsed.chips[0].values, ["drama", "sci-fi"]);
        assert!(parse_quick_search(r#"actor:"mel"#).open_quote);
    }

    #[test]
    fn chips_turn_into_the_filters_the_library_page_uses() {
        let preference = RatingPreference::default();
        let parsed = parse_quick_search("year:2010..2019 -watched resolution:4k heat");
        let filters = parsed.filters(&preference);
        assert_eq!(filters.len(), 4);
        assert!(matches!(&filters[0], Filter::Search { text } if text == "heat"));
        assert!(matches!(
            &filters[1],
            Filter::Year { range } if range.min == Some(2010) && range.max == Some(2019)
        ));
        assert!(matches!(
            &filters[2],
            Filter::Flag {
                flag: ItemFlag::Watched,
                value: false
            }
        ));
        assert!(matches!(
            &filters[3],
            Filter::VideoHeight { range } if range.min == Some(1500)
        ));
    }
}

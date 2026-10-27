//! Output encoders for export templates: HTML, XML and JSON escaping of every rendered token.

use std::borrow::Cow;
use std::fmt::Write as _;

use crate::eval::OutputAppender;

/// Escapes for HTML 4 text: `& < > "` and every character that has a named entity in HTML 4
/// (Latin-1 letters and symbols, Greek, arrows, math and typographic punctuation).
#[derive(Debug, Clone, Copy, Default)]
pub struct HtmlEncoder;

/// Escapes for XML 1.0 text: `& < > " '`, and the control characters XML cannot carry as
/// numeric references.
#[derive(Debug, Clone, Copy, Default)]
pub struct XmlEncoder;

/// Escapes for the inside of a JSON string: quotes, backslash, `/`, control characters and
/// everything outside printable ASCII as `\uXXXX`.
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonEncoder;

impl OutputAppender for HtmlEncoder {
    fn append<'a>(&self, value: &'a str, _token: &str) -> Cow<'a, str> {
        escape_html(value)
    }
}

impl OutputAppender for XmlEncoder {
    fn append<'a>(&self, value: &'a str, _token: &str) -> Cow<'a, str> {
        escape_xml(value)
    }
}

impl OutputAppender for JsonEncoder {
    fn append<'a>(&self, value: &'a str, _token: &str) -> Cow<'a, str> {
        escape_json(value)
    }
}

/// The encoder for an export file extension (`html`, `xml`, `json`), or `None` for formats
/// that are written as they are.
pub fn encoder_for_extension(extension: &str) -> Option<Box<dyn OutputAppender>> {
    match extension.to_ascii_lowercase().as_str() {
        "html" | "htm" => Some(Box::new(HtmlEncoder)),
        "xml" => Some(Box::new(XmlEncoder)),
        "json" => Some(Box::new(JsonEncoder)),
        _ => None,
    }
}

pub fn escape_html(text: &str) -> Cow<'_, str> {
    if !text.chars().any(|c| html_entity(c).is_some()) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        match html_entity(c) {
            Some(name) => {
                out.push('&');
                out.push_str(name);
                out.push(';');
            }
            None => out.push(c),
        }
    }
    Cow::Owned(out)
}

pub fn escape_xml(text: &str) -> Cow<'_, str> {
    let needs = |c: char| {
        matches!(c, '&' | '<' | '>' | '"' | '\'')
            || is_xml_removed(c)
            || matches!(c, '\u{7f}'..='\u{84}' | '\u{86}'..='\u{9f}')
    };
    if !text.chars().any(needs) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if is_xml_removed(c) => {}
            c @ ('\u{7f}'..='\u{84}' | '\u{86}'..='\u{9f}') => {
                let _ = write!(out, "&#{};", c as u32);
            }
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Characters XML 1.0 forbids: they are dropped.
fn is_xml_removed(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}')
}

pub fn escape_json(text: &str) -> Cow<'_, str> {
    let plain = |c: char| (' '..='\u{7f}').contains(&c) && !matches!(c, '"' | '\\' | '/');
    if text.chars().all(plain) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\u{8}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if plain(c) => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{unit:04X}");
                }
            }
        }
    }
    Cow::Owned(out)
}

/// The HTML 4 entity name of `c`, if it has one.
fn html_entity(c: char) -> Option<&'static str> {
    match c {
        '"' => Some("quot"),
        '&' => Some("amp"),
        '<' => Some("lt"),
        '>' => Some("gt"),
        '\u{a0}'..='\u{ff}' => Some(LATIN1[c as usize - 0xa0]),
        _ => EXTENDED
            .binary_search_by_key(&(c as u32), |(code, _)| *code)
            .ok()
            .map(|i| EXTENDED[i].1),
    }
}

/// Entity names of U+00A0 to U+00FF.
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf",
    "laquo", "not", "shy", "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro",
    "para", "middot", "cedil", "sup1", "ordm", "raquo", "frac14", "frac12", "frac34", "iquest",
    "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig", "Ccedil", "Egrave", "Eacute",
    "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve", "Oacute",
    "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute",
    "THORN", "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil",
    "egrave", "eacute", "ecirc", "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde",
    "ograve", "oacute", "ocirc", "otilde", "ouml", "divide", "oslash", "ugrave", "uacute", "ucirc",
    "uuml", "yacute", "thorn", "yuml",
];

/// HTML 4 entities above U+00FF, sorted by code point.
const EXTENDED: [(u32, &str); 152] = [
    (338, "OElig"),
    (339, "oelig"),
    (352, "Scaron"),
    (353, "scaron"),
    (376, "Yuml"),
    (402, "fnof"),
    (710, "circ"),
    (732, "tilde"),
    (913, "Alpha"),
    (914, "Beta"),
    (915, "Gamma"),
    (916, "Delta"),
    (917, "Epsilon"),
    (918, "Zeta"),
    (919, "Eta"),
    (920, "Theta"),
    (921, "Iota"),
    (922, "Kappa"),
    (923, "Lambda"),
    (924, "Mu"),
    (925, "Nu"),
    (926, "Xi"),
    (927, "Omicron"),
    (928, "Pi"),
    (929, "Rho"),
    (931, "Sigma"),
    (932, "Tau"),
    (933, "Upsilon"),
    (934, "Phi"),
    (935, "Chi"),
    (936, "Psi"),
    (937, "Omega"),
    (945, "alpha"),
    (946, "beta"),
    (947, "gamma"),
    (948, "delta"),
    (949, "epsilon"),
    (950, "zeta"),
    (951, "eta"),
    (952, "theta"),
    (953, "iota"),
    (954, "kappa"),
    (955, "lambda"),
    (956, "mu"),
    (957, "nu"),
    (958, "xi"),
    (959, "omicron"),
    (960, "pi"),
    (961, "rho"),
    (962, "sigmaf"),
    (963, "sigma"),
    (964, "tau"),
    (965, "upsilon"),
    (966, "phi"),
    (967, "chi"),
    (968, "psi"),
    (969, "omega"),
    (977, "thetasym"),
    (978, "upsih"),
    (982, "piv"),
    (8194, "ensp"),
    (8195, "emsp"),
    (8201, "thinsp"),
    (8204, "zwnj"),
    (8205, "zwj"),
    (8206, "lrm"),
    (8207, "rlm"),
    (8211, "ndash"),
    (8212, "mdash"),
    (8216, "lsquo"),
    (8217, "rsquo"),
    (8218, "sbquo"),
    (8220, "ldquo"),
    (8221, "rdquo"),
    (8222, "bdquo"),
    (8224, "dagger"),
    (8225, "Dagger"),
    (8226, "bull"),
    (8230, "hellip"),
    (8240, "permil"),
    (8242, "prime"),
    (8243, "Prime"),
    (8249, "lsaquo"),
    (8250, "rsaquo"),
    (8254, "oline"),
    (8260, "frasl"),
    (8364, "euro"),
    (8465, "image"),
    (8472, "weierp"),
    (8476, "real"),
    (8482, "trade"),
    (8501, "alefsym"),
    (8592, "larr"),
    (8593, "uarr"),
    (8594, "rarr"),
    (8595, "darr"),
    (8596, "harr"),
    (8629, "crarr"),
    (8656, "lArr"),
    (8657, "uArr"),
    (8658, "rArr"),
    (8659, "dArr"),
    (8660, "hArr"),
    (8704, "forall"),
    (8706, "part"),
    (8707, "exist"),
    (8709, "empty"),
    (8711, "nabla"),
    (8712, "isin"),
    (8713, "notin"),
    (8715, "ni"),
    (8719, "prod"),
    (8721, "sum"),
    (8722, "minus"),
    (8727, "lowast"),
    (8730, "radic"),
    (8733, "prop"),
    (8734, "infin"),
    (8736, "ang"),
    (8743, "and"),
    (8744, "or"),
    (8745, "cap"),
    (8746, "cup"),
    (8747, "int"),
    (8756, "there4"),
    (8764, "sim"),
    (8773, "cong"),
    (8776, "asymp"),
    (8800, "ne"),
    (8801, "equiv"),
    (8804, "le"),
    (8805, "ge"),
    (8834, "sub"),
    (8835, "sup"),
    (8836, "nsub"),
    (8838, "sube"),
    (8839, "supe"),
    (8853, "oplus"),
    (8855, "otimes"),
    (8869, "perp"),
    (8901, "sdot"),
    (8968, "lceil"),
    (8969, "rceil"),
    (8970, "lfloor"),
    (8971, "rfloor"),
    (9001, "lang"),
    (9002, "rang"),
    (9674, "loz"),
    (9824, "spades"),
    (9827, "clubs"),
    (9829, "hearts"),
    (9830, "diams"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_names_latin_and_symbols() {
        assert_eq!(
            escape_html("Müller & Söhne <Teil 1> \"x\" – ok").as_ref(),
            "M&uuml;ller &amp; S&ouml;hne &lt;Teil 1&gt; &quot;x&quot; &ndash; ok"
        );
        assert_eq!(escape_html("it's plain").as_ref(), "it's plain");
        assert!(matches!(escape_html("plain"), Cow::Borrowed(_)));
        assert_eq!(escape_html("\u{a0}€™").as_ref(), "&nbsp;&euro;&trade;");
        // Characters without a name stay as they are.
        assert_eq!(escape_html("日本").as_ref(), "日本");
    }

    #[test]
    fn entity_table_is_sorted_and_complete() {
        assert!(EXTENDED.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(html_entity('\u{e9}'), Some("eacute"));
        assert_eq!(html_entity('\u{ff}'), Some("yuml"));
        assert_eq!(html_entity('\u{a1}'), Some("iexcl"));
    }

    #[test]
    fn xml_escapes_markup_and_drops_invalid_controls() {
        assert_eq!(
            escape_xml("a & b < c > \"d\" 'e' ü – \u{1}\u{7f}").as_ref(),
            "a &amp; b &lt; c &gt; &quot;d&quot; &apos;e&apos; ü – &#127;"
        );
        assert!(matches!(escape_xml("plain ü"), Cow::Borrowed(_)));
    }

    #[test]
    fn json_escapes_like_the_fixtures() {
        assert_eq!(
            escape_json("\"Q\" a/b \\ <x> & ü – \n\t\u{1}").as_ref(),
            "\\\"Q\\\" a\\/b \\\\ <x> & \\u00FC \\u2013 \\n\\t\\u0001"
        );
        assert_eq!(escape_json("😀").as_ref(), "\\uD83D\\uDE00");
        assert!(matches!(escape_json("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn extension_picks_the_encoder() {
        assert!(encoder_for_extension("HTML").is_some());
        assert!(encoder_for_extension("xml").is_some());
        assert!(encoder_for_extension("json").is_some());
        assert!(encoder_for_extension("csv").is_none());
    }
}

//! Text clean-ups the scraper language applies to captured groups.

use super::xml::decode_entities;

/// Removes HTML tags. A line break or paragraph end becomes a newline, so a plot written with
/// `<br />` keeps its lines. Text between `<` and a missing `>` is kept.
pub(crate) fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let starts_tag =
            after[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        let Some(end) = after.find('>').filter(|_| starts_tag) else {
            out.push('<');
            rest = &after[1..];
            continue;
        };
        let name: String = after[1..end]
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        let closing = after[1..].starts_with('/');
        if name == "br" || (closing && matches!(name.as_str(), "p" | "div" | "li")) {
            out.push('\n');
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Decodes HTML entities: the XML ones, numeric references and the common named ones.
pub(crate) fn decode_html(text: &str) -> String {
    let xml = decode_entities(text);
    if !xml.contains('&') {
        return xml;
    }
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml.as_str();
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let named = after
            .find(';')
            .filter(|end| (2..=8).contains(end))
            .and_then(|end| named_entity(&after[..end]).map(|c| (c, end)));
        match named {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "nbsp" => '\u{a0}',
        "copy" => '\u{a9}',
        "reg" => '\u{ae}',
        "hellip" => '\u{2026}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "laquo" => '\u{ab}',
        "raquo" => '\u{bb}',
        "middot" => '\u{b7}',
        "bull" => '\u{2022}',
        "deg" => '\u{b0}',
        "eacute" => '\u{e9}',
        "egrave" => '\u{e8}',
        "agrave" => '\u{e0}',
        "aacute" => '\u{e1}',
        "acirc" => '\u{e2}',
        "auml" => '\u{e4}',
        "ouml" => '\u{f6}',
        "uuml" => '\u{fc}',
        "Auml" => '\u{c4}',
        "Ouml" => '\u{d6}',
        "Uuml" => '\u{dc}',
        "szlig" => '\u{df}',
        "ccedil" => '\u{e7}',
        "ntilde" => '\u{f1}',
        "iacute" => '\u{ed}',
        "oacute" => '\u{f3}',
        "uacute" => '\u{fa}',
        "ecirc" => '\u{ea}',
        "icirc" => '\u{ee}',
        "ocirc" => '\u{f4}',
        _ => return None,
    })
}

/// Percent-encodes `text` for a URL: letters, digits and `-_.!()` stay, a space becomes `+`.
pub(crate) fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'(' | b')' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_keeps_line_breaks() {
        assert_eq!(strip_tags("a<b>bold</b><br />c<p>d</p>"), "abold\ncd\n");
        assert_eq!(strip_tags("1 < 2 and 3 > 2"), "1 < 2 and 3 > 2");
        assert_eq!(strip_tags("x<a href=\"u\">y"), "xy");
    }

    #[test]
    fn decodes_named_and_numeric_entities() {
        assert_eq!(
            decode_html("Caf&eacute; &amp; Bar&#8230; &hellip;"),
            "Caf\u{e9} & Bar\u{2026} \u{2026}"
        );
        assert_eq!(decode_html("&unknown; &"), "&unknown; &");
    }

    #[test]
    fn url_encoding() {
        assert_eq!(url_encode("Heat (1995) & more"), "Heat+(1995)+%26+more");
        assert_eq!(url_encode("caf\u{e9}"), "caf%C3%A9");
    }
}

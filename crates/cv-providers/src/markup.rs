//! A small, forgiving HTML reader for the providers that read web pages.
//!
//! It is not a full HTML parser. It finds tags, reads attributes, returns the inner HTML of an
//! element (matching close tags by name and depth) and extracts JSON-LD blocks, which is all the
//! page providers need. Malformed pages never panic: a tag that never closes simply ends at the
//! end of the document.

use serde_json::Value;

use crate::shared::{decode_entities, html_to_text};

/// Elements that never have a closing tag.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// One element: its opening tag and everything inside it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Element<'a> {
    /// The opening tag without the angle brackets, for example `a href="/x" class="y"`.
    pub open: &'a str,
    /// The HTML between the opening and the closing tag.
    pub inner: &'a str,
}

impl Element<'_> {
    /// An attribute of the opening tag, with entities decoded.
    pub fn attr(&self, name: &str) -> Option<String> {
        attr(self.open, name)
    }

    /// The text inside, tags removed and whitespace collapsed to single spaces.
    pub fn text(&self) -> String {
        plain_text(self.inner)
    }
}

/// The text of an HTML fragment on one line: tags removed, entities decoded, whitespace
/// collapsed.
pub(crate) fn plain_text(html: &str) -> String {
    html_to_text(html)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The value of attribute `name` in an opening tag (`a href="/x"`), entities decoded.
pub(crate) fn attr(open_tag: &str, name: &str) -> Option<String> {
    let bytes = open_tag.as_bytes();
    // Skip the tag name.
    let mut i = open_tag
        .find(|c: char| c.is_whitespace())
        .unwrap_or(open_tag.len());
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'=' | b'/')
        {
            i += 1;
        }
        let key = &open_tag[start..i];
        if key.is_empty() {
            i += 1;
            continue;
        }
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = "";
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && matches!(bytes[i], b'"' | b'\'') {
                let quote = bytes[i];
                let from = i + 1;
                let to = bytes[from..]
                    .iter()
                    .position(|b| *b == quote)
                    .map_or(bytes.len(), |p| from + p);
                value = &open_tag[from..to];
                i = (to + 1).min(bytes.len());
            } else {
                let from = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = &open_tag[from..i];
            }
        }
        if key.eq_ignore_ascii_case(name) {
            return Some(decode_entities(value));
        }
    }
    None
}

/// Whether the space separated attribute `name` of the tag holds `token`.
fn has_token(open_tag: &str, name: &str, token: &str) -> bool {
    attr(open_tag, name).is_some_and(|v| v.split_whitespace().any(|t| t == token))
}

/// A tag found by [`next_tag`].
struct Tag<'a> {
    /// Byte offset of `<`.
    start: usize,
    /// Byte offset just after `>`.
    end: usize,
    /// Lower-case tag name.
    name: String,
    closing: bool,
    /// The text between `<` and `>`.
    body: &'a str,
}

/// The offset of the `>` that ends the tag at the start of `rest`. A `>` inside a quoted
/// attribute value (a tooltip holding markup, for example) does not end it.
fn tag_close(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut quote: Option<u8> = None;
    // The last byte that was not white space, to tell a value's opening quote (after `=`) from
    // a stray one.
    let mut previous = 0u8;
    for (i, &b) in bytes.iter().enumerate().skip(1) {
        if let Some(q) = quote {
            if b == q {
                quote = None;
                previous = b;
            }
            continue;
        }
        match b {
            b'>' => return Some(i),
            b'"' | b'\'' if previous == b'=' => quote = Some(b),
            _ => {}
        }
        if !b.is_ascii_whitespace() {
            previous = b;
        }
    }
    None
}

/// The first tag at or after `from`. Comments, doctypes and the content of `script` and `style`
/// elements are skipped.
fn next_tag(html: &str, mut from: usize) -> Option<Tag<'_>> {
    loop {
        let start = from + html.get(from..)?.find('<')?;
        let rest = &html[start..];
        if rest.starts_with("<!--") {
            from = rest.find("-->").map_or(html.len(), |p| start + p + 3);
            continue;
        }
        let close = tag_close(rest)?;
        let body = &rest[1..close];
        let first = body.chars().next();
        if !first.is_some_and(|c| c.is_ascii_alphabetic() || c == '/') {
            // `<!doctype>`, `<?xml?>`, or a stray `<`.
            from = start + 1;
            continue;
        }
        let closing = body.starts_with('/');
        let name: String = body
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == ':')
            .collect::<String>()
            .to_ascii_lowercase();
        let end = start + close + 1;
        return Some(Tag {
            start,
            end,
            name,
            closing,
            body,
        });
    }
}

/// The byte offset of the first ASCII-case-insensitive `needle` in `haystack`.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Where the raw-text element opened by `tag` ends: the offsets of its closing tag and just after
/// it.
fn raw_end(html: &str, tag: &Tag<'_>) -> (usize, usize) {
    match find_ci(&html[tag.end..], &format!("</{}", tag.name)) {
        Some(p) => {
            let at = tag.end + p;
            (at, html[at..].find('>').map_or(html.len(), |g| at + g + 1))
        }
        None => (html.len(), html.len()),
    }
}

fn is_self_closing(tag: &Tag<'_>) -> bool {
    tag.body.trim_end().ends_with('/') || VOID.contains(&tag.name.as_str())
}

/// The element opened by `open`: its inner HTML up to the matching close tag.
fn element_from<'a>(html: &'a str, open: &Tag<'a>) -> Element<'a> {
    let open_text = open.body;
    if is_self_closing(open) {
        return Element {
            open: open_text,
            inner: "",
        };
    }
    let mut depth = 1usize;
    let mut at = open.end;
    // Raw text elements hold text only.
    if open.name == "script" || open.name == "style" {
        let (inner_end, _) = raw_end(html, open);
        return Element {
            open: open_text,
            inner: &html[open.end..inner_end],
        };
    }
    while let Some(tag) = next_tag(html, at) {
        at = tag.end;
        if tag.name == "script" || tag.name == "style" {
            if !tag.closing {
                at = raw_end(html, &tag).1;
            }
            continue;
        }
        if tag.name != open.name {
            continue;
        }
        if tag.closing {
            depth -= 1;
            if depth == 0 {
                return Element {
                    open: open_text,
                    inner: &html[open.end..tag.start],
                };
            }
        } else if !is_self_closing(&tag) {
            depth += 1;
        }
    }
    Element {
        open: open_text,
        inner: &html[open.end..],
    }
}

/// Every element for which `keep(open_tag)` is true, in document order. Elements nested in a
/// match are found too.
fn find_all<'a>(html: &'a str, keep: impl Fn(&Tag<'a>) -> bool) -> Vec<Element<'a>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(tag) = next_tag(html, at) {
        at = tag.end;
        if !tag.closing && (tag.name == "script" || tag.name == "style") {
            if keep(&tag) {
                out.push(element_from(html, &tag));
            }
            at = raw_end(html, &tag).1;
            continue;
        }
        if !tag.closing && keep(&tag) {
            out.push(element_from(html, &tag));
        }
    }
    out
}

/// Every `<tag>` element.
pub(crate) fn elements<'a>(html: &'a str, tag: &str) -> Vec<Element<'a>> {
    find_all(html, |t| t.name == tag)
}

/// Every element (of any tag) whose attribute `name` holds `token` as one of its
/// space-separated values, for example `with_token(html, "itemprop", "genre")`.
pub(crate) fn with_token<'a>(html: &'a str, name: &str, token: &str) -> Vec<Element<'a>> {
    find_all(html, |t| has_token(t.body, name, token))
}

/// The first element carrying `id="value"`.
pub(crate) fn by_id<'a>(html: &'a str, id: &str) -> Option<Element<'a>> {
    find_all(html, |t| attr(t.body, "id").as_deref() == Some(id))
        .into_iter()
        .next()
}

/// The first `<meta>` content for `property="name"` or `name="name"`.
pub(crate) fn meta(html: &str, name: &str) -> Option<String> {
    elements(html, "meta").into_iter().find_map(|m| {
        let matches = attr(m.open, "property").as_deref() == Some(name)
            || attr(m.open, "name").as_deref() == Some(name);
        matches
            .then(|| attr(m.open, "content"))
            .flatten()
            .map(|c| c.trim().to_owned())
            .filter(|c| !c.is_empty())
    })
}

/// The JSON of every `<script type="application/ld+json">` block. Arrays and `@graph` lists are
/// flattened, so each returned value is one node. Blocks that are not valid JSON are skipped.
pub(crate) fn json_ld(html: &str) -> Vec<Value> {
    let mut nodes = Vec::new();
    for script in elements(html, "script") {
        let kind = attr(script.open, "type").unwrap_or_default();
        if !kind.eq_ignore_ascii_case("application/ld+json") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(script.inner.trim()) else {
            continue;
        };
        flatten(value, &mut nodes);
    }
    nodes
}

fn flatten(value: Value, out: &mut Vec<Value>) {
    match value {
        Value::Array(list) => list.into_iter().for_each(|v| flatten(v, out)),
        Value::Object(mut map) => {
            let graph = map.remove("@graph");
            if !map.is_empty() {
                out.push(Value::Object(map));
            }
            if let Some(graph) = graph {
                flatten(graph, out);
            }
        }
        _ => {}
    }
}

/// Whether a JSON-LD node has schema.org type `kind` (its `@type` is a string or a list).
pub(crate) fn has_type(node: &Value, kind: &str) -> bool {
    match node.get("@type") {
        Some(Value::String(s)) => s.eq_ignore_ascii_case(kind),
        Some(Value::Array(list)) => list
            .iter()
            .any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(kind))),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_are_read_in_any_quoting() {
        let tag = r#"a href="/film/1,Heat/" class='x y' data-n=5 disabled title="A &amp; B""#;
        assert_eq!(attr(tag, "href").as_deref(), Some("/film/1,Heat/"));
        assert_eq!(attr(tag, "class").as_deref(), Some("x y"));
        assert_eq!(attr(tag, "data-n").as_deref(), Some("5"));
        assert_eq!(attr(tag, "disabled").as_deref(), Some(""));
        assert_eq!(attr(tag, "title").as_deref(), Some("A & B"));
        assert_eq!(attr(tag, "missing"), None);
        // An attribute name inside a value is not an attribute.
        assert_eq!(attr(r#"a title="href=x""#, "href"), None);
    }

    #[test]
    fn inner_html_matches_nested_tags() {
        let html = "<div id=a><div>inner</div>tail</div><div id=b>second</div>";
        let a = by_id(html, "a").unwrap();
        assert_eq!(a.inner, "<div>inner</div>tail");
        assert_eq!(by_id(html, "b").unwrap().text(), "second");
        assert_eq!(elements(html, "div").len(), 3);
    }

    #[test]
    fn void_and_unclosed_elements_do_not_break_matching() {
        let html = "<p>one<br>two<img src=x> three</p><span>unclosed";
        assert_eq!(elements(html, "p")[0].text(), "one two three");
        assert_eq!(elements(html, "span")[0].inner, "unclosed");
    }

    #[test]
    fn a_greater_than_sign_inside_an_attribute_does_not_end_the_tag() {
        let html =
            r#"<a href="/x" title="<img src='a.jpg'>">Name</a><span title='a > b'>Two</span>"#;
        let links = elements(html, "a");
        assert_eq!(links[0].text(), "Name");
        assert_eq!(links[0].attr("title").as_deref(), Some("<img src='a.jpg'>"));
        assert_eq!(elements(html, "span")[0].text(), "Two");
    }

    #[test]
    fn scripts_and_comments_are_skipped() {
        let html = "<!-- <div>no</div> --><script>var a = '<div>x</div>';</script><div>yes</div>";
        let divs = elements(html, "div");
        assert_eq!(divs.len(), 1);
        assert_eq!(divs[0].text(), "yes");
    }

    #[test]
    fn tokens_match_whole_words() {
        let html = r#"<span itemprop="name">A</span><span itemprop="nameless">B</span><b itemprop="genre name">C</b>"#;
        let names: Vec<_> = with_token(html, "itemprop", "name")
            .iter()
            .map(Element::text)
            .collect();
        assert_eq!(names, ["A", "C"]);
    }

    #[test]
    fn meta_reads_property_or_name() {
        let html = r#"<meta property="og:title" content="Tatort &amp; mehr"><meta name="description" content=" Krimi ">"#;
        assert_eq!(meta(html, "og:title").as_deref(), Some("Tatort & mehr"));
        assert_eq!(meta(html, "description").as_deref(), Some("Krimi"));
        assert_eq!(meta(html, "og:image"), None);
    }

    #[test]
    fn json_ld_nodes_are_flattened() {
        let html = r#"<script type="application/ld+json">{"@graph":[{"@type":"TVSeries","name":"A"},{"@type":["Thing","TVEpisode"]}]}</script>
            <script type="application/ld+json">[{"@type":"Movie"}]</script>
            <script type="application/ld+json">{broken</script><script>var x</script>"#;
        let nodes = json_ld(html);
        assert_eq!(nodes.len(), 3);
        assert!(has_type(&nodes[0], "TVSeries"));
        assert!(has_type(&nodes[1], "tvepisode"));
        assert!(has_type(&nodes[2], "Movie"));
        assert!(!has_type(&nodes[2], "TVSeries"));
    }

    #[test]
    fn garbage_does_not_panic() {
        for html in ["", "<", "<a", "<a href=", "<<<>>>", "</p></p>", "<div"] {
            let _ = elements(html, "div");
            let _ = json_ld(html);
            let _ = meta(html, "x");
        }
    }
}

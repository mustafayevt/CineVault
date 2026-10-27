//! A minimal XML tree and the serializer that gives NFO files their exact layout.
//!
//! Layout rules, fixed by the compatibility fixtures:
//!
//! * the document starts with `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>`;
//! * two-space indentation, CRLF between lines, a CRLF after the last line;
//! * an element with child nodes puts each child on its own line; an element with only text stays
//!   on one line; an element with neither is self-closing (`<year/>`);
//! * attributes are written in alphabetical order;
//! * text escapes `&`, `<` and `>`, attributes also `"`; a line break inside text (LF, CR or CRLF)
//!   is written as CRLF like every other line end; characters XML 1.0 forbids are dropped.

use std::fmt::Write as _;

/// A child of an element: an element or a comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Comment(String),
}

/// An XML element. When `children` is not empty they are written and `text` is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Node>,
}

impl Element {
    /// An empty element (`<name/>`).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attrs: Vec::new(),
            text: String::new(),
            children: Vec::new(),
        }
    }

    /// An element with text; empty text gives a self-closing element.
    pub fn text(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::new(name)
        }
    }

    /// Adds an attribute. Attributes are written sorted by name.
    #[must_use]
    pub fn attr(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.attrs.push((name.into(), value.into()));
        self
    }

    /// Adds a child element.
    #[must_use]
    pub fn child(mut self, child: Element) -> Self {
        self.children.push(Node::Element(child));
        self
    }

    /// Appends a child element.
    pub fn push(&mut self, child: Element) {
        self.children.push(Node::Element(child));
    }

    /// Appends a comment.
    pub fn push_comment(&mut self, text: impl Into<String>) {
        self.children.push(Node::Comment(text.into()));
    }

    /// The value of an attribute.
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The first child element with a name.
    pub fn find(&self, name: &str) -> Option<&Element> {
        self.children.iter().find_map(|n| match n {
            Node::Element(e) if e.name == name => Some(e),
            _ => None,
        })
    }
}

/// A document: an optional leading comment and one or more root elements (several roots are how
/// multi-episode files are written). The declaration is written once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub comment: Option<String>,
    pub roots: Vec<Element>,
}

/// The XML declaration line every NFO starts with.
pub const DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

const EOL: &str = "\r\n";

impl Document {
    pub fn new(root: Element) -> Self {
        Self {
            comment: None,
            roots: vec![root],
        }
    }

    /// Serializes the document: UTF-8 text with two-space indentation and CRLF line ends.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(4096);
        out.push_str(DECLARATION);
        out.push_str(EOL);
        if let Some(comment) = &self.comment {
            write_comment(&mut out, 0, comment);
        }
        for root in &self.roots {
            write_element(&mut out, 0, root);
        }
        out
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_comment(out: &mut String, depth: usize, text: &str) {
    indent(out, depth);
    out.push_str("<!--");
    out.push_str(&text.replace("--", "- -"));
    out.push_str("-->");
    out.push_str(EOL);
}

fn write_element(out: &mut String, depth: usize, element: &Element) {
    indent(out, depth);
    out.push('<');
    out.push_str(&element.name);
    let mut attrs: Vec<&(String, String)> = element.attrs.iter().collect();
    attrs.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, value) in attrs {
        let _ = write!(out, " {name}=\"");
        escape_into(out, value, true);
        out.push('"');
    }
    if !element.children.is_empty() {
        out.push('>');
        out.push_str(EOL);
        for child in &element.children {
            match child {
                Node::Element(e) => write_element(out, depth + 1, e),
                Node::Comment(c) => write_comment(out, depth + 1, c),
            }
        }
        indent(out, depth);
        let _ = write!(out, "</{}>", element.name);
    } else if element.text.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        escape_into(out, &element.text, false);
        let _ = write!(out, "</{}>", element.name);
    }
    out.push_str(EOL);
}

/// True for the characters XML 1.0 allows.
fn is_xml_char(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}

fn escape_into(out: &mut String, text: &str, attribute: bool) {
    let mut chars = text.chars().filter(|c| is_xml_char(*c)).peekable();
    while let Some(c) = chars.next() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\t' if attribute => out.push_str("&#9;"),
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str(if attribute { "&#10;" } else { EOL });
            }
            other => out.push(other),
        }
    }
}

/// Removes XML comments from NFO text, together with a line that holds nothing else. Used to
/// decide whether a rewrite would change anything: the generator comment carries a timestamp.
pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_comment = false;
    for line in text.split_inclusive('\n') {
        let mut rest = line;
        let mut kept = String::new();
        let mut had_comment = in_comment;
        loop {
            if in_comment {
                match rest.find("-->") {
                    Some(end) => {
                        rest = &rest[end + 3..];
                        in_comment = false;
                    }
                    None => break,
                }
            } else {
                match rest.find("<!--") {
                    Some(start) => {
                        kept.push_str(&rest[..start]);
                        rest = &rest[start + 4..];
                        in_comment = true;
                        had_comment = true;
                    }
                    None => {
                        kept.push_str(rest);
                        break;
                    }
                }
            }
        }
        if had_comment && kept.trim().is_empty() {
            continue;
        }
        out.push_str(&kept);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_indented_crlf_with_self_closing_empties() {
        let root = Element::new("movie")
            .child(Element::text("title", "A & B <1>"))
            .child(Element::new("year"))
            .child(
                Element::new("set")
                    .child(Element::text("name", "S"))
                    .child(Element::new("overview")),
            );
        let text = Document::new(root).render();
        assert_eq!(
            text,
            format!(
                "{DECLARATION}\r\n<movie>\r\n  <title>A &amp; B &lt;1&gt;</title>\r\n  <year/>\r\n  <set>\r\n    <name>S</name>\r\n    <overview/>\r\n  </set>\r\n</movie>\r\n"
            )
        );
    }

    #[test]
    fn attributes_are_sorted_and_escaped() {
        let e = Element::text("rating", "x")
            .attr("name", "a\"b")
            .attr("default", "true");
        let text = Document::new(e).render();
        assert!(text.contains(r#"<rating default="true" name="a&quot;b">x</rating>"#));
    }

    #[test]
    fn text_keeps_quotes_and_writes_line_breaks_as_crlf() {
        let e = Element::text("plot", "He said \"hi\".\nNext\r\nline\rend");
        let text = Document::new(e).render();
        assert!(text.contains("<plot>He said \"hi\".\r\nNext\r\nline\r\nend</plot>"));
    }

    #[test]
    fn attribute_line_breaks_are_character_references() {
        let e = Element::new("a").attr("b", "x\ny");
        assert!(Document::new(e).render().contains(r#"<a b="x&#10;y"/>"#));
    }

    #[test]
    fn forbidden_characters_are_dropped() {
        let e = Element::text("a", "x\u{0}y\u{8}z");
        assert!(Document::new(e).render().contains("<a>xyz</a>"));
    }

    #[test]
    fn comments_are_written_on_their_own_line() {
        let mut root = Element::new("movie");
        root.push_comment("meta data");
        root.push(Element::new("a"));
        let mut doc = Document::new(root);
        doc.comment = Some("created".to_owned());
        assert_eq!(
            doc.render(),
            format!(
                "{DECLARATION}\r\n<!--created-->\r\n<movie>\r\n  <!--meta data-->\r\n  <a/>\r\n</movie>\r\n"
            )
        );
    }

    #[test]
    fn several_roots_share_one_declaration() {
        let doc = Document {
            comment: None,
            roots: vec![Element::new("a"), Element::new("b")],
        };
        let text = doc.render();
        assert_eq!(text.matches("<?xml").count(), 1);
        assert!(text.ends_with("<a/>\r\n<b/>\r\n"));
    }

    #[test]
    fn stripping_comments_removes_comment_only_lines() {
        let text = "<?xml?>\r\n<!--created on x-->\r\n<movie>\r\n  <!--a\r\n  b-->\r\n  <a/><!--c-->\r\n</movie>\r\n";
        assert_eq!(
            strip_comments(text),
            "<?xml?>\r\n<movie>\r\n  <a/>\r\n</movie>\r\n"
        );
    }
}

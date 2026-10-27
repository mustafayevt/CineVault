//! A forgiving XML reader.
//!
//! Scraper definitions are well-formed, but the text a scraper's regular expressions assemble
//! is not always: a title can hold a bare `&`, a tag can be left open. This reader never fails.
//! It builds a tree from whatever it can see: unknown entities stay as they are, a `<` that does
//! not start a tag is text, an end tag closes the nearest open element of that name, and
//! elements still open at the end are closed there.

/// One element with its attributes and children.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The child elements named `name`, in order.
    pub fn elements<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter_map(move |node| match node {
            Node::Element(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    /// Every child element, in order.
    pub fn child_elements(&self) -> impl Iterator<Item = &Element> + '_ {
        self.children.iter().filter_map(|node| match node {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub fn first(&self, name: &str) -> Option<&Element> {
        self.child_elements().find(|e| e.name == name)
    }

    /// All text inside the element, descendants included, in document order.
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        for node in &self.children {
            match node {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => e.collect_text(out),
            }
        }
    }

    /// The element as XML text, with text and attribute values escaped.
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        self.write_xml(&mut out);
        out
    }

    fn write_xml(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.name);
        for (name, value) in &self.attrs {
            out.push_str(&format!(" {name}=\"{}\"", escape(value)));
        }
        if self.children.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for node in &self.children {
            match node {
                Node::Text(t) => out.push_str(&escape(t)),
                Node::Element(e) => e.write_xml(out),
            }
        }
        out.push_str("</");
        out.push_str(&self.name);
        out.push('>');
    }

    /// The text of the first child named `name`, trimmed; empty when there is none.
    pub fn child_text(&self, name: &str) -> String {
        self.first(name)
            .map(|e| e.text().trim().to_owned())
            .unwrap_or_default()
    }
}

/// Parses `input` into a synthetic root whose children are the document's top-level nodes.
pub(crate) fn parse(input: &str) -> Element {
    let mut stack: Vec<Element> = vec![Element::default()];
    let bytes = input.as_bytes();
    let mut pos = 0;
    let mut text_start = 0;

    macro_rules! flush_text {
        ($end:expr) => {
            if $end > text_start {
                let raw = &input[text_start..$end];
                push_node(&mut stack, Node::Text(decode_entities(raw)));
            }
        };
    }

    while pos < bytes.len() {
        if bytes[pos] != b'<' {
            pos += 1;
            continue;
        }
        let rest = &input[pos..];
        if rest.starts_with("<![CDATA[") {
            flush_text!(pos);
            let body_start = pos + 9;
            let end = input[body_start..]
                .find("]]>")
                .map_or(input.len(), |i| body_start + i);
            push_node(&mut stack, Node::Text(input[body_start..end].to_owned()));
            pos = (end + 3).min(input.len());
            text_start = pos;
        } else if rest.starts_with("<!--") {
            flush_text!(pos);
            pos = input[pos + 4..]
                .find("-->")
                .map_or(input.len(), |i| pos + 4 + i + 3);
            text_start = pos;
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            flush_text!(pos);
            pos = input[pos..].find('>').map_or(input.len(), |i| pos + i + 1);
            text_start = pos;
        } else if let Some(after) = rest.strip_prefix("</") {
            if !after.starts_with(is_name_start) {
                pos += 1;
                continue;
            }
            flush_text!(pos);
            let end = input[pos..].find('>').map_or(input.len(), |i| pos + i + 1);
            let name_end = end.saturating_sub(1).max(pos + 2);
            let name = input[pos + 2..name_end].trim();
            close(&mut stack, name);
            pos = end;
            text_start = pos;
        } else if rest[1..].starts_with(is_name_start) {
            flush_text!(pos);
            let (element, self_closing, consumed) = parse_start_tag(rest);
            pos += consumed;
            text_start = pos;
            if self_closing {
                push_node(&mut stack, Node::Element(element));
            } else {
                stack.push(element);
            }
        } else {
            pos += 1;
        }
    }
    flush_text!(bytes.len());
    while stack.len() > 1 {
        let done = stack.pop().expect("the stack has an open element");
        push_node(&mut stack, Node::Element(done));
    }
    stack.pop().expect("the root is always on the stack")
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == ':'
}

fn push_node(stack: &mut [Element], node: Node) {
    if let Some(top) = stack.last_mut() {
        top.children.push(node);
    }
}

/// Closes the nearest open element called `name`, and everything opened inside it. An end tag
/// with no matching open element is ignored.
fn close(stack: &mut Vec<Element>, name: &str) {
    let Some(index) = stack.iter().rposition(|e| e.name == name) else {
        return;
    };
    if index == 0 {
        return;
    }
    while stack.len() > index {
        let done = stack.pop().expect("the stack is longer than the index");
        push_node(stack, Node::Element(done));
    }
}

/// Reads `<name attr="v" ...>` or `<name .../>`. Returns the element, whether it was
/// self-closing, and the bytes consumed. `rest` starts at the `<`.
fn parse_start_tag(rest: &str) -> (Element, bool, usize) {
    let bytes = rest.as_bytes();
    let name_end = rest[1..]
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .map_or(rest.len(), |n| n + 1);
    let mut element = Element {
        name: rest[1..name_end].to_owned(),
        ..Element::default()
    };
    let mut i = name_end;
    let mut self_closing = false;
    let is_space = |b: u8| (b as char).is_whitespace();
    loop {
        while i < bytes.len() && is_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        match bytes[i] {
            b'>' => {
                i += 1;
                break;
            }
            b'/' => {
                self_closing = true;
                i += 1;
            }
            _ => {
                let start = i;
                while i < bytes.len()
                    && !is_space(bytes[i])
                    && !matches!(bytes[i], b'=' | b'>' | b'/')
                {
                    i += 1;
                }
                if start == i {
                    i += 1;
                    continue;
                }
                self_closing = false;
                let attr_name = rest[start..i].to_owned();
                while i < bytes.len() && is_space(bytes[i]) {
                    i += 1;
                }
                let mut value = String::new();
                if i < bytes.len() && bytes[i] == b'=' {
                    i += 1;
                    while i < bytes.len() && is_space(bytes[i]) {
                        i += 1;
                    }
                    if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                        let quote = bytes[i];
                        i += 1;
                        let value_start = i;
                        while i < bytes.len() && bytes[i] != quote {
                            i += 1;
                        }
                        value = decode_entities(&rest[value_start..i]);
                        i = (i + 1).min(bytes.len());
                    } else {
                        let value_start = i;
                        while i < bytes.len() && !is_space(bytes[i]) && bytes[i] != b'>' {
                            i += 1;
                        }
                        value = decode_entities(&rest[value_start..i]);
                    }
                }
                element.attrs.push((attr_name, value));
            }
        }
    }
    (element, self_closing, i)
}

/// Escapes the characters XML text and attribute values cannot hold.
pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Decodes the XML entities and numeric references; anything else stays as written.
pub(crate) fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let decoded = after
            .find(';')
            .filter(|end| *end <= 10)
            .and_then(|end| entity(&after[..end]).map(|c| (c, end)));
        match decoded {
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

fn entity(name: &str) -> Option<char> {
    match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let digits = name.strip_prefix('#')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nested_elements_attributes_and_text() {
        let root = parse(r#"<a x="1" y='2'><b>one</b><c/><b>two &amp; three</b></a>"#);
        let a = root.first("a").unwrap();
        assert_eq!(a.attr("x"), Some("1"));
        assert_eq!(a.attr("y"), Some("2"));
        let texts: Vec<_> = a.elements("b").map(Element::text).collect();
        assert_eq!(texts, ["one", "two & three"]);
        assert!(a.first("c").is_some());
    }

    #[test]
    fn tolerates_bare_ampersands_stray_angle_brackets_and_unclosed_tags() {
        let root = parse("<r><t>Tom & Jerry</t><u>a < b</u><v>open");
        let r = root.first("r").unwrap();
        assert_eq!(r.child_text("t"), "Tom & Jerry");
        assert_eq!(r.child_text("u"), "a < b");
        assert_eq!(r.child_text("v"), "open");
    }

    #[test]
    fn cdata_comments_and_declarations() {
        let root = parse("<?xml version=\"1.0\"?><!-- note --><r><![CDATA[<b>x</b>]]></r>");
        assert_eq!(root.first("r").unwrap().text(), "<b>x</b>");
    }

    #[test]
    fn a_stray_end_tag_is_ignored_and_a_mismatch_closes_inner_elements() {
        let root = parse("</x><a><b>t</a><c>u</c>");
        let a = root.first("a").unwrap();
        assert_eq!(a.child_text("b"), "t");
        assert_eq!(root.child_text("c"), "u");
    }

    #[test]
    fn writes_back_what_it_read() {
        let root = parse(r#"<a x="1&amp;2"><b>t &lt; u</b><c/></a>"#);
        assert_eq!(
            root.first("a").unwrap().to_xml(),
            r#"<a x="1&amp;2"><b>t &lt; u</b><c/></a>"#
        );
    }

    #[test]
    fn numeric_references() {
        assert_eq!(decode_entities("&#65;&#x42;&bogus;&"), "AB&bogus;&");
    }
}

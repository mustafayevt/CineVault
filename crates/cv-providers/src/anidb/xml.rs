//! A small element tree for AniDB's XML answers.
//!
//! AniDB answers with a few hundred elements per anime and nothing exotic, so the tree keeps
//! just what the reader needs: the name, the attributes, the text and the children of every
//! element. Text is joined from its pieces and entities are resolved; a document that is cut
//! off or malformed is an error, because a half-read anime would write half-true data.

use std::borrow::Cow;

use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

/// One XML element.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    /// The text directly inside, trimmed. Empty for an element that holds only elements.
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The first child called `name`.
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// The children called `name`.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The non-blank text of the first child called `name`.
    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name)
            .map(|c| c.text.as_str())
            .filter(|t| !t.is_empty())
    }
}

fn node_of(start: &BytesStart<'_>) -> Node {
    let mut node = Node {
        name: start.name().as_ref().to_owned(),
        ..Node::default()
    };
    for attribute in start.attributes().with_checks(false).flatten() {
        let key = attribute.key.as_ref().to_owned();
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_or_else(|_| attribute.value.clone().into_owned(), Cow::into_owned);
        node.attrs.push((key, value));
    }
    node
}

/// Parses `text` into its root element.
pub(crate) fn parse(text: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(text.trim_start_matches('\u{feff}'));
    // The element stack with the text collected inside each.
    let mut open: Vec<(Node, String)> = Vec::new();
    let mut root: Option<Node> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => open.push((node_of(&start), String::new())),
            Ok(Event::Empty(start)) => {
                let node = node_of(&start);
                attach(&mut open, &mut root, node);
            }
            Ok(Event::End(_)) => {
                let (mut node, text) = open.pop().ok_or("an end tag without a start tag")?;
                node.text = text.trim().to_owned();
                attach(&mut open, &mut root, node);
            }
            Ok(Event::Text(t)) => push_text(&mut open, &t.xml10_content()),
            Ok(Event::CData(t)) => push_text(&mut open, &t.xml10_content()),
            Ok(Event::GeneralRef(reference)) => {
                if let Ok(Some(c)) = reference.resolve_char_ref() {
                    push_text(&mut open, c.encode_utf8(&mut [0; 4]));
                } else if let Some(resolved) = quick_xml::escape::resolve_xml_entity(&reference) {
                    push_text(&mut open, resolved);
                } else {
                    push_text(&mut open, &format!("&{};", &*reference));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    if !open.is_empty() {
        return Err("the document ends inside an element".to_owned());
    }
    root.ok_or_else(|| "the document has no root element".to_owned())
}

fn push_text(open: &mut [(Node, String)], text: &str) {
    if let Some((_, buffer)) = open.last_mut() {
        buffer.push_str(text);
    }
}

fn attach(open: &mut [(Node, String)], root: &mut Option<Node>, node: Node) {
    match open.last_mut() {
        Some((parent, _)) => parent.children.push(node),
        None => {
            if root.is_none() {
                *root = Some(node);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_attributes_and_text_are_read() {
        let doc = parse(
            "<?xml version=\"1.0\"?><a id=\"1\"><b xml:lang=\"en\"> x &amp; &#233; </b><b/><c><d>1</d></c></a>",
        )
        .unwrap();
        assert_eq!(doc.name, "a");
        assert_eq!(doc.attr("id"), Some("1"));
        let bs: Vec<_> = doc.all("b").collect();
        assert_eq!(bs.len(), 2);
        assert_eq!(bs[0].attr("xml:lang"), Some("en"));
        assert_eq!(bs[0].text, "x & é");
        assert_eq!(doc.child("c").and_then(|c| c.child_text("d")), Some("1"));
        assert_eq!(doc.child_text("missing"), None);
    }

    #[test]
    fn broken_documents_are_errors() {
        assert!(parse("").is_err());
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("<a><b>text").is_err());
        assert!(parse("just text").is_err());
    }
}

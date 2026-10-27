//! A forgiving XML reader that builds the element tree of an NFO.
//!
//! Old NFOs are written by many tools and edited by hand, so the reader never fails. A closing tag
//! that matches no open element closes the innermost one (unless that one already holds child
//! elements: then the tag is stray and ignored), one that matches an outer element closes the
//! elements inside it, unknown entities are kept as written, and text that ends in a syntax error or in
//! the middle of an element keeps everything read so far, with the open elements closed.
//!
//! The tree is the one the writers use ([`Element`]), so an element read here can be written back
//! unchanged. Two limits come from that model: an element with child elements keeps only the
//! non-blank text read directly inside it (the pieces between its children are joined), and
//! comments are kept only inside elements that have child elements. The writers ignore that text.
//! The readers use it to keep the value of a tag that was never closed.

use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use crate::xml::{Element, Node};

/// Reads the root elements of `text`. A leading byte-order mark, the XML declaration, comments
/// outside the root and processing instructions are skipped.
pub fn parse_roots(text: &str) -> Vec<Element> {
    let mut reader = Reader::from_str(text.trim_start_matches('\u{feff}'));
    reader.config_mut().check_end_names = false;
    let mut builder = Builder::default();
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => builder.open(element_of(&start)),
            Ok(Event::Empty(start)) => {
                builder.open(element_of(&start));
                builder.close_top();
            }
            Ok(Event::End(end)) => builder.close_named(end.name().as_ref()),
            Ok(Event::Text(t)) => builder.text(&t.xml10_content()),
            Ok(Event::CData(t)) => builder.text(&t.xml10_content()),
            Ok(Event::GeneralRef(reference)) => {
                if let Ok(Some(c)) = reference.resolve_char_ref() {
                    builder.text(c.encode_utf8(&mut [0; 4]));
                } else if let Some(resolved) = quick_xml::escape::resolve_xml_entity(&reference) {
                    builder.text(resolved);
                } else {
                    builder.text(&format!("&{};", &*reference));
                }
            }
            Ok(Event::Comment(comment)) => {
                builder.comment(&comment.xml10_content());
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    builder.finish()
}

fn element_of(start: &BytesStart<'_>) -> Element {
    let mut element = Element::new(start.name().as_ref());
    for attribute in start.attributes().with_checks(false).flatten() {
        let key = attribute.key.as_ref().to_owned();
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_or_else(
                |_| attribute.value.clone().into_owned(),
                std::borrow::Cow::into_owned,
            );
        element.attrs.push((key, value));
    }
    element
}

/// Open elements with the text read into each, and the finished roots.
#[derive(Default)]
struct Builder {
    open: Vec<(Element, String)>,
    roots: Vec<Element>,
}

impl Builder {
    fn open(&mut self, element: Element) {
        self.open.push((element, String::new()));
    }

    fn text(&mut self, text: &str) {
        if let Some((_, buffer)) = self.open.last_mut() {
            buffer.push_str(text);
        }
    }

    fn comment(&mut self, text: &str) {
        if let Some((element, _)) = self.open.last_mut() {
            element.children.push(Node::Comment(text.to_owned()));
        }
    }

    fn close_top(&mut self) {
        let Some((mut element, text)) = self.open.pop() else {
            return;
        };
        let has_elements = element
            .children
            .iter()
            .any(|n| matches!(n, Node::Element(_)));
        if has_elements {
            element.text = if text.trim().is_empty() {
                String::new()
            } else {
                text
            };
        } else {
            element.children.clear();
            element.text = text;
        }
        match self.open.last_mut() {
            Some((parent, _)) => parent.children.push(Node::Element(element)),
            None => self.roots.push(element),
        }
    }

    /// Closes the innermost open element called `name` and everything inside it. A name that is
    /// not open is taken for a misspelt closing tag of the innermost open element, unless that
    /// element already holds child elements: then it is a stray tag and is ignored (an element
    /// that is really finished is closed at its own tag or at the end of the text).
    fn close_named(&mut self, name: &str) {
        let Some(depth) = self.open.iter().rposition(|(e, _)| e.name == name) else {
            let has_children = self
                .open
                .last()
                .is_some_and(|(e, _)| e.children.iter().any(|n| matches!(n, Node::Element(_))));
            if !has_children {
                self.close_top();
            }
            return;
        };
        while self.open.len() > depth {
            self.close_top();
        }
    }

    fn finish(mut self) -> Vec<Element> {
        while !self.open.is_empty() {
            self.close_top();
        }
        self.roots
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(elements: &[Element]) -> Vec<&str> {
        elements.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn reads_nested_elements_attributes_and_text() {
        let roots = parse_roots(
            "\u{feff}<?xml version=\"1.0\"?>\r\n<!--made by x-->\r\n<movie>\r\n  <title>A &amp; B &#233;</title>\r\n  <nested><inner a=\"1\" b='x&quot;y'>v</inner></nested>\r\n  <empty/>\r\n</movie>\r\n",
        );
        assert_eq!(names(&roots), ["movie"]);
        let movie = &roots[0];
        assert_eq!(movie.find("title").unwrap().text, "A & B \u{e9}");
        let inner = movie.find("nested").unwrap().find("inner").unwrap();
        assert_eq!(inner.text, "v");
        assert_eq!(inner.attribute("a"), Some("1"));
        assert_eq!(inner.attribute("b"), Some("x\"y"));
        assert!(movie.find("empty").unwrap().text.is_empty());
        assert!(
            movie.text.is_empty(),
            "whitespace between children is not text"
        );
    }

    #[test]
    fn cdata_is_text_and_unknown_entities_are_kept() {
        let roots = parse_roots("<m><a><![CDATA[x < y]]></a><b>caf&eacute;</b></m>");
        assert_eq!(roots[0].find("a").unwrap().text, "x < y");
        assert_eq!(roots[0].find("b").unwrap().text, "caf&eacute;");
    }

    #[test]
    fn a_misspelt_closing_tag_closes_the_innermost_and_an_outer_one_closes_inner_elements() {
        let roots = parse_roots("<m><a>1</b><c><d>2</c><e>3</e></m>");
        let m = &roots[0];
        assert_eq!(m.find("a").unwrap().text, "1");
        assert_eq!(m.find("c").unwrap().find("d").unwrap().text, "2");
        assert_eq!(m.find("e").unwrap().text, "3");
    }

    #[test]
    fn truncated_text_keeps_what_was_read() {
        let roots = parse_roots("<movie><title>Heat</title><plot>Cut o");
        assert_eq!(roots[0].find("title").unwrap().text, "Heat");
        assert_eq!(names(&roots), ["movie"]);
        assert!(parse_roots("").is_empty());
        assert!(parse_roots("not xml at all").is_empty());
    }

    #[test]
    fn several_roots_are_all_read() {
        let roots = parse_roots(
            "<episodedetails><title>A</title></episodedetails>\r\n<episodedetails><title>B</title></episodedetails>",
        );
        assert_eq!(names(&roots), ["episodedetails", "episodedetails"]);
    }

    #[test]
    fn a_stray_closing_tag_does_not_end_the_root() {
        let roots = parse_roots("<m><a>1</a></x><b>2</b></m>");
        assert_eq!(roots[0].find("a").unwrap().text, "1");
        assert_eq!(roots[0].find("b").unwrap().text, "2");
    }

    #[test]
    fn text_before_a_child_of_an_unclosed_tag_is_kept() {
        let roots = parse_roots("<m><year>1995<plot>x</plot></m>");
        let year = roots[0].find("year").unwrap();
        assert_eq!(year.text, "1995");
        assert_eq!(year.find("plot").unwrap().text, "x");
    }

    #[test]
    fn comments_inside_elements_with_children_are_kept() {
        let roots = parse_roots("<m><x><!--note--><y/></x></m>");
        let x = roots[0].find("x").unwrap();
        assert_eq!(x.children.len(), 2);
        assert_eq!(x.children[0], Node::Comment("note".to_owned()));
    }
}

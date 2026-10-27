//! Finding and reading the `__NEXT_DATA__` block of an IMDb page.
//!
//! IMDb's pages are rendered with Next.js, which embeds the data the page was built from as JSON
//! in `<script id="__NEXT_DATA__" type="application/json">`. Everything the provider reads comes
//! from there. Reading is strict about what a page must have to be usable and tolerant about
//! the rest: a missing required value is a [`ParseError`], never a half-filled result.

use serde_json::Value;

/// Why a page could not be read. All of these mean the page is not in the shape the provider
/// knows, and all of them surface as [`crate::ProviderError::FormatChanged`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The page has no `__NEXT_DATA__` block (a different page, such as a consent or bot check).
    #[error("the page has no __NEXT_DATA__ block")]
    NoNextData,
    /// The block is there but is not valid JSON.
    #[error("the __NEXT_DATA__ block is not valid JSON ({0})")]
    InvalidJson(String),
    /// A value the provider requires is missing or has the wrong type.
    #[error("the page data has no usable {0}")]
    Missing(&'static str),
}

const MARKER: &str = "__NEXT_DATA__";

/// The JSON of the page's `__NEXT_DATA__` block.
pub(super) fn extract(html: &str) -> Result<Value, ParseError> {
    let json = script_body(html).ok_or(ParseError::NoNextData)?;
    serde_json::from_str(json.trim()).map_err(|e| ParseError::InvalidJson(e.to_string()))
}

/// The text between the `<script ... id="__NEXT_DATA__" ...>` tag and its `</script>`.
fn script_body(html: &str) -> Option<&str> {
    let mut rest = html;
    while let Some(start) = rest.find("<script") {
        let after = &rest[start..];
        let tag_end = after.find('>')?;
        let tag = &after[..tag_end];
        let body = &after[tag_end + 1..];
        if tag.contains(MARKER) {
            let end = body.find("</script>")?;
            return Some(&body[..end]);
        }
        rest = body;
    }
    None
}

/// `props.pageProps` of the page data.
pub(super) fn page_props(data: &Value) -> Result<&Value, ParseError> {
    data.pointer("/props/pageProps")
        .filter(|v| v.is_object())
        .ok_or(ParseError::Missing("pageProps"))
}

/// A non-blank string at the JSON pointer `path`.
pub(super) fn text<'a>(value: &'a Value, path: &str) -> Option<&'a str> {
    value
        .pointer(path)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// A non-negative integer at `path`.
pub(super) fn number(value: &Value, path: &str) -> Option<u64> {
    value.pointer(path).and_then(Value::as_u64)
}

/// A floating point number at `path`.
pub(super) fn float(value: &Value, path: &str) -> Option<f64> {
    value.pointer(path).and_then(Value::as_f64)
}

/// The array at `path`, or an empty slice.
pub(super) fn list<'a>(value: &'a Value, path: &str) -> &'a [Value] {
    value
        .pointer(path)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_block_whatever_the_attribute_order() {
        let html = r#"<html><script>var a = 1;</script>
            <script type="application/json" id="__NEXT_DATA__">{"props":{"pageProps":{"a":1}}}</script></html>"#;
        let data = extract(html).unwrap();
        assert_eq!(number(page_props(&data).unwrap(), "/a"), Some(1));
    }

    #[test]
    fn a_page_without_the_block_is_reported() {
        assert_eq!(
            extract("<html><body>Are you a robot?</body></html>"),
            Err(ParseError::NoNextData)
        );
    }

    #[test]
    fn a_block_that_is_not_json_is_reported() {
        let html = r#"<script id="__NEXT_DATA__" type="application/json">{"props": </script>"#;
        assert!(matches!(extract(html), Err(ParseError::InvalidJson(_))));
    }

    #[test]
    fn an_unterminated_block_is_reported_as_missing() {
        let html = r#"<script id="__NEXT_DATA__" type="application/json">{"props":{}}"#;
        assert_eq!(extract(html), Err(ParseError::NoNextData));
    }

    #[test]
    fn page_props_must_be_an_object() {
        let data: Value = serde_json::json!({"props": {"pageProps": 3}});
        assert_eq!(page_props(&data), Err(ParseError::Missing("pageProps")));
    }

    #[test]
    fn helpers_ignore_blank_and_mistyped_values() {
        let v: Value = serde_json::json!({"a": "  ", "b": "x ", "c": 4, "d": [1]});
        assert_eq!(text(&v, "/a"), None);
        assert_eq!(text(&v, "/b"), Some("x"));
        assert_eq!(text(&v, "/c"), None);
        assert_eq!(number(&v, "/c"), Some(4));
        assert_eq!(list(&v, "/d").len(), 1);
        assert!(list(&v, "/c").is_empty());
    }
}

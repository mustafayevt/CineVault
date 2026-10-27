//! The values a template reads: a small dynamic model that callers build from library items.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::datetime::{DEFAULT_DATE_PATTERN, DateTime};

/// A model object whose fields are looked up by name when a template asks for them.
///
/// Implement this for library items so that a template only pays for the fields it reads.
pub trait Object: fmt::Debug + Send + Sync {
    /// The field `key`, or `None` when the object has no such field.
    fn get(&self, key: &str) -> Option<Value>;

    /// The text shown when a template prints the object itself.
    fn display(&self) -> String {
        String::new()
    }
}

/// A value in the template model.
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(Arc<str>),
    DateTime(DateTime),
    List(Arc<[Value]>),
    Map(Arc<BTreeMap<String, Value>>),
    Object(Arc<dyn Object>),
}

impl Value {
    pub fn text(text: impl AsRef<str>) -> Value {
        Value::Text(Arc::from(text.as_ref()))
    }

    pub fn list(items: impl IntoIterator<Item = Value>) -> Value {
        Value::List(items.into_iter().collect())
    }

    pub fn map<K: Into<String>>(entries: impl IntoIterator<Item = (K, Value)>) -> Value {
        Value::Map(Arc::new(
            entries.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        ))
    }

    pub fn object(object: impl Object + 'static) -> Value {
        Value::Object(Arc::new(object))
    }

    /// The field `name` of a map or object. Other values have no fields.
    pub fn get(&self, name: &str) -> Option<Value> {
        match self {
            Value::Map(map) => map.get(name).cloned(),
            Value::Object(object) => object.get(name),
            _ => None,
        }
    }

    /// Whether the value counts as true in `${if ...}`.
    ///
    /// Null, `false`, empty text, empty lists and empty maps are false. Every number is true,
    /// zero included: `${if year}` is true for year 0 even though `${year}` prints nothing
    /// in the renamer.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(_) | Value::Float(_) => true,
            Value::Text(t) => !t.is_empty(),
            Value::DateTime(_) | Value::Object(_) => true,
            Value::List(items) => !items.is_empty(),
            Value::Map(map) => !map.is_empty(),
        }
    }

    /// The text a template prints for the value. With `zero_is_empty`, numeric zero prints as
    /// nothing so that `Title (${year})` can be cleaned up by the renamer.
    pub fn to_text(&self, zero_is_empty: bool) -> String {
        match self {
            Value::Null | Value::Map(_) => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n) if zero_is_empty && *n == 0 => String::new(),
            Value::Int(n) => n.to_string(),
            Value::Float(f) if zero_is_empty && *f == 0.0 => String::new(),
            Value::Float(f) => plain_double(*f),
            Value::Text(t) => t.to_string(),
            Value::DateTime(d) => d.format(DEFAULT_DATE_PATTERN).unwrap_or_default(),
            // A bare list prints in brackets: `[a, b]`. An empty list prints nothing.
            Value::List(items) if items.is_empty() => String::new(),
            Value::List(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(|item| item.to_text(false))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Value::Object(object) => object.display(),
        }
    }

    /// The value as a number, for renderers that format numbers. Text that parses as a number
    /// counts as one.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) => Some(*f),
            Value::Bool(_) | Value::Null => None,
            Value::Text(t) => t.trim().parse().ok(),
            _ => None,
        }
    }

    /// The value as an integer; floats are truncated toward zero.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Float(f) if f.is_finite() => Some(f.trunc() as i64),
            Value::Text(t) => t.trim().parse().ok(),
            _ => None,
        }
    }

    /// The items of a list, one item for any other non-null value, none for null.
    pub fn items(&self) -> Vec<Value> {
        match self {
            Value::Null => Vec::new(),
            Value::List(items) => items.to_vec(),
            other => vec![other.clone()],
        }
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Value {
        Value::text(text)
    }
}

impl From<String> for Value {
    fn from(text: String) -> Value {
        Value::Text(Arc::from(text))
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Value {
        Value::Int(n)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Value {
        Value::Float(f)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

impl From<DateTime> for Value {
    fn from(d: DateTime) -> Value {
        Value::DateTime(d)
    }
}

/// Formats a floating-point number with at least one decimal digit,
/// and scientific notation below 1e-3 and from 1e7.
pub fn plain_double(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .to_owned();
    }
    let abs = value.abs();
    if (1e-3..1e7).contains(&abs) {
        let text = format!("{value}");
        if text.contains('.') {
            text
        } else {
            format!("{text}.0")
        }
    } else {
        // Shortest digits, then mantissa d.ddd and exponent.
        let sci = format!("{value:e}");
        let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
        let mantissa = if mantissa.contains('.') {
            mantissa.to_owned()
        } else {
            format!("{mantissa}.0")
        };
        format!("{mantissa}E{exp}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_print_with_one_decimal_or_exponent() {
        assert_eq!(plain_double(1.0), "1.0");
        assert_eq!(plain_double(23.976), "23.976");
        assert_eq!(plain_double(0.0), "0.0");
        assert_eq!(plain_double(1e7), "1.0E7");
        assert_eq!(plain_double(12_345_678.0), "1.2345678E7");
        assert_eq!(plain_double(0.0001), "1.0E-4");
        assert_eq!(plain_double(-2.5), "-2.5");
    }

    #[test]
    fn zero_prints_empty_only_when_asked() {
        assert_eq!(Value::Int(0).to_text(true), "");
        assert_eq!(Value::Int(0).to_text(false), "0");
        assert_eq!(Value::Float(0.0).to_text(true), "");
        assert_eq!(Value::Int(7).to_text(true), "7");
    }

    #[test]
    fn lists_print_in_brackets_unless_empty() {
        let list = Value::list([Value::text("a"), Value::Int(2)]);
        assert_eq!(list.to_text(false), "[a, 2]");
        assert_eq!(Value::list([]).to_text(false), "");
    }

    #[test]
    fn truthiness() {
        assert!(!Value::Null.is_truthy());
        assert!(!Value::text("").is_truthy());
        assert!(Value::text("x").is_truthy());
        assert!(Value::Int(0).is_truthy());
        assert!(Value::Float(0.0).is_truthy());
        assert!(!Value::list([]).is_truthy());
        assert!(Value::list([Value::Int(1)]).is_truthy());
    }
}

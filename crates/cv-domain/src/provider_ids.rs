//! Provider IDs: an open map from provider key to string value.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Well-known provider keys. Keys outside this list are accepted too.
pub mod id_key {
    pub const IMDB: &str = "imdb";
    pub const TMDB: &str = "tmdb";
    pub const TMDB_SET: &str = "tmdbSet";
    pub const TVDB: &str = "tvdb";
    pub const TRAKT: &str = "trakt";
    pub const ANIDB: &str = "anidb";
    pub const TVMAZE: &str = "tvmaze";
    pub const WIKIDATA: &str = "wikidata";
}

/// Provider IDs of one entity or person.
///
/// Empty, `"0"` and `"null"` values are dropped, so a stored ID is always meaningful. Numbers
/// are accepted on input and stored as their decimal text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderIds(BTreeMap<String, String>);

impl ProviderIds {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets an ID. Returns `false` (and removes any existing value for the key) when the key or
    /// value is blank, `"0"` or `"null"`.
    pub fn set(&mut self, key: &str, value: impl AsRef<str>) -> bool {
        let key = key.trim();
        let value = value.as_ref().trim();
        if key.is_empty() || is_empty_id(value) {
            self.0.remove(key);
            return false;
        }
        self.0.insert(key.to_owned(), value.to_owned());
        true
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// The ID parsed as a number, for providers with numeric IDs.
    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get(key)?.parse().ok()
    }

    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.0.remove(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

fn is_empty_id(value: &str) -> bool {
    value.is_empty() || value == "0" || value.eq_ignore_ascii_case("null")
}

impl<K: AsRef<str>, V: AsRef<str>> FromIterator<(K, V)> for ProviderIds {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut ids = Self::new();
        for (k, v) in iter {
            ids.set(k.as_ref(), v);
        }
        ids
    }
}

impl Serialize for ProviderIds {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

/// An ID value as it may appear in stored JSON: text, a number or null.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawId {
    Text(String),
    Int(i64),
    Uint(u64),
    Float(f64),
    Null(()),
}

impl RawId {
    fn into_text(self) -> String {
        match self {
            Self::Text(s) => s,
            Self::Int(n) => n.to_string(),
            Self::Uint(n) => n.to_string(),
            // Whole floats (e.g. 603.0) are integers that went through a float; keep them integral.
            Self::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => (f as i64).to_string(),
            Self::Float(f) => f.to_string(),
            Self::Null(()) => String::new(),
        }
    }
}

impl<'de> Deserialize<'de> for ProviderIds {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = BTreeMap::<String, RawId>::deserialize(d)?;
        Ok(raw.into_iter().map(|(k, v)| (k, v.into_text())).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_empty_zero_and_null() {
        let mut ids = ProviderIds::new();
        assert!(ids.set("imdb", "tt0078748"));
        assert!(!ids.set("tmdb", ""));
        assert!(!ids.set("tvdb", "0"));
        assert!(!ids.set("trakt", "null"));
        assert!(!ids.set("anidb", "NULL"));
        assert_eq!(ids.len(), 1);
        // Setting a blank value clears an existing one.
        assert!(!ids.set("imdb", "0"));
        assert!(ids.is_empty());
    }

    #[test]
    fn unknown_provider_key_round_trips() {
        let mut ids = ProviderIds::new();
        ids.set("kinopoisk", "12345");
        ids.set(id_key::TMDB_SET, "126125");
        let json = serde_json::to_string(&ids).unwrap();
        let back: ProviderIds = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ids);
        assert_eq!(back.get("kinopoisk"), Some("12345"));
        assert_eq!(back.get_u64(id_key::TMDB_SET), Some(126125));
    }

    #[test]
    fn accepts_numbers_nulls_and_drops_empties_on_input() {
        let ids: ProviderIds = serde_json::from_str(
            r#"{"tmdb": 603, "tvdb": 0, "imdb": "tt0133093", "trakt": null, "x": 12.0, "y": "", "z": "null"}"#,
        )
        .unwrap();
        assert_eq!(ids.get("tmdb"), Some("603"));
        assert_eq!(ids.get("imdb"), Some("tt0133093"));
        assert_eq!(ids.get("x"), Some("12"));
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn serialises_values_as_strings() {
        let ids: ProviderIds = [("tmdb", "603")].into_iter().collect();
        assert_eq!(serde_json::to_string(&ids).unwrap(), r#"{"tmdb":"603"}"#);
    }
}

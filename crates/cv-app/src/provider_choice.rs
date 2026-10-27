//! Which metadata provider scrapes what: the choices on the Providers page.
//!
//! | Key | Default | Meaning |
//! |---|---|---|
//! | `providers.metadata-movie` | first ready provider | the provider movies are matched and scraped with |
//! | `providers.metadata-show` | first ready provider | the provider TV shows are matched and scraped with |
//! | `providers.fallback-enabled` | off | when the provider finds nothing, try the fallback providers |
//! | `providers.fallback-order` | every eligible provider, in the default order | the fallback providers, tried in order |
//! | `providers.universal` | no choices | the [`UniversalConfig`] of the universal provider (search provider, one source per field, episode provider) |
//!
//! A choice that names a provider that is not ready (its key was removed) or does not handle the
//! kind is ignored, so the default is again the first ready provider and matching never stops for
//! a stale setting. The universal provider is listed beside the real ones and is chosen the same
//! way; Kodi XML scraper add-ons are listed too but never join the fallback chain.

use std::collections::BTreeMap;

use cv_providers::{FallbackChain, MetadataProvider, UNIVERSAL_ID, UniversalConfig};
use serde_json::Value;
use std::sync::Arc;

/// Setting: the provider for movies.
pub const METADATA_MOVIE_KEY: &str = "providers.metadata-movie";
/// Setting: the provider for TV shows.
pub const METADATA_SHOW_KEY: &str = "providers.metadata-show";
/// Setting: use the fallback providers.
pub const FALLBACK_ENABLED_KEY: &str = "providers.fallback-enabled";
/// Setting: the fallback providers, in order.
pub const FALLBACK_ORDER_KEY: &str = "providers.fallback-order";
/// Setting: the universal provider's configuration.
pub const UNIVERSAL_KEY: &str = "providers.universal";

/// The provider choices from the settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderChoices {
    /// The provider chosen for movies; `None` is automatic.
    pub movie: Option<String>,
    /// The provider chosen for TV shows; `None` is automatic.
    pub show: Option<String>,
    pub fallback_enabled: bool,
    /// The fallback providers in order. `None` (never set) means every eligible provider in the
    /// default order; `Some` is exactly what the user kept.
    pub fallback_order: Option<Vec<String>>,
    pub universal: UniversalConfig,
}

impl ProviderChoices {
    /// Reads the choices from a settings map; anything missing or malformed takes its default.
    #[must_use]
    pub fn from_values(values: &BTreeMap<String, Value>) -> Self {
        Self::from_lookup(|key| values.get(key).cloned())
    }

    /// Reads the choices through `get`.
    #[must_use]
    pub fn from_lookup(get: impl Fn(&str) -> Option<Value>) -> Self {
        let text = |key: &str| {
            get(key)
                .and_then(|v| v.as_str().map(|s| s.trim().to_owned()))
                .filter(|s| !s.is_empty())
        };
        let fallback_order = get(FALLBACK_ORDER_KEY).and_then(|v| match v {
            Value::Array(items) => {
                let mut order: Vec<String> = Vec::new();
                for id in items.iter().filter_map(Value::as_str) {
                    let id = id.trim();
                    if !id.is_empty() && !order.iter().any(|o| o == id) {
                        order.push(id.to_owned());
                    }
                }
                Some(order)
            }
            _ => None,
        });
        Self {
            movie: text(METADATA_MOVIE_KEY),
            show: text(METADATA_SHOW_KEY),
            fallback_enabled: get(FALLBACK_ENABLED_KEY)
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            fallback_order,
            universal: get(UNIVERSAL_KEY)
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
        }
    }

    /// The fallback chain over the ready providers `ready` (in default order) of which
    /// `eligible` may join. Empty when fallback is off.
    #[must_use]
    pub fn chain(
        &self,
        ready: &[Arc<dyn MetadataProvider>],
        eligible: impl Fn(&str) -> bool,
    ) -> FallbackChain {
        if !self.fallback_enabled {
            return FallbackChain::disabled();
        }
        let usable = |p: &&Arc<dyn MetadataProvider>| p.id() != UNIVERSAL_ID && eligible(p.id());
        let ordered: Vec<Arc<dyn MetadataProvider>> = match &self.fallback_order {
            None => ready.iter().filter(usable).cloned().collect(),
            Some(order) => order
                .iter()
                .filter_map(|id| ready.iter().find(|p| p.id() == id))
                .filter(usable)
                .cloned()
                .collect(),
        };
        FallbackChain::new(ordered)
    }
}

/// One metadata provider as the Providers page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataProviderInfo {
    pub id: String,
    pub name: String,
    pub movies: bool,
    pub shows: bool,
    /// Its credentials are in place (or it needs none), so it can be asked.
    pub ready: bool,
    /// A Kodi XML scraper add-on.
    pub kodi: bool,
    /// It may be part of the fallback chain.
    pub can_fallback: bool,
}

/// A Kodi add-on scraper that is listed but cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedAddon {
    pub addon_id: String,
    pub addon_name: String,
    /// `movies` or `tvshows`.
    pub content: String,
    pub reason: String,
}

/// What the Providers page needs to offer the choices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderCatalogue {
    /// Every metadata provider, in the default order, ready or not. The universal provider is
    /// last.
    pub providers: Vec<MetadataProviderInfo>,
    /// Add-ons found in the Kodi folders that cannot run, with the reason.
    pub unsupported_addons: Vec<UnsupportedAddon>,
    /// The fields the universal provider can take from a chosen source (`MetaField::key`).
    pub universal_fields: Vec<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn values(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn nothing_stored_means_automatic_and_no_fallback() {
        let choices = ProviderChoices::from_values(&BTreeMap::new());
        assert_eq!(choices, ProviderChoices::default());
        assert!(!choices.fallback_enabled);
        assert_eq!(choices.fallback_order, None);
    }

    #[test]
    fn the_choices_are_read_and_malformed_values_are_ignored() {
        let choices = ProviderChoices::from_values(&values(&[
            (METADATA_MOVIE_KEY, json!(" tmdb ")),
            (METADATA_SHOW_KEY, json!("tvdb")),
            (FALLBACK_ENABLED_KEY, json!(true)),
            (FALLBACK_ORDER_KEY, json!(["ofdb", "ofdb", "", "imdb", 7])),
            (
                UNIVERSAL_KEY,
                json!({"search": "tmdb", "fields": {"plot": "imdb"}}),
            ),
        ]));
        assert_eq!(choices.movie.as_deref(), Some("tmdb"));
        assert_eq!(choices.show.as_deref(), Some("tvdb"));
        assert!(choices.fallback_enabled);
        assert_eq!(
            choices.fallback_order,
            Some(vec!["ofdb".to_owned(), "imdb".to_owned()])
        );
        assert_eq!(choices.universal.search, "tmdb");
        assert_eq!(choices.universal.fields.len(), 1);

        let bad = ProviderChoices::from_values(&values(&[
            (METADATA_MOVIE_KEY, json!(5)),
            (FALLBACK_ENABLED_KEY, json!("yes")),
            (FALLBACK_ORDER_KEY, json!("ofdb")),
            (UNIVERSAL_KEY, json!([1, 2])),
        ]));
        assert_eq!(bad, ProviderChoices::default());
    }
}

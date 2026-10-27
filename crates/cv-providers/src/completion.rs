//! Cross-provider ID completion.
//!
//! After a scrape an item usually has the IDs of the provider it came from and a few the provider
//! reported. [`complete_ids`] fills the IDs that are still missing (IMDb, TMDB, TVDB, Trakt and,
//! for movies, the collection ID) from the providers that know them:
//!
//! 1. For anime, the offline [`AnimeCrossReference`] maps one anime ID to the others without any
//!    request. It adds every ID of the matching row, including the IDs other anime sites use.
//! 2. Each provider that can resolve the title from an ID the item already has is asked for its
//!    details, in the order given, until nothing is missing. A provider that has no such title
//!    is skipped; one that fails is skipped and the failure is reported, never raised.
//!
//! An ID that is already set is never changed: completion only adds.

use std::collections::BTreeMap;

use cv_domain::{ProviderIds, id_key};
use serde::Deserialize;

use crate::error::ProviderError;
use crate::model::{MediaKind, RequestOptions};
use crate::traits::MetadataProvider;

/// The ID keys completion fills for a title of `kind`.
pub fn wanted_keys(kind: MediaKind) -> &'static [&'static str] {
    match kind {
        MediaKind::Movie => &[
            id_key::IMDB,
            id_key::TMDB,
            id_key::TVDB,
            id_key::TRAKT,
            id_key::TMDB_SET,
        ],
        MediaKind::Show => &[id_key::IMDB, id_key::TMDB, id_key::TVDB, id_key::TRAKT],
    }
}

/// The wanted keys that `ids` does not have yet.
pub fn missing_keys(kind: MediaKind, ids: &ProviderIds) -> Vec<&'static str> {
    wanted_keys(kind)
        .iter()
        .copied()
        .filter(|key| !ids.contains(key))
        .collect()
}

/// Where a filled ID came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdSource {
    /// The offline anime ID cross-reference.
    AnimeCrossReference,
    /// A provider, by key (for example `tmdb`).
    Provider(String),
}

/// One ID that completion added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedId {
    pub key: String,
    pub value: String,
    pub source: IdSource,
}

/// What completion did.
#[derive(Debug, Clone, Default)]
pub struct CompletedIds {
    /// The IDs after completion: the given ones plus [`Self::added`].
    pub ids: ProviderIds,
    pub added: Vec<AddedId>,
    /// Providers that failed (key and error). Their IDs are simply missing.
    pub failures: Vec<(String, ProviderError)>,
}

/// An offline map between the IDs one anime has at different services.
///
/// Entries are rows of IDs for one anime; any ID in a row finds the others. The rows come from a
/// downloaded crosswalk file (see [`AnimeCrossReference::from_json`]); an empty map does nothing.
#[derive(Debug, Clone, Default)]
pub struct AnimeCrossReference {
    rows: Vec<ProviderIds>,
    /// `(key, value)` to the index of the first row that has it.
    index: BTreeMap<(String, String), usize>,
}

/// One row of the crosswalk file: IDs by key, as numbers or text.
#[derive(Deserialize)]
struct Row(ProviderIds);

impl AnimeCrossReference {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the map from rows of IDs. A row with fewer than two IDs links nothing and is
    /// dropped.
    pub fn from_rows(rows: impl IntoIterator<Item = ProviderIds>) -> Self {
        let mut map = Self::default();
        for row in rows.into_iter().filter(|row| row.len() > 1) {
            let at = map.rows.len();
            for (key, value) in row.iter() {
                map.index
                    .entry((key.to_owned(), value.to_owned()))
                    .or_insert(at);
            }
            map.rows.push(row);
        }
        map
    }

    /// Reads a JSON array of rows such as `[{"anidb": 1, "tvdb": 76885, "imdb": "tt0000001"}]`.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let rows: Vec<Row> = serde_json::from_str(json)?;
        Ok(Self::from_rows(rows.into_iter().map(|row| row.0)))
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Whether any ID of `ids` is in the map.
    pub fn knows(&self, ids: &ProviderIds) -> bool {
        self.row_for(ids).is_some()
    }

    /// The first row that shares an ID with `ids`.
    fn row_for(&self, ids: &ProviderIds) -> Option<&ProviderIds> {
        ids.iter()
            .find_map(|(key, value)| self.index.get(&(key.to_owned(), value.to_owned())))
            .map(|at| &self.rows[*at])
    }

    /// The IDs the map knows for the anime `ids` identifies. Empty when it is not in the map.
    pub fn lookup(&self, ids: &ProviderIds) -> ProviderIds {
        self.row_for(ids).cloned().unwrap_or_default()
    }
}

/// Fills the missing IDs of a `kind` title.
///
/// `providers` are asked in order. `anime` is consulted first when it knows the title.
pub async fn complete_ids(
    kind: MediaKind,
    ids: &ProviderIds,
    providers: &[&dyn MetadataProvider],
    anime: Option<&AnimeCrossReference>,
    options: &RequestOptions,
) -> CompletedIds {
    let mut done = CompletedIds {
        ids: ids.clone(),
        ..CompletedIds::default()
    };

    if let Some(anime) = anime.filter(|a| a.knows(&done.ids)) {
        // Everything the row knows is added, the other anime sites' IDs (AniDB, MyAnimeList,
        // AniList, Kitsu, ...) as well as any IMDb, TMDB, TVDB or Trakt ID it carries: a provider
        // that understands one of them can then resolve the title.
        for (key, value) in anime.lookup(&done.ids).iter() {
            if !done.ids.contains(key) && done.ids.set(key, value) {
                done.added.push(AddedId {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    source: IdSource::AnimeCrossReference,
                });
            }
        }
    }

    for provider in providers {
        if missing_keys(kind, &done.ids).is_empty() {
            break;
        }
        if !can_resolve(*provider, kind, &done.ids) {
            continue;
        }
        match provider.details(kind, &done.ids, options).await {
            Ok(item) => merge(
                kind,
                &item.core.ids,
                &IdSource::Provider(provider.id().to_owned()),
                &mut done,
            ),
            // The provider does not have the title: nothing to add, nothing wrong.
            Err(ProviderError::NotFound(_)) => {}
            Err(error) => {
                tracing::warn!(provider = provider.id(), %error, "ID completion failed");
                done.failures.push((provider.id().to_owned(), error));
            }
        }
    }
    done
}

/// The provider handles this kind and `ids` has an ID it can look up.
pub(crate) fn can_resolve(
    provider: &dyn MetadataProvider,
    kind: MediaKind,
    ids: &ProviderIds,
) -> bool {
    let capabilities = provider.capabilities();
    let kind_ok = match kind {
        MediaKind::Movie => capabilities.movies,
        MediaKind::Show => capabilities.shows,
    };
    kind_ok && capabilities.lookup_ids.iter().any(|key| ids.contains(key))
}

/// Adds the wanted IDs of `found` that `done` lacks.
fn merge(kind: MediaKind, found: &ProviderIds, source: &IdSource, done: &mut CompletedIds) {
    for key in missing_keys(kind, &done.ids) {
        if let Some(value) = found.get(key)
            && done.ids.set(key, value)
        {
            done.added.push(AddedId {
                key: key.to_owned(),
                value: value.to_owned(),
                source: source.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(pairs: &[(&str, &str)]) -> ProviderIds {
        pairs.iter().copied().collect()
    }

    #[test]
    fn collection_ids_are_wanted_for_movies_only() {
        assert!(wanted_keys(MediaKind::Movie).contains(&id_key::TMDB_SET));
        assert!(!wanted_keys(MediaKind::Show).contains(&id_key::TMDB_SET));
        assert_eq!(
            missing_keys(MediaKind::Show, &ids(&[("tmdb", "1")])),
            vec!["imdb", "tvdb", "trakt"]
        );
    }

    #[test]
    fn the_cross_reference_finds_a_row_by_any_of_its_ids() {
        let map = AnimeCrossReference::from_json(
            r#"[{"anidb": 23, "tvdb": 76885, "imdb": "tt0213338"},
                {"anidb": 4, "tmdb": 55}, {"anidb": 99}]"#,
        )
        .unwrap();
        // A row of one ID says nothing, so it is dropped.
        assert_eq!(map.len(), 2);

        let by_tvdb = map.lookup(&ids(&[("tvdb", "76885")]));
        assert_eq!(by_tvdb.get("anidb"), Some("23"));
        assert_eq!(by_tvdb.get("imdb"), Some("tt0213338"));
        assert!(map.knows(&ids(&[("anidb", "4")])));
        assert!(!map.knows(&ids(&[("anidb", "99")])));
        assert!(map.lookup(&ids(&[("anidb", "5")])).is_empty());
    }

    #[test]
    fn the_cross_reference_adds_the_other_anime_site_ids_too() {
        let map = AnimeCrossReference::from_json(
            r#"[{"myanimelist": 1535, "anidb": 4563, "anilist": 1535, "imdb": "tt0877057"}]"#,
        )
        .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let done = rt.block_on(complete_ids(
            MediaKind::Show,
            &ids(&[("myanimelist", "1535"), ("imdb", "tt9999999")]),
            &[],
            Some(&map),
            &RequestOptions::default(),
        ));
        assert_eq!(done.ids.get("anidb"), Some("4563"));
        assert_eq!(done.ids.get("anilist"), Some("1535"));
        // An ID the item already has is never changed.
        assert_eq!(done.ids.get("imdb"), Some("tt9999999"));
        let keys: Vec<_> = done.added.iter().map(|a| a.key.as_str()).collect();
        assert_eq!(keys, ["anidb", "anilist"]);
    }

    #[test]
    fn merge_only_adds_wanted_missing_ids() {
        let mut done = CompletedIds {
            ids: ids(&[("tmdb", "1"), ("imdb", "tt1")]),
            ..CompletedIds::default()
        };
        let found = ids(&[
            ("tmdb", "2"),
            ("imdb", "tt2"),
            ("tvdb", "3"),
            ("wikidata", "Q1"),
        ]);
        merge(
            MediaKind::Show,
            &found,
            &IdSource::Provider("x".to_owned()),
            &mut done,
        );
        assert_eq!(done.ids.get("tmdb"), Some("1"));
        assert_eq!(done.ids.get("imdb"), Some("tt1"));
        assert_eq!(done.ids.get("tvdb"), Some("3"));
        assert!(!done.ids.contains("wikidata"));
        assert_eq!(done.added.len(), 1);
    }
}

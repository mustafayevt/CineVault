//! One list out of many sources: the same release from several sources and queries
//! becomes one row.
//!
//! Two raw releases are the same when their info-hashes match (hex and base32 spell one hash).
//! When either has no hash they are the same when their normalised names match and their sizes
//! are within [`SIZE_TOLERANCE`] (1 MiB). A merged row keeps every source that returned it, the
//! highest seeder and leecher counts, the earliest date and the first magnet or `.torrent` link
//! seen; a magnet link is built from the info-hash and the public trackers when no source gave
//! one.
//!
//! Rows are identified by `id` (the hash, or a name-and-size key). When a late arrival links a
//! hashless row to a hash, the row takes the hash as its id and lists the old id in `absorbs`,
//! so a consumer that replaces rows by id can drop the old one.

use std::collections::HashMap;

use crate::magnet::{PUBLIC_TRACKERS, build_magnet, normalise_info_hash, validate_magnet};
use crate::model::{RawRelease, Release};
use crate::query::normalise;

/// Sizes this close (in bytes) are taken as the same file when no hash is known.
pub const SIZE_TOLERANCE: u64 = 1024 * 1024;

/// A merged release as it leaves the merge: the row and the ids it replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedRow {
    pub id: String,
    /// Ids of rows that were folded into this one at some point. A consumer drops them.
    pub absorbs: Vec<String>,
    pub release: Release,
}

#[derive(Debug)]
struct Entry {
    absorbs: Vec<String>,
    release: Release,
}

/// The merge of one search. Cheap to keep for the whole search; not shared between searches.
#[derive(Debug, Default)]
pub struct MergeState {
    entries: Vec<Entry>,
    by_id: HashMap<String, usize>,
    by_name: HashMap<String, Vec<usize>>,
}

/// FNV-1a, enough to shorten a normalised name into a stable key.
fn fnv(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn within_tolerance(a: Option<u64>, b: Option<u64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.abs_diff(b) <= SIZE_TOLERANCE,
        // Two releases that both lack a size are told apart by their names only.
        (None, None) => true,
        _ => false,
    }
}

impl MergeState {
    /// An empty merge.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many distinct releases there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing was added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every row, in the order the releases were first seen.
    #[must_use]
    pub fn rows(&self) -> Vec<MergedRow> {
        self.entries.iter().map(Self::row_of).collect()
    }

    fn row_of(entry: &Entry) -> MergedRow {
        MergedRow {
            id: entry.release.id.clone(),
            absorbs: entry.absorbs.clone(),
            release: entry.release.clone(),
        }
    }

    /// The row with this id, if any.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<MergedRow> {
        self.by_id.get(id).map(|&i| Self::row_of(&self.entries[i]))
    }

    /// Adds a release and returns the row it ended up in (always one row; empty only when the
    /// release has no name, no hash and no link to say what it is).
    pub fn add(&mut self, raw: RawRelease) -> Vec<MergedRow> {
        let hash = raw
            .info_hash
            .as_deref()
            .and_then(normalise_info_hash)
            .or_else(|| {
                raw.magnet
                    .as_deref()
                    .and_then(|m| validate_magnet(m).ok())
                    .map(|m| m.info_hash)
            });
        let normalised_name = normalise(&raw.name);
        if hash.is_none() && normalised_name.is_empty() {
            return Vec::new();
        }

        let index = match &hash {
            Some(hash) => self.place_hashed(hash, &normalised_name, raw.size),
            None => self.place_hashless(&normalised_name, raw.size),
        };
        let index = index.unwrap_or_else(|| self.create(hash.as_deref(), &normalised_name, &raw));
        self.fold(index, raw, hash);
        vec![Self::row_of(&self.entries[index])]
    }

    /// Adds many releases and returns the rows they touched, once each, in first-touch order,
    /// with each row as it is after the last addition.
    pub fn add_all(&mut self, raws: impl IntoIterator<Item = RawRelease>) -> Vec<MergedRow> {
        let mut touched: Vec<usize> = Vec::new();
        for raw in raws {
            let rows = self.add(raw);
            if let Some(row) = rows.first()
                && let Some(&index) = self.by_id.get(&row.id)
                && !touched.contains(&index)
            {
                touched.push(index);
            }
        }
        touched
            .into_iter()
            .map(|i| Self::row_of(&self.entries[i]))
            .collect()
    }

    /// The row a hashed release belongs to: the row with that hash, else a hashless row with the
    /// same name and a size within tolerance.
    fn place_hashed(&self, hash: &str, name: &str, size: Option<u64>) -> Option<usize> {
        if let Some(&i) = self.by_id.get(hash) {
            return Some(i);
        }
        self.by_name.get(name)?.iter().copied().find(|&i| {
            let release = &self.entries[i].release;
            release.info_hash.is_none() && within_tolerance(release.size, size)
        })
    }

    /// The row a hashless release belongs to: any row with the same name and a size within
    /// tolerance (a hashed one is preferred).
    fn place_hashless(&self, name: &str, size: Option<u64>) -> Option<usize> {
        let fits: Vec<usize> = self
            .by_name
            .get(name)?
            .iter()
            .copied()
            .filter(|&i| within_tolerance(self.entries[i].release.size, size))
            .collect();
        fits.iter()
            .copied()
            .find(|&i| self.entries[i].release.info_hash.is_some())
            .or_else(|| fits.first().copied())
    }

    fn create(&mut self, hash: Option<&str>, normalised_name: &str, raw: &RawRelease) -> usize {
        let mut id = match hash {
            Some(hash) => hash.to_owned(),
            None => format!(
                "n:{:016x}:{}",
                fnv(normalised_name),
                raw.size.map_or(0, |size| size / SIZE_TOLERANCE)
            ),
        };
        // Names within one size bucket always merge, so a clash is only possible after a rename
        // to a hash; the counter keeps ids unique regardless.
        while self.by_id.contains_key(&id) {
            id.push('+');
        }
        let index = self.entries.len();
        self.entries.push(Entry {
            absorbs: Vec::new(),
            release: Release {
                id: id.clone(),
                name: raw.name.trim().to_owned(),
                ..Release::default()
            },
        });
        self.by_id.insert(id, index);
        self.by_name
            .entry(normalised_name.to_owned())
            .or_default()
            .push(index);
        index
    }

    fn fold(&mut self, index: usize, raw: RawRelease, hash: Option<String>) {
        // A hashless row that now has a hash takes it as its id.
        if let Some(hash) = &hash
            && self.entries[index].release.info_hash.is_none()
        {
            let old = self.entries[index].release.id.clone();
            if old != *hash {
                self.by_id.remove(&old);
                self.by_id.insert(hash.clone(), index);
                let entry = &mut self.entries[index];
                entry.absorbs.push(old);
                entry.release.id.clone_from(hash);
            }
            self.entries[index].release.info_hash = Some(hash.clone());
        }

        let entry = &mut self.entries[index];
        let release = &mut entry.release;
        if release.name.is_empty() {
            release.name = raw.name.trim().to_owned();
        }
        if let Some(source) = raw.source {
            if let Some(url) = raw.details_url.as_deref().and_then(crate::model::web_page)
                && !release.pages.iter().any(|page| page.source == source)
            {
                release.pages.push(crate::model::SourcePage {
                    source: source.clone(),
                    url,
                });
            }
            if !release.sources.contains(&source) {
                release.sources.push(source);
            }
        }
        release.seeders = release.seeders.max(raw.seeders);
        release.leechers = release.leechers.max(raw.leechers);
        release.published = match (release.published, raw.published) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        release.size = release.size.or(raw.size);
        if release.torrent_url.is_none() {
            release.torrent_url = raw.torrent_url.filter(|url| !url.trim().is_empty());
        }
        if release.imdb.is_none() {
            release.imdb = raw.imdb;
        }
        release.season = release.season.or(raw.season);
        release.episode = release.episode.or(raw.episode);
        if release.category.is_none() {
            release.category = raw.category;
        }
        for file in raw.files {
            if !release.files.contains(&file) {
                release.files.push(file);
            }
        }
        // The first magnet link a source gave, when it is a usable one.
        if release.magnet.is_none()
            && let Some(magnet) = raw.magnet
            && validate_magnet(&magnet).is_ok()
        {
            release.magnet = Some(magnet);
        }
        if release.magnet.is_none()
            && let Some(hash) = &release.info_hash
        {
            release.magnet = build_magnet(hash, &release.name, &PUBLIC_TRACKERS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceId;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    fn raw(source: &str, name: &str, hash: Option<&str>, size: Option<u64>) -> RawRelease {
        RawRelease {
            source: Some(SourceId(source.into())),
            name: name.into(),
            info_hash: hash.map(str::to_owned),
            size,
            seeders: Some(1),
            ..RawRelease::default()
        }
    }

    #[test]
    fn a_merged_release_keeps_one_page_per_source_that_gave_one() {
        let mut state = MergeState::new();
        let page = |source: &str, url: Option<&str>| RawRelease {
            details_url: url.map(str::to_owned),
            ..raw(source, "Movie.2020.1080p", Some(HASH), None)
        };
        state.add(page("torrentio", None));
        state.add(page(
            "apibay",
            Some("https://thepiratebay.org/description.php?id=1"),
        ));
        // A second row of the same source keeps the first page; an address that is not a web page
        // (a magnet link, a relative path) is no page.
        state.add(page(
            "apibay",
            Some("https://thepiratebay.org/description.php?id=2"),
        ));
        state.add(page("knaben", Some("magnet:?xt=urn:btih:abc")));
        let rows = state.add(page("yts", Some("https://yts.gg/movies/x")));
        let pages: Vec<_> = rows[0]
            .release
            .pages
            .iter()
            .map(|p| (p.source.0.as_str(), p.url.as_str()))
            .collect();
        assert_eq!(
            pages,
            [
                ("apibay", "https://thepiratebay.org/description.php?id=1"),
                ("yts", "https://yts.gg/movies/x"),
            ]
        );
        assert_eq!(rows[0].release.sources.len(), 4);
    }

    #[test]
    fn three_sources_with_one_hash_are_one_row() {
        let mut state = MergeState::new();
        state.add(raw(
            "torrentio",
            "Movie.2020.1080p",
            Some(HASH),
            Some(2_000_000_000),
        ));
        state.add(raw(
            "apibay",
            "Movie 2020 1080p",
            Some(&HASH.to_uppercase()),
            Some(2_000_000_000),
        ));
        let rows = state.add(raw("knaben", "movie.2020.1080p", Some(HASH), None));
        assert_eq!(state.len(), 1);
        let release = &rows[0].release;
        assert_eq!(release.sources.len(), 3);
        assert_eq!(release.size, Some(2_000_000_000));
        assert_eq!(rows[0].id, HASH);
    }

    #[test]
    fn base32_and_hex_hashes_are_one_row() {
        let b32 = data_encoding::BASE32_NOPAD.encode(
            &(0..20)
                .map(|i| u8::try_from(i * 7).unwrap())
                .collect::<Vec<_>>(),
        );
        let hex: String = (0..20).map(|i| format!("{:02x}", i * 7)).collect();
        let mut state = MergeState::new();
        state.add(raw("a", "Movie.2020", Some(&b32), Some(10)));
        let rows = state.add(raw("b", "Movie.2020", Some(&hex), Some(10)));
        assert_eq!(state.len(), 1);
        assert_eq!(rows[0].id, hex);
    }

    #[test]
    fn different_hashes_stay_apart_even_with_the_same_name_and_size() {
        let mut state = MergeState::new();
        state.add(raw("a", "Movie.2020", Some(HASH), Some(10)));
        state.add(raw(
            "b",
            "Movie.2020",
            Some("ffffffffffffffffffffffffffffffffffffffff"),
            Some(10),
        ));
        assert_eq!(state.len(), 2);
    }

    #[test]
    fn hashless_rows_merge_by_name_and_size_within_one_mib() {
        let mut state = MergeState::new();
        state.add(raw("a", "Movie 2020 1080p", None, Some(2_000_000_000)));
        state.add(raw(
            "b",
            "Movie.2020.1080p",
            None,
            Some(2_000_000_000 + 1024 * 1024),
        ));
        assert_eq!(state.len(), 1);
        state.add(raw(
            "c",
            "Movie.2020.1080p",
            None,
            Some(2_000_000_000 + 1024 * 1024 + 1),
        ));
        assert_eq!(
            state.len(),
            2,
            "one byte past the tolerance is another file"
        );
        state.add(raw("d", "Other.Movie", None, Some(2_000_000_000)));
        assert_eq!(state.len(), 3);
    }

    #[test]
    fn missing_sizes_merge_only_with_missing_sizes() {
        let mut state = MergeState::new();
        state.add(raw("a", "Movie.2020", None, None));
        state.add(raw("b", "Movie.2020", None, None));
        assert_eq!(state.len(), 1);
        state.add(raw("c", "Movie.2020", None, Some(5)));
        assert_eq!(state.len(), 2);
    }

    #[test]
    fn a_late_hash_renames_the_hashless_row_and_absorbs_its_id() {
        let mut state = MergeState::new();
        let first = state.add(raw("a", "Movie.2020.1080p", None, Some(1_500_000_000)));
        let old_id = first[0].id.clone();
        assert!(old_id.starts_with("n:"));
        assert_eq!(first[0].release.info_hash, None);

        let rows = state.add(raw(
            "b",
            "Movie 2020 1080p",
            Some(HASH),
            Some(1_500_000_000),
        ));
        assert_eq!(state.len(), 1);
        assert_eq!(rows[0].id, HASH);
        assert_eq!(rows[0].absorbs, vec![old_id.clone()]);
        assert_eq!(rows[0].release.info_hash.as_deref(), Some(HASH));
        assert!(state.get(&old_id).is_none());
        assert!(state.get(HASH).is_some());
        assert!(rows[0].release.magnet.as_deref().unwrap().contains(HASH));
    }

    #[test]
    fn a_hashless_arrival_joins_a_hashed_row() {
        let mut state = MergeState::new();
        state.add(raw(
            "a",
            "Movie.2020.1080p",
            Some(HASH),
            Some(1_500_000_000),
        ));
        let rows = state.add(raw("b", "Movie 2020 1080p", None, Some(1_500_000_100)));
        assert_eq!(state.len(), 1);
        assert_eq!(rows[0].id, HASH);
        assert_eq!(rows[0].release.sources.len(), 2);
    }

    #[test]
    fn the_highest_seeders_and_the_earliest_date_win() {
        let mut state = MergeState::new();
        let mut a = raw("a", "Movie.2020", Some(HASH), Some(10));
        a.seeders = Some(5);
        a.leechers = Some(9);
        a.published = Some("2024-03-02T00:00:00Z".parse().unwrap());
        let mut b = raw("b", "Movie.2020", Some(HASH), Some(10));
        b.seeders = Some(40);
        b.leechers = Some(2);
        b.published = Some("2024-03-01T00:00:00Z".parse().unwrap());
        let mut c = raw("c", "Movie.2020", Some(HASH), Some(10));
        c.seeders = None;
        state.add(a);
        state.add(b);
        let rows = state.add(c);
        let release = &rows[0].release;
        assert_eq!(release.seeders, Some(40));
        assert_eq!(release.leechers, Some(9));
        assert_eq!(
            release.published,
            Some("2024-03-01T00:00:00Z".parse().unwrap())
        );
    }

    #[test]
    fn a_source_listed_twice_is_listed_once() {
        let mut state = MergeState::new();
        state.add(raw("a", "Movie.2020", Some(HASH), Some(10)));
        let rows = state.add(raw("a", "Movie.2020", Some(HASH), Some(10)));
        assert_eq!(rows[0].release.sources, vec![SourceId("a".into())]);
    }

    #[test]
    fn the_first_usable_magnet_is_kept_and_one_is_built_when_none_given() {
        let mut state = MergeState::new();
        let mut a = raw("a", "Movie.2020", Some(HASH), Some(10));
        a.magnet = Some("javascript:alert(1)".into());
        let rows = state.add(a);
        let built = rows[0].release.magnet.clone().unwrap();
        assert!(built.starts_with(&format!("magnet:?xt=urn:btih:{HASH}&dn=Movie.2020")));
        assert!(built.contains("&tr="));

        let mut b = raw(
            "b",
            "Other",
            Some("ffffffffffffffffffffffffffffffffffffffff"),
            Some(10),
        );
        b.magnet =
            Some("magnet:?xt=urn:btih:ffffffffffffffffffffffffffffffffffffffff&dn=Mine".into());
        let rows = state.add(b);
        assert!(
            rows[0]
                .release
                .magnet
                .as_deref()
                .unwrap()
                .ends_with("dn=Mine")
        );
    }

    #[test]
    fn the_hash_is_taken_from_a_magnet_when_the_source_gave_none() {
        let mut state = MergeState::new();
        let mut a = raw("a", "Movie.2020", None, Some(10));
        a.magnet = Some(format!("magnet:?xt=urn:btih:{HASH}&dn=x"));
        let rows = state.add(a);
        assert_eq!(rows[0].id, HASH);
    }

    #[test]
    fn a_torrent_url_without_a_hash_is_kept_as_a_hashless_row() {
        let mut state = MergeState::new();
        let mut a = raw("a", "Movie.2020", None, Some(10));
        a.torrent_url = Some("https://x.example/a.torrent".into());
        let rows = state.add(a);
        assert_eq!(rows[0].release.magnet, None);
        assert_eq!(
            rows[0].release.torrent_url.as_deref(),
            Some("https://x.example/a.torrent")
        );
    }

    #[test]
    fn a_release_with_nothing_to_identify_it_is_dropped() {
        let mut state = MergeState::new();
        assert!(state.add(raw("a", "  ", None, Some(10))).is_empty());
        assert!(state.add(raw("a", "...", None, Some(10))).is_empty());
        assert!(state.is_empty());
    }

    #[test]
    fn a_bad_hash_is_ignored_not_trusted() {
        let mut state = MergeState::new();
        let rows = state.add(raw("a", "Movie.2020", Some("not-a-hash"), Some(10)));
        assert_eq!(rows[0].release.info_hash, None);
        assert!(rows[0].id.starts_with("n:"));
    }

    #[test]
    fn add_all_returns_each_touched_row_once_in_the_state_it_ended_in() {
        let mut state = MergeState::new();
        state.add(raw(
            "x",
            "Untouched.Movie",
            Some("ffffffffffffffffffffffffffffffffffffffff"),
            Some(1),
        ));
        let rows = state.add_all(vec![
            raw("a", "Movie.2020", Some(HASH), Some(10)),
            raw(
                "b",
                "Other.Movie",
                Some("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
                Some(11),
            ),
            raw("c", "Movie.2020", Some(HASH), Some(10)),
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, HASH);
        assert_eq!(rows[0].release.sources.len(), 2);
        assert_eq!(state.len(), 3);
    }

    #[test]
    fn rows_come_back_in_first_seen_order() {
        let mut state = MergeState::new();
        state.add(raw("a", "B.Movie", Some(HASH), Some(1)));
        state.add(raw(
            "a",
            "A.Movie",
            Some("ffffffffffffffffffffffffffffffffffffffff"),
            Some(2),
        ));
        let names: Vec<_> = state.rows().into_iter().map(|r| r.release.name).collect();
        assert_eq!(names, vec!["B.Movie", "A.Movie"]);
    }
}

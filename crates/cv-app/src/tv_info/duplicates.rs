//! Duplicate episodes: two files that map to the same episode number.

use std::collections::BTreeMap;

/// The numbers that more than one entry claims, with how many claim each.
///
/// `entries` are the shown `(season, episode)` numbers of the episodes in the show's ordering;
/// episodes without a valid number are not passed in (they are listed as uncategorised instead).
/// A multi-episode file is one entry per number it covers, so it clashes only when another file
/// covers one of the same numbers.
#[must_use]
pub fn duplicate_numbers<N: Ord>(entries: impl IntoIterator<Item = N>) -> BTreeMap<N, u32> {
    let mut counts: BTreeMap<N, u32> = BTreeMap::new();
    for number in entries {
        *counts.entry(number).or_default() += 1;
    }
    counts.retain(|_, count| *count > 1);
    counts
}

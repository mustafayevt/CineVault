//! The list of known aspect ratios, rounding to it, and choosing the primary and secondary ratio
//! from the votes of the samples.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Ratios below this are not a picture shape; above the maximum, nothing is a film.
const MIN_RATIO: f32 = 0.5;
const MAX_RATIO: f32 = 10.0;
/// Two list entries closer than this are one entry.
const MERGE_BELOW: f32 = 0.005;
/// A measurement this far outside the list (as a factor) is kept as measured instead of being
/// pulled onto the nearest end of the list.
const OUTSIDE_FACTOR: f32 = 1.05;
/// A second ratio must hold at least this share (percent) of the usable samples.
const SECONDARY_SHARE_PERCENT: u32 = 6;

/// The ratios a measurement is rounded to. Editable by the user; invalid entries are dropped, the
/// rest is sorted and de-duplicated, so any list is safe to use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "Vec<f32>", into = "Vec<f32>")]
pub struct AspectRatioList(Vec<f32>);

impl AspectRatioList {
    /// The ratios of the common film and video formats.
    pub const DEFAULT: [f32; 13] = [
        1.33, 1.37, 1.43, 1.66, 1.78, 1.85, 1.90, 2.00, 2.20, 2.35, 2.39, 2.55, 2.76,
    ];

    /// Builds a list from user values: drops what is not a sensible ratio, sorts, removes
    /// near-duplicates.
    #[must_use]
    pub fn new(values: impl IntoIterator<Item = f32>) -> Self {
        let mut list: Vec<f32> = values
            .into_iter()
            .filter(|v| v.is_finite() && (MIN_RATIO..=MAX_RATIO).contains(v))
            .collect();
        list.sort_by(f32::total_cmp);
        list.dedup_by(|b, a| (*b - *a).abs() < MERGE_BELOW);
        Self(list)
    }

    /// An empty list: measurements are only rounded to hundredths.
    #[must_use]
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    #[must_use]
    pub fn values(&self) -> &[f32] {
        &self.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Rounds a measured ratio onto the list.
    ///
    /// * Nearest (`round_up` false): between two entries the boundary is their geometric mean, so
    ///   the choice does not favour wide or narrow shapes.
    /// * `round_up`: the next wider entry (an entry within half a hundredth below the
    ///   measurement counts as equal).
    ///
    /// A measurement far outside the list is returned as measured, to hundredths. A value that
    /// is not a ratio gives `0.0`.
    #[must_use]
    pub fn round(&self, ratio: f32, round_up: bool) -> f32 {
        if !ratio.is_finite() || ratio <= 0.0 {
            return 0.0;
        }
        let (Some(&first), Some(&last)) = (self.0.first(), self.0.last()) else {
            return hundredths(ratio);
        };
        if round_up {
            return self
                .0
                .iter()
                .copied()
                .find(|entry| *entry >= ratio - MERGE_BELOW)
                .unwrap_or_else(|| hundredths(ratio));
        }
        if ratio < first / OUTSIDE_FACTOR || ratio > last * OUTSIDE_FACTOR {
            return hundredths(ratio);
        }
        if ratio <= first {
            return first;
        }
        if ratio >= last {
            return last;
        }
        let at = self.0.partition_point(|entry| *entry <= ratio);
        let (lower, upper) = (self.0[at - 1], self.0[at]);
        if ratio < (lower * upper).sqrt() {
            lower
        } else {
            upper
        }
    }
}

impl Default for AspectRatioList {
    fn default() -> Self {
        Self::new(Self::DEFAULT)
    }
}

impl From<Vec<f32>> for AspectRatioList {
    fn from(values: Vec<f32>) -> Self {
        Self::new(values)
    }
}

impl From<AspectRatioList> for Vec<f32> {
    fn from(list: AspectRatioList) -> Self {
        list.0
    }
}

fn hundredths(ratio: f32) -> f32 {
    (ratio * 100.0).round() / 100.0
}

/// Which ratio of a multi-format film is recorded as the main one (accurate mode only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MultiFormat {
    /// The ratio seen in most samples.
    #[default]
    MostCommon,
    /// The taller picture, which is the narrower ratio.
    Higher,
    /// The wider ratio.
    Wider,
}

/// The ratios seen in the samples and how many samples voted for each.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Votes {
    /// Hundredths of the ratio mapped to the ratio and its votes, ascending.
    bins: BTreeMap<i32, (f32, u32)>,
    total: u32,
}

impl Votes {
    /// Adds one sample's (already rounded) ratio.
    pub fn add(&mut self, ratio: f32) {
        // A hundredth is far below any real difference between formats.
        #[allow(clippy::cast_possible_truncation)]
        let key = (ratio * 100.0).round() as i32;
        let slot = self.bins.entry(key).or_insert((ratio, 0));
        slot.1 += 1;
        self.total += 1;
    }

    #[must_use]
    pub fn total(&self) -> u32 {
        self.total
    }

    /// Ratios with their votes, narrowest first.
    #[must_use]
    pub fn entries(&self) -> Vec<(f32, u32)> {
        self.bins.values().copied().collect()
    }

    /// Chooses `(primary, secondary)`; the secondary is `0.0` when there is none.
    ///
    /// The primary is the most common ratio (a tie goes to the narrower ratio: black-bar
    /// detection errs towards cropping too much, which reads as too wide). With `secondary`
    /// enabled, every other ratio holding at least 6 % of the samples counts as a second format:
    /// the most common of them becomes the secondary, and `multi` may swap which of the formats
    /// is the primary.
    #[must_use]
    pub fn choose(&self, secondary: bool, multi: MultiFormat) -> (f32, f32) {
        // Most votes first; ties keep the narrower ratio first (the map is ascending and the
        // sort is stable).
        let mut ranked = self.entries();
        ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        let Some(&(most, _)) = ranked.first() else {
            return (0.0, 0.0);
        };
        if !secondary {
            return (most, 0.0);
        }
        let formats: Vec<(f32, u32)> = ranked
            .iter()
            .copied()
            .filter(|(_, votes)| votes * 100 >= self.total * SECONDARY_SHARE_PERCENT)
            .collect();
        if formats.len() < 2 {
            return (most, 0.0);
        }
        let primary = match multi {
            MultiFormat::MostCommon => formats[0].0,
            MultiFormat::Higher => formats.iter().map(|f| f.0).fold(f32::INFINITY, f32::min),
            MultiFormat::Wider => formats.iter().map(|f| f.0).fold(0.0, f32::max),
        };
        let second = formats
            .iter()
            .find(|(ratio, _)| (*ratio - primary).abs() > f32::EPSILON)
            .map_or(0.0, |f| f.0);
        (primary, second)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.0001
    }

    #[test]
    fn nearest_uses_the_geometric_mean_between_entries() {
        let list = AspectRatioList::default();
        // 2.35 and 2.39: the boundary is about 2.3699.
        assert!(close(list.round(2.368, false), 2.35));
        assert!(close(list.round(2.372, false), 2.39));
        assert!(close(list.round(2.388, false), 2.39));
        assert!(close(list.round(1.7778, false), 1.78));
        assert!(close(list.round(1.3333, false), 1.33));
        assert!(close(list.round(1.90, false), 1.90));
        assert!(close(list.round(1.905, false), 1.90));
    }

    #[test]
    fn a_wide_list_gap_is_split_at_the_geometric_mean() {
        let list = AspectRatioList::new([1.0, 4.0]);
        // sqrt(1 * 4) = 2.
        assert!(close(list.round(1.99, false), 1.0));
        assert!(close(list.round(2.01, false), 4.0));
    }

    #[test]
    fn round_up_takes_the_next_wider_entry() {
        let list = AspectRatioList::default();
        assert!(close(list.round(1.7778, true), 1.78));
        assert!(close(list.round(1.79, true), 1.85));
        assert!(close(list.round(2.36, true), 2.39));
        assert!(close(list.round(2.35, true), 2.35));
        // Wider than anything listed: kept as measured.
        assert!(close(list.round(3.2, true), 3.2));
    }

    #[test]
    fn values_far_outside_the_list_are_kept_as_measured() {
        let list = AspectRatioList::default();
        assert!(close(list.round(4.0, false), 4.0));
        assert!(close(list.round(1.0, false), 1.0));
        // Just beyond an end still snaps to it.
        assert!(close(list.round(2.78, false), 2.76));
        assert!(close(list.round(1.31, false), 1.33));
    }

    #[test]
    fn an_empty_list_rounds_to_hundredths_and_junk_gives_zero() {
        let list = AspectRatioList::empty();
        assert!(close(list.round(2.3871, false), 2.39));
        let list = AspectRatioList::default();
        assert!(close(list.round(0.0, false), 0.0));
        assert!(close(list.round(f32::NAN, false), 0.0));
        assert!(close(list.round(-1.0, false), 0.0));
    }

    #[test]
    fn the_list_cleans_up_user_input() {
        let list = AspectRatioList::new([2.39, f32::NAN, 1.78, 0.0, 99.0, 1.781, 2.39, -3.0]);
        assert_eq!(list.values(), &[1.78, 2.39]);
        let back: AspectRatioList = serde_json::from_str("[2.39,1.33,1.33]").expect("parses");
        assert_eq!(back.values(), &[1.33, 2.39]);
        let json = serde_json::to_string(&back).expect("serialises");
        assert_eq!(json, "[1.33,2.39]");
    }

    fn votes(entries: &[(f32, u32)]) -> Votes {
        let mut votes = Votes::default();
        for (ratio, count) in entries {
            for _ in 0..*count {
                votes.add(*ratio);
            }
        }
        votes
    }

    #[test]
    fn the_most_common_ratio_is_primary() {
        let v = votes(&[(2.39, 20), (1.90, 8), (1.78, 1)]);
        assert_eq!(v.total(), 29);
        // 1.78 holds 3 %: not a format.
        let (primary, secondary) = v.choose(true, MultiFormat::MostCommon);
        assert!(close(primary, 2.39) && close(secondary, 1.90));
    }

    #[test]
    fn higher_and_wider_decide_which_format_is_primary() {
        let v = votes(&[(2.39, 20), (1.90, 8)]);
        let (primary, secondary) = v.choose(true, MultiFormat::Higher);
        assert!(close(primary, 1.90) && close(secondary, 2.39));
        let (primary, secondary) = v.choose(true, MultiFormat::Wider);
        assert!(close(primary, 2.39) && close(secondary, 1.90));
        let v = votes(&[(1.90, 20), (2.39, 8)]);
        let (primary, secondary) = v.choose(true, MultiFormat::Wider);
        assert!(close(primary, 2.39) && close(secondary, 1.90));
    }

    #[test]
    fn without_secondary_detection_only_the_most_common_counts() {
        let v = votes(&[(2.39, 20), (1.90, 8)]);
        let (primary, secondary) = v.choose(false, MultiFormat::Higher);
        assert!(close(primary, 2.39) && close(secondary, 0.0));
    }

    #[test]
    fn a_format_needs_six_percent_of_the_samples() {
        // 2 of 30 is 6.7 %, 1 of 30 is 3.3 %.
        let v = votes(&[(2.39, 28), (1.90, 2)]);
        assert!(close(v.choose(true, MultiFormat::MostCommon).1, 1.90));
        let v = votes(&[(2.39, 29), (1.90, 1)]);
        assert!(close(v.choose(true, MultiFormat::MostCommon).1, 0.0));
    }

    #[test]
    fn a_tie_goes_to_the_narrower_ratio() {
        let v = votes(&[(2.39, 3), (1.85, 3)]);
        assert!(close(v.choose(false, MultiFormat::MostCommon).0, 1.85));
    }

    #[test]
    fn no_votes_give_nothing() {
        assert_eq!(
            Votes::default().choose(true, MultiFormat::Wider),
            (0.0, 0.0)
        );
    }
}

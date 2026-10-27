//! Searching a show's episode list by title, for an episode whose number is unknown.
//!
//! The list is what the provider returned for the show's ordering. A hit carries the number in
//! that ordering, the aired date and the start of the plot, so the user can tell two similar
//! titles apart, and the provider IDs that [`EpisodePick`] sends back to assign it.

use cv_domain::{CalendarDate, EpisodeGroup, EpisodeNumbers, Item, ItemDetail, ProviderIds};
use cv_match::dice_bigram;

use crate::episode_order::matcher::number_in;

/// The lowest score a hit needs. The Dice similarity of unrelated titles is well below it.
const MIN_SCORE: f64 = 0.5;
/// How many hits a search returns at most.
pub const MAX_HITS: usize = 20;
/// The characters of plot a hit carries.
const PLOT_CHARS: usize = 240;

/// One episode of the provider's list that fits the typed text.
#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeHit {
    pub title: String,
    /// Season and episode in the show's ordering. `None` when the list gives the episode no
    /// number in it.
    pub number: Option<(u32, u32)>,
    pub first_aired: Option<CalendarDate>,
    /// The start of the plot, for telling hits apart.
    pub plot: String,
    /// The provider's IDs of the episode.
    pub ids: ProviderIds,
    /// 0 to 1, higher is closer.
    pub score: f64,
}

impl EpisodeHit {
    /// What identifies the hit when it is assigned.
    #[must_use]
    pub fn pick(&self) -> EpisodePick {
        EpisodePick {
            ids: self.ids.clone(),
            number: self.number,
        }
    }
}

/// A hit chosen by the user: the entry of the list to assign. It is found again by ID, then by
/// number.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EpisodePick {
    pub ids: ProviderIds,
    pub number: Option<(u32, u32)>,
}

/// The text compared: lower case, letters and digits only, single spaces.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut gap = true;
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            gap = false;
        } else if !gap {
            out.push(' ');
            gap = true;
        }
    }
    out.truncate(out.trim_end().len());
    out
}

/// Whether every word of `query` is a word of `title` or starts one (three letters at least), so
/// a half-typed `quiet ti` finds `The Quiet Tide`.
fn words_fit(query: &str, title: &str) -> bool {
    let title_words: Vec<&str> = title.split(' ').collect();
    query.split(' ').all(|word| {
        title_words
            .iter()
            .any(|t| *t == word || (word.chars().count() >= 3 && t.starts_with(word)))
    })
}

/// How well `title` fits `query`, both already plain. A title that is the query scores 1, one
/// that contains it as whole words at least 0.9, one that starts each of its words at least
/// 0.85, else the Dice similarity of the two (it forgives a typo, and unlike a prefix-weighted
/// measure it does not rate unrelated words as close).
fn score(query: &str, title: &str) -> f64 {
    if query.is_empty() || title.is_empty() {
        return 0.0;
    }
    if query == title {
        return 1.0;
    }
    let similarity = dice_bigram(query, title);
    let padded_title = format!(" {title} ");
    if query.chars().count() >= 3 && padded_title.contains(&format!(" {query} ")) {
        similarity.max(0.9)
    } else if words_fit(query, title) {
        similarity.max(0.85)
    } else {
        similarity
    }
}

/// The titles an episode can be found by.
fn titles(item: &Item) -> impl Iterator<Item = &str> {
    let english = match &item.detail {
        ItemDetail::Episode(fields) => fields.english_title.as_str(),
        _ => "",
    };
    [
        item.core.title.as_str(),
        item.core.original_title.as_str(),
        english,
    ]
    .into_iter()
    .filter(|t| !t.trim().is_empty())
}

fn number_of(numbers: &EpisodeNumbers, group: &EpisodeGroup) -> Option<(u32, u32)> {
    number_in(numbers, group)
        .or_else(|| numbers.get(&EpisodeGroup::aired()))
        .map(|n| (n.season, n.episode))
}

/// The entries of `list` (episodes in the ordering `group`) whose title fits `query`, best
/// first; equal scores are in number order. At most [`MAX_HITS`].
#[must_use]
pub fn search_titles(list: &[Item], group: &EpisodeGroup, query: &str) -> Vec<EpisodeHit> {
    let query = plain(query);
    if query.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<EpisodeHit> = list
        .iter()
        .filter_map(|item| {
            let ItemDetail::Episode(fields) = &item.detail else {
                return None;
            };
            let best = titles(item)
                .map(|t| score(&query, &plain(t)))
                .fold(0.0, f64::max);
            (best >= MIN_SCORE).then(|| EpisodeHit {
                title: item.core.title.clone(),
                number: number_of(&fields.numbers, group),
                first_aired: fields.first_aired,
                plot: item.core.plot.chars().take(PLOT_CHARS).collect(),
                ids: item.core.ids.clone(),
                score: best,
            })
        })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.number.cmp(&b.number))
    });
    hits.truncate(MAX_HITS);
    hits
}

/// The index in `list` of the entry `pick` names: by a shared ID, else by number in `group`.
#[must_use]
pub fn find_pick(list: &[Item], group: &EpisodeGroup, pick: &EpisodePick) -> Option<usize> {
    let by_id = list.iter().position(|item| {
        pick.ids
            .iter()
            .any(|(key, value)| item.core.ids.get(key) == Some(value))
    });
    by_id.or_else(|| {
        let wanted = pick.number?;
        list.iter().position(|item| match &item.detail {
            ItemDetail::Episode(fields) => {
                number_in(&fields.numbers, group).is_some_and(|n| (n.season, n.episode) == wanted)
            }
            _ => false,
        })
    })
}

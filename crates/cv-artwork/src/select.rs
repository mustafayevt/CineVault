//! Automatic best-image selection: which of the gathered candidates becomes the poster, the
//! fanart and so on.
//!
//! Spec: `artwork` (Automatic best-image selection). The input is what `cv_providers::gather_artwork` found (plus frame grabs, see
//! [`crate::frame_source`]); the output is a ranked list of one artwork type, best first.
//!
//! # Order of the ranked list
//!
//! Online images are sorted by their [`quality`](crate::quality::quality) (how much people like
//! them, then how big they are), best first; equal images keep the order they came in, which is
//! the provider order the user chose. Then the list is built in tiers, each tier adding the
//! images not yet listed:
//!
//! 1. **Language and size.** For each language of the preferred list in turn, the images of that
//!    language at the preferred size. The list may contain `none`, which stands for images
//!    without text.
//! 2. **Textless.** Only if tier 1 found nothing: images without text at the preferred size.
//! 3. **Frame grabs.** Only if nothing is listed yet: frame grabs at the preferred size.
//! 4. **Other sizes** (when enabled). The other sizes ordered by distance from the preferred one,
//!    bigger first on ties, for each language in turn, then for frame grabs. Without this setting
//!    a frame grab of any size is taken instead, but only when nothing is listed yet.
//! 5. **Any language** (when enabled). Every remaining online image, bigger sizes first and images
//!    of unknown size last, then the remaining frame grabs.
//!
//! For fanart, when "prefer fanart without text" is on, the best textless image at the preferred
//! size is moved to the front of the list.
//!
//! Images that are too small or the wrong shape for their type
//! ([`is_low_quality`](crate::quality::is_low_quality)) go through the same tiers on their own and
//! follow every other image: they are taken only when nothing better is offered.
//!
//! # The picker's order
//!
//! The picker lists every provider's images as they arrive, so it cannot wait for the whole list.
//! [`best_score`] gives each image a key that follows the same rules: the fit to the settings
//! first, then the position of its language in the preferred list, then its quality.
//!
//! # Sizes
//!
//! Only posters and fanart have a preferred size (the settings). The preferred size "large" (the
//! default) takes "extra large" images as well, so the biggest images are never ranked behind
//! smaller ones of the same language. Every other type is asked for
//! "any size": all images match the preferred size, so the list is just language order, then
//! score. Images that have no size bucket (width unknown or below the smallest bucket) match no
//! preferred size of a bucketed type and only appear in tier 5.

use std::collections::HashSet;

use cv_domain::MediaFileType;
use cv_providers::SizeBucket;
use cv_providers::SourcedArtwork;
use cv_providers::artwork::{primary_language, text_language};

use crate::frame_source::is_frame;
use crate::quality::{is_low_quality, quality};

/// The word in the language list that stands for images without text.
pub const NO_TEXT: &str = "none";

/// The user's choices for automatic selection (Settings, artwork).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionSettings {
    /// Preferred languages, best first. Region and case are ignored; `none` stands for images
    /// without text. Default: the UI language, then English (see [`Self::for_ui_language`]).
    pub languages: Vec<String>,
    /// Preferred size of posters (and season posters).
    pub poster_size: SizeBucket,
    /// Preferred size of fanart (and season fanart).
    pub fanart_size: SizeBucket,
    /// Fall back to the other sizes when none fits the preferred one. Default on.
    pub other_resolutions: bool,
    /// Put a fanart without text first. Default on.
    pub prefer_fanart_without_text: bool,
    /// As a last resort take any language at any size. Default on.
    pub any_language_fallback: bool,
}

impl Default for SelectionSettings {
    fn default() -> Self {
        Self::for_ui_language("en")
    }
}

impl SelectionSettings {
    /// The defaults for a UI language: `[ui language, en]` and large posters and fanart.
    pub fn for_ui_language(ui_language: &str) -> Self {
        let ui = primary_language(ui_language);
        let mut languages = Vec::new();
        if !ui.is_empty() && ui != "en" {
            languages.push(ui);
        }
        languages.push("en".to_owned());
        Self {
            languages,
            poster_size: SizeBucket::Large,
            fanart_size: SizeBucket::Large,
            other_resolutions: true,
            prefer_fanart_without_text: true,
            any_language_fallback: true,
        }
    }

    /// The preferred size of an artwork type.
    pub fn size_wish(&self, kind: MediaFileType) -> SizeWish {
        match kind {
            MediaFileType::Poster | MediaFileType::SeasonPoster => {
                SizeWish::Bucket(self.poster_size)
            }
            MediaFileType::Fanart | MediaFileType::SeasonFanart | MediaFileType::Extrafanart => {
                SizeWish::Bucket(self.fanart_size)
            }
            _ => SizeWish::Any,
        }
    }

    fn lists_no_text(&self) -> bool {
        self.languages
            .iter()
            .any(|language| language.trim().eq_ignore_ascii_case(NO_TEXT))
    }
}

/// The size an artwork type is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeWish {
    /// Every size is as good as any other.
    Any,
    /// This size, then the others by distance.
    Bucket(SizeBucket),
}

impl SizeWish {
    /// Whether an image is at the preferred size. "Large" takes extra-large images too: an image
    /// bigger than large is never a worse large image, and a 4K backdrop must not wait behind
    /// every 2K one. Among the images that fit, the bigger one wins a near tie through the
    /// resolution part of [`quality`].
    fn accepts(self, item: &SourcedArtwork) -> bool {
        match self {
            Self::Any => true,
            Self::Bucket(wanted) => item.candidate.size_bucket().is_some_and(|bucket| {
                bucket == wanted || (wanted == SizeBucket::Large && bucket == SizeBucket::XLarge)
            }),
        }
    }

    /// The sizes other than the preferred one, nearest first, the bigger one first on a tie.
    fn other_sizes(self) -> Vec<SizeBucket> {
        let Self::Bucket(wanted) = self else {
            return Vec::new();
        };
        let position = |bucket: SizeBucket| {
            SizeBucket::ALL
                .iter()
                .position(|b| *b == bucket)
                .unwrap_or(0)
        };
        let wanted_at = position(wanted);
        let mut others: Vec<SizeBucket> = SizeBucket::ALL
            .into_iter()
            .filter(|bucket| *bucket != wanted)
            .collect();
        // `ALL` is largest first, so a stable sort by distance keeps the bigger size ahead on ties.
        others.sort_by_key(|bucket| position(*bucket).abs_diff(wanted_at));
        others
    }
}

fn is_fanart(kind: MediaFileType) -> bool {
    matches!(kind, MediaFileType::Fanart | MediaFileType::SeasonFanart)
}

/// Whether the image's text language is `wanted` (a language-list entry).
fn language_matches(item: &SourcedArtwork, wanted: &str) -> bool {
    let textless = text_language(item.candidate.language.as_deref()).is_none();
    if wanted.trim().eq_ignore_ascii_case(NO_TEXT) {
        return textless;
    }
    text_language(item.candidate.language.as_deref())
        .is_some_and(|language| language == primary_language(wanted))
}

/// Builds the ranked list in tiers, never listing a URL twice.
struct Ranking<'a> {
    list: Vec<&'a SourcedArtwork>,
    seen: HashSet<&'a str>,
}

impl<'a> Ranking<'a> {
    fn add(&mut self, item: &'a SourcedArtwork) {
        if self.seen.insert(item.candidate.url.as_str()) {
            self.list.push(item);
        }
    }

    fn add_all<'b>(
        &mut self,
        pool: &'b [&'a SourcedArtwork],
        keep: impl Fn(&SourcedArtwork) -> bool,
    ) {
        for item in pool.iter().copied().filter(|item| keep(item)) {
            self.add(item);
        }
    }

    fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

/// Orders candidates by [`quality`], best first, then by the provider's own score. Stable: equal
/// candidates keep the provider order they came in.
fn by_quality(list: &mut [&SourcedArtwork]) {
    list.sort_by(|a, b| {
        quality(&b.candidate)
            .total_cmp(&quality(&a.candidate))
            .then_with(|| b.candidate.score.total_cmp(&a.candidate.score))
    });
}

/// The candidates of `kind`, best first, by the rules in the module docs.
///
/// Candidates of other types are ignored, so the whole gathered list can be passed in. The result
/// is empty when nothing fits (or when only tiers the settings switch off would have fitted).
pub fn rank<'a>(
    candidates: &'a [SourcedArtwork],
    kind: MediaFileType,
    settings: &SelectionSettings,
) -> Vec<&'a SourcedArtwork> {
    let of_kind = |frame: bool, low: bool| -> Vec<&'a SourcedArtwork> {
        let mut list: Vec<&SourcedArtwork> = candidates
            .iter()
            .filter(|item| {
                item.candidate.kind == kind
                    && is_frame(item) == frame
                    && is_low_quality(item, settings) == low
            })
            .collect();
        by_quality(&mut list);
        list
    };
    // Images that are too small or misshapen come after every other image, in the same tiers.
    let mut list = tiers(
        &of_kind(false, false),
        &of_kind(true, false),
        kind,
        settings,
    );
    let low = tiers(&of_kind(false, true), &of_kind(true, true), kind, settings);
    let mut seen: HashSet<&str> = list
        .iter()
        .map(|item| item.candidate.url.as_str())
        .collect();
    list.extend(
        low.into_iter()
            .filter(|item| seen.insert(item.candidate.url.as_str())),
    );
    list
}

/// The tiers of the module docs over one pool of online images and frame grabs, each already in
/// [`quality`] order.
fn tiers<'a>(
    online: &[&'a SourcedArtwork],
    frames: &[&'a SourcedArtwork],
    kind: MediaFileType,
    settings: &SelectionSettings,
) -> Vec<&'a SourcedArtwork> {
    let wish = settings.size_wish(kind);
    let mut ranking = Ranking {
        list: Vec::new(),
        seen: HashSet::new(),
    };

    // 1. Each preferred language at the preferred size.
    for language in &settings.languages {
        ranking.add_all(online, |item| {
            language_matches(item, language) && wish.accepts(item)
        });
    }
    // 2. Images without text at the preferred size.
    if ranking.is_empty() {
        ranking.add_all(online, |item| {
            language_matches(item, NO_TEXT) && wish.accepts(item)
        });
    }
    // 3. Frame grabs at the preferred size.
    if ranking.is_empty() {
        ranking.add_all(frames, |item| wish.accepts(item));
    }
    // 4. Other sizes, nearest first, per language and then for frame grabs.
    if settings.other_resolutions {
        let sizes = wish.other_sizes();
        for language in &settings.languages {
            for bucket in &sizes {
                ranking.add_all(online, |item| {
                    language_matches(item, language)
                        && item.candidate.size_bucket() == Some(*bucket)
                });
            }
        }
        for bucket in &sizes {
            ranking.add_all(frames, |item| item.candidate.size_bucket() == Some(*bucket));
        }
    } else if ranking.is_empty() {
        ranking.add_all(frames, |_| true);
    }
    // 5. Any language at any size: bigger first, unknown size last (the sort is stable).
    if settings.any_language_fallback {
        let mut rest: Vec<&SourcedArtwork> = online.to_vec();
        rest.sort_by_key(|item| std::cmp::Reverse(item.candidate.size_order()));
        ranking.add_all(&rest, |_| true);
        ranking.add_all(frames, |_| true);
    }

    let mut list = ranking.list;
    // Fanart without text goes first (the Kodi fanart specification).
    if settings.prefer_fanart_without_text
        && is_fanart(kind)
        && let Some(at) = list.iter().position(|item| {
            !is_frame(item) && language_matches(item, NO_TEXT) && wish.accepts(item)
        })
    {
        let textless = list.remove(at);
        list.insert(0, textless);
    }
    list
}

/// The best candidate of `kind`: the first of [`rank`].
pub fn select_best<'a>(
    candidates: &'a [SourcedArtwork],
    kind: MediaFileType,
    settings: &SelectionSettings,
) -> Option<&'a SourcedArtwork> {
    rank(candidates, kind, settings).into_iter().next()
}

/// The images for extra fanart or extra thumbs: the next `count` entries of the ranked list of
/// `kind` (`Fanart` or `Thumb`) after the one chosen as the main image.
pub fn select_extras<'a>(
    candidates: &'a [SourcedArtwork],
    kind: MediaFileType,
    settings: &SelectionSettings,
    count: usize,
) -> Vec<&'a SourcedArtwork> {
    rank(candidates, kind, settings)
        .into_iter()
        .skip(1)
        .take(count)
        .collect()
}

/// The score the picker shows for a candidate, from how well it fits the settings: 90 for language
/// and size, 50 for language only, 40 for size only, 30 for a frame grab at the preferred size,
/// otherwise 0. Images without text count as a language match when `none` is listed, or for a
/// fanart when "prefer fanart without text" is on.
pub fn matching_score(
    item: &SourcedArtwork,
    kind: MediaFileType,
    settings: &SelectionSettings,
) -> u32 {
    let size_fits = settings.size_wish(kind).accepts(item);
    if is_frame(item) {
        return if size_fits { 30 } else { 0 };
    }
    let textless = language_matches(item, NO_TEXT);
    let language_fits = if textless {
        settings.lists_no_text() || (is_fanart(kind) && settings.prefer_fanart_without_text)
    } else {
        settings
            .languages
            .iter()
            .any(|language| language_matches(item, language))
    };
    match (language_fits, size_fits) {
        (true, true) => 90,
        (true, false) => 50,
        (false, true) => 40,
        (false, false) => 0,
    }
}

/// The picker's "Best" key of a candidate, higher first, in the order [`rank`] gives. It reads as
/// four digits of decreasing weight, so a part only counts when the ones before it are equal:
///
/// 1. the fit to the settings ([`matching_score`]: 90, 50, 40, 30 or 0), times 1000;
/// 2. the language, times 100: 9 for a textless fanart the settings prefer, 8 for the first
///    preferred language, 7 for the second and so on, 0 for any other;
/// 3. the size, times 10: 9 at the preferred size, less the farther its size class is from it
///    (the bigger one first on a tie), 0 when the size is unknown;
/// 4. the [`quality`], scaled to just under 10.
#[must_use]
pub fn best_score(item: &SourcedArtwork, kind: MediaFileType, settings: &SelectionSettings) -> f32 {
    let fit = matching_score(item, kind, settings) as f32;
    fit * 1000.0
        + language_digit(item, kind, settings) * 100.0
        + size_digit(item, settings.size_wish(kind)) * 10.0
        + 9.99 * quality(&item.candidate)
}

fn language_digit(item: &SourcedArtwork, kind: MediaFileType, settings: &SelectionSettings) -> f32 {
    if is_frame(item) {
        return 0.0;
    }
    if is_fanart(kind) && settings.prefer_fanart_without_text && language_matches(item, NO_TEXT) {
        return 9.0;
    }
    settings
        .languages
        .iter()
        .position(|language| language_matches(item, language))
        .map_or(0.0, |at| 8.0 - at.min(7) as f32)
}

fn size_digit(item: &SourcedArtwork, wish: SizeWish) -> f32 {
    let Some(bucket) = item.candidate.size_bucket() else {
        return if wish == SizeWish::Any { 9.0 } else { 0.0 };
    };
    if wish.accepts(item) {
        return 9.0;
    }
    let others = wish.other_sizes();
    others
        .iter()
        .position(|b| *b == bucket)
        .map_or(0.0, |at| 8.0 - at.min(7) as f32)
}

#[cfg(test)]
mod tests {
    use cv_providers::model::{ArtworkCandidate, Popularity};

    use super::*;

    fn item(
        provider: &'static str,
        kind: MediaFileType,
        url: &str,
        language: Option<&str>,
        width: u32,
        score: f32,
    ) -> SourcedArtwork {
        SourcedArtwork {
            provider,
            candidate: ArtworkCandidate {
                kind,
                url: url.to_owned(),
                language: language.map(str::to_owned),
                width,
                height: match kind {
                    MediaFileType::Poster => width * 3 / 2,
                    MediaFileType::Fanart | MediaFileType::Thumb => width * 9 / 16,
                    _ => width / 3,
                },
                score,
                // The score stands for well-voted images, so the tests read as before.
                popularity: Popularity::Rated {
                    average: score,
                    votes: 50,
                },
            },
        }
    }

    fn poster(url: &str, language: Option<&str>, width: u32, score: f32) -> SourcedArtwork {
        item("tmdb", MediaFileType::Poster, url, language, width, score)
    }

    fn fanart(url: &str, language: Option<&str>, width: u32, score: f32) -> SourcedArtwork {
        item("tmdb", MediaFileType::Fanart, url, language, width, score)
    }

    fn urls(list: &[&SourcedArtwork]) -> Vec<String> {
        list.iter().map(|i| i.candidate.url.clone()).collect()
    }

    fn settings(languages: &[&str]) -> SelectionSettings {
        SelectionSettings {
            languages: languages.iter().map(|l| (*l).to_owned()).collect(),
            ..SelectionSettings::default()
        }
    }

    // --- spec scenarios -----------------------------------------------------------------------

    #[test]
    fn scenario_prefer_textless_fanart() {
        let list = [
            fanart("with-text", Some("en"), 1920, 9.0),
            fanart("textless", None, 1920, 3.0),
        ];
        let on = SelectionSettings {
            prefer_fanart_without_text: true,
            ..settings(&["en"])
        };
        assert_eq!(
            select_best(&list, MediaFileType::Fanart, &on)
                .unwrap()
                .candidate
                .url,
            "textless"
        );
        // Switched off, the better rated image of the preferred language wins.
        let off = SelectionSettings {
            prefer_fanart_without_text: false,
            ..settings(&["en"])
        };
        assert_eq!(
            select_best(&list, MediaFileType::Fanart, &off)
                .unwrap()
                .candidate
                .url,
            "with-text"
        );
    }

    #[test]
    fn scenario_poster_language_preference() {
        let list = [
            poster("ja-best", Some("ja"), 1000, 9.5),
            poster("en-low", Some("en"), 1000, 2.0),
            poster("en-best", Some("en"), 1000, 7.0),
        ];
        let best = select_best(&list, MediaFileType::Poster, &settings(&["tr", "en"])).unwrap();
        assert_eq!(best.candidate.url, "en-best");
    }

    // --- language ------------------------------------------------------------------------------

    #[test]
    fn the_first_language_with_an_image_wins_over_score() {
        let list = [
            poster("en", Some("en"), 1000, 9.0),
            poster("tr", Some("tr-TR"), 1000, 1.0),
        ];
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["TR", "en"]));
        assert_eq!(urls(&ranked), ["tr", "en"]);
    }

    #[test]
    fn the_ui_language_comes_before_english_by_default() {
        assert_eq!(
            SelectionSettings::for_ui_language("de-AT").languages,
            ["de", "en"]
        );
        assert_eq!(
            SelectionSettings::for_ui_language("en-GB").languages,
            ["en"]
        );
    }

    #[test]
    fn none_in_the_list_places_textless_images_at_that_position() {
        let list = [
            poster("en", Some("en"), 1000, 9.0),
            poster("textless", Some("00"), 1000, 1.0),
        ];
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["none", "en"]));
        assert_eq!(urls(&ranked), ["textless", "en"]);
    }

    #[test]
    fn textless_images_are_used_when_no_listed_language_has_one() {
        let list = [
            poster("ja", Some("ja"), 1000, 9.0),
            poster("textless", None, 1000, 1.0),
        ];
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["tr", "en"]));
        assert_eq!(urls(&ranked), ["textless", "ja"]);
    }

    // --- size ----------------------------------------------------------------------------------

    #[test]
    fn the_preferred_size_beats_a_better_scored_other_size() {
        let list = [
            poster("big", Some("en"), 500, 9.0),
            poster("large", Some("en"), 1000, 1.0),
        ];
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["en"]));
        assert_eq!(urls(&ranked), ["large", "big"]);
    }

    #[test]
    fn other_sizes_are_walked_by_distance_bigger_first_on_ties() {
        // Preferred Large: XLarge and Big are one step away (XLarge first), then Medium, Small.
        let list = [
            poster("small", Some("en"), 185, 5.0),
            poster("medium", Some("en"), 342, 5.0),
            poster("big", Some("en"), 500, 5.0),
            poster("xlarge", Some("en"), 2000, 5.0),
        ];
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["en"]));
        assert_eq!(urls(&ranked), ["xlarge", "big", "medium", "small"]);
        assert_eq!(
            SizeWish::Bucket(SizeBucket::Large).other_sizes(),
            [
                SizeBucket::XLarge,
                SizeBucket::Big,
                SizeBucket::Medium,
                SizeBucket::Small
            ]
        );
    }

    /// A user report: a 2048-wide textless backdrop was chosen over a 3840-wide one that was
    /// liked as much, because "large" did not take the 4K image and it waited for the last tier.
    #[test]
    fn a_4k_backdrop_counts_as_large_and_wins_a_near_tie() {
        let list = [
            fanart("en-text", Some("en"), 1920, 5.5),
            fanart("textless-2k", None, 2048, 5.5),
            fanart("textless-4k", None, 3840, 5.4),
        ];
        let ranked = rank(&list, MediaFileType::Fanart, &settings(&["en"]));
        assert_eq!(urls(&ranked)[0], "textless-4k");
        assert_eq!(
            urls(&ranked),
            ["textless-4k", "en-text", "textless-2k"],
            "the textless images that fit follow the language list as before"
        );
        // An image that is clearly better liked still wins over a bigger one.
        let liked = [
            fanart("textless-2k", None, 2048, 8.0),
            fanart("textless-4k", None, 3840, 4.0),
        ];
        let ranked = rank(&liked, MediaFileType::Fanart, &settings(&["en"]));
        assert_eq!(urls(&ranked)[0], "textless-2k");
    }

    #[test]
    fn an_extra_large_poster_counts_as_large_but_not_as_a_smaller_size() {
        let list = [
            poster("large", Some("en"), 1000, 5.0),
            poster("xlarge", Some("en"), 2000, 5.0),
        ];
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["en"]));
        assert_eq!(urls(&ranked), ["xlarge", "large"]);
        let x = poster("xlarge", Some("en"), 2000, 5.0);
        assert_eq!(
            matching_score(&x, MediaFileType::Poster, &settings(&["en"])),
            90
        );
        // A smaller preferred size is asked for on purpose: a bigger image does not fit it.
        let big = SelectionSettings {
            poster_size: SizeBucket::Big,
            ..settings(&["en"])
        };
        assert_eq!(matching_score(&x, MediaFileType::Poster, &big), 50);
    }

    #[test]
    fn a_different_preferred_size_changes_the_walk() {
        let list = [
            poster("large", Some("en"), 1000, 5.0),
            poster("small", Some("en"), 185, 5.0),
            poster("medium", Some("en"), 342, 5.0),
        ];
        let wanted = SelectionSettings {
            poster_size: SizeBucket::Small,
            ..settings(&["en"])
        };
        let ranked = rank(&list, MediaFileType::Poster, &wanted);
        assert_eq!(urls(&ranked), ["small", "medium", "large"]);
    }

    #[test]
    fn other_sizes_of_the_first_language_come_before_the_next_language() {
        let list = [
            poster("tr-big", Some("tr"), 500, 5.0),
            poster("en-large", Some("en"), 1000, 5.0),
        ];
        // Tier 1 finds the English one at the preferred size; the Turkish one follows as another size.
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["tr", "en"]));
        assert_eq!(urls(&ranked), ["en-large", "tr-big"]);
    }

    #[test]
    fn without_other_resolutions_only_the_preferred_size_is_used() {
        let list = [
            poster("big", Some("en"), 500, 9.0),
            poster("large", Some("en"), 1000, 1.0),
        ];
        let strict = SelectionSettings {
            other_resolutions: false,
            any_language_fallback: false,
            ..settings(&["en"])
        };
        assert_eq!(
            urls(&rank(&list, MediaFileType::Poster, &strict)),
            ["large"]
        );
        let only_other = [poster("big", Some("en"), 500, 9.0)];
        assert!(rank(&only_other, MediaFileType::Poster, &strict).is_empty());
    }

    #[test]
    fn types_without_a_preferred_size_use_language_then_score() {
        let logos = |url: &str, language: Option<&str>, width: u32, score: f32| {
            item(
                "fanart",
                MediaFileType::Clearlogo,
                url,
                language,
                width,
                score,
            )
        };
        let list = [
            logos("de-big", Some("de"), 800, 9.0),
            logos("en-small", Some("en"), 350, 1.0),
            logos("en-best", Some("en"), 400, 4.0),
        ];
        let ranked = rank(&list, MediaFileType::Clearlogo, &settings(&["en"]));
        assert_eq!(urls(&ranked), ["en-best", "en-small", "de-big"]);
    }

    // --- any-language fallback -----------------------------------------------------------------

    #[test]
    fn any_language_is_the_last_resort_and_can_be_switched_off() {
        let list = [poster("ja", Some("ja"), 1000, 9.0)];
        let on = settings(&["tr", "en"]);
        assert_eq!(urls(&rank(&list, MediaFileType::Poster, &on)), ["ja"]);
        let off = SelectionSettings {
            any_language_fallback: false,
            ..on
        };
        assert!(select_best(&list, MediaFileType::Poster, &off).is_none());
    }

    #[test]
    fn the_fallback_lists_bigger_sizes_first_and_unknown_sizes_last() {
        let list = [
            poster("unknown", Some("ja"), 0, 9.0),
            poster("big", Some("fr"), 500, 9.0),
            poster("large", Some("ko"), 1000, 1.0),
        ];
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["en"]));
        assert_eq!(urls(&ranked), ["large", "big", "unknown"]);
    }

    #[test]
    fn other_types_and_empty_input_select_nothing() {
        let list = [fanart("f", None, 1920, 1.0)];
        assert!(select_best(&list, MediaFileType::Poster, &SelectionSettings::default()).is_none());
        assert!(select_best(&[], MediaFileType::Poster, &SelectionSettings::default()).is_none());
    }

    // --- textless fanart -----------------------------------------------------------------------

    #[test]
    fn textless_fanart_must_match_the_preferred_size() {
        let list = [
            fanart("text-large", Some("en"), 1920, 9.0),
            fanart("textless-medium", None, 1280, 9.0),
        ];
        let best = select_best(&list, MediaFileType::Fanart, &settings(&["en"])).unwrap();
        assert_eq!(best.candidate.url, "text-large");
    }

    #[test]
    fn textless_fanart_moves_to_the_front_and_posters_are_unaffected() {
        let fanarts = [
            fanart("a", Some("en"), 1920, 9.0),
            fanart("b", Some("en"), 1920, 8.0),
            fanart("textless", None, 1920, 1.0),
        ];
        let ranked = rank(&fanarts, MediaFileType::Fanart, &settings(&["en"]));
        assert_eq!(urls(&ranked), ["textless", "a", "b"]);
        let posters = [
            poster("a", Some("en"), 1000, 9.0),
            poster("textless", None, 1000, 1.0),
        ];
        let ranked = rank(&posters, MediaFileType::Poster, &settings(&["en"]));
        assert_eq!(urls(&ranked), ["a", "textless"]);
    }

    // --- frame grabs and extras ----------------------------------------------------------------

    fn frame(url: &str, kind: MediaFileType, width: u32) -> SourcedArtwork {
        item(
            crate::frame_source::PROVIDER_ID,
            kind,
            url,
            None,
            width,
            0.0,
        )
    }

    #[test]
    fn frame_grabs_are_used_only_when_no_online_image_fits() {
        let list = [
            frame("file:///f.jpg", MediaFileType::Fanart, 1920),
            fanart("online", Some("en"), 1920, 1.0),
        ];
        let best = select_best(&list, MediaFileType::Fanart, &settings(&["en"])).unwrap();
        assert_eq!(best.candidate.url, "online");
        // Online images of an unlisted language do not count as fitting; the frame is used.
        let list = [
            frame("file:///f.jpg", MediaFileType::Fanart, 1920),
            fanart("ja", Some("ja"), 1920, 1.0),
        ];
        let ranked = rank(&list, MediaFileType::Fanart, &settings(&["en"]));
        assert_eq!(urls(&ranked), ["file:///f.jpg", "ja"]);
        let only_frame = [frame("file:///f.jpg", MediaFileType::Fanart, 1920)];
        let best = select_best(&only_frame, MediaFileType::Fanart, &settings(&["en"])).unwrap();
        assert!(is_frame(best));
    }

    #[test]
    fn a_frame_at_the_preferred_size_beats_online_images_of_other_sizes() {
        let list = [
            fanart("online-small", Some("en"), 300, 9.0),
            frame("file:///f.jpg", MediaFileType::Fanart, 1920),
        ];
        let ranked = rank(&list, MediaFileType::Fanart, &settings(&["en"]));
        assert_eq!(urls(&ranked), ["file:///f.jpg", "online-small"]);
    }

    #[test]
    fn any_size_of_frame_is_taken_when_other_resolutions_are_off() {
        let only_frame = [frame("file:///f.jpg", MediaFileType::Thumb, 0)];
        let strict = SelectionSettings {
            other_resolutions: false,
            any_language_fallback: false,
            ..SelectionSettings::default()
        };
        assert!(select_best(&only_frame, MediaFileType::Thumb, &strict).is_some());
        let fanart_frame = [frame("file:///f.jpg", MediaFileType::Fanart, 640)];
        assert!(select_best(&fanart_frame, MediaFileType::Fanart, &strict).is_some());
    }

    #[test]
    fn extras_are_the_entries_after_the_chosen_image() {
        let list = [
            fanart("a", Some("en"), 1920, 9.0),
            fanart("b", Some("en"), 1920, 8.0),
            fanart("c", Some("en"), 1920, 7.0),
            fanart("d", Some("en"), 1920, 6.0),
        ];
        let s = SelectionSettings {
            prefer_fanart_without_text: false,
            ..settings(&["en"])
        };
        let best = select_best(&list, MediaFileType::Fanart, &s).unwrap();
        let extras = select_extras(&list, MediaFileType::Fanart, &s, 2);
        assert_eq!(best.candidate.url, "a");
        assert_eq!(urls(&extras), ["b", "c"]);
        assert_eq!(select_extras(&list, MediaFileType::Fanart, &s, 10).len(), 3);
        assert!(select_extras(&list, MediaFileType::Fanart, &s, 0).is_empty());
    }

    // --- providers and ties --------------------------------------------------------------------

    #[test]
    fn candidates_of_every_provider_are_considered_and_ties_keep_provider_order() {
        let logo = |provider, url: &str, score| {
            item(
                provider,
                MediaFileType::Clearlogo,
                url,
                Some("en"),
                500,
                score,
            )
        };
        let list = [
            logo("tmdb", "tmdb-1", 5.0),
            logo("fanart", "fanart-best", 8.0),
            logo("fanart", "fanart-tie", 5.0),
        ];
        let ranked = rank(&list, MediaFileType::Clearlogo, &settings(&["en"]));
        assert_eq!(urls(&ranked), ["fanart-best", "tmdb-1", "fanart-tie"]);
    }

    #[test]
    fn a_url_offered_twice_is_listed_once() {
        let list = [
            poster("same", Some("en"), 1000, 5.0),
            poster("same", Some("en"), 1000, 4.0),
        ];
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["en", "en"]));
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].candidate.score, 5.0);
    }

    // --- popularity and quality ---------------------------------------------------------------

    fn voted(url: &str, provider: &'static str, popularity: Popularity) -> SourcedArtwork {
        SourcedArtwork {
            provider,
            candidate: ArtworkCandidate {
                popularity,
                ..poster(url, Some("en"), 2000, 0.0).candidate
            },
        }
    }

    fn rated(average: f32, votes: u32) -> Popularity {
        Popularity::Rated { average, votes }
    }

    #[test]
    fn the_most_liked_image_wins_within_a_tier() {
        let list = [
            voted("few", "tmdb", rated(6.0, 2)),
            voted("many", "tmdb", rated(6.0, 40)),
            voted("none", "tmdb", rated(0.0, 0)),
            voted("low", "tmdb", rated(3.0, 30)),
        ];
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["en"]));
        assert_eq!(urls(&ranked), ["many", "few", "none", "low"]);
    }

    #[test]
    fn one_high_vote_does_not_beat_fifty_good_ones() {
        let list = [
            voted("one-ten", "tmdb", rated(10.0, 1)),
            voted("fifty", "tmdb", rated(5.6, 50)),
        ];
        let best = select_best(&list, MediaFileType::Poster, &settings(&["en"])).unwrap();
        assert_eq!(best.candidate.url, "fifty");
        // Equal votes tie, and the provider order breaks the tie.
        let tie = [
            voted("first", "tmdb", rated(5.6, 50)),
            voted("second", "tmdb", rated(5.6, 50)),
        ];
        assert_eq!(
            urls(&rank(&tie, MediaFileType::Poster, &settings(&["en"]))),
            ["first", "second"]
        );
    }

    #[test]
    fn fanart_likes_are_weighed_against_tmdb_votes() {
        let list = [
            voted("tmdb-voted", "tmdb", rated(5.6, 50)),
            voted("fanart-liked", "fanart", Popularity::Liked { likes: 12 }),
            voted("fanart-one", "fanart", Popularity::Liked { likes: 1 }),
            voted("tmdb-unvoted", "tmdb", rated(0.0, 0)),
        ];
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["en"]));
        assert_eq!(
            urls(&ranked),
            ["fanart-liked", "tmdb-voted", "fanart-one", "tmdb-unvoted"]
        );
    }

    #[test]
    fn low_quality_images_follow_every_other_image() {
        let tiny = SourcedArtwork {
            candidate: ArtworkCandidate {
                width: 300,
                height: 450,
                ..voted("tiny-loved", "fanart", Popularity::Liked { likes: 50 }).candidate
            },
            ..voted("x", "fanart", Popularity::Unknown)
        };
        let square = SourcedArtwork {
            candidate: ArtworkCandidate {
                width: 1000,
                height: 1000,
                ..voted("square", "tmdb", rated(8.0, 90)).candidate
            },
            ..voted("x", "tmdb", Popularity::Unknown)
        };
        let list = [
            tiny,
            square,
            voted("plain-ja", "tmdb", rated(0.0, 0)),
            poster("ok", Some("en"), 500, 1.0),
        ];
        let ranked = rank(&list, MediaFileType::Poster, &settings(&["en"]));
        let order = urls(&ranked);
        assert_eq!(order.len(), 4, "nothing is dropped");
        assert_eq!(&order[2..], ["square", "tiny-loved"]);
        // Alone, a low-quality image is still taken.
        let only = [poster("small", Some("en"), 342, 9.0)];
        assert!(select_best(&only, MediaFileType::Poster, &settings(&["en"])).is_some());
    }

    #[test]
    fn the_picker_order_follows_the_automatic_order() {
        let list = [
            voted("tmdb-voted", "tmdb", rated(5.6, 50)),
            voted("fanart-liked", "fanart", Popularity::Liked { likes: 12 }),
            poster("ja", Some("ja"), 2000, 9.0),
            poster("tr", Some("tr"), 2000, 1.0),
            poster("en-big", Some("en"), 780, 9.0),
            voted("tmdb-unvoted", "tmdb", rated(0.0, 0)),
        ];
        let s = settings(&["tr", "en"]);
        let ranked = urls(&rank(&list, MediaFileType::Poster, &s));
        let mut by_key: Vec<&SourcedArtwork> = list.iter().collect();
        by_key.sort_by(|a, b| {
            best_score(b, MediaFileType::Poster, &s).total_cmp(&best_score(
                a,
                MediaFileType::Poster,
                &s,
            ))
        });
        assert_eq!(urls(&by_key), ranked);
        assert_eq!(ranked[0], "tr");
    }

    #[test]
    fn the_picker_key_puts_preferred_textless_fanart_first() {
        let s = settings(&["en"]);
        let textless = fanart("textless", None, 1920, 1.0);
        let english = fanart("english", Some("en"), 1920, 9.0);
        assert!(
            best_score(&textless, MediaFileType::Fanart, &s)
                > best_score(&english, MediaFileType::Fanart, &s)
        );
    }

    // --- picker score --------------------------------------------------------------------------

    #[test]
    fn matching_scores_follow_the_table() {
        let s = settings(&["en"]);
        let score = |i: &SourcedArtwork, kind| matching_score(i, kind, &s);
        let p = |lang, width| poster("p", lang, width, 1.0);
        assert_eq!(score(&p(Some("en"), 1000), MediaFileType::Poster), 90);
        assert_eq!(score(&p(Some("en"), 500), MediaFileType::Poster), 50);
        assert_eq!(score(&p(Some("ja"), 1000), MediaFileType::Poster), 40);
        assert_eq!(score(&p(Some("ja"), 500), MediaFileType::Poster), 0);
        // Without text counts as a language match for fanart that prefers it, not for posters.
        assert_eq!(score(&p(None, 1000), MediaFileType::Poster), 40);
        let textless_fanart = fanart("f", None, 1920, 1.0);
        assert_eq!(score(&textless_fanart, MediaFileType::Fanart), 90);
        let frame = frame("file:///f.jpg", MediaFileType::Fanart, 1920);
        assert_eq!(score(&frame, MediaFileType::Fanart), 30);
        // `none` in the list makes textless posters a language match.
        let with_none = settings(&["en", "none"]);
        assert_eq!(
            matching_score(&p(None, 1000), MediaFileType::Poster, &with_none),
            90
        );
    }
}

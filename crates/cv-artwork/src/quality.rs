//! How good an artwork candidate is apart from the language and size settings: how much people
//! like it, how big it is, and whether it is too small or the wrong shape for its type.
//!
//! Spec: `artwork` (Automatic best-image selection, Artwork picker). The automatic choice
//! ([`crate::select::rank`]) and the picker's "Best" order ([`crate::select::best_score`]) both
//! use [`quality`], so a scrape picks the image the picker lists first.
//!
//! # The formula
//!
//! `quality = 0.75 × popularity + 0.25 × resolution`, both from 0 to 1:
//!
//! * **popularity** is [`popularity_value`]: a Bayesian average of TMDB votes, Fanart.tv likes on
//!   a log scale, and the value of an image nobody voted on for providers that say nothing;
//! * **resolution** is the width against the width a type needs to look sharp
//!   ([`ideal_width`]), at most 1; an unknown width counts as half.
//!
//! # Low quality
//!
//! An image is [`is_low_quality`] when it is narrower than the smallest width its type should
//! have ([`minimum_width`], lowered to the user's preferred size when that is smaller), or when
//! its shape is far from the shape of its type (a square "poster", a tall "backdrop"). Such
//! images are never dropped: the automatic choice takes them only after every other image, and
//! the picker lists them behind "Show N more".

use cv_domain::MediaFileType;
use cv_providers::SourcedArtwork;
use cv_providers::artwork::popularity_value;
use cv_providers::model::ArtworkCandidate;

use crate::select::{SelectionSettings, SizeWish};

/// The share of popularity in [`quality`]; the rest is resolution.
pub const POPULARITY_WEIGHT: f32 = 0.75;

/// The width at which an image of `kind` is as sharp as it needs to be.
#[must_use]
pub fn ideal_width(kind: MediaFileType) -> u32 {
    use MediaFileType::*;
    match kind {
        Poster | SeasonPoster | Keyart => 2000,
        // A backdrop fills the whole window behind a title page; 4K ones stay sharp on large
        // and high-density screens, so a 3840 backdrop beats a 2048 one that is liked as much.
        Fanart | SeasonFanart | Extrafanart => 3840,
        Thumb | SeasonThumb | Extrathumb => 1920,
        Banner | SeasonBanner => 1000,
        Clearlogo => 800,
        Characterart => 512,
        _ => 1000,
    }
}

/// The smallest width an image of `kind` should have; narrower ones are low quality. `0` for
/// types without a minimum.
#[must_use]
pub fn minimum_width(kind: MediaFileType) -> u32 {
    use MediaFileType::*;
    match kind {
        Poster | SeasonPoster | Keyart => 500,
        Fanart | SeasonFanart | Extrafanart => 1280,
        Thumb | SeasonThumb | Extrathumb => 500,
        Banner | SeasonBanner => 600,
        Clearlogo => 300,
        Clearart | Disc | Squareart => 400,
        Characterart => 256,
        _ => 0,
    }
}

/// The width/height range an image of `kind` should fall in; `None` for types of any shape
/// (logos, clear art, character art).
#[must_use]
pub fn aspect_range(kind: MediaFileType) -> Option<(f32, f32)> {
    use MediaFileType::*;
    match kind {
        // 2:3 is 0.667; posters run from about 0.6 (27×40) to 0.75 (3:4 DVD covers).
        Poster | SeasonPoster | Keyart => Some((0.55, 0.8)),
        // 16:9 is 1.78; 16:10 and 1.85 framings are fine, a square or a strip is not.
        Fanart | SeasonFanart | Extrafanart | Thumb | SeasonThumb | Extrathumb => Some((1.4, 2.1)),
        // The banner standard is 758×140 (5.4).
        Banner | SeasonBanner => Some((3.5, 7.5)),
        Disc | Squareart => Some((0.85, 1.18)),
        _ => None,
    }
}

/// How sharp an image is for its type, from 0 to 1; an unknown width counts as half.
#[must_use]
pub fn resolution_value(kind: MediaFileType, width: u32) -> f32 {
    if width == 0 {
        return 0.5;
    }
    (width as f32 / ideal_width(kind) as f32).min(1.0)
}

/// How good a candidate is, from 0 to 1 (see the module docs).
#[must_use]
pub fn quality(candidate: &ArtworkCandidate) -> f32 {
    POPULARITY_WEIGHT * popularity_value(candidate.popularity)
        + (1.0 - POPULARITY_WEIGHT) * resolution_value(candidate.kind, candidate.width)
}

/// The smallest width the settings accept for `kind`: [`minimum_width`], unless the user asks
/// for a smaller preferred size, whose own minimum then counts.
#[must_use]
pub fn accepted_width(kind: MediaFileType, settings: &SelectionSettings) -> u32 {
    let minimum = minimum_width(kind);
    match settings.size_wish(kind) {
        SizeWish::Bucket(bucket) => {
            let wished = cv_providers::artwork::bucket_width(kind, bucket).unwrap_or(minimum);
            minimum.min(wished)
        }
        SizeWish::Any => minimum,
    }
}

/// Whether a candidate is too small or the wrong shape for its type (see the module docs). An
/// unknown size is never low quality.
#[must_use]
pub fn is_low_quality(item: &SourcedArtwork, settings: &SelectionSettings) -> bool {
    let candidate = &item.candidate;
    if candidate.width > 0 && candidate.width < accepted_width(candidate.kind, settings) {
        return true;
    }
    if candidate.width > 0
        && candidate.height > 0
        && let Some((low, high)) = aspect_range(candidate.kind)
    {
        let ratio = candidate.width as f32 / candidate.height as f32;
        return ratio < low || ratio > high;
    }
    false
}

#[cfg(test)]
mod tests {
    use cv_providers::model::Popularity;

    use super::*;

    fn poster(width: u32, height: u32, popularity: Popularity) -> SourcedArtwork {
        SourcedArtwork {
            provider: "tmdb",
            candidate: ArtworkCandidate {
                kind: MediaFileType::Poster,
                url: "u".to_owned(),
                language: Some("en".to_owned()),
                width,
                height,
                score: 0.0,
                popularity,
            },
        }
    }

    #[test]
    fn popularity_outweighs_resolution() {
        let liked = poster(1000, 1500, Popularity::Liked { likes: 10 });
        let big = poster(2000, 3000, Popularity::Unknown);
        assert!(quality(&liked.candidate) > quality(&big.candidate));
        // At the same popularity the bigger image wins.
        let small = poster(1000, 1500, Popularity::Unknown);
        assert!(quality(&big.candidate) > quality(&small.candidate));
        // Above the ideal width nothing more is gained.
        let huge = poster(4000, 6000, Popularity::Unknown);
        assert!((quality(&huge.candidate) - quality(&big.candidate)).abs() < 1e-6);
    }

    #[test]
    fn small_and_misshapen_images_are_low_quality_and_unknown_sizes_are_not() {
        let settings = SelectionSettings::default();
        let low =
            |width, height| is_low_quality(&poster(width, height, Popularity::Unknown), &settings);
        assert!(low(342, 513), "below the poster minimum");
        assert!(!low(500, 750));
        assert!(low(1000, 1000), "a square is not a poster");
        assert!(low(1920, 1080), "a backdrop is not a poster");
        assert!(!low(0, 0), "an unknown size is kept");
        assert!(!low(1000, 0), "an unknown height cannot be judged");
    }

    #[test]
    fn a_smaller_preferred_size_lowers_the_minimum() {
        let small = SelectionSettings {
            poster_size: cv_providers::SizeBucket::Small,
            ..SelectionSettings::default()
        };
        let item = poster(342, 513, Popularity::Unknown);
        assert!(is_low_quality(&item, &SelectionSettings::default()));
        assert!(!is_low_quality(&item, &small));
    }

    #[test]
    fn types_without_a_shape_take_any_shape() {
        let logo = SourcedArtwork {
            provider: "fanart",
            candidate: ArtworkCandidate {
                kind: MediaFileType::Clearlogo,
                url: "l".to_owned(),
                language: None,
                width: 800,
                height: 800,
                score: 0.0,
                popularity: Popularity::Unknown,
            },
        };
        assert!(!is_low_quality(&logo, &SelectionSettings::default()));
    }
}

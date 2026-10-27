//! What the artwork providers share: size buckets, the provider-side pre-sort and gathering
//! candidates from several providers.
//!
//! Spec: `artwork` (Artwork providers).
//!
//! * [`SizeBucket`] ranks an image by its width, per artwork type ([`size_bucket`]). The best-image
//!   selection asks for a preferred bucket and falls back to the nearest others.
//! * [`presort`] orders one provider's candidates best first: the preferred language, then the
//!   fallback language, then English, then images without text, then the rest; each group by the
//!   provider's own score, highest first. Providers that serve one image in several sizes keep the
//!   sizes of an image together, largest first.
//! * [`gather_artwork`] asks several providers at once and tags every candidate with the provider
//!   it came from, so the picker can filter by provider and the selection can weigh them all
//!   (a clearlogo from TMDB and one from Fanart.tv are both considered).

use std::sync::Arc;

use cv_domain::{MediaFileType, ProviderIds};

use crate::error::ProviderError;
use crate::model::{ArtworkCandidate, MediaKind, Popularity, RequestOptions};
use crate::traits::ArtworkProvider;

/// A size class of an image, with the weight the selection code uses to order them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SizeBucket {
    Small = 1,
    Medium = 2,
    Big = 4,
    Large = 8,
    XLarge = 16,
}

/// The size order that means "any size": what artwork types without buckets are asked for.
pub const MAX_SIZE_ORDER: u32 = 32;

impl SizeBucket {
    /// Every bucket, largest first.
    pub const ALL: [SizeBucket; 5] = [
        SizeBucket::XLarge,
        SizeBucket::Large,
        SizeBucket::Big,
        SizeBucket::Medium,
        SizeBucket::Small,
    ];

    /// The bucket's weight (1, 2, 4, 8 or 16).
    pub const fn order(self) -> u32 {
        self as u32
    }

    /// The bucket with this weight.
    pub fn from_order(order: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.order() == order)
    }
}

/// Minimum widths per bucket, largest bucket first. A bucket a type has no width for is skipped.
type Thresholds = [(SizeBucket, u32); 5];

const POSTER: Thresholds = [
    (SizeBucket::XLarge, 2000),
    (SizeBucket::Large, 1000),
    (SizeBucket::Big, 500),
    (SizeBucket::Medium, 342),
    (SizeBucket::Small, 185),
];

/// Fanart has no `Big` bucket.
const FANART: Thresholds = [
    (SizeBucket::XLarge, 3840),
    (SizeBucket::Large, 1920),
    (SizeBucket::Big, u32::MAX),
    (SizeBucket::Medium, 1280),
    (SizeBucket::Small, 300),
];

/// The notes list no width for the medium thumb; 500 sits between its neighbours.
const THUMB: Thresholds = [
    (SizeBucket::XLarge, 3840),
    (SizeBucket::Large, 1920),
    (SizeBucket::Big, 1000),
    (SizeBucket::Medium, 500),
    (SizeBucket::Small, 300),
];

fn thresholds(kind: MediaFileType) -> Option<&'static Thresholds> {
    match kind {
        MediaFileType::Poster | MediaFileType::SeasonPoster => Some(&POSTER),
        MediaFileType::Fanart | MediaFileType::SeasonFanart | MediaFileType::Extrafanart => {
            Some(&FANART)
        }
        MediaFileType::Thumb | MediaFileType::SeasonThumb | MediaFileType::Extrathumb => {
            Some(&THUMB)
        }
        _ => None,
    }
}

/// The largest bucket of `kind` whose minimum width is at most `width`. Types without buckets
/// (logos, clearart, banners, discs, keyart...) and images narrower than the smallest bucket have
/// none.
pub fn size_bucket(kind: MediaFileType, width: u32) -> Option<SizeBucket> {
    thresholds(kind)?
        .iter()
        .find(|(_, minimum)| *minimum <= width)
        .map(|(bucket, _)| *bucket)
}

/// The smallest width of `bucket` for `kind`; `None` for types without buckets and for a bucket
/// the type does not have (fanart has no `Big`).
pub fn bucket_width(kind: MediaFileType, bucket: SizeBucket) -> Option<u32> {
    thresholds(kind)?
        .iter()
        .find(|(b, minimum)| *b == bucket && *minimum != u32::MAX)
        .map(|(_, minimum)| *minimum)
}

impl ArtworkCandidate {
    /// The candidate's size bucket, from its type and width.
    pub fn size_bucket(&self) -> Option<SizeBucket> {
        size_bucket(self.kind, self.width)
    }

    /// The weight of [`Self::size_bucket`], `0` when the candidate has none.
    pub fn size_order(&self) -> u32 {
        self.size_bucket().map_or(0, SizeBucket::order)
    }
}

// --- provider order ---------------------------------------------------------------------------

/// The key of the frame-grab source: stills taken from the video itself (see `cv-artwork`). It is
/// not a network provider, so it is only ever named in provider order lists.
pub const FRAME_GRAB_ID: &str = "ffmpeg";

/// The order artwork providers are consulted in by default, per kind of library. Frame grabs
/// always rank below online artwork whatever their place here. Settings lets the user enable,
/// disable and reorder them per library type.
pub fn default_provider_order(kind: MediaKind) -> &'static [&'static str] {
    match kind {
        MediaKind::Movie => &["tmdb", "fanart", "imdb", "mpdb", FRAME_GRAB_ID, "tvdb"],
        MediaKind::Show => &["tvdb", "fanart", "tmdb", "imdb", FRAME_GRAB_ID],
    }
}

// --- provider-side pre-sort -------------------------------------------------------------------

/// The primary language subtag of a code, lower case: `pt-BR` and `PT_br` are both `pt`.
pub fn primary_language(code: &str) -> String {
    code.trim()
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// The primary language of the text on an image, or `None` when the image has no text: a missing
/// language and the placeholders providers use for "no language" (`00`, `xx`, `zxx`, `und`).
pub fn text_language(language: Option<&str>) -> Option<String> {
    let language = primary_language(language.unwrap_or(""));
    (!matches!(language.as_str(), "" | "00" | "xx" | "zxx" | "und")).then_some(language)
}

/// How well an image's text language fits the request: `0` the preferred language, `1` the
/// fallback language, `2` English, `3` no text, `4` anything else. Region suffixes are ignored
/// (`pt-BR` is `pt`), as are letter case and the placeholders providers use for "no language".
pub fn language_rank(language: Option<&str>, options: &RequestOptions) -> u8 {
    let Some(language) = text_language(language) else {
        return 3;
    };
    if language == primary_language(&options.language) {
        0
    } else if language == primary_language(&options.fallback_language) {
        1
    } else if language == "en" {
        2
    } else {
        4
    }
}

/// Orders one provider's candidates best first: by artwork type, then [`language_rank`], then
/// [`popularity_value`], then the provider's score, highest first. The sort is stable, so sizes
/// of one image stay together and candidates with equal keys keep the order the provider gave
/// them.
pub fn presort(candidates: &mut [ArtworkCandidate], options: &RequestOptions) {
    candidates.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| {
                language_rank(a.language.as_deref(), options)
                    .cmp(&language_rank(b.language.as_deref(), options))
            })
            .then_with(|| popularity_value(b.popularity).total_cmp(&popularity_value(a.popularity)))
            .then_with(|| b.score.total_cmp(&a.score))
    });
}

// --- popularity -------------------------------------------------------------------------------

/// The votes every rated image starts with, at [`PRIOR_AVERAGE`]: a few votes cannot outweigh
/// many (one vote of 10 stays below fifty votes of 5.6).
pub const PRIOR_VOTES: f32 = 8.0;

/// The average those starting votes have. It sits below what voted images usually reach, so an
/// image nobody voted on ranks under one that people rate as average.
pub const PRIOR_AVERAGE: f32 = 4.0;

/// The vote averages that map to 0 and 1.
const RATED_FLOOR: f32 = 2.0;
const RATED_CEILING: f32 = 8.0;

/// The likes at which a Fanart.tv image counts as liked by everybody; the scale is logarithmic.
pub const FULL_LIKES: f32 = 60.0;

/// How much people like an image, from 0 to 1, comparable across providers.
///
/// * **Votes** (TMDB) use a Bayesian average: `(PRIOR_VOTES × PRIOR_AVERAGE + votes × average)
///   / (PRIOR_VOTES + votes)`, then 2 to 8 on that scale becomes 0 to 1.
/// * **Likes** (Fanart.tv) count on a log scale from the same starting point as an image without
///   votes: `start + (1 - start) × ln(1 + likes) / ln(1 + FULL_LIKES)`, at most 1.
/// * **Unknown** is that starting point, the value of an image nobody voted on.
#[must_use]
pub fn popularity_value(popularity: Popularity) -> f32 {
    let rated =
        |average: f32| ((average - RATED_FLOOR) / (RATED_CEILING - RATED_FLOOR)).clamp(0.0, 1.0);
    let start = rated(PRIOR_AVERAGE);
    match popularity {
        Popularity::Unknown => start,
        Popularity::Rated { average, votes } => {
            let votes = votes as f32;
            let average = if average.is_finite() {
                average.clamp(0.0, 10.0)
            } else {
                0.0
            };
            rated((PRIOR_VOTES * PRIOR_AVERAGE + votes * average) / (PRIOR_VOTES + votes))
        }
        Popularity::Liked { likes } => {
            let share = ((1.0 + likes as f32).ln() / (1.0 + FULL_LIKES).ln()).min(1.0);
            start + (1.0 - start) * share
        }
    }
}

// --- gathering --------------------------------------------------------------------------------

/// A candidate with the provider it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcedArtwork {
    /// Key of the provider (`tmdb`, `fanart`, `ffmpeg`...).
    pub provider: &'static str,
    pub candidate: ArtworkCandidate,
}

/// What [`gather_artwork`] found.
#[derive(Debug, Default)]
pub struct Gathered {
    /// Every candidate, grouped by provider in the order the providers were given, each group
    /// best first.
    pub candidates: Vec<SourcedArtwork>,
    /// The providers that failed, with why. A provider that does not know the title is not a
    /// failure; it just offers nothing.
    pub failures: Vec<(&'static str, ProviderError)>,
}

/// What a gathering asks every provider for.
#[derive(Clone)]
enum Scope {
    /// The images of a movie or show.
    Item(MediaKind),
    /// The images of one season of a show.
    Season(u32),
}

/// Asks every provider for its artwork at once. One provider failing (no credentials, a site that
/// changed) never costs the others' candidates.
pub async fn gather_artwork(
    providers: &[Arc<dyn ArtworkProvider>],
    kind: MediaKind,
    ids: &ProviderIds,
    options: &RequestOptions,
) -> Gathered {
    gather_scoped(providers, Scope::Item(kind), ids, options).await
}

/// As [`gather_artwork`], for one season of the show with `show` IDs (`0` is the specials). The
/// candidates are season posters, fanart, banners and thumbs; a provider without season images
/// offers nothing.
pub async fn gather_season_artwork(
    providers: &[Arc<dyn ArtworkProvider>],
    show: &ProviderIds,
    season: u32,
    options: &RequestOptions,
) -> Gathered {
    gather_scoped(providers, Scope::Season(season), show, options).await
}

async fn gather_scoped(
    providers: &[Arc<dyn ArtworkProvider>],
    scope: Scope,
    ids: &ProviderIds,
    options: &RequestOptions,
) -> Gathered {
    let mut tasks = tokio::task::JoinSet::new();
    for (index, provider) in providers.iter().enumerate() {
        let (provider, ids, options, scope) = (
            Arc::clone(provider),
            ids.clone(),
            options.clone(),
            scope.clone(),
        );
        tasks.spawn(async move {
            let id = provider.id();
            let answer = match scope {
                Scope::Item(kind) => provider.artwork(kind, &ids, &options).await,
                Scope::Season(season) => provider.season_artwork(&ids, season, &options).await,
            };
            (index, id, answer)
        });
    }
    let mut answers = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok(answer) => answers.push(answer),
            Err(error) => tracing::warn!(%error, "an artwork provider task stopped"),
        }
    }
    answers.sort_by_key(|(index, _, _)| *index);

    let mut gathered = Gathered::default();
    for (_, provider, answer) in answers {
        match answer {
            Ok(list) => gathered
                .candidates
                .extend(list.into_iter().map(|candidate| SourcedArtwork {
                    provider,
                    candidate,
                })),
            Err(ProviderError::NotFound(_)) => {}
            Err(error) => gathered.failures.push((provider, error)),
        }
    }
    gathered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(kind: MediaFileType, language: Option<&str>, score: f32) -> ArtworkCandidate {
        ArtworkCandidate {
            kind,
            url: format!("https://i.test/{score}-{language:?}"),
            language: language.map(str::to_owned),
            width: 1000,
            height: 1500,
            score,
            popularity: Popularity::Unknown,
        }
    }

    #[test]
    fn default_orders_differ_per_library_type() {
        assert_eq!(default_provider_order(MediaKind::Movie)[0], "tmdb");
        assert_eq!(default_provider_order(MediaKind::Show)[0], "tvdb");
        for kind in [MediaKind::Movie, MediaKind::Show] {
            let order = default_provider_order(kind);
            assert!(order.contains(&"fanart") && order.contains(&FRAME_GRAB_ID));
        }
        // MPDb.tv only knows movies.
        assert!(!default_provider_order(MediaKind::Show).contains(&"mpdb"));
    }

    #[test]
    fn poster_buckets_follow_the_width_table() {
        let bucket = |w| size_bucket(MediaFileType::Poster, w);
        assert_eq!(bucket(3000), Some(SizeBucket::XLarge));
        assert_eq!(bucket(2000), Some(SizeBucket::XLarge));
        assert_eq!(bucket(1999), Some(SizeBucket::Large));
        assert_eq!(bucket(1000), Some(SizeBucket::Large));
        assert_eq!(bucket(780), Some(SizeBucket::Big));
        assert_eq!(bucket(500), Some(SizeBucket::Big));
        assert_eq!(bucket(342), Some(SizeBucket::Medium));
        assert_eq!(bucket(185), Some(SizeBucket::Small));
        assert_eq!(bucket(184), None);
        assert_eq!(bucket(0), None);
    }

    #[test]
    fn fanart_has_no_big_bucket() {
        let bucket = |w| size_bucket(MediaFileType::Fanart, w);
        assert_eq!(bucket(3840), Some(SizeBucket::XLarge));
        assert_eq!(bucket(1920), Some(SizeBucket::Large));
        // 1280 to 1919 is medium; nothing lands between large and medium.
        assert_eq!(bucket(1919), Some(SizeBucket::Medium));
        assert_eq!(bucket(1280), Some(SizeBucket::Medium));
        assert_eq!(bucket(300), Some(SizeBucket::Small));
        assert_eq!(bucket(299), None);
    }

    #[test]
    fn thumb_buckets_and_the_types_that_share_a_table() {
        assert_eq!(
            size_bucket(MediaFileType::Thumb, 1000),
            Some(SizeBucket::Big)
        );
        assert_eq!(
            size_bucket(MediaFileType::Thumb, 300),
            Some(SizeBucket::Small)
        );
        assert_eq!(
            size_bucket(MediaFileType::SeasonPoster, 1000),
            size_bucket(MediaFileType::Poster, 1000)
        );
        assert_eq!(
            size_bucket(MediaFileType::SeasonFanart, 1920),
            size_bucket(MediaFileType::Fanart, 1920)
        );
        assert_eq!(
            size_bucket(MediaFileType::Extrathumb, 1920),
            Some(SizeBucket::Large)
        );
    }

    #[test]
    fn types_without_buckets_have_none() {
        for kind in [
            MediaFileType::Clearlogo,
            MediaFileType::Clearart,
            MediaFileType::Banner,
            MediaFileType::Disc,
            MediaFileType::Keyart,
        ] {
            assert_eq!(size_bucket(kind, 4000), None, "{kind:?}");
        }
        let logo = candidate(MediaFileType::Clearlogo, None, 1.0);
        assert_eq!(logo.size_order(), 0);
        assert_eq!(MAX_SIZE_ORDER, 32);
    }

    #[test]
    fn candidates_report_their_bucket_and_weight() {
        let poster = candidate(MediaFileType::Poster, None, 1.0);
        assert_eq!(poster.size_bucket(), Some(SizeBucket::Large));
        assert_eq!(poster.size_order(), 8);
        assert_eq!(SizeBucket::from_order(8), Some(SizeBucket::Large));
        assert_eq!(SizeBucket::from_order(3), None);
        let weights: Vec<u32> = SizeBucket::ALL.iter().map(|b| b.order()).collect();
        assert_eq!(weights, [16, 8, 4, 2, 1]);
    }

    #[test]
    fn language_ranks_ignore_region_case_and_placeholders() {
        let options = RequestOptions {
            language: "pt-BR".into(),
            fallback_language: "de".into(),
            ..RequestOptions::default()
        };
        assert_eq!(language_rank(Some("PT"), &options), 0);
        assert_eq!(language_rank(Some("pt_PT"), &options), 0);
        assert_eq!(language_rank(Some("de"), &options), 1);
        assert_eq!(language_rank(Some("en"), &options), 2);
        for none in [None, Some(""), Some(" "), Some("00"), Some("xx")] {
            assert_eq!(language_rank(none, &options), 3, "{none:?}");
        }
        assert_eq!(language_rank(Some("ja"), &options), 4);
    }

    #[test]
    fn presort_puts_language_before_score_and_keeps_kinds_apart() {
        let options = RequestOptions {
            language: "tr".into(),
            fallback_language: "tr".into(),
            ..RequestOptions::default()
        };
        let mut list = vec![
            candidate(MediaFileType::Poster, Some("ja"), 9.0),
            candidate(MediaFileType::Fanart, None, 1.0),
            candidate(MediaFileType::Poster, None, 8.0),
            candidate(MediaFileType::Poster, Some("en"), 2.0),
            candidate(MediaFileType::Poster, Some("tr"), 1.0),
            candidate(MediaFileType::Poster, Some("en"), 6.0),
        ];
        presort(&mut list, &options);
        let order: Vec<(MediaFileType, Option<&str>, f32)> = list
            .iter()
            .map(|c| (c.kind, c.language.as_deref(), c.score))
            .collect();
        assert_eq!(
            order,
            vec![
                (MediaFileType::Poster, Some("tr"), 1.0),
                (MediaFileType::Poster, Some("en"), 6.0),
                (MediaFileType::Poster, Some("en"), 2.0),
                (MediaFileType::Poster, None, 8.0),
                (MediaFileType::Poster, Some("ja"), 9.0),
                (MediaFileType::Fanart, None, 1.0),
            ]
        );
    }

    #[test]
    fn presort_is_stable_for_equal_keys() {
        let options = RequestOptions::default();
        let mut list: Vec<ArtworkCandidate> = (0..4)
            .map(|i| ArtworkCandidate {
                url: format!("u{i}"),
                ..candidate(MediaFileType::Poster, Some("en"), 3.0)
            })
            .collect();
        presort(&mut list, &options);
        let urls: Vec<&str> = list.iter().map(|c| c.url.as_str()).collect();
        assert_eq!(urls, ["u0", "u1", "u2", "u3"]);
    }

    #[test]
    fn a_few_high_votes_do_not_beat_many_good_ones() {
        let one_ten = popularity_value(Popularity::Rated {
            average: 10.0,
            votes: 1,
        });
        let fifty = popularity_value(Popularity::Rated {
            average: 5.6,
            votes: 50,
        });
        assert!(fifty > one_ten, "{fifty} > {one_ten}");
        // Without votes an image sits at the start, below a voted average image.
        let none = popularity_value(Popularity::Rated {
            average: 0.0,
            votes: 0,
        });
        assert!((none - popularity_value(Popularity::Unknown)).abs() < 1e-6);
        let average = popularity_value(Popularity::Rated {
            average: 5.0,
            votes: 20,
        });
        assert!(average > none);
        // More votes at the same average weigh more.
        let few = popularity_value(Popularity::Rated {
            average: 7.0,
            votes: 3,
        });
        let many = popularity_value(Popularity::Rated {
            average: 7.0,
            votes: 30,
        });
        assert!(many > few);
    }

    #[test]
    fn likes_count_on_a_log_scale_from_the_same_start() {
        let value = |likes| popularity_value(Popularity::Liked { likes });
        assert!((value(0) - popularity_value(Popularity::Unknown)).abs() < 1e-6);
        assert!(value(1) > value(0));
        assert!(value(10) > value(3));
        // The steps shrink: the first likes matter most.
        assert!(value(3) - value(0) > value(30) - value(27));
        assert!((value(60) - 1.0).abs() < 1e-6);
        assert!((value(5_000) - 1.0).abs() < 1e-6);
        // A handful of likes stands level with a well-voted TMDB image.
        let voted = popularity_value(Popularity::Rated {
            average: 5.6,
            votes: 50,
        });
        assert!(value(1) < voted && value(5) > voted);
    }

    #[test]
    fn odd_vote_averages_stay_in_range() {
        for average in [f32::NAN, f32::INFINITY, -3.0, 42.0] {
            let value = popularity_value(Popularity::Rated { average, votes: 9 });
            assert!((0.0..=1.0).contains(&value), "{average}: {value}");
        }
    }

    #[test]
    fn presort_orders_by_popularity_within_a_language() {
        let options = RequestOptions::default();
        let liked = |url: &str, likes| ArtworkCandidate {
            url: url.to_owned(),
            popularity: Popularity::Liked { likes },
            ..candidate(MediaFileType::Poster, Some("en"), 0.0)
        };
        let mut list = vec![liked("one", 1), liked("ten", 10), liked("none", 0)];
        presort(&mut list, &options);
        let urls: Vec<&str> = list.iter().map(|c| c.url.as_str()).collect();
        assert_eq!(urls, ["ten", "one", "none"]);
    }
}

//! The data providers take and return.

use cv_domain::{EpisodeOrdering, Item, MediaFileType, MissingMovie, ProviderIds};

/// What a lookup is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Movie,
    Show,
}

/// A collection (movie set) at a provider: the set item and the movies the provider lists in it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderCollection {
    /// A set item with the collection's name, plot and artwork URLs.
    pub item: Item,
    /// Every movie of the collection, in the provider's order. The caller decides which of them
    /// the library already has.
    pub movies: Vec<MissingMovie>,
}

/// Per-request settings that apply to every provider call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestOptions {
    /// Preferred metadata language, for example `en` or `tr`.
    pub language: String,
    /// Used for titles and plots the preferred language lacks.
    pub fallback_language: String,
    /// Country for certifications (and release dates when [`Self::local_release_date`] is on),
    /// for example `US`.
    pub country: String,
    /// Use the release date of [`Self::country`] instead of the film's original (worldwide)
    /// release date. Off by default: the premiered date is the original release.
    pub local_release_date: bool,
    /// Skip the response cache (forced rescrapes).
    pub bypass_cache: bool,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            language: "en".to_owned(),
            fallback_language: "en".to_owned(),
            country: "US".to_owned(),
            local_release_date: false,
            bypass_cache: false,
        }
    }
}

/// A text search with an optional year.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub kind: MediaKind,
    pub title: String,
    pub year: Option<u16>,
}

/// One candidate from a search or an ID lookup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchHit {
    /// Key of the provider that found it.
    pub provider: String,
    /// Every ID the provider knows for it, including its own.
    pub ids: ProviderIds,
    pub title: String,
    pub original_title: String,
    /// Release year, `0` when unknown.
    pub year: u16,
    pub overview: String,
    pub poster_url: Option<String>,
    /// Found by one of the item's IDs rather than by text, so it is the requested title and
    /// matching scores it 1.0.
    pub exact_id: bool,
    /// The provider's popularity figure for the title (TMDB's `popularity`), `0.0` when unknown.
    /// Ranking uses it to put the likelier of two same-named titles first.
    pub popularity: f32,
    /// How many votes the provider has for the title, `0` when unknown.
    pub votes: u32,
}

/// What a metadata provider can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub movies: bool,
    pub shows: bool,
    /// Episode orderings it can return, for TV providers.
    pub orderings: Vec<EpisodeOrdering>,
    /// ID keys it can look up directly (see `cv_domain::id_key`).
    pub lookup_ids: Vec<&'static str>,
    /// Backed by website parsing, so it may break when the site changes.
    pub best_effort: bool,
}

/// An image a provider offers for an item.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtworkCandidate {
    pub kind: MediaFileType,
    pub url: String,
    /// Language of text on the image, if any.
    pub language: Option<String>,
    pub width: u32,
    pub height: u32,
    /// The provider's own vote score, `0.0` when unknown.
    pub score: f32,
    /// How much people like the image at the provider: the votes TMDB keeps, the likes
    /// Fanart.tv counts. [`Popularity::Unknown`] for providers without either.
    pub popularity: Popularity,
}

/// What people at a provider think of an image (see `crate::artwork::popularity_value`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Popularity {
    /// The provider says nothing about it.
    #[default]
    Unknown,
    /// An average of votes on a 0 to 10 scale, and how many votes it is made of (TMDB).
    Rated { average: f32, votes: u32 },
    /// A count of likes (Fanart.tv).
    Liked { likes: u32 },
}

impl Popularity {
    /// The number of people behind it: the votes or the likes, `0` when unknown.
    #[must_use]
    pub fn count(self) -> u32 {
        match self {
            Self::Unknown => 0,
            Self::Rated { votes, .. } => votes,
            Self::Liked { likes } => likes,
        }
    }
}

//! Metadata, artwork and rating providers with shared HTTP, cache and rate limiting.
//!
//! * [`traits`] and [`model`] define what a provider offers and what it returns.
//! * [`credentials`] holds the credential schema and the resolver that picks a value: the
//!   user's own key, or none (CineVault ships no keys). [`catalogue`] lists the providers that
//!   need credentials and how important each is.
//! * [`http`] is the shared HTTP client every provider sends requests through: per-provider rate
//!   limits and concurrency, 429 handling, a disk cache with forced TTLs, request coalescing and
//!   credential redaction.
//! * [`tmdb`] is the TMDB provider: the most complete implementation of the provider traits.
//! * [`tvdb`] is the TheTVDB provider: key plus optional subscriber PIN, every episode ordering
//!   and artwork.
//! * [`imdb`] is the IMDb provider: titles read from the website's `__NEXT_DATA__` JSON. It is
//!   best-effort, and a page it cannot read fails with a "page format changed" error.
//! * [`omdb`], [`trakt`], [`tvmaze`] and [`moviemeter`] are four more metadata providers: OMDb
//!   (IMDb ID in, details and IMDb, Metacritic and Rotten Tomatoes ratings out), Trakt (public
//!   metadata with the app's client ID; the OAuth sync is separate), TVmaze (TV shows with
//!   alternate episode orderings, no key) and MovieMeter (Dutch movie data with a personal key).
//! * [`anidb`], [`mpdb`], [`ofdb`], [`fernsehserien`] and [`thesportsdb`] are five more metadata
//!   providers: AniDB (anime, searched in its title dump, one request per two seconds), MPDb.tv
//!   (a private French movie service), OFDb and fernsehserien.de (German websites, best-effort),
//!   and TheSportsDB (leagues as shows, events as episodes).
//! * [`anime_db`] downloads the anime offline ID database on demand into the cache folder and
//!   converts it to the [`AnimeCrossReference`] rows [`complete_ids`] uses.
//! * [`completion`] fills a title's missing IMDb, TMDB, TVDB, Trakt and collection IDs from the
//!   providers that know them. [`attribution`] holds the credit each provider's terms require.
//! * [`universal`] is the universal provider: one title assembled field by field from several
//!   providers, with cross-provider ID resolution. [`fallback`] is the ordered chain of providers
//!   tried when the primary one finds nothing.
//! * [`kodi`] runs Kodi XML scrapers: an interpreter of the ScraperXML language, discovery of
//!   installed metadata add-ons (Python ones are listed as unsupported) and a provider per scraper.
//! * [`ratings`] fetches a title's ratings from the scraper, OMDb, MDBList and the IMDb ratings
//!   dataset, skipping the sources that have no credentials, and holds the fetch settings.
//! * [`artwork`] is what the artwork providers share: size buckets, the provider-side pre-sort
//!   and gathering the candidates of several providers. [`fanart`] is the Fanart.tv provider
//!   (artwork only); TMDB, TheTVDB, IMDb, MPDb.tv and OFDb offer artwork too.
//! * [`registry`] collects the provider implementations so Settings can test their credentials.

pub mod anidb;
pub mod anime_db;
pub mod artwork;
pub mod attribution;
pub mod catalogue;
pub mod completion;
pub mod credentials;
pub mod error;
pub mod fallback;
pub mod fanart;
pub mod fernsehserien;
mod german;
pub mod http;
pub mod imdb;
pub mod kodi;
mod markup;
pub mod model;
pub mod moviemeter;
pub mod mpdb;
pub mod ofdb;
pub mod omdb;
pub mod ratings;
pub mod registry;
mod shared;
pub mod thesportsdb;
pub mod tmdb;
pub mod traits;
pub mod trakt;
pub mod tvdb;
pub mod tvmaze;
pub mod universal;
mod webpage;

pub use anidb::AnidbProvider;
pub use anime_db::AnimeOfflineDb;
pub use artwork::{SizeBucket, SourcedArtwork, gather_artwork, gather_season_artwork};
pub use attribution::{ATTRIBUTIONS, Attribution};
pub use completion::{AnimeCrossReference, CompletedIds, complete_ids};
pub use credentials::{
    CredentialError, CredentialResolver, CredentialSchema, CredentialState, CredentialStatus,
    CredentialValue, FieldKind, FieldStatus, Importance, MemoryUserCredentials, RejectionLog,
    ResolvedCredentials, UserCredentials, credential_account,
};
pub use error::ProviderError;
pub use fallback::{FallbackChain, FallbackScrape, FallbackSearch};
pub use fanart::FanartProvider;
pub use fernsehserien::FernsehserienProvider;
pub use http::{HttpClient, HttpRequest, HttpResponse};
pub use imdb::{ImdbProvider, Top250Entry, Top250List, Top250Source};
pub use kodi::{KodiCatalogue, KodiProvider};
pub use moviemeter::MovieMeterProvider;
pub use mpdb::MpdbProvider;
pub use ofdb::OfdbProvider;
pub use omdb::OmdbProvider;
pub use ratings::{
    FetchedRatings, ImdbRatingsDataset, MdbListProvider, RatingFetchSettings, RatingFetcher,
    RatingNotice, RatingSettings, RatingSource, RunNotices, SkipReason, SourceOutcome,
    SourceStatus,
};
pub use registry::ProviderRegistry;
pub use thesportsdb::TheSportsDbProvider;
pub use tmdb::TmdbProvider;
pub use traits::{ArtworkProvider, MetadataProvider, Provider, RatingProvider};
pub use trakt::TraktProvider;
pub use tvdb::TvdbProvider;
pub use tvmaze::TvmazeProvider;
pub use universal::{UNIVERSAL_ID, UniversalConfig, UniversalProvider, UniversalScrape};

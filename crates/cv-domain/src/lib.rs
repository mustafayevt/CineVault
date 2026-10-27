//! Domain model: library items, media files, streams, people, ratings, IDs and open-ended value types.
//!
//! This crate is pure data: no I/O, no database and no provider logic. Every type derives
//! `serde` so the storage, NFO, API and IPC layers can share one vocabulary.
//!
//! Conventions:
//! - Struct fields serialise in `camelCase`; closed enums in `snake_case`.
//! - Structs use `#[serde(default)]`, so older or partial documents still load.
//! - Open-ended values ([`Genre`], [`MediaSource`], [`Edition`]) have a built-in set of known
//!   values and keep anything else as free text (`Other`). They never discard input.
//! - Provider IDs are an open string map ([`ProviderIds`]); numbers are accepted on input and
//!   empty, `0` and `null` values are dropped.

mod availability;
mod country;
mod date;
mod episode;
mod item;
mod media_file;
mod open_enum;
mod people;
mod provider_ids;
mod rating;
mod rating_preference;
mod review;
mod scrape;
mod source;
mod trailer;
mod trailer_pick;
mod web;

pub use availability::Availability;
pub use country::{country_code, country_display, country_name, country_stored};
pub use date::{CalendarDate, DateParseError};
pub use episode::{EpisodeGroup, EpisodeNumber, EpisodeNumbers, EpisodeOrdering};
pub use item::{
    AiredStatus, Certification, EpisodeFields, Item, ItemCore, ItemDetail, ItemId, ItemKind,
    MissingEpisode, MissingMovie, MovieFields, RenameHistory, RenameStep, SeasonFields, SeasonText,
    SetFields, ShowFields, Tags,
};
pub use media_file::{
    AudioStream, HdrDetails, MediaFile, MediaFileType, StreamFlag, SubtitleStream, Video3dFormat,
};
pub use open_enum::{EDITION_SPELLINGS, Edition, Genre, MediaSource};
pub use people::{Credit, CreditKind, Person, merge_crew};
pub use provider_ids::{ProviderIds, id_key};
pub use rating::{
    Rating, Ratings, UserRatingError, canonical_rating_id, check_user_rating, known_max_value,
    rating_key, user_rating_from_stars,
};
pub use rating_preference::{DEFAULT_PREFERRED, FALLBACK_ORDER, PickedRating, RatingPreference};
pub use review::ReviewState;
pub use scrape::{
    FieldChange, FieldComparison, FieldSet, MetaField, ScrapeBlock, ScrapeOptions, ScrapeOutcome,
    UnknownField, all_fields, apply_scrape, clear_for_relink, compare_field, revert_locked,
};
pub use source::{DataSource, SourceKind};
pub use trailer::Trailer;
pub use trailer_pick::{
    TrailerPreferences, TrailerQuality, download_candidates, language_code, mark_preferred,
    preferred_index, same_language,
};
pub use web::web_address;

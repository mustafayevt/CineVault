//! Torrent search.
//!
//! The crate holds everything behind the Torrent search page that is not the app glue:
//!
//! * [`model`]: the query, the raw and merged releases and the source types;
//! * [`query`]: title spellings and the text queries sent to the sources;
//! * [`release`]: reading a release name (resolution, source, codec, HDR, audio, season, flags);
//! * [`magnet`]: info-hash normalisation, magnet links and their validation;
//! * [`merge`] and [`rank`]: one list out of many sources, relevance, order, flags, upgrade marker;
//! * [`torznab`]: the Torznab and RSS readers;
//! * [`source`] and [`sources`]: the [`source::TorrentSource`] trait and one module per source;
//! * [`search`]: the orchestrator that runs the sources at once;
//! * [`latest`]: the "Latest releases" feed: each source's newest releases, page by page;
//! * [`indexer_service`]: the managed indexer service (process, admin API, provisioning).
//!
//! The pure modules (query, release, magnet, merge, rank, torznab) do no I/O and are table-tested.
//! No test in this crate touches the live network.

pub mod cancel;
pub mod indexer_service;
pub mod latest;
pub mod magnet;
pub mod merge;
pub mod model;
pub mod query;
pub mod rank;
pub mod release;
pub mod search;
pub mod source;
pub mod sources;
pub mod torznab;

pub use cancel::CancelToken;
pub use model::*;

/// Errors of the torrent code that are not a single source failing (those are
/// [`model::SourceError`]).
#[derive(Debug, thiserror::Error)]
pub enum TorrentError {
    /// The responsibility notice was not accepted yet; nothing was contacted.
    #[error("Accept the Torrent search notice first. Nothing was contacted.")]
    NoticeNotAccepted,
    /// A setting, a URL or a value could not be used.
    #[error("{0}")]
    Invalid(String),
    /// The indexer service failed.
    #[error("indexer service: {0}")]
    IndexerService(String),
    /// Reading or writing a file failed.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

//! The transport-neutral command layer shared by the desktop app and the browser server.
//!
//! Everything that decides what a command does lives here, next to the services it uses. The
//! desktop app wraps each command in a Tauri handler; the server calls the same function from its
//! HTTP dispatcher. Neither adapter owns behaviour, so a fix or a feature is written once.
//!
//! The pieces that are not commands live here too: [`AppContext`] builds the one set of services
//! both surfaces use, [`EventSink`] carries the typed events, and [`Hub`] is the replaying fan-out
//! behind job progress and notices.

pub mod artwork;
pub mod artwork_stream;
pub mod collections;
pub mod context;
pub mod detail;
pub mod drives;
pub mod edit;
pub mod episode_scrape;
pub mod error;
pub mod events;
pub mod history;
pub mod hub;
pub mod image;
pub mod jobs;
pub mod library;
pub mod library_facets;
pub mod library_ops;
pub mod lifecycle;
pub mod matching;
pub mod nfo;
pub mod notices;
pub mod onboarding;
pub mod organize;
pub mod people;
pub mod provider_stream;
pub mod scan;
pub mod scrape_preview;
pub mod scrape_stream;
pub mod settings;
pub mod sink;
pub mod sources;
pub mod tools;
pub mod torrent;
pub mod torrent_indexer;
pub mod torrent_stream;
pub mod tv;
pub mod updates;

pub use context::AppContext;
pub use error::{CommandError, CommandResult};
pub use events::{EventSink, NoopEventSink};
pub use hub::Hub;

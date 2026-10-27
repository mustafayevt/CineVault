//! Fetching and storing the IMDb Top 250 positions.
//!
//! "Fetch IMDb Top 250" reads the chart (movies or TV shows) from a [`Top250Source`] and stores
//! each title's position on the matching library item (found by its IMDb ID). Titles that fell off
//! the chart lose their position; locked titles keep theirs. The positions are shown as a column
//! and a filter in the library, and are written to the NFO and available as a renamer token.
//!
//! Nothing is stored unless the whole chart was read: a source error leaves every position as it
//! was.

use cv_db::repo::{Top250Report, apply_top250};
use cv_db::{Database, DbError};
use cv_domain::ItemKind;
use cv_providers::model::RequestOptions;
use cv_providers::{ProviderError, Top250List, Top250Source};

/// Why the Top 250 could not be fetched and stored.
#[derive(Debug, thiserror::Error)]
pub enum Top250Error {
    /// The chart could not be fetched or read; nothing was stored.
    #[error(transparent)]
    Source(#[from] ProviderError),
    /// The library could not be read or written.
    #[error(transparent)]
    Storage(#[from] DbError),
}

/// The library kind a chart ranks.
pub fn kind_of(list: Top250List) -> ItemKind {
    match list {
        Top250List::Movies => ItemKind::Movie,
        Top250List::Shows => ItemKind::Show,
    }
}

/// Fetches the chart `list` from `source` and stores the positions.
///
/// # Errors
/// [`Top250Error::Source`] when the chart cannot be read, [`Top250Error::Storage`] when the
/// library cannot be written. Neither changes any position.
pub async fn refresh_top250(
    db: &Database,
    source: &dyn Top250Source,
    list: Top250List,
    options: &RequestOptions,
) -> Result<Top250Report, Top250Error> {
    let entries = source.top250(list, options).await?;
    let ranks: Vec<(String, u32)> = entries
        .into_iter()
        .map(|entry| (entry.imdb_id, entry.rank))
        .collect();
    let kind = kind_of(list);
    Ok(db.write(move |tx| apply_top250(tx, kind, &ranks))?)
}

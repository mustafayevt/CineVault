//! One indexer of the managed indexer service as a source.
//!
//! The service serves every indexer it holds as its own Torznab feed on the loopback port:
//! `http://127.0.0.1:<port>/api/v2.0/indexers/<id>/results/torznab/api`, with the service's API key.
//! The source is the Torznab reader on that feed, so what is asked follows what the feed says it
//! can do: a search by IMDb ID (`t=movie`, `t=tvsearch` with season and episode) where the caps
//! allow it, otherwise the text queries of the title. All indexers share the loopback lane
//! ([`INDEXER_LANE`]), which is wide because the service is on this computer.

use async_trait::async_trait;
use cv_providers::http::HttpClient;

use super::torznab::TorznabSource;
use crate::model::{RawRelease, SourceError, SourceId, SourceKind, Support, TorrentQuery};
use crate::source::{INDEXER_LANE, SearchContext, SourceHttp, TorrentSource};

/// The prefix of an indexer's source id: `indexer:<service id>`.
pub const ID_PREFIX: &str = "indexer:";

/// The Torznab feed of the indexer `indexer_id` of the service at `base_url`
/// (`http://127.0.0.1:9117`).
#[must_use]
pub fn feed_url(base_url: &str, indexer_id: &str) -> String {
    format!(
        "{}/api/v2.0/indexers/{indexer_id}/results/torznab/api",
        base_url.trim_end_matches('/')
    )
}

/// An indexer of the service, searched through its Torznab feed.
pub struct IndexerSource {
    inner: TorznabSource,
}

impl IndexerSource {
    /// A source for the indexer `indexer_id` (named `name` on the page) of the service at
    /// `base_url`, which accepts `api_key`.
    #[must_use]
    pub fn new(
        client: HttpClient,
        base_url: &str,
        api_key: &str,
        indexer_id: &str,
        name: &str,
    ) -> Self {
        let inner = TorznabSource::new(
            SourceHttp::on_lane(client, INDEXER_LANE),
            &format!("{ID_PREFIX}{indexer_id}"),
            name,
            SourceKind::Indexer,
            &feed_url(base_url, indexer_id),
            Some(api_key),
        );
        Self { inner }
    }
}

#[async_trait]
impl TorrentSource for IndexerSource {
    fn id(&self) -> &SourceId {
        self.inner.id()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn kind(&self) -> SourceKind {
        SourceKind::Indexer
    }

    fn supports(&self, query: &TorrentQuery) -> Support {
        self.inner.supports(query)
    }

    async fn search(
        &self,
        query: &TorrentQuery,
        context: &SearchContext,
    ) -> Result<Vec<RawRelease>, SourceError> {
        self.inner.search(query, context).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feed_address_is_the_services_torznab_path_for_the_indexer() {
        assert_eq!(
            feed_url("http://127.0.0.1:9117/", "1337x"),
            "http://127.0.0.1:9117/api/v2.0/indexers/1337x/results/torznab/api"
        );
    }
}

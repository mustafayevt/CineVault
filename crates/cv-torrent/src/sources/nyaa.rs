//! Nyaa: anime releases by RSS text search.
//!
//! `GET {base}/?page=rss&q={text}&c=1_0&f=0` (all anime categories, no filter). Only asked when
//! the title is anime. Anime groups number episodes without a season (`Show - 05`), so the text
//! queries are the title with the episode number, then the title alone, instead of an `S01E05`
//! code. The feed's `link` is a `.torrent` address and `nyaa:infoHash` the hash.

use std::collections::HashSet;

use async_trait::async_trait;
use cv_providers::http::HttpClient;

use super::run_text_queries_from;
use crate::latest::{FeedKind, FeedSkip, FeedSupport, LatestAnswer};
use crate::model::{
    RawRelease, Scope, SkipReason, SourceError, SourceId, SourceKind, Support, TorrentQuery,
};
use crate::query::{MAX_TEXT_QUERIES, variants};
use crate::source::{SearchContext, SourceHttp, TorrentSource};
use crate::torznab::parse_feed;

/// The id of this source.
pub const ID: &str = "nyaa";

/// The text queries for an anime: the title, with the episode number for an episode.
#[must_use]
pub fn anime_queries(query: &TorrentQuery) -> Vec<String> {
    let spellings = variants(
        &query.title,
        query.original_title.as_deref(),
        &query.alternative_titles,
    );
    let mut out: Vec<String> = Vec::new();
    let mut add = |text: String| {
        if !out.iter().any(|seen| seen.eq_ignore_ascii_case(&text)) {
            out.push(text);
        }
    };
    match query.scope {
        Scope::Episode(_, episode) => {
            if let Some(first) = spellings.first() {
                add(format!("{first} {episode:02}"));
                add(first.clone());
            }
            for spelling in spellings.iter().skip(1) {
                add(format!("{spelling} {episode:02}"));
            }
        }
        _ => {
            for spelling in &spellings {
                add(spelling.clone());
            }
        }
    }
    let mut seen = HashSet::new();
    out.retain(|text| seen.insert(text.to_lowercase()));
    out.truncate(MAX_TEXT_QUERIES);
    out
}

/// The Nyaa source.
pub struct NyaaSource {
    id: SourceId,
    http: SourceHttp,
    base_url: String,
}

impl NyaaSource {
    /// A source that asks `base_url` (no trailing slash) through `client`.
    #[must_use]
    pub fn new(client: HttpClient, base_url: &str) -> Self {
        Self {
            id: SourceId(ID.to_owned()),
            http: SourceHttp::new(client, ID),
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }
}

#[async_trait]
impl TorrentSource for NyaaSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn name(&self) -> &str {
        "Nyaa"
    }

    fn kind(&self) -> SourceKind {
        SourceKind::BuiltIn
    }

    fn supports(&self, query: &TorrentQuery) -> Support {
        if query.anime {
            Support::Yes
        } else {
            Support::Skip(SkipReason::AnimeOnly)
        }
    }

    async fn search(
        &self,
        query: &TorrentQuery,
        context: &SearchContext,
    ) -> Result<Vec<RawRelease>, SourceError> {
        let url = format!("{}/", self.base_url);
        run_text_queries_from(anime_queries(query), context, |text| {
            let url = url.clone();
            async move {
                let body = self
                    .http
                    .get_text(
                        &url,
                        &[("page", "rss"), ("q", &text), ("c", "1_0"), ("f", "0")],
                    )
                    .await?;
                let feed = parse_feed(&body)?;
                Ok(feed
                    .items
                    .into_iter()
                    .map(|mut row| {
                        row.source = Some(SourceId(ID.to_owned()));
                        row
                    })
                    .collect())
            }
        })
        .await
    }

    fn latest_support(&self, kind: FeedKind) -> FeedSupport {
        if kind.wants_tv() {
            FeedSupport::Yes
        } else {
            FeedSupport::Skip(FeedSkip::TvOnly)
        }
    }

    /// The site's RSS of the newest English-translated anime (`c=1_2`): one page of 75. The RSS
    /// has no further pages.
    async fn latest(
        &self,
        _kind: FeedKind,
        page: u32,
        _context: &SearchContext,
    ) -> Result<LatestAnswer, SourceError> {
        if page > 0 {
            return Ok(LatestAnswer::Page {
                rows: Vec::new(),
                more: false,
            });
        }
        let url = format!("{}/", self.base_url);
        let body = self
            .http
            .get_text(&url, &[("page", "rss"), ("c", "1_2"), ("f", "0")])
            .await?;
        Ok(LatestAnswer::Page {
            rows: latest_rows(&body)?,
            more: false,
        })
    }
}

/// The rows of the newest-anime RSS. They are listed with TV: most are episodes, and the site's
/// own category ("Anime - English-translated") says nothing about movie or show.
fn latest_rows(body: &str) -> Result<Vec<RawRelease>, SourceError> {
    Ok(parse_feed(body)?
        .items
        .into_iter()
        .map(|mut row| {
            row.source = Some(SourceId(ID.to_owned()));
            row.category = Some("TV / Anime".to_owned());
            row
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MediaKind, TitleIds};

    fn query(scope: Scope, title: &str) -> TorrentQuery {
        TorrentQuery {
            kind: MediaKind::Show,
            ids: TitleIds::default(),
            title: title.to_owned(),
            original_title: Some("Sousou no Frieren".to_owned()),
            alternative_titles: vec!["Frieren: Beyond Journey's End".to_owned()],
            year: Some(2023),
            scope,
            runtime_minutes: None,
            anime: true,
            library_copy: None,
        }
    }

    #[test]
    fn an_episode_asks_with_the_number_then_without() {
        assert_eq!(
            anime_queries(&query(Scope::Episode(1, 5), "Frieren")),
            vec!["Frieren 05", "Frieren", "Sousou no Frieren 05"]
        );
    }

    #[test]
    fn a_season_or_show_asks_for_the_spellings() {
        assert_eq!(
            anime_queries(&query(Scope::Season(2), "Frieren")),
            vec![
                "Frieren",
                "Sousou no Frieren",
                "Frieren: Beyond Journey's End"
            ]
        );
        assert_eq!(anime_queries(&query(Scope::Show, "Frieren")).len(), 3);
    }
}

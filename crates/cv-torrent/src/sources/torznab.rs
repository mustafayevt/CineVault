//! A Torznab feed as a source: the user's own instance, and (through the indexer source) each
//! indexer of the managed service.
//!
//! The feed address is the instance's `.../api` URL and the key travels as the `apikey` query
//! parameter, kept apart from the request's identity so it never reaches a log or a cache key.
//! The instance's `t=caps` answer decides what is asked: where it offers `movie-search` or
//! `tv-search` by IMDb ID, that request runs next to the text queries of the title (with the year,
//! or `S01E02` / `S01` for shows), never instead of them. An all-indexers feed passes an ID search
//! only to the sites that understand IDs and silently drops the rest, and a site that does search
//! by ID often knows fewer releases by ID than by name. The two answers are merged and each
//! release is listed once (by info-hash, else by name and size).

use std::collections::HashSet;

use async_trait::async_trait;
use cv_providers::http::HttpClient;
use tokio::sync::OnceCell;

use super::run_text_queries;
use crate::latest::{FeedKind, FeedSupport, LatestAnswer};
use crate::magnet::normalise_info_hash;
use crate::model::{
    MediaKind, RawRelease, Scope, SourceError, SourceId, SourceKind, Support, TorrentQuery,
};
use crate::source::{
    AGGREGATE_REQUEST_TIMEOUT, SearchContext, SourceHttp, SourceResponse, TorrentSource,
    check_status, looks_like_html,
};
use crate::torznab::{Caps, FeedError, parse_caps, parse_feed};

/// Releases asked for per page of the newest releases.
pub const LATEST_PAGE_SIZE: u32 = 100;

/// The Torznab categories of a feed kind: movies `2000`, TV `5000`. Asking only these keeps
/// adult (`6000`) and everything else out.
#[must_use]
pub fn latest_categories(kind: FeedKind) -> &'static str {
    match kind {
        FeedKind::All => "2000,5000",
        FeedKind::Movies => "2000",
        FeedKind::Tv => "5000",
    }
}

/// Whether a Torznab error says the feed cannot list without search words: a missing or wrong
/// parameter (codes 200 to 299), or a site failure (900) whose text is about the query.
#[must_use]
pub fn rejects_empty_query(code: u32, description: &str) -> bool {
    if (200..300).contains(&code) {
        return true;
    }
    let lower = description.to_lowercase();
    [
        "query",
        "keyword",
        "search term",
        "search string",
        "not supported",
    ]
    .iter()
    .any(|word| lower.contains(word))
        && !(100..200).contains(&code)
}

/// The id of the user's own instance.
pub const USER_INSTANCE_ID: &str = "torznab";

/// The name of the user's own instance on the page.
pub const USER_INSTANCE_NAME: &str = "Your Torznab instance";

/// What a connection test found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapsSummary {
    pub search: bool,
    pub movie_search: bool,
    pub tv_search: bool,
    /// Whether movies or shows can be searched by IMDb ID.
    pub imdb_ids: bool,
    /// How many indexers the instance has configured, when it says.
    pub indexers: Option<usize>,
}

fn digits(imdb: &str) -> &str {
    imdb.trim_start_matches("tt")
}

/// The parameters of the search by IMDb ID for this query and these caps, when there is one.
#[must_use]
pub fn id_search_params(query: &TorrentQuery, caps: &Caps) -> Option<Vec<(&'static str, String)>> {
    let imdb = digits(query.ids.imdb.as_deref()?).to_owned();
    if imdb.is_empty() {
        return None;
    }
    match query.scope {
        Scope::Movie if caps.movie_by_imdb() => {
            Some(vec![("t", "movie".to_owned()), ("imdbid", imdb)])
        }
        Scope::Show if caps.tv_by_imdb() => {
            Some(vec![("t", "tvsearch".to_owned()), ("imdbid", imdb)])
        }
        Scope::Season(season) if caps.tv_by_imdb() && caps.tv_by_season_episode() => Some(vec![
            ("t", "tvsearch".to_owned()),
            ("imdbid", imdb),
            ("season", season.to_string()),
        ]),
        Scope::Episode(season, episode) if caps.tv_by_imdb() && caps.tv_by_season_episode() => {
            Some(vec![
                ("t", "tvsearch".to_owned()),
                ("imdbid", imdb),
                ("season", season.to_string()),
                ("ep", episode.to_string()),
            ])
        }
        _ => None,
    }
}

/// The `t=` value for a text search on an instance with these caps.
#[must_use]
pub fn text_search_function(kind: MediaKind, caps: &Caps) -> &'static str {
    if caps.search.is_some() {
        "search"
    } else if kind == MediaKind::Movie && caps.movie_search.is_some() {
        "movie"
    } else if kind == MediaKind::Show && caps.tv_search.is_some() {
        "tvsearch"
    } else {
        "search"
    }
}

fn feed_error(error: &FeedError) -> SourceError {
    match error.code {
        100..=199 => SourceError::KeyRejected,
        _ if error.description.is_empty() => {
            SourceError::BadAnswer(format!("the feed reported error {}", error.code))
        }
        _ => described_error(&error.description),
    }
}

/// The error for a failure the feed describes in words. The managed indexer service reports a
/// site that failed as error 900 with the whole exception text, which says why: a browser
/// challenge, a site that does not answer, or something else. The first line is kept for the
/// last case.
fn described_error(description: &str) -> SourceError {
    let lower = description.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|word| lower.contains(word));
    if any(&[
        "challenge detected",
        "cloudflare",
        "flaresolverr",
        "captcha",
        "ddos-guard",
    ]) {
        return SourceError::Blocked;
    }
    let first = description.lines().next().unwrap_or_default().trim();
    let first = first
        .strip_prefix("Jackett.Common.IndexerException: ")
        .unwrap_or(first);
    let short: String = first.chars().take(160).collect();
    if any(&[
        "timed out",
        "timeout",
        "unable to connect",
        "connection refused",
        "connection reset",
        "name or service not known",
        "requested name is valid",
        "no such host",
        "ssl connection",
        "task was canceled",
        "error while copying content",
    ]) {
        return SourceError::Unreachable(short);
    }
    SourceError::BadAnswer(short)
}

/// Maps the HTTP part of an answer; a key problem is its own error.
fn check_answer(response: &SourceResponse) -> Result<(), SourceError> {
    if matches!(response.status, 401 | 403) && !looks_like_html(&response.body) {
        // Jackett and Prowlarr answer a wrong key with 401 and a Torznab error element.
        if let Ok(feed) = parse_feed(&response.text())
            && let Some(error) = &feed.error
            && !(100..=199).contains(&error.code)
        {
            return Err(feed_error(error));
        }
        return Err(SourceError::KeyRejected);
    }
    check_status(response)?;
    if looks_like_html(&response.body) {
        return Err(SourceError::Blocked);
    }
    Ok(())
}

/// A Torznab source.
pub struct TorznabSource {
    id: SourceId,
    name: String,
    kind: SourceKind,
    http: SourceHttp,
    feed_url: String,
    api_key: Option<String>,
    caps: OnceCell<Caps>,
}

impl TorznabSource {
    /// A source for the feed at `feed_url`, sent through `http`.
    #[must_use]
    pub fn new(
        http: SourceHttp,
        id: &str,
        name: &str,
        kind: SourceKind,
        feed_url: &str,
        api_key: Option<&str>,
    ) -> Self {
        Self {
            id: SourceId(id.to_owned()),
            name: name.to_owned(),
            kind,
            http,
            feed_url: feed_url.trim().trim_end_matches('/').to_owned(),
            api_key: api_key.map(str::to_owned).filter(|key| !key.is_empty()),
            caps: OnceCell::new(),
        }
    }

    /// The user's own instance: id `torznab`, lane `torrent:torznab`. Its requests may take
    /// [`AGGREGATE_REQUEST_TIMEOUT`]: an all-indexers feed asks every site before it answers.
    #[must_use]
    pub fn user_instance(client: HttpClient, feed_url: &str, api_key: &str) -> Self {
        Self::new(
            SourceHttp::new(client, USER_INSTANCE_ID).with_timeout(AGGREGATE_REQUEST_TIMEOUT),
            USER_INSTANCE_ID,
            USER_INSTANCE_NAME,
            SourceKind::Torznab,
            feed_url,
            Some(api_key),
        )
    }

    async fn get(&self, query: &[(&str, &str)]) -> Result<SourceResponse, SourceError> {
        let credentials: Vec<(&str, &str)> = self
            .api_key
            .as_deref()
            .map(|key| vec![("apikey", key)])
            .unwrap_or_default();
        let response = self
            .http
            .get_credentialed(&self.feed_url, query, &credentials)
            .await?;
        check_answer(&response)?;
        Ok(response)
    }

    /// What the instance can search for; asked once and kept. An instance that answers `caps`
    /// with something unusable is treated as plain text search.
    ///
    /// # Errors
    /// A rejected key, a challenge page or an unreachable instance are returned; they would
    /// fail the search too.
    pub async fn caps(&self) -> Result<Caps, SourceError> {
        self.caps
            .get_or_try_init(|| async {
                let response = self.get(&[("t", "caps")]).await?;
                match parse_caps(&response.text()) {
                    Ok((_, Some(error))) => Err(feed_error(&error)),
                    Ok((caps, None)) if caps.can_search() => Ok(caps),
                    Ok(_) | Err(_) => Ok(Caps::basic()),
                }
            })
            .await
            .cloned()
    }

    async fn fetch(&self, params: &[(&str, String)]) -> Result<Vec<RawRelease>, SourceError> {
        let query: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let response = self.get(&query).await?;
        let feed = parse_feed(&response.text())?;
        if let Some(error) = &feed.error {
            return Err(feed_error(error));
        }
        Ok(feed
            .items
            .into_iter()
            .map(|mut row| {
                row.source = Some(self.id.clone());
                row
            })
            .collect())
    }
}

#[async_trait]
impl TorrentSource for TorznabSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> SourceKind {
        self.kind
    }

    fn supports(&self, _query: &TorrentQuery) -> Support {
        Support::Yes
    }

    async fn search(
        &self,
        query: &TorrentQuery,
        context: &SearchContext,
    ) -> Result<Vec<RawRelease>, SourceError> {
        let caps = self.caps().await?;
        let function = text_search_function(query.kind, &caps);
        let by_id = async {
            let params = id_search_params(query, &caps)?;
            let found = self.fetch(&params).await;
            if let Ok(rows) = &found {
                context.partial(rows.clone());
            }
            Some(found)
        };
        let by_title = run_text_queries(query, context, |text| async move {
            self.fetch(&[("t", function.to_owned()), ("q", text)]).await
        });
        let (by_id, by_title) = tokio::join!(by_id, by_title);
        combine(by_id, by_title)
    }

    fn latest_support(&self, _kind: FeedKind) -> FeedSupport {
        FeedSupport::Yes
    }

    /// `t=search` with no `q`, the movie and TV categories and `limit`/`offset` paging: most feeds
    /// answer with their newest releases. A feed that refuses a search without words takes no
    /// part ([`LatestAnswer::Unsupported`]); that is not shown as a failure.
    async fn latest(
        &self,
        kind: FeedKind,
        page: u32,
        _context: &SearchContext,
    ) -> Result<LatestAnswer, SourceError> {
        let limit = LATEST_PAGE_SIZE.to_string();
        let offset = (u64::from(page) * u64::from(LATEST_PAGE_SIZE)).to_string();
        let credentials: Vec<(&str, &str)> = self
            .api_key
            .as_deref()
            .map(|key| vec![("apikey", key)])
            .unwrap_or_default();
        let response = self
            .http
            .get_credentialed(
                &self.feed_url,
                &[
                    ("t", "search"),
                    ("cat", latest_categories(kind)),
                    ("limit", &limit),
                    ("offset", &offset),
                ],
                &credentials,
            )
            .await?;
        latest_answer(&response)
    }
}

/// Reads one page of a feed's newest releases.
///
/// # Errors
/// A rejected key, a challenge page, an unreachable feed or an unreadable answer.
pub fn latest_answer(response: &SourceResponse) -> Result<LatestAnswer, SourceError> {
    // A feed that wants search words often says so with HTTP 400 and a Torznab error element.
    if matches!(response.status, 400 | 422) && !looks_like_html(&response.body) {
        return match parse_feed(&response.text()) {
            Ok(feed) => match &feed.error {
                Some(error) if (100..200).contains(&error.code) => Err(SourceError::KeyRejected),
                Some(error) if !rejects_empty_query(error.code, &error.description) => {
                    Err(feed_error(error))
                }
                _ => Ok(LatestAnswer::Unsupported),
            },
            Err(_) => Ok(LatestAnswer::Unsupported),
        };
    }
    check_answer(response)?;
    let feed = parse_feed(&response.text())?;
    if let Some(error) = &feed.error {
        if rejects_empty_query(error.code, &error.description) {
            return Ok(LatestAnswer::Unsupported);
        }
        return Err(feed_error(error));
    }
    let more = !feed.items.is_empty();
    Ok(LatestAnswer::Page {
        rows: feed.items,
        more,
    })
}

/// The answers of the ID search (`None` when there was none) and the title search as one list.
///
/// A rejected key fails the search whichever request saw it. Otherwise the rows of both are
/// kept, each release once; a failure only counts when nothing was found at all (then the
/// title search's error, which says the most, else the ID search's).
fn combine(
    by_id: Option<Result<Vec<RawRelease>, SourceError>>,
    by_title: Result<Vec<RawRelease>, SourceError>,
) -> Result<Vec<RawRelease>, SourceError> {
    if matches!(by_id, Some(Err(SourceError::KeyRejected)))
        || matches!(by_title, Err(SourceError::KeyRejected))
    {
        return Err(SourceError::KeyRejected);
    }
    match (by_id, by_title) {
        (None, by_title) => by_title.map(distinct),
        (Some(Ok(id_rows)), Ok(title_rows)) => {
            Ok(distinct(id_rows.into_iter().chain(title_rows).collect()))
        }
        (Some(Ok(rows)), Err(error)) | (Some(Err(error)), Ok(rows)) => {
            if rows.is_empty() {
                Err(error)
            } else {
                Ok(distinct(rows))
            }
        }
        (Some(Err(_)), Err(error)) => Err(error),
    }
}

/// Each release once, in the order first seen: by info-hash, else by name and size.
#[must_use]
pub fn distinct(rows: Vec<RawRelease>) -> Vec<RawRelease> {
    let mut seen: HashSet<String> = HashSet::new();
    rows.into_iter()
        .filter(|row| {
            let key = row
                .info_hash
                .as_deref()
                .and_then(normalise_info_hash)
                .unwrap_or_else(|| {
                    format!(
                        "{}|{}",
                        row.name.trim().to_lowercase(),
                        row.size.unwrap_or_default()
                    )
                });
            seen.insert(key)
        })
        .collect()
}

/// How many `<indexer>` elements an `t=indexers` answer lists.
fn indexer_count(text: &str) -> Option<usize> {
    let count = text.matches("<indexer ").count() + text.matches("<indexer>").count();
    (count > 0 || text.contains("<indexers")).then_some(count)
}

/// Where a Jackett the person installed listens by default.
pub const JACKETT_DEFAULT_ADDRESS: &str = "http://127.0.0.1:9117";

/// How long the look for a Jackett may take: it is on this computer or the local network.
pub const DETECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// The all-indexers Torznab feed of a Jackett at `base_url` (`http://127.0.0.1:9117`).
#[must_use]
pub fn jackett_all_feed(base_url: &str) -> String {
    super::indexer::feed_url(base_url, "all")
}

/// Whether a Jackett answers at `base_url`. Its all-indexers feed, asked for its `caps` without a
/// key, answers with the Torznab error "Invalid API Key" (code 100); nothing else on that path
/// does. No key is sent and nothing is changed.
pub async fn detect_jackett(client: HttpClient, base_url: &str) -> bool {
    let http = SourceHttp::new(client, "jackett-detect").with_timeout(DETECT_TIMEOUT);
    let Ok(response) = http
        .get_raw(&jackett_all_feed(base_url), &[("t", "caps")])
        .await
    else {
        return false;
    };
    if looks_like_html(&response.body) {
        return false;
    }
    matches!(
        parse_caps(&response.text()),
        Ok((
            _,
            Some(FeedError {
                code: 100..=199,
                ..
            })
        ))
    )
}

/// Tests an instance: asks for its `caps` with the key and says what it can do.
///
/// # Errors
/// [`SourceError::KeyRejected`] for a wrong key (HTTP 401 or 403, or a Torznab error 1xx),
/// [`SourceError::Unreachable`] or [`SourceError::Timeout`] when nothing answers,
/// [`SourceError::BadAnswer`] when the address answers with something that is not Torznab.
pub async fn test_connection(
    client: HttpClient,
    feed_url: &str,
    api_key: &str,
) -> Result<CapsSummary, SourceError> {
    let source = TorznabSource::new(
        SourceHttp::new(client, USER_INSTANCE_ID),
        USER_INSTANCE_ID,
        USER_INSTANCE_NAME,
        SourceKind::Torznab,
        feed_url,
        Some(api_key),
    );
    let response = source.get(&[("t", "caps")]).await?;
    let (caps, error) = parse_caps(&response.text())
        .map_err(|_| SourceError::BadAnswer("This is not a Torznab feed".to_owned()))?;
    if let Some(error) = error {
        return Err(feed_error(&error));
    }
    // The count is a courtesy: an instance without `t=indexers` just does not report one.
    let indexers = match source
        .get(&[("t", "indexers"), ("configured", "true")])
        .await
    {
        Ok(response) => indexer_count(&response.text()),
        Err(_) => None,
    };
    Ok(CapsSummary {
        search: caps.search.is_some(),
        movie_search: caps.movie_search.is_some(),
        tv_search: caps.tv_search.is_some(),
        imdb_ids: caps.movie_by_imdb() || caps.tv_by_imdb(),
        indexers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TitleIds;

    fn query(kind: MediaKind, scope: Scope, imdb: Option<&str>) -> TorrentQuery {
        TorrentQuery {
            kind,
            ids: TitleIds {
                tmdb: None,
                imdb: imdb.map(str::to_owned),
                tvdb: None,
            },
            title: "T".into(),
            original_title: None,
            alternative_titles: Vec::new(),
            year: None,
            scope,
            runtime_minutes: None,
            anime: false,
            library_copy: None,
        }
    }

    fn full_caps() -> Caps {
        Caps {
            search: Some(vec!["q".into()]),
            movie_search: Some(vec!["q".into(), "imdbid".into()]),
            tv_search: Some(vec![
                "q".into(),
                "season".into(),
                "ep".into(),
                "imdbid".into(),
            ]),
        }
    }

    fn pairs(params: Option<Vec<(&'static str, String)>>) -> Vec<(String, String)> {
        params
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect()
    }

    #[test]
    fn the_id_search_is_chosen_by_scope_and_caps() {
        let caps = full_caps();
        let movie = query(MediaKind::Movie, Scope::Movie, Some("tt0063350"));
        assert_eq!(
            pairs(id_search_params(&movie, &caps)),
            vec![
                ("t".into(), "movie".into()),
                ("imdbid".into(), "0063350".into())
            ]
        );
        let episode = query(MediaKind::Show, Scope::Episode(1, 2), Some("tt0098904"));
        assert_eq!(
            pairs(id_search_params(&episode, &caps)),
            vec![
                ("t".into(), "tvsearch".into()),
                ("imdbid".into(), "0098904".into()),
                ("season".into(), "1".into()),
                ("ep".into(), "2".into())
            ]
        );
        let season = query(MediaKind::Show, Scope::Season(3), Some("tt1"));
        assert_eq!(id_search_params(&season, &caps).unwrap().len(), 3);
        let show = query(MediaKind::Show, Scope::Show, Some("tt1"));
        assert_eq!(id_search_params(&show, &caps).unwrap().len(), 2);
    }

    #[test]
    fn no_id_search_without_an_id_or_the_capability() {
        let movie = query(MediaKind::Movie, Scope::Movie, None);
        assert_eq!(id_search_params(&movie, &full_caps()), None);
        let with_id = query(MediaKind::Movie, Scope::Movie, Some("tt1"));
        assert_eq!(id_search_params(&with_id, &Caps::basic()), None);
        let no_ep = Caps {
            tv_search: Some(vec!["q".into(), "imdbid".into()]),
            ..Caps::default()
        };
        let episode = query(MediaKind::Show, Scope::Episode(1, 1), Some("tt1"));
        assert_eq!(id_search_params(&episode, &no_ep), None);
        assert!(
            id_search_params(&query(MediaKind::Show, Scope::Show, Some("tt1")), &no_ep).is_some()
        );
    }

    #[test]
    fn the_text_function_follows_the_caps() {
        assert_eq!(
            text_search_function(MediaKind::Movie, &full_caps()),
            "search"
        );
        let only_movie = Caps {
            movie_search: Some(vec!["q".into()]),
            ..Caps::default()
        };
        assert_eq!(text_search_function(MediaKind::Movie, &only_movie), "movie");
        assert_eq!(text_search_function(MediaKind::Show, &only_movie), "search");
        let only_tv = Caps {
            tv_search: Some(vec!["q".into()]),
            ..Caps::default()
        };
        assert_eq!(text_search_function(MediaKind::Show, &only_tv), "tvsearch");
    }

    #[test]
    fn error_codes_map_to_a_key_problem_or_a_bad_answer() {
        let e = |code, text: &str| {
            feed_error(&FeedError {
                code,
                description: text.into(),
            })
        };
        assert_eq!(
            e(100, "Incorrect user credentials"),
            SourceError::KeyRejected
        );
        assert_eq!(e(101, ""), SourceError::KeyRejected);
        assert_eq!(
            e(201, "Incorrect parameter"),
            SourceError::BadAnswer("Incorrect parameter".into())
        );
        assert!(matches!(e(900, ""), SourceError::BadAnswer(m) if m.contains("900")));
    }

    #[test]
    fn an_indexer_services_exception_text_says_what_kind_of_failure_it_was() {
        let e = |text: &str| {
            feed_error(&FeedError {
                code: 900,
                description: text.into(),
            })
        };
        // As the real service words them (recorded 2026-10-26).
        assert_eq!(
            e(
                "Jackett.Common.IndexerException: Exception (tokyotosho): Challenge detected but \
               FlareSolverr is not configured\r\n ---> FlareSolverrSharp.Exceptions.FlareSolverrException\r\n   at x"
            ),
            SourceError::Blocked
        );
        assert_eq!(
            e("Jackett.Common.IndexerException: Exception (anisource): The requested name is valid, \
               but no data of the requested type was found.\r\n ---> System.Net.Http.HttpRequestException"),
            SourceError::Unreachable(
                "Exception (anisource): The requested name is valid, but no data of the requested type was found.".into()
            )
        );
        assert_eq!(
            e(
                "Jackett.Common.IndexerException: Exception (therarbg): Error Parsing Json Response: Status=Found\r\n at x"
            ),
            SourceError::BadAnswer(
                "Exception (therarbg): Error Parsing Json Response: Status=Found".into()
            )
        );
    }

    fn row(name: &str, hash: Option<&str>, size: Option<u64>) -> RawRelease {
        RawRelease {
            name: name.into(),
            info_hash: hash.map(str::to_owned),
            size,
            ..RawRelease::default()
        }
    }

    const HASH_A: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn the_id_and_the_title_search_are_merged_each_release_once() {
        let by_id = vec![row("Dune.Part.Two.2024.2160p", Some(HASH_A), Some(10))];
        let by_title = vec![
            // The same release by name, its hash in upper case.
            row(
                "Dune Part Two 2024 2160p",
                Some(&HASH_A.to_uppercase()),
                Some(10),
            ),
            row("Dune.Part.Two.2024.1080p", None, Some(5)),
            row("Dune.Part.Two.2024.1080p", None, Some(5)),
            row("Dune.Part.Two.2024.1080p", None, Some(6)),
        ];
        let merged = combine(Some(Ok(by_id)), Ok(by_title)).expect("rows");
        let names: Vec<_> = merged.iter().map(|r| (r.name.as_str(), r.size)).collect();
        assert_eq!(
            names,
            vec![
                ("Dune.Part.Two.2024.2160p", Some(10)),
                ("Dune.Part.Two.2024.1080p", Some(5)),
                ("Dune.Part.Two.2024.1080p", Some(6)),
            ]
        );
    }

    #[test]
    fn the_title_search_keeps_what_an_id_search_misses_or_fails_on() {
        let title = || Ok(vec![row("Blade Runner 1982", None, Some(1))]);
        assert_eq!(combine(Some(Ok(Vec::new())), title()).unwrap().len(), 1);
        assert_eq!(
            combine(Some(Err(SourceError::BadAnswer("x".into()))), title())
                .unwrap()
                .len(),
            1
        );
        assert_eq!(combine(None, title()).unwrap().len(), 1);
        // The ID rows survive a title search that failed.
        let id = Some(Ok(vec![row("Blade Runner 1982", Some(HASH_A), None)]));
        assert_eq!(combine(id, Err(SourceError::Timeout)).unwrap().len(), 1);
    }

    #[test]
    fn a_failure_counts_only_when_nothing_was_found_and_a_bad_key_always() {
        assert_eq!(
            combine(Some(Ok(Vec::new())), Err(SourceError::Timeout)),
            Err(SourceError::Timeout)
        );
        assert_eq!(
            combine(
                Some(Err(SourceError::Blocked)),
                Err(SourceError::Unreachable("down".into()))
            ),
            Err(SourceError::Unreachable("down".into()))
        );
        assert_eq!(
            combine(Some(Err(SourceError::Blocked)), Ok(Vec::new())),
            Err(SourceError::Blocked)
        );
        assert_eq!(
            combine(
                Some(Err(SourceError::KeyRejected)),
                Ok(vec![row("x", None, None)])
            ),
            Err(SourceError::KeyRejected)
        );
        assert_eq!(combine(None, Ok(Vec::new())), Ok(Vec::new()));
    }

    #[test]
    fn indexers_are_counted() {
        assert_eq!(
            indexer_count(r#"<indexers><indexer id="a"/><indexer id="b"></indexer></indexers>"#),
            Some(2)
        );
        assert_eq!(indexer_count("<indexers></indexers>"), Some(0));
        assert_eq!(indexer_count("<html>"), None);
    }
}

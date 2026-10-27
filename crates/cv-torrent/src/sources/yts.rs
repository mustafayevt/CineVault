//! YTS: movie releases by IMDb ID.
//!
//! `GET {base}/api/v2/list_movies.json?query_term={imdb}`. Each movie in `data.movies` lists its
//! `torrents` (one per quality); every torrent becomes one release named like the site names its
//! files: `{title_long} {quality} {type} {video_codec} YTS`. Movies only.

use async_trait::async_trait;
use chrono::DateTime;
use cv_providers::http::HttpClient;
use serde::Deserialize;

use super::{flexible_u32, flexible_u64};
use crate::magnet::normalise_info_hash;
use crate::model::{
    MediaKind, RawRelease, SkipReason, SourceError, SourceId, SourceKind, Support, TorrentQuery,
};
use crate::source::{SearchContext, SourceHttp, TorrentSource};

/// The id of this source.
pub const ID: &str = "yts";

#[derive(Debug, Deserialize)]
struct Answer {
    status: Option<String>,
    status_message: Option<String>,
    data: Option<Data>,
}

#[derive(Debug, Deserialize)]
struct Data {
    #[serde(default)]
    movies: Vec<Movie>,
}

#[derive(Debug, Deserialize)]
struct Movie {
    url: Option<String>,
    imdb_code: Option<String>,
    title: Option<String>,
    title_long: Option<String>,
    #[serde(default)]
    torrents: Vec<Torrent>,
}

#[derive(Debug, Deserialize)]
struct Torrent {
    url: Option<String>,
    hash: Option<String>,
    quality: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    video_codec: Option<String>,
    bit_depth: Option<serde_json::Value>,
    seeds: Option<serde_json::Value>,
    peers: Option<serde_json::Value>,
    size_bytes: Option<serde_json::Value>,
    date_uploaded_unix: Option<i64>,
}

fn name_of(movie: &Movie, torrent: &Torrent) -> Option<String> {
    let title = movie
        .title_long
        .as_deref()
        .or(movie.title.as_deref())?
        .trim();
    let mut name = title.to_owned();
    for part in [&torrent.quality, &torrent.kind, &torrent.video_codec] {
        if let Some(part) = part.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            name.push(' ');
            name.push_str(part);
        }
    }
    if flexible_u32(torrent.bit_depth.as_ref()) == Some(10) {
        name.push_str(" 10bit");
    }
    name.push_str(" YTS");
    Some(name)
}

fn parse_answer(answer: Answer, imdb: &str) -> Result<Vec<RawRelease>, SourceError> {
    if answer.status.as_deref().is_some_and(|s| s != "ok") {
        return Err(SourceError::BadAnswer(
            answer
                .status_message
                .unwrap_or_else(|| "the site reported an error".to_owned()),
        ));
    }
    let movies = answer.data.map(|d| d.movies).unwrap_or_default();
    let mut out = Vec::new();
    for movie in movies {
        // The query term is an IMDb ID, so every movie should be that one; a stray one is dropped.
        if movie
            .imdb_code
            .as_deref()
            .is_some_and(|code| !code.eq_ignore_ascii_case(imdb))
        {
            continue;
        }
        for torrent in &movie.torrents {
            let Some(info_hash) = torrent.hash.as_deref().and_then(normalise_info_hash) else {
                continue;
            };
            let Some(name) = name_of(&movie, torrent) else {
                continue;
            };
            out.push(RawRelease {
                details_url: movie.url.as_deref().and_then(crate::model::web_page),
                source: Some(SourceId(ID.to_owned())),
                name,
                info_hash: Some(info_hash),
                torrent_url: torrent
                    .url
                    .clone()
                    .filter(|u| u.starts_with("http://") || u.starts_with("https://")),
                size: flexible_u64(torrent.size_bytes.as_ref()),
                seeders: flexible_u32(torrent.seeds.as_ref()),
                leechers: flexible_u32(torrent.peers.as_ref()),
                published: torrent
                    .date_uploaded_unix
                    .and_then(|secs| DateTime::from_timestamp(secs, 0)),
                imdb: movie.imdb_code.clone(),
                ..RawRelease::default()
            });
        }
    }
    Ok(out)
}

/// The YTS source.
pub struct YtsSource {
    id: SourceId,
    http: SourceHttp,
    base_url: String,
}

impl YtsSource {
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
impl TorrentSource for YtsSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn name(&self) -> &str {
        "YTS"
    }

    fn kind(&self) -> SourceKind {
        SourceKind::BuiltIn
    }

    fn supports(&self, query: &TorrentQuery) -> Support {
        if query.kind != MediaKind::Movie {
            Support::Skip(SkipReason::MoviesOnly)
        } else if query.ids.imdb.is_none() {
            Support::Skip(SkipReason::NoImdbId)
        } else {
            Support::Yes
        }
    }

    async fn search(
        &self,
        query: &TorrentQuery,
        _context: &SearchContext,
    ) -> Result<Vec<RawRelease>, SourceError> {
        let Some(imdb) = query.ids.imdb.as_deref() else {
            return Err(SourceError::BadAnswer("no IMDb ID".to_owned()));
        };
        let url = format!("{}/api/v2/list_movies.json", self.base_url);
        let answer: Answer = self.http.get_json(&url, &[("query_term", imdb)]).await?;
        parse_answer(answer, imdb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(json: &str) -> Answer {
        serde_json::from_str(json).expect("test JSON")
    }

    #[test]
    fn each_torrent_of_a_movie_is_a_release_named_like_the_site_names_it() {
        let rows = parse_answer(
            answer(
                r#"{"status":"ok","data":{"movies":[{"imdb_code":"tt1","title_long":"Heat (1995)","torrents":[
                {"url":"https://yts.mx/torrent/download/AAAA","hash":"0123456789ABCDEF0123456789ABCDEF01234567","quality":"1080p","type":"bluray","video_codec":"x265","bit_depth":"10","seeds":40,"peers":"5","size_bytes":2000000000,"date_uploaded_unix":1577872800},
                {"hash":"1123456789abcdef0123456789abcdef01234567","quality":"720p","type":"web","video_codec":"x264","bit_depth":"8","seeds":0,"peers":0,"size_bytes":900000000}]}]}}"#,
            ),
            "tt1",
        )
        .expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "Heat (1995) 1080p bluray x265 10bit YTS");
        assert_eq!(
            rows[0].info_hash.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(rows[0].seeders, Some(40));
        assert_eq!(rows[0].leechers, Some(5));
        assert_eq!(rows[0].size, Some(2_000_000_000));
        assert_eq!(
            rows[0].torrent_url.as_deref(),
            Some("https://yts.mx/torrent/download/AAAA")
        );
        assert_eq!(
            rows[0].published.map(|d| d.timestamp()),
            Some(1_577_872_800)
        );
        assert_eq!(rows[1].name, "Heat (1995) 720p web x264 YTS");
        assert_eq!(rows[1].torrent_url, None);
    }

    #[test]
    fn a_site_error_and_odd_answers_are_handled() {
        let error = parse_answer(
            answer(r#"{"status":"error","status_message":"Too many"}"#),
            "tt1",
        )
        .unwrap_err();
        assert_eq!(error, SourceError::BadAnswer("Too many".into()));
        assert!(
            parse_answer(answer(r#"{"status":"ok","data":{"movie_count":0}}"#), "tt1")
                .unwrap()
                .is_empty()
        );
        assert!(parse_answer(answer(r"{}"), "tt1").unwrap().is_empty());
        let stray = parse_answer(
            answer(r#"{"data":{"movies":[{"imdb_code":"tt9","title_long":"Other (2000)","torrents":[{"hash":"0123456789abcdef0123456789abcdef01234567"}]}]}}"#),
            "tt1",
        )
        .unwrap();
        assert!(stray.is_empty(), "a movie with another IMDb ID is dropped");
    }

    #[test]
    fn torrents_without_a_usable_hash_are_dropped() {
        let rows = parse_answer(
            answer(r#"{"data":{"movies":[{"title":"X","torrents":[{"hash":"bad"},{"quality":"720p"}]}]}}"#),
            "tt1",
        )
        .unwrap();
        assert!(rows.is_empty());
    }
}

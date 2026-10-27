//! Torznab and RSS readers.
//!
//! One reader serves three kinds of answer: Torznab feeds (the user's own instance and the
//! managed indexer service), Nyaa's RSS and the Torznab `caps` document. Elements are matched by
//! their local name, so `torznab:attr`, `newznab:attr` and `nyaa:seeders` need no namespace
//! handling. A feed with one broken item gives the other items; a feed that is cut off gives
//! the items read before the cut; text that is not a feed at all is an error.

use chrono::{DateTime, Utc};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use crate::magnet::{normalise_info_hash, validate_magnet};
use crate::model::{RawRelease, SourceError, web_page};

/// A Torznab error element: `<error code="100" description="Incorrect user credentials"/>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedError {
    pub code: u32,
    pub description: String,
}

/// What a feed holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Feed {
    pub items: Vec<RawRelease>,
    /// How many items were skipped because they could not be read.
    pub skipped: usize,
    /// The feed's own error, when the answer was an `<error>` element.
    pub error: Option<FeedError>,
}

fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

fn attrs_of(start: &BytesStart<'_>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for attribute in start.attributes().with_checks(false).flatten() {
        let key = attribute.key.as_ref().to_owned();
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_or_else(
                |_| attribute.value.clone().into_owned(),
                std::borrow::Cow::into_owned,
            );
        out.push((local(&key).to_owned(), value));
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn name_of(start: &BytesStart<'_>) -> String {
    local(start.name().as_ref()).to_owned()
}

/// A size as text: bytes (`1234`), or a number with a unit (`26.8 GiB`, `700 MB`). Units with an
/// `i` are binary, the others decimal.
#[must_use]
pub fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim();
    if let Ok(bytes) = text.parse::<u64>() {
        return Some(bytes);
    }
    let split = text.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))?;
    let (number, unit) = text.split_at(split);
    let number: f64 = number.replace(',', "").parse().ok()?;
    let unit = unit.trim().to_ascii_lowercase();
    let (base, power) = match unit.as_str() {
        "b" | "bytes" => (1000f64, 0),
        "kib" => (1024f64, 1),
        "mib" => (1024f64, 2),
        "gib" => (1024f64, 3),
        "tib" => (1024f64, 4),
        "kb" => (1000f64, 1),
        "mb" => (1000f64, 2),
        "gb" => (1000f64, 3),
        "tb" => (1000f64, 4),
        _ => return None,
    };
    let bytes = number * base.powi(power);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (bytes.is_finite() && bytes >= 0.0).then_some(bytes as u64)
}

fn parse_date(text: &str) -> Option<DateTime<Utc>> {
    let text = text.trim();
    DateTime::parse_from_rfc2822(text)
        .or_else(|_| DateTime::parse_from_rfc3339(text))
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn imdb_of(value: &str) -> Option<String> {
    let digits = value.trim().trim_start_matches("tt");
    (!digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && digits.bytes().any(|b| b != b'0'))
    .then(|| format!("tt{digits}"))
}

/// The pieces of one `<item>` while it is read.
#[derive(Default)]
struct ItemBuilder {
    title: Option<String>,
    link: Option<String>,
    guid: Option<String>,
    pub_date: Option<String>,
    enclosure_url: Option<String>,
    enclosure_length: Option<String>,
    size: Option<String>,
    category: Option<String>,
    comments: Option<String>,
    info_hash: Option<String>,
    magnet: Option<String>,
    seeders: Option<String>,
    leechers: Option<String>,
    peers: Option<String>,
    imdb: Option<String>,
    season: Option<String>,
    episode: Option<String>,
    attr_category: Option<String>,
}

impl ItemBuilder {
    /// An element with text.
    fn text(&mut self, name: &str, text: String) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        let slot = match name {
            "title" => &mut self.title,
            "link" => &mut self.link,
            "guid" => &mut self.guid,
            "pubDate" => &mut self.pub_date,
            "size" => &mut self.size,
            "category" => &mut self.category,
            "comments" => &mut self.comments,
            "infoHash" => &mut self.info_hash,
            "seeders" => &mut self.seeders,
            "leechers" => &mut self.leechers,
            _ => return,
        };
        slot.get_or_insert(text);
    }

    /// An element with attributes only: `enclosure`, `attr`.
    fn attributes(&mut self, name: &str, attrs: &[(String, String)]) {
        match name {
            "enclosure" => {
                if let Some(url) = attr(attrs, "url") {
                    self.enclosure_url.get_or_insert_with(|| url.to_owned());
                }
                if let Some(length) = attr(attrs, "length") {
                    self.enclosure_length
                        .get_or_insert_with(|| length.to_owned());
                }
            }
            "attr" => {
                let (Some(key), Some(value)) = (attr(attrs, "name"), attr(attrs, "value")) else {
                    return;
                };
                let value = value.trim().to_owned();
                if value.is_empty() {
                    return;
                }
                let slot = match key {
                    "seeders" => &mut self.seeders,
                    "peers" => &mut self.peers,
                    "leechers" => &mut self.leechers,
                    "infohash" => &mut self.info_hash,
                    "magneturl" => &mut self.magnet,
                    "imdbid" | "imdb" => &mut self.imdb,
                    "season" => &mut self.season,
                    "episode" => &mut self.episode,
                    "size" => &mut self.size,
                    "category" => &mut self.attr_category,
                    _ => return,
                };
                slot.get_or_insert(value);
            }
            _ => {}
        }
    }

    fn build(self) -> Option<RawRelease> {
        let name = self.title?.trim().to_owned();
        if name.is_empty() {
            return None;
        }
        // The links an item can offer: a magnet link (attribute, or the item link itself), a
        // `.torrent` address (enclosure, or the item link).
        let mut magnet = self.magnet.filter(|m| validate_magnet(m).is_ok());
        let mut torrent_url = None;
        for candidate in [self.link.as_deref(), self.enclosure_url.as_deref()]
            .into_iter()
            .flatten()
        {
            if candidate.to_ascii_lowercase().starts_with("magnet:") {
                if magnet.is_none() && validate_magnet(candidate).is_ok() {
                    magnet = Some(candidate.to_owned());
                }
            } else if (candidate.starts_with("http://") || candidate.starts_with("https://"))
                && torrent_url.is_none()
            {
                torrent_url = Some(candidate.to_owned());
            }
        }
        let info_hash = self
            .info_hash
            .as_deref()
            .and_then(normalise_info_hash)
            .or_else(|| {
                magnet
                    .as_deref()
                    .and_then(|m| validate_magnet(m).ok())
                    .map(|m| m.info_hash)
            });
        if info_hash.is_none() && magnet.is_none() && torrent_url.is_none() {
            return None;
        }
        let seeders: Option<u32> = self.seeders.as_deref().and_then(|v| v.parse().ok());
        let leechers = self
            .leechers
            .as_deref()
            .and_then(|v| v.parse::<u32>().ok())
            .or_else(|| {
                // Torznab's `peers` counts everyone: seeders and leechers.
                let peers: u32 = self.peers.as_deref()?.parse().ok()?;
                Some(peers.saturating_sub(seeders.unwrap_or(0)))
            });
        Some(RawRelease {
            source: None,
            name,
            info_hash,
            magnet,
            torrent_url,
            size: self
                .size
                .as_deref()
                .and_then(parse_size)
                .or_else(|| self.enclosure_length.as_deref().and_then(parse_size))
                .filter(|size| *size > 0),
            seeders,
            leechers,
            published: self.pub_date.as_deref().and_then(parse_date),
            imdb: self.imdb.as_deref().and_then(imdb_of),
            season: self.season.as_deref().and_then(|v| v.parse().ok()),
            episode: self.episode.as_deref().and_then(|v| v.parse().ok()),
            files: Vec::new(),
            category: self.attr_category.or(self.category),
            // The comments address is the release page on Jackett and most feeds; the guid is
            // one on feeds that have no comments (Nyaa). A guid that is a magnet is no page.
            details_url: self
                .comments
                .as_deref()
                .and_then(web_page)
                .or_else(|| self.guid.as_deref().and_then(web_page)),
        })
    }
}

/// Reads a Torznab or RSS feed.
///
/// # Errors
/// [`SourceError::BadAnswer`] when the text is not an RSS or Torznab document at all (no `rss`,
/// `feed` or `error` element before any content).
pub fn parse_feed(text: &str) -> Result<Feed, SourceError> {
    let mut reader = Reader::from_str(text.trim_start_matches('\u{feff}'));
    let mut feed = Feed::default();
    let mut saw_feed = false;
    let mut item: Option<ItemBuilder> = None;
    // The leaf element being read inside an item and its text so far.
    let mut leaf: Option<(String, String)> = None;
    let mut item_depth = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                let name = name_of(&start);
                if item.is_some() {
                    item_depth += 1;
                    if item_depth == 1 {
                        leaf = Some((name, String::new()));
                    }
                } else if name == "item" || name == "entry" {
                    item = Some(ItemBuilder::default());
                    item_depth = 0;
                } else {
                    if matches!(name.as_str(), "rss" | "feed" | "channel") {
                        saw_feed = true;
                    }
                    if name == "error" {
                        saw_feed = true;
                        feed.error = Some(error_of(&attrs_of(&start)));
                    }
                }
            }
            Ok(Event::Empty(start)) => {
                let name = name_of(&start);
                if let Some(builder) = item.as_mut() {
                    builder.attributes(&name, &attrs_of(&start));
                } else if name == "error" {
                    saw_feed = true;
                    feed.error = Some(error_of(&attrs_of(&start)));
                }
            }
            Ok(Event::Text(text)) => {
                if let Some((_, buffer)) = leaf.as_mut() {
                    buffer.push_str(&text.xml10_content());
                }
            }
            Ok(Event::CData(data)) => {
                if let Some((_, buffer)) = leaf.as_mut() {
                    buffer.push_str(&data.xml10_content());
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some((_, buffer)) = leaf.as_mut() {
                    if let Ok(Some(c)) = reference.resolve_char_ref() {
                        buffer.push(c);
                    } else if let Some(resolved) = quick_xml::escape::resolve_xml_entity(&reference)
                    {
                        buffer.push_str(resolved);
                    }
                }
            }
            Ok(Event::End(_)) => {
                if item.is_some() {
                    if item_depth == 0 {
                        if let Some(builder) = item.take() {
                            match builder.build() {
                                Some(release) => feed.items.push(release),
                                None => feed.skipped += 1,
                            }
                        }
                        leaf = None;
                    } else {
                        if item_depth == 1
                            && let Some((leaf_name, text)) = leaf.take()
                            && let Some(builder) = item.as_mut()
                        {
                            builder.text(&leaf_name, text);
                        }
                        item_depth -= 1;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                // A feed cut off or broken part-way keeps what was read; one that never started
                // is not a feed.
                if saw_feed || !feed.items.is_empty() {
                    break;
                }
                return Err(SourceError::BadAnswer(format!(
                    "the answer is not a feed ({error})"
                )));
            }
        }
    }
    if !saw_feed && feed.items.is_empty() && feed.skipped == 0 {
        return Err(SourceError::BadAnswer(
            "the answer is not a feed".to_owned(),
        ));
    }
    Ok(feed)
}

fn error_of(attrs: &[(String, String)]) -> FeedError {
    FeedError {
        code: attr(attrs, "code")
            .and_then(|c| c.parse().ok())
            .unwrap_or(0),
        description: attr(attrs, "description").unwrap_or_default().to_owned(),
    }
}

/// What a Torznab instance can search for, from its `caps` document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Caps {
    /// The parameters `t=search` accepts, when it is available.
    pub search: Option<Vec<String>>,
    pub movie_search: Option<Vec<String>>,
    pub tv_search: Option<Vec<String>>,
}

impl Caps {
    /// Plain text search only, for an instance whose caps are not known.
    #[must_use]
    pub fn basic() -> Self {
        Self {
            search: Some(vec!["q".to_owned()]),
            ..Self::default()
        }
    }

    fn supports(params: Option<&Vec<String>>, name: &str) -> bool {
        params.is_some_and(|p| p.iter().any(|p| p == name))
    }

    #[must_use]
    pub fn movie_by_imdb(&self) -> bool {
        Self::supports(self.movie_search.as_ref(), "imdbid")
    }

    #[must_use]
    pub fn tv_by_imdb(&self) -> bool {
        Self::supports(self.tv_search.as_ref(), "imdbid")
    }

    #[must_use]
    pub fn tv_by_season_episode(&self) -> bool {
        Self::supports(self.tv_search.as_ref(), "season")
            && Self::supports(self.tv_search.as_ref(), "ep")
    }

    /// Whether the instance searches at all.
    #[must_use]
    pub fn can_search(&self) -> bool {
        self.search.is_some() || self.movie_search.is_some() || self.tv_search.is_some()
    }
}

/// Reads a `caps` document.
///
/// # Errors
/// [`SourceError::BadAnswer`] when the text is not a caps document; a Torznab `<error>` answer is
/// returned in the second field of the result instead.
pub fn parse_caps(text: &str) -> Result<(Caps, Option<FeedError>), SourceError> {
    let mut reader = Reader::from_str(text.trim_start_matches('\u{feff}'));
    let mut caps = Caps::default();
    let mut saw_caps = false;
    let mut error = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start) | Event::Empty(start)) => {
                let name = name_of(&start);
                let attrs = attrs_of(&start);
                match name.as_str() {
                    "caps" => saw_caps = true,
                    "error" => error = Some(error_of(&attrs)),
                    "search" | "movie-search" | "tv-search"
                        if attr(&attrs, "available")
                            .is_some_and(|v| v.eq_ignore_ascii_case("yes")) =>
                    {
                        let params: Vec<String> = attr(&attrs, "supportedParams")
                            .unwrap_or("q")
                            .split(',')
                            .map(|p| p.trim().to_owned())
                            .filter(|p| !p.is_empty())
                            .collect();
                        match name.as_str() {
                            "search" => caps.search = Some(params),
                            "movie-search" => caps.movie_search = Some(params),
                            _ => caps.tv_search = Some(params),
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => {
                if saw_caps {
                    break;
                }
                return Err(SourceError::BadAnswer(format!(
                    "the answer is not a Torznab caps document ({err})"
                )));
            }
        }
    }
    if !saw_caps && error.is_none() {
        return Err(SourceError::BadAnswer(
            "the answer is not a Torznab caps document".to_owned(),
        ));
    }
    Ok((caps, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(items: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed" xmlns:nyaa="https://nyaa.si/xmlns/nyaa"><channel><title>x</title>{items}</channel></rss>"#
        )
    }

    #[test]
    fn a_torznab_item_is_read_with_its_attributes() {
        let parsed = parse_feed(&feed(
            r#"<item><title>Movie.2020.1080p.WEB-DL</title><guid>g1</guid><link>https://t.example/dl/1.torrent?apikey=k</link>
            <pubDate>Sat, 17 Oct 2026 15:42:41 +0000</pubDate><size>2147483648</size>
            <enclosure url="https://t.example/dl/1.torrent" length="2147483648" type="application/x-bittorrent"/>
            <torznab:attr name="seeders" value="40"/><torznab:attr name="peers" value="55"/>
            <torznab:attr name="infohash" value="0123456789ABCDEF0123456789ABCDEF01234567"/>
            <torznab:attr name="magneturl" value="magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&amp;dn=x"/>
            <torznab:attr name="imdbid" value="0063350"/><torznab:attr name="category" value="2040"/>
            <torznab:attr name="category" value="100001"/><torznab:attr name="season" value="2"/><torznab:attr name="episode" value="5"/></item>"#,
        ))
        .expect("feed");
        assert_eq!(parsed.items.len(), 1);
        let row = &parsed.items[0];
        assert_eq!(row.name, "Movie.2020.1080p.WEB-DL");
        assert_eq!(
            row.info_hash.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(row.size, Some(2_147_483_648));
        assert_eq!(
            (row.seeders, row.leechers),
            (Some(40), Some(15)),
            "peers include seeders"
        );
        assert_eq!(row.imdb.as_deref(), Some("tt0063350"));
        assert_eq!(row.category.as_deref(), Some("2040"));
        assert_eq!((row.season, row.episode), (Some(2), Some(5)));
        assert!(row.magnet.as_deref().is_some_and(|m| m.contains("dn=x")));
        assert_eq!(
            row.torrent_url.as_deref(),
            Some("https://t.example/dl/1.torrent?apikey=k")
        );
        assert!(row.published.is_some());
    }

    #[test]
    fn an_item_with_only_a_torrent_link_has_no_hash_and_no_magnet() {
        let parsed = parse_feed(&feed(
            r#"<item><title>No.Hash.2020</title><link>https://t.example/dl/2</link><torznab:attr name="seeders" value="3"/></item>"#,
        ))
        .expect("feed");
        let row = &parsed.items[0];
        assert_eq!(row.info_hash, None);
        assert_eq!(row.magnet, None);
        assert_eq!(row.torrent_url.as_deref(), Some("https://t.example/dl/2"));
    }

    #[test]
    fn a_magnet_item_link_gives_the_hash() {
        let parsed = parse_feed(&feed(
            r#"<item><title>M.2020</title><link>magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567</link></item>"#,
        ))
        .expect("feed");
        assert_eq!(
            parsed.items[0].info_hash.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(parsed.items[0].torrent_url, None);
    }

    #[test]
    fn a_nyaa_item_is_read() {
        let parsed = parse_feed(&feed(
            r#"<item><title><![CDATA[[Group] Frieren - 05 [1080p]]]></title><link>https://nyaa.si/download/1.torrent</link>
            <nyaa:seeders>67</nyaa:seeders><nyaa:leechers>2</nyaa:leechers><nyaa:infoHash>8e4e8471b226e311eb7b3b77476eec7834052dbd</nyaa:infoHash>
            <nyaa:category>Anime</nyaa:category><nyaa:size>26.8 GiB</nyaa:size></item>"#,
        ))
        .expect("feed");
        let row = &parsed.items[0];
        assert_eq!(row.name, "[Group] Frieren - 05 [1080p]");
        assert_eq!((row.seeders, row.leechers), (Some(67), Some(2)));
        assert_eq!(row.size, parse_size("26.8 GiB"));
        assert_eq!(
            row.info_hash.as_deref(),
            Some("8e4e8471b226e311eb7b3b77476eec7834052dbd")
        );
        assert_eq!(
            row.torrent_url.as_deref(),
            Some("https://nyaa.si/download/1.torrent")
        );
    }

    #[test]
    fn entities_in_titles_are_resolved() {
        let parsed = parse_feed(&feed(
            r"<item><title>Fast &amp; Furious &#233;</title><link>https://t.example/1</link></item>",
        ))
        .expect("feed");
        assert_eq!(parsed.items[0].name, "Fast & Furious \u{e9}");
    }

    #[test]
    fn broken_items_are_skipped_and_counted() {
        let parsed = parse_feed(&feed(
            r#"<item><title>Good.One</title><link>https://t.example/1</link></item>
            <item><link>https://t.example/2</link></item>
            <item><title>No.Link.Or.Hash</title></item>
            <item><title>Bad.Hash</title><torznab:attr name="infohash" value="zz"/></item>
            <item><title>Good.Two</title><torznab:attr name="infohash" value="0123456789abcdef0123456789abcdef01234567"/></item>"#,
        ))
        .expect("feed");
        let names: Vec<_> = parsed.items.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["Good.One", "Good.Two"]);
        assert_eq!(parsed.skipped, 3);
    }

    #[test]
    fn a_cut_off_feed_keeps_the_items_before_the_cut() {
        let full = feed(
            r#"<item><title>One.2020</title><link>https://t.example/1</link></item><item><title>Two.2020</title><link>https://t.example/2</link></item>"#,
        );
        let cut = &full[..full.find("<item><title>Two").unwrap() + 20];
        let parsed = parse_feed(cut).expect("a partial feed");
        assert_eq!(parsed.items.len(), 1);
    }

    #[test]
    fn text_that_is_not_a_feed_is_refused() {
        for text in [
            "",
            "not xml at all",
            "{\"json\":true}",
            "<html><body>hi</body></html>",
        ] {
            assert!(parse_feed(text).is_err(), "{text:?}");
        }
        let empty = parse_feed(&feed("")).expect("an empty feed");
        assert!(empty.items.is_empty());
    }

    #[test]
    fn an_error_answer_is_reported() {
        let parsed = parse_feed(
            r#"<?xml version="1.0"?><error code="100" description="Incorrect user credentials"/>"#,
        )
        .expect("an error feed");
        assert_eq!(
            parsed.error,
            Some(FeedError {
                code: 100,
                description: "Incorrect user credentials".into()
            })
        );
        assert!(parsed.items.is_empty());
    }

    #[test]
    fn sizes_are_bytes_or_text_with_a_unit() {
        assert_eq!(parse_size("1234"), Some(1234));
        assert_eq!(parse_size("1 KiB"), Some(1024));
        assert_eq!(parse_size("1.5 GiB"), Some(1_610_612_736));
        assert_eq!(parse_size("700 MB"), Some(700_000_000));
        assert_eq!(parse_size("1,024 KiB"), Some(1_048_576));
        assert_eq!(parse_size("abc"), None);
        assert_eq!(parse_size("5 parsecs"), None);
        assert_eq!(parse_size(""), None);
    }

    #[test]
    fn caps_say_what_can_be_searched() {
        let (caps, error) = parse_caps(
            r#"<?xml version="1.0"?><caps><server title="x"/><searching>
            <search available="yes" supportedParams="q"/>
            <tv-search available="yes" supportedParams="q,season,ep,imdbid,tvdbid"/>
            <movie-search available="no" supportedParams="q,imdbid"/>
            </searching><categories><category id="2000" name="Movies"/></categories></caps>"#,
        )
        .expect("caps");
        assert_eq!(error, None);
        assert!(caps.can_search());
        assert!(caps.tv_by_imdb() && caps.tv_by_season_episode());
        assert!(!caps.movie_by_imdb(), "unavailable means unsupported");
        assert_eq!(caps.search, Some(vec!["q".to_owned()]));
    }

    #[test]
    fn caps_errors_and_other_documents() {
        let (_, error) =
            parse_caps(r#"<error code="100" description="Wrong key"/>"#).expect("an error answer");
        assert_eq!(error.map(|e| e.code), Some(100));
        assert!(parse_caps("<html></html>").is_err());
        assert!(parse_caps("nope").is_err());
        assert!(!Caps::basic().movie_by_imdb());
        assert!(Caps::basic().can_search());
    }
}

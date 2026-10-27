//! Trailers.

use serde::{Deserialize, Serialize};

/// A trailer reference: a remote URL or a local file, plus where it came from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Trailer {
    pub id: String,
    pub name: String,
    pub url: String,
    /// Quality label such as `1080p`.
    pub quality: String,
    pub language: String,
    /// Source provider key.
    pub provider: String,
    /// Whether the trailer URL is written to the NFO file.
    pub in_nfo: bool,
    /// Publication date.
    pub date: Option<crate::CalendarDate>,
    /// The scraper that found it.
    pub scraped_by: String,
    /// Length in seconds when the provider says, `0` when it does not.
    pub length_secs: u32,
}

impl Trailer {
    /// Whether the URL points at YouTube (`youtube.com`, `youtu.be`, `youtube-nocookie.com`).
    pub fn is_youtube(&self) -> bool {
        matches!(
            host_of(&self.url).as_deref(),
            Some(host) if host == "youtu.be" || host_is(host, "youtube.com") || host_is(host, "youtube-nocookie.com")
        )
    }

    /// Whether the URL is a web address (`http` or `https`), as opposed to a local file.
    pub fn is_web(&self) -> bool {
        let url = self.url.trim().to_ascii_lowercase();
        url.starts_with("http://") || url.starts_with("https://")
    }

    /// Whether the URL is a file on disk (`file:`), as opposed to a web address.
    pub fn is_local(&self) -> bool {
        self.url.trim().to_ascii_lowercase().starts_with("file:")
    }

    /// The site the trailer is hosted on, as the Trailers table shows it in its source column:
    /// `YouTube`, `Vimeo`, `IMDb`, `HD-Trailers`, the host name for any other web address, and
    /// `Local file` for a file on disk. Empty when the URL says nothing.
    pub fn source_site(&self) -> String {
        if self.is_local() {
            return "Local file".to_owned();
        }
        let Some(host) = host_of(&self.url) else {
            return String::new();
        };
        if self.is_youtube() {
            "YouTube".to_owned()
        } else if host_is(&host, "vimeo.com") {
            "Vimeo".to_owned()
        } else if host_is(&host, "imdb.com") {
            "IMDb".to_owned()
        } else if host_is(&host, "hd-trailers.net") {
            "HD-Trailers".to_owned()
        } else {
            host.strip_prefix("www.").unwrap_or(&host).to_owned()
        }
    }
}

/// The lower-case host of an `http` or `https` URL.
fn host_of(url: &str) -> Option<String> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = authority.split(':').next()?.to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Whether `host` is `domain` or one of its sub-domains.
fn host_is(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CalendarDate;

    #[test]
    fn round_trip() {
        let t = Trailer {
            id: "abc".into(),
            name: "Official Trailer".into(),
            url: "https://example.test/t".into(),
            quality: "1080p".into(),
            language: "en".into(),
            provider: "tmdb".into(),
            in_nfo: true,
            date: CalendarDate::new(2024, 3, 1),
            scraped_by: "tmdb".into(),
            length_secs: 0,
        };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Trailer>(&json).unwrap(), t);
    }

    fn at(url: &str) -> Trailer {
        Trailer {
            url: url.into(),
            ..Trailer::default()
        }
    }

    #[test]
    fn the_source_site_comes_from_the_url() {
        let cases = [
            ("https://www.youtube.com/watch?v=abc", "YouTube", true, true),
            ("https://youtu.be/abc", "YouTube", true, true),
            ("https://m.youtube.com/watch?v=abc", "YouTube", true, true),
            (
                "https://notyoutube.com/watch?v=abc",
                "notyoutube.com",
                false,
                true,
            ),
            ("https://vimeo.com/123", "Vimeo", false, true),
            ("https://www.imdb.com/video/vi123/", "IMDb", false, true),
            (
                "https://www.hd-trailers.net/movie/heat/",
                "HD-Trailers",
                false,
                true,
            ),
            (
                "http://cdn.example.test:8080/t.mp4",
                "cdn.example.test",
                false,
                true,
            ),
            (
                "file:///C:/Movies/Heat-trailer.mkv",
                "Local file",
                false,
                false,
            ),
            ("not a url", "", false, false),
        ];
        for (url, site, youtube, web) in cases {
            let trailer = at(url);
            assert_eq!(trailer.source_site(), site, "{url}");
            assert_eq!(trailer.is_youtube(), youtube, "{url}");
            assert_eq!(trailer.is_web(), web, "{url}");
        }
        assert!(at("FILE:///x.mkv").is_local());
        assert!(!at("https://youtube.com/x").is_local());
    }
}

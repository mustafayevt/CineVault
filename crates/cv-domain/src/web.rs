//! Web addresses the app may hand to the browser.

/// `url` as an address the browser may open: an absolute `http` or `https` address with a host
/// and no whitespace or control characters. Anything else (another scheme, a path, a relative
/// address, an empty host) is `None`. The scheme is matched without regard to case.
#[must_use]
pub fn web_address(url: &str) -> Option<String> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://")?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_ok = !host.is_empty() && !host.contains('@') && !host.starts_with(':');
    let clean = !url.chars().any(|c| c.is_whitespace() || c.is_control());
    (host_ok && clean).then(|| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::web_address;

    #[test]
    fn http_and_https_addresses_with_a_host_pass() {
        for ok in [
            "https://www.imdb.com/title/tt0083658/",
            "http://example.org",
            "HTTPS://letterboxd.com/imdb/tt0083658/",
            "  https://trakt.tv/search/imdb/tt0083658  ",
        ] {
            assert_eq!(web_address(ok).as_deref(), Some(ok.trim()), "{ok}");
        }
    }

    #[test]
    fn other_schemes_paths_and_odd_addresses_are_refused() {
        for bad in [
            "",
            "file:///C:/Windows/system32/calc.exe",
            "javascript:alert(1)",
            "mailto:someone@example.org",
            "ftp://example.org/",
            r"C:\Windows\notepad.exe",
            "/m/blade_runner",
            "https://",
            "https:///path",
            "https://user@evil.example/",
            "https://exa mple.org/",
            "https://example.org/\u{0}",
            "cv://img/1",
        ] {
            assert_eq!(web_address(bad), None, "{bad:?}");
        }
    }
}

//! Info-hashes, magnet links and their validation.
//!
//! Everything a magnet link carries comes from a website, so [`validate_magnet`] is strict: one
//! `urn:btih:` hash, only the parameters a torrent app needs, no control characters, bounded
//! length. The web view never opens a magnet itself; the command that does calls this first.

use data_encoding::BASE32_NOPAD;

/// The longest magnet link accepted.
pub const MAX_MAGNET_LEN: usize = 8 * 1024;

/// Public trackers added to a magnet link built from a bare info-hash.
pub const PUBLIC_TRACKERS: [&str; 8] = [
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.tracker.cl:1337/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://open.stealth.si:80/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
    "udp://explodie.org:6969/announce",
    "udp://tracker.tiny-vps.com:6969/announce",
];

/// Why a magnet link was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MagnetError {
    #[error("this is not a magnet link")]
    NotMagnet,
    #[error("the magnet link is too long")]
    TooLong,
    #[error("the magnet link has unusable characters")]
    BadCharacters,
    #[error("the magnet link has no info-hash")]
    NoHash,
    #[error("the magnet link has more than one info-hash")]
    ManyHashes,
    #[error("the info-hash is not 40 hex or 32 base32 characters")]
    BadHash,
    #[error("the magnet link has an unexpected parameter: {0}")]
    UnknownParameter(String),
    #[error("the magnet link has an unusable address: {0}")]
    BadAddress(String),
}

/// The lower-case hex form of an info-hash given as 40 hex characters or 32 base32 characters,
/// with or without a `urn:btih:` prefix. `None` for anything else.
#[must_use]
pub fn normalise_info_hash(text: &str) -> Option<String> {
    let text = text.trim();
    let text = text
        .get(..9)
        .filter(|prefix| prefix.eq_ignore_ascii_case("urn:btih:"))
        .map_or(text, |_| &text[9..]);
    match text.len() {
        40 if text.bytes().all(|b| b.is_ascii_hexdigit()) => Some(text.to_ascii_lowercase()),
        32 if text
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || (b'2'..=b'7').contains(&b)) =>
        {
            let bytes = BASE32_NOPAD
                .decode(text.to_ascii_uppercase().as_bytes())
                .ok()?;
            (bytes.len() == 20).then(|| hex(&bytes))
        }
        _ => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Percent-encodes everything except letters, digits and `-._~`.
#[must_use]
pub fn percent_encode(text: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Decodes `%XX` escapes and `+` as a space. `None` for a broken escape or bytes that are not
/// UTF-8 once decoded.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let pair = bytes.get(i + 1..i + 3)?;
                let value = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
                out.push(value);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// A magnet link for `info_hash` (hex or base32) with the display name and trackers given.
/// Returns `None` when the hash is not valid.
#[must_use]
pub fn build_magnet(info_hash: &str, name: &str, trackers: &[&str]) -> Option<String> {
    let hash = normalise_info_hash(info_hash)?;
    let mut uri = format!("magnet:?xt=urn:btih:{hash}");
    let name = name.trim();
    if !name.is_empty() {
        uri.push_str("&dn=");
        uri.push_str(&percent_encode(name));
    }
    for tracker in trackers {
        uri.push_str("&tr=");
        uri.push_str(&percent_encode(tracker));
    }
    Some(uri)
}

/// A magnet link that passed [`validate_magnet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidMagnet {
    /// 40 lower-case hex characters.
    pub info_hash: String,
    /// The display name (`dn`), decoded.
    pub name: Option<String>,
    pub trackers: Vec<String>,
}

fn is_tracker_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["udp://", "http://", "https://", "wss://", "ws://"]
        .iter()
        .any(|scheme| lower.starts_with(scheme) && lower.len() > scheme.len())
}

fn is_web_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["http://", "https://"]
        .iter()
        .any(|scheme| lower.starts_with(scheme) && lower.len() > scheme.len())
}

/// Checks a magnet link: the `magnet:?` scheme, exactly one `xt=urn:btih:` with 40 hex or 32
/// base32 characters (a `urn:btmh:` hash may sit beside it and is ignored), and no parameters
/// other than `xt`, `dn`, `tr` (also `tr.1`, `tr.2`, ...), `xl` and `ws`.
///
/// # Errors
/// [`MagnetError`] names the first problem found.
pub fn validate_magnet(uri: &str) -> Result<ValidMagnet, MagnetError> {
    if uri.len() > MAX_MAGNET_LEN {
        return Err(MagnetError::TooLong);
    }
    if uri.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(MagnetError::BadCharacters);
    }
    let query = uri
        .get(..8)
        .filter(|prefix| prefix.eq_ignore_ascii_case("magnet:?"))
        .map(|_| &uri[8..])
        .ok_or(MagnetError::NotMagnet)?;

    let mut hash: Option<String> = None;
    let mut name = None;
    let mut trackers = Vec::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = key.to_ascii_lowercase();
        let value = percent_decode(raw_value).ok_or(MagnetError::BadCharacters)?;
        if value.chars().any(char::is_control) {
            return Err(MagnetError::BadCharacters);
        }
        match key.as_str() {
            "xt" => {
                let lower = value.to_ascii_lowercase();
                if lower.starts_with("urn:btih:") {
                    if hash.is_some() {
                        return Err(MagnetError::ManyHashes);
                    }
                    hash = Some(normalise_info_hash(&value).ok_or(MagnetError::BadHash)?);
                } else if !lower.starts_with("urn:btmh:") {
                    return Err(MagnetError::UnknownParameter(format!("xt={value}")));
                }
            }
            "dn" => name = Some(value),
            "xl" => {
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(MagnetError::UnknownParameter("xl".into()));
                }
            }
            "ws" => {
                if !is_web_url(&value) {
                    return Err(MagnetError::BadAddress(value));
                }
            }
            k if k == "tr"
                || k.strip_prefix("tr.")
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) =>
            {
                if !is_tracker_url(&value) {
                    return Err(MagnetError::BadAddress(value));
                }
                trackers.push(value);
            }
            other => return Err(MagnetError::UnknownParameter(other.to_owned())),
        }
    }
    Ok(ValidMagnet {
        info_hash: hash.ok_or(MagnetError::NoHash)?,
        name,
        trackers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "0123456789abcdef0123456789abcdef01234567";

    fn base32_of(hex_hash: &str) -> String {
        let bytes: Vec<u8> = (0..20)
            .map(|i| u8::from_str_radix(&hex_hash[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        BASE32_NOPAD.encode(&bytes)
    }

    #[test]
    fn hex_hashes_are_lowercased() {
        assert_eq!(
            normalise_info_hash("0123456789ABCDEF0123456789ABCDEF01234567").as_deref(),
            Some(HEX)
        );
        assert_eq!(
            normalise_info_hash(&format!("urn:btih:{HEX}")).as_deref(),
            Some(HEX)
        );
        assert_eq!(
            normalise_info_hash(&format!("  {HEX}  ")).as_deref(),
            Some(HEX)
        );
    }

    #[test]
    fn base32_and_hex_spell_one_hash() {
        let b32 = base32_of(HEX);
        assert_eq!(b32.len(), 32);
        assert_eq!(normalise_info_hash(&b32).as_deref(), Some(HEX));
        assert_eq!(
            normalise_info_hash(&b32.to_lowercase()).as_deref(),
            Some(HEX)
        );
    }

    #[test]
    fn wrong_hashes_are_refused() {
        for bad in [
            "",
            "abc",
            "zz23456789abcdef0123456789abcdef01234567",
            "0123456789abcdef0123456789abcdef0123456",
            "0123456789abcdef0123456789abcdef012345678",
            "11111111111111111111111111111111",
            "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!",
        ] {
            assert_eq!(normalise_info_hash(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_built_magnet_validates_and_round_trips() {
        let uri = build_magnet(HEX, "Movie (2020) 1080p & more", &PUBLIC_TRACKERS).unwrap();
        assert!(uri.starts_with(&format!("magnet:?xt=urn:btih:{HEX}&dn=Movie%20%282020%29")));
        let parsed = validate_magnet(&uri).unwrap();
        assert_eq!(parsed.info_hash, HEX);
        assert_eq!(parsed.name.as_deref(), Some("Movie (2020) 1080p & more"));
        assert_eq!(parsed.trackers.len(), PUBLIC_TRACKERS.len());
        assert_eq!(parsed.trackers[0], PUBLIC_TRACKERS[0]);
    }

    #[test]
    fn a_magnet_from_a_base32_hash_is_built_in_hex() {
        let uri = build_magnet(&base32_of(HEX), "", &[]).unwrap();
        assert_eq!(uri, format!("magnet:?xt=urn:btih:{HEX}"));
        assert_eq!(build_magnet("nope", "x", &[]), None);
    }

    #[test]
    fn valid_magnets_pass() {
        let ok = format!(
            "MAGNET:?xt=urn:btih:{HEX}&dn=A+B&tr=udp%3A%2F%2Ft.example%3A80%2Fannounce&tr.1=https://t.example/a&xl=123&ws=https://x.example/f&xt=urn:btmh:1220abcd"
        );
        let parsed = validate_magnet(&ok).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("A B"));
        assert_eq!(parsed.trackers.len(), 2);
        assert_eq!(
            validate_magnet(&format!("magnet:?xt=urn:btih:{}", base32_of(HEX)))
                .unwrap()
                .info_hash,
            HEX
        );
    }

    #[test]
    fn invalid_magnets_name_the_problem() {
        let cases: Vec<(String, MagnetError)> = vec![
            ("https://x.example".into(), MagnetError::NotMagnet),
            ("".into(), MagnetError::NotMagnet),
            ("magnet:?dn=x".into(), MagnetError::NoHash),
            ("magnet:?xt=urn:btih:zz".into(), MagnetError::BadHash),
            (
                format!("magnet:?xt=urn:btih:{HEX}&xt=urn:btih:{HEX}"),
                MagnetError::ManyHashes,
            ),
            (
                format!("magnet:?xt=urn:btih:{HEX}&x.pe=1.2.3.4:5"),
                MagnetError::UnknownParameter("x.pe".into()),
            ),
            (
                format!("magnet:?xt=urn:btih:{HEX}&xt=urn:sha1:abc"),
                MagnetError::UnknownParameter("xt=urn:sha1:abc".into()),
            ),
            (
                format!("magnet:?xt=urn:btih:{HEX}&tr=javascript:alert(1)"),
                MagnetError::BadAddress("javascript:alert(1)".into()),
            ),
            (
                format!("magnet:?xt=urn:btih:{HEX}&ws=file:///c:/x"),
                MagnetError::BadAddress("file:///c:/x".into()),
            ),
            (
                format!("magnet:?xt=urn:btih:{HEX}&xl=big"),
                MagnetError::UnknownParameter("xl".into()),
            ),
            (
                format!("magnet:?xt=urn:btih:{HEX}&dn=a%0Ab"),
                MagnetError::BadCharacters,
            ),
            (
                format!("magnet:?xt=urn:btih:{HEX}&dn=a%ZZ"),
                MagnetError::BadCharacters,
            ),
            (
                format!("magnet:?xt=urn:btih:{HEX}&dn=a b"),
                MagnetError::BadCharacters,
            ),
            (
                format!("magnet:?xt=urn:btih:{HEX}\r\n"),
                MagnetError::BadCharacters,
            ),
            (
                format!(
                    "magnet:?xt=urn:btih:{HEX}&dn={}",
                    "a".repeat(MAX_MAGNET_LEN)
                ),
                MagnetError::TooLong,
            ),
        ];
        for (uri, expected) in cases {
            let shown = if uri.len() > 80 {
                format!("{}...", &uri[..80])
            } else {
                uri.clone()
            };
            assert_eq!(validate_magnet(&uri), Err(expected), "{shown}");
        }
    }

    #[test]
    fn percent_encoding_round_trips_unicode() {
        let name = "Amélie – 2001 [1080p]";
        let uri = build_magnet(HEX, name, &[]).unwrap();
        assert_eq!(validate_magnet(&uri).unwrap().name.as_deref(), Some(name));
    }
}

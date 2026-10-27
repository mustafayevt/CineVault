//! Secret redaction for logs, error messages and exports.
//!
//! Two layers work together:
//!
//! 1. **Pattern rules** mask credentials by shape: API-key style query parameters, credential
//!    headers, `Bearer` tokens and `user:password@` URL userinfo.
//! 2. **Known values**: every secret the app stores or reads through
//!    [`crate::secrets::SecretStore`] is registered here, and any literal occurrence of it
//!    (raw or percent-encoded) is masked wherever it shows up, for example inside a URL path.
//!
//! [`RedactingMakeWriter`] plugs both layers into a `tracing-subscriber` formatter so nothing
//! reaches the log sink unredacted.

use std::borrow::Cow;
use std::io::{self, Write};
use std::sync::{OnceLock, RwLock};

use regex::{Captures, Regex};
use tracing_subscriber::fmt::MakeWriter;

/// What replaces a secret in redacted text.
pub const MASK: &str = "[REDACTED]";

/// Known values shorter than this are not registered: masking them would mangle ordinary text.
const MIN_KNOWN_LEN: usize = 6;

static KNOWN: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Query / form parameter names whose values are credentials.
const PARAM_NAMES: &str = "api[_-]?key|apikey|key|token|access[_-]?token|refresh[_-]?token|\
id[_-]?token|client[_-]?secret|secret|password|passwd|pwd|session[_-]?id|auth|signature|sig";

/// Header names whose values are credentials.
const HEADER_NAMES: &str = "authorization|proxy-authorization|x-api-key|api-key|apikey|\
x-auth-token|x-emby-token|x-mediabrowser-token|x-plex-token|cookie|set-cookie";

struct Rules {
    param: Regex,
    header: Regex,
    bearer: Regex,
    userinfo: Regex,
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| Rules {
        // `?api_key=VALUE`, `&token=VALUE`, or a bare `key=VALUE` at a word start.
        param: Regex::new(&format!(
            r#"(?i)(?P<pre>^|[?&;\s"'(,])(?P<name>{PARAM_NAMES})=(?P<val>[^&\s"'#;,)]+)"#
        ))
        .expect("param rule compiles"),
        // `Authorization: Bearer abc`, `"x-api-key": "abc"`, `cookie=abc`.
        header: Regex::new(&format!(
            r#"(?i)(?P<head>["']?\b(?:{HEADER_NAMES})["']?\s*[:=]\s*["']?)(?P<val>[^\r\n"']+)"#
        ))
        .expect("header rule compiles"),
        bearer: Regex::new(r"(?i)\b(?P<scheme>bearer)\s+[A-Za-z0-9._~+/=-]{8,}")
            .expect("bearer rule compiles"),
        userinfo: Regex::new(r"(?i)(?P<head>://[^/\s:@]+:)[^@/\s]+@")
            .expect("userinfo rule compiles"),
    })
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Registers a secret value so every later occurrence of it is masked. Values shorter than six
/// characters are ignored.
pub fn register_secret(value: &str) {
    if value.len() < MIN_KNOWN_LEN {
        return;
    }
    let mut known = KNOWN.write().unwrap_or_else(|e| e.into_inner());
    for form in [value.to_owned(), percent_encode(value)] {
        if !known.contains(&form) {
            known.push(form);
        }
    }
}

/// Forgets a secret value, for example after the user deletes the key.
pub fn unregister_secret(value: &str) {
    let encoded = percent_encode(value);
    let mut known = KNOWN.write().unwrap_or_else(|e| e.into_inner());
    known.retain(|k| k != value && *k != encoded);
}

/// Returns `text` with credentials masked. Borrows when nothing needed masking.
pub fn redact(text: &str) -> Cow<'_, str> {
    let rules = rules();
    let mut current = text.to_owned();

    let masked = rules.param.replace_all(&current, |c: &Captures| {
        format!("{}{}={MASK}", &c["pre"], &c["name"])
    });
    if let Cow::Owned(s) = masked {
        current = s;
    }
    let masked = rules
        .header
        .replace_all(&current, |c: &Captures| format!("{}{MASK}", &c["head"]));
    if let Cow::Owned(s) = masked {
        current = s;
    }
    let masked = rules
        .bearer
        .replace_all(&current, |c: &Captures| format!("{} {MASK}", &c["scheme"]));
    if let Cow::Owned(s) = masked {
        current = s;
    }
    let masked = rules
        .userinfo
        .replace_all(&current, |c: &Captures| format!("{}{MASK}@", &c["head"]));
    if let Cow::Owned(s) = masked {
        current = s;
    }

    {
        let known = KNOWN.read().unwrap_or_else(|e| e.into_inner());
        let mut hits: Vec<&String> = known
            .iter()
            .filter(|k| current.contains(k.as_str()))
            .collect();
        // Longest first so a secret that contains another is masked whole.
        hits.sort_by_key(|k| std::cmp::Reverse(k.len()));
        for k in hits {
            current = current.replace(k.as_str(), MASK);
        }
    }

    if current == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(current)
    }
}

/// A [`Write`] adapter that redacts each line before passing it on.
///
/// Bytes are held until a newline arrives (or the writer is flushed or dropped) so a secret
/// split across two writes is still caught.
pub struct RedactingWriter<W: Write> {
    inner: W,
    pending: Vec<u8>,
}

impl<W: Write> RedactingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            pending: Vec::new(),
        }
    }

    fn emit(&mut self, bytes: &[u8]) -> io::Result<()> {
        let text = String::from_utf8_lossy(bytes);
        self.inner.write_all(redact(&text).as_bytes())
    }

    fn drain(&mut self, all: bool) -> io::Result<()> {
        while let Some(pos) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=pos).collect();
            self.emit(&line)?;
        }
        if all && !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            self.emit(&rest)?;
        }
        Ok(())
    }
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        self.drain(false)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.drain(true)?;
        self.inner.flush()
    }
}

impl<W: Write> Drop for RedactingWriter<W> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// Wraps any `tracing-subscriber` writer factory so every log line is redacted:
///
/// ```ignore
/// tracing_subscriber::fmt().with_writer(RedactingMakeWriter::new(std::io::stderr)).init();
/// ```
#[derive(Debug, Clone)]
pub struct RedactingMakeWriter<M>(M);

impl<M> RedactingMakeWriter<M> {
    pub fn new(inner: M) -> Self {
        Self(inner)
    }
}

impl<'a, M> MakeWriter<'a> for RedactingMakeWriter<M>
where
    M: MakeWriter<'a>,
{
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter::new(self.0.make_writer())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_query_parameters() {
        let out =
            redact("GET https://api.example.org/3/movie/603?api_key=abcd1234efgh&language=en");
        assert_eq!(
            out,
            "GET https://api.example.org/3/movie/603?api_key=[REDACTED]&language=en"
        );
        assert_eq!(
            redact("https://x.test/?a=1&Token=zzzzzzzz"),
            "https://x.test/?a=1&Token=[REDACTED]"
        );
    }

    #[test]
    fn does_not_touch_lookalike_parameters() {
        let text = "https://x.test/?monkey=1&keyword=heat&language=en";
        assert!(matches!(redact(text), Cow::Borrowed(_)));
    }

    #[test]
    fn masks_headers_in_plain_and_debug_form() {
        assert_eq!(
            redact("Authorization: Bearer abcdef123456"),
            "Authorization: [REDACTED]"
        );
        assert_eq!(
            redact(r#"{"x-api-key": "sekrit-value", "accept": "json"}"#),
            r#"{"x-api-key": "[REDACTED]", "accept": "json"}"#
        );
    }

    #[test]
    fn masks_bearer_tokens_outside_headers() {
        assert_eq!(
            redact("sent bearer eyJhbGciOi.payload.sig ok"),
            "sent bearer [REDACTED] ok"
        );
    }

    #[test]
    fn masks_url_userinfo() {
        assert_eq!(
            redact("ftp://alice:hunter2pw@host/path"),
            "ftp://alice:[REDACTED]@host/path"
        );
    }

    #[test]
    fn masks_registered_values_raw_and_encoded() {
        register_secret("redact-unit/value+1");
        let out = redact("path /redact-unit/value+1/x and q=redact-unit%2Fvalue%2B1");
        assert!(!out.contains("redact-unit"), "{out}");
        unregister_secret("redact-unit/value+1");
        assert!(redact("redact-unit/value+1").contains("redact-unit"));
    }

    #[test]
    fn writer_catches_secrets_split_across_writes() {
        register_secret("split-secret-value");
        let mut sink = Vec::new();
        {
            let mut w = RedactingWriter::new(&mut sink);
            w.write_all(b"key was split-sec").unwrap();
            w.write_all(b"ret-value here\n").unwrap();
        }
        let text = String::from_utf8(sink).unwrap();
        assert_eq!(text, "key was [REDACTED] here\n");
        unregister_secret("split-secret-value");
    }
}

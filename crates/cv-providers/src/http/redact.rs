//! Keeps credentials out of URLs, log lines and error messages that the HTTP client produces.
//!
//! The client knows exactly which parts of a request are credentials (the values passed through
//! [`crate::http::HttpRequest::credential_query`], `credential_header` and `secret`), so it masks
//! those directly. Two more layers catch what it does not know about:
//!
//! * query parameters with a credential-style name (`api_key`, `token`, ...) are masked in any
//!   URL it prints, even one a provider built by hand;
//! * an optional application-wide [`Redactor`] (the app passes its secret-storage redaction) is
//!   applied last, so values registered there are masked wherever they show up.

use std::sync::Arc;

use reqwest::Url;

/// What replaces a secret in redacted text.
pub const MASK: &str = "[REDACTED]";

/// Secrets shorter than this are not matched as free text: they would mangle ordinary words.
const MIN_LITERAL_LEN: usize = 6;

/// Query parameter names whose values are credentials, compared case-insensitively.
const SENSITIVE_PARAMS: &[&str] = &[
    "api_key",
    "api-key",
    "apikey",
    "key",
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "client_secret",
    "secret",
    "password",
    "passwd",
    "pwd",
    "session_id",
    "sessionid",
    "auth",
    "signature",
    "sig",
];

/// An application-wide text redactor applied after the client's own masking.
pub type Redactor = Arc<dyn Fn(&str) -> String + Send + Sync>;

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

fn is_sensitive_param(name: &str) -> bool {
    SENSITIVE_PARAMS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

/// The masking rules for one request: its own secret values plus the optional global redactor.
#[derive(Clone, Default)]
pub(crate) struct Masker {
    literals: Vec<String>,
    global: Option<Redactor>,
}

impl Masker {
    pub(crate) fn new(secrets: &[String], global: Option<Redactor>) -> Self {
        let mut literals = Vec::new();
        for secret in secrets {
            if secret.len() < MIN_LITERAL_LEN {
                continue;
            }
            for form in [secret.clone(), percent_encode(secret)] {
                if !literals.contains(&form) {
                    literals.push(form);
                }
            }
        }
        // Longest first so a secret that contains another is masked whole.
        literals.sort_by_key(|l| std::cmp::Reverse(l.len()));
        Self { literals, global }
    }

    /// Masks the secrets in free text (an error message, a log field).
    pub(crate) fn text(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for literal in &self.literals {
            if out.contains(literal.as_str()) {
                out = out.replace(literal.as_str(), MASK);
            }
        }
        match &self.global {
            Some(global) => global(&out),
            None => out,
        }
    }

    /// Renders a URL for logs and cache metadata: user info and credential-style query values are
    /// masked, then known secrets anywhere in it (for example in the path).
    pub(crate) fn url(&self, url: &Url) -> String {
        let mut shown = url.clone();
        if !shown.username().is_empty() || shown.password().is_some() {
            let _ = shown.set_username("");
            let _ = shown.set_password(None);
        }
        if shown.query().is_some() {
            let pairs: Vec<(String, String)> = shown
                .query_pairs()
                .map(|(k, v)| {
                    let value = if is_sensitive_param(&k) {
                        MASK.to_owned()
                    } else {
                        v.into_owned()
                    };
                    (k.into_owned(), value)
                })
                .collect();
            shown.query_pairs_mut().clear().extend_pairs(pairs);
        }
        // The query serialiser percent-encodes the brackets; show the mask readably.
        let rendered = shown.to_string().replace("%5BREDACTED%5D", MASK);
        self.text(&rendered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_sensitive_query_parameters_and_user_info() {
        let url = Url::parse("https://user:pw@example.test/3/movie?api_key=abc123xyz&query=heat")
            .unwrap();
        let shown = Masker::default().url(&url);
        assert!(!shown.contains("abc123xyz"), "{shown}");
        assert!(!shown.contains("pw@"), "{shown}");
        assert!(shown.contains("query=heat"), "{shown}");
        assert!(shown.contains(MASK), "{shown}");
    }

    #[test]
    fn masks_known_secret_in_path_raw_and_encoded() {
        let masker = Masker::new(&["sec ret/value".to_owned()], None);
        let url = Url::parse("https://example.test/sec%20ret%2Fvalue/items").unwrap();
        let shown = masker.url(&url);
        assert!(!shown.contains("ret%2Fvalue"), "{shown}");
        assert_eq!(
            masker.text("failed for sec ret/value now"),
            format!("failed for {MASK} now")
        );
    }

    #[test]
    fn short_secrets_are_not_matched_as_text() {
        let masker = Masker::new(&["ab".to_owned()], None);
        assert_eq!(masker.text("abab"), "abab");
    }

    #[test]
    fn global_redactor_runs_last() {
        let global: Redactor = Arc::new(|t: &str| t.replace("magic-word", MASK));
        let masker = Masker::new(&[], Some(global));
        assert_eq!(masker.text("a magic-word b"), format!("a {MASK} b"));
    }
}

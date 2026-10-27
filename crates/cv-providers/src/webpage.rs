//! Fetching and reading a web page for the best-effort providers (OFDb, fernsehserien.de).
//!
//! The rules are the ones the spec sets for providers backed by website parsing:
//!
//! * a page that cannot be read fails the call with [`ProviderError::FormatChanged`] and returns
//!   nothing, so a caller never writes partial or garbage data;
//! * the unreadable page is dropped from the HTML cache, and a cached page that no longer parses
//!   is fetched once more, so a bad answer never sticks;
//! * a human-check page (HTTP 202 or 403) is a failed request, not a format change.

use crate::error::{HUMAN_CHECK, ProviderError};
use crate::http::{HttpClient, HttpRequest, HttpResponse};

/// Why a page could not be read: a short phrase for the log. The user sees
/// [`ProviderError::FormatChanged`].
pub(crate) type PageError = String;

/// Sends `request` and reads the page with `parse`. `provider` is the name shown in messages.
pub(crate) async fn fetch_page<T>(
    http: &HttpClient,
    provider: &str,
    mut request: HttpRequest,
    parse: impl Fn(&str) -> Result<T, PageError>,
) -> Result<T, ProviderError> {
    // A site that asked for a human check a moment ago is left alone for a while.
    if http.human_check_pause(provider) {
        return Err(human_check(provider, None));
    }
    loop {
        let response = http.send(request.clone()).await?;
        let outcome = read(provider, &response, &parse);
        if matches!(response.status(), 202 | 403) {
            http.note_human_check(provider);
        }
        match outcome {
            Ok(value) => return Ok(value),
            Err(error) => {
                if response.is_success() {
                    http.forget(&request).await;
                }
                let retry_fresh =
                    response.from_cache() && matches!(error, ProviderError::FormatChanged(_));
                if retry_fresh {
                    request = request.bypass_cache(true);
                    continue;
                }
                return Err(error);
            }
        }
    }
}

fn read<T>(
    provider: &str,
    response: &HttpResponse,
    parse: &impl Fn(&str) -> Result<T, PageError>,
) -> Result<T, ProviderError> {
    match response.status() {
        404 | 410 => Err(ProviderError::NotFound(provider.to_owned())),
        202 | 403 => Err(human_check(provider, Some(response.status()))),
        _ if !response.is_success() => Err(request_error(
            provider,
            format!("HTTP {}", response.status()),
        )),
        _ => parse(&response.text()).map_err(|reason| {
            tracing::warn!(provider, %reason, "provider page could not be read");
            ProviderError::FormatChanged(provider.to_owned())
        }),
    }
}

/// The request error of a site that asks for a human check; `status` is `None` when the site was
/// not asked at all because it asked a moment ago.
pub(crate) fn human_check(provider: &str, status: Option<u16>) -> ProviderError {
    let message = match status {
        Some(status) => format!("the site {HUMAN_CHECK} (HTTP {status}); try again later"),
        None => format!("the site {HUMAN_CHECK}; it is left alone for a few minutes"),
    };
    request_error(provider, message)
}

pub(crate) fn request_error(provider: &str, message: String) -> ProviderError {
    ProviderError::Request {
        provider: provider.to_owned(),
        message,
    }
}

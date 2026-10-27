//! Sending requests to TMDB and reading the answers.

use serde::de::DeserializeOwned;

use super::api::{ApiError, Configuration};
use super::language::request_language;
use super::{DEFAULT_IMAGE_BASE_URL, DISPLAY_NAME, PROVIDER_ID, TmdbProvider};
use crate::catalogue;
use crate::credentials::ResolvedCredentials;
use crate::error::ProviderError;
use crate::http::{HttpRequest, HttpResponse};
use crate::model::RequestOptions;

/// Query parameters of a request, besides the API key.
pub(super) type Params = Vec<(&'static str, String)>;

/// The `language` parameter for the preferred language.
pub(super) fn language_params(options: &RequestOptions) -> Params {
    request_language(&options.language)
        .map(|language| vec![("language", language)])
        .unwrap_or_default()
}

fn request_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message: message.into(),
    }
}

impl TmdbProvider {
    /// GET `path` and parse the JSON answer. The key is resolved now, so one entered or reset in
    /// Settings applies to the next call.
    pub(super) async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: Params,
        options: &RequestOptions,
    ) -> Result<T, ProviderError> {
        let credentials = self.inner.resolver.require(PROVIDER_ID)?;
        let response = self
            .send(&credentials, path, &params, options.bypass_cache, true)
            .await?;
        parse(&response)
    }

    /// Like [`Self::get`], but a missing resource is `Ok(None)`.
    pub(super) async fn get_optional<T: DeserializeOwned>(
        &self,
        path: &str,
        params: Params,
        options: &RequestOptions,
    ) -> Result<Option<T>, ProviderError> {
        match self.get(path, params, options).await {
            Ok(value) => Ok(Some(value)),
            Err(ProviderError::NotFound(_)) => Ok(None),
            Err(other) => Err(other),
        }
    }

    async fn send(
        &self,
        credentials: &ResolvedCredentials,
        path: &str,
        params: &Params,
        bypass_cache: bool,
        cached: bool,
    ) -> Result<HttpResponse, ProviderError> {
        self.send_with(credentials, path, params, bypass_cache, cached)
            .await
            .map_err(|e| e.with_key_hint(credentials, PROVIDER_ID))
    }

    async fn send_with(
        &self,
        credentials: &ResolvedCredentials,
        path: &str,
        params: &Params,
        bypass_cache: bool,
        cached: bool,
    ) -> Result<HttpResponse, ProviderError> {
        let key = credentials
            .get(catalogue::API_KEY)
            .map(|v| v.expose().to_owned())
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(missing_key)?;
        let url = format!("{}/{}", self.inner.base_url, path.trim_start_matches('/'));
        let mut request = HttpRequest::get(PROVIDER_ID, &url)
            .credential_query("api_key", key.trim())
            .bypass_cache(bypass_cache);
        for (name, value) in params {
            request = request.query(name, value);
        }
        if !cached {
            request = request.uncached();
        }
        let response = self.inner.http.send(request).await?;
        check_status(&response)?;
        Ok(response)
    }

    /// One cheap authenticated call with `credentials`, never served from the cache, so a
    /// changed key is judged on its own.
    pub(super) async fn verify(
        &self,
        credentials: &ResolvedCredentials,
    ) -> Result<(), ProviderError> {
        let response = self
            .send(credentials, "configuration", &Vec::new(), true, false)
            .await?;
        parse::<Configuration>(&response).map(|_| ())
    }

    /// The base URL for image paths: `images.secure_base_url` from `/configuration`, looked up
    /// once. Until it answers (or when it cannot), the documented default is used.
    pub(super) async fn image_base(&self, options: &RequestOptions) -> String {
        if let Some(known) = self
            .inner
            .image_base
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return known;
        }
        let fetched = self
            .get::<Configuration>("configuration", Vec::new(), options)
            .await
            .ok()
            .and_then(|c| c.images.secure_base_url)
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty());
        match fetched {
            Some(base) => {
                *self
                    .inner
                    .image_base
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(base.clone());
                base
            }
            None => DEFAULT_IMAGE_BASE_URL.to_owned(),
        }
    }
}

fn missing_key() -> ProviderError {
    let (url, fields) = super::TmdbProvider::schema()
        .map(|s| {
            (
                s.registration_url.to_owned(),
                vec![catalogue::API_KEY.to_owned()],
            )
        })
        .unwrap_or_default();
    ProviderError::MissingCredentials {
        provider: DISPLAY_NAME.to_owned(),
        fields,
        registration_url: url,
    }
}

/// Maps an HTTP status to the error a caller can act on. 2xx passes.
fn check_status(response: &HttpResponse) -> Result<(), ProviderError> {
    let status = response.status();
    if response.is_success() {
        return Ok(());
    }
    let message = response
        .json::<ApiError>()
        .ok()
        .and_then(|e| e.status_message)
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty());
    match status {
        401 => Err(ProviderError::Auth {
            provider: DISPLAY_NAME.to_owned(),
            message: message.unwrap_or_else(|| "Invalid API key".to_owned()),
        }),
        404 => Err(ProviderError::NotFound(DISPLAY_NAME.to_owned())),
        _ => Err(request_error(match message {
            Some(m) => format!("HTTP {status}: {m}"),
            None => format!("HTTP {status}"),
        })),
    }
}

fn parse<T: DeserializeOwned>(response: &HttpResponse) -> Result<T, ProviderError> {
    response.json().map_err(|e| {
        tracing::debug!(error = %e, "TMDB answered with an unexpected body");
        request_error("the response was not in the expected format")
    })
}

#[cfg(test)]
mod tests {
    use super::super::api::Text;
    use super::*;

    #[test]
    fn language_param_uses_the_request_code() {
        let options = RequestOptions {
            language: "tr".into(),
            ..RequestOptions::default()
        };
        assert_eq!(
            language_params(&options),
            vec![("language", "tr-TR".to_owned())]
        );
        let blank = RequestOptions {
            language: " ".into(),
            ..RequestOptions::default()
        };
        assert!(language_params(&blank).is_empty());
    }

    #[test]
    fn nullable_text_reads_as_empty() {
        let missing: Option<String> = None;
        assert_eq!(missing.text(), "");
    }
}

//! Signing in to TheTVDB and sending requests with the session token.
//!
//! `POST login` with the API key (and the subscriber PIN, when the user entered one with their
//! own key) answers with a token that every other request sends as `Authorization: Bearer`.
//! The token is kept for the provider's lifetime and replaced when the credentials change or
//! TheTVDB answers 401 (it expired).

use serde::de::DeserializeOwned;

use super::api::{ApiError, ArtworkType, Envelope, LoginData};
use super::{DISPLAY_NAME, PROVIDER_ID, Session, TvdbProvider};
use crate::catalogue;
use crate::credentials::ResolvedCredentials;
use crate::error::ProviderError;
use crate::http::{HttpRequest, HttpResponse};
use crate::model::RequestOptions;

/// Query parameters of a request.
pub(super) type Params = Vec<(&'static str, String)>;

fn request_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message: message.into(),
    }
}

/// The API key and the optional subscriber PIN.
fn login_parts(
    credentials: &ResolvedCredentials,
) -> Result<(String, Option<String>), ProviderError> {
    let key = credentials
        .get(catalogue::API_KEY)
        .map(|v| v.expose().trim().to_owned())
        .filter(|k| !k.is_empty())
        .ok_or_else(missing_key)?;
    let pin = credentials
        .get(catalogue::TVDB_PIN)
        .map(|v| v.expose().trim().to_owned())
        .filter(|p| !p.is_empty());
    Ok((key, pin))
}

fn missing_key() -> ProviderError {
    let (url, fields) = TvdbProvider::schema()
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

impl TvdbProvider {
    /// GET `path` and read the `data` of the answer. A missing `data` is [`ProviderError::NotFound`].
    pub(super) async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: Params,
        options: &RequestOptions,
    ) -> Result<T, ProviderError> {
        self.envelope::<T>(path, params, options)
            .await?
            .data
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))
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

    /// The whole answer, including paging links.
    pub(super) async fn envelope<T: DeserializeOwned>(
        &self,
        path: &str,
        params: Params,
        options: &RequestOptions,
    ) -> Result<Envelope<T>, ProviderError> {
        let credentials = self.inner.resolver.require(PROVIDER_ID)?;
        let response = self
            .send(&credentials, path, &params, options.bypass_cache)
            .await?;
        response.json().map_err(|e| {
            tracing::debug!(error = %e, "TheTVDB answered with an unexpected body");
            request_error("the response was not in the expected format")
        })
    }

    async fn send(
        &self,
        credentials: &ResolvedCredentials,
        path: &str,
        params: &Params,
        bypass_cache: bool,
    ) -> Result<HttpResponse, ProviderError> {
        let url = format!("{}/{}", self.inner.base_url, path.trim_start_matches('/'));
        let mut renewed = false;
        loop {
            let token = self.token(credentials).await?;
            let mut request = HttpRequest::get(PROVIDER_ID, &url)
                .credential_header("Authorization", &format!("Bearer {token}"))
                .header("Accept", "application/json")
                .bypass_cache(bypass_cache);
            for (name, value) in params {
                request = request.query(name, value);
            }
            let response = self
                .inner
                .http
                .send(request)
                .await
                .map_err(|e| e.with_key_hint(credentials, PROVIDER_ID))?;
            if response.status() == 401 && !renewed {
                // The token expired (or was revoked): sign in again once.
                renewed = true;
                self.forget_token().await;
                continue;
            }
            return check_status(&response, credentials);
        }
    }

    /// The session token for `credentials`, signing in when there is none for them yet.
    async fn token(&self, credentials: &ResolvedCredentials) -> Result<String, ProviderError> {
        let (key, pin) = login_parts(credentials)?;
        let wanted = format!("{key}\n{}", pin.as_deref().unwrap_or(""));
        let mut session = self.inner.session.lock().await;
        if let Some(current) = session.as_ref()
            && current.credentials == wanted
        {
            return Ok(current.token.clone());
        }
        let token = self.login(credentials).await?;
        *session = Some(Session {
            credentials: wanted,
            token: token.clone(),
        });
        Ok(token)
    }

    async fn forget_token(&self) {
        *self.inner.session.lock().await = None;
    }

    /// `POST login`: the token for `credentials`.
    async fn login(&self, credentials: &ResolvedCredentials) -> Result<String, ProviderError> {
        let (key, pin) = login_parts(credentials)?;
        let mut body = serde_json::json!({ "apikey": key });
        if let Some(pin) = &pin {
            body["pin"] = serde_json::Value::String(pin.clone());
        }
        let url = format!("{}/login", self.inner.base_url);
        let mut request = HttpRequest::post(PROVIDER_ID, &url, body.to_string().into_bytes())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .secret(&key);
        if let Some(pin) = &pin {
            request = request.secret(pin);
        }
        let response = self
            .inner
            .http
            .send(request)
            .await
            .map_err(|e| e.with_key_hint(credentials, PROVIDER_ID))?;
        if response.status() == 401 || response.status() == 403 {
            let message = if pin.is_some() {
                "Invalid API key or subscriber PIN"
            } else {
                "Invalid API key"
            };
            return Err(ProviderError::Auth {
                provider: DISPLAY_NAME.to_owned(),
                message: message.to_owned(),
            }
            .with_key_hint(credentials, PROVIDER_ID));
        }
        let response = check_status(&response, credentials)?;
        let token = response
            .json::<Envelope<LoginData>>()
            .ok()
            .and_then(|e| e.data)
            .and_then(|d| d.token)
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty());
        token.ok_or_else(|| request_error("sign-in did not return a token"))
    }

    /// Signs in with `credentials` to see whether TheTVDB accepts them. Never served from a
    /// cache, and the session token is not kept.
    pub(super) async fn verify(
        &self,
        credentials: &ResolvedCredentials,
    ) -> Result<(), ProviderError> {
        self.login(credentials).await.map(|_| ())
    }

    /// The artwork types (name, record type, size), read once and kept.
    pub(super) async fn artwork_types(
        &self,
        options: &RequestOptions,
    ) -> Result<std::sync::Arc<Vec<ArtworkType>>, ProviderError> {
        if let Some(known) = self
            .inner
            .artwork_types
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Ok(known);
        }
        let types: Vec<ArtworkType> = self.get("artwork/types", Vec::new(), options).await?;
        let types = std::sync::Arc::new(types);
        *self
            .inner
            .artwork_types
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(types.clone());
        Ok(types)
    }
}

/// Maps an HTTP status to the error a caller can act on. 2xx passes the response through.
fn check_status(
    response: &HttpResponse,
    credentials: &ResolvedCredentials,
) -> Result<HttpResponse, ProviderError> {
    if response.is_success() {
        return Ok(response.clone());
    }
    let status = response.status();
    let message = response
        .json::<ApiError>()
        .ok()
        .and_then(|e| e.message)
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty() && m.len() <= 200);
    let error = match status {
        401 | 403 => ProviderError::Auth {
            provider: DISPLAY_NAME.to_owned(),
            message: message.unwrap_or_else(|| "Invalid API key or subscriber PIN".to_owned()),
        },
        404 => ProviderError::NotFound(DISPLAY_NAME.to_owned()),
        _ => request_error(match message {
            Some(m) => format!("HTTP {status}: {m}"),
            None => format!("HTTP {status}"),
        }),
    };
    Err(error.with_key_hint(credentials, PROVIDER_ID))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{CredentialSchema, ResolvedCredentials};

    fn credentials(pairs: &[(&str, &str)]) -> ResolvedCredentials {
        let schema: &CredentialSchema = TvdbProvider::schema().expect("tvdb schema");
        ResolvedCredentials::from_user_values(schema, pairs.iter().copied()).unwrap()
    }

    #[test]
    fn the_pin_goes_with_the_users_own_key() {
        let (key, pin) =
            login_parts(&credentials(&[("api_key", " key-1234 "), ("pin", "pin-9")])).unwrap();
        assert_eq!(key, "key-1234");
        assert_eq!(pin.as_deref(), Some("pin-9"));

        let (_, none) = login_parts(&credentials(&[("api_key", "key-1234")])).unwrap();
        assert_eq!(none, None);
    }

    #[test]
    fn a_missing_key_names_the_field() {
        let err = login_parts(&credentials(&[("pin", "pin-9")])).unwrap_err();
        assert!(
            matches!(&err, ProviderError::MissingCredentials { fields, .. } if fields == &["api_key"]),
            "{err:?}"
        );
    }
}

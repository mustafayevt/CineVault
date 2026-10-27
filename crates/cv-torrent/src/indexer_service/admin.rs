//! A client for the indexer service's own admin API on the loopback port: signing in, the admin
//! password, the list of indexers, configuring, testing and deleting one.
//!
//! The service protects this API with a session cookie once an admin password is set, so the client
//! keeps a cookie jar. Nothing here is a search: searches use the Torznab feed of each indexer.

use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use reqwest::redirect::Policy;
use serde::Deserialize;
use serde_json::Value;

use crate::source::user_agent;

/// How long one admin request may take. A connection test can take a minute; the caller sets its
/// own, shorter limit where it needs one.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Why an admin request failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdminError {
    /// Nothing answered on the port.
    #[error("The indexer service did not answer: {0}")]
    Unreachable(String),
    /// The service wants a sign-in this client does not have.
    #[error("The indexer service did not accept the sign-in.")]
    Unauthorized,
    /// The service answered with an error; the text is what it said.
    #[error("{message}")]
    Failed { status: u16, message: String },
    /// The answer was not what the admin API gives.
    #[error("The indexer service gave an answer CineVault cannot read: {0}")]
    BadAnswer(String),
}

impl AdminError {
    /// The words of the error, for classifying a failed test.
    #[must_use]
    pub fn text(&self) -> String {
        self.to_string()
    }
}

/// One indexer as the service lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexerInfo {
    pub id: String,
    pub name: String,
    /// What the service says the site is (one line; empty when it says nothing).
    pub description: String,
    /// `public`, `semi-public` or `private`.
    pub kind: String,
    /// Already set up in the service.
    pub configured: bool,
    /// The category IDs it can return (`2000` movies, `5000` TV, `6000` adult).
    pub categories: Vec<u32>,
    /// The site's language as the service names it (`en-US`, `ru-RU`); empty when it says none.
    pub language: String,
}

/// The admin API of one running service.
#[derive(Debug, Clone)]
pub struct AdminClient {
    http: reqwest::Client,
    base: String,
}

impl AdminClient {
    /// A client for the service at `base_url` (`http://127.0.0.1:9117`).
    ///
    /// # Errors
    /// [`AdminError::Unreachable`] when the HTTP client cannot be built.
    pub fn new(base_url: &str) -> Result<Self, AdminError> {
        let http = reqwest::Client::builder()
            .cookie_store(true)
            // The sign-in answers with a redirect; the cookie is what matters.
            .redirect(Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .user_agent(user_agent())
            .no_proxy()
            .build()
            .map_err(|error| AdminError::Unreachable(error.to_string()))?;
        Ok(Self {
            http,
            base: base_url.trim_end_matches('/').to_owned(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Asks the service for a session without a password, the way a browser does. A service that
    /// has no admin password hands one to anyone: `GET /UI/Dashboard` redirects to the sign-in page,
    /// which sets a test cookie and redirects back until the cookie is seen, then issues the
    /// session cookie. The redirects are followed here by hand (the client does not follow them,
    /// because the real sign-in must not) and the session is checked with the next call.
    ///
    /// # Errors
    /// [`AdminError::Unauthorized`] when the service wants a password.
    pub async fn begin_session(&self) -> Result<(), AdminError> {
        let mut url = reqwest::Url::parse(&self.url("/UI/Dashboard"))
            .map_err(|error| AdminError::BadAnswer(error.to_string()))?;
        // The real handshake takes four hops; the bound only stops a service that loops.
        for _ in 0..8 {
            let response = self
                .http
                .get(url.clone())
                .send()
                .await
                .map_err(unreachable)?;
            if !response.status().is_redirection() {
                break;
            }
            let Some(next) = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|location| url.join(location).ok())
            else {
                break;
            };
            // Never follow the redirect off this service.
            if next.host_str() != url.host_str() || next.port() != url.port() {
                break;
            }
            url = next;
        }
        self.server_config().await.map(|_| ())
    }

    /// Signs in with the admin password and checks that the session works.
    ///
    /// # Errors
    /// [`AdminError::Unauthorized`] when the password is not accepted.
    pub async fn login(&self, password: &str) -> Result<(), AdminError> {
        let response = self
            .http
            .post(self.url("/UI/Dashboard"))
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(format!("password={}", form_encode(password)))
            .send()
            .await
            .map_err(unreachable)?;
        // A wrong password answers with a redirect to the sign-in page and sets no cookie; a right
        // one sets the session cookie. The next call tells which.
        drop(response);
        self.server_config().await.map(|_| ())
    }

    /// The service's own settings. Needs a session once a password is set.
    ///
    /// # Errors
    /// [`AdminError::Unauthorized`] when a sign-in is needed.
    pub async fn server_config(&self) -> Result<Value, AdminError> {
        let response = self
            .http
            .get(self.url("/api/v2.0/server/config"))
            .send()
            .await
            .map_err(unreachable)?;
        read_json(response).await
    }

    /// Sets the admin password. The session of the caller may end with it; sign in again.
    ///
    /// # Errors
    /// An [`AdminError`] when the service refuses.
    pub async fn set_admin_password(&self, password: &str) -> Result<(), AdminError> {
        let response = self
            .http
            .post(self.url("/api/v2.0/server/adminpassword"))
            .header(CONTENT_TYPE, "application/json")
            .body(json_body(&password)?)
            .send()
            .await
            .map_err(unreachable)?;
        read_empty(response).await
    }

    /// Every indexer the service knows, configured or not.
    ///
    /// # Errors
    /// An [`AdminError`] when the service does not answer or answers something else.
    pub async fn list_indexers(&self) -> Result<Vec<IndexerInfo>, AdminError> {
        let response = self
            .http
            .get(self.url("/api/v2.0/indexers"))
            .send()
            .await
            .map_err(unreachable)?;
        let value = read_json(response).await?;
        let array = value
            .as_array()
            .ok_or_else(|| AdminError::BadAnswer("the list of indexers is not a list".into()))?;
        Ok(array.iter().filter_map(read_indexer).collect())
    }

    /// The setting fields of one indexer, as the service sends them.
    ///
    /// # Errors
    /// An [`AdminError`].
    pub async fn get_config(&self, id: &str) -> Result<Value, AdminError> {
        let path = indexer_path(id, "config")?;
        let response = self
            .http
            .get(self.url(&path))
            .send()
            .await
            .map_err(unreachable)?;
        read_json(response).await
    }

    /// Saves the setting fields of an indexer. Sent unchanged, a public indexer is set up.
    ///
    /// # Errors
    /// An [`AdminError::Failed`] with the service's own words when it refuses.
    pub async fn post_config(&self, id: &str, config: &Value) -> Result<(), AdminError> {
        let path = indexer_path(id, "config")?;
        let response = self
            .http
            .post(self.url(&path))
            .header(CONTENT_TYPE, "application/json")
            .body(json_body(config)?)
            .send()
            .await
            .map_err(unreachable)?;
        read_empty(response).await
    }

    /// Asks the service to run a search on the indexer and report whether it worked.
    ///
    /// # Errors
    /// An [`AdminError::Failed`] with the service's own words when the test fails.
    pub async fn test(&self, id: &str) -> Result<(), AdminError> {
        let path = indexer_path(id, "test")?;
        let response = self
            .http
            .post(self.url(&path))
            .send()
            .await
            .map_err(unreachable)?;
        read_empty(response).await
    }

    /// Removes an indexer from the service.
    ///
    /// # Errors
    /// An [`AdminError`].
    pub async fn delete(&self, id: &str) -> Result<(), AdminError> {
        let path = indexer_path(id, "")?;
        let response = self
            .http
            .delete(self.url(path.trim_end_matches('/')))
            .send()
            .await
            .map_err(unreachable)?;
        read_empty(response).await
    }
}

/// `/api/v2.0/indexers/<id>/<tail>`, for an id that is a plain name.
fn indexer_path(id: &str, tail: &str) -> Result<String, AdminError> {
    let plain = !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !plain {
        return Err(AdminError::BadAnswer(format!(
            "{id:?} is not an indexer id"
        )));
    }
    Ok(format!("/api/v2.0/indexers/{id}/{tail}"))
}

fn json_body<T: serde::Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, AdminError> {
    serde_json::to_vec(value).map_err(|error| AdminError::BadAnswer(error.to_string()))
}

/// A value for an `application/x-www-form-urlencoded` body.
fn form_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn unreachable(error: reqwest::Error) -> AdminError {
    AdminError::Unreachable(error.without_url().to_string())
}

/// What the service says about a failure: its `error` text, or the body, or the status.
async fn failure(response: reqwest::Response) -> AdminError {
    let status = response.status();
    if matches!(
        status,
        StatusCode::UNAUTHORIZED
            | StatusCode::FORBIDDEN
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
    ) {
        return AdminError::Unauthorized;
    }
    let body = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| body.trim().chars().take(300).collect());
    let message = if message.is_empty() {
        format!("The indexer service answered {status}.")
    } else {
        message
    };
    AdminError::Failed {
        status: status.as_u16(),
        message,
    }
}

async fn read_json(response: reqwest::Response) -> Result<Value, AdminError> {
    if !response.status().is_success() {
        return Err(failure(response).await);
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| AdminError::BadAnswer(error.without_url().to_string()))?;
    serde_json::from_slice(&bytes).map_err(|error| AdminError::BadAnswer(error.to_string()))
}

async fn read_empty(response: reqwest::Response) -> Result<(), AdminError> {
    if response.status().is_success() {
        Ok(())
    } else {
        Err(failure(response).await)
    }
}

#[derive(Deserialize)]
struct RawCategory {
    #[serde(rename = "ID")]
    id: Value,
}

fn read_indexer(value: &Value) -> Option<IndexerInfo> {
    let id = value.get("id")?.as_str()?.to_owned();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_owned();
    let categories = value
        .get("caps")
        .and_then(Value::as_array)
        .map(|caps| {
            caps.iter()
                .filter_map(|cap| serde_json::from_value::<RawCategory>(cap.clone()).ok())
                .filter_map(|cap| match cap.id {
                    Value::String(text) => text.trim().parse().ok(),
                    Value::Number(number) => number.as_u64().and_then(|n| u32::try_from(n).ok()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Some(IndexerInfo {
        id,
        name,
        description,
        kind: value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        configured: value
            .get("configured")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        categories,
        language: value
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_indexer_is_read_with_string_or_number_category_ids() {
        let info = read_indexer(&json!({
            "id": "yts", "name": "YTS", "type": "public", "configured": true,
            "caps": [ { "ID": "2000", "Name": "Movies" }, { "ID": 2040 }, { "ID": "x" }, {} ]
        }))
        .expect("indexer");
        assert_eq!(info.id, "yts");
        assert_eq!(info.kind, "public");
        assert!(info.configured);
        assert_eq!(info.categories, [2000, 2040]);
        assert!(read_indexer(&json!({ "name": "no id" })).is_none());
        let bare = read_indexer(&json!({ "id": "a" })).expect("bare");
        assert_eq!((bare.name.as_str(), bare.configured), ("a", false));
        assert!(bare.categories.is_empty());
    }

    #[test]
    fn form_values_are_percent_encoded() {
        assert_eq!(form_encode("abc-XYZ_0.9~"), "abc-XYZ_0.9~");
        assert_eq!(form_encode("a b&c=d+é"), "a%20b%26c%3Dd%2B%C3%A9");
    }

    #[test]
    fn only_plain_ids_make_a_path() {
        assert_eq!(
            indexer_path("1337x", "test").as_deref(),
            Ok("/api/v2.0/indexers/1337x/test")
        );
        for bad in ["", "a/b", "a b", "../x", "a?b", "a#b"] {
            assert!(indexer_path(bad, "test").is_err(), "{bad:?}");
        }
    }
}

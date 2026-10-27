//! Errors shared by every provider.

/// Why a provider call did not produce a result.
///
/// The messages are plain sentences meant for the user. They never contain credential values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    /// A required credential has no value from the user.
    #[error(
        "{provider} is inactive: enter your {}. Get {} at {registration_url}",
        field_list(fields),
        if fields.len() > 1 { "them" } else { "one" }
    )]
    MissingCredentials {
        /// Display name of the provider.
        provider: String,
        /// Keys of the required fields that have no value.
        fields: Vec<String>,
        registration_url: String,
    },
    /// The provider rejected the credentials.
    #[error("{provider} rejected the credentials: {message}")]
    Auth { provider: String, message: String },
    /// The provider is limiting requests.
    #[error("{provider} is limiting requests; try again in {retry_after_secs} seconds")]
    RateLimited {
        provider: String,
        retry_after_secs: u32,
    },
    /// Nothing matches the ID or query.
    #[error("{0} has no match")]
    NotFound(String),
    /// A website-backed provider could not read the page (best-effort providers only).
    #[error("{0}: the provider page format changed")]
    FormatChanged(String),
    /// The provider could not be reached or answered with an error.
    #[error("{provider} request failed: {message}")]
    Request { provider: String, message: String },
    /// The provider does not offer this operation.
    #[error("{provider} does not support {operation}")]
    Unsupported {
        provider: String,
        operation: &'static str,
    },
    /// Anything else, with a message for the user.
    #[error("{0}")]
    Other(String),
    /// The user's own key was rejected or rate limited. Wraps the original error and carries
    /// the provider key, so Settings can link the message to that provider's card.
    #[error("{}", key_problem_message(.cause))]
    KeyProblem {
        cause: Box<ProviderError>,
        /// Catalogue key of the provider, for example `tmdb`.
        provider_key: String,
    },
}

/// The name of a credential field in a sentence, as the Providers page labels it.
fn field_label(key: &str) -> String {
    match key {
        "api_key" => "API key".to_owned(),
        "app_key" => "app key".to_owned(),
        "pin" => "subscriber PIN".to_owned(),
        "username" => "account name".to_owned(),
        "client_id" => "Client ID".to_owned(),
        other => other.replace('_', " "),
    }
}

/// The missing fields as a list in a sentence: `API key`, `account name and password`.
fn field_list(fields: &[String]) -> String {
    let labels: Vec<String> = fields.iter().map(|key| field_label(key)).collect();
    match labels.as_slice() {
        [] => "credentials".to_owned(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn key_problem_message(cause: &ProviderError) -> String {
    match cause.root() {
        ProviderError::Auth { provider, message } => format!(
            "{provider} rejected your key: {}. Check it in Settings, Scraping and providers.",
            message.trim_end_matches('.')
        ),
        ProviderError::RateLimited {
            provider,
            retry_after_secs,
        } => format!(
            "{provider} is limiting requests for your key; try again in {retry_after_secs} seconds. See your key's limits in Settings, Scraping and providers."
        ),
        other => other.to_string(),
    }
}

/// The words of a request error whose site answered with a human check (HTTP 202 or 403).
pub const HUMAN_CHECK: &str = "asked for a human check";

impl ProviderError {
    /// Whether the site asked for a human check: nothing a program does passes it, so the
    /// interface says "unavailable right now" instead of showing a failure.
    #[must_use]
    pub fn is_human_check(&self) -> bool {
        matches!(self.root(), Self::Request { message, .. } if message.contains(HUMAN_CHECK))
    }

    /// The error without the key-problem wrapper, for code that classifies errors.
    pub fn root(&self) -> &Self {
        match self {
            Self::KeyProblem { cause, .. } => cause.root(),
            other => other,
        }
    }

    /// The catalogue key of the provider whose card Settings should link to, when this error is
    /// a rejected or rate-limited user key.
    pub fn key_problem_provider(&self) -> Option<&str> {
        match self {
            Self::KeyProblem { provider_key, .. } => Some(provider_key),
            _ => None,
        }
    }

    /// Marks a rejection or a rate limit as a problem with the user's own key for
    /// `provider_key`, so the message says so and the UI can link to the provider's card. A
    /// rejection is also remembered on `credentials`' rejection log, so Settings can show the
    /// provider as Rejected. Other errors pass through unchanged.
    pub fn with_key_hint(
        self,
        credentials: &crate::credentials::ResolvedCredentials,
        provider_key: &str,
    ) -> Self {
        match self {
            Self::Auth { .. } => {
                credentials.note_rejected(provider_key);
                Self::KeyProblem {
                    cause: Box::new(self),
                    provider_key: provider_key.to_owned(),
                }
            }
            Self::RateLimited { .. } => Self::KeyProblem {
                cause: Box::new(self),
                provider_key: provider_key.to_owned(),
            },
            other => other,
        }
    }
}

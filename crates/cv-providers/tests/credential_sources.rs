//! A mock provider exercised with the user's key and without one. CineVault ships no keys, so a
//! provider with nothing saved makes no request. The "server" accepts exactly one key, so each
//! case proves which key the provider actually sent.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cv_domain::{Item, ProviderIds};
use cv_providers::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use cv_providers::{
    CredentialError, CredentialResolver, CredentialSchema, CredentialState, MemoryUserCredentials,
    MetadataProvider, Provider, ProviderError, ProviderRegistry, ResolvedCredentials, catalogue,
};

const USER_KEY: &str = "user-key-bbbb";
const OTHER_KEY: &str = "other-key-cccc";

/// Stands in for TMDB: answers a search only when the request carries `valid_key`.
struct MockProvider {
    resolver: CredentialResolver,
    valid_key: &'static str,
    /// The key of every request that reached the "server".
    seen: Mutex<Vec<String>>,
}

impl MockProvider {
    fn server(&self, credentials: &ResolvedCredentials) -> Result<(), ProviderError> {
        let key = credentials
            .get(catalogue::API_KEY)
            .map(|v| v.expose().to_owned())
            .unwrap_or_default();
        self.seen.lock().unwrap().push(key.clone());
        if key == self.valid_key {
            Ok(())
        } else {
            Err(ProviderError::Auth {
                provider: "TMDB".to_owned(),
                message: "Invalid API key".to_owned(),
            })
        }
    }
}

fn tmdb_schema() -> &'static CredentialSchema {
    catalogue::CREDENTIAL_SCHEMAS
        .iter()
        .find(|s| s.provider == catalogue::key::TMDB)
        .expect("tmdb schema")
}

#[async_trait]
impl Provider for MockProvider {
    fn id(&self) -> &'static str {
        catalogue::key::TMDB
    }

    fn name(&self) -> &'static str {
        "TMDB"
    }

    fn credentials(&self) -> Option<&'static CredentialSchema> {
        Some(tmdb_schema())
    }

    async fn check_credentials(
        &self,
        credentials: &ResolvedCredentials,
    ) -> Result<(), ProviderError> {
        self.server(credentials)
    }
}

#[async_trait]
impl MetadataProvider for MockProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: true,
            orderings: Vec::new(),
            lookup_ids: vec!["tmdb", "imdb"],
            best_effort: false,
        }
    }

    async fn search(
        &self,
        query: &SearchQuery,
        _options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        // Resolved per request, so a key changed in Settings applies to the next call.
        let credentials = self.resolver.require(self.id())?;
        self.server(&credentials)?;
        Ok(vec![SearchHit {
            provider: self.id().to_owned(),
            title: query.title.clone(),
            ..SearchHit::default()
        }])
    }

    async fn lookup(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        Ok(None)
    }

    async fn details(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        Err(ProviderError::NotFound("TMDB".to_owned()))
    }
}

fn setup(server_accepts: &'static str) -> Arc<MockProvider> {
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    Arc::new(MockProvider {
        resolver,
        valid_key: server_accepts,
        seen: Mutex::new(Vec::new()),
    })
}

fn query() -> SearchQuery {
    SearchQuery {
        kind: MediaKind::Movie,
        title: "Heat".to_owned(),
        year: Some(1995),
    }
}

fn last_key(provider: &MockProvider) -> String {
    provider
        .seen
        .lock()
        .unwrap()
        .last()
        .cloned()
        .unwrap_or_default()
}

#[tokio::test]
async fn the_users_key_is_the_one_sent_and_a_changed_key_applies_to_the_next_request() {
    let provider = setup(USER_KEY);
    let options = RequestOptions::default();

    provider
        .resolver
        .set_user("tmdb", catalogue::API_KEY, OTHER_KEY)
        .unwrap();
    let error = provider.search(&query(), &options).await.unwrap_err();
    assert!(matches!(error, ProviderError::Auth { .. }), "{error:?}");
    assert_eq!(last_key(&provider), OTHER_KEY);

    provider
        .resolver
        .set_user("tmdb", catalogue::API_KEY, USER_KEY)
        .unwrap();
    let status = provider.resolver.status("tmdb").unwrap();
    assert_eq!(status.state, CredentialState::Active);
    let hits = provider.search(&query(), &options).await.unwrap();
    assert_eq!(hits[0].title, "Heat");
    assert_eq!(last_key(&provider), USER_KEY);
}

#[tokio::test]
async fn removing_the_key_stops_requests() {
    let provider = setup(USER_KEY);
    let options = RequestOptions::default();
    provider
        .resolver
        .set_user("tmdb", catalogue::API_KEY, USER_KEY)
        .unwrap();
    provider.search(&query(), &options).await.unwrap();

    assert!(
        provider
            .resolver
            .clear_user("tmdb", catalogue::API_KEY)
            .unwrap()
    );
    let seen = provider.seen.lock().unwrap().len();
    assert!(matches!(
        provider.search(&query(), &options).await,
        Err(ProviderError::MissingCredentials { .. })
    ));
    assert_eq!(provider.seen.lock().unwrap().len(), seen, "no new request");
    assert_eq!(
        provider.resolver.status("tmdb").unwrap().state,
        CredentialState::Missing
    );
}

#[tokio::test]
async fn with_no_key_at_all_the_provider_is_inactive_and_makes_no_request() {
    let provider = setup(USER_KEY);
    let status = provider.resolver.status("tmdb").unwrap();
    assert_eq!(status.state, CredentialState::Missing);
    assert!(!status.active);

    let error = provider
        .search(&query(), &RequestOptions::default())
        .await
        .unwrap_err();
    let ProviderError::MissingCredentials {
        registration_url, ..
    } = &error
    else {
        panic!("expected MissingCredentials, got {error:?}");
    };
    assert_eq!(registration_url, tmdb_schema().registration_url);
    assert!(
        provider.seen.lock().unwrap().is_empty(),
        "no request without a key"
    );
}

#[tokio::test]
async fn test_button_checks_a_typed_key_without_saving_it() {
    let provider = setup(USER_KEY);
    provider
        .resolver
        .set_user("tmdb", catalogue::API_KEY, OTHER_KEY)
        .unwrap();
    let mut registry = ProviderRegistry::new();
    registry.register(provider.clone());

    let draft =
        ResolvedCredentials::from_user_values(tmdb_schema(), [(catalogue::API_KEY, USER_KEY)])
            .unwrap();
    registry
        .check_credentials("tmdb", "TMDB", &draft)
        .await
        .unwrap();
    assert_eq!(last_key(&provider), USER_KEY);
    // The draft was only tested; the saved key is unchanged and still the one in use.
    let saved = provider.resolver.resolve("tmdb").unwrap();
    assert_eq!(saved.get(catalogue::API_KEY).unwrap().expose(), OTHER_KEY);

    let wrong =
        ResolvedCredentials::from_user_values(tmdb_schema(), [(catalogue::API_KEY, "wrong")])
            .unwrap();
    assert!(matches!(
        registry.check_credentials("tmdb", "TMDB", &wrong).await,
        Err(ProviderError::Auth { .. })
    ));
    assert!(matches!(
        registry
            .check_credentials("omdb", "OMDb", &ResolvedCredentials::default())
            .await,
        Err(ProviderError::Unsupported { .. })
    ));
}

#[test]
fn unknown_providers_are_refused() {
    let provider = setup(USER_KEY);
    assert!(matches!(
        provider.resolver.status("nowhere"),
        Err(CredentialError::UnknownProvider(_))
    ));
}

use async_trait::async_trait;
use cv_providers::catalogue::{API_KEY, key};
use cv_providers::traits::Provider;
use cv_providers::{CredentialState, Importance};

use super::*;

const MINE: &str = "my-own-key-2";

/// Accepts exactly one key.
struct FakeTmdb;

#[async_trait]
impl Provider for FakeTmdb {
    fn id(&self) -> &'static str {
        key::TMDB
    }

    fn name(&self) -> &'static str {
        "TMDB"
    }

    async fn check_credentials(
        &self,
        credentials: &ResolvedCredentials,
    ) -> Result<(), ProviderError> {
        if credentials.get(API_KEY).map(CredentialValue::expose) == Some(MINE) {
            Ok(())
        } else {
            Err(ProviderError::Auth {
                provider: "TMDB".to_owned(),
                message: "Invalid API key".to_owned(),
            })
        }
    }
}

fn service() -> (ProviderCredentialsService, Arc<SecretStore>) {
    let store = Arc::new(SecretStore::in_memory());
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(FakeTmdb));
    (
        ProviderCredentialsService::new(Arc::clone(&store), registry),
        store,
    )
}

fn tmdb(service: &ProviderCredentialsService) -> CredentialStatus {
    service
        .list()
        .unwrap()
        .into_iter()
        .find(|s| s.provider == key::TMDB)
        .unwrap()
}

#[test]
fn the_users_key_lives_in_the_secret_store_and_remove_deletes_it() {
    let (service, store) = service();
    assert_eq!(tmdb(&service).state, CredentialState::Missing);

    service.set(key::TMDB, API_KEY, MINE).unwrap();
    assert!(store.contains("tmdb.api_key").unwrap());
    assert_eq!(tmdb(&service).state, CredentialState::Active);

    service.remove(key::TMDB).unwrap();
    assert!(!store.contains("tmdb.api_key").unwrap());
    assert_eq!(tmdb(&service).state, CredentialState::Missing);
}

#[test]
fn with_a_fresh_store_every_provider_is_missing_and_the_importance_is_declared() {
    let (service, _) = service();
    let all = service.list().unwrap();
    assert_eq!(all.len(), cv_providers::catalogue::CREDENTIAL_SCHEMAS.len());
    for status in &all {
        assert_eq!(
            status.state,
            CredentialState::Missing,
            "{}",
            status.provider
        );
        assert!(!status.configured, "{}", status.provider);
    }
    let importance = |provider: &str| {
        all.iter()
            .find(|s| s.provider == provider)
            .map(|s| s.importance)
    };
    assert_eq!(importance(key::TMDB), Some(Importance::Required));
    for provider in [key::OMDB, key::TVDB, key::FANART] {
        assert_eq!(importance(provider), Some(Importance::Recommended));
    }
    assert_eq!(importance(key::MDBLIST), Some(Importance::Optional));
}

#[test]
fn clearing_one_field_and_bad_input() {
    let (service, store) = service();
    service.set(key::OMDB, API_KEY, MINE).unwrap();
    assert!(store.contains("omdb.api_key").unwrap());
    service.clear(key::OMDB, API_KEY).unwrap();
    assert!(!store.contains("omdb.api_key").unwrap());
    assert!(service.set(key::OMDB, API_KEY, "  ").is_err());
    assert!(service.set("nowhere", API_KEY, MINE).is_err());
}

#[test]
fn the_banner_lists_missing_required_and_recommended_keys_but_never_optional_ones() {
    let (service, _) = service();
    let attention = service.attention().unwrap();
    let names = |entries: &[AttentionEntry]| -> Vec<&str> {
        entries.iter().map(|entry| entry.provider).collect()
    };
    assert_eq!(names(&attention.required), [key::TMDB]);
    assert_eq!(
        names(&attention.recommended),
        [key::OMDB, key::TVDB, key::FANART]
    );
    assert!(!attention.is_empty());

    service.set(key::TMDB, API_KEY, MINE).unwrap();
    service.set(key::TVDB, API_KEY, "tvdb-key").unwrap();
    let attention = service.attention().unwrap();
    assert!(attention.required.is_empty());
    assert_eq!(names(&attention.recommended), [key::OMDB, key::FANART]);
}

#[test]
fn online_identification_is_paused_while_the_registered_tmdb_has_no_key() {
    let (service, _) = service();
    assert!(service.identification_paused());
    service.set(key::TMDB, API_KEY, MINE).unwrap();
    assert!(!service.identification_paused());
    service.remove(key::TMDB).unwrap();
    assert!(service.identification_paused());

    // A build whose registry has no TMDB provider (a test with a scripted one) is not paused.
    let store = Arc::new(SecretStore::in_memory());
    let service = ProviderCredentialsService::new(store, ProviderRegistry::new());
    assert!(!service.identification_paused());
}

#[tokio::test]
async fn test_reports_the_providers_answer_for_a_typed_key_without_saving_it() {
    let (service, store) = service();
    let typed = BTreeMap::from([(API_KEY.to_owned(), MINE.to_owned())]);
    service.test(key::TMDB, &typed).await.unwrap();
    assert!(
        !store.contains("tmdb.api_key").unwrap(),
        "testing does not save"
    );

    let wrong = BTreeMap::from([(API_KEY.to_owned(), "nope".to_owned())]);
    let error = service.test(key::TMDB, &wrong).await.unwrap_err();
    assert!(matches!(
        error,
        TestError::Provider(ProviderError::Auth { .. })
    ));
    assert_eq!(
        tmdb(&service).state,
        CredentialState::Missing,
        "a typed key that fails says nothing about the saved one"
    );
}

#[tokio::test]
async fn a_failed_test_of_the_saved_key_marks_it_rejected_until_it_is_replaced() {
    let (service, _) = service();
    service.set(key::TMDB, API_KEY, "stale-key").unwrap();
    assert_eq!(tmdb(&service).state, CredentialState::Active);

    let error = service.test(key::TMDB, &BTreeMap::new()).await.unwrap_err();
    assert!(matches!(
        error,
        TestError::Provider(ProviderError::Auth { .. })
    ));
    assert_eq!(tmdb(&service).state, CredentialState::Rejected);

    service.set(key::TMDB, API_KEY, MINE).unwrap();
    assert_eq!(tmdb(&service).state, CredentialState::Active);
    service.test(key::TMDB, &BTreeMap::new()).await.unwrap();
    assert_eq!(tmdb(&service).state, CredentialState::Active);
}

#[tokio::test]
async fn test_names_the_fields_that_are_actually_missing() {
    let (service, _) = service();
    let only = |field: &str, value: &str| BTreeMap::from([(field.to_owned(), value.to_owned())]);
    for (provider, typed, expected) in [
        (
            key::MPDB,
            only(API_KEY, "a-key"),
            "enter your account name and subscription key",
        ),
        (key::TMDB, BTreeMap::new(), "enter your API key. Get one at"),
    ] {
        let error = service.test(provider, &typed).await.unwrap_err();
        let text = error.to_string();
        assert!(text.contains(expected), "{provider}: {text}");
        if provider != key::TMDB {
            assert!(!text.contains("API key is required"), "{provider}: {text}");
        }
    }
}

#[tokio::test]
async fn test_without_a_key_says_which_is_missing_and_unlisted_providers_are_unsupported() {
    let (service, _) = service();
    let error = service.test(key::TMDB, &BTreeMap::new()).await.unwrap_err();
    assert!(matches!(
        error,
        TestError::Provider(ProviderError::MissingCredentials { .. })
    ));

    let typed = BTreeMap::from([(API_KEY.to_owned(), MINE.to_owned())]);
    let error = service.test(key::OMDB, &typed).await.unwrap_err();
    assert!(matches!(
        error,
        TestError::Provider(ProviderError::Unsupported { .. })
    ));
}

#[test]
fn undo_of_remove_puts_the_removed_credentials_back() {
    let (service, store) = service();
    service.set(key::TMDB, API_KEY, MINE).unwrap();
    service.remove(key::TMDB).unwrap();
    assert!(!store.contains("tmdb.api_key").unwrap());
    assert_eq!(tmdb(&service).state, CredentialState::Missing);

    assert!(service.restore(key::TMDB).unwrap());
    assert_eq!(tmdb(&service).state, CredentialState::Active);
    assert_eq!(
        service
            .resolver()
            .resolve(key::TMDB)
            .unwrap()
            .get(API_KEY)
            .map(CredentialValue::expose),
        Some(MINE)
    );
    // Undo works once; there is nothing left to put back.
    assert!(!service.restore(key::TMDB).unwrap());
}

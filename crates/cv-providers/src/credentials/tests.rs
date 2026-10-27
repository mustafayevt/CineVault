use super::*;
use crate::catalogue::{API_KEY, key};

const USER_SECRET: &str = "user-secret-value-4567";

fn resolver() -> CredentialResolver {
    CredentialResolver::new(Arc::new(MemoryUserCredentials::default()))
}

#[test]
fn a_provider_without_a_key_is_inactive_and_names_what_is_missing() {
    let resolver = resolver();
    let status = resolver.status(key::OMDB).unwrap();
    assert!(!status.active && !status.configured);
    assert_eq!(status.state, CredentialState::Missing);
    assert_eq!(status.missing, vec![API_KEY]);
    assert!(status.fields.iter().all(|field| !field.present));
    match resolver.require(key::OMDB) {
        Err(ProviderError::MissingCredentials {
            provider,
            fields,
            registration_url,
        }) => {
            assert_eq!(provider, "OMDb");
            assert_eq!(fields, vec![API_KEY.to_owned()]);
            assert!(registration_url.starts_with("https://"));
        }
        other => panic!("expected MissingCredentials, got {other:?}"),
    }
}

#[test]
fn a_fresh_resolver_resolves_nothing_for_any_provider() {
    let resolver = resolver();
    for schema in resolver.schemas() {
        let resolved = resolver.resolve(schema.provider).unwrap();
        for field in schema.fields {
            assert!(
                !resolved.has(field.key),
                "{}.{}",
                schema.provider,
                field.key
            );
        }
        let status = resolver.status(schema.provider).unwrap();
        assert_eq!(
            status.state,
            CredentialState::Missing,
            "{}",
            schema.provider
        );
    }
}

#[test]
fn the_users_key_makes_a_provider_active_and_removing_it_makes_it_missing_again() {
    let resolver = resolver();
    resolver.set_user(key::TMDB, API_KEY, USER_SECRET).unwrap();
    let status = resolver.status(key::TMDB).unwrap();
    assert!(status.active && status.configured);
    assert_eq!(status.state, CredentialState::Active);
    assert_eq!(status.importance, Importance::Required);
    assert_eq!(
        resolver
            .require(key::TMDB)
            .unwrap()
            .get(API_KEY)
            .unwrap()
            .expose(),
        USER_SECRET
    );

    assert!(resolver.clear_user(key::TMDB, API_KEY).unwrap());
    assert!(!resolver.clear_user(key::TMDB, API_KEY).unwrap());
    let status = resolver.status(key::TMDB).unwrap();
    assert_eq!(status.state, CredentialState::Missing);
    assert!(resolver.require(key::TMDB).is_err());
}

#[test]
fn a_rejected_request_marks_the_provider_rejected_until_the_key_changes() {
    let resolver = resolver();
    resolver.set_user(key::TMDB, API_KEY, USER_SECRET).unwrap();
    let resolved = resolver.require(key::TMDB).unwrap();
    let error = ProviderError::Auth {
        provider: "TMDB".to_owned(),
        message: "Invalid API key".to_owned(),
    }
    .with_key_hint(&resolved, key::TMDB);

    assert_eq!(error.key_problem_provider(), Some(key::TMDB));
    assert!(matches!(error.root(), ProviderError::Auth { .. }));
    assert!(error.to_string().contains("rejected your key"), "{error}");
    assert_eq!(
        resolver.status(key::TMDB).unwrap().state,
        CredentialState::Rejected
    );

    // A new key starts clean; so does a successful test of the saved one.
    resolver
        .set_user(key::TMDB, API_KEY, "another-key")
        .unwrap();
    assert_eq!(
        resolver.status(key::TMDB).unwrap().state,
        CredentialState::Active
    );
    resolver.note_rejected(key::TMDB);
    resolver.note_accepted(key::TMDB);
    assert_eq!(
        resolver.status(key::TMDB).unwrap().state,
        CredentialState::Active
    );
}

#[test]
fn a_rate_limit_links_to_the_card_but_does_not_mark_the_key_rejected() {
    let resolver = resolver();
    resolver.set_user(key::TMDB, API_KEY, USER_SECRET).unwrap();
    let resolved = resolver.require(key::TMDB).unwrap();
    let error = ProviderError::RateLimited {
        provider: "TMDB".to_owned(),
        retry_after_secs: 12,
    }
    .with_key_hint(&resolved, key::TMDB);
    assert_eq!(error.key_problem_provider(), Some(key::TMDB));
    assert!(error.to_string().contains("limiting requests for your key"));
    assert!(error.to_string().contains("12 seconds"));
    assert_eq!(
        resolver.status(key::TMDB).unwrap().state,
        CredentialState::Active
    );
}

#[test]
fn other_errors_pass_through_without_a_card_link() {
    let resolved = ResolvedCredentials::default();
    let error = ProviderError::NotFound("TMDB".to_owned()).with_key_hint(&resolved, key::TMDB);
    assert_eq!(error, ProviderError::NotFound("TMDB".to_owned()));
    assert_eq!(error.key_problem_provider(), None);
}

#[test]
fn removing_a_provider_deletes_every_field_it_holds() {
    let resolver = resolver();
    for field in ["api_key", "username", "subscription_key"] {
        resolver.set_user(key::MPDB, field, "value").unwrap();
    }
    assert!(resolver.status(key::MPDB).unwrap().active);
    assert!(resolver.remove_all(key::MPDB).unwrap());
    let status = resolver.status(key::MPDB).unwrap();
    assert!(!status.active && !status.configured);
    assert!(!resolver.remove_all(key::MPDB).unwrap());
}

#[test]
fn a_partly_filled_provider_is_missing_and_lists_the_rest() {
    let resolver = resolver();
    resolver.set_user(key::MPDB, "api_key", "app").unwrap();
    let status = resolver.status(key::MPDB).unwrap();
    assert!(status.configured && !status.active);
    assert_eq!(status.state, CredentialState::Missing);
    assert_eq!(status.missing, vec!["username", "subscription_key"]);
}

#[test]
fn needing_attention_lists_the_providers_of_an_importance_without_a_working_key() {
    let resolver = resolver();
    let names = |importance| -> Vec<&str> {
        resolver
            .needing_attention(importance)
            .unwrap()
            .into_iter()
            .map(|schema| schema.provider)
            .collect()
    };
    assert_eq!(names(Importance::Required), [key::TMDB]);
    assert_eq!(
        names(Importance::Recommended),
        [key::OMDB, key::TVDB, key::FANART]
    );
    resolver.set_user(key::TMDB, API_KEY, USER_SECRET).unwrap();
    assert!(names(Importance::Required).is_empty());
}

#[test]
fn status_and_resolved_credentials_never_show_a_value() {
    let resolver = resolver();
    resolver.set_user(key::OMDB, API_KEY, USER_SECRET).unwrap();

    let statuses = serde_json::to_string(&resolver.statuses().unwrap()).unwrap();
    let debug = format!(
        "{:?} {:?}",
        resolver.statuses().unwrap(),
        resolver.resolve(key::OMDB).unwrap()
    );
    for text in [statuses, debug] {
        assert!(!text.contains(USER_SECRET), "{text}");
    }
    let value = CredentialValue::new(USER_SECRET);
    assert_eq!(
        format!("{value} {value:?}"),
        "******** CredentialValue(********)"
    );
    assert!(!format!("{resolver:?}").contains(USER_SECRET));
}

#[test]
fn bad_requests_are_refused() {
    let resolver = resolver();
    assert!(matches!(
        resolver.status("nope"),
        Err(CredentialError::UnknownProvider(_))
    ));
    assert!(matches!(
        resolver.set_user(key::TMDB, "token", "x"),
        Err(CredentialError::UnknownField { .. })
    ));
    assert!(matches!(
        resolver.set_user(key::TMDB, API_KEY, "   "),
        Err(CredentialError::Invalid(_))
    ));
}

#[test]
fn a_draft_value_layers_over_the_saved_one_and_leaves_the_rejection_log_alone() {
    let resolver = resolver();
    resolver.set_user(key::TMDB, API_KEY, USER_SECRET).unwrap();
    let schema = resolver.schema(key::TMDB).unwrap();
    let draft =
        ResolvedCredentials::from_user_values(schema, [(API_KEY, "  typed-key  ")]).unwrap();
    let layered = draft.over(&resolver.resolve(key::TMDB).unwrap());
    assert_eq!(layered.get(API_KEY).unwrap().expose(), "typed-key");

    // A typed key that is turned down says nothing about the saved one.
    ProviderError::Auth {
        provider: "TMDB".to_owned(),
        message: "no".to_owned(),
    }
    .with_key_hint(&layered, key::TMDB);
    assert_eq!(
        resolver.status(key::TMDB).unwrap().state,
        CredentialState::Active
    );

    let empty = ResolvedCredentials::from_user_values(schema, [(API_KEY, " ")]).unwrap();
    let layered = empty.over(&resolver.resolve(key::TMDB).unwrap());
    assert_eq!(layered.get(API_KEY).unwrap().expose(), USER_SECRET);
    assert!(ResolvedCredentials::from_user_values(schema, [("bogus", "x")]).is_err());
}

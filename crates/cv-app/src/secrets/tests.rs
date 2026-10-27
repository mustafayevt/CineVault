use super::*;

#[test]
fn memory_store_round_trips_replaces_and_deletes() {
    let store = SecretStore::in_memory();
    assert!(store.get("tmdb.api_key").unwrap().is_none());
    assert!(!store.contains("tmdb.api_key").unwrap());

    store.set("tmdb.api_key", "mem-key-0001").unwrap();
    assert_eq!(
        store.get("tmdb.api_key").unwrap().unwrap().expose(),
        "mem-key-0001"
    );

    store.set("tmdb.api_key", "mem-key-0002").unwrap();
    assert_eq!(
        store.get("tmdb.api_key").unwrap().unwrap().expose(),
        "mem-key-0002"
    );

    assert!(store.delete("tmdb.api_key").unwrap());
    assert!(!store.delete("tmdb.api_key").unwrap());
    assert!(store.get("tmdb.api_key").unwrap().is_none());
}

#[test]
fn rejects_empty_account_and_empty_value() {
    let store = SecretStore::in_memory();
    assert!(matches!(
        store.set(" ", "value-123456"),
        Err(SecretError::Invalid(_))
    ));
    assert!(matches!(store.set("a", ""), Err(SecretError::Invalid(_))));
    assert!(matches!(store.get(""), Err(SecretError::Invalid(_))));
}

#[test]
fn secret_never_shows_in_debug_or_display() {
    let secret = Secret::new("debug-leak-check-99");
    assert!(!format!("{secret:?}").contains("debug-leak"));
    assert!(!format!("{secret}").contains("debug-leak"));

    #[derive(Debug)]
    struct Holder {
        _key: Secret,
    }
    let holder = Holder { _key: secret };
    assert!(!format!("{holder:?}").contains("debug-leak"));
}

#[test]
fn stored_values_are_registered_for_redaction_until_deleted() {
    let store = SecretStore::in_memory();
    store.set("trakt.token", "registered-token-abc123").unwrap();
    assert_eq!(
        redact::redact("GET /x/registered-token-abc123/y"),
        "GET /x/[REDACTED]/y"
    );

    store.delete("trakt.token").unwrap();
    assert!(redact::redact("GET /x/registered-token-abc123/y").contains("registered-token"));
}

mod per_location {
    use std::fs;

    use crate::data_location::{DataLocation, LocationInputs};

    use super::super::*;

    fn location(root: &std::path::Path, portable: bool) -> DataLocation {
        let exe_dir = root.join("exe");
        fs::create_dir_all(&exe_dir).unwrap();
        if portable {
            fs::write(exe_dir.join("portable"), "").unwrap();
        }
        DataLocation::resolve(&LocationInputs {
            exe_dir,
            user_data_base: root.join("appdata"),
            user_config_base: root.join("config"),
        })
    }

    #[test]
    fn an_installed_copy_uses_the_shared_service() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(service_name_for(&location(tmp.path(), false)), SERVICE_NAME);
    }

    #[test]
    fn a_portable_copy_has_a_service_of_its_own_that_survives_a_move() {
        let tmp = tempfile::tempdir().unwrap();
        let first = location(&tmp.path().join("a"), true);
        let other = location(&tmp.path().join("b"), true);
        let name = service_name_for(&first);
        assert!(name.starts_with("CineVault portable "), "{name}");
        assert_ne!(name, SERVICE_NAME);
        // Stable for the copy, different from another portable copy.
        assert_eq!(service_name_for(&first), name);
        assert_ne!(service_name_for(&other), name);
        // The id lives in the data folder, so a copy moved elsewhere keeps its keys.
        let id = fs::read_to_string(first.root().join(PORTABLE_SECRETS_ID_FILE)).unwrap();
        assert!(name.ends_with(id.trim()));
        let moved = location(&tmp.path().join("moved"), true);
        fs::create_dir_all(moved.root()).unwrap();
        fs::copy(
            first.root().join(PORTABLE_SECRETS_ID_FILE),
            moved.root().join(PORTABLE_SECRETS_ID_FILE),
        )
        .unwrap();
        assert_eq!(service_name_for(&moved), name);
    }
}

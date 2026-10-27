//! The providers that need credentials, and the credential schema each one declares.
//!
//! Settings renders these schemas; nothing here holds a value.

use crate::credentials::{CredentialField, CredentialSchema, FieldKind, Importance};

/// Provider keys, as used in the credential store (`<key>.<field>`) and in Settings.
pub mod key {
    pub const TMDB: &str = "tmdb";
    pub const TVDB: &str = "tvdb";
    pub const OMDB: &str = "omdb";
    pub const FANART: &str = "fanart";
    pub const MOVIEMETER: &str = "moviemeter";
    pub const MDBLIST: &str = "mdblist";
    pub const ANIDB: &str = "anidb";
    pub const TRAKT: &str = "trakt";
    pub const MPDB: &str = "mpdb";
    pub const THESPORTSDB: &str = "thesportsdb";
}

/// The field name of a provider's single API key.
pub const API_KEY: &str = "api_key";

/// The field name of TheTVDB's optional subscriber PIN.
pub const TVDB_PIN: &str = "pin";

/// The field name of AniDB's optional client version (the number registered with the client
/// name; `1` when empty).
pub const ANIDB_CLIENT_VERSION: &str = "client_version";

/// The field name of the MPDb.tv account name.
pub const MPDB_USERNAME: &str = "username";

/// The field name of the MPDb.tv subscription key.
pub const MPDB_SUBSCRIPTION_KEY: &str = "subscription_key";

const API_KEY_ONLY: &[CredentialField] = &[CredentialField {
    key: API_KEY,
    kind: FieldKind::Secret,
    required: true,
}];

// The PIN belongs to a user's own TheTVDB subscription and goes with their key.
const TVDB_FIELDS: &[CredentialField] = &[
    CredentialField {
        key: API_KEY,
        kind: FieldKind::Secret,
        required: true,
    },
    CredentialField {
        key: TVDB_PIN,
        kind: FieldKind::Secret,
        required: false,
    },
];

// AniDB identifies an app by the client name the user registered (the `api_key` field) and a
// version number.
const ANIDB_FIELDS: &[CredentialField] = &[
    CredentialField {
        key: API_KEY,
        kind: FieldKind::Secret,
        required: true,
    },
    CredentialField {
        key: ANIDB_CLIENT_VERSION,
        kind: FieldKind::Text,
        required: false,
    },
];

// MPDb.tv is a private service: every call needs the app key, the account name and the
// subscription key of the account.
const MPDB_FIELDS: &[CredentialField] = &[
    CredentialField {
        key: API_KEY,
        kind: FieldKind::Secret,
        required: true,
    },
    CredentialField {
        key: MPDB_USERNAME,
        kind: FieldKind::Text,
        required: true,
    },
    CredentialField {
        key: MPDB_SUBSCRIPTION_KEY,
        kind: FieldKind::Secret,
        required: true,
    },
];

// TheSportsDB works with its public test key. A supporter's own key lifts the free limits.
const THESPORTSDB_FIELDS: &[CredentialField] = &[CredentialField {
    key: API_KEY,
    kind: FieldKind::Secret,
    required: false,
}];

const TRAKT_FIELDS: &[CredentialField] = &[CredentialField {
    key: "client_id",
    kind: FieldKind::Secret,
    required: true,
}];

const fn schema(
    provider: &'static str,
    display_name: &'static str,
    importance: Importance,
    rank: u8,
    fields: &'static [CredentialField],
    registration_url: &'static str,
) -> CredentialSchema {
    CredentialSchema {
        provider,
        display_name,
        importance,
        rank,
        fields,
        registration_url,
    }
}

/// Every provider with credentials, in the order Settings lists them: by `rank`. TMDB first, then
/// OMDb, TheTVDB and Fanart.tv, then the optional providers.
pub const CREDENTIAL_SCHEMAS: &[CredentialSchema] = &[
    schema(
        key::TMDB,
        "TMDB",
        Importance::Required,
        1,
        API_KEY_ONLY,
        "https://www.themoviedb.org/settings/api",
    ),
    schema(
        key::OMDB,
        "OMDb",
        Importance::Recommended,
        2,
        API_KEY_ONLY,
        "https://www.omdbapi.com/apikey.aspx",
    ),
    schema(
        key::TVDB,
        "TheTVDB",
        Importance::Recommended,
        3,
        TVDB_FIELDS,
        "https://thetvdb.com/api-information",
    ),
    schema(
        key::FANART,
        "Fanart.tv",
        Importance::Recommended,
        4,
        API_KEY_ONLY,
        "https://fanart.tv/get-an-api-key/",
    ),
    schema(
        key::MOVIEMETER,
        "MovieMeter",
        Importance::Optional,
        5,
        API_KEY_ONLY,
        "https://www.moviemeter.nl/site/registerclient/",
    ),
    schema(
        key::MDBLIST,
        "MDBList",
        Importance::Optional,
        6,
        API_KEY_ONLY,
        "https://mdblist.com/preferences/",
    ),
    schema(
        key::ANIDB,
        "AniDB",
        Importance::Optional,
        7,
        ANIDB_FIELDS,
        "https://anidb.net/perl-bin/animedb.pl?show=client",
    ),
    schema(
        key::TRAKT,
        "Trakt",
        Importance::Optional,
        8,
        TRAKT_FIELDS,
        "https://trakt.tv/oauth/applications",
    ),
    schema(
        key::MPDB,
        "MPDb.tv",
        Importance::Optional,
        9,
        MPDB_FIELDS,
        "https://mpdb.tv/",
    ),
    schema(
        key::THESPORTSDB,
        "TheSportsDB",
        Importance::Optional,
        10,
        THESPORTSDB_FIELDS,
        "https://www.thesportsdb.com/pricing",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn schema_of(provider: &str) -> &'static CredentialSchema {
        CREDENTIAL_SCHEMAS
            .iter()
            .find(|s| s.provider == provider)
            .expect("provider has a schema")
    }

    #[test]
    fn provider_keys_are_unique_and_every_schema_is_well_formed() {
        let mut seen = HashSet::new();
        for schema in CREDENTIAL_SCHEMAS {
            assert!(
                seen.insert(schema.provider),
                "duplicate {}",
                schema.provider
            );
            assert!(
                !schema.fields.is_empty(),
                "{} has no field",
                schema.provider
            );
            assert!(schema.registration_url.starts_with("https://"));
            let keys: HashSet<_> = schema.fields.iter().map(|f| f.key).collect();
            assert_eq!(
                keys.len(),
                schema.fields.len(),
                "{} fields",
                schema.provider
            );
        }
    }

    #[test]
    fn tmdb_is_required_and_the_recommended_set_is_fixed() {
        let of = |importance| {
            CREDENTIAL_SCHEMAS
                .iter()
                .filter(|s| s.importance == importance)
                .map(|s| s.provider)
                .collect::<Vec<_>>()
        };
        assert_eq!(of(Importance::Required), [key::TMDB]);
        // OMDb is recommended: it carries IMDb, Rotten Tomatoes and Metacritic.
        assert_eq!(
            of(Importance::Recommended),
            [key::OMDB, key::TVDB, key::FANART]
        );
        let optional = of(Importance::Optional);
        assert_eq!(
            optional.len(),
            CREDENTIAL_SCHEMAS.len() - 4,
            "everything else is optional: {optional:?}"
        );
    }

    #[test]
    fn the_rank_orders_the_catalogue_from_one_without_gaps() {
        let ranks: Vec<u8> = CREDENTIAL_SCHEMAS.iter().map(|s| s.rank).collect();
        let expected: Vec<u8> = (1..=u8::try_from(CREDENTIAL_SCHEMAS.len()).unwrap()).collect();
        assert_eq!(ranks, expected, "the list is in rank order");
        let order: Vec<&str> = CREDENTIAL_SCHEMAS.iter().map(|s| s.provider).collect();
        assert_eq!(order[..4], [key::TMDB, key::OMDB, key::TVDB, key::FANART]);
        // Every optional provider ranks after the four that matter.
        assert!(
            CREDENTIAL_SCHEMAS
                .iter()
                .filter(|s| s.importance == Importance::Optional)
                .all(|s| s.rank > 4)
        );
    }

    #[test]
    fn anidb_takes_a_client_name_and_an_optional_version() {
        let schema = schema_of(key::ANIDB);
        assert!(schema.field(API_KEY).is_some_and(|f| f.required));
        let version = schema.field(ANIDB_CLIENT_VERSION).expect("version field");
        assert!(!version.required);
        assert_eq!(version.kind, FieldKind::Text);
    }

    #[test]
    fn mpdb_needs_an_app_key_an_account_and_a_subscription_key() {
        let schema = schema_of(key::MPDB);
        assert_eq!(schema.fields.len(), 3);
        assert!(schema.fields.iter().all(|f| f.required));
        assert_eq!(
            schema.field(MPDB_USERNAME).map(|f| f.kind),
            Some(FieldKind::Text)
        );
        assert_eq!(
            schema.field(MPDB_SUBSCRIPTION_KEY).map(|f| f.kind),
            Some(FieldKind::Secret)
        );
    }

    #[test]
    fn thesportsdb_works_without_a_key() {
        assert!(
            schema_of(key::THESPORTSDB)
                .fields
                .iter()
                .all(|f| !f.required)
        );
    }

    #[test]
    fn the_tvdb_pin_is_optional() {
        let schema = schema_of(key::TVDB);
        assert!(schema.field(API_KEY).is_some_and(|f| f.required));
        let pin = schema.field(TVDB_PIN).expect("pin field");
        assert!(!pin.required);
        assert_eq!(pin.kind, FieldKind::Secret);
    }

    #[test]
    fn trakt_registers_an_app() {
        assert!(
            schema_of(key::TRAKT)
                .field("client_id")
                .is_some_and(|f| f.required)
        );
        assert!(schema_of(key::TRAKT).field("client_secret").is_none());
        assert!(
            schema_of(key::TRAKT)
                .registration_url
                .contains("trakt.tv/oauth/applications")
        );
    }
}

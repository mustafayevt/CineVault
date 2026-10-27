## MODIFIED Requirements

### Requirement: Keys live in the system credential store
Keys SHALL be stored in the operating system credential store (Windows Credential Manager, the macOS Keychain, or the Secret Service on Linux), never in the settings file. A portable copy SHALL use a credential service of its own, so it starts with no keys and never changes an installed copy's keys.

#### Scenario: Settings file holds no key
- **WHEN** the user saves a TMDB key and the settings file is inspected
- **THEN** the file does not contain the key

#### Scenario: Key kept in GNOME Keyring
- **WHEN** the user saves a TMDB key on a Linux desktop running GNOME Keyring and restarts CineVault
- **THEN** the key is still configured, and the keyring lists it under the service `CineVault`

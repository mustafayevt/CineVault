## MODIFIED Requirements

### Requirement: Secrets in the operating system credential store
API keys, passwords and tokens SHALL be stored in the operating system credential store (Windows Credential Manager, macOS Keychain, or on Linux the Secret Service such as GNOME Keyring or KWallet) under the service name `CineVault`, addressed by account names such as `tmdb.api_key`, and never in the settings file. Settings SHALL hold only non-secret facts, such as whether a key is configured. When no credential store is reachable (on Linux, no Secret Service is running), saving a key SHALL fail with "No system keyring is running. Install or unlock GNOME Keyring or KWallet, then try again." and the key SHALL NOT be written to any file.

#### Scenario: API key not in the settings file
- **WHEN** the user saves a TMDB API key
- **THEN** the settings file contains no key material, and the key is available after restart

#### Scenario: No keyring on Linux
- **WHEN** the user saves a TMDB key on a Linux desktop where no Secret Service is running
- **THEN** the save fails with the message above, and no file in the data folder contains the key

### Requirement: Data folder location
The data folder SHALL be chosen in this order: portable mode when a file named `portable` sits next to the executable (data in a `data` folder beside it); else a custom folder named in `data-location.json` in the per-user configuration folder; else the per-user application-data folder (`%APPDATA%\CineVault` on Windows, `~/Library/Application Support/CineVault` on macOS, `$XDG_DATA_HOME/CineVault` on Linux, which is `~/.local/share/CineVault` by default). On Linux the per-user configuration folder SHALL be `$XDG_CONFIG_HOME/CineVault` (by default `~/.config/CineVault`). A pointer file that cannot be used SHALL fall back to the per-user folder with a warning and SHALL not be overwritten.

#### Scenario: Portable mode
- **WHEN** a `portable` marker file exists next to the executable
- **THEN** the library, settings and caches are stored in a `data` folder beside the executable

#### Scenario: Broken pointer file
- **WHEN** `data-location.json` holds a relative path
- **THEN** the app uses the per-user folder, reports a warning and leaves the pointer file as it is

#### Scenario: Linux default
- **WHEN** CineVault starts for the first time on Linux with `XDG_DATA_HOME` unset
- **THEN** its library and settings are in `~/.local/share/CineVault`

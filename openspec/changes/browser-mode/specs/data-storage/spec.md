# Spec Delta

## MODIFIED Requirements

### Requirement: Data folder location
The data folder SHALL be chosen in this order: `CINEVAULT_DATA_DIR` when it is set to an absolute path (the Environment mode); else portable mode when a file named `portable` sits next to the executable (data in a `data` folder beside it); else a custom folder named in `data-location.json` in the per-user configuration folder; else the per-user application-data folder (`%APPDATA%\CineVault` on Windows, `~/Library/Application Support/CineVault` on macOS, `$XDG_DATA_HOME/CineVault` on Linux, which is `~/.local/share/CineVault` by default). The environment variable SHALL be read before any per-user folder is looked up, so a process without `HOME` can still start. A `CINEVAULT_DATA_DIR` that is relative or empty SHALL be refused with a warning and the next rule SHALL decide, so the app still starts. The Environment mode SHALL share the non-portable credential service, exactly as the custom and per-user modes do. On Linux the per-user configuration folder SHALL be `$XDG_CONFIG_HOME/CineVault` (by default `~/.config/CineVault`). A pointer file that cannot be used SHALL fall back to the per-user folder with a warning and SHALL not be overwritten.

#### Scenario: Portable mode
- **WHEN** a `portable` marker file exists next to the executable and `CINEVAULT_DATA_DIR` is not set
- **THEN** the library, settings and caches are stored in a `data` folder beside the executable

#### Scenario: Broken pointer file
- **WHEN** `data-location.json` holds a relative path
- **THEN** the app uses the per-user folder, reports a warning and leaves the pointer file as it is

#### Scenario: Linux default
- **WHEN** CineVault starts for the first time on Linux with `XDG_DATA_HOME` unset and no other choice made
- **THEN** its library and settings are in `~/.local/share/CineVault`

#### Scenario: Environment wins
- **WHEN** `CINEVAULT_DATA_DIR=/srv/cinevault/data` is set on a machine that also has a `portable` marker and a pointer file
- **THEN** the library, settings and caches are in `/srv/cinevault/data`, neither portable mode nor the pointer file is read, and the per-user folder is not used

#### Scenario: Relative environment value
- **WHEN** `CINEVAULT_DATA_DIR=data` is set
- **THEN** the app reports that the folder must be absolute and uses the next rule, so it still starts

#### Scenario: No home folder
- **WHEN** the server starts in a container with no `HOME` and `CINEVAULT_DATA_DIR=/data`
- **THEN** it uses `/data` and does not fail looking for a per-user folder

### Requirement: Secrets in the operating system credential store
On the desktop app, API keys, passwords and tokens SHALL be stored in the operating system credential store (Windows Credential Manager, macOS Keychain, or on Linux the Secret Service such as GNOME Keyring or KWallet) under the service name `CineVault`, addressed by account names such as `tmdb.api_key`, and never in the settings file. Settings SHALL hold only non-secret facts, such as whether a key is configured. When no credential store is reachable (on Linux, no Secret Service is running), saving a key SHALL fail with "No system keyring is running. Install or unlock GNOME Keyring or KWallet, then try again." and the key SHALL NOT be written to any file. The server SHALL use an encrypted secrets file instead, because a headless service has no session bus: `secrets.enc` in the data folder, encrypted with a 32-byte master key supplied outside the folder as `CINEVAULT_SECRET_KEY`, `CINEVAULT_SECRET_KEY_FILE`, a systemd credential or a Docker secret. The file SHALL be written atomically with mode `0600`; the master key and every secret SHALL be registered for log redaction. When the master key is missing the server SHALL start, report `"keys":"locked"` in health, and say in Settings how to supply it; it SHALL NOT write a key in plain text or silently forget one. Rotating the master key SHALL rewrite the file in one transaction.

#### Scenario: API key not in the settings file
- **WHEN** the user saves a TMDB API key in the desktop app
- **THEN** the settings file contains no key material, and the key is available after restart

#### Scenario: No keyring on Linux
- **WHEN** the user saves a TMDB key on a Linux desktop where no Secret Service is running
- **THEN** the save fails with the message above, and no file in the data folder contains the key

#### Scenario: Server key survives a restart
- **WHEN** the server saves a TMDB key with a master key from a Docker secret and restarts
- **THEN** the key is read from `secrets.enc` and the scrape works without entering it again

#### Scenario: Master key missing
- **WHEN** the server starts without `CINEVAULT_SECRET_KEY_FILE`
- **THEN** it starts, health says `"keys":"locked"`, Settings explains how to supply the key, and no plaintext key is written

#### Scenario: Wrong master key
- **WHEN** the configured master key cannot decrypt `secrets.enc`
- **THEN** the server says the key is wrong, keeps the file, and writes nothing over it

### Requirement: Contents of the data folder
The data folder SHALL hold `library.db`, `settings.json` and `cache`, and SHALL gain `tools` (downloaded external tools), `tmp` (work in progress such as update downloads and upload staging) and `nfo-undo` (what Read NFO and Rewrite NFO need to undo an action) when they are first needed. The server SHALL add `server.json` (bind address, port and non-secret server settings), `server-auth.json` (the salted password hash only, mode `0600`), `secrets.enc` (the encrypted provider keys) and `library-owner.json` (the single-writer owner and heartbeat). None of them SHALL hold key material in plain text.

#### Scenario: First start
- **WHEN** the app starts with an empty data folder
- **THEN** it creates the data folder, `library.db` and the `cache` folder, and no `tools` folder until a tool is installed

#### Scenario: Server first start
- **WHEN** the server starts with an empty data folder and a password is set
- **THEN** `server.json` and `server-auth.json` appear, `server-auth.json` is readable only by its owner, and no provider key is in either file

#### Scenario: Portable copy on a USB drive
- **WHEN** a portable copy is run from `E:\CineVault` and closed
- **THEN** its web view data and window state are under `E:\CineVault\data`, and it uses its own credential service

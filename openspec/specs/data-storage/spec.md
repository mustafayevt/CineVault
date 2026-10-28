# data-storage Specification

## Purpose
Covers how CineVault keeps its data on disk: the SQLite library database (`library.db`) with its schema and ordered migrations, durable and transactional writes, the rule that loading never deletes library rows, query speed, provider IDs and open-ended values, the settings file, secrets in the operating system credential store, the data folder per operating system and in portable mode, the fresh start that sets aside a library or settings file of an earlier schema, the caches, and logging with secret redaction.

## Requirements

### Requirement: One SQLite library database
The library SHALL be stored in one SQLite database, `library.db`, in the data folder, created from one schema definition (`crates/cv-db/src/schema.sql`). Movies, movie sets, shows, seasons and episodes SHALL share one `item` table told apart by kind. Files, streams, credits, ratings, provider IDs, tags, genres, artwork, trailers, episode numbers, locks, reviews, the rename journal, History, jobs and data sources SHALL have their own tables. Filter and sort fields SHALL be indexed columns.

#### Scenario: Round trip of a fully populated movie
- **WHEN** a movie with every field filled is saved and the app is restarted
- **THEN** every field reads back with the same value

#### Scenario: Several numbering schemes per episode
- **WHEN** an episode has aired, DVD and absolute numbers
- **THEN** all three numbers are stored, one row per ordering, and the show's selected ordering decides which one is displayed

### Requirement: Schema version and ordered migrations
The schema version SHALL be SQLite's `user_version`. An empty database SHALL get the base schema (version 100) and every migration after it in one transaction. Migrations SHALL be numbered from 101 upward in steps of 1 (`crates/cv-db/src/migrations/`). Pending migrations SHALL run in one transaction with their version bump, so a failing migration leaves the file unchanged and the error is reported.

#### Scenario: New install
- **WHEN** the app starts with no library database
- **THEN** it creates `library.db` with the current schema in one transaction and opens an empty library

#### Scenario: Failed migration rolls back
- **WHEN** a migration fails part-way through
- **THEN** the database file is unchanged and the error names the migration

### Requirement: Backup before a migration
Before migrating a database that already holds data, the app SHALL write a snapshot into `backups/pre-migration/` next to the database and verify it, and SHALL not migrate when the snapshot fails. A new, empty database SHALL need no snapshot. A database whose version is higher than this build supports SHALL be refused without being changed.

#### Scenario: Pending migration on an existing library
- **WHEN** a library at version 101 is opened by a build whose last migration is 102
- **THEN** a verified copy is written to `backups/pre-migration/` first, then migration 102 runs and the library opens at version 102

#### Scenario: Database from a newer build
- **WHEN** the app finds a `library.db` whose schema version is higher than the one it knows
- **THEN** it does not change the file and reports that this build does not support that schema version

### Requirement: Durable, atomic writes
Every library write SHALL go through one writer connection on its own thread, in write-ahead-log mode with `synchronous = FULL`, so a committed change survives a crash or power loss. Writes SHALL be transactional, so a crash never leaves an entity partly written, and a write that panics SHALL be rolled back. Reads SHALL use a pool of read-only connections. Every connection SHALL enable foreign keys and a 5-second busy timeout.

#### Scenario: Crash after an edit
- **WHEN** the user edits a title and the process is killed 2 seconds later
- **THEN** the edited title is present after restart

#### Scenario: A write that fails half way
- **WHEN** a write transaction fails after changing some rows
- **THEN** none of its changes are in the database

### Requirement: No silent deletion of library rows
Loading the library or refreshing availability SHALL NOT delete or hide titles because their paths are missing, belong to another operating system or have no episodes. Availability SHALL be computed from the file system and stored with a reason as `online`, `offline`, `missing` or `invalid`, and only the user SHALL decide whether to remove a title. Removing a data source SHALL leave its titles in the library.

#### Scenario: Library opened while a drive is disconnected
- **WHEN** the app starts while drive `F:` is unplugged
- **THEN** all titles on `F:` remain in the library, marked offline, and nothing is deleted

#### Scenario: Show with zero episodes
- **WHEN** a TV show has no episode files
- **THEN** the show remains in the library, marked invalid with the reason "no episodes found"

#### Scenario: Path of another operating system
- **WHEN** a title's path is `D:\Movies\Example Movie (2020)` and the library is opened on macOS
- **THEN** the title stays in the library, marked invalid

### Requirement: History outlives titles
The rename journal and the job queue SHALL keep their rows when the title they refer to is removed. Removing a show or a set SHALL be an explicit, ordered action, never a side effect of removing another row.

#### Scenario: Journal after a title is removed
- **WHEN** a renamed title is removed from the library
- **THEN** its rename journal rows remain

### Requirement: Persisted runtime flags
The NEW flag, the review state, the last scrape provider and language, the field and item locks and the rename history SHALL be stored in the library, so they survive restarts.

#### Scenario: NEW flag survives restart
- **WHEN** three movies are added by a scan and the app is restarted before they are opened
- **THEN** the three movies still show the NEW indicator

### Requirement: Fast library queries
Filtering, sorting and full-text search over a 20,000-title library SHALL return the first page of results within 100 ms. Full-text search SHALL use an FTS5 index over titles, original titles, sort titles, plot, people, tags and file names, ignore diacritics and rank titles above plot.

#### Scenario: Full-text search
- **WHEN** the user searches for a director's surname in a 20,000-title library
- **THEN** every title crediting that person is returned within 100 ms

#### Scenario: Accents ignored
- **WHEN** the user searches "amelie" and the library holds "Amélie Example (2001)"
- **THEN** that title is found

### Requirement: Provider IDs stored as an open map
Each title SHALL store provider IDs as a map from provider key (for example `imdb`, `tmdb`, `tvdb`, `trakt`, `tmdbSet`, `anidb`, `tvmaze`, `wikidata`) to a string value. Any provider key SHALL be accepted. Empty, `0` and `null` values SHALL be dropped. IDs SHALL be indexed by provider and value, so a title can be found from an ID.

#### Scenario: Unknown provider ID kept
- **WHEN** a read NFO carries `<uniqueid type="kinopoisk">12345</uniqueid>`
- **THEN** the movie stores `kinopoisk = "12345"` and writes it back to the NFO unchanged

#### Scenario: Zero ID dropped
- **WHEN** a provider answers with a TVDB ID of `0`
- **THEN** no TVDB ID is stored

### Requirement: Open-ended genres, editions and media sources
Genres, editions and media sources SHALL have a built-in set of known values. Unknown values SHALL be stored as free text, read back exactly as written and never discarded.

#### Scenario: Custom edition preserved
- **WHEN** a folder is named `Example Movie (2020) {edition-Ultimate Archive}`
- **THEN** the movie's edition is stored as "Ultimate Archive"

### Requirement: Original language stored per title
Every movie and show SHALL store its original language as reported by the provider it was scraped from (for TMDB, `original_language`). Episodes SHALL use their show's original language.

#### Scenario: Stored on scrape
- **WHEN** a movie is scraped from TMDB with `original_language` "ja"
- **THEN** its original language reads back as Japanese after a restart

#### Scenario: Episode uses the show
- **WHEN** an episode of a show whose original language is English is opened
- **THEN** its original language is English

### Requirement: Atomic settings file
Settings SHALL be stored in a human-readable JSON file, `settings.json`, in a versioned envelope (`{ "version": 2, "settings": { ... } }`). A save SHALL write a temp file in the same folder, flush it to disk and replace the real file with a rename, so a crash leaves either the old or the new file, never a half-written one.

#### Scenario: Crash during a save
- **WHEN** the process is killed while a setting is being saved
- **THEN** `settings.json` holds either the old or the new settings in full

### Requirement: Settings never reset silently
A settings file that cannot be used (invalid JSON, wrong shape, written by a newer build) SHALL never be overwritten: it SHALL be renamed to `settings.json.corrupt-<UTC timestamp>`, and the app SHALL start with defaults and tell the user. A file that exists but cannot be read SHALL be read again after short waits; if it still fails, the app SHALL use defaults, leave the file alone, and rename it to `settings.json.unreadable-<UTC timestamp>` before the first save, or fail that save.

#### Scenario: Corrupt settings file
- **WHEN** `settings.json` contains invalid JSON at startup
- **THEN** the app starts with defaults, renames the broken file with a `.corrupt-<timestamp>` suffix, and a notice says "Your settings could not be read, so defaults are in use" and where the old file was kept

#### Scenario: Settings file held at start-up
- **WHEN** another program holds `settings.json` for a moment while the app starts
- **THEN** the app reads it once the program lets go, and the user's settings load unchanged

#### Scenario: Settings file still unreadable
- **WHEN** `settings.json` exists but cannot be read after the retries
- **THEN** no defaults are saved over it, Activity shows "Your settings could not be loaded", and the first save renames it to `settings.json.unreadable-<time>` before writing

### Requirement: Secrets in the operating system credential store
API keys, passwords and tokens SHALL be stored in the operating system credential store (Windows Credential Manager, macOS Keychain, or on Linux the Secret Service such as GNOME Keyring or KWallet) under the service name `CineVault`, addressed by account names such as `tmdb.api_key`, and never in the settings file. Settings SHALL hold only non-secret facts, such as whether a key is configured. When no credential store is reachable (on Linux, no Secret Service is running), saving a key SHALL fail with "No system keyring is running. Install or unlock GNOME Keyring or KWallet, then try again." and the key SHALL NOT be written to any file.

#### Scenario: API key not in the settings file
- **WHEN** the user saves a TMDB API key
- **THEN** the settings file contains no key material, and the key is available after restart

#### Scenario: No keyring on Linux
- **WHEN** the user saves a TMDB key on a Linux desktop where no Secret Service is running
- **THEN** the save fails with the message above, and no file in the data folder contains the key

### Requirement: Portable copy keeps its own secrets
A portable copy SHALL file its secrets under a service of its own, `CineVault portable <id>`, with a random id kept in `credential-store-id` in its data folder (derived from the folder path when that file cannot be written). It SHALL never read or change the keys of an installed copy or of another portable copy.

#### Scenario: Portable copy starts without keys
- **WHEN** a portable copy is started on a computer where the installed copy has a TMDB key
- **THEN** the portable copy shows no TMDB key, and saving one there leaves the installed copy's key unchanged

### Requirement: Secrets redacted from logs
Log output SHALL pass through a redacting writer that replaces credentials with `[REDACTED]`. It SHALL mask by shape (API-key style query and form parameters, credential headers, `Bearer` tokens and `user:password@` in URLs) and by value (every secret stored in or read from the credential store, of at least 6 characters, raw or percent-encoded, wherever it appears).

#### Scenario: Key in a logged URL
- **WHEN** detailed logging is on and a provider request with `api_key=<key>` in its URL is logged
- **THEN** the log line shows `api_key=[REDACTED]`

#### Scenario: Key inside a URL path
- **WHEN** a stored key appears inside a logged URL path, where no parameter rule applies
- **THEN** the key is masked because it is a known secret value

### Requirement: Log detail setting
The log detail SHALL follow the setting `advanced.log-level` ("Log detail"), with the choices "Errors only" (`error`), "Normal" (`info`, the default) and "Detailed" (`debug`). A change SHALL apply at once without a restart. The `CINEVAULT_LOG` and `RUST_LOG` environment variables, when set, SHALL decide the filter and the setting SHALL leave it alone.

#### Scenario: Detailed logging switched on
- **WHEN** the user changes Log detail to "Detailed"
- **THEN** CineVault's own debug lines are logged from that moment, without a restart, and secrets stay masked

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

### Requirement: Contents of the data folder
The data folder SHALL hold `library.db`, `settings.json` and `cache`, and SHALL gain `tools` (downloaded external tools), `tmp` (work in progress such as update downloads) and `nfo-undo` (what Read NFO and Rewrite NFO need to undo an action) when they are first needed.

#### Scenario: First start
- **WHEN** the app starts with an empty data folder
- **THEN** it creates the data folder, `library.db` and the `cache` folder, and no `tools` folder until a tool is installed

### Requirement: Portable copy writes nothing outside its folder
In portable mode the web view profile SHALL be kept in a `webview` folder inside the data folder, and the main window's size and place SHALL be kept in `window-state.json` inside the data folder, so a portable copy writes nothing to the user profile.

#### Scenario: Portable copy on a USB drive
- **WHEN** a portable copy is run from `E:\CineVault` and closed
- **THEN** its web view data and window state are under `E:\CineVault\data`

### Requirement: Library of an earlier schema set aside
At start-up, before any SQLite connection, the app SHALL read the header of `library.db`. A database of an earlier schema (version 1 to 99, or 0 with content) SHALL be moved with its `-wal` and `-shm` files by a same-folder rename to `library-old.db` (or the first free `library-old-2.db`, `library-old-3.db`, ...), never copied, read, migrated or deleted. A new empty library SHALL then be created, and the first scan rebuilds it from the media folders.

#### Scenario: Library of an earlier schema
- **WHEN** the app starts and `library.db` has schema version 42
- **THEN** `library-old.db` holds that database byte for byte, `library.db` is new and empty, and the first scan rebuilds the library from the folders

#### Scenario: Old copy already present
- **WHEN** `library-old.db` already exists and another `library.db` of an earlier schema is found
- **THEN** the database is moved to `library-old-2.db` and `library-old.db` is left as it is

### Requirement: Setting aside is safe and runs once
If a rename fails part-way, the files already moved SHALL be renamed back and the app SHALL stop with a message naming the file. Each step of the fresh start SHALL do nothing when there is nothing to do, so a stop between steps is finished on the next start and later starts change nothing. The `cinevault` command line SHALL never make these moves.

#### Scenario: The move cannot be made
- **WHEN** the old database cannot be moved because another program holds it
- **THEN** the app retries briefly, then stops with a message naming the file, and neither file is changed or deleted

#### Scenario: Later start
- **WHEN** the app starts again after the old library was set aside
- **THEN** nothing is moved

### Requirement: Settings of an earlier version set aside
A `settings.json` whose envelope version is lower than this build's SHALL be renamed to `settings-old.json` (or the first free `settings-old-N.json`) and never read. The app SHALL then start from default settings with the first-run setup open. Provider keys and passwords in the credential store SHALL be kept and used.

#### Scenario: Keys kept with earlier settings
- **WHEN** a `settings.json` with envelope version 1 is found and TMDB and OMDb keys are in the credential store
- **THEN** the file becomes `settings-old.json`, the first-run setup opens with every setting at its default, and the TMDB and OMDb keys are already filled

### Requirement: Caches hold only rebuildable data
The thumbnail cache SHALL be capped by `advanced.image-cache-size` in megabytes (default 1024, accepted range 100 to 51,200); a new cap SHALL apply at once, trimming least recently used thumbnails first. Grabbed video frames SHALL count toward the image cache, and frames older than 24 hours SHALL be removed at start. Provider responses SHALL be cached for 15 minutes (API answers) or 24 hours (web pages). No cache SHALL hold artwork files of the library.

#### Scenario: Cap lowered
- **WHEN** the thumbnail cache holds 900 MB and the cap is set to 500 MB
- **THEN** the least recently used thumbnails are removed until the cache fits, and they are made again when shown

#### Scenario: Old frame grabs
- **WHEN** the app starts and a grabbed frame in the cache is two days old
- **THEN** the frame is removed

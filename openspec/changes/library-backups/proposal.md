## Why

`library.db` holds the whole library, the History and the rename journal that every Undo depends on, and nothing ever backs it up. The only copy CineVault makes is the snapshot taken just before a schema migration (`crates/cv-db/src/migrate.rs`). Yet three comments speak of daily backups as if they existed (`src-tauri/src/main.rs`, `crates/cv-app/tests/app_services.rs`, `crates/cv-db/src/migrate.rs`). A damaged disk, a bad sector under the database or a mistaken bulk action can cost years of History, with no way back.

## What Changes

1. **A daily backup of the library.** The desktop app and the headless server (whichever holds the data folder) write a copy of `library.db` once a day into the data folder's `backups` folder as `library-YYYY-MM-DD.db`. The first check runs a minute after start-up, then every hour while CineVault runs. A backup is due when the newest one is more than 24 hours old.
2. **Only verified copies.** Each copy is written to a temporary file as a consistent snapshot of the open database and passes SQLite's integrity check. Only then does it take its final name, so a crash or a full disk never leaves a half-written backup in place.
3. **Seven kept.** The newest seven daily backups are kept and older ones are removed. The pre-migration snapshots in `backups/pre-migration/` and any other file are never touched.
4. **Never in the way.** Library reads and writes go on while a backup is written. A failed backup is logged with its reason and tried again at the next check. It never stops the app or the server.
5. **Restore by hand.** The README and the server guide say how to restore: close CineVault, set the current `library.db` aside, copy a backup in its place.
6. **Honest comments.** The three comments describe what now exists.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `data-storage`: daily verified backups of the library database, their retention, and restoring one.

## Out of scope

- A restore button or a backups list in Settings, and settings to change the schedule or the number kept.
- Backing up `settings.json`, the credential store, caches or media-folder files (NFO, artwork).
- Backups to another drive or to the network.
- A backup on quit. The hourly check covers long sessions, and a quit has to be quick (container stop timeouts, system shutdown, update installs). See design.md.

## Impact

- `crates/cv-db`: a `backup` module (write, verify, name, prune), sharing the verification with the pre-migration snapshot.
- `crates/cv-app`: `AppServices::start` schedules the checks; `shutdown` stops them.
- Comments in `src-tauri/src/main.rs`, `crates/cv-app/tests/app_services.rs` and `crates/cv-db/src/migrate.rs`.
- New files in the data folder: `backups/library-YYYY-MM-DD.db` (up to seven).
- `README.md`, `packaging/README.md`, `CHANGELOG.md` and `site/changelog.html`.

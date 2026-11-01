## ADDED Requirements

### Requirement: Daily library backup
While the desktop app or the server holds the data folder, it SHALL check a minute after start-up and then every hour whether the newest daily backup in the data folder's `backups` folder is more than 24 hours old or missing, and if so SHALL write a copy of `library.db` there as `library-YYYY-MM-DD.db` (the local date). The command line SHALL NOT write daily backups.

#### Scenario: First start after the update
- **WHEN** CineVault starts with a library and no daily backup
- **THEN** about a minute later `backups/library-2026-11-02.db` exists on 2 November 2026

#### Scenario: Backup is recent
- **WHEN** CineVault starts and the newest daily backup is 5 hours old
- **THEN** no backup is written until it is more than 24 hours old

#### Scenario: Server running for days
- **WHEN** the server runs for three days without a restart
- **THEN** a new daily backup is written each day

### Requirement: Only verified backups
A daily backup SHALL be a consistent snapshot of the open library, written under a temporary name, checked with SQLite's integrity check and for the library's schema version, and only then renamed to its final name in one step. A copy that fails SHALL be removed, and no file with a backup's name SHALL ever hold an unfinished or unchecked copy.

#### Scenario: Crash during a backup
- **WHEN** the process is killed while a backup is being written
- **THEN** no `library-*.db` file holds the partial copy, and the next start removes the temporary file

#### Scenario: Copy fails its check
- **WHEN** the copy does not pass the integrity check
- **THEN** it is removed, the earlier backups are unchanged and the log says why

#### Scenario: Backup opens as a library
- **WHEN** a daily backup is opened in place of `library.db`
- **THEN** CineVault opens it and shows the titles and History the library had when the copy was made

### Requirement: Seven daily backups kept
After a verified backup is written, only the newest seven `library-YYYY-MM-DD.db` files in `backups` SHALL be kept and older ones removed. The pre-migration snapshots and every other file or folder in `backups` SHALL never be removed by this rule.

#### Scenario: Eighth backup
- **WHEN** an eighth daily backup is written
- **THEN** the oldest daily backup is removed and the newest seven remain

#### Scenario: Pre-migration snapshots untouched
- **WHEN** daily backups are pruned while `backups/pre-migration/` holds three snapshots
- **THEN** the three snapshots are still there

#### Scenario: Backup fails, nothing pruned
- **WHEN** a backup fails
- **THEN** no earlier daily backup is removed

### Requirement: Backups never block the library
Library reads and writes SHALL go on while a backup is written. A failed backup SHALL be logged with its reason and tried again at the next hourly check, and SHALL NOT stop, slow the start of, or show an error dialog in the app or the server.

#### Scenario: Disk full
- **WHEN** the data folder's drive is full when a backup is due
- **THEN** the log says the backup failed and why, the library keeps working, and the next check tries again

#### Scenario: Editing during a backup
- **WHEN** the user edits a title while a backup is being written
- **THEN** the edit is saved without waiting for the backup

### Requirement: Restoring a backup
The README and the server guide SHALL describe restoring a daily backup: close CineVault, set the current `library.db` and its `-wal` and `-shm` files aside, and copy the backup to `library.db`. They SHALL say that History and Undo go back to the backup's day.

#### Scenario: Restore by hand
- **WHEN** the user follows the steps with `library-2026-11-01.db`
- **THEN** CineVault starts with the library of that day, and the set-aside `library.db` can be put back the same way

### Requirement: Backups that keep failing are reported
When no backup has succeeded for three days while one was due, CineVault SHALL put one message in the notification centre ("The library has not been backed up since <date>. <reason>") and keep it until a backup succeeds. A successful backup SHALL clear it.

#### Scenario: Three days of failures
- **WHEN** the backups folder has been read-only for three days
- **THEN** the notification centre says the library has not been backed up since the day of the last good backup, with the reason

#### Scenario: Fixed
- **WHEN** the next backup after that message succeeds
- **THEN** the message leaves the notification centre

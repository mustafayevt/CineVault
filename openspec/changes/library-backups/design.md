# Design

## Context

- `crates/cv-db/src/migrate.rs` takes the only copy today. `snapshot` (`migrate.rs:97`) runs `VACUUM INTO` into `backups/pre-migration/<stem>-v<version>-<millis>.db` next to the database. `verify` (`migrate.rs:128`) then opens the copy, runs `PRAGMA integrity_check` and compares `user_version`. A failed copy is removed. `PRE_MIGRATION_DIR` (`migrate.rs:27`) says it is "kept apart from the rolling daily backups", which do not exist.
- `src-tauri/src/main.rs:237` lists "daily backups" among the background work, and `crates/cv-app/tests/app_services.rs:2` lists them among what its tests cover. Neither exists.
- The database runs in WAL mode with one writer thread and a pool of read-only connections (`crates/cv-db/src/lib.rs`; readers set `query_only`, `connection.rs:46`). `Database::path` gives the file.
- Both owners of a data folder, the desktop app (`src-tauri/src/boot.rs:139`) and the headless server (`crates/cv-web/src/headless.rs`), start the background work through `AppServices::start` (`crates/cv-app/src/services.rs:679`) and stop it through `AppServices::shutdown` (`services.rs:744`), which aborts the tasks in `self.tasks`. The command line does not call `start`, so it never backs up. That is right, because it runs only while no owner holds the folder, or forwards to the owner.

## Goals / Non-Goals

**Goals:** a verified copy of `library.db` no older than about a day, seven days deep, for both owners, that never blocks or slows the library noticeably and never leaves a broken file under a backup's name.

**Non-Goals:** restore UI, configurable schedule or retention, backups elsewhere than the data folder, anything beyond `library.db`.

## Decisions

### D1. Where and what

The backups go in `<data folder>/backups/library-YYYY-MM-DD.db`, using the local date at the moment the copy starts. The folder is the parent of `backups/pre-migration/`, which stays untouched. The work in progress is `backups/library-YYYY-MM-DD.db.partial`.

### D2. Writing one backup (`cv_db::backup::write`)

1. Remove any `*.partial` left in `backups/` by an earlier interrupted run.
2. Open a dedicated connection to the database with `SQLITE_OPEN_READ_ONLY`, without `query_only`, and run `VACUUM INTO '<partial>'`. In WAL mode this reads one consistent snapshot and does not block the writer, so scans and edits go on. A test checks that `VACUUM INTO` works on that read-only connection while the writer commits. *Fallback, if a SQLite version refuses it:* run it on the writer thread, which pauses writes only for as long as the copy takes.
3. Open the partial file, `sync_all` it, and close it.
4. Verify it with the function the pre-migration snapshot uses, moved to `backup::verify`: it must open, `PRAGMA integrity_check` must say `ok`, and its `user_version` must equal the live database's.
5. Rename the partial file to `library-YYYY-MM-DD.db` in the same folder. That is one atomic replace, and it overwrites a same-day file if the clock went back. On Unix, sync the folder.
6. Prune (D4).

Any failure removes the partial file and returns the error. Nothing else is changed.

### D3. When

`AppServices::start` spawns one task into `self.tasks`. It waits 60 seconds, so the cold start and the start-up scan come first, then checks. After that it checks every hour. A check is due when no `library-*.db` exists in `backups/`, or when the newest one by modified time is more than 24 hours old. A due check runs D2 in `spawn_blocking`. An `AtomicBool` keeps it to one backup at a time.

**No backup on quit.** A backup at a clean quit, when one is overdue, was considered. The hourly check makes that unnecessary: a session long enough to be overdue has already had its check. A quit also has hard limits: Docker stops a container 10 seconds after `SIGTERM`, the server's own grace is 15 seconds, and Windows shutdown and the update installer do not wait. A large library's copy and integrity check could push past those limits. `shutdown` aborts the task. A copy in flight on the blocking thread either finishes or dies with the process, and its `.partial` is removed at the next start.

### D4. Retention

After a successful write, list the plain files in `backups/` whose names match exactly `library-\d{4}-\d{2}-\d{2}\.db`, sort them by name (the date), and delete all but the newest seven. Folders (including `pre-migration`), `.partial` files and any other name are never touched. These are CineVault's own copies, not the user's media, so they are deleted outright, not sent to the deleted-files destination. A failed delete is logged and tried again after the next backup.

### D5. Failure

A failed backup logs `warn!` with the reason (full disk, unreadable database, integrity failure) and the path. The next hourly check tries again. The library keeps working throughout. A failed integrity check of the copy also logs that the live database should be checked, but CineVault does not act on the live file.

### D6. Restore (documented, no UI)

The README ("Backups") and `packaging/README.md` say how to restore:

1. Quit CineVault, or stop the server.
2. In the data folder, rename `library.db` to `library-before-restore.db`, and move `library.db-wal` and `library.db-shm` aside if they exist.
3. Copy `backups/library-YYYY-MM-DD.db` to `library.db`.
4. Start CineVault.

The text also says that the History and Undo go back to that day. Changes made to files after it are found again by the next scan, but cannot be undone from CineVault.

A backup is an ordinary library file. A backup from an earlier schema is migrated, with its own pre-migration snapshot. Step 2 keeps the previous library, so a restore can itself be undone by swapping the files back.

### D7. Safety when interrupted, and undo

Nothing is written in a media folder. Inside the data folder, only `.partial` ever holds an unfinished copy, and it never carries a backup's name. The rename is atomic, so a crash leaves either the old same-day backup or the new one. The live `library.db` is only read. Pruning deletes only files that match the exact daily name, and only after a newer backup has been verified, so at least one verified backup always remains once one has been made.

## Risks / Trade-offs

- [Disk use: up to seven copies, compacted by `VACUUM INTO`] → A library of tens of thousands of titles is tens of MB, so seven copies are small next to the artwork cache. A full disk only makes the backup fail and logs it.
- [A copy on a large library takes a few seconds of disk reading once a day] → It runs on a blocking thread, after start-up, without blocking writes.
- [Portable copies keep backups on the same USB drive] → The same is true of the library itself. Copies elsewhere are out of scope.
- [A damaged live database copies as damaged] → The integrity check catches it, and the log says so. The older backups stay, because pruning runs only after a verified write.

## Migration Plan

Nothing to migrate. The first check after the update writes the first backup.

## Open Questions

None. Backups that keep failing were settled by the owner: after three days without a successful backup the notification centre says so ("Backups that keep failing are reported"), in addition to the log line of D5.

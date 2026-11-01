# Tasks

## 1. Writing a verified backup

- [x] 1.1 Add `cv_db::backup` with `verify` moved from `migrate.rs` (the pre-migration snapshot calls it from there) and `write(db, backups_dir, date)`: remove stale `*.partial`, `VACUUM INTO` on a dedicated read-only connection, `sync_all`, verify, rename, sync the folder on Unix; remove the partial file on any failure. Verified by `cargo test -p cv-db`: the backup opens with the same rows and `user_version`; a write committed during the copy is not blocked; a stale `.partial` is removed; a same-day file is replaced; a forced verification failure leaves no `library-*.db` and no `.partial`; the existing pre-migration tests still pass.
- [ ] 1.2 Add `backup::prune(backups_dir, 7)` over exact `library-YYYY-MM-DD.db` names only. Verified by tests with nine dated files, a `pre-migration` folder, a `.partial` and an unrelated file: the newest seven daily files and every other entry remain.

## 2. Scheduling in both owners

- [ ] 2.1 Add the backup task to `AppServices::start` (60-second delay, hourly check, due after 24 hours, one at a time, `spawn_blocking`, failure logged) and stop it in `shutdown`. Verified by `cv-app` tests with a short injected delay and clock: a library with no backup gets one; a 5-hour-old backup is left alone; a 25-hour-old one is replaced by today's; a failure (read-only `backups` folder) is logged, does not panic and is retried at the next check; `shutdown` stops the task.
- [ ] 2.2 Report backups that keep failing: after three days without a successful backup while one was due, one notification-centre message with the date of the last good backup and the reason, cleared by the next success; in the server, the same text in its notifications. Verified by `cv-app` tests with an injected clock (no message after two days of failures, one message after three, none after a success).
- [ ] 2.3 Make the three comments true and precise: `src-tauri/src/main.rs:237`, `crates/cv-app/tests/app_services.rs:2` (with a test in that file that a booted temporary data folder gets a daily backup), and `crates/cv-db/src/migrate.rs:27`. Verified by `cargo test -p cv-app --test app_services` and by reading the diff.

## 3. Documentation

- [ ] 3.1 Add a "Backups" section to `README.md` and the restore steps to `packaging/README.md` (data folder in the container, `docker stop` first); add a `CHANGELOG.md` line under the unreleased version and run `bun run site:changelog`. Verified by `bun run site:changelog -- --check` and by following the steps once on a throwaway data folder.

## 4. Integration checks

- [ ] 4.1 Full checks: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test -p cv-compat`, `openspec validate library-backups --strict`. Verified by the commands passing.
- [ ] 4.2 Real run with a throwaway data folder on Windows (desktop app) and Linux (headless server): a backup appears about a minute after start, the app stays responsive during it, killing the process mid-copy leaves only a `.partial` that the next start removes, and restoring a backup by the documented steps opens that library. Needs a person for the desktop run. Verified by the recorded results.

# Tasks

## 1. Storage

- [x] 1.1 Add the `network_share` table and its migration in `cv-db` (server, share, user, local root unique, remember, created, last connected, last error code, unused since), with a repository to insert, list, update and delete rows and to find the share that holds a path (case-insensitive on Windows). Verified by `cargo test -p cv-db`: migration from the previous schema with the backup taken, path lookup for a share root, a folder below it, a sibling with a common prefix (`\\nas\movies2`) and a Windows case difference.

## 2. Connectors

- [x] 2.1 Create `crates/cv-shares` with the `ShareConnector` trait, the share address and input checks (server, share, folder, user; parsing `\\nas\movies\Films`, `//nas/movies` and `smb://nas/movies/Films`), the error kinds (wrong login, unreachable, share not found, other user connected, no user-level mounting, other with the system message) and a fake connector. Verified by `cargo test -p cv-shares` with cases for every accepted and refused input.
- [x] 2.2 Add the Windows connector (`New-SmbMapping` through a fixed PowerShell script, password on standard input as UTF-8, JSON result, error-code mapping, `Get-SmbMapping` check, `Remove-SmbMapping`). Verified by unit tests that the built command's arguments never contain the password and that every listed Win32 code maps to its kind, and by a manual check on Windows against a real SMB share (needs a person with a NAS or a second machine): connect, wrong password, unknown share, Explorer already connected as another user, disconnect.
- [x] 2.3 Add the macOS connector (`mount_smbfs` on a pseudo-terminal, mount folder `~/CineVault Shares/<server>/<share>` with mode `0700`, `umount`, a "is this folder a mount point" check, prompt and error mapping). Verified by unit tests over recorded tool output and argument checks, and by a manual check on a Mac with a real SMB share (needs a person): connect, wrong password, unmount, no password in `ps -ax -o args`.
- [x] 2.4 Add the Linux connector (`gio` detection, `gio mount` with prompts answered on standard input after reading each prompt, `gio info` for the local path, `gio mount -u`). Verified by unit tests against a fake `gio` script that records its arguments and input, including a changed prompt order that must fail without sending the password, and by a manual check on a GNOME or KDE desktop with a real share (needs a person).

## 3. App service

- [x] 3.1 Add `NetworkShareService` in `cv-app`: connect, check the folder, add it through `DataSourceService::add` with the usual rules, store the password through `SecretStore` (`network-share.<id>`) or keep it in memory when not remembered, register it for redaction, roll back the row when the credential cannot be written, and "Connect without remembering" after `NoKeyring`. Verified by `cargo test -p cv-app` with the fake connector and the in-memory secret store: added folder, missing folder, nested folder refused, keyring failure, no password in the settings file or the database file, a short password never in a formatted error.
- [ ] 3.2 Reconnect remembered shares at start before the folder health check and the start-up scan, in parallel per server with a 15-second limit, letting folders on other volumes start at once; record the failure code; scan a share's folders after a later successful connect. Verified by tests with a fake connector that is slow, fails and succeeds, checking the order of events and that no title of a reachable share is ever marked offline at start.
- [ ] 3.3 Offline reasons in folder health ("Wrong password", "Server not reachable", "Share not found", "Password needed") and the macOS rule that a share mount folder that is not a mount point does not answer. Verified by tests: a failed share's folders are offline with the reason, their titles are never Missing after a scan, and an empty unmounted mount folder is offline.
- [ ] 3.4 Group folders below a share's local root under the drive `\\server\share` in `drives.rs`. Verified by tests for a macOS mount folder and a gvfs path with two library folders each.
- [ ] 3.5 Forget unused shares: mark on removal of the last folder, clear on Undo, sweep one minute later and at start, wait for jobs on the share, disconnect (tolerating open files), delete the credential and the row; at start, unmount and `rmdir` CineVault mount folders with no row, never recursively. Verified by tests for Undo within the minute, removal with a running scan, a failed disconnect, an orphaned mount folder that is still mounted (left alone) and one that is not (removed only when empty).

## 4. Commands

- [ ] 4.1 Add the share commands in `cv-ipc` and `src-tauri` (capabilities of this OS, connect and add, connect again, cancel, list shares with their state), with an input type whose `Debug` hides the password; regenerate `ui/src/bindings.ts`. Verified by `cargo test -p cinevault-app` (bindings up to date) and a test that the logged command input holds no password.
- [ ] 4.2 Answer the share commands with `not_applicable` in `cv-web`'s dispatcher. Verified by `cargo test -p cv-web` (parity test and a request for each command).

## 5. Interface

- [ ] 5.1 Add the Network share page (fields, paste parsing, path preview, Show/Hide, Remember with the store's name, busy state with Esc to cancel, inline errors and banners, the Linux explanation, reconnect mode) and show it on `/gallery`. Verified by vitest tests for each state and error, and `bun run lint`, `typecheck`, `format:check`.
- [ ] 5.2 Add "Network share…" to Settings › Library folders and the onboarding Folders step, the offline reason and "Connect…" on folder rows, and the "on \\nas\movies as alex" line. Verified by vitest tests for the section and the step with a fake backend.
- [ ] 5.3 Add the browser popover for "Network share…" with the installer command and Copy. Verified by a vitest test in server mode that shows the popover and no password field.
- [ ] 5.4 Refresh the screenshots that show Library folders (`bun run shots`, `.github/assets/`, `site/img/`), the landing page wording and the README where Library folders are described, and add the `CHANGELOG.md` line with `bun run site:changelog`. Verified by `bun run site:changelog -- --check` and the refreshed images in the pull request.

## 6. Installer and server guide

- [ ] 6.1 `install.sh` for the Linux service: accept share addresses in the folder questions and `CINEVAULT_MEDIA`, ask user and hidden password (or `CINEVAULT_SHARE_USER` and `CINEVAULT_SHARE_PASSWORD_FILE`), install `cifs-utils` after asking, write the root-only credentials file and the mount unit for `/mnt/<server>-<share>`, add `Wants=`/`After=` and the web root to the drop-in, start and check the mount. Verified by a `--self-test` mode for the address parsing, unit names and file contents, `sh -n` and shellcheck, and a manual install on a Linux machine or VM against a Samba share (needs a person): mount works, credentials file is `0600` root, the service starts with the NAS off.
- [ ] 6.2 `install.sh --docker`: on Linux reuse the host mount and bind it with `rslave`; on macOS create the CIFS volume through the Docker Engine API with the body on standard input. Verified by self-tests for the generated bind and JSON body, and a manual run on Linux and on a Mac with Docker Desktop (needs a person): the container sees the share at `/media/<name>`, a rename succeeds, and no `docker` command line holds the password.
- [ ] 6.3 `install.ps1`: accept share addresses, read the password with `Read-Host -AsSecureString`, create the CIFS volume through the `docker_engine` pipe, mount it at `/media/<name>`. Verified by a `-SelfTest` switch for parsing and the JSON body, and a manual run with Docker Desktop on Windows (needs a person).
- [ ] 6.4 Remove the mounts, credentials files and volumes of shares dropped by "Change folders" and by uninstall, using `rmdir` only after `mountpoint -q` says not mounted. Verified by self-tests of the removal plan and the manual runs of 6.1 to 6.3 repeated with a share dropped.
- [ ] 6.5 Document shares in the README server guide and `docker-compose.yml` (commented CIFS volume with `.env` values, the manual systemd mount). Verified by `bun run check:scripts` and by following the compose example once against a test share (needs a person).

## 7. Integration checks

- [ ] 7.1 Full checks: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, the UI suite and build, `openspec validate connect-network-shares --strict`. Verified by the commands passing.
- [ ] 7.2 End-to-end on Windows with a throwaway data folder and a real SMB share (needs a person): connect with Remember on, scan, rename one title and undo it, restart the computer and see the titles online, change the NAS password and see "Wrong password" with "Connect…", reconnect, remove the last folder and confirm the credential is gone from Credential Manager, and search the log folder for the password. Verified by the recorded results and screenshots.
- [ ] 7.3 Data-safety review of the forget and mount-folder cleanup paths (no recursive delete, no delete while mounted) and of every place a password passes. Verified by the review notes in the pull request and a test for each finding.

## Workflow follow-up

- Archive the change after merge with `/opsx:archive`, which creates `openspec/specs/network-shares/spec.md`.

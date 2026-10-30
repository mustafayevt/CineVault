# Tasks

## 1. Command layer and shared context (no user-visible change)

- [x] 1.1 Create `crates/cv-ipc` with `AppContext` (every service and state object built once), the `EventSink` trait, sink-based `ProgressHub` and `NoticeHub` (with a bounded replay ring) and the shared error envelope. Verified by `cargo test -p cv-ipc` and the desktop app opening the fixture library.
- [x] 1.2 Move the library read commands and DTOs into `cv-ipc`, leaving one-line Tauri wrappers. Verified by the moved library tests and a desktop run through the Library page.
- [x] 1.3 Move the page read commands (movie, show, season, episode, people, collections, history, drives, source health, activity) and their DTOs. Verified by the moved tests and each page opening in the desktop app.
- [x] 1.4 Move the jobs, notices, move-progress, rating-sweep, tool-install and update channel commands onto `ProgressHub`/`NoticeHub`. Verified by the channel tests and a desktop scan whose progress card updates.
- [x] 1.5 Move the provider stream engine and its state onto the sink, deleting the Tauri channel adapters from the engine. Verified by a desktop scrape that still streams candidates and by the stream tests.
- [x] 1.6 Move the scan command module. Verified by the scan tests and a desktop scan, rescan and media info read.
- [ ] 1.7 Move the match, scrape, preview and artwork command modules. Verified by the match tests and a desktop scrape and artwork change.
- [ ] 1.8 Move the rename, organize, edit, NFO, collections and lifecycle command modules. Verified by the rename and edit tests and a desktop rename with Undo.
- [ ] 1.9 Move the library-ops (delete and moves), torrents, tools, updates and onboarding command modules. Verified by their tests and a desktop delete, move and tool install.
- [ ] 1.10 Add the `Platform` trait for desktop-only actions and mark the desktop-only commands in one list. Verified by `cargo test -p cv-ipc` and the desktop app still opening folders and starting a player.
- [ ] 1.11 Add `CommandRegistry` with generated argument structs (`camelCase`, `deny_unknown_fields`), the channel argument, sync/async and the desktop-only flag; add the parity test against a separate server table. Verified by deleting one dispatcher arm and one argument and seeing the test fail.
- [ ] 1.12 Rewrite the tests that build Tauri state (`app.state::<X>()` and the mock builders) against `AppContext`, and move the event tests to `EventSink` fakes. Verified by `cargo test --workspace`.
- [ ] 1.13 Update `AGENTS.md` and `CONTRIBUTING.md` for the crate layout, `AppContext`, the registry and the one-implementation rule. Verified by `bun run check:scripts` once it exists (task 2.10) and by a review of both files.

## 2. Server foundation

- [ ] 2.1 Create `crates/cv-web`: bind the port first, answer `/healthz` 503 then 200, serve the UI, handle graceful shutdown and remove the owner file on exit. Verified by starting it with `CINEVAULT_DATA_DIR` and observing 503, then 200, then a clean stop.
- [ ] 2.2 Add `ui/src/lib/ipc.ts` with the server flag, the two flags (`isTauri` for the backend, `transport()` for the desktop-only seams), the Tauri-internals shim (invoke, callbacks, metadata, `convertFileSrc`, event plugin) and the AppShell contract test. Verified by `bun run typecheck`, the shell render test, `bun run dev` still using the synthetic backend, and a server page loading the shell.
- [ ] 2.3 Add `POST /api/invoke` with the generated dispatcher, the `{ok,data}`/`{ok,error}` envelope, the 1 MiB command cap, blocking work on a blocking thread and fail-closed CSRF. Verified by contract tests for success, a command error, an unknown command, a missing token and a foreign `Origin`.
- [ ] 2.4 Add the welcome password page (30-minute window, first-browser claim, no token), argon2id storage in `server-auth.json` through `write_atomic_with_mode(0600)`, sessions (30-day idle, 90-day absolute, 32 cap), sign-out and the 401 `signed_out` value. Verified by tests for first run, second browser, expired window, wrong password, lockout, sign-out and the session cap, and by signing in from a browser.
- [ ] 2.5 Add `CINEVAULT_WEB_TRUST_PROXY`, the effective scheme and client address, `Secure` cookies, the `Origin` check against the effective host, the global rate-limit ceiling and the plain-HTTP warning. Verified by tests for a trusted and a spoofed `X-Forwarded-Proto`, a proxy address that cannot lock out every client, and the warning on the welcome and sign-in pages.
- [x] 2.6 Read `CINEVAULT_DATA_DIR` before any per-user folder is looked up, add `LocationMode::Environment` and the `resolve_with_env` pure function. Verified by tests for absolute, relative, portable and pointer precedence and a test that starts with no `HOME`.
- [ ] 2.7 Add `library-owner.json` (role, pid, host, protocol, heartbeat), take and check it on every platform, remove it on clean stop and take over a stale one after 30 seconds; make the CLI exit 2 when the owner is fresh but the endpoint is unreachable; republish the automation endpoint from the server. Verified by tests for desktop refusal, CLI refusal, stale takeover and a same-host forwarded batch.
- [ ] 2.8 Add the server secrets backend: `secrets.enc` (ChaCha20-Poly1305), the master key from env/file/systemd credential/Docker secret, the "keys locked" health state, redaction and key rotation. Verified by tests for save/restart, missing key, wrong key and rotation, and by a browser key save read back after a restart.
- [ ] 2.9 Add the Server section to Settings with the address, version, session lifetime, password change, sign-out every session, permanent-delete toggle, Kodi connection and proxy state; hide the desktop-only settings in a browser. Verified by the Settings tests and a browser run.
- [ ] 2.10 Add `server:dev`, `server:build`, `server:image`, `server:package`, `server:tarball` and `check:scripts`, and list them in `AGENTS.md`. Verified by running each command and by breaking a documented command name and seeing `check:scripts` fail.
- [ ] 2.11 Add the HTTP contract harness (test server, signed-in client, CSRF helper, SSE client) used by groups 3 to 6. Verified by the harness's own tests.

## 3. Read-only parity

- [ ] 3.1 Serve the library page: window and rows, counts, size totals, source groups, facets, filter catalogue and people search, with session and CSRF. Verified by contract tests and the fixture library rendering in a browser with correct counts.
- [ ] 3.2 Serve the title pages: movie, show, season, episode and people. Verified by contract tests and opening a movie, a show and an episode in a browser.
- [ ] 3.3 Serve collections, history, drives, source health, the review inbox, the Activity list and the settings read model. Verified by contract tests and each page opening in a browser.
- [ ] 3.4 Serve images at `/img/<key>/<size>`: session required, private immutable cache, ETag, 304, 404, 405, HEAD, `full` original and the cached-size stand-in. Verified by header tests, a signed-out request, an unknown key and a browser grid whose posters load.
- [ ] 3.5 Serve the SSE stream: per-page client id, monotonic ids, `Last-Event-ID` replay, `reset` on overflow, five-second reconnect and closing on sign-out or password change. Verified by ordering, replay, overflow, idle and sign-out tests.
- [ ] 3.6 Serve the browser shell: deep links, browser back and forward, no window controls, `index.html` no-store, immutable assets and the build-id reload. Verified by navigation tests and a browser run.
- [ ] 3.7 Guard every route: a test matrix that asks each page, API, image, media and stream address without a session and gets `signed_out`. Verified by the matrix passing.

## 4. Management loop

- [ ] 4.1 Serve Scan, Rescan, Find missing and media info with progress, cancel and a start call that returns before the work finishes. Verified by contract tests and a browser scan of the fixture folder whose result matches a desktop scan.
- [ ] 4.2 Serve Scrape: candidate streams, preview, accept, confident accept and undo. Verified by contract tests and a browser scrape whose NFO and library rows match a desktop run.
- [ ] 4.3 Serve the artwork picker, candidate streams, apply and undo, and the upload route (32 MiB, content sniffing, preview before apply, cross-volume atomic write, leftover cleanup). Verified by upload tests and a browser artwork change whose old file lands in the trash.
- [ ] 4.4 Serve Rename: preview, conflicts, apply and undo through the journal. Verified by contract tests and a browser rename and undo compared with a desktop run.
- [ ] 4.5 Add setup's folder write probe and the "This folder is not writable by the server." message. Verified by a test with a read-only fixture folder and a browser setup that refuses it.

## 5. Remaining writes

- [ ] 5.1 Serve bin/trash delete with no dialog, and permanent delete with the server-minted one-time intent, the current password and the server setting that enables it. Verified by tests for each destination, a replayed intent, a wrong password and a browser delete that can be restored.
- [ ] 5.2 Serve Move to another library folder with progress, cancel, interruption recovery and the busy slot. Verified by contract tests, a browser move and a second move refused as busy.
- [ ] 5.3 Serve metadata editing, watched state and rating changes. Verified by contract tests and a browser edit whose NFO and rows match a desktop run.
- [ ] 5.4 Serve NFO read and rewrite, NFO undo, collections and collections undo, and History undo. Verified by contract tests and browser actions whose files match a desktop run.
- [ ] 5.5 Serve settings writes, provider key save/test/remove and key rotation from Settings. Verified by contract tests and a browser key save read back after a restart.
- [ ] 5.6 Serve torrent search streams, the notice, source settings, the `.torrent` download route, the browser hand-off and the indexer-manager route. Verified by contract tests, a browser search and a hand-off that reaches a client torrent app.
- [ ] 5.7 Serve tool install streams, the updates read model and the server update instructions. Verified by contract tests and a browser tool install.
- [ ] 5.8 Add multi-client attribution: every job records its session and client, the UI says "Started from another session", and cancel/pause from another session asks first. Verified by a two-tab test and a two-tab browser run.
- [ ] 5.9 Serve the Kodi connection (host, port, credentials, path mapping), the library-update call after a batch and "Open in Kodi". Verified by tests against a mock Kodi and a browser run.

## 6. Browser replacements

- [ ] 6.1 Add the server folder browser: folder-only listing, full path, parent, make-folder with CSRF and dot-directory refusal, default roots (data and library folders), `/` opt-in and the root jail. Verified by tests for traversal, symlinks, a typed path, make-folder and a path outside the roots.
- [ ] 6.2 Wire the first-run setup to the folder browser and the write probe, with no folder drop in a browser. Verified by the onboarding tests and a fresh browser setup.
- [ ] 6.3 Add browser playback: the media route with ranges, a video content-type allowlist, nosniff, a sandboxing CSP, the library-folder check, the download attachment, disc folders and the cannot-play message. Verified by range tests, a traversal test, a browser play with seeking and a download.
- [ ] 6.4 Mark the desktop-only commands `not_applicable` and hide or disable their controls through `transport()`: window, zoom, pickers, open folder, open page, player, torrent open, indexer manager, installer. Verified by the parity test's desktop-only list and a browser page without those controls.
- [ ] 6.5 Add the folder-drop message "Use Add folder…" in a browser and keep the desktop drop. Verified by a DOM drop test and a desktop drop.

## 7. Packaging, scripts and documentation

- [ ] 7.1 Add the multi-stage `Dockerfile` with a non-root user, `PUID`/`PGID`, read-write volumes, `.deletedByCineVault` as the default destination and the health check. Verified by building the image and scanning, renaming and writing artwork in it.
- [ ] 7.2 Add the master-key delivery for the image (Docker secret) and the packages (systemd credential), and the "keys locked" health state. Verified by a container test that saves a key, restarts and reads it back.
- [ ] 7.3 Add the Docker Compose and systemd unit examples with the volumes, permissions and secret. Verified by starting both examples and completing the first-run setup.
- [ ] 7.4 Add `cinevault-server` `.deb` and `.rpm` packages containing the server, the CLI and the UI assets, with distinct names from the desktop packages. Verified by installing each in a clean container and starting the service.
- [ ] 7.5 Add the tarball (server, CLI, UI, unit file) and a start script. Verified by unpacking it on a clean container without a package manager and starting it.
- [ ] 7.6 Add the `linux-aarch64` tool manifest entry, or drop arm64; build amd64 on pull requests and arm64 on the release workflow with a native runner. Verified by a media info read in the arm64 image, or by the release not offering arm64.
- [ ] 7.7 Publish the image to `ghcr.io/<owner>/cinevault` with semver and `latest`, attach the packages and tarball to the release, and extend `check-version.mjs` to the image tag and package versions. Verified by a dry run of the workflow and the version check.
- [ ] 7.8 Run `bun run notices` for the new dependencies and verify the notices check. Verified by `bun run notices` and the resulting `THIRD_PARTY_NOTICES.md` diff.
- [ ] 7.9 Write the "Run on a home server" page, the README section, the landing-page copy, `CHANGELOG.md`, the regenerated `site/changelog.html` and the browser screenshots. Verified by `bun run site:changelog -- --check`, `cd ui && bun run shots`, and reading the page against the running container.
- [ ] 7.10 Add the Linux CI container job that builds the image, runs the smoke test (scan, rename, artwork, health) and the package install test. Verified by the job passing on the branch.

## 8. Integration checks

- [ ] 8.1 Run the full parity sweep on the fixture library in a real browser: every page, action and stream, compared with the desktop app. Verified by no remaining gaps; a gap blocks release and gets a ticket.
- [ ] 8.2 Run the security review: sign-in, setup window, sessions, CSRF and `Origin`, proxy trust, rate limits, request and connection caps, path jails, uploads, media, image headers, owner file and secrets. Verified by the review notes and a test for every finding.
- [ ] 8.3 Run the performance budgets over HTTP with a 5,000-title fixture, including a NAS-class cold thumbnail fill and a grid scroll; add the HTTP probe the budgets lack. Verified by `bun run perf:budgets` and `bun run perf:grid` against the server within budget.
- [ ] 8.4 Run the interruption tests for uploads, renames, moves and deletes: kill the server mid-write and check that no file is half written and the next start recovers. Verified by the tests and a byte-for-byte file comparison.
- [ ] 8.5 Check a real NAS and a Kodi HTPC by hand: add the folder, scan it, scrape, rename, browse from the HTPC, play a file and update Kodi's library. Requires a human with the hardware. Verified by the written result against the issue's setup.
- [ ] 8.6 Upgrade a running container to the next version, confirm the data survives, then restore a pre-upgrade copy after a forced failed migration. Verified by the upgrade report and the restored library.

# Tasks

## 1. Commands and settings a browser cannot use

- [x] 1.1 Remove `set_tool_path` from `cv_ipc::commands::COMMANDS` and make its `cv-web` dispatcher arm answer `not_applicable("A tool's path is set on the computer that runs CineVault.")`. Verified by `the_desktop_only_commands_answer_not_applicable`, the parity check and a dispatcher test that the file is not run and the path is unchanged (`cargo test -p cv-web -p cv-ipc`).
- [x] 1.2 Add `HOST_SETTINGS` and the guard before `set_setting` and `reset_settings`: refuse the update channel and key, `tools.path.*` and `player.*`; accept `files.trash-folder` and `metadata.nfo-collection-folder` only when empty or inside `allowed_roots` (canonicalised, component-wise); a browser reset skips the refused keys. Verified by dispatcher tests for each key, `..` and a link out of the roots, a path inside the roots, and a reset that keeps a custom channel.
- [x] 1.3 Add the test that walks the `mono` text settings in `ui/src/features/settings/schema.ts` and fails for a key that is neither in `HOST_SETTINGS` nor in the reviewed allow-list. Verified by the test passing, and failing when a made-up `player.example-path` key is added to a copy of the schema in the test.
- [x] 1.4 UI: no change needed. Neither surface has a control for the update channel, the update key or a tool's path (the schema lists the two update settings but no section renders them, and `setPath` of the tools backend has no caller), so there is nothing to make read-only; the settings delta says so instead. The tools' downloads from the first-run setup are unchanged. Verified by reading `ui/src/features/settings` and `ui/src/features/tools` (no `setPath` caller, no `update-channel` control).

## 2. Home network behind a proxy

- [x] 2.1 Add `network::forwarded(headers)` and use it in `from_home_network` and `home_network_guard`, with the one-time warning. Verified by `cv-web` tests: welcome, setup and reset refused from `127.0.0.1` with each header, accepted with a trusted proxy, accepted with no header from `172.17.0.1`, the desktop guard refusing a forwarded loopback request, and one log line for three requests.

## 3. Sign-in limits

- [ ] 3.1 Make `Auth::login` and `check_password` async: provisional failure recorded under the lock, a two-permit semaphore, Argon2 in `spawn_blocking`, the record cleared on success; the global ceiling applies only outside the home network. Verified by `auth` tests: five failures block an address; 100 public failures do not block a home-network sign-in but block a new public address; ten concurrent wrong attempts from one address leave at most five checked; a right password after four wrong ones signs in; the failures lock is not held during the check (a second address is answered while a check runs).

## 4. Origin and framing

- [ ] 4.1 Keep `origin_matches` as it is and add tests for an invoke without `Origin` (runs with the token, 403 without it) and with a foreign `Origin` (403). Verified by `cargo test -p cv-web`.
- [ ] 4.2 Add the response layer that sets `X-Frame-Options: DENY` and `frame-ancestors 'none'`, merging it into `MEDIA_CSP`. Verified by router tests on the shell, the sign-in page, the welcome page, an image, a media range and a 404, each with both headers and a single CSP header.

## 5. Sessions kept across restarts

- [ ] 5.1 Move `Session` expiry to wall-clock times and key the map by the SHA-256 of the cookie value. Verified by the existing `auth` tests and a test that the map never holds a cookie value.
- [ ] 5.2 Add the `server-sessions.json` store: load with expiry and the password fingerprint, write atomically with mode `0600` on sign-in, sign-out, sign-out-all, eviction and password change, uses written at most every 5 minutes, a damaged file set aside as `.bad`. Verified by tests: a session survives a new `Auth` over the same folder; a changed `server-auth.json` drops every session; an expired one is dropped on load; a damaged file starts empty and is kept aside; the file mode is `0600` on Unix; the file holds no cookie value.
- [ ] 5.3 In `embed`, `begin_stop` (quit) keeps sessions and `stop` (switch off, port change) ends them. Verified by embed tests: sign in, `begin_stop`, start again, the cookie still works; sign in, `stop`, start again, the cookie is signed out.

## 6. Documentation and integration

- [ ] 6.1 `packaging/README.md`: the trusted-proxy paragraph says a proxied request is outside the home network unless listed, and how to set a tool path in `settings.json` while the server is stopped; `CHANGELOG.md` lines under the unreleased version and `bun run site:changelog`. Verified by `bun run site:changelog -- --check`.
- [ ] 6.2 Full checks: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, the UI suite and build, `openspec validate harden-browser-access --strict`. Verified by the commands passing.
- [ ] 6.3 Real run with a throwaway data folder: the headless server behind a local reverse proxy without and with `CINEVAULT_WEB_TRUST_PROXY` (setup refused, then allowed), restart with a signed-in curl session (still signed in), `set_setting` for the update channel refused, headers checked with curl; and on Windows, the desktop app's Browser access survives a quit and start with a signed-in headless browser. Needs a person for the desktop part. Verified by the recorded results.

# Tasks

## 1. Server as a library

- [x] 1.1 Split `cv-web` into a library and the `cinevault-server` binary: `lib.rs`, `server.rs` (router and handlers), `network.rs`; the binary keeps argument parsing, `passwd`, boot and signals. Verified by `cargo test -p cv-web` and a start with `CINEVAULT_DATA_DIR` answering `/healthz`.
- [x] 1.2 Add `UiAssets` (a folder or an embedded resolver) behind the static route, keeping the traversal and session rules. Verified by unit tests for both kinds, a traversal attempt and a deep link.
- [x] 1.3 Add `network::is_home_address` and the `lan_only` guard layer. Verified by tests for every range, IPv4-mapped IPv6, a public address, and a guarded router answering 403 to a public peer.

## 2. Sign-in rules

- [x] 2.1 Replace the 30-minute welcome window with the home-network rule and a lapsing claim; add the "Finish setup from your home network" page. Verified by tests for a private peer, a public peer, a spoofed `X-Forwarded-For`, a trusted proxy, a claim that lapses and a long-running server without a password.
- [x] 2.2 Add the PIN reset (`/api/reset/start`, `/api/reset/finish`, the PIN file, constant-time compare, five attempts, 15-minute expiry, one-minute cooldown, sessions ended, browser signed in) and the sign-in page's "Forgot password?". Verified by tests for each rule, the file mode on Unix, and the log line without the PIN.
- [x] 2.3 Add the desktop host mode: no welcome page, the "Change it in CineVault's Settings on <computer>" text, reset routes refused; session handles, address, browser and times; sign out by handle. Verified by unit tests.

## 3. Embedding

- [x] 3.1 Add `LibraryOpsState::sharing_moves_with` in `cv-ipc`. Verified by a test that a move started through one state is refused as busy through the other.
- [x] 3.2 Add `cv_web::embed`: `start` (password required, typed port-in-use error), `RunningServer::stop` (sessions, streams, listener, tasks), `BrowserAccessConfig` load and save, and the address list. Verified by integration tests: start, sign in, invoke, stop and port closed; restart on another port; port in use; no password; sessions closed on stop; a public peer refused with LAN-only on.

## 4. Desktop app

- [x] 4.1 Add `browser_access_ipc` with the commands (status, enable, port, LAN only, password, sign out, tray, autostart), start on launch, stop on exit; add the dispatcher's `not_applicable` arms; regenerate the bindings. Verified by `cargo test -p cinevault-app` (bindings and parity) and `cargo test -p cv-web`.
- [x] 4.2 Add the tray icon and hide-on-close while Browser access and "Keep running in the tray" are on; add the autostart plugin with `--background`. Verified by unit tests of the close decision and a manual run on Windows: closing hid the window while the server kept answering, a second launch showed it again, and the Run entry was added and removed. The tray icon itself still needs a person to look at it.

## 5. Interface

- [x] 5.1 Add the Browser access section (desktop only): switch, password, addresses with Copy and QR, port, state, LAN only, sessions, tray and autostart switches, firewall and help text; show the desktop hint in the browser's Server section. Verified by vitest tests for the section's states and by `bun run lint`, `typecheck`, `format:check`.

## 6. Integration checks

- [x] 6.1 Changelog entry and `bun run site:changelog`. Verified by `bun run site:changelog -- --check`.
- [x] 6.2 Full checks: `cargo fmt`, clippy with `-D warnings`, `cargo test --workspace`, the UI suite and build, OpenSpec validation. Verified by the commands passing.
- [x] 6.3 Real run on Windows with a throwaway data folder and library: switch on, open the address with curl and headless Edge, sign in, browse, play a small file, change the port, switch off and confirm the port closed; headless server first run and PIN reset with curl. Verified by the recorded results and screenshots.

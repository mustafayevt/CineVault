# Code review of browser mode

The final review read the whole branch against `main`, ran the targeted Rust and UI checks, and
built the server to test the riskiest routes live. This file records what it found and what was
done about it.

## Fixed before merging

- **Unauthenticated file read through the static handler.** The handler joined the raw request
  path and checked it lexically, so `/assets/../../data/library.db` escaped the interface folder
  and the `assets/` prefix skipped the session. It now rejects any non-normal path component,
  canonicalises the file and the interface folder, requires the canonical prefix, and only the
  hashed `assets/` files are public. `index.html` is `no-store`.
- **Upload and command body limits.** Axum's 2 MiB default rejected real posters. `/api/invoke`
  is capped at 1 MiB and `/api/artwork/upload` at 33 MiB (the artwork limit plus multipart
  overhead), with the artwork-specific 32 MiB message.
- **Streams outlived sign-out and shutdown.** A stream is now tied to its session; signing out,
  changing the password and "sign out every session" close that session's streams. SIGTERM is
  handled next to SIGINT, the stream registry is closed, `AppServices::shutdown()` pauses the
  jobs, and a 15-second cap stops a client holding the process open. A clean stop removes the
  owner file.
- **Sign-in rate limiting.** The check, the password verification and the failure record happen
  under one lock; there is a global ceiling of 100 failures a minute; the forwarded-for chain is
  walked from the trusted end, so a client cannot rotate the header to bypass the limit.
- **Folder picker roots.** The listing returns the allowed roots (the data folder and the library
  folders, plus `/media`, `/mnt` and `/srv` on Linux), the picker shows them and accepts a typed
  absolute path, so `/media/movies` is reachable on the documented setup.
- **A second move replaced the first's pause and cancel.** A move now refuses a second start
  while one runs and only clears its own control when it ends.
- **Provider keys while the key store is locked.** `set_provider_credential` is refused with the
  sentence that names `CINEVAULT_SECRET_KEY_FILE`, instead of accepting a key into memory that
  vanishes on restart.
- **Origin behind a reverse proxy.** The effective host falls back to `Host` and compares with
  the default ports removed, so a proxy that does not set `X-Forwarded-Host` does not refuse every
  write.
- **Images were cached publicly.** The browser image route rewrites the desktop protocol's
  `public` cache to `private` and varies on the cookie.
- **History and Drives did not refresh.** The server broadcasts `history-changed` and
  `drives-changed` after the commands that change them.
- **Browser hand-offs.** Torrent magnets, pages and `.torrent` files open in the client's browser;
  rating links open a tab; "Open folder" says the folder is on the server instead of doing
  nothing.
- **Artwork sources.** A browser may only place an image it uploaded to the server's staging
  folder; a `file:` address or another server path is refused, and the staged file is removed.
- **Job attribution.** A job records the browser session that started it, the progress DTO carries
  `started_by`, and the progress card says "Started from another session".
- **Logs and warnings.** The server uses the shared redacting logging initialiser, and the sign-in
  and welcome pages warn on plain HTTP when the host is not loopback.

## Known gaps (follow-ups, not hidden)

- **Playback actions.** Play opens the browser's player with seeking and the media route answers
  ranges; "Copy stream link" and "Open in Kodi" are not in the UI yet, and Kodi is not configured
  in the server settings.
- **`/healthz` always answers 200.** The listener is bound after the library opens, so a slow open
  is connection-refused rather than 503.
- **`server.json`, key rotation, session lifetime and trusted-proxy state in Settings.** The
  address and port come from the environment; the settings page shows the running values but does
  not edit them.
- **SSE reset.** The server replays by `Last-Event-ID` and sends a `reset` when its buffer is too
  small, but the page does not yet reload on that event, and `end` is not sent.
- **Deep links and CSRF after a cached page.** `index.html` is `no-store`; a deep link is still
  lost after signing in.
- **`AppContext` parity.** The server builds its state in `cv-web/src/state.rs`; the desktop still
  builds its own in `src-tauri/src/main.rs`, so the "one constructor" goal is not met yet.
- **Packaging.** The release builds amd64 only; arm64 needs the `linux-aarch64` tool manifest and
  a native runner. The container smoke test covers `/healthz`; the scan/rename/artwork smoke test
  is still to add.
- **Not tested here.** A real NAS, a real Kodi HTPC and a Docker daemon (unavailable on this
  machine). Those are the final human checks.

## Verification

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt`.
- `bun run typecheck`, `bun run lint`, `bun run test` (1209), `bun run build`.
- Live checks against a fixture: traversal (`/assets/../../etc/hosts` → 404), signed-out pages
  redirect, 3 MiB upload accepted, sign-out closes a stream, SIGINT stops the server and removes
  the owner file, permanent delete refuses a wrong password and a mismatched intent.

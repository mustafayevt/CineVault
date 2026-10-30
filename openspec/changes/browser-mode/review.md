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

## QA pass before 1.2.0

A second pass ran the server for real: natively on Windows (debug build, `CINEVAULT_WEB_ROOTS`
set, a disposable Kodi-style library of generated MP4/MKV files) and in Docker Desktop
(`docker compose up` with the image built from this branch, `PUID`/`PGID` 1000, a Docker-secret
master key), driven from a throwaway headless Chrome over CDP and with curl. No TMDB key was
available to the server, so live scraping was not exercised.

### Bugs found and fixed

- **The server stopped itself after 15 seconds.** The shutdown cap wrapped the whole serve future,
  so every start exited cleanly 15 seconds later (and `restart: unless-stopped` looped). The cap
  now starts at SIGINT/SIGTERM.
- **Add folder… crashed to a blank page.** The picker reads `roots` from `/api/fs/list`, which the
  route never sent. The listing now returns the existing roots, opens on a media root rather than
  the data folder, and shows plain paths (not `\\?\C:\…` on Windows).
- **Play showed an empty player.** The media route's bare `CSP: sandbox` gave the browser's player
  document an opaque origin, so its own video request was refused by CORS. It is now
  `sandbox allow-same-origin`. A range past the end answers 416 instead of the whole file.
- **Artwork upload never ran in a browser.** The server shim makes `isTauri()` true and the
  artwork backend asked that first, so a chosen or dropped file failed with "available in the
  desktop app". A file over 32 MiB is now refused with that sentence before sending (the server
  closes an oversized body, which the browser only reports as a network error).
- **Artwork source guard bypasses.** `place_artwork` compared lexically, so a quoted path, an
  upper-case `FILE:` address or `<data>/tmp/../file` passed it, and the last was deleted after the
  placement; `place_extra_artwork` had no guard. Both now accept a web address or a canonical
  `artwork-*` file in the staging folder only.
- **Job attribution never worked.** The client id was a thread-local set around an async command,
  lost (or mixed with another session's) after the first `.await`. It is a task-local now, carried
  to blocking threads by `cv_jobs::client::spawn_blocking`.
- **Media kept streaming after sign-out.** A playing or downloading file now ends when its
  session ends; other sessions keep streaming.
- **`cinevault-server passwd` needed a restart** to take effect while the server ran; the server
  now reads the password file on each check.
- **Open redirect on sign-in.** `?next=/\evil.example` passed the prefix check; the pages now resolve
  `next` and require the same origin. A deep link also keeps its query string through sign-in.
- **Documented master key crash-looped the container.** The guide said "a long random value";
  the server needs base64 of 32 bytes. The guide, compose file and unit now say
  `openssl rand -base64 32`, and the error names it.
- **UI format check** failed on the pull request; the files are formatted.

### Verified working

First-run welcome (setup window closes after, second browser refused, `/setup` redirects to sign-in
afterwards); sign-in, wrong password, the five-a-minute limit (a rotated `X-Forwarded-For` from an
untrusted peer does not bypass it); sign-out; deep link after sign-in; server restart with the page
open (the page returns to sign-in with its address); folder browser with typed paths and the root
jail (also in the container: `/etc` and `/media/../etc` refused); onboarding to a scanned library;
grid, table and detail pages; job progress over the event stream; Rename preview, apply and
History Undo on disk; artwork upload and Undo; delete to the trash folder and Restore, including,
after merging the named-companions delete from `main`, a title in a library folder's root whose
`.en.srt` and poster were added after the Scan (both went with it, another title's subtitle
stayed, Restore brought all three back) (in the
container, with no system bin, delete falls back to `.deletedByCineVault`); Play in Chrome with
range requests; two sessions streaming while one signs out; every API, image, media and stream
route without a session (401 or redirect); CSRF token, foreign `Origin`, spoofed
`X-Forwarded-Host`, `text/plain` bodies, 1 MiB command cap and 33 MiB upload cap; static and media
traversal; cookie flags (`HttpOnly; SameSite=Strict`, no `Secure` from a spoofed
`X-Forwarded-Proto`); no password, token or key in the logs or `settings.json`; provider key stored
only in `secrets.enc` and still there after a container restart; MediaInfo download in the
container; non-root server process (UID 1000), Docker health `healthy`, clean stop in under a
second with the owner file removed; the CLI refuses with exit 2 natively and in the container
while the server runs.

### Not verified here

- Live scraping and fixing a match in a browser (no TMDB key could be given to the server without
  copying secrets).
- Real Linux file permissions with a mismatched `PUID` (Docker Desktop bind mounts on Windows are
  world-writable), arm64, the `.deb`/`.rpm` and the tarball.
- The desktop app as a window: its unit tests pass and it builds, but the installed CineVault was
  running on the test machine and the single-instance guard hands a second copy to it.
- The "Started from another session" label on screen (the job's `started_by` is now right on the
  wire; scans of the fixture finish too fast to see the card).

## Known gaps (follow-ups, not hidden)

- **The CLI does not forward to the server.** It refuses with exit 2 ("send the batch through
  it"), but the server does not republish the automation endpoint, so a scheduled CLI batch
  cannot run while the server is up.
- **Phone width.** Sign-in and welcome fit a phone; the library keeps the full sidebar at 390 px
  (the proposal designs the library for a tablet or larger).
- **Wording.** Some texts still speak of "this computer" (the TMDB key step) or call a Docker
  bind mount a network drive in the delete note.

- **Playback actions.** Play opens the browser's player with seeking and the media route answers
  ranges; "Copy stream link" and "Open in Kodi" are not in the UI yet, and Kodi is not configured
  in the server settings. A stream link needs a token in the address first: the media route takes
  only the session cookie, so a copied link would not play in Kodi or VLC.
- **`/healthz` always answers 200.** The listener is bound after the library opens, so a slow open
  is connection-refused rather than 503.
- **`server.json`, key rotation, session lifetime and trusted-proxy state in Settings.** The
  address and port come from the environment; the settings page shows the running values but does
  not edit them.
- **SSE `end`.** The page reloads on `reset` (d03990f8); the server still does not send `end`.
- **`AppContext` parity.** The server builds its state in `cv-web/src/state.rs`; the desktop still
  builds its own in `src-tauri/src/main.rs`, so the "one constructor" goal is not met yet.
- **Packaging.** The release builds amd64 only; arm64 needs the `linux-aarch64` tool manifest and
  a native runner. The container smoke test covers `/healthz`; the scan/rename/artwork smoke test
  is still to add.
- **Not tested here.** A real NAS and a real Kodi HTPC. Those are the final human checks; the
  container itself was run under Docker Desktop in the QA pass above.

## Verification

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt`.
- `bun run typecheck`, `bun run lint`, `bun run test` (1209), `bun run build`.
- Live checks against a fixture: traversal (`/assets/../../etc/hosts` → 404), signed-out pages
  redirect, 3 MiB upload accepted, sign-out closes a stream, SIGINT stops the server and removes
  the owner file, permanent delete refuses a wrong password and a mismatched intent.

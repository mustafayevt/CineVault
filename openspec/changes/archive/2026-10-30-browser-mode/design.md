# Design

## Context

See `proposal.md` for the motivation and the delivery stages. The constraints that shape the approach:

- All behaviour already lives in `cv-app` and the crates below it. The Tauri binary is an adapter: about 178 commands across 40 modules, 3 typed events, 22 `Channel` uses, and 26 `.manage()` state objects built in `src-tauri/src/main.rs`.
- The command handlers live in a binary crate, take `tauri::State`, and are not callable from another binary.
- The UI is a React SPA with 41 files referencing `isTauri()` and 25 of them having a second backend. It already runs without Tauri for development. Images are content-keyed URLs, not IPC bytes.
- `cv-artwork::img::ImageService` already speaks HTTP (method, path, `If-None-Match` → status, headers, body), and `provider_stream` already takes a `Sink`. The plumbing is closer to transport-neutral than the handler layer.
- The CLI forwards a batch to a running app over a local socket with a `0600` endpoint file and a token. The Linux instance lock exists but is not taken on macOS or Windows and holds no owner identity.
- A throwaway spike (reverted) proved that the unmodified UI, with `window.__TAURI_INTERNALS__` backed by HTTP, rendered a real 12-title library in headless Chrome with zero console errors. Opening a title then failed on the first unhandled command, which is the per-command work this design plans.

The four architecture, security, product and spec reviews changed several central decisions. Those changes are recorded below rather than hidden.

## Goals / Non-Goals

**Goals:**

- One implementation of every command, called by two adapters.
- A server with no Tauri, WebKit or display dependency.
- The browser experience is the desktop experience, including writes, streams and failure behaviour.
- A first run a home-server owner can finish without reading logs or installing a keyring daemon.
- Security and data safety in the first release, not a follow-up.

**Non-Goals:**

- Multi-user, roles or per-user libraries.
- Server-side transcoding.
- Windows and macOS server builds.
- Moving the desktop app off Tauri.
- A mobile layout beyond sign-in, first-run and the player.

## Decisions

### D1. Extract the command layer into a Tauri-free `cv-ipc` crate

`cv-ipc` owns the handler logic, the DTOs and four pieces the first draft missed:

- `AppContext`, which builds and holds every service and state object once (the 26 `.manage()` calls become one constructor). The desktop and the server both use it, so a state object cannot be wired in one surface and forgotten in the other.
- `EventSink`, a trait with `emit(event, payload)`. `cv-app` and `cv-ipc` publish through it. `src-tauri` implements it with `tauri::Emitter`, `cv-web` with the stream. This avoids the orphan-rule problem: the three `tauri_specta::Event` types stay in `src-tauri`.
- `ProgressHub` and `NoticeHub`, sink-based. The job feed, the move progress, the notice backlog and the taskbar mirror all go through them. The notice hub keeps a bounded ring and replays it to new subscribers, because the current `publish` drops notices when a sink is connected and a second tab would miss them.
- `CommandRegistry`: for each command, the name, a generated args struct (`#[serde(rename_all = "camelCase", deny_unknown_fields)]`), which argument is a channel, whether it is sync or async, and whether it is desktop-only.

`src-tauri` keeps one thin `#[tauri::command]` wrapper per command plus the `Platform` trait implementation (opener, picker, window, taskbar, updater). The desktop-only commands stay there and are marked in the registry.

*Alternatives:* make `src-tauri` a library and depend on it from the server (links Tauri and WebKit, defeats the container); duplicate handlers (the double work this design prevents).

### D2. One UI transport shim, gated by an explicit server flag

The shim is imported first in `main.tsx` and does nothing unless the server injected `window.__CINEVAULT_TRANSPORT__ = "server"`. `bun run dev`, the gallery, the screenshot harness and vitest keep their synthetic backends because the flag is absent.

When it is present, the shim installs `window.__TAURI_INTERNALS__` with `invoke`, `transformCallback`, `unregisterCallback`, `convertFileSrc`, `metadata` and the `plugin:event|*` commands, and keeps `@tauri-apps/api`'s real `Channel` class working: its `toJSON` yields `__CHANNEL__:<id>`, so the server can identify a channel without `instanceof`.

`bindings.ts` is not changed. The first draft assumed the generator header controls the imports; it does not, and post-processing a generated file would be brittle.

Two flags with different meanings:

- `isTauri()` stays true in server mode, so the feature backends take the real command path.
- `transport()` (new, exported from `ui/src/lib/ipc.ts`) is `"desktop"` or `"browser"`. The shell-only call sites move to it: window controls, zoom, native pickers, open-folder, the opener, drag and drop, the update installer, and the `desktopGuards` key/context-menu policy. There are about 16 such sites, not 39.

The contract test renders `AppShell`, not only the shim, and fails when `metadata`, a channel, an event or `convertFileSrc` changes shape in the pinned `@tauri-apps/api`.

*Alternatives:* a third backend per feature (25 fallbacks to duplicate and a new one per feature); rewriting `bindings.ts` imports (not supported by the pinned generator).

### D3. One invoke endpoint, a generated dispatcher, and fail-closed CSRF

`POST /api/invoke` takes `{cmd, args, client}` and answers `{ok, data}` or `{ok, error: {code, message}}`. The dispatcher is generated from `CommandRegistry`, and every command gets an args struct with exact field names and `deny_unknown_fields`, so an argument rename fails the contract test instead of failing silently at runtime. Channel arguments are extracted before deserialization and attached to the caller's stream.

Sync handlers run on a blocking thread; the HTTP worker is never blocked. Every invoke requires the session CSRF token, not only the writes: fail-closed is simpler than classifying 178 commands and cannot be wrong in the permissive direction.

Long-running commands do not hold one HTTP request. Scan, scrape, rename, moves, tool installs and folder analysis already run as jobs; the ones that await the whole operation (`apply_change_source`, `install_tool`, `organize_apply`, `analyze_library_folder`) gain a start call that returns an operation id and a stream for progress, with the existing job list as the reconciliation source after a reconnect.

The parity test does not compare a generated list to itself. The server dispatcher is a separate table, and the test compares both names and argument names against the desktop registry; deleting a real dispatcher arm fails it. Desktop-only commands are listed explicitly.

*Alternatives:* a REST resource API (a second contract, 178 routes, no parity by construction); CSRF only on writes (needs a classification that can be wrong).

### D4. One SSE stream per page, with resumable ids

Server-to-client streams use one `GET /api/stream?client=<id>` Server-Sent Events connection per page. The page mints a `client` id in the shim and sends it on the stream and on every invoke, so a command's channel registration is bound to that page's connection rather than to the shared session cookie.

Messages carry `id` (a monotonic per-client event id), `event` (`progress`, `notice`, `channel`, `end`) and a JSON payload with the stream id. The browser reconnects automatically and sends `Last-Event-ID`; the server replays from a bounded per-client ring. When a ring overflows, the server sends a `reset` event and the page reloads the job or stream state, so the consumer never silently doubles items. A dropped connection resumes within five seconds.

SSE replaces the first draft's WebSocket: it is server-to-client only, which is all the streams need, it gives `Last-Event-ID` replay for free, it uses one ordinary HTTP connection that the documented reverse proxies already handle, and it avoids a second protocol for auth and origin checks.

*Alternatives:* WebSocket (bidirectional is unused; replay has to be built by hand); long polling (latency and churn).

### D5. Friendly first run, single-user sessions, fail-closed writes

The first start with no password shows a welcome page: create a password (12 characters minimum) and confirm it. No setup token, no terminal. The setup pages exist only until a password is set and only for 30 minutes after the first start; the first browser to open them claims the setup with a short-lived setup cookie, and a second browser cannot take over. The server binds loopback by default; the welcome page warns when the address is not loopback. After the window, `cinevault-server passwd` on the server is the only reset.

The password is argon2id with a random salt, stored in `server-auth.json` in the data folder, written through a new `cv_fs::write_atomic_with_mode` with `0600` (the existing atomic write has no mode argument, which the first draft missed; a `chmod` after the write is a window where the hash is readable).

Sessions are random 256-bit ids held in memory, referenced by an `HttpOnly`, `SameSite=Strict` cookie, `Secure` when the effective scheme is HTTPS. Idle lifetime is 30 days and absolute lifetime 90 days; at most 32 live sessions, oldest evicted. Restarting signs everyone out. No signing key is stored.

Writes and every invoke require a double-submit CSRF token and a matching `Origin`. The token is issued with the session. Five failed sign-ins from one address in a minute are refused without a check. A 401 rejects with the typed value `{code: "signed_out"}` (not an `Error`, which `typedError` would rethrow), and the shell redirects to sign-in without losing the page.

Behind a reverse proxy the server sees HTTP. `CINEVAULT_WEB_TRUST_PROXY` names the proxy addresses (CIDRs); only requests from those addresses are allowed to set the effective scheme from `X-Forwarded-Proto`, the client address from `X-Forwarded-For`, and the origin host from `X-Forwarded-Host`. Without it, the socket peer is used, the cookie is not `Secure`, and the plain-HTTP warning shows. The sign-in rate limit has a global ceiling so a proxy address cannot lock out the household.

*Alternatives:* a setup token in logs (retained by Docker/journald, and unfriendly); HTTP Basic (no sign-out, no rate limit); reverse-proxy auth (a second product to configure, and it does not protect a native install).

### D6. An encrypted secrets file for the server, the OS store for the desktop

Both reviews showed the OS keyring is not a dependable headless backend: a systemd service has no session bus, and a NAS has no keyring daemon. The desktop keeps the OS credential store. The server adds a backend of its own:

- `secrets.enc` in the data folder, ChaCha20-Poly1305, one key per record, a versioned envelope and an authentication tag.
- The 32-byte master key comes from `CINEVAULT_SECRET_KEY` (base64), `CINEVAULT_SECRET_KEY_FILE`, a Docker secret or a systemd `LoadCredential`. It is never in the data folder, the image or `settings.json`.
- The file is written atomically with mode `0600`; the master key and every secret are registered with `redact`.
- When the master key is missing, the server starts and Settings says "Provider keys are locked: set CINEVAULT_SECRET_KEY_FILE and restart." It does not silently lose keys or fall back to plaintext.
- Rotating the key rewrites the file in one transaction.

This keeps the project rule's intent ("no key in plain text") while giving a home server a backend that survives a reboot without a second daemon.

*Alternatives:* Secret Service (no session bus on a NAS; fragile unlock); environment variables per provider (visible to `docker inspect`, no UI, no rotation); plaintext file (rejected).

### D7. `CINEVAULT_DATA_DIR` before anything that needs a home folder

`LocationInputs::from_environment()` asks `dirs::data_dir()` and `dirs::config_dir()` first, so a container without `HOME` fails before the environment variable is consulted. The resolver reads `CINEVAULT_DATA_DIR` before building those inputs; `resolve_with_env` is a pure function and the tests do not need the ambient environment. An absolute value wins over portable mode and the pointer file, with a new `LocationMode::Environment` that shares the non-portable credential service. A relative or empty value is refused with a warning and the next rule decides.

*Alternative:* a `--data` flag (does not reach a container's entry point as cleanly, and the CLI needs it too).

### D8. A portable owner file, checked by every surface

The Linux-only `flock` is not enough: the desktop does not take it on macOS or Windows, it carries no owner identity, and it is advisory on the SMB/NFS shares some users will try. Replace it with a portable owner protocol in the data folder:

- `library-owner.json` holds `{ role: "app" | "server" | "cli", pid, host, started_at, protocol, heartbeat }`, written atomically with `0600`.
- Every surface takes an OS lock *and* writes the owner file. The owner is fresh while its heartbeat is under 30 seconds old; a clean exit removes the file.
- The desktop app refuses a data folder whose owner is a fresh server, and says so in the window. The CLI tries the local automation endpoint; if the owner file says a server holds the folder and the endpoint is unreachable (for example a container socket the host cannot see), it exits with code 2 and a sentence naming the server, instead of opening the library itself.
- The server republishes the automation endpoint for the same-host `docker exec`/host CLI case. The docs say the data folder must be on a local filesystem; SMB/NFS is unsupported and the owner file says so when it detects a network mount.
- A stale owner (crashed process) is taken over after the heartbeat window.

*Alternatives:* keep `flock` only (two writers on a NAS); a database-level advisory lock (SQLite has none across processes); a separate daemon (too much for a home server).

### D9. Browser replacements and the desktop-only list

- **Folder browser.** Folder-only listing with full path, parent and make-folder. The default roots are the data folder and the existing library folders; `/` requires an explicit `CINEVAULT_WEB_ROOTS=/`. Paths are canonicalised and compared with `Path::starts_with`, never string prefixes. Make-folder is a POST with CSRF, refuses dot-directories and paths outside the roots, and has no undo (it creates an empty folder only). `add_data_source` uses the same guard, so the browser cannot bypass the roots through a command.
- **Artwork upload.** The request body is buffered in the data folder's `tmp` and validated (32 MiB cap, content sniffing, decode limits), then written through the existing `cv-artwork::download::files::store`, which stages the temporary file beside the target, moves the old file to the trash, persists and takes the old file back if the persist fails. Staging in `/data` and renaming to `/media` is `EXDEV`; the store already solves this. Leftover `.cvpart` files in media folders are swept on start.
- **Playback.** `GET /api/media/<item>/<part>` canonicalises the path, checks it is inside a library folder with `Path::starts_with`, re-resolves per request (a symlink planted after the scan cannot escape), answers ranges, and uses a video content-type allowlist with `application/octet-stream` for anything else, `X-Content-Type-Options: nosniff`, `Content-Security-Policy: sandbox` and `Content-Disposition: attachment` for downloads. Disc folders open their main index file when readable; otherwise Play is off with a sentence. A download route is separate and always `attachment`.
- **Play targets.** "Play in browser", "Copy stream link", and optional "Open in Kodi" (JSON-RPC `Player.Open`). If the browser cannot decode the file, it says so instead of failing a `<video>`.
- **Torrent hand-off.** A magnet opens through `window.open`; a `.torrent` is served from an authenticated keyed route so the browser downloads it; the toast says "Opening your torrent app…" and always offers "Copy magnet link" with a selection fallback for plain-HTTP pages where the clipboard API is unavailable. The indexer manager and own-Jackett cards show the server's address (proxied when configured), never the client's loopback.
- **Desktop-only commands.** `open_folder`, `pick_folder`, `pick_files`, `play_external`, `open_rating_page`, `open_logs`, `torrent_open_page`, `torrent_open_magnet`, `torrent_open_torrent_file`, `torrent_open_indexer_manager`, the window and taskbar plugins, `install_update` and the release-page opener. Each answers `{code: "not_applicable", message}` where the browser has no equivalent, and the UI hides its control via `transport()`.
- **Folder drops.** A browser does not provide an absolute folder path. The drop handler says "Use Add folder…"; it never pretends to have a path.
- **Multiple clients.** Every job records the client and session that started it. The UI shows "Started from another session" when it did not start the job, and cancel/pause from another session asks first. The shared operation slots (move control, tool download, update install) return a busy error keyed by operation id instead of overwriting each other.

### D10. Data safety

Every media-file write still goes through the existing paths: `cv-fs` atomic replace, the rename planner and journal, the deleted-files destination, and History for undo. The server adds no new file write except the upload, which goes through the artwork store as in D9, and the make-folder action, which creates an empty folder and has no undo.

Permanent delete is the one place a network session changes the risk. The desktop's `erase` accepts the literal `"DELETE"`, which a same-origin script could replay. In browser mode permanent delete is off unless enabled in Settings › Server. When enabled, the server mints a one-time intent bound to the exact item IDs, valid for 60 seconds, and the request must also carry the current password; a missing, reused or mismatched intent is refused before any file is touched. The bin/trash delete keeps its no-dialog behaviour, as `title-deletion` specifies.

The data folder's lock and owner file make two writers impossible; the library database still has one writer thread and the same migrations.

### D11. Packaging

- **Image:** `ghcr.io/mustafevt/cinevault` (owner confirmed at release), semver tags plus `latest`, amd64 and arm64, non-root, `PUID`/`PGID`, `/data` and the media folders read-write, the portable owner lock, a health check and a write probe.
- **Packages:** `cinevault-server` for `.deb` and `.rpm` (the desktop package keeps `cinevault`), both containing the server, the CLI and the UI assets. A tarball for hosts with neither.
- **Tools:** MediaInfo and FFmpeg download at runtime into `/data/tools` as today, with the pinned manifest; the image does not bundle them. A `linux-aarch64` manifest entry is added so arm64 is not a lie, or arm64 is dropped from the first release.
- **systemd:** a user or system unit with `User=`, `CINEVAULT_DATA_DIR`, `LoadCredential=cinevault_key:...` and `After=network-online.target`; no Secret Service.
- **Compose:** `PUID`/`PGID`, the secrets file as a Docker secret, read-write mounts, `restart: unless-stopped`, and `.deletedByCineVault` as the default deleted-files destination because the freedesktop trash is not writable on a bind mount.
- **CI:** amd64 on pull requests, arm64 on the release workflow with a native runner; the smoke test scans, renames and writes artwork, not only `/healthz`.
- **Version:** the image tag and package versions join the four version files in the version check.
- **Assets:** `index.html` is `no-store`; hashed assets are immutable; a build-id mismatch offers a reload.

### D12. Updates and rollback

The server never runs the desktop installer. Settings › Updates in a browser shows the running version, the image/package instruction and a link to the notes. Pulling a new image can migrate the library; rolling back requires a same-or-newer build, so the docs and the release notes say to copy `library.db` and `settings.json` before an upgrade. The upgrade test covers a failed migration and a restore.

### D13. Kodi integration

Optional, off by default: a Kodi host, port and optional credentials in Settings. After a Scan, Scrape or Rename batch, the server can call Kodi's JSON-RPC `VideoLibrary.Scan`; "Open in Kodi" calls `Player.Open` with the file path as Kodi sees it. The path mapping (the server's `/media/movies` versus Kodi's `/storage/movies`) is a setting. A failure says Kodi is unreachable; it never blocks the batch.

## Risks / Trade-offs

- **Tauri internals drift** → pin `@tauri-apps/api`, a contract test that renders `AppShell` and covers `metadata`, `transformCallback`, events and `convertFileSrc`, and the shim as the only file that touches the internals.
- **Two transports drift** → the shared `cv-ipc` crate, the `AppContext`, the `EventSink`, and a parity test that compares names and arguments against a separate server table.
- **A remote library manager is a security surface** → loopback by default, a 30-minute setup window, argon2id, fail-closed CSRF and `Origin`, trusted-proxy-only headers, rate limits, request and connection caps, path jails, a security review task, and plain HTTP only on a LAN or VPN.
- **The browser cannot play much of a Kodi library** → do not promise HTML5 parity: offer the browser player, a stream link and Kodi; state the limitation in the UI and the docs.
- **A NAS has no keyring** → the encrypted secrets file with a master key from a Docker secret or systemd credential, and a visible "keys are locked" state.
- **Two writers on a shared data folder** → the portable owner file, the CLI's refusal when the endpoint is unreachable, and the documented local-filesystem requirement.
- **Streams lose events on reconnect** → SSE with `Last-Event-ID`, a bounded per-client ring and an explicit `reset`.
- **Performance on an ARM NAS** → windowed queries, immutable images, ETag/304, gzip on large windows, h2c or a proxy; a NAS-class budget and a cold thumbnail fill measurement, not only the desktop budget.
- **A large change lands in one go** → the three delivery stages, each leaving the desktop app working and the server usable.
- **Uploads and media serving are new file-read paths** → canonicalisation, `Path::starts_with`, content-type allowlists, no directory listing, and tests for `..`, encoded traversal and symlinks.
- **Upgrades can migrate the library** → pre-upgrade backup guidance, a same-or-newer rollback rule, and an upgrade/restore test.
- **`cinevault-server` and `cinevault` package names** → distinct names, both in the image and the tarball, and a version check that covers them.

## Migration Plan

No data migration. The data folder format is unchanged; `server.json`, `server-auth.json`, `secrets.enc` and `library-owner.json` are additive. Deleting `server-auth.json` resets the password; deleting `secrets.enc` removes the server's provider keys; neither touches the library.

Rollback: stop the server, start a desktop app at the same or a newer schema with the same data folder. The owner file is removed on a clean stop; a stale one is taken over after 30 seconds. The image or package can be removed without touching the volumes.

The command extraction changes no user-visible behaviour and lands before the server is exposed, verified by the existing test suite.

## Open Questions

None that change the specs or tasks. The image owner/registry name, the exact arm64 tool sources and the reverse-proxy snippets are decisions to make at release time, recorded in tasks 6.5 and 6.7.

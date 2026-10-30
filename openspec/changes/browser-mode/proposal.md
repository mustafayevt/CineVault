# Proposal

## Why

CineVault manages media files, and those files usually live on a home server or NAS, not on the machine where the user sits. Today the only way to run CineVault on that server is the `cinevault` command line: it scans, scrapes and renames, but it cannot show the library, fix a wrong match, choose artwork or review a rename before it happens. The result is that a headless setup either needs a desktop app on another machine with the library mounted over the network, or gets no interface at all. Issue #12 is exactly this case: a Kodi HTPC that cannot run desktop apps, with the files on a server.

A browser mode closes that gap. One copy of CineVault runs next to the files and serves the same interface to any browser on the network, so the machine holding the library needs no desktop session and the client needs no mounted drives. The desktop app stays exactly as it is.

## What Changes

1. **A headless server.** A new `cinevault-server` binary boots the existing `AppServices`, serves the built React UI and answers the whole typed command surface over HTTP. It links no Tauri, no WebKit and no display stack, so it runs in a slim container or on a minimal Linux install. The `cinevault` CLI ships in the same packages.
2. **One command layer, two transports.** The command handlers move out of the Tauri binary into a shared crate that depends on `cv-app` but not on Tauri. Desktop-only actions (native picker, opening a folder, window, taskbar, updater) sit behind one small platform trait and stay in the desktop adapter. The desktop app and the server are thin adapters over the same functions, so a fix or a feature is implemented once.
3. **One UI transport shim.** `invoke`, channels, events and image URLs are backed by HTTP and WebSocket when the page is served by the server, and by Tauri in the desktop app. The shim exposes a separate `transport` value, so the few places that differ can hide a desktop-only control without pretending to be Tauri.
4. **A friendly first run and sign-in.** The server has one user and one password. The first visit shows a welcome page that creates the password, then the normal setup. No default password, no setup token to copy from logs. The setup page is open only for 30 minutes after the first start and only until a password exists; the first browser to open it claims it. After that, only `cinevault-server passwd` on the server can set or reset it. The sign-in page is one password field with show/hide, clear errors and phone-width layout.
5. **Desktop-only behaviour replaced, not dropped.** A server-side folder browser replaces the native picker; artwork upload replaces drag-and-drop from the desktop; Play offers the browser's player, a stream link and an optional "Open in Kodi"; there is no taskbar, tray, window chrome or native snap layout.
6. **Streaming over the wire.** Job progress, notices and every provider stream (scrape candidates, artwork candidates, torrent results, tool installs, update downloads) move from Tauri channels to one WebSocket per page, with the same ordering, throttling and completion rules. Sign-out, a password change and expiry close that session's sockets.
7. **Single writer, on any host.** The server holds a portable lock marker in the data folder and republishes the local automation endpoint. The desktop app refuses a folder whose marker it cannot clear, and the `cinevault` CLI refuses (exit 2) when the marker is held but the server's channel is unreachable, instead of opening the library itself. The lock is documented as local-filesystem only.
8. **A data folder chosen by the environment.** `CINEVAULT_DATA_DIR` selects the data folder before portable mode and the pointer file, with a defined `Environment` mode, so a container mounts one volume and the CLI and server agree.
9. **Headless provider keys.** The desktop keeps the OS credential store. The server adds an encrypted secrets file in the data folder whose master key comes from `CINEVAULT_SECRET_KEY`, a Docker secret or a systemd credential. Keys are never plaintext, never in `settings.json` and never in an image layer. When the master key is missing the server says so and offers the key entry in Settings instead of silently losing keys.
10. **Packaging.** A multi-arch container image published to a named registry (amd64 and arm64), `.deb` and `.rpm` packages for the server, and a tarball for hosts with neither, all carrying the same version. The CLI is in every package. The image and service run as a non-root user with `PUID`/`PGID` mapping, declare the data and library volumes, expose a health check and run a write probe on each library folder during setup.
11. **Server updates.** A container or package is updated by pulling the new image or installing the new package; the desktop in-app installer and the signed update manifest stay desktop-only. `index.html` is never cached, hashed assets are immutable, and a build-id mismatch offers a reload.
12. **First-run setup in the browser.** The welcome flow runs in the browser with the server folder browser and upload, checking that each folder can be written before the scan. The tools it needs (MediaInfo, FFmpeg, the indexer service) download on the server as they do on a desktop.
13. **A trusted proxy and an honest LAN mode.** The documented HTTPS path is a reverse proxy. `CINEVAULT_WEB_TRUST_PROXY` names the proxy addresses, and only then does the server honour `X-Forwarded-Proto`, `-Host` and `-For` for the `Secure` cookie, the `Origin` check, the plain-HTTP warning and the sign-in rate limit.
14. **Kodi, the reason this exists.** Settings offers an optional Kodi connection (host, port, optional credentials) so a finished Scan, Scrape or Rename batch can ask Kodi to update its library, and a "Open in Kodi" action can hand a title to the player. It is off by default and never required.

No desktop behaviour is removed. The desktop app keeps its window, taskbar progress, native pickers, player choice and self-update.

## Delivery

The end state is full parity: every page and every action the desktop app has. It lands in stages so the issue is answered early:

- **Release one, the management loop:** server, sign-in, folder browser, scan, scrape (including fixing a match), artwork, rename, browse, packages. This is what the issue's user needs.
- **Release two, the rest of the writes:** delete, moves, NFO, collections, settings writes, provider keys in the UI.
- **Release three, the long tail:** torrent search, tool installs, update page, Kodi hand-off, and the remaining pickers.

Each stage leaves the desktop app working and the server usable; later stages add commands, not a new architecture.

## Capabilities

### New Capabilities

- `web-server`: the headless server and the browser surface. Covers the server lifecycle, command dispatch, authentication and sessions, the HTTP image and media routes, the WebSocket streams, the server folder browser and artwork upload, browser playback, single-writer coordination, the health endpoint, the Server settings section, and the container, package and tarball packaging.

### Modified Capabilities

- `app-shell`: which shell behaviours are desktop-only (window chrome, app zoom, single instance) and that the browser shell keeps the sidebar, pages, themes and navigation.
- `cli`: the command line forwards to the server as well as to the desktop app, ships with the server, and refuses (exit 2) when the data folder is held but unreachable.
- `data-storage`: `CINEVAULT_DATA_DIR` as the first data-folder rule with an `Environment` mode; the server's encrypted secrets file and master key; `server.json` and `server-auth.json` in the data folder.
- `artwork`: the `/img` route in a browser, its own owner of the image semantics, with the same keys, cache headers, ETag, stand-in rules and HEAD support.
- `playback`: Play in a browser (player, stream link, optional Kodi hand-off, download) while the external-player requirements stay desktop-only; disc folders handled honestly.
- `job-system`: progress reaches the browser over the server stream; the taskbar and dock mirror is desktop-only; jobs are attributed to the session that started them.
- `onboarding`: the first-run setup runs in a browser with the server folder browser, a write probe and no native drag region or window buttons.
- `release-and-updates`: the container image (registry and tag), server packages and tarball are published and versioned; the desktop update requirements stay desktop-only; rollback needs a same-or-newer build.
- `settings`: a Server section, the settings that do not apply in a browser, and which operating system's conventions a browser session follows.
- `desktop-interaction`: which rules apply in a browser (context menu, browser keys, drag and drop, window state) and which do not.
- `title-deletion`: permanent delete from a browser needs a server-minted, one-time intent and password re-entry; the bin delete keeps its no-dialog behaviour.
- `torrent-search`: the browser hand-off and its honest messages.
- `torrent-sources`: the indexer manager and own-Jackett addresses in a browser.
- `developer-tooling`: the server crates in the workspace checks, a real parity test, HTTP and stream contract tests, the container CI job, the new scripts and the notices/version checks.
- `ui-design-system`: the sign-in page and the folder browser are pages, not dialogs; the password change is inline.

## Impact

- **Rust:** a new `crates/cv-ipc` (handlers, DTOs and a command registry, Tauri-free) and `crates/cv-web` (server, auth, HTTP/WebSocket, static files, folder browser, uploads, media), with `src-tauri` reduced to Tauri adapters, a `Platform` trait for desktop-only actions and the desktop binary. `cv-app` gains the portable lock marker, the server secrets backend, the folder probe, the media range reader and `CINEVAULT_DATA_DIR`.
- **UI:** `ui/src/lib/ipc.ts` as the single transport shim with an explicit `transport` value and a generator that points `ui/src/bindings.ts` at it; a welcome/sign-in flow; a Server section; browser variants of the folder picker, artwork drop, Play and the desktop-only controls; job session attribution; build-id reload.
- **Dependencies:** `axum` (already a dev-dependency), `tower-http` for static files and ranges, `argon2` for password hashing, `chacha20poly1305` (or `age`) for the secrets file, and a WebSocket implementation; no `unsafe` is added.
- **Configuration:** `CINEVAULT_DATA_DIR`, `CINEVAULT_WEB_BIND`, `CINEVAULT_WEB_PORT`, `CINEVAULT_WEB_TRUST_PROXY`, `CINEVAULT_SECRET_KEY`, `CINEVAULT_WEB_ROOTS`, and `server.json`/`server-auth.json` in the data folder (no key material in either).
- **Packaging and CI:** `Dockerfile`, `.dockerignore`, `docker-compose.yml`, `scripts/package-server.sh`, a systemd unit example, a tarball target, a container build and smoke test in CI, and a server test job.
- **Docs and site:** a "Run on a home server" page (one path per install method, permissions, proxy, updates, backups, troubleshooting), README and landing-page copy, browser screenshots, `CHANGELOG.md` and the regenerated `site/changelog.html`.
- **Testing:** the existing command tests move with the handlers; new tests cover the HTTP contract, parity, auth and sessions, CSRF and proxy trust, WebSocket ordering and reconnect, image and media headers, range requests, the folder browser's root jail, upload atomicity across volumes, the lock marker and the secrets backend.

## Out of scope

- Multi-user accounts, per-user libraries, roles and permissions. The server has one user.
- Public internet exposure as a supported deployment. The documented path is a LAN or a VPN, with HTTPS from a reverse proxy.
- Server-side transcoding. The browser plays what the file holds, or downloads it; Kodi remains the player for everything else.
- Windows and macOS server builds; the server ships for Linux only. The desktop app stays on all three.
- Native mobile apps. Sign-in and first-run work at phone width; the library is designed for a tablet or larger.
- Moving the desktop app off Tauri, or changing its behaviour when no server is used.
- Importing a library from another media manager.

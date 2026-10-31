# Design

## Context

- `cv-web` is a binary crate (`cinevault-server`). Its handlers live in `main.rs`; `state.rs` builds every `cv-ipc` state object over one `AppServices`; `auth.rs` holds the password, the sessions and the sign-in pages; `dispatch.rs` maps every command of `bindings.ts` to the `cv-ipc` function the desktop calls.
- The desktop app builds the same `cv-ipc` state objects in `src-tauri/src/main.rs` and keeps them in Tauri state (owned values, borrowed by the command wrappers). Most of them are thin wrappers over `Arc` services from `AppServices`; a few hold in-memory coordination of their own: the image source index (`LibrarySources`, inside `ImageContext`), the batch runner of the selection actions (`CommandBackend`, in `AutomationState`), the drives service (`DrivesService`, which emits `DrivesChanged`) and the running-move slot of `LibraryOpsState`.
- The desktop app runs inside Tauri's tokio runtime (`tauri::async_runtime`), the same `tokio` 1.x the server uses. The main thread enters that runtime at start.
- The desktop holds the data folder's owner file with role `app`; the command line forwards batches to it over the local automation channel.
- There is no tray icon and no autostart today. Closing the main window quits the app (and may install a downloaded update first).

## Goals / Non-Goals

**Goals:** one server implementation for both surfaces; the desktop's open library served without a second process; start, stop and restart without leaking tasks, ports or sessions; the security model of the headless server unchanged in the app; a first password and a forgotten password that need no terminal on the home network.

**Non-Goals:** HTTPS in the app, access from the internet, a second user, a new command surface.

## Decisions

### D1. `cv-web` becomes a library with a thin binary

`src/lib.rs` owns the modules (`auth`, `dispatch`, `state`, `server` for the router and handlers, `network` for address rules, `embed` for the desktop API). `src/main.rs` keeps only argument parsing, `passwd`, the boot of its own services and the signal handling. The router is built by one function, `server::router(state)`, so both surfaces serve exactly the same routes.

`WebState` gains three fields:

- `host: HostMode` — `Server` or `Desktop { computer }`. It decides the welcome page (never in `Desktop`), the recovery text and the reset routes.
- `lan_only: bool` — a middleware refuses every request whose peer is not a home-network address (D5) with 403 and one sentence. The headless server keeps `false` (it binds loopback by default and has its own proxy rules).
- `ui: UiAssets` — `Folder(PathBuf)` (the server's `--ui`) or `Embedded(Arc<dyn Fn(&str) -> Option<(Vec<u8>, String)>>)`, which the desktop backs with Tauri's asset resolver, so the app serves the interface it was built with and needs no `ui/dist` beside it. A debug app built against the dev server has no embedded assets; it falls back to `ui/dist` in the repository and says so in the section when that is missing.

*Alternative:* a second, desktop-only HTTP server in `src-tauri` (two implementations of every security rule — rejected).

### D2. Shared state, not a second copy

`embed::start(options, shared)` takes a `Shared` value from the desktop:

- `services: Arc<AppServices>` — the app's open library, settings, jobs and provider keys.
- `images: ImageContext` — the desktop's own (it is `Clone` over `Arc`s), so renames from a browser update the desktop's poster index and the other way round.
- `actions: Option<Arc<CommandBackend>>` — the desktop's batch runner, so the selection actions of both surfaces queue on one runner.
- `drives: Option<Arc<DrivesService>>` — the desktop's, so a move from a browser refreshes the desktop's Drives page.
- `moves: LibraryOpsState` — built with `LibraryOpsState::sharing_moves_with(&desktop_ops, sources)`, a new constructor whose move slot is the same `Arc<Mutex<…>>` as the desktop's. A second cross-drive move is refused as busy whichever surface starts it.

The remaining state objects are built by `Commands::build_shared` over the same services. They are wrappers over shared `Arc` services; the lazily built services inside them (`OrganizeService`, `HistoryService`, `EditService`) read and write the same database through its single writer thread, exactly as the headless server and the command line do. The review inbox's in-memory undo list is per surface: an undo offered in a browser undoes what that browser did.

Provider keys stay in the OS credential store. Browser commands reach them only through `AppServices` (a scrape uses the key; Settings sees `present: true`, never a value), as on the desktop. The server's `secrets.enc` and its master key are not used in the app, and "keys locked" never shows.

### D3. Lifecycle and the runtime

`embed::start` is synchronous and must run inside a tokio runtime (every Tauri async command does):

1. Refuse to start without a password (`StartError::NoPassword`).
2. Bind with `std::net::TcpListener::bind((0.0.0.0, port))`, so "port in use" is a typed error (`StartError::PortInUse`) the section can show at once, then `set_nonblocking` and `tokio::net::TcpListener::from_std`.
3. Open `Auth` in desktop mode (D4), build the commands, subscribe the change relay with a `Weak` reference (a stopped server's relay does nothing), and spawn the progress feed and `axum::serve(…).with_graceful_shutdown(…)` with `tokio::spawn`.
4. Return `RunningServer { addr, auth, streams, shutdown, tasks }`.

`RunningServer::stop()` (async): ends every session (`Auth::logout_all_except(None)`), closes every SSE stream, signals the graceful shutdown (the accept loop ends and the listener is dropped), aborts the progress feed, and waits for the serve task for at most 3 seconds before aborting it. A media download still in flight stops at its next 64 KiB chunk, because its session is gone. The port is closed when `stop` returns, so a port change is stop-then-start and a test can assert the port is free.

The desktop keeps `BrowserAccessState { config, running: tokio::sync::Mutex<Option<RunningServer>>, last_error }`. Commands that change it serialize on that mutex. On launch the app starts the server after `start_background` when `browser-access.json` says it is on; a failure (port in use) is shown in the section and as a notice, never a crash. On exit, `stop` runs before the services shut down.

### D4. Passwords and sessions in the app

The password is the same argon2id `server-auth.json` in the data folder, so a library later served by `cinevault-server` keeps it. The section creates or changes it with `Auth::set_password_unattended` — the person is at the computer, the same trust boundary as `cinevault-server passwd` — and a change signs every browser out. The welcome page never appears (`/setup` redirects to the sign-in page), and the server does not start without a password.

Sessions gain a public handle (16 random bytes, distinct from the cookie value), the client address, a short browser description from `User-Agent` and their wall-clock sign-in and last-use times. The section lists them and signs one out by handle or all of them; the session id itself never leaves the server. Turning Browser access off drops the `Auth` value, so every session ends; the next start begins with none.

### D5. Home-network addresses

`network::is_home_address(ip)` is true for loopback, `10/8`, `172.16/12`, `192.168/16`, `169.254/16`, `100.64/10` (carrier-grade NAT, as used by some VPN meshes), `::1`, `fc00::/7`, `fe80::/10`, and IPv4-mapped IPv6 forms of those. The client address is the socket peer, or the right-most untrusted `X-Forwarded-For` hop only when the peer is in `CINEVAULT_WEB_TRUST_PROXY` (the existing `client_ip`). The desktop never trusts a proxy.

Used by: the app's "Only devices on my home network" guard (every route), the server's welcome page and setup request, and the server's PIN reset. A Docker port mapping makes the peer the bridge gateway (a private address); the docs should say that setup through a published port is therefore treated as home-network, and that an internet-facing reverse proxy must be listed in `CINEVAULT_WEB_TRUST_PROXY`, or every request through it looks local.

### D6. The headless welcome without a time limit

`Auth::needs_setup` is true exactly while no password exists; the 30-minute deadline is removed. The first home-network browser to open the welcome page claims it with the `cv_setup` cookie; the claim lapses after 30 minutes without use, so an abandoned tab does not lock setup until a restart. A public-address browser gets a 403 page: "Finish setup from your home network." with the `cinevault-server passwd` fallback.

### D7. Forgotten password: a PIN in a file, not in the log

`POST /api/reset/start` (headless only, home-network peers only) creates an 8-digit PIN from the OS random source, valid for 15 minutes, writes `password-reset-pin.txt` to the data folder with mode `0600` through `cv_fs::write_atomic_with_mode`, and logs one line naming the file and its expiry. It answers with the file's path so the page can say where to look. While a PIN is live, asking again does not make a new one (the attempts counter cannot be reset by asking again); after one is used, expires or is voided, a new one can be made after one minute.

`POST /api/reset/finish {pin, password, confirm}` (same peers) compares the PIN in constant time (`subtle`). Five wrong PINs void it and delete the file. A right PIN with a password that is too short or does not match is answered without spending the PIN. On success: the password is set, every session and stream ends, the file is deleted, and the browser is signed in with a new session. An expired PIN's file is deleted on the next request and at start.

*Why not log the PIN:* the original design rejected setup tokens in logs because Docker and journald keep logs, ship them to aggregators and show them to anyone who can read the journal; a reset PIN is a password-equivalent for 15 minutes. A file in the data folder needs the same access as `cinevault-server passwd` (the server user, or `docker exec`), so it adds no new trust boundary. The log line gives the exact command for Docker (`docker exec <container> cat /data/password-reset-pin.txt`), which keeps it nearly as easy as reading the log.

In the app, the sign-in page's "Forgot password?" says "Change it in CineVault's Settings on <computer>." and the reset routes answer 403.

### D8. Addresses, firewall and port

The section lists `http://<address>:<port>` for the computer's primary IPv4 address (the local address of a UDP socket "connected" to a public address, which sends nothing) and the other private IPv4 addresses of real interfaces (`if-addrs`, skipping loopback, link-local and virtual adapters such as `vEthernet`, `WSL`, `docker`, `veth`, `virbr`, `VirtualBox`, `VMware`), then `http://localhost:<port>`. The QR code encodes the first address.

The listener binds `0.0.0.0` (IPv4) only while Browser access is on; nothing listens when it is off. On Windows the first bind of a new executable shows the Defender Firewall prompt; the section explains it before the first switch-on and, when Windows is detected, keeps a short line under the address. The port defaults to 8787 and accepts 1024–65535. A port in use leaves Browser access "on, not running" with "Port 8787 is in use by another program. Choose another port, such as 8788." The next launch tries again.

### D9. Tray and start at sign-in

- Tauri's `tray-icon` feature. The tray icon exists while "Keep running in the tray" is on and Browser access is on; its menu has "Open CineVault" and "Quit CineVault"; a left click opens the window.
- Closing the main window while both are on hides it instead of quitting (no update-on-close install then; "Quit CineVault" from the tray installs a waiting update as closing used to). Otherwise closing quits as before.
- `tauri-plugin-autostart` (official, Rust side only, no web view capability): the Windows `HKCU\…\Run` entry, a Linux `~/.config/autostart/*.desktop` file, a macOS launch agent. It passes `--background`; a launch with it and both options on starts with the window hidden. The switch reads the real OS state, so removing the entry outside CineVault shows as off.
- `browser-access.json` holds `enabled`, `port`, `lanOnly` and `keepInTray`, written atomically. It is not in `settings.json`, so a browser user cannot change it through the settings commands; every Browser access command is desktop-only and answers `not_applicable` from the server's dispatcher.

### D10. Lock semantics and the command line

The app still owns `library-owner.json` as `app`. The embedded server opens nothing itself and takes no owner file. The command line forwards to the app's automation channel as before, whether or not Browser access is on; a headless `cinevault-server` on the same data folder is still refused while the app runs.

### D11. Data safety

Browser access adds no new write to media folders: browser commands use the same `cv-ipc` paths (atomic writes, rename journal, deleted-files destination, History). The new data-folder files are written atomically: `browser-access.json`, `server-auth.json` (mode `0600`) and the reset PIN file (mode `0600`, deleted after use or expiry). Interrupting a start or stop leaves at most a closed port and signed-out browsers.

## Risks / Trade-offs

- **A home server on the LAN is a target** → off by default, a 12-character password before it can start, home-network-only by default, the server's rate limits and CSRF; the section says the connection is not encrypted.
- **Antivirus or firewall prompts alarm people** → the section explains the prompt before the first switch-on.
- **Two surfaces' in-memory state drifts** → the coordinating pieces are shared (D2); the rest are wrappers over shared services.
- **A hung connection keeps a stop waiting** → a 3-second cap, then abort; sessions are already gone.
- **Docker peers look local** → documented; the trusted-proxy setting decides behind a proxy.

## Migration Plan

Additive. A data folder without `browser-access.json` has Browser access off. The headless server's first-run behaviour changes only by dropping the 30-minute limit in favour of the home-network rule; existing passwords and `cinevault-server passwd` are unchanged.

## Open Questions

None blocking. Wording for the README, the website and the install guides is left to the documentation change (see the pull request notes).

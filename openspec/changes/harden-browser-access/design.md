# Design

## Context

Line numbers are those of `main` at `f153f5cd` (the 1.3.0 release).

- **Programs run from a browser.** `crates/cv-web/src/dispatch.rs:1611` dispatches `set_tool_path` and `:1619` `install_tool` to a browser, because both are in the served list (`crates/cv-ipc/src/commands.rs:292`, `:677`). `set_tool_path` reaches `ToolsService::set_path` (`crates/cv-app/src/tools.rs:111`), which calls `ToolsManager::check_override` (`crates/cv-media/src/tools/manager.rs:166`). That runs the chosen file at once to ask its version (`crates/cv-media/src/tools/version.rs:37`, `:48`). `normalize_override` (`manager.rs:491`) only requires an absolute path whose file name contains the tool's name, so a UNC path (`\\host\share\ffmpeg.exe`) or any file named like the tool passes. MediaInfo is a library and is not run, but it is loaded on next use, which runs its code just the same.
- **Any setting from a browser.** `set_setting` (`dispatch.rs:343`) and `reset_settings` (`:352`) pass any key to `SettingsService::set` (`crates/cv-app/src/app_settings.rs:185`). That checks only the key's shape and the value's size. Sensitive keys a browser can write today:
  - `general.update-channel` and `general.update-public-key` (`crates/cv-app/src/updates/service.rs:26`, `:40`). Together they let a browser name a manifest signed with its own key, which the desktop then trusts (`service.rs:461`, `:549`).
  - `tools.path.mediainfo` and `tools.path.ffmpeg` (`crates/cv-app/src/tools.rs:20`). Written this way they skip even the version check, because the next start restores them unchecked (`tools.rs:72`).
  - `player.kind`, `player.path` and `player.custom-command` (`crates/cv-app/src/playback.rs:34-36`). The desktop runs these when someone presses Play at the computer.
  - `files.trash-folder` (`crates/cv-app/src/deleted_files.rs:32`), which sends every deleted and replaced file to any absolute folder.
  - `metadata.nfo-collection-folder` (`crates/cv-app/src/artwork/collection.rs:43`), where collection NFO and artwork files are written.

  The data folder location is not a setting. It lives in `data-location.json`, and no served command writes it, so nothing changes there.
- **Proxied visitors count as home network.** `from_home_network` (`crates/cv-web/src/server.rs:279-281`) judges `client_ip` (`server.rs:163`), which is the socket peer unless the peer is in `CINEVAULT_WEB_TRUST_PROXY`. `is_home_v4` (`crates/cv-web/src/network.rs:26`) counts loopback and private ranges. The desktop's `home_network_guard` (`server.rs:85-95`) uses the peer alone. A reverse proxy or tunnel client on the same machine connects from `127.0.0.1`, so with no trusted proxy configured, every internet visitor counts as home network. Such a visitor can reach the welcome page (`setup_page`, `server.rs:291`; `setup`, `:371`) and the PIN reset (`:431`, `:485`).
- **Sign-in limits.** `MAX_FAILURES = 5`, `MAX_GLOBAL_FAILURES = 100` per `FAILURE_WINDOW` of 60 s (`crates/cv-web/src/auth.rs:37-39`). `blocked` (`auth.rs:182`) refuses every client once the global count is reached. `Auth::login` (`auth.rs:414-441`) runs Argon2 (`:426-433`) with the failures mutex held, and the `login` handler (`server.rs:333`) calls it on the async runtime.
- **Origin.** `origin_matches` (`server.rs:207-219`) returns true when the request has no `Origin`. The `web-server` spec ("Write protection") asks for a matching `Origin` on every command.
- **Framing.** `static_file` (`server.rs:1504`) sends only `Cache-Control` and `X-Content-Type-Options` (`server.rs:1563-1571`). The sign-in, welcome and away pages send no framing header either.
- **Sessions in memory.** `Auth.sessions: Mutex<HashMap<String, Session>>` (`auth.rs:157`) is keyed by the raw cookie value. `Session` (`auth.rs:69`) holds `Instant`s. `RunningServer::begin_stop` and `stop` (`crates/cv-web/src/embed.rs:274`, `:284`) end every session, both when Browser access is switched off and when the app quits.

## Goals / Non-Goals

**Goals:** a browser session cannot run, choose or load a program on the host, and cannot redirect where deleted files go outside the folders the server already exposes. A proxy cannot make the internet look like the home network. A flood cannot stop the owner from signing in. Spec and code agree on `Origin`. No framing. Restarts keep people signed in.

**Non-Goals:** a security review of web addresses the server fetches; HTTPS; more than one user.

## Decisions

### D1. `set_tool_path` is desktop-only; `install_tool` stays

`set_tool_path` leaves `cv_ipc::commands::COMMANDS`, and its dispatcher arm answers `not_applicable("A tool's path is set on the computer that runs CineVault.")`. The existing test `the_desktop_only_commands_answer_not_applicable` (`dispatch.rs:2021`) then covers it with no new test code, and `every_binding_command_is_dispatched` keeps it accounted for.

`install_tool` stays served. It installs only the package this build pins in its manifest, by exact URL, size and SHA-256 (`manager.rs:412`), into the app's own tools folder. A browser cannot choose what it downloads or runs. The headless server has no other way to get MediaInfo and FFmpeg, because its first-run setup downloads them from the browser. `install_spec`, the path that takes a digest from outside, is not reachable from any command.

### D2. Host-sensitive settings

One list in `cv-web`, `dispatch::HOST_SETTINGS`, checked before `set_setting` and `reset_settings`:

| Key | From a browser |
|---|---|
| `general.update-channel`, `general.update-public-key` | refused |
| `tools.path.*` (any key with that prefix) | refused |
| `player.kind`, `player.path`, `player.custom-command` | refused |
| `files.trash-folder` | accepted only for an empty value or an absolute folder inside `allowed_roots` (`server.rs:902`) |
| `metadata.nfo-collection-folder` | same as `files.trash-folder` |

A refused `set_setting` answers with the sentence "This setting is changed on the computer that runs CineVault." and changes nothing. `reset_settings` from a browser leaves the refused keys as they are and resets the rest, so "Reset to defaults" still works. A path check uses the same component-wise containment as `inside_roots` (`dispatch.rs:198`), after canonicalising the existing part of the path, so `..` and links cannot leave the roots.

The list is code, not a setting. A test walks the setting keys declared in `ui/src/features/settings/schema.ts` and fails when a key whose value is a path, a program or a command (the schema's `mono` text fields) is neither in the list nor in a small reviewed allow-list. A new key of that kind then needs a decision.

On the headless server, tool paths and the player are set in `settings.json` while the server is stopped (`packaging/README.md` says how). The player is unused there anyway. *Alternative:* a `cinevault-server set` command. It is left out to keep this change small; it can follow if people ask.

### D3. Forwarding headers from an untrusted peer

`network::forwarded(headers)` is true when the request has any of `Forwarded`, `X-Forwarded-For`, `X-Forwarded-Host`, `X-Forwarded-Proto`, `X-Real-IP`, `CF-Connecting-IP` or `True-Client-IP`. These are what common reverse proxies and tunnel clients add. `from_home_network` becomes:

```
if !trusted(peer) && forwarded(headers) { return false }
is_home_address(client_ip(headers, peer, trust))
```

`home_network_guard` uses the same rule, so the desktop's "Only devices on my home network" refuses a proxied request. The desktop never trusts a proxy, so a person who puts a proxy in front of Browser access turns that option off, and the section's existing warning applies. The first such request in a run logs one `warn!`: "A request came through a proxy that is not in CINEVAULT_WEB_TRUST_PROXY; it is treated as outside the home network." A `OnceLock`/`AtomicBool` keeps the log quiet afterwards.

A Docker port mapping adds no header, so setup through a published port still works from the home network. A raw TCP forward (an SSH tunnel, a TCP tunnel) adds no header and still looks local. The welcome page already says to finish setup before the server is reachable from the internet, and the PIN reset needs the PIN file in the data folder, so a stranger who starts a reset gets nothing.

*Why no setup code:* printing a code to the log was rejected before, because Docker and journald keep and ship logs (archived design `2026-10-31-desktop-browser-access`, D7). A code in a file would make every first run harder to close a gap that only a raw TCP forward opened before setup can reach. The header rule closes the common case at no cost to the first run.

### D4. Sign-in limits

- **Per address:** 5 failures a minute, as today, for every address.
- **Global ceiling:** 100 failures a minute counted over all addresses. When it is reached, it refuses sign-in only to addresses outside the home network (judged by D3). A home-network address is still checked against its own limit. A distributed flood therefore cannot lock the owner out at home, and a signed-in browser never needs to sign in again (D6).
- **The check:** `Auth::login` becomes async. (1) Under the failures lock it prunes, checks `blocked` and records a *provisional* failure for the address, then drops the lock. (2) It takes one permit of a `tokio::sync::Semaphore` with 2 permits, so at most two Argon2 checks run at once whatever the request rate. (3) It runs `verify_password` in `spawn_blocking`. (4) On success it takes the lock again, removes the provisional record and clears the address. Because the attempt is recorded before the check, concurrent attempts cannot slip past the limit together, which the current single lock was there to ensure. A wrong password keeps the record. `check_password` (used for "Change password" and permanent delete) moves to `spawn_blocking` the same way and takes a permit.

### D5. `Origin`

The code stays as it is, and the spec is corrected. When a request has an `Origin`, it must match the effective host. When it has none, the CSRF token in `X-CineVault-Csrf` decides. A cross-site page cannot read the token or set that header without a CORS allowance, which the server never sends, and the session cookie is `SameSite=Strict`. Browsers send `Origin` on every `fetch` POST, so the check still runs for the interface. A script that holds the cookie and the token can send any `Origin`, so requiring one would protect nothing.

### D6. Frame headers

A `map_response` layer on `server::router` adds `X-Frame-Options: DENY` to every response. Where a response has no `Content-Security-Policy`, it adds `Content-Security-Policy: frame-ancestors 'none'`. Where a handler sets its own policy (the media route's `MEDIA_CSP`, `server.rs:1285`), that policy gains `frame-ancestors 'none'`, so one policy is sent. Tested on the shell, the sign-in page, the welcome page, an image, a media range and a 404.

### D7. Sessions on disk

- **File.** `server-sessions.json` in the data folder:
  ```json
  {"version":1,"password":"<sha256 of the PHC string>","sessions":[{"id":"<sha256 of the cookie>","csrf":"…","handle":"…","address":"…","agent":"…","signedIn":1730000000000,"lastUsed":1730000500000}]}
  ```
  It is written with `cv_fs::write_atomic_with_mode(…, 0o600)`. The cookie value never reaches the disk. A lookup hashes the cookie with SHA-256; the id is 32 random bytes, so a slow hash adds nothing. The CSRF token is kept so open tabs keep working after a restart. It is useless without the cookie.
- **Expiry.** Expiry uses wall-clock times (`signedIn`, `lastUsed`), not `Instant`s, so it works across restarts: 30 days idle, 90 days total, 32 sessions, as today. Expired entries are dropped on load and on every write.
- **Writes.** On sign-in, sign-out, sign-out-all, eviction and password change. A use updates `lastUsed` in memory, and the file is written at most once every 5 minutes because of uses, so browsing does not write to disk on every request. A session idle for close to 30 days may therefore last up to 5 minutes longer, which is harmless.
- **Password binding.** `password` is the SHA-256 of the current hash. On load, a mismatch drops every session. This covers `cinevault-server passwd` and `passwd --reset`, which change the password while the server is stopped.
- **Desktop.** `begin_stop` (quitting) keeps the sessions on disk. `stop` (switching off, changing the port) and "Sign out all browsers" end them and write an empty list. The next start of Browser access loads what is left.
- **Interrupted or damaged.** The atomic replace leaves the old file or the new one. An unreadable file or one of an unknown version is logged, set aside as `server-sessions.json.bad`, and the server starts with no sessions: everyone signs in again, and nothing else is affected. Nothing in a media folder is written, so there is nothing to undo.

## Risks / Trade-offs

- [A person who uses a local proxy for HTTPS without listing it in `CINEVAULT_WEB_TRUST_PROXY` can no longer run first-run setup or a PIN reset through it] → The page says to finish setup from the home network, and the warning names the variable. Setting it is the documented setup already.
- [The global ceiling no longer protects home-network addresses] → Each one still has its 5-a-minute limit, and the semaphore caps the CPU.
- [A copied `server-sessions.json` lets the copier keep sessions] → It holds only hashes. A copy of the cookie is needed to use a session, and the file is owner-only.
- [An existing user whose tool path a browser had changed keeps it] → The path is kept, and the refusal applies only to new changes.

## Migration Plan

No migration. Existing sessions in memory are lost once, at the update that brings this change. After that, restarts keep sessions.

## Open Questions

- Should the headless server get a `cinevault-server set <key> <value>` command for tool paths, instead of editing `settings.json`? (D2 assumes not.)

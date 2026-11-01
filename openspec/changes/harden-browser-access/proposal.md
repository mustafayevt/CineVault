## Why

A review of the browser server (the headless `cinevault-server` and the desktop app's Browser access) found six gaps, each checked against the code (see design.md for the places):

- A signed-in browser can make the host run a program: it can point a tool path at any file, which CineVault runs at once to ask its version, and it can write any setting, including the update channel and its public key (so the desktop's next update could trust someone else's manifest) and the custom player command.
- Behind a reverse proxy or tunnel on the same machine, every visitor arrives from `127.0.0.1` and counts as the home network, so a stranger could claim first-run setup or start a password reset, and the desktop's "Only devices on my home network" lets everyone in.
- A flood of wrong passwords from many addresses trips the global ceiling and locks the owner out of signing in, and every password check runs while the failure lock is held, on the async runtime.
- The spec asks for a matching `Origin` on every command, while the code accepts a request without one.
- No page response forbids framing, so another site can show CineVault in a frame.
- Sessions live only in memory, so every server restart or app update signs every browser out.

## What Changes

1. **Host-sensitive commands and settings are desktop-only.** `set_tool_path` answers `not_applicable` in a browser. A browser cannot set or reset the update channel, the update public key, tool paths or the player settings; the custom trash folder and the collection data folder are accepted from a browser only inside the server's folder roots. `install_tool` stays available, because it installs only the pinned, checksummed downloads.
2. **Forwarded requests are not home network.** A request that carries a forwarding header (`Forwarded`, `X-Forwarded-For`, `X-Real-IP` and the like) from a peer that is not a trusted proxy is judged outside the home network for the welcome page, the PIN reset and the desktop's home-network-only guard. The first such request in a run is logged as a warning that names `CINEVAULT_WEB_TRUST_PROXY`.
3. **Sign-in limits that cannot lock the owner out.** Five failures per address per minute stay. The global ceiling (100 failures a minute) applies only to addresses outside the home network. The password check runs on a blocking thread, outside the failure lock, at most two at a time.
4. **`Origin` rule matches the code.** A command request with an `Origin` must match the effective host; a request without one is protected by the CSRF token alone. The spec is corrected.
5. **No framing.** Every response carries `X-Frame-Options: DENY` and a `Content-Security-Policy` with `frame-ancestors 'none'`.
6. **Sessions survive a restart.** Sessions are kept in `server-sessions.json` in the data folder (hashed ids, owner-only, written atomically) and are dropped on sign out, "Sign out every session", a password change (including `cinevault-server passwd`) and, in the desktop app, turning Browser access off or changing its port.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-server`: desktop-only commands gain `set_tool_path`; host-sensitive settings refused from a browser; forwarded requests not home network; sign-in limits; `Origin` rule; no framing; sessions kept across restarts.
- `settings`: Settings in a browser shows the host-sensitive settings read-only.

## Out of scope

- Settings that name a web address the server fetches (torrent source and solver addresses). They reach the network, not a program or a file, and need their own review.
- A first-run setup code. The forwarding-header rule covers HTTP proxies and tunnels; design.md says why a code is not added.
- A command on `cinevault-server` to set tool paths; the server guide says how to set them in `settings.json` while the server is stopped.
- HTTPS, multiple users, changes to the CSRF token itself.

## Impact

- `crates/cv-web`: `dispatch.rs` (the `set_tool_path` arm, the settings guard), `server.rs` (forwarding-header rule, warning, frame headers), `network.rs`, `auth.rs` (sign-in limits, session store), `embed.rs` (stop keeps or ends sessions).
- `crates/cv-ipc`: `commands.rs` loses `set_tool_path` from the served list.
- `ui`: the Tools rows and the Updates section show their paths and channel read-only in a browser.
- New file in the data folder: `server-sessions.json`.
- `packaging/README.md` (trusted proxy and tool paths on the server), `CHANGELOG.md` and `site/changelog.html`.

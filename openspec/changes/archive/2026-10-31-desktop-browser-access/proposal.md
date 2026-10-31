## Why

CineVault 1.2.0 can serve its interface to a browser, but only through the headless `cinevault-server`, which today means Docker or a Linux package. Most people already run the desktop app on the computer that holds their media. They expect what other home media apps do: flip a switch in the app they installed and open the library from a phone, a TV or another computer on the same network, with no container, no terminal and no second install.

Setting the password on the headless server is also harder than it needs to be: the welcome page closes after 30 minutes, and a forgotten password needs shell access to the server.

## What Changes

1. **Browser access in the desktop app.** Settings gains a Browser access section (desktop only, off by default). When it is on, the desktop app runs the same web server as `cinevault-server` inside its own process, over the library it already has open: same sign-in, sessions, CSRF and `Origin` checks, rate limits, media session binding, folder jail and command surface. No second process opens the library, so the one-writer rule is untouched, and the command line keeps forwarding its batches to the app as it does today.
2. **Set up at the machine.** The password is created and changed in the section; a browser never sees a welcome page in this mode. The section shows the addresses to open (the computer's network address and `localhost`) with Copy buttons and a QR code, the port (changeable), the state (running, port in use, or an error with a plain fix), the signed-in browsers with "Sign out" for each and "Sign out all browsers", and "Only devices on my home network" (on by default: the server answers only loopback, private, link-local and carrier-grade NAT addresses). Turning Browser access off stops the server, closes its port and signs every browser out.
3. **Windows firewall explained before it asks.** The server binds only while Browser access is on. Before the first switch-on, the section says "Windows will ask to allow CineVault on private networks. Choose Allow."
4. **Keep running and start at sign-in.** Two options in the same section: "Keep running in the tray when the window is closed" (closing the window hides it while Browser access is on; a tray icon opens it again or quits) and "Start CineVault when I sign in" (the Windows Run entry, a Linux `.desktop` autostart file, a macOS login item), which starts in the tray when both options are on.
5. **Easier first password on the headless server.** The welcome page no longer closes after 30 minutes. It is available until a password exists, but only to browsers on the same machine or the home network (loopback, private, link-local and carrier-grade NAT addresses, judged by the real peer address, or by `X-Forwarded-For` only from a trusted proxy). A browser from a public address is told to finish setup from the home network.
6. **Forgot password with a one-time PIN.** The headless server's sign-in page gains "Forgot password?". From a home-network browser it writes a one-time 8-digit PIN, valid for 15 minutes, to `password-reset-pin.txt` in the data folder (owner-only), logs where to find it, and asks for the PIN and a new password. Five wrong PINs void it; success deletes the file and signs every other browser out. In the desktop app's Browser access, "Forgot password?" says to change it in CineVault's Settings on that computer. `cinevault-server passwd` stays as the administrator's fallback.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-server`: the server can run inside the desktop app (Browser access); the first-run welcome is limited to the home network instead of 30 minutes; a PIN-based password reset; the sign-in page's recovery text depends on where the server runs; sessions record their address and browser for the sessions list.
- `settings`: a Browser access section in the desktop app.
- `app-shell`: keep running in the tray while Browser access is on, and start at sign-in.

## Out of scope

- HTTPS inside the app (certificates). The section says the connection is not encrypted and is meant for the home network; HTTPS stays the reverse proxy's job, as for the server.
- Reaching the app from outside the home network (port forwarding, relays, accounts). Turning "Only devices on my home network" off is possible, with a warning, but nothing configures a router.
- Multiple users or per-user libraries.
- Discovery protocols (mDNS/DLNA) and casting.
- macOS-specific firewall wording; macOS shows its own prompt and the section's generic text.
- README, website, install-script and release-workflow wording: another change owns those files. The section carries its own help text.

## Impact

- `crates/cv-web`: becomes a library plus the `cinevault-server` binary; an `embed` module starts and stops the server over shared services; the router, the static assets (a folder or the app's embedded files), the home-network guard, the PIN reset and the session details live in the library.
- `crates/cv-ipc`: `LibraryOpsState` can share its move slot with a second surface.
- `crates/cv-app`: the owner file's host name helper becomes public.
- `src-tauri`: a `browser_access_ipc` module (commands, the running server, the tray and autostart), the tray feature and the official autostart plugin, start on launch when the setting is on, hide-to-tray on close.
- `ui`: the Browser access section, its tests, a QR code helper (`uqr`, MIT, no dependencies), and the regenerated bindings.
- New files in the data folder: `browser-access.json` (the section's settings) and, briefly, `password-reset-pin.txt`.
- `CHANGELOG.md` and `site/changelog.html`.

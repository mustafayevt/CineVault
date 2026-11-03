## Why

People want the browser version of CineVault on their phone like an app (#51): Add to Home Screen in Safari on iOS, Install app in Chrome on Android. Today the interface has no web app manifest and no home-screen icon tags, so iOS makes a bookmark that opens in a Safari tab with a screenshot for an icon, and Android offers no install. The icons already exist in `assets/brand/favicon/`; the browser is never told about them, and the server would not hand them out without a session anyway.

## What Changes

- The interface build carries a web app manifest (`manifest.webmanifest`: name, `start_url` and `scope` `/`, `display: standalone`, the dark window ground as background and theme colour, 192 and 512 icons, one of them maskable) and the brand icons (`apple-touch-icon.png`, `icon-192.png`, `icon-512.png`, `favicon.ico`, `favicon.svg`).
- The server serves those fixed-name files without a session (browsers fetch the manifest without cookies) with a one-day cache, since their names are not hashed. Everything else keeps needing a session.
- The browser shell and the server's own pages (sign-in, welcome, "finish setup from your home network") carry the install head tags: the manifest link, the icon and `apple-touch-icon` links, the full-screen (`*-web-app-capable`), status bar and title tags for iOS, and `theme-color` for dark and light.
- The desktop app does not link the manifest; the server adds the tags only to what it serves to a browser.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-server`: new requirements "Installable from a browser" and "Install files without a session".

## Impact

- `ui/public/` (new: manifest and icon copies), `crates/cv-web/src/server.rs` (public files, cache header, manifest content type, head tags on the shell), `crates/cv-web/src/auth.rs` (head tags on the server's pages), their tests and a UI test that keeps the copies equal to `assets/brand/favicon/`.
- No IPC, database or media-folder changes.

## Out of scope

- The phone and tablet layout (sidebar as a drawer, stacked title pages, touch equivalents of hover and right-click menus, safe-area padding, `viewport-fit=cover`). It starts with a mock-up and stays on #51.
- A service worker and offline page.
- Playback changes for iOS.

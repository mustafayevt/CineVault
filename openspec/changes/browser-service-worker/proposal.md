## Why

The browser version installed on a phone (#51) starts by downloading its whole interface every time it is opened, and when the server can't be reached, for example away from home or with the computer asleep, it shows the browser's own error page. Over HTTPS a service worker can keep the build files and show a CineVault page instead.

## What Changes

- A service worker (`/sw.js`) is registered by the browser version, only when it is reached over HTTPS and the browser has service workers. The desktop app, a development browser and a plain `http://` home-network address go without one.
- It keeps the hashed build files (`/assets/*`) and serves them from storage first, dropping old builds' files beyond 80.
- When a page can't be loaded because the server does not answer, it shows "Can't reach CineVault" with Try again.
- It never stores a page, a command, a stream, an image or a media file.
- The server serves `/sw.js` and `/offline.html` without a session; `/sw.js` with `Cache-Control: no-cache` so an updated worker reaches installed apps.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-server`: a new requirement "Faster start over HTTPS".

## Impact

- `ui/public/sw.js`, `ui/public/offline.html`, `ui/src/lib/serviceWorker.ts` (registration) and `ui/src/main.tsx`; `crates/cv-web/src/server.rs` (public files, cache header, the offline page is not the app page); tests for each; the ESLint service-worker globals for `public/sw.js`.
- Builds on `installable-browser-app`.

## Out of scope

- Using the library offline: titles, images and commands always need the server.
- Push notifications and background sync.

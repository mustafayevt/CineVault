## Context

`installable-browser-app` lets a phone install the browser version. `index.html` is `no-store` and carries the session's CSRF token and the build id; hashed build files under `assets/` are `immutable`; everything under `/api`, `/img` and `/media` belongs to a session.

## Decisions

- **HTTPS only.** Browsers allow service workers on HTTPS and on `localhost`. Registration also requires the `https:` protocol, so a development browser on `http://localhost` and the desktop app never get one, and a plain home-network address keeps working as before.
- **What is stored.** Only same-origin `/assets/*` answers that are OK, cache-first; their names change with their content, so they never go stale. Pages are always fetched from the network (`index.html` holds the session token and build id); only when that fails does the worker answer with the stored offline page. Commands, streams, images and media are not touched at all.
- **Growth.** Each update adds new hashed files and the worker script itself does not change between builds, so the asset cache is trimmed to the newest 80 entries after each store. Caches named `cinevault-*` other than the current two are removed when a new worker takes over.
- **Updates of the worker.** `/sw.js` is served `no-cache` and the worker calls `skipWaiting` and `clients.claim`, so a changed worker takes over on the next start.
- **The offline page** is a self-contained HTML file with the window ground, text and gold colours of the tokens for dark and light, a 44px Try again button, and nothing to load. The server serves it without a session and does not treat it as the app page (no transport marker).
- **Tests.** The registration rule is unit-tested with a stand-in host; the worker file itself runs in a test against a small stand-in for the service worker scope and Cache Storage.

## Risks / Trade-offs

- A worker that misbehaves could keep serving stale files. The cache holds only content-hashed names and never `index.html`, so a new build always loads its own files.

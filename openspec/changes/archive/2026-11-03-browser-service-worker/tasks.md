## 1. Worker and offline page

- [x] 1.1 Add `ui/public/sw.js` (hashed assets cache-first with a trim to 80, offline page for failed page loads, nothing else touched, old `cinevault-*` caches removed on activate) and `ui/public/offline.html`. Verify: `ui/src/lib/serviceWorker.test.ts` runs the worker file against a stand-in scope.
- [x] 1.2 Register it from `main.tsx` only in the browser version over HTTPS with service workers. Verify: `serviceWorker.test.ts` registration cases.

## 2. Server

- [x] 2.1 Serve `sw.js` (`no-cache`) and `offline.html` without a session, and do not treat the offline page as the app page. Verify: `cv-web` test `the_app_can_be_installed_from_a_browser`.

## 3. Integration

- [x] 3.1 Run `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p cv-web`, the UI lint, typecheck, format check, tests and build, and `openspec validate browser-service-worker --strict`. Verify: every command passes.
- [ ] 3.2 Needs a human with an HTTPS setup (a reverse proxy in front of the server): the installed app starts, and with the server stopped it shows "Can't reach CineVault".

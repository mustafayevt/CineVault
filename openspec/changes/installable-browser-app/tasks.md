## 1. Manifest and icons in the build

- [x] 1.1 Add `ui/public/manifest.webmanifest` and copies of `apple-touch-icon.png`, `icon-192.png`, `icon-512.png`, `favicon.ico` and `favicon.svg` from `assets/brand/favicon/`. Verify: `ui/src/installable.test.ts` checks the copies match, the manifest fields and colours against `tokens.css`, the icons exist, and that `ui/index.html` links no manifest; `bun run build` puts them in `ui/dist`.

## 2. Server

- [x] 2.1 Serve the fixed-name install files without a session with `public, max-age=86400` and the manifest as `application/manifest+json`; hashed assets stay `immutable`. Verify: `cv-web` test `the_app_can_be_installed_from_a_browser`.
- [x] 2.2 Add the install head tags (`auth::INSTALL_HEAD`) to the browser shell and to the sign-in, welcome and away pages. Verify: `cv-web` tests `every_server_page_can_install_the_app` and `the_app_can_be_installed_from_a_browser`.

## 3. Integration

- [x] 3.1 Run `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p cv-web`, the UI lint, typecheck, format check, tests and build, `openspec validate installable-browser-app --strict`, and fetch the files from a `cinevault-server` started on a throwaway data folder. Verify: every command passes and the files answer 200 with the expected content type and cache header.
- [ ] 3.2 Needs a human with a phone: on an iPhone, Add to Home Screen from the sign-in page shows the CineVault name and icon and opens full screen, and signing in inside it sticks; on Android, Chrome offers Install app.

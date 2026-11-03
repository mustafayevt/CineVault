## Context

`cinevault-server` and Browser access in the desktop app serve the Vite build (`ui/dist`, or the copy embedded in the desktop app) through `static_file`, which lets only `assets/*`, `favicon.*` and `robots.txt` through without a session and adds the transport marker to `index.html`. The sign-in, welcome and away pages are HTML strings in `auth.rs`. `ui/index.html` is shared by the desktop app and the browser.

## Decisions

- **Files in `ui/public/`.** Vite copies `public/` into the build as is, so the desktop embed and `ui/dist` both carry them. They are copies of `assets/brand/favicon/`; a UI test fails when a copy drifts. The 512 icon has its glyph well inside the maskable safe zone (a circle of 40% radius), so the same file is listed once as `any` and once as `maskable`.
- **Public by an explicit list.** `static_file` keeps a fixed list of public names (the five icon files, the manifest and `robots.txt`) next to the hashed `assets/` folder. Browsers fetch the manifest without cookies, so it cannot need a session, and none of these files holds library data. The list is exact names, so no other file becomes public.
- **Cache.** Hashed assets stay `immutable`. The fixed-name public files get `public, max-age=86400`, so a changed icon or manifest reaches an installed app within a day. `index.html` stays `no-store`.
- **Head tags from the server.** One constant (`auth::INSTALL_HEAD`) is added by the server to `index.html` together with the transport marker, and to each of its own pages. `ui/index.html` stays as it is, so the desktop window does not link a manifest.
- **iOS full screen without layout work.** The tags ask for the standalone mode with a `black` status bar. `viewport-fit=cover` is left out on purpose: without it iOS keeps the content inside the safe areas, so nothing sits under the notch or the home indicator before the phone layout adds safe-area padding.
- **Colours.** The manifest and `theme-color` use the window ground of the tokens (`--surface-window`, dark `#121211`, light `#f2f1ee`); the UI test checks the manifest against `tokens.css`.

## Risks / Trade-offs

- The layout is still built for desktop sizes, so an installed app on a phone shows the desktop layout until the phone layout lands. Installing is still useful on tablets and as a full-screen shortcut.
- A home-screen app on iOS has its own cookie jar, so people sign in once more inside it. Sessions last 30 days idle and 90 days at most, as before.

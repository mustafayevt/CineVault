# CineVault landing page

The public website at <https://mustafayevt.github.io/CineVault/>. It is a static page (one HTML, one CSS, a small script, WebP screenshots) served by GitHub Pages, which the **Deploy site** workflow (`.github/workflows/site.yml`) feeds with the contents of this folder. This folder is the source; merging a change to it (or to `CHANGELOG.md`) into `main` publishes it.

- **Download links** point at `https://github.com/mustafayevt/CineVault/releases/latest/download/CineVault-setup-x64.exe` (and `CineVault-x64.msi`, `CineVault-portable-x64.zip`). `bun run release:publish` uploads these version-less copies with every release, and the Linux ones too (`CineVault-x86_64.AppImage`, `CineVault-amd64.deb`, `CineVault-x86_64.rpm`), so the page needs no change for a new version.
- **Version and size** are written into the page as a fallback; `site.js` replaces them from `api.github.com/repos/mustafayevt/CineVault/releases/latest` when it loads. Update the fallback (`data-version`, `data-size` in `index.html`) now and then.
- **Screenshots** in `img/` come from `bun run shots` in `ui/` against the synthetic backend (generated posters only), one per theme (`*-dark.webp`, `*-light.webp`, picked with `prefers-color-scheme`; library, table, show, search, movie, match, rename, naming, drives, torrents and history at 1440×900; the search and naming shots type into the box and click the nav with `eval:` steps of `bun run shots`), converted with the FFmpeg in `.dev-tools/` (`-c:v libwebp -quality 82`). Keep each under about 300 KB. `og.png` is `assets/brand/social/social-preview.png`; the favicons are from `assets/brand/favicon/`.
- **Install scripts:** `scripts/install/install.sh` and `install.ps1` (the one-command server install) are copied to the site root by the workflow, so they are served at `https://mustafayevt.github.io/CineVault/install.sh` and `install.ps1`. They are not in this folder; edit them in `scripts/install/`.
- **Rules:** no gradients, brand tokens from `ui/src/styles/tokens.css`, never name other media-manager apps, keep the TMDB notice in the footer. Torrent search is shown with its own first-visit notice: "You are responsible for what you download and share." The page claims only what the app has; check a new claim against the code before adding it. macOS is shown as coming soon until a DMG is published; the page offers the Windows and Linux downloads.

## Deploy

The page is deployed by the **Deploy site** workflow (`.github/workflows/site.yml`). It runs on every push to `main` that changes `site/**`, `scripts/install/**` or `CHANGELOG.md`, and by hand:

```sh
bun run site:changelog         # changelog.html must match CHANGELOG.md (CI and the workflow run it with -- --check)
gh workflow run site.yml       # redeploy without a change (Actions → Deploy site → Run workflow)
```

The workflow checks `changelog.html` against `CHANGELOG.md`, copies this folder without this README into the Pages artifact and deploys it; it normally takes about a minute. Pages must use the **GitHub Actions** source (Settings → Pages → Build and deployment), or once with `gh api -X PUT repos/mustafayevt/CineVault/pages -f build_type=workflow`. The old `gh-pages` branch is no longer used. Check the page in headless Chrome at 1440, 1024 and 390 px in both themes before merging a change to it.

### The old address

The earlier site, `https://mustafayevt.github.io/CineVault-releases/`, now only redirects. `site-redirect/index.html` (at the repository root) is its one page: a meta refresh, a script that keeps the path and `#` part, and a visible link, in both themes. To put it live, in a clone of `mustafayevt/CineVault-releases`:

```sh
git rm -r docs && mkdir docs
cp ../CineVault/site-redirect/index.html docs/index.html
cp ../CineVault/site-redirect/index.html docs/404.html   # old deep links land on the new site too
git add docs && git commit -m "docs(site): redirect to the new website" && git push
```

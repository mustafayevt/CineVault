# CineVault landing page

The public website at <https://mustafayevt.github.io/CineVault-releases/>. It is a static page (one HTML, one CSS, a small script, WebP screenshots) served by GitHub Pages from the `docs/` folder of `main` in the public repository `mustafayevt/CineVault-releases`. This folder is the source copy.

- **Download links** point at `https://github.com/mustafayevt/CineVault-releases/releases/latest/download/CineVault-setup-x64.exe` (and `CineVault-x64.msi`, `CineVault-portable-x64.zip`). `bun run release:publish` uploads these version-less copies with every release, and the Linux ones too (`CineVault-x86_64.AppImage`, `CineVault-amd64.deb`, `CineVault-x86_64.rpm`), so the page needs no change for a new version.
- **Version and size** are written into the page as a fallback; `site.js` replaces them from `api.github.com/repos/mustafayevt/CineVault-releases/releases/latest` when it loads. Update the fallback (`data-version`, `data-size` in `index.html`) now and then.
- **Screenshots** in `img/` come from `bun run shots` in `ui/` against the synthetic backend (generated posters only), one per theme (`*-dark.webp`, `*-light.webp`, picked with `prefers-color-scheme`; library, show, movie, match, rename, drives, torrents and history at 1440×900), converted with the FFmpeg in `.dev-tools/` (`-c:v libwebp -quality 82`). Keep each under about 300 KB. `og.png` is `assets/brand/social/social-preview.png`; the favicons are from `assets/brand/favicon/`.
- **Rules:** no gradients, brand tokens from `ui/src/styles/tokens.css`, never name other media-manager apps, keep the TMDB notice in the footer. Torrent search is shown with its own first-visit notice: "You are responsible for what you download and share." The page claims only what the app has; check a new claim against the code before adding it. macOS is shown as coming soon until a DMG is published; the page offers Windows downloads only.

## Deploy (maintainers)

```sh
git clone git@github.com:mustafayevt/CineVault-releases.git
cp -r site/. CineVault-releases/docs/      # README.md here is not needed there
rm CineVault-releases/docs/README.md
cd CineVault-releases && git add docs && git commit -m "feat(site): ..." && git push
```

Pages (Settings → Pages, source `main` `/docs`) rebuilds within a minute or two. Check it in headless Chrome at 1440, 1024 and 390 px in both themes before pushing.

<div align="center">

<img src=".github/assets/readme-banner.png" alt="CineVault: every title in its place" width="100%">

# CineVault

**A fast, free and open-source desktop library manager for your movies and TV shows, for Windows and Linux (beta).**

[![Latest release](https://img.shields.io/github/v/release/mustafayevt/CineVault?style=flat-square&label=release)](https://github.com/mustafayevt/CineVault/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/mustafayevt/CineVault/ci.yml?style=flat-square&label=CI)](https://github.com/mustafayevt/CineVault/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-555?style=flat-square)](LICENSE)

**[Download CineVault](https://mustafayevt.github.io/CineVault/)** · [All releases](https://github.com/mustafayevt/CineVault/releases) · [Changelog](CHANGELOG.md)

</div>

<p align="center">
  <img src=".github/assets/library.png" alt="The Library in the dark theme: a poster grid with stage marks, stage filters with counts, and the sidebar with Scrape, Rename, Drives and Torrents" width="100%">
</p>

## What it does

- **Scan → Scrape → Rename.** Every title moves through three stages: New, Scraped, Renamed. Scan finds the movies and shows in your folders, Scrape matches each one online, Rename tidies folders and files from a template.
- **NFO and artwork for Kodi, Jellyfin and Emby.** Each Scrape writes complete NFO files and every artwork type (poster, fanart, logo, clear art, banner, disc art, thumb) in the formats your media centre reads.
- **TV shows.** Show, season and episode pages with missing and unaired episodes, specials and season artwork.
- **Ratings side by side.** TMDB, plus IMDb, Rotten Tomatoes, Metacritic, Letterboxd and Trakt with your own free OMDb or MDBList key.
- **Search as you type.** Type in the search box and matching titles appear at once, grouped, with your recent searches and See all results. Punctuation, spacing, accents and small typos do not matter; messy release names still match, with a confidence score for every candidate.
- **Library your way.** A poster grid with a size you choose, or a table with a Stage column whose columns you can move and resize.
- **HDR details.** The Technical card shows the exact HDR format (Dolby Vision profile, HDR10+, HDR10, HLG), audio and file details read from the file.
- **Drives with safe moves.** Free space per drive and Move to another drive: copy, verify, and only then do the originals go to the Recycle Bin (the Trash on Linux).
- **Deleted files handled your way.** Delete sends a title to the Recycle Bin or Trash, a trash folder on each drive or a trash folder you choose, and old trash can be emptied automatically. Shift+Delete erases a title only after you type a confirmation.
- **History and Undo.** Renames, moves, deletes, artwork and match changes are listed in History, each with Undo.
- **Collections.** A collection appears on its own once you own two or more films of the same set.
- **Torrent search hand-off.** Pick a title from the suggestions, search its releases from inside the app and see whether it is better than your copy; CineVault hands the one you pick to your own torrent app. You are responsible for what you download and share.
- **Desktop feel.** Light and dark themes that follow the system, zoom from 80% to 150%, keyboard chords for everyday actions, and a `cinevault` command line.
- **Naming styles.** One Style control in Settings › Naming (Clean, Detailed, Media-server safe) fills the movie folder, movie file and TV episode templates; edit any of them to make your own.
- **Private by design.** You bring your own provider keys, kept in the OS credential store (the system keyring on Linux). No account, no telemetry.

## Getting started

1. **Install.** Download the installer from the [website](https://mustafayevt.github.io/CineVault/) or the [releases page](https://github.com/mustafayevt/CineVault/releases). The Windows builds are not code-signed yet, so SmartScreen ("More info", then "Run anyway") warns on first launch.
   - **Linux (beta, 64-bit):** Ubuntu 22.04 or newer, Fedora 38 or newer. The AppImage needs FUSE 2 (`libfuse2t64` on Ubuntu 24.04): `chmod +x CineVault-x86_64.AppImage`, then open it; it updates itself from inside the app. Or install the `.deb` (`sudo apt install ./CineVault-amd64.deb`) or the `.rpm` (`sudo dnf install ./CineVault-x86_64.rpm`). If the window stays blank on some graphics drivers, start CineVault with `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
   - **macOS:** a version is planned.
2. **Add your folders and a free TMDB key.** The first-run screen asks where your movies and shows are, then for a [TMDB API key](https://www.themoviedb.org/settings/api) (free; the only key needed to Scrape).
3. **Scan, Scrape, Rename.** Scanning starts on its own. Scrape your titles, check the Rename preview, apply. Everything can be undone from History.

## Run on a home server

CineVault can run headless next to the media files and be used from a browser on another machine.
The server links no Tauri, no WebKit and no display stack, so it fits a NAS, a mini-PC or a small
container.

```yaml
services:
  cinevault:
    image: ghcr.io/mustafayevt/cinevault:latest
    ports: ["8787:8787"]
    environment:
      CINEVAULT_DATA_DIR: /data
      CINEVAULT_WEB_BIND: 0.0.0.0
    volumes:
      - ./cinevault-data:/data
      - /mnt/media:/media
    restart: unless-stopped
```

```bash
docker compose up -d
```

Open `http://<server>:8787`. The first visit creates the password; then the normal setup adds the
library folder, the TMDB key and the naming profile. The library folders must be readable and
writable by the server, because Scan, Scrape and Rename write NFO and artwork next to the media.

A `.deb`, an `.rpm` and a tarball are published with each release for hosts without Docker; see
[`packaging/README.md`](packaging/README.md) for the packages, the systemd unit and the tarball.
The provider keys are kept in an encrypted `secrets.enc`; supply the master key as a Docker secret
or a systemd credential (`CINEVAULT_SECRET_KEY_FILE`) and back it up with the data folder.

The desktop app and the server can share a data folder, but only one may open it at a time. The
server holds a `library-owner.json` lock; the desktop app refuses that folder and the `cinevault`
command line exits with code 2 instead of writing behind the server's back.

Keep the server on the LAN or behind a VPN. For HTTPS, put it behind a reverse proxy and set
`CINEVAULT_WEB_TRUST_PROXY` to the proxy addresses so the session cookie is marked `Secure` and the
sign-in limit uses the real client address. Never port-forward it to the internet.

## Screenshots

<table>
  <tr>
    <td width="50%" valign="top"><img src=".github/assets/library-table.png" alt="The Library as a table with a Stage column of New, Scraped and Renamed tags"><br><sub>Library table: a Stage column, movable and resizable columns</sub></td>
    <td width="50%" valign="top"><img src=".github/assets/search.png" alt="The search box with matching titles grouped as top hit, movies, TV shows and collections"><br><sub>Search as you type, with your recent searches</sub></td>
  </tr>
  <tr>
    <td width="50%" valign="top"><img src=".github/assets/detail.png" alt="A movie page with poster, ratings from several sources, the plot, facts, cast and the Technical card"><br><sub>Movie page: ratings, cast and the Technical card</sub></td>
    <td width="50%" valign="top"><img src=".github/assets/rename.png" alt="The Rename page previewing renames and conflicts, with removed words struck through and added words marked"><br><sub>Rename: a word-level preview, conflicts resolved on the row</sub></td>
  </tr>
  <tr>
    <td width="50%" valign="top"><img src=".github/assets/naming.png" alt="Settings, Naming, with the Style control and the movie folder, movie file and TV episode templates"><br><sub>Naming styles: one choice fills all three templates</sub></td>
    <td width="50%" valign="top"><img src=".github/assets/drives.png" alt="The Drives page with free space per drive and the titles of one drive, biggest first"><br><sub>Drives: free space and safe moves between drives</sub></td>
  </tr>
  <tr>
    <td width="50%" valign="top"><img src=".github/assets/torrents.png" alt="Torrent search with title suggestions under the search field"><br><sub>Torrents: pick the title from the suggestions</sub></td>
    <td width="50%" valign="top"><img src=".github/assets/first-run.png" alt="The first-run setup asking where the movies and shows are"><br><sub>First run: add your folders, then your own provider keys</sub></td>
  </tr>
</table>

<p align="center"><sub>Screenshots are taken against the app's built-in sample library.</sub></p>

## Build from source

You need the [Rust](https://rustup.rs) stable toolchain (MSVC on Windows), [bun](https://bun.sh) and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) (WebView2 on Windows; on Linux the WebKitGTK and `libdbus` development packages).

```sh
bun install && (cd ui && bun install)
bun run tools:fetch      # pinned MediaInfo, FFmpeg and helper tools into .dev-tools/
bun run tauri dev        # run the desktop app
bun run tauri build      # installers under target/release/bundle/
```

The checks to run before a pull request are listed in [CONTRIBUTING.md](CONTRIBUTING.md).

## Contributing

CineVault is open source, and contributions are welcome: bug reports, ideas, documentation and pull requests. Read [CONTRIBUTING.md](CONTRIBUTING.md) first; contributors and AI coding assistants also follow [AGENTS.md](AGENTS.md). Every change, including the maintainer's, lands through a pull request from a branch, and CI must pass before it is merged. Please report security issues privately, as described in [SECURITY.md](SECURITY.md).

## Licence

CineVault is licensed under the [Apache License 2.0](LICENSE). See [NOTICE](NOTICE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

<sub>This product uses the TMDB API but is not endorsed or certified by TMDB.</sub>

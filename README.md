<div align="center">

<img src=".github/assets/readme-banner.png" alt="CineVault: every title in its place" width="100%">

# CineVault

**A fast desktop library manager for your movies and TV shows, for Windows and macOS.**

[![Latest release](https://img.shields.io/github/v/release/mustafayevt/CineVault-releases?style=flat-square&label=release)](https://github.com/mustafayevt/CineVault-releases/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/mustafayevt/CineVault/ci.yml?style=flat-square&label=CI)](https://github.com/mustafayevt/CineVault/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-555?style=flat-square)](LICENSE)

**[Download CineVault](https://mustafayevt.github.io/CineVault-releases/)** · [All releases](https://github.com/mustafayevt/CineVault-releases/releases)

</div>

<p align="center">
  <img src=".github/assets/library.png" alt="The Library in the dark theme: a poster grid with stage marks, stage filters with counts, and the sidebar with Scrape, Rename, Drives and Torrents" width="100%">
</p>

## What it does

- **Scan → Scrape → Rename.** Every title moves through three stages: New, Scraped, Renamed. Scan finds the movies and shows in your folders, Scrape matches each one online, Rename tidies folders and files from a template.
- **NFO and artwork for Kodi, Jellyfin and Emby.** Each Scrape writes complete NFO files and every artwork type (poster, fanart, logo, clear art, banner, disc art, thumb) in the formats your media centre reads.
- **TV shows.** Show, season and episode pages with missing and unaired episodes, specials and season artwork.
- **Ratings side by side.** TMDB, plus IMDb, Rotten Tomatoes, Metacritic, Letterboxd and Trakt with your own free OMDb or MDBList key.
- **Forgiving search.** Find titles whatever the punctuation, spacing, accents or small typos; messy release names still match, with a confidence score for every candidate.
- **HDR details.** The Technical card shows the exact HDR format (Dolby Vision profile, HDR10+, HDR10, HLG), audio and file details read from the file.
- **Drives with safe moves.** Free space per drive and Move to another drive: copy, verify, and only then do the originals go to the Recycle Bin.
- **Deleted files handled your way.** Delete sends a title to the Recycle Bin, a trash folder on each drive or a trash folder you choose, and old trash can be emptied automatically. Shift+Delete erases a title only after you type a confirmation.
- **History and Undo.** Renames, moves, deletes, artwork and match changes are listed in History, each with Undo.
- **Collections.** A collection appears on its own once you own two or more films of the same set.
- **Torrent search hand-off.** Search a release from inside the app and see whether it is better than your copy; CineVault hands the one you pick to your own torrent app. You are responsible for what you download and share.
- **Desktop feel.** Light and dark themes that follow the system, zoom from 80% to 150%, keyboard chords for everyday actions, and a `cinevault` command line.
- **Private by design.** You bring your own provider keys, kept in the OS credential store. No account, no telemetry.

## Getting started

1. **Install.** Download the installer from the [website](https://mustafayevt.github.io/CineVault-releases/) or the [releases page](https://github.com/mustafayevt/CineVault-releases/releases). The builds are not code-signed yet, so Windows SmartScreen ("More info", then "Run anyway") or macOS Gatekeeper (right-click, then Open) warns on first launch.
2. **Add your folders and a free TMDB key.** The first-run screen asks where your movies and shows are, then for a [TMDB API key](https://www.themoviedb.org/settings/api) (free; the only key needed to Scrape).
3. **Scan, Scrape, Rename.** Scanning starts on its own. Scrape your titles, check the Rename preview, apply. Everything can be undone from History.

## Screenshots

<table>
  <tr>
    <td width="50%" valign="top"><img src=".github/assets/detail.png" alt="A movie page with poster, ratings from several sources, the plot, facts, cast and the Technical card"><br><sub>Movie page: ratings, cast and the Technical card</sub></td>
    <td width="50%" valign="top"><img src=".github/assets/rename.png" alt="The Rename page previewing renames and conflicts, with removed words struck through and added words marked"><br><sub>Rename: a word-level preview, conflicts resolved on the row</sub></td>
  </tr>
  <tr>
    <td colspan="2" align="center"><img src=".github/assets/first-run.png" alt="The first-run setup asking where the movies and shows are" width="50%"><br><sub>First run: add your folders, then your own provider keys</sub></td>
  </tr>
</table>

<p align="center"><sub>Screenshots are taken against the app's built-in sample library.</sub></p>

## Build from source

You need the [Rust](https://rustup.rs) stable toolchain (MSVC on Windows), [bun](https://bun.sh) and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
bun install && (cd ui && bun install)
bun run tools:fetch      # pinned MediaInfo, FFmpeg and helper tools into .dev-tools/
bun run tauri dev        # run the desktop app
bun run tauri build      # installers under target/release/bundle/
```

The checks to run before a pull request are listed in [CONTRIBUTING.md](CONTRIBUTING.md).

## Contributing

Bug reports, ideas and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first; contributors and AI coding assistants also follow [AGENTS.md](AGENTS.md). Please report security issues privately, as described in [SECURITY.md](SECURITY.md).

## Licence

CineVault is licensed under the [Apache License 2.0](LICENSE). See [NOTICE](NOTICE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

<sub>This product uses the TMDB API but is not endorsed or certified by TMDB.</sub>

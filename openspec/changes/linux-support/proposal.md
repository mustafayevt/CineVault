## Why

CineVault ships for Windows (and macOS is on its way), but many people keep their media libraries on Linux desktops and home servers with a desktop session. The code base already builds its Unix parts for macOS, so most of the core runs on Linux unchanged; what is missing is the platform layer: the freedesktop Trash with its per-volume folders, a credential store, the uncached read-back, tool downloads for Linux, window chrome, packaging, updates and the release flow. Several of those gaps are not cosmetic: without a Linux keyring feature the `keyring` crate silently keeps keys in memory only, and the `trash` crate copies across file systems into the home Trash when a volume's own Trash cannot be made, which is slow and not the rename-only safety contract CineVault keeps on Windows. This change adds a Linux beta: an AppImage that updates itself and a `.deb` and `.rpm` package, built on Ubuntu in GitHub Actions, published by the existing release flow and offered on the website and in the README.

## What Changes

1. **Trash on Linux (data safety).** Delete and a move's originals go to the freedesktop Trash only when the item can reach a Trash folder on its own file system by a single rename: the home Trash when it is on the same device, else the volume's `$topdir/.Trash/$uid` (a valid shared folder) or `$topdir/.Trash-$uid`. Network and read-only file systems, and volumes where no such folder exists or can be made, use the library folder's `.deletedByCineVault/<batch>/` as today. CineVault writes the `.trashinfo` file and does the rename itself; it never copies an item into a Trash and never falls back to erasing. Restore, presence checks and Empty keep using the `trash` crate's listing.
2. **Paths compared the Linux way.** Path comparisons that decide which file is which (restore, purge, rename, cleanup, NFO ownership, rescan keys) are case-sensitive on Linux; Windows and macOS keep their case-insensitive comparisons.
3. **Volumes and drives on Linux.** Drives are named by their mount point (`/media/<user>/<name>`, `/run/media/<user>/<name>`, `/mnt/<name>`, `/`), an unmounted mount point counts as offline, and rename recovery waits for a drive whose mount point is empty instead of acting on the root file system.
4. **Uncached read-back on Linux** opens the copy with `O_DIRECT` through the safe `OpenOptionsExt::custom_flags`, reusing the aligned reader, with the existing fallback to a normal read.
5. **Credential store on Linux.** Keys live in the Secret Service (GNOME Keyring, KWallet's Secret Service, KeePassXC). With no Secret Service the app says so and never writes a key to disk in plain text.
6. **Tools on Linux.** The tools manifest gains `linux-x86_64` downloads of MediaInfo (the generic Linux shared library), FFmpeg (a static build packed as `.tar.xz`, so the downloader learns `tar.xz`) and the indexer service; `bun run tools:fetch` fetches them on Linux.
7. **Window chrome on Linux.** The window has no native frame, like Windows, and draws its own minimize, maximize and close buttons; "Match system" replaces "Match Windows" for the theme; the bin is called "Trash" in every message on Linux and macOS.
8. **Players and opening.** Play finds mpv and VLC in their usual Linux places (`/usr/bin`, `/usr/local/bin`, `/snap/bin`, Flatpak exports) before the `PATH`; "Open folder" and the system player use `xdg-open` through the opener plugin.
9. **Single instance backstop.** Where the D-Bus session bus is missing (the single-instance plugin then lets a second copy start), a lock file in the data folder stops a second process from opening the library.
10. **Packaging and updates.** `tauri.linux.conf.json` builds an AppImage, a `.deb` and an `.rpm`. An AppImage copy updates itself: the verified download replaces the AppImage file by a same-folder rename and the new version starts. A `.deb` or `.rpm` copy opens the release page instead.
11. **Release flow.** A new `release-linux.yml` (run by hand) builds on `ubuntu-22.04` through the shared `release-build.yml`, writes `SHA256SUMS-Linux.txt` and puts the files on the GitHub Release; `bun run release:publish` adds the AppImage to the signed manifest as `linux-x86_64` and uploads stable copies (`CineVault-x86_64.AppImage`, `CineVault-amd64.deb`, `CineVault-x86_64.rpm`); `bun run bundle:check` checks the Linux configuration and installers; CI gains an Ubuntu job.
12. **Website and README** offer the Linux beta with install notes.

## Capabilities

### New Capabilities

None; Linux is added to existing capabilities.

### Modified Capabilities

- `title-deletion`: the Trash on Linux and its per-volume rules; the bin's name per platform.
- `drives`: drive identity, offline mount points and the uncached read-back on Linux.
- `release-and-updates`: Linux installers, workflow, manifest entry, stable names and the AppImage self-update.
- `data-storage`: the Linux data and configuration folders and the Secret Service as the credential store.
- `metadata-providers`: keys in the Secret Service on Linux.
- `app-shell`: Linux as a supported system, its window chrome, theme label and single-instance backstop.
- `media-info`: Linux tool downloads.
- `playback`: player places on Linux.
- `developer-tooling`: `tools:fetch` on Linux and the Ubuntu CI job.

## Impact

- Rust: `crates/cv-app` (library_ops trash, bin volumes, uncached read, secrets, drives, organize start-up recovery, scanning health, playback, updates), `crates/cv-fs` (a path comparison helper), `crates/cv-rename`, `crates/cv-nfo`, `crates/cv-artwork`, `crates/cv-scan` (case-sensitive comparisons on Linux), `crates/cv-media` (Linux platform, `tar.xz`), `crates/cv-torrent` (indexer service start on Linux), `src-tauri` (Linux config, instance lock, update wiring).
- Dependencies: `keyring` gains `sync-secret-service` and `crypto-rust` (links `libdbus-1` on Linux); `trash` gains `chrono`; `rustix` (safe `getuid`, `O_DIRECT` flag, `access`) and `liblzma` are added. No `unsafe` is added.
- UI: platform detection, title bar, onboarding, Appearance, bin wording in messages.
- Scripts and workflows: `scripts/check-bundle.mjs`, `scripts/release-publish.mjs`, `.github/workflows/{ci,release-build,release-linux}.yml`.
- Docs and site: `README.md`, `AGENTS.md`, `openspec/config.yaml` context, `site/index.html`, `site/site.js`.
- Windows and macOS behaviour does not change, except that macOS messages say "Trash" instead of "Recycle Bin".

## Out of scope

- Linux on ARM (aarch64), Flatpak, Snap and distribution repositories (an APT or RPM repository).
- A portable package for Linux; the `portable` marker keeps working beside a binary but no portable archive is made, and an AppImage is not portable.
- Detecting system-wide copies of FFmpeg and MediaInfo automatically (the user can still point Settings › Tools at `/usr/bin/ffmpeg` or a system `libmediainfo.so.0`).
- Code-signing or GPG-signing the Linux packages; the in-app update is protected by the signed manifest as on Windows.
- Setting `WEBKIT_DISABLE_DMABUF_RENDERER` from inside the app (changing the environment of the running process needs `unsafe` in edition 2024); the README documents it as a launch option instead.
- Moving caches to `$XDG_CACHE_HOME`; caches stay inside the data folder as on the other systems.
- Torrent client integrations beyond what already works through `xdg-open` of magnet links.

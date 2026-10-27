## Context

CineVault is a Tauri 2 app with a Rust core. Its Unix code paths already exist for macOS (`#[cfg(unix)]` volume keys by `st_dev`, Unix sockets for the CLI, process groups for tools, `wchar_t` as `u32` in the MediaInfo FFI), so the workspace is expected to compile on Linux with few changes. What is Windows- or macOS-only today:

| Area | Today | Linux gap |
|---|---|---|
| System bin (`library_ops/trash_bin.rs`) | `trash::delete_all`, tracked through `trash::os_limited` on Windows (and, by the `not(macos)` cfg, already on Linux) | The `trash` crate falls back to the **home Trash with a cross-device copy and delete** when a volume's `.Trash-$uid` cannot be made (`freedesktop.rs` `delete_all_canonicalized`, `move_items_no_replace`); `chrono` is off, so no `DeletionDate` is written |
| Bin volumes (`library_ops/bin_volumes.rs`) | Windows: drive type and Recycle Bin registry settings; elsewhere every path "takes" | No Linux rules: network and read-only mounts, volumes without a usable Trash folder |
| Path comparison | Lower-cased in `trash_bin::same_path`, `cv_fs::volume::inside`, `cv_rename::exec::fs::comparable`, `cv_nfo::ownership`, `cv_artwork::download::record`, `nfo/service.rs`, `actions/cleanup.rs`, `trash_folders.rs`, `cv_scan::rescan::build::path_key`, `drives::is_under` | Linux file systems are case-sensitive: `Movie.mkv` and `movie.mkv` are two files |
| Uncached read-back (`library_ops/uncached.rs`) | `FILE_FLAG_NO_BUFFERING` on Windows, normal read elsewhere | `O_DIRECT` is available through safe `OpenOptionsExt::custom_flags` |
| Keyring (`secrets.rs`, workspace `keyring` features) | `windows-native`, `apple-native` | No Linux store: `keyring` 3 then uses its mock store and keys vanish at exit |
| Tools manifest (`cv-media/src/tools`) | `windows-x86_64`, `macos-*`; zip, tar.gz, tar.bz2 | No `linux-x86_64`; the usual static FFmpeg builds are `.tar.xz`; library file name is `libmediainfo.dylib` on every non-Windows system |
| Drives (`drives.rs`), recovery (`organize/startup.rs`) | Drive letters, shares, `/Volumes/<name>` | Linux mount roots are unknown, so every Linux path is volume `/`; an unmounted mount point is an empty folder that "answers" |
| Window chrome | No frame on Windows (`tauri.windows.conf.json`), overlay title bar on macOS | Base config keeps the native frame, so Linux would show a GTK title bar above the app's own |
| UI platform (`ui/src/features/shell/platform.ts`) | `windows`, `macos`, `other` | Linux is `other`: no caption buttons, "Match Windows" label |
| Updates (`updates/install.rs`) | Silent NSIS setup on Windows, open the installer elsewhere | No AppImage replacement; `.deb`/`.rpm` cannot be updated by the app |
| Release (`release-build.yml`, `release-publish.mjs`, `check-bundle.mjs`) | Windows and macOS | No Linux runner, assets, manifest entry or stable names |
| Single instance | `tauri-plugin-single-instance` | On Linux it uses D-Bus and **launches normally** when the session bus is missing |

Constraints: `unsafe` stays denied (only the MediaInfo FFI module may use it); no other media-manager product is named; Windows and macOS behaviour stays as it is.

## Goals / Non-Goals

**Goals:** a Linux x86_64 beta with the same data-safety guarantees as Windows; AppImage with in-app updates; `.deb` and `.rpm`; Linux CI; release, website and README.

**Non-Goals:** ARM, Flatpak, Snap, package repositories, a portable Linux package, automatic use of system FFmpeg or MediaInfo, setting environment variables inside the running process.

## Decisions

### D1. The Linux Trash contract: rename into a Trash on the same file system, or the drive's trash folder

This is the decision that keeps files from being erased, so it is spelled out in full.

**Rule.** On Linux, the system bin takes a path `P` only when all of these hold, checked right before the send:

1. `P` exists (`symlink_metadata`), and its parent directory is on a **local, writable file system**: the mount entry that holds `P` (longest mount point prefix in `/proc/self/mountinfo`, matched by device as well) is not a network type (`nfs`, `nfs4`, `cifs`, `smb3`, `smbfs`, `9p`, `afs`, `ceph`, `glusterfs`, `fuse.sshfs`, `fuse.rclone`, `davfs`, `fuse.davfs2`, `fuse.gvfsd-fuse`) and not mounted read-only (`ro` in its mount options).
2. A **Trash directory on the same device** as `P`'s parent can be used, chosen in the freedesktop order:
   1. the home Trash `$XDG_DATA_HOME/Trash` (default `~/.local/share/Trash`) when its nearest existing ancestor has the same `st_dev` as `P`'s parent;
   2. else, with `topdir` the mount point of `P`: `topdir/.Trash/$uid` when `topdir/.Trash` is a real directory (not a link) with the sticky bit set, and `$uid` below it is a directory owned by the user or can be created;
   3. else `topdir/.Trash-$uid` when it is a real directory owned by the user, or `topdir` is writable by the user so it can be created (checked with `rustix::fs::access`).
3. When none applies, `P` is refused with a reason ("This drive has no Trash folder CineVault can use", "This is a network drive, which has no Trash", "This drive is read-only"), and `send_or_fallback` moves it to `<library folder>/.deletedByCineVault/<batch>/` exactly as on Windows.

**Send.** CineVault does the freedesktop send itself (`library_ops/xdg_trash.rs`, Linux only) instead of `trash::delete_all`:

1. Create `files/` and `info/` below the chosen Trash directory (`create_dir_all`, mode left to the umask; `.Trash-$uid` itself is created with mode `0700`).
2. Pick a free name `N` (`name`, `name.2`, `name.3`…) by creating `info/N.trashinfo` with `create_new(true)`; write `[Trash Info]`, `Path=<percent-encoded absolute path as given>` and `DeletionDate=<local time %Y-%m-%dT%H:%M:%S>`, then `sync_all`.
3. Reserve `files/N` with a placeholder (`create_new` file for a file or link, `create_dir` for a folder), then `std::fs::rename(P, files/N)`.
4. **Any error after step 2 removes the info file and the placeholder and returns the error; nothing is ever copied.** In particular `EXDEV` (another device after all) is an error, never a copy. `send_or_fallback` then moves the paths that are still in place to the library folder's trash folder (its existing "the bin refused" branch), so the result is always "kept somewhere", never "erased".
5. The `trash` crate stays the reader: `os_limited::list` finds the item by the diff before and after the send (unchanged code in `send_tracked`), `present`, `restore_all` (a rename that refuses to replace) and `purge_all` (Empty, only the recorded ids) work on the info files CineVault wrote. The `chrono` feature of `trash` is switched on so its own writes and reads agree on `DeletionDate`.

**Why not `trash::delete_all`:** when `topdir/.Trash-$uid` cannot be created it moves the item into the home Trash with a recursive copy and a `remove_dir_all` of the original (`move_items_no_replace`). For a 60 GB film on a second disk that is a long, unflushed copy that can fill the home disk; the contract on every platform is that the bin is a rename or not used at all.

**Removable drives.** Unlike Windows, a Linux removable drive keeps its own `.Trash-$uid`, which every Linux file manager uses; such a drive takes the Trash when rule 2 holds. FAT and exFAT drives have no owner or mode bits: `.Trash-$uid` there counts as owned by the user when it was mounted with the user's uid (its `st_uid` equals `getuid()`); otherwise the drive's trash folder is used.

**Size.** The freedesktop Trash has no size limit CineVault can read; `max_bytes` is `None`, and the existing check after the send reports an item that left its place but is not in the Trash.

**Interrupted send.** A crash between steps 2 and 3 leaves an info file without a file, which file managers ignore and `trash::os_limited::list` lists with a missing file; `present` then says "no longer in the Trash" and Undo reports it, while `P` is still in place. A crash after the rename leaves a normal Trash entry that History records only if the journal row was written first; the existing History flow records items after `send_tracked` returns, as on Windows, so such an item is in the Trash and can be restored by any file manager.

**Undo.** Unchanged: `restore_all` renames the item back and refuses when the old path is taken or the item is no longer in the Trash.

### D2. Case-sensitive path comparisons on Linux

A new helper module `cv_fs::paths` gives `same_path`, `is_within` and `key` (the folded form used as a map key). They fold case, separators and the `\\?\` prefix on Windows and macOS, as today, and compare exactly on Linux (separators are already `/`, trailing `/` is ignored). Each comparison listed in the Context table is switched to it. Where a comparison only dedupes or protects (cleanup's protected set, `trash_folders` dedupe) the switch is still made, so that on Linux a second folder that differs only by case is not skipped. Rescan keys (`cv_scan::rescan::build::path_key`) and the database's own fold (`cv_db::repo::library_review::fold`) change only on Linux; Windows and macOS libraries keep their keys.

Rejected: comparing `(st_dev, st_ino)` everywhere. It is right only for paths that exist; restore, planning and journal recovery compare paths that do not exist yet.

### D3. Drives and mount points

`drives::volume_key` learns Linux roots by path alone (for offline drives): `/media/<user>/<name>`, `/run/media/<user>/<name>`, `/mnt/<name>`, else `/`. Online drives keep their key from `sysinfo` (the mount point). The label of a Linux drive is the mount point's last part when the system gives no label. `is_under` uses `cv_fs::paths::is_within`.

`organize::startup::volume_root` uses the same Linux roots, and `volume_reachable` on Linux requires the root to be a **mount point** (its `st_dev` differs from its parent's, or it is `/`), so recovery of a rename batch on an unmounted drive is deferred instead of running against the empty folder on the root file system.

Scanning: on Linux a library folder whose mount root by path (the D3 roots: `/media/<user>/<name>`, `/run/media/<user>/<name>`, `/mnt/<name>`) exists but is **not a mount point** counts as **offline** in the source health check, so a scan of an unmounted drive's empty folder never marks its titles missing. A library folder elsewhere (root `/`) keeps today's rule (online when it is a folder).

### D4. Uncached read-back with `O_DIRECT`

`open_uncached` on Linux opens with `custom_flags(rustix::fs::OFlags::DIRECT.bits() as i32)` (a safe constant) and the existing `AlignedReader`. A file system that refuses `O_DIRECT` at open (tmpfs, some FUSE) falls through to the existing normal open; one that refuses an aligned read (`EINVAL`) triggers the existing `fall_back`. The kernel writes dirty pages before a direct read of the same range, so the read comes from the device. No `posix_fadvise` is needed.

### D5. Secret Service with no plain-text fallback

Workspace `keyring` features become `windows-native`, `apple-native`, `sync-secret-service`, `crypto-rust` (each is ignored on the other systems; Secret Service links `libdbus-1`, so the Linux build needs `libdbus-1-dev` and the packages depend on `libdbus-1-3`). `keyring::Error::NoStorageAccess` and a D-Bus "service unknown" platform failure map to `SecretError::Unavailable`, whose message on Linux reads "No system keyring is running. Install or unlock GNOME Keyring or KWallet, then try again." Keys are never written to the settings file or another file. Rejected: `linux-native` (kernel keyutils), which forgets keys at logout.

### D6. Tools on Linux

`Platform::LinuxX64` (`linux-x86_64`). `ToolId::file_name` on Linux is `libmediainfo.so.0`, `ffmpeg`, `jackett`. `ArchiveFormat::TarXz` (`tar.xz`) decodes with `liblzma` (static build, no system library). Manifest entries: MediaInfo's generic Linux shared library build (the "Lambda" x86_64 zip from the same release as the other platforms, `lib/libmediainfo.so.0.0.0` → `libmediainfo.so.0`), a static FFmpeg build (a pinned BtbN `linux64-gpl` release asset, `bin/ffmpeg`), and the indexer service's `LinuxAMDx64` tar.gz tree. Exact URLs, sizes and SHA-256 are taken from the real downloads when the task is done; a download that fails its checksum installs nothing, as everywhere. The indexer service is started on Linux with `DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1` set on its `Command` (not on the app's own environment), so it runs without the system ICU library.

### D7. Window chrome, platform and wording

`tauri.linux.conf.json` sets `decorations: false` on the main window (Tauri's Linux runtime resizes an undecorated window from its edges). The UI gains a `linux` platform (user agent with `Linux` or `X11`, not `Android`); the title bar and onboarding show `CaptionButtons` on Windows and Linux; drag regions are the existing `data-tauri-drag-region`. The theme option reads "Match system" on Linux. Messages that name the bin take it from one place: "Recycle Bin" on Windows, "Trash" on macOS and Linux (Rust: a `BIN_NAME` constant in `library_ops`; UI: a `{bin}` argument filled by `binName(platform)`). Hand-written CSS that uses `backdrop-filter` also sets `-webkit-backdrop-filter` for older WebKitGTK. Fonts are bundled already, so no system font is assumed.

### D8. Single-instance backstop

On Linux only, after the data folder is resolved and before the database opens, the app takes an exclusive lock on `<data>/instance.lock` (`fs4` `try_lock_exclusive`, held for the life of the process). When the lock is held by another process, the app logs it, shows a native message "CineVault is already running." (dialog plugin) and exits with code 0. The single-instance plugin stays first, so on a normal desktop the second launch still focuses the running window and never reaches the lock.

### D9. AppImage self-update; packages open the release page

`install_method` takes the target OS and the `APPIMAGE` path as inputs (pure and tested everywhere):

- Linux with `APPIMAGE` set to an existing regular file whose folder is writable → `InstallMethod::ReplaceAppImage { target }`.
- Linux otherwise (a `.deb` or `.rpm` install, an unpacked build, a read-only folder) → `OpenInstaller(OpenReason::LinuxPackage)`, which opens the public release page instead of the downloaded file, with "This copy was installed from a package, so it cannot replace itself. The release page was opened; download the new .deb or .rpm there."

`replace_appimage(installer, target)`:

1. `installer.reverify()` and keep the handle;
2. copy it to `<target dir>/.CineVault-update-<version>.AppImage.part` (`create_new`), `sync_all`, set mode `0755`;
3. hash the copy again against the signed SHA-256; on a mismatch remove it and fail;
4. `rename(part, target)` (same folder, atomic; the running AppImage keeps its open inode), then `sync_all` on the folder;
5. start `target` and quit through the normal shutdown.

Interrupted before step 4: the old AppImage is untouched and the `.part` file is removed at the next start (it is in the AppImage's folder and named for CineVault only). Interrupted after step 4: the new version is in place. Undo is not offered (as on Windows); a bad update is fixed by downloading the previous AppImage from the release page. The `linux-x86_64` manifest entry names the AppImage only.

### D10. Release flow

`release-build.yml` accepts `os: linux` and runs on `ubuntu-22.04` (glibc 2.35 baseline; WebKitGTK 4.1 is in its repositories) with `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev libdbus-1-dev libxdo-dev patchelf file rpm`. `release-linux.yml` calls it by hand. Collected files: `bundle/appimage/*.AppImage`, `bundle/deb/*.deb`, `bundle/rpm/*.rpm`, `SHA256SUMS-Linux.txt`. `release-publish.mjs` classifies `.AppImage` as the `linux-x86_64` installer and `.deb`/`.rpm` as uploads, with stable names `CineVault-x86_64.AppImage`, `CineVault-amd64.deb`, `CineVault-x86_64.rpm`. `check-bundle.mjs` reads `tauri.linux.conf.json` and requires `appimage`, `deb`, `rpm`; `--artifacts` on Linux requires all three. The `.deb` depends on `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `libdbus-1-3`; the `.rpm` on `webkit2gtk4.1`, `gtk3`, `dbus-libs`. The AppImage needs FUSE 2 on the host (`libfuse2`, `libfuse2t64` on Ubuntu 24.04), which the README says.

## Risks / Trade-offs

- [The Linux Trash rules refuse a Trash a file manager would use, for example a removable drive mounted with another uid] → the file goes to the drive's `.deletedByCineVault`, with a note naming why; nothing is lost.
- [A file system lies about rename atomicity (some FUSE)] → FUSE types other than the listed network ones are treated as local; a failed rename leaves the file in place and falls back to the trash folder.
- [Case-sensitive comparison changes a decision on Linux that tests only cover on Windows] → each switched call site gets a Linux-only test with two names that differ only by case, and the final review checks every one.
- [WebKitGTK rendering differences, DMABUF renderer crashes under some GPUs and WSLg] → the WSL2 checklist covers it; the README documents `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
- [AppImage needs FUSE 2, missing on fresh Ubuntu 24.04] → documented; the `.deb` is the fallback.
- [Secret Service missing on minimal window managers] → clear message, keys can't be saved; documented.
- [OpenSSL in the AppImage] → `native-tls` uses OpenSSL on Linux; linuxdeploy bundles `libssl` from the Ubuntu 22.04 build host, which works on newer distributions.
- [The `trash` crate changes its id format] → the crate is pinned at major 5 and the Linux live test (opt-in) round-trips send, list, restore and purge.

## Migration Plan

Nothing migrates: Linux is a new platform. Windows and macOS data, settings and library keys are unchanged (every behaviour change is behind `cfg(target_os = "linux")` or the platform test in the UI, except the macOS wording "Trash").

## Open Questions

- Whether the MediaInfo "Lambda" build loads on every target distribution without extra libraries; the WSL2 checklist verifies it on Ubuntu, and the fallback is to point Settings › Tools at the system `libmediainfo.so.0`.

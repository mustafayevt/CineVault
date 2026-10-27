## How to run these tasks

Read this block once; every task below relies on it.

- **Where:** only in the worktree `C:\Work\Personal Projects\CineVault-linux` (branch `linux`). Never touch `C:\Work\Personal Projects\CineVault`. Never push.
- **Before Rust work:** `export PATH="$HOME/.cargo/bin:$PATH"` (Git Bash). Windows checks: `cargo fmt --all`, `cargo clippy -p <crate> --all-targets -- -D warnings`, `cargo test -p <crate>`.
- **Linux checks (WSL):** code under `#[cfg(target_os = "linux")]` is not compiled on Windows. Every task that adds such code also runs its crate on Linux through WSL (set up by task 0.1):
  `wsl.exe -d Ubuntu -- bash -lc 'cd "/mnt/c/Work/Personal Projects/CineVault-linux" && export CARGO_TARGET_DIR=$HOME/.cache/cv-linux-target && cargo test -p <crate> <filter> && cargo clippy -p <crate> --all-targets -- -D warnings'`
  Call this **LINUX CHECK** below. If WSL is not set up yet, write "LINUX CHECK pending" in the commit body and leave the task unticked for task 9.1 to finish.
- **Keep Linux logic testable on Windows:** put decisions in pure functions that take plain inputs (strings, numbers, paths) and compile on every OS; keep the `cfg(target_os = "linux")` part to thin I/O wrappers.
- **No `unsafe`** (only `crates/cv-media/src/mediainfo/ffi.rs` may use it). No `std::env::set_var` (it is `unsafe` in edition 2024). `thiserror` in libraries. Never name other media-manager products.
- **UI checks** (inside `ui/`): `bun run lint`, `bun run typecheck`, `bun run format:check`, `bun run test`.
- **Commit:** one commit per task, Conventional Commits, pathspec only: `git add <exact files> && git commit -m "<message>" -- <exact files>`. No AI attribution, no `Co-Authored-By`. Tick the task's box in this file in the same commit.
- **Load:** at most two agents run cargo at once (`.cargo/config.toml` already limits jobs).
- **Risk marks:** `[haiku]` = a small model can do it alone. `[risky: needs Sonnet/Opus]` = decides whether user files are kept or erased, replaces the app binary, or changes path identity; a stronger model does it, with the review in 9.2.
- **Parallel waves:** tasks in the same wave touch disjoint files and may run at the same time once their "After" tasks are done.

| Wave | Tasks |
|---|---|
| 0 | 0.1 (human), 1.1 |
| 1 | 2.1, 2.4, 3.1, 4.1, 4.2, 5.1, 5.3, 5.5, 5.6, 6.1, 6.4, 7.2, 8.1 |
| 2 | 2.2, 3.2, 3.6, 3.7, 3.8, 4.3, 5.2, 5.4, 6.2 |
| 3 | 2.3, 3.3, 4.4, 4.5, 6.3, 7.1 |
| 4 | 3.4, 8.2 |
| 5 | 3.5 |
| 6 | 9.1, then 9.2, then 9.3 (human) |

## 0. Linux environment

- [ ] 0.1 WSL2 Ubuntu toolchain for Linux checks `[human]`
  - Who: the owner (needs `sudo`). Nothing is committed.
  - Steps (in Windows Terminal): `wsl --install -d Ubuntu-24.04` if missing; inside Ubuntu: `sudo apt update && sudo apt install -y build-essential curl file pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev libdbus-1-dev libxdo-dev patchelf rpm libfuse2t64 gnome-keyring dbus-x11 mpv`; `curl https://sh.rustup.rs -sSf | sh -s -- -y`; `curl -fsSL https://bun.sh/install | bash`.
  - Verify: `wsl.exe -d Ubuntu -- bash -lc 'cargo --version && bun --version && pkg-config --modversion webkit2gtk-4.1'` prints three versions.

## 1. Dependencies

- [x] 1.1 Add the Linux dependencies in one place `[haiku]`
  - After: nothing. Blocks every Rust task.
  - Touch: `Cargo.toml` (root), `crates/cv-app/Cargo.toml`, `crates/cv-media/Cargo.toml`, `crates/cv-scan/Cargo.toml`, `crates/cv-db/Cargo.toml`, `src-tauri/Cargo.toml`, `Cargo.lock`.
  - Do NOT touch: any `.rs` file. Later tasks do not edit `Cargo.toml` or `Cargo.lock`, so they can run in parallel.
  - Steps:
    1. Root `Cargo.toml` line 73 `trash`: features `["coinit_apartmentthreaded", "chrono"]`; update its comment to "Recycle Bin, macOS Trash, freedesktop Trash (read side on Linux)".
    2. Root line 75 `keyring`: features `["windows-native", "apple-native", "sync-secret-service", "crypto-rust"]`; comment: "Windows Credential Manager, macOS Keychain, Linux Secret Service (links libdbus-1)".
    3. Add to `[workspace.dependencies]`: `rustix = { version = "1", default-features = false, features = ["std", "fs", "process"] }` with comment "Safe getuid, access and O_DIRECT on Linux (cv-app)"; `liblzma = { version = "0.4", default-features = false, features = ["static"] }` with comment "tar.xz tool downloads (cv-media)". Check the newest 0.x of `liblzma` on crates.io; if `static` is not a feature name there, use the one that builds liblzma from source.
    4. `crates/cv-app/Cargo.toml`: add `[target.'cfg(target_os = "linux")'.dependencies]` with `rustix.workspace = true` (copy the shape of the `cfg(windows)` block at line 48).
    5. `crates/cv-media/Cargo.toml`: `liblzma.workspace = true` under `[dependencies]` next to `bzip2` (line 11).
    6. `crates/cv-scan/Cargo.toml` and `crates/cv-db/Cargo.toml`: `cv-fs.workspace = true` (used by 3.8; check `cargo tree -p cv-fs` shows no dependency on either, so there is no cycle). `src-tauri/Cargo.toml`: `fs4.workspace = true` (used by 5.6).
  - Accept: `cargo check --workspace` passes on Windows; LINUX CHECK with `cargo check --workspace --all-targets` passes (this is the first proof the workspace builds on Linux; list any compile error in the commit body and leave fixing to 9.1).
  - Commit: `build(deps): add Linux keyring, trash, rustix and liblzma features`

## 2. Tools on Linux

- [x] 2.1 Linux platform in the tools manifest code `[haiku]`
  - After: 1.1. Parallel with wave 1.
  - Touch: `crates/cv-media/src/tools/manifest.rs` only.
  - Do NOT touch: `manifest.json`, `archive.rs`.
  - Steps:
    1. `Platform` (line 150): add `LinuxX64`; `ALL` gets 4 entries; `current()` returns it for `all(target_os = "linux", target_arch = "x86_64")`; `key()` → `"linux-x86_64"`.
    2. `ToolId::file_name` (line 83): split the non-Windows branch: on `target_os = "linux"` MediaInfo is `"libmediainfo.so.0"`; macOS keeps `"libmediainfo.dylib"`. Update the doc comments at lines 23 and 72 to list `libmediainfo.so.0`.
    3. If a test asserts every platform in `ALL` has a manifest entry, make it skip `LinuxX64` until 2.3 lands (comment `// 2.3 adds linux-x86_64`), or keep it and land after 2.3.
    4. Tests: `Platform::LinuxX64.key() == "linux-x86_64"`; parsing a manifest JSON with a `linux-x86_64` platform returns its spec from `platform_spec(.., Platform::LinuxX64)` (copy the JSON-building helper used by the tests near line 668).
  - Accept: `cargo test -p cv-media --lib tools::manifest`; LINUX CHECK `cargo test -p cv-media --lib tools::manifest`.
  - Commit: `feat(tools): add the linux-x86_64 tool platform`

- [x] 2.2 Unpack `.tar.xz` downloads `[haiku]`
  - After: 2.1 (same crate, enum in `manifest.rs`).
  - Touch: `crates/cv-media/src/tools/archive.rs`, `crates/cv-media/src/tools/manifest.rs` (the `ArchiveFormat` enum only).
  - Do NOT touch: `manifest.json`, `install.rs`, `download.rs`.
  - Steps:
    1. `ArchiveFormat` (manifest.rs, after line 186): add `#[serde(rename = "tar.xz")] TarXz`.
    2. `extract_files` (archive.rs line 57): add `ArchiveFormat::TarXz => { let file = ...; extract_tar(liblzma::read::XzDecoder::new(file), files, dest_dir) }`, copying the `TarGz` arm at line 70 exactly.
    3. Any other `match` on `ArchiveFormat` (search the crate) gets the new arm.
    4. Tests in archive.rs's test module: build a small tar in memory with the `tar` crate, compress with `liblzma::write::XzEncoder`, extract one `files` entry and, for `mode: "tree"`, a tree; assert contents. Copy the existing tar.gz test (search `TarGz` in the tests) and change only the encoder.
  - Accept: `cargo test -p cv-media --lib tools::archive`; `cargo test -p cv-media --test tools_tree`.
  - Commit: `feat(tools): unpack tar.xz downloads`

- [ ] 2.3 Pin the Linux tool downloads `[haiku]` (needs network)
  - After: 2.1, 2.2.
  - Touch: `crates/cv-media/src/tools/manifest.json` only.
  - Do NOT touch: any `.rs` file. Never guess a hash.
  - Steps:
    1. For each package add `"linux-x86_64"` next to the existing platforms, using the same `version` as the Windows entry where the vendor publishes Linux builds of it:
       - mediainfo: `https://mediaarea.net/download/binary/libmediainfo0/26.10/MediaInfo_DLL_26.10_Lambda_x86_64.zip` (format `zip`). Download it into a fresh empty folder under the scratch/temp dir, list it (`unzip -l`), and map the real `.so` (expected `lib/libmediainfo.so.0.0.0` or similar) to `"to": "libmediainfo.so.0"` with `"provides": "mediainfo"`, plus its licence file to `LICENSE.html` or `LICENSE.txt`. If that URL is 404, look at `https://mediaarea.net/download/binary/libmediainfo0/26.10/` for the generic Linux x86_64 DLL archive and use it.
       - ffmpeg: a pinned BtbN release asset `https://github.com/BtbN/FFmpeg-Builds/releases/download/<dated autobuild tag>/ffmpeg-n<8.x>-latest-linux64-gpl-<8.x>.tar.xz` (format `tar.xz`), `bin/ffmpeg` → `ffmpeg` (`provides: ffmpeg`), `LICENSE.txt`. Use a dated tag, never `latest`. Set `version` to the FFmpeg version it contains.
       - indexer-service: `https://github.com/Jackett/Jackett/releases/download/v0.24.2903/Jackett.Binaries.LinuxAMDx64.tar.gz`, format `tar.gz`, `mode: "tree"`, `root: "Jackett/"`, `program: { "provides": "indexer-service", "path": "jackett" }` (copy the `macos-x86_64` entry's shape).
    2. For each: `sha256sum` and `stat -c %s` (or PowerShell `Get-FileHash`, `(Get-Item).Length`) of the downloaded file → `sha256` and `size`.
    3. Delete the downloads afterwards.
  - Accept: `cargo test -p cv-media` passes (the manifest parser test reads the embedded JSON); LINUX CHECK `bun run tools:fetch -- --dir $HOME/.cache/cv-tools` then `ls` shows `libmediainfo.so.0`, `ffmpeg`, the `Jackett` tree; LINUX CHECK `CINEVAULT_LIVE_TOOLS=1 cargo test -p cv-media --test tools_live`.
  - Commit: `feat(tools): pin MediaInfo, FFmpeg and the indexer service for Linux`

- [x] 2.4 Start the indexer service without system ICU on Linux `[haiku]`
  - After: 1.1. Wave 1.
  - Touch: `crates/cv-torrent/src/indexer_service/process.rs` only.
  - Steps: in `service_command` (line 111), add under `#[cfg(target_os = "linux")]` `command.env("DOTNET_SYSTEM_GLOBALIZATION_INVARIANT", "1");` (on the child's `Command`, never on the app's environment), with a one-line comment why. Add a `#[cfg(target_os = "linux")]` unit test that builds the command and checks `get_envs()` contains the pair (copy the arrangement of an existing `service_command` test if there is one; otherwise build `StartParams` as other tests in the file do).
  - Accept: `cargo test -p cv-torrent --lib indexer_service`; LINUX CHECK same filter.
  - Commit: `fix(torrent): run the indexer service in invariant globalization mode on Linux`

## 3. Trash and path identity (data safety)

- [x] 3.1 Path comparison and mount helpers in cv-fs `[haiku]`
  - After: 1.1. Wave 1.
  - Touch: new `crates/cv-fs/src/paths.rs`, `crates/cv-fs/src/lib.rs` (one `pub mod paths;` line).
  - Do NOT touch: `crates/cv-fs/src/volume.rs` (3.7 switches it), any other crate.
  - Steps — public functions, each with doc comments and tests:
    1. `pub const CASE_SENSITIVE: bool = cfg!(target_os = "linux");`
    2. `pub fn key_with(path: &str, case_sensitive: bool) -> String`: `\` → `/`, strip a leading `//?/` (the `\\?\` prefix after the swap) and `UNC/` to `//`, trim trailing `/` (keep a lone `/`), lower-case when `!case_sensitive`. `pub fn key(path: &Path) -> String` = `key_with(&path.to_string_lossy(), CASE_SENSITIVE)`.
    3. `pub fn same_path(a: &Path, b: &Path) -> bool` = keys equal. `pub fn is_within(path: &Path, folder: &Path) -> bool` = equal or `path_key` starts with `folder_key + "/"` (empty folder key → false). Copy the logic of `cv_fs::volume::inside` (volume.rs line 172) but through `key`.
    4. `pub fn linux_mount_root(path: &str) -> String`: by path alone, `/media/<user>/<name>`, `/run/media/<user>/<name>`, `/mnt/<name>` (first two or three components kept), else `/`. Pure, compiled everywhere.
    5. `#[cfg(unix)] pub fn is_mount_point(path: &Path) -> bool`: `true` for `/`; else `std::fs::metadata(path).dev() != std::fs::metadata(parent).dev()`; `false` when either cannot be read. Copy the `MetadataExt` use at volume.rs line 80.
  - Tests (all OSes, through `key_with` with both flags): `Movie.mkv` vs `movie.mkv` differ when case-sensitive and match when not; `D:\Movies\` equals `d:/movies`; `\\?\D:\x` equals `D:\x`; `is_within("/a/bc", "/a/b")` is false; mount roots for `/media/alex/Films/Movies/x.mkv` → `/media/alex/Films`, `/run/media/alex/Films/TV` → `/run/media/alex/Films`, `/mnt/nas/Movies` → `/mnt/nas`, `/home/alex/Movies` → `/`. A `#[cfg(unix)]` test: `is_mount_point(Path::new("/"))` is true.
  - Accept: `cargo test -p cv-fs`; LINUX CHECK `cargo test -p cv-fs`.
  - Commit: `feat(fs): add platform-aware path comparison and Linux mount helpers`

- [x] 3.2 Linux rules for which volume can keep a Trash item `[risky: needs Sonnet/Opus]`
  - After: 1.1. Wave 2.
  - Touch: new `crates/cv-app/src/library_ops/xdg_dirs.rs`, `crates/cv-app/src/library_ops/bin_volumes.rs`, `crates/cv-app/src/library_ops/mod.rs` (module line only).
  - Do NOT touch: `trash_bin.rs`, `policy_bin.rs`, Windows code in `bin_volumes.rs`.
  - Read first: design.md D1 (the whole decision), `bin_volumes.rs` (all), `trash_bin.rs` lines 1-160 and 442-545.
  - Steps:
    1. `bin_volumes.rs`: add `NoBin::NoTrashFolder` and `NoBin::ReadOnly`; `bin_refusal` words: Network → on non-Windows "This is a network drive, which has no Trash"; `NoTrashFolder` → "This drive has no Trash folder CineVault can use"; `ReadOnly` → "This drive is read-only". Keep the Windows strings exactly.
    2. `xdg_dirs.rs` pure part (compiled everywhere, `#[cfg_attr(not(target_os = "linux"), allow(dead_code))]`):
       - `struct MountEntry { mount_point: String, fs_type: String, read_only: bool }` and `fn parse_mountinfo(text: &str) -> Vec<MountEntry>` for `/proc/self/mountinfo` (field 5 is the mount point with `\040`-style octal escapes decoded, the optional fields end at `-`, then fs type, source, super options; read-only when the per-mount options (field 6) contain `ro`).
       - `fn entry_for<'a>(path: &str, entries: &'a [MountEntry]) -> Option<&'a MountEntry>`: longest mount point that is `path` or a parent of it (component-wise, case-sensitive).
       - `fn is_network_fs(fs_type: &str) -> bool` with the list in design D1 (`nfs`, `nfs4`, `cifs`, `smb3`, `smbfs`, `9p`, `afs`, `ceph`, `glusterfs`, `fuse.sshfs`, `fuse.rclone`, `davfs`, `fuse.davfs2`, `fuse.gvfsd-fuse`).
       - `enum TrashChoice { Home(PathBuf), Shared(PathBuf), Own(PathBuf) }` and `fn choose_trash(facts: &TrashFacts) -> Result<TrashChoice, NoBin>` where `TrashFacts` holds plain booleans/paths gathered by the I/O part (home trash same device; `topdir/.Trash` is dir, not link, sticky; `$uid` subdir exists-and-owned or creatable; `.Trash-$uid` exists-as-real-dir-owned or topdir writable; network; read-only). Order exactly as design D1 rule 2.
    3. `xdg_dirs.rs` I/O part under `#[cfg(target_os = "linux")]`: `pub(crate) fn trash_dir_for(path: &Path) -> Result<PathBuf, NoBin>` reads `/proc/self/mountinfo`, gathers `TrashFacts` with `symlink_metadata`, `MetadataExt::{dev, uid, mode}` (sticky bit `0o1000`), `rustix::process::getuid()`, `rustix::fs::access(topdir, Access::WRITE_OK)`, home trash = `dirs::data_dir()/Trash` (nearest existing ancestor's `dev`), then calls `choose_trash`. Creating folders is NOT done here (3.3 does it).
    4. `bin_volumes.rs`: replace the `#[cfg(not(windows))]` impl (line 80) by two impls: macOS keeps `Takes { max_bytes: None }`; Linux returns `Takes { max_bytes: None }` when `trash_dir_for(path)` is `Ok`, else `None(reason)`. Update the module doc (lines 1-16) for Linux.
  - Tests (pure, on Windows too): mountinfo parsing of a fixture with an escaped space, an `nfs4` line, a `ro` line; `entry_for` picks `/media/alex/Films` over `/`; every branch of `choose_trash` incl. "home trash on same device wins", "sticky .Trash with uid dir", "non-sticky .Trash ignored", ".Trash-uid creatable", "nothing usable → NoTrashFolder", "network → Network", "read-only → ReadOnly". A `#[cfg(target_os = "linux")]` test: a temp dir under `$HOME` resolves to the home Trash.
  - Accept: `cargo test -p cv-app --lib library_ops::bin_volumes library_ops::xdg_dirs`; LINUX CHECK same filters.
  - Commit: `feat(trash): decide on Linux which volumes can keep Trash items`

- [ ] 3.3 Rename-only freedesktop Trash sender `[risky: needs Sonnet/Opus]`
  - After: 3.2. Wave 3.
  - Touch: new `crates/cv-app/src/library_ops/xdg_trash.rs`, `crates/cv-app/src/library_ops/mod.rs` (module line only).
  - Do NOT touch: `trash_bin.rs` (3.4 wires it), `bin_volumes.rs`, `xdg_dirs.rs` beyond calling it.
  - Steps (design D1 "Send", step by step):
    1. `pub(crate) fn move_into(trash_dir: &Path, path: &Path, now: &str) -> Result<PathBuf, String>` (compiled on all Unix, tested with temp dirs; `now` is the formatted `DeletionDate` so tests are deterministic): create `files/` and `info/` (for a `.Trash-$uid` dir that does not exist yet, create it with `std::os::unix::fs::DirBuilderExt::mode(0o700)`); loop `n = 1..`: name `N` = file name, then `name.2`, `name.3`…; `OpenOptions::create_new` on `info/N.trashinfo` (on `AlreadyExists` continue); write `[Trash Info]\nPath=<encoded>\nDeletionDate=<now>\n`, `sync_all`; reserve `files/N` (`create_new` empty file for a file or link, `create_dir` for a real folder; on `AlreadyExists` remove the info file and continue); `std::fs::rename(path, files/N)`; on any error remove the placeholder and the info file and return `Err` naming the path. Return the info file path. Never call `std::fs::copy`.
    2. `fn encode_path(path: &Path) -> String`: percent-encode every byte except unreserved ASCII and `/` (copy the rule of the `trash` crate's `encode_uri_path`, freedesktop spec: RFC 2396 escaping).
    3. `#[cfg(target_os = "linux")] pub(crate) fn send_all(paths: &[PathBuf]) -> Result<(), String>`: for each path, `trash_dir_for` (3.2) → `move_into(dir, path, &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string())`; stop at the first error and return it (the paths not sent stay in place for `send_or_fallback`).
  - Tests (Unix; run in WSL): a file and a folder land in `files/` with matching `info/*.trashinfo` holding the encoded path and date; a second item of the same name becomes `name.2`; a path with a space and `%` round-trips through decoding; rename failure (source missing, or `trash_dir` on a read-only temp dir) leaves no info file and no placeholder; the source is never copied (assert the inode of the moved file equals the original's).
  - Accept: `cargo test -p cv-app --lib library_ops::xdg_trash` on Windows compiles (tests are `#[cfg(unix)]`); LINUX CHECK same filter.
  - Commit: `feat(trash): send items to the freedesktop Trash by rename only`

- [ ] 3.4 Use the Linux sender, exact path matching and the bin's name `[risky: needs Sonnet/Opus]`
  - After: 3.1, 3.3. Wave 4.
  - Touch: `crates/cv-app/src/library_ops/trash_bin.rs`, `crates/cv-app/src/library_ops/policy_bin.rs` (strings only), `crates/cv-app/src/library_ops/mod.rs` (export `BIN_NAME`).
  - Do NOT touch: the macOS impl (line 280-345) logic, `send_or_fallback`'s control flow.
  - Steps:
    1. `mod.rs`: `pub const BIN_NAME: &str = if cfg!(windows) { "Recycle Bin" } else { "Trash" };`.
    2. `trash_bin.rs`: every user-facing string with "Recycle Bin" (lines 36, 103, 171, 228, 260, 505-509, 520-522, 529-532) uses `BIN_NAME` via `format!`; keep the Windows text byte-for-byte (tests compare it). `NOT_IN_BIN` stays a `const` ("It is no longer in the Recycle Bin or Trash").
    3. `OsTrash::send` (line 156) and the `delete_all` call in `send_tracked` (line 177): on `target_os = "linux"` call `super::xdg_trash::send_all(paths)`; elsewhere keep `trash::delete_all`. Nothing else in `send_tracked`, `present`, `restore`, `purge` changes.
    4. `same_path` (line 640): replace the body with `cv_fs::paths::same_path(a, b)` and drop its `cfg_attr`.
    5. Module docs (lines 1-17, 26, 48-50): mention the freedesktop Trash and the rename-only rule.
  - Tests: the existing fake-bin tests keep passing on Windows; add a `#[cfg(target_os = "linux")]` test that `same_path` treats `Movie.mkv` and `movie.mkv` as different; add `#[cfg(target_os = "linux")] #[ignore]` live test `bin_live_round_trip_through_the_trash` copying the Windows live test at line 985 (send a temp folder under `$HOME`, `present` true, `restore` puts it back, `purge` removes only it). Run it in WSL with `-- --ignored`.
  - Accept: `cargo test -p cv-app --lib library_ops`; LINUX CHECK `cargo test -p cv-app --lib library_ops` and `cargo test -p cv-app --lib bin_live_round_trip -- --ignored` (needs a Trash under `$HOME`; it is created).
  - Commit: `feat(trash): use the freedesktop Trash on Linux and match its items exactly`

- [ ] 3.5 PolicyBin and fallback tests for Linux refusals `[risky: needs Sonnet/Opus]`
  - After: 3.4. Wave 5.
  - Touch: `crates/cv-app/src/library_ops/policy_bin.rs` (tests module only), `crates/cv-app/src/library_ops/tests/safety.rs`.
  - Do NOT touch: non-test code. If a test exposes a bug, stop and write it in the commit body for 9.2 instead of fixing it here, unless the fix is under 20 lines in `policy_bin.rs`.
  - Steps: with the `fake` bin (trash_bin.rs line 651) and a `BinVolumes` stub (copy the one at trash_bin.rs line 913), cover: (a) `DeletedMode::Bin` with a `NoTrashFolder` refusal → item in `<root>/.deletedByCineVault/<batch>/` with the note naming the reason and the word `Trash` on non-Windows; (b) a bin that refuses mid-batch (`send_tracked` error) → items still in place go to the trash folder, none erased; (c) `DeletedMode::Custom` on another device → `OTHER_VOLUME` and the drive's trash folder (use two temp dirs; on Unix compare `st_dev` and skip with a message when equal); (d) restore of a `folder:` item whose original differs only by case from an existing file, on Linux, restores to its own path and leaves the other file untouched; (e) after every case, assert the total bytes of the inputs equal the bytes found in place plus in the bin plus in the trash folder (nothing lost).
  - Accept: `cargo test -p cv-app --lib library_ops`; LINUX CHECK same.
  - Commit: `test(trash): cover Linux refusals and fallbacks of the deleted-files policy`

- [ ] 3.6 Case-sensitive comparisons in the renamer on Linux `[risky: needs Sonnet/Opus]`
  - After: 3.1. Wave 2.
  - Touch: `crates/cv-rename/src/exec/fs.rs` only (`cv-fs` is already a dependency of cv-rename).
  - Do NOT touch: `crates/cv-rename/src/conflict/mod.rs` (its `case_insensitive` is already `cfg!(any(windows, target_os = "macos"))` at line 219).
  - Steps: `comparable` (fs.rs line 327) lower-cases only when `!cv_fs::paths::CASE_SENSITIVE`; `is_within` and `same_path` (lines 317-325) follow automatically. Read every caller (`exec/run.rs` lines 974-1033, `exec/run/transfer.rs` lines 188 and 273) and confirm in the commit body that a case-only difference on Linux now means "another path" and that no caller then renames over an existing file (the executor's own existence checks must still run; quote the line that does it).
  - Tests: `#[cfg(target_os = "linux")]` tests in `exec/edge_tests.rs` or `safety_tests.rs` (copy a test that renames a folder): a library with `Example (2020)/movie.mkv` and `Example (2020)/Movie.mkv`, renaming `movie.mkv` to the template result `Movie.mkv` is a conflict and both files survive with their bytes.
  - Accept: `cargo test -p cv-rename`; LINUX CHECK `cargo test -p cv-rename`.
  - Commit: `fix(rename): compare paths case-sensitively on Linux`

- [ ] 3.7 Case-sensitive comparisons in cv-app and cv-fs on Linux `[risky: needs Sonnet/Opus]`
  - After: 3.1. Wave 2.
  - Touch: `crates/cv-fs/src/volume.rs` (`inside`, line 172), `crates/cv-app/src/nfo/service.rs` (`same_path`, line 727), `crates/cv-app/src/artwork/collection.rs` (lines 74-78), `crates/cv-app/src/artwork/seasons.rs` (line 169), `crates/cv-app/src/actions/cleanup.rs` (lines 148, 325, 344), `crates/cv-app/src/trash_folders.rs` (line 99), `crates/cv-app/src/nfo/readback.rs` (line 344), `crates/cv-app/src/nfo/rewrite.rs` (line 465).
  - Do NOT touch: `library_ops/*` (3.4 owns `trash_bin.rs`), `drives.rs` (4.3), `title_guard.rs` and `secrets.rs` (they key files of the app itself; leave them).
  - Steps: replace each hand-made fold with `cv_fs::paths::{key, same_path, is_within}`; keep behaviour on Windows and macOS identical (the helper folds there). For map keys use `cv_fs::paths::key`.
  - Tests: per file, one `#[cfg(target_os = "linux")]` test with two names that differ only by case (copy the nearest existing test in that file); existing tests must pass unchanged on Windows.
  - Accept: `cargo test -p cv-fs` and `cargo test -p cv-app --lib nfo artwork actions trash_folders`; LINUX CHECK the same.
  - Commit: `fix(app): compare file paths case-sensitively on Linux`

- [ ] 3.8 Case-sensitive comparisons in cv-nfo, cv-artwork, cv-scan and cv-db on Linux `[risky: needs Sonnet/Opus]`
  - After: 3.1. Wave 2.
  - Touch: `crates/cv-nfo/src/ownership.rs` (line 109), `crates/cv-artwork/src/download/record.rs` (line 17), `crates/cv-artwork/src/download/named.rs` (line 116), `crates/cv-scan/src/rescan/build.rs` (`path_key`, line 16), `crates/cv-scan/src/rescan/mod.rs` (`is_under`, line 260), `crates/cv-db/src/repo/library_review.rs` (`fold`, line 100). The `cv-fs` dependency of cv-scan and cv-db comes from 1.1.
  - Do NOT touch: `crates/cv-db/src/repo/search.rs` (text search, not paths), anything in `cv-app`.
  - Steps: as 3.7. For `path_key` and `library_review::fold`, Windows and macOS keys must stay byte-identical (existing libraries rely on them); only Linux stops lower-casing. If `library_review::fold` is mirrored by SQL (`lower(path)` in a query), make the SQL side match on Linux too or note in the commit body why it is not needed.
  - Tests: one Linux-only case test per function; existing tests unchanged.
  - Accept: `cargo test -p cv-nfo -p cv-artwork -p cv-scan -p cv-db`; LINUX CHECK the same.
  - Commit: `fix(scan): keep case in path keys on Linux`

## 4. Platform services

- [x] 4.1 Uncached read-back with `O_DIRECT` on Linux `[haiku]`
  - After: 1.1. Wave 1.
  - Touch: `crates/cv-app/src/library_ops/uncached.rs` only.
  - Steps: in `open_uncached` (line 131) add a `#[cfg(target_os = "linux")]` block shaped like the Windows one (lines 132-145): `std::os::unix::fs::OpenOptionsExt::custom_flags(rustix::fs::OFlags::DIRECT.bits() as i32)`, `read(true)`; on success return `AlignedReader::new(file, native, block)`. The rest (fallback open, `fall_back` on `EINVAL`) stays. Update the module doc (lines 1-13) and the "Uncached read-back" wording.
  - Tests: existing tests run on Linux through the new path (WSL's ext4 home supports `O_DIRECT`); add `#[cfg(target_os = "linux")]` test that a file under `/dev/shm` (tmpfs refuses `O_DIRECT`) still reads back whole.
  - Accept: `cargo test -p cv-app --lib library_ops::uncached`; LINUX CHECK same (run with `TMPDIR=$HOME/.cache/cv-tmp` so temp files are on ext4).
  - Commit: `feat(move): read copies back with O_DIRECT on Linux`

- [x] 4.2 Secret Service errors on Linux `[haiku]`
  - After: 1.1. Wave 1.
  - Touch: `crates/cv-app/src/secrets.rs`, `crates/cv-app/src/secrets/tests.rs`.
  - Steps:
    1. Add `SecretError::NoKeyring` with `#[error("No system keyring is running. Install or unlock GNOME Keyring or KWallet, then try again.")]`.
    2. `from_keyring` (line 101): on `target_os = "linux"`, `NoStorageAccess(_)` and `PlatformFailure(e)` whose text contains `org.freedesktop.secrets`, `ServiceUnknown`, `was not provided by any`, `DBUS_SESSION_BUS_ADDRESS` or `Connection refused` map to `NoKeyring`; keep other mappings. Put the text test in a pure `fn is_missing_service(text: &str) -> bool` tested on all OSes.
    3. Docs (lines 1-16, 135): add the Secret Service.
    4. Check `crates/cv-app/src/provider_credentials.rs` line 20: `NoKeyring` becomes `CredentialError::Store(message)`; confirm the Settings page shows that message text (grep `"store"` in `src-tauri/src/providers_ipc.rs` line 265 and the UI's handling); if the UI shows a fixed text instead, note it in the commit body for 5.4.
  - Accept: `cargo test -p cv-app --lib secrets`; LINUX CHECK `cargo test -p cv-app --lib secrets`; in WSL without a keyring (`env -u DBUS_SESSION_BUS_ADDRESS`), a tiny test `#[ignore]` saving a key returns `NoKeyring`.
  - Commit: `feat(secrets): use the Secret Service on Linux and say when none is running`

- [ ] 4.3 Drives on Linux `[haiku]`
  - After: 3.1. Wave 2.
  - Touch: `crates/cv-app/src/drives.rs`, `crates/cv-app/src/drives/tests.rs`.
  - Steps: `volume_key` (line 181): before returning `"/"`, use `cv_fs::paths::linux_mount_root` for paths starting with `/media/`, `/run/media/`, `/mnt/` on every OS (pure; a Windows path never starts so). `is_under` (line 230): `cv_fs::paths::is_within`. Label (line 272): on Linux use the mount point's last part when `disk.name()` is empty or starts with `/dev/`. Key comparisons at lines 515-610 and 773-778 that lower-case: use `cv_fs::paths::key`. Docs at lines 7-9.
  - Tests: `volume_key("/media/alex/Films/Movies") == "/media/alex/Films"`, `/run/media/...`, `/mnt/nas/x`, `/home/alex/x == "/"`; Windows keys unchanged; the grouping test (copy "Two folders on one drive") with Linux paths.
  - Accept: `cargo test -p cv-app --lib drives`; LINUX CHECK same.
  - Commit: `feat(drives): name Linux drives by their mount point`

- [ ] 4.4 Rename recovery waits for unmounted Linux drives `[risky: needs Sonnet/Opus]`
  - After: 3.1. Wave 3.
  - Touch: `crates/cv-app/src/organize/startup.rs` only.
  - Steps: `volume_root` (line 78): after the `/Volumes/<name>` branch, on every OS map `/media/…`, `/run/media/…`, `/mnt/…` through `cv_fs::paths::linux_mount_root`. `volume_reachable` (line 102): on `target_os = "linux"` require `root.is_dir() && cv_fs::paths::is_mount_point(&root)`; other systems unchanged. Read `cv_rename::exec::recover_with` and say in the commit body what an unreachable volume causes (deferral) and that no write happens below an unreachable root.
  - Tests: pure `volume_root` cases for the Linux roots (all OSes); a Linux test that a temp dir (same device as its parent) under a fake `/mnt`-shaped path is not reachable — build it as `volume_reachable` over a temp dir that is not a mount point, by testing a helper `fn reachable_root(root: &Path) -> bool` you extract.
  - Accept: `cargo test -p cv-app --lib organize::startup`; LINUX CHECK same.
  - Commit: `fix(rename): defer recovery on unmounted Linux drives`

- [ ] 4.5 Unmounted Linux mount points are offline `[risky: needs Sonnet/Opus]`
  - After: 3.1. Wave 3.
  - Touch: `crates/cv-app/src/scanning.rs` (the probe at line 984 and its helpers only).
  - Do NOT touch: `SourceState` logic at line 956 beyond what the probe returns.
  - Steps: where a source path is probed as "is a folder", on `target_os = "linux"` also require that `linux_mount_root(path)` is `/` or `is_mount_point(root)` (design D3). Read how a scan uses the health answer and confirm in the commit body that an offline source is skipped by Scan and never marks titles missing.
  - Tests: extract `fn answers(path: &Path, root_is_mounted: bool) -> bool` and test it on all OSes; a Linux test with a temp dir.
  - Accept: `cargo test -p cv-app --lib scanning`; LINUX CHECK same.
  - Commit: `fix(scan): treat an unmounted Linux drive as offline`

## 5. Desktop shell and UI

- [x] 5.1 Linux Tauri configuration `[haiku]`
  - After: nothing. Wave 1.
  - Touch: new `src-tauri/tauri.linux.conf.json`, `src-tauri/src/chrome.rs` (tests and doc comment).
  - Do NOT touch: `tauri.conf.json`, the Windows or macOS config.
  - Steps:
    1. Copy `tauri.windows.conf.json`'s `app.windows[0]` (with `"decorations": false`). `bundle`: `"targets": ["appimage", "deb", "rpm"]`, `"linux": { "appimage": { "bundleMediaFramework": false }, "deb": { "depends": ["libwebkit2gtk-4.1-0", "libgtk-3-0", "libdbus-1-3"], "section": "video" }, "rpm": { "depends": ["webkit2gtk4.1", "gtk3", "dbus-libs"] } }`. Check the key names against the Tauri 2 config schema (`https://schema.tauri.app/config/2`) and fix any that differ.
    2. `chrome.rs`: doc (lines 1-7) mentions Linux; add a test copying `platform_windows_keep_the_base_title_and_sizes_after_merge` (line 56) for `tauri.linux.conf.json`, asserting `decorations == false` and the min size 1100×680.
  - Accept: `cargo test -p cinevault-app chrome`; `node scripts/check-bundle.mjs` still passes.
  - Commit: `feat(shell): add the Linux window and bundle configuration`

- [ ] 5.2 Bundle check for Linux `[haiku]`
  - After: 5.1. Wave 2.
  - Touch: `scripts/check-bundle.mjs` only.
  - Steps: read `tauri.linux.conf.json` (line 20 pattern); `need(linux, "tauri.linux.conf.json", ["appimage", "deb", "rpm"])`; include it in `configText` (line 92); require `release-linux.yml` in the list at line 107 **only if it exists** until 7.1 lands, then always (7.1 removes the condition); `--artifacts` on `process.platform === "linux"`: `[["appimage", ".AppImage"], ["deb", ".deb"], ["rpm", ".rpm"]]`.
  - Accept: `node scripts/check-bundle.mjs` passes; temporarily removing `"rpm"` from the Linux targets makes it fail (revert).
  - Commit: `build(bundle): check the Linux bundle configuration`

- [x] 5.3 Linux platform in the UI `[haiku]`
  - After: nothing. Wave 1.
  - Touch: `ui/src/features/shell/platform.ts`, `ui/src/features/shell/platform.test.ts`, `ui/src/features/shell/TitleBar.tsx`, `ui/src/features/onboarding/OnboardingPage.tsx`, `ui/src/features/settings/AppearanceSection.tsx`, `ui/src/i18n/messages/en/settings.ts` (one new key only).
  - Steps: `Platform` = `"windows" | "macos" | "linux" | "other"`; `platformFromUserAgent`: `/Android/` → other, `/Linux|X11/` → linux. `TitleBar.tsx` line 96 and `OnboardingPage.tsx` line 294: show `CaptionButtons` for `windows` or `linux`. Appearance (line 31): `linux` → new key `settingsPage.appearance.matchSystem` = "Match system" (add after line 680). Search the UI for other `platform === "windows"` checks and treat `linux` like Windows where it is about window chrome or Ctrl shortcuts; list them in the commit body.
  - Tests: `platform.test.ts` expects `linux` for `Mozilla/5.0 (X11; Linux x86_64)` (it currently expects `other`, line 11) and `other` for an Android UA; a TitleBar test (copy an existing one if present) renders caption buttons for `linux`.
  - Accept: `bun run lint && bun run typecheck && bun run format:check && bun run test` in `ui/`.
  - Commit: `feat(ui): recognise Linux for window chrome and the theme label`

- [ ] 5.4 One name for the bin per platform `[haiku]`
  - After: 5.3. Wave 2.
  - Touch: `ui/src/i18n/messages/en/{artwork,delete,drives,history,library,scrape,settings}.ts` (only strings that say "Recycle Bin"), `ui/src/features/delete/deleteActions.ts`, `ui/src/features/delete/PermanentDeleteDialog.tsx`, `ui/src/features/drives/drivesModel.ts`, `ui/src/features/drives/EmptyTrashDialog.tsx`, `ui/src/features/history/HistoryPage.tsx`, `ui/src/features/history/historyBackend.ts`, `ui/src/features/move-to/runMove.ts`, `ui/src/features/settings/DeletedFilesSection.tsx`, new `ui/src/features/shell/binName.ts` (+ test).
  - Do NOT touch: `ui/src/bindings.ts` (generated), synthetic/gallery fixtures unless a test needs them.
  - Note: the main checkout has uncommitted edits in `ui/src/i18n/messages/en/library.ts`; keep the change to that file to the one string so the later merge is trivial.
  - Steps: `binName(platform)` returns "Recycle Bin" for `windows` and "Trash" otherwise; replace "Recycle Bin" in each message by `{bin}` and pass `bin: binName(detectPlatform())` where the message is formatted. Windows output must be byte-identical. "Delete to Recycle Bin" becomes "Delete to {bin}".
  - Tests: `binName` unit test; one rendered message per platform (copy an existing i18n/message test); `ui/src/i18n/verbs.test.ts` still passes.
  - Accept: UI checks as in 5.3.
  - Commit: `feat(ui): call the bin Trash on macOS and Linux`

- [x] 5.5 WebKitGTK CSS prefixes `[haiku]`
  - After: nothing. Wave 1.
  - Touch: hand-written CSS with `backdrop-filter`: `ui/src/features/title/title.css`, and any `.css` under `ui/src/styles` that sets it (search `backdrop-filter` in `*.css`); not `.tsx` (Tailwind adds prefixes).
  - Steps: next to each `backdrop-filter: X;` add `-webkit-backdrop-filter: X;` before it. No colour or value changes.
  - Accept: UI checks; `bun run build` succeeds; `grep -c` of `-webkit-backdrop-filter` equals that of `backdrop-filter:` lines in the touched files.
  - Commit: `style(ui): add WebKit backdrop-filter prefixes for Linux web views`

- [x] 5.6 Instance lock backstop on Linux `[haiku]`
  - After: nothing. Wave 1.
  - Touch: new `src-tauri/src/instance_lock.rs`, `src-tauri/src/boot.rs`, `src-tauri/src/lib.rs` or `main.rs` (module line only; check which declares modules).
  - Steps: `pub fn acquire(data_root: &Path) -> Result<Option<std::fs::File>, String>` using `fs4::fs_std::FileExt::try_lock_exclusive` on `<root>/instance.lock` (`fs4` comes from 1.1); `Ok(None)` means another process holds it. In `Boot::start` (boot.rs line 25), right after the data location resolves and only on `target_os = "linux"`: hold the file in the `Boot` struct; when held elsewhere, show "CineVault is already running." with `tauri_plugin_dialog` (or log and return `Self::failed` if no app handle is available there — read how `failed` is shown) and exit cleanly.
  - Tests: unit tests for `acquire` on all OSes: second acquire in the same process on another handle returns `Ok(None)` (fs4 locks are per handle), after drop it returns `Some`.
  - Accept: `cargo test -p cinevault-app instance_lock`; LINUX CHECK same.
  - Commit: `feat(shell): stop a second Linux process when the session bus is missing`

## 6. Updates and playback

- [x] 6.1 Update method for Linux copies `[haiku]`
  - After: nothing. Wave 1.
  - Touch: `crates/cv-app/src/updates/install.rs` only.
  - Do NOT touch: `src-tauri/src/updates_ipc.rs` (6.3).
  - Steps: add `pub enum TargetOs { Windows, MacOs, Linux }` with `current()`; change `install_method(exe_dir, installer, os: TargetOs, appimage: Option<&Path>, appimage_dir_writable: bool)`; Linux: `Some(path)` that `is_file()` and writable dir → `InstallMethod::ReplaceAppImage { target: path.to_path_buf() }`; else `OpenInstaller(OpenReason::LinuxPackage)` with the sentence from the release-and-updates spec and code `linux_package`. macOS keeps `NotWindows`. Keep a thin wrapper so the two existing callers in `updates_ipc.rs` (lines 137, 436) still compile: `install_method_for_current(exe_dir, installer)` that reads `APPIMAGE` and checks writability (`tempfile::NamedTempFile::new_in(dir).is_ok()` is not allowed in prod code without the dep; use `std::fs::OpenOptions::create_new` of a `.cv-write-test` file and remove it). Update `install_on_close` to treat `ReplaceAppImage` like `SilentSetup` via a new `start` parameter only in 6.3 — here it returns `Nothing` for it.
  - Tests: table tests for every OS × input combination (copy the existing `install_method` tests near line 300).
  - Accept: `cargo test -p cv-app --lib updates`; `cargo check -p cinevault-app` (callers still compile).
  - Commit: `feat(updates): choose how a Linux copy installs an update`

- [ ] 6.2 Replace the AppImage with the verified download `[risky: needs Sonnet/Opus]`
  - After: 6.1. Wave 2.
  - Touch: `crates/cv-app/src/updates/install.rs` only.
  - Steps (design D9): `pub fn replace_appimage(installer: &VerifiedInstaller, target: &Path) -> Result<(), UpdateError>`: `reverify()` and keep the handle; copy into `<dir>/.CineVault-update-<version>.AppImage.part` opened with `create_new`; `sync_all`; mode `0o755` (`PermissionsExt`, Unix only); hash the copy and compare with the signed SHA-256 (reuse `super::service::hex` and `Sha256` as `reverify` does at lines 80-108); `std::fs::rename(part, target)`; open the folder and `sync_all` it; any error removes the `.part` and returns. `pub fn remove_stale_parts(dir: &Path)` removes only files matching `.CineVault-update-*.AppImage.part`. Unix-only functions under `#[cfg(unix)]`.
  - Tests (Unix, WSL): replacing a fake AppImage in a temp dir leaves the new bytes, mode 755, no `.part`; a corrupted copy (inject via a test hook that flips one byte, or by verifying against a wrong hash) leaves the old file untouched and no `.part`; `remove_stale_parts` leaves other files alone.
  - Accept: `cargo test -p cv-app --lib updates` (Windows compiles); LINUX CHECK same.
  - Commit: `feat(updates): replace a Linux AppImage with the verified update`

- [ ] 6.3 Wire Linux updates into the app `[risky: needs Sonnet/Opus]`
  - After: 6.2. Wave 3.
  - Touch: `src-tauri/src/updates_ipc.rs`, `crates/cv-app/src/updates/install.rs` (`install_on_close` only).
  - Steps: "Update now" (updates_ipc.rs around line 436): `ReplaceAppImage { target }` → status "Installing CineVault <version>…", `replace_appimage`, then spawn `target` (plain `Command::new(target).spawn()`) and quit through the same shutdown path the silent setup uses; `OpenInstaller(LinuxPackage)` → open the public releases page URL (the constant the update service already knows; else `https://github.com/mustafayevt/CineVault-releases/releases/latest`) with the opener's URL call, show the reason. Install-on-close (line 137): pass the Linux start function so `ReplaceAppImage` replaces and relaunches on close. At start-up, when `APPIMAGE` is set, call `remove_stale_parts` on its folder (boot or updates state init; one line).
  - Tests: the existing updates_ipc tests pass; add a test of the decision-to-action mapping with fakes if the file has such tests (copy their pattern).
  - Accept: `cargo test -p cinevault-app updates`; LINUX CHECK same; manual in 9.3.
  - Commit: `feat(updates): update a Linux AppImage in place and send package installs to the release page`

- [x] 6.4 Players in their usual Linux places `[haiku]`
  - After: nothing. Wave 1.
  - Touch: `crates/cv-app/src/playback/player.rs` only.
  - Steps: `Os` (line 195) gains `Linux` (`cfg!(target_os = "linux")`); `known_locations` (line 232): `(Os::Linux, Mpv)` → `/usr/bin/mpv`, `/usr/local/bin/mpv`, `/snap/bin/mpv`, `/var/lib/flatpak/exports/bin/io.mpv.Mpv`, `$HOME/.local/share/flatpak/exports/bin/io.mpv.Mpv`; `(Os::Linux, Vlc)` → `/usr/bin/vlc`, `/usr/local/bin/vlc`, `/snap/bin/vlc`, `/var/lib/flatpak/exports/bin/org.videolan.VLC`, `$HOME/.local/share/flatpak/exports/bin/org.videolan.VLC` (take `$HOME` from `dirs::home_dir()` passed in as a parameter so the function stays pure). Fix any `match` on `Os` that becomes non-exhaustive.
  - Tests: `known_locations(Mpv, Os::Linux, …)` starts with `/usr/bin/mpv`; Windows and macOS lists unchanged.
  - Accept: `cargo test -p cv-app --lib playback`; LINUX CHECK same.
  - Commit: `feat(playback): find mpv and VLC in their Linux locations`

## 7. Release flow

- [ ] 7.1 Linux release and CI workflows `[haiku]`
  - After: 5.2. Wave 3.
  - Touch: `.github/workflows/release-build.yml`, new `.github/workflows/release-linux.yml`, `.github/workflows/ci.yml`, `scripts/check-bundle.mjs` (drop the "only if it exists" condition from 5.2), `AGENTS.md` (Commands and Releases: add `release-linux.yml`, `gh workflow run release-linux.yml`, "Release (Linux)").
  - Steps:
    1. `release-build.yml`: `os` description "windows, macos or linux"; `runs-on`: `windows-latest` / `macos-latest` / `ubuntu-22.04`; new step "Linux build packages" (`if: runner.os == 'Linux'`): `sudo apt-get update && sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev libdbus-1-dev libxdo-dev patchelf file rpm`; "Collect installers" gets a Linux branch copying `target/release/bundle/appimage/*.AppImage bundle/deb/*.deb bundle/rpm/*.rpm`; the sums file is `SHA256SUMS-Linux.txt` (already `$RUNNER_OS`). Update the header comment and the comment above "Build installers".
    2. `release-linux.yml`: copy `release-windows.yml` with `os: linux`, name "Release (Linux)", concurrency group `release-linux-…`, comment listing AppImage, .deb, .rpm.
    3. `ci.yml`: matrix `[windows-latest, macos-latest, ubuntu-22.04]` and the same apt step guarded by `runner.os == 'Linux'`.
  - Accept: `node scripts/check-bundle.mjs` passes; `bunx --bun yaml-lint` is not available, so parse each YAML with `node -e "require('fs')"` plus `bun x js-yaml <file>` (prints JSON, fails on a syntax error).
  - Commit: `ci(release): build the Linux AppImage, deb and rpm`

- [x] 7.2 Publish Linux files and the manifest entry `[haiku]`
  - After: nothing. Wave 1.
  - Touch: `scripts/release-publish.mjs` only.
  - Steps: `classifyAsset` (line 61): `/\.AppImage$/i` → `{ kind: "installer", platforms: ["linux-x86_64"] }` (`aarch64|arm64` → `linux-aarch64`); `/\.deb$/i`, `/\.rpm$/i` → `{ kind: "upload" }`. `stableName` (line 82): AppImage → `CineVault-x86_64.AppImage` (`CineVault-aarch64.AppImage` for arm), deb → `CineVault-amd64.deb` (`-arm64.deb`), rpm → `CineVault-x86_64.rpm` (`-aarch64.rpm`); note the existing `arch` variable is x64/arm64 for Windows names, so compute Linux arch names separately. Header comment (lines 16-19) lists the Linux names.
  - Tests: extend `selfTest` (line 362): files with `CineVault_1.2.0_amd64.AppImage`, `CineVault_1.2.0_amd64.deb`, `CineVault-1.2.0-1.x86_64.rpm` add `linux-x86_64` only for the AppImage; platform keys sort as `darwin-aarch64,darwin-x86_64,linux-x86_64,windows-x86_64`; stable names as above; a stable copy is not classified as a second installer.
  - Accept: `node scripts/release-publish.mjs --self-test` passes.
  - Commit: `feat(release): publish the Linux AppImage in the signed manifest`

## 8. Website and documents

- [x] 8.1 Linux on the website `[haiku]`
  - After: nothing. Wave 1.
  - Touch: `site/index.html`, `site/site.js`, `site/style.css` only if a third card needs layout.
  - Steps: add a Linux card in the download section (after the Windows card, line 284-292 pattern) with "Linux (beta)", "Ubuntu 22.04 or newer, Fedora 38 or newer, x86_64. The AppImage updates itself; it needs FUSE 2 (`libfuse2t64` on Ubuntu 24.04).", buttons to `…/releases/latest/download/CineVault-x86_64.AppImage`, links to `CineVault-amd64.deb` and `CineVault-x86_64.rpm`, and a Tux-free generic icon (inline SVG like the others, `aria-hidden`). `site.js`: `isLinux()` (`/Linux|X11/` and not Android or ChromeOS `CrOS`) → the hero button points to the AppImage with "Download for Linux (beta)"; Mac logic unchanged. Meta descriptions (lines 7, 18) say "for Windows and Linux".
  - Accept: open `site/index.html` with headless Chrome (throwaway profile, see AGENTS.md "Browser checks") at 1440×900 and 390×844 with a Linux user agent (`--user-agent="Mozilla/5.0 (X11; Linux x86_64)"`), screenshot, no console errors; stop Chrome by its PID.
  - Commit: `docs(site): offer the Linux beta downloads`

- [ ] 8.2 README, AGENTS and project context `[haiku]`
  - After: 7.1 (AGENTS.md). Wave 4.
  - Touch: `README.md`, `AGENTS.md`, `openspec/config.yaml` (the `context` text only).
  - Steps: README: tagline "for Windows, macOS and Linux (beta)"; Getting started gets a Linux paragraph (AppImage: `chmod +x`, FUSE 2 package, in-app updates; `.deb`: `sudo apt install ./CineVault-amd64.deb`; `.rpm`: `sudo dnf install ./CineVault-x86_64.rpm`; a running Secret Service (GNOME Keyring or KWallet) to save keys; troubleshooting: blank window → start with `WEBKIT_DISABLE_DMABUF_RENDERER=1`); Build from source lists the apt packages from task 7.1. AGENTS.md: first line "Windows, macOS and Linux"; Setup on a new machine adds the Linux prerequisites; Releases step (2) adds `gh workflow run release-linux.yml`. `openspec/config.yaml` context: "for Windows, macOS and Linux".
  - Accept: `openspec validate --specs --strict` still passes; the README renders (no broken Markdown tables).
  - Commit: `docs: describe the Linux beta`

## 9. Integration and review

- [ ] 9.1 Workspace green on Linux `[risky: needs Sonnet/Opus]`
  - After: every task above except 9.2 and 9.3.
  - Touch: only what the failures need; each fix in its own commit with its own Conventional message.
  - Steps: LINUX CHECK `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Windows `cargo test --workspace` and the UI suite. Tests that are Windows-only by nature get `#[cfg(windows)]`; tests that assume case-insensitive paths get a Linux twin rather than being disabled. Then LINUX CHECK `bun install && cd ui && bun install && cd .. && bun run tauri build` and confirm `target/release/bundle/{appimage,deb,rpm}` hold one file each, then `node scripts/check-bundle.mjs --artifacts`. Finish every task left with "LINUX CHECK pending".
  - Accept: all of the above pass; the commit bodies list what changed.
  - Commit: per fix, e.g. `fix(linux): …` / `test(linux): …`

- [ ] 9.2 Opus review of the whole branch `[risky: needs Opus]`
  - After: 9.1.
  - Scope: `git diff main...linux`, reading every changed code path that writes, moves, renames or deletes files in media folders or replaces the app: `library_ops/{xdg_dirs,xdg_trash,trash_bin,bin_volumes,policy_bin,uncached}.rs`, every switched path comparison (3.6-3.8), `drives.rs`, `organize/startup.rs`, `scanning.rs`, `updates/install.rs`, `updates_ipc.rs`, `instance_lock.rs`.
  - Check against design D1-D9: no copy into any Trash; every failure leaves the item in place or in a `.deletedByCineVault`; no case-folded comparison on Linux decides which file is renamed, restored or purged; recovery never writes below an unmounted root; the AppImage is replaced only by a verified, same-folder rename; no `unsafe`, no `set_var`, no secret in logs; Windows and macOS behaviour unchanged (diff of Windows strings and keys is empty). Every finding gets a test and a fix commit (`fix(trash): …`), or a written reason it is not a problem in the final commit body.
  - Accept: `openspec validate linux-support --strict`; full Windows and LINUX CHECK suites green after the fixes.
  - Commit: per finding; final `docs(openspec): record the Linux review` ticking this box.

- [ ] 9.3 WSL2 manual test (WSLg on Windows 11) `[human]`
  - After: 9.1 (an AppImage, `.deb` and `.rpm` from the local Linux build in 9.1 or from `gh workflow run release-linux.yml -f publish=false`).
  - The owner runs these in Ubuntu under WSL2 and ticks each line in `docs/handoff/manual-checks.md` (create the file if missing):
    - [ ] Start: `chmod +x CineVault_*_amd64.AppImage && ./CineVault_*_amd64.AppImage` opens the window with no system title bar; own minimize, maximize, close work; dragging the title bar moves the window; dragging an edge resizes; the window stops at 1100×680. If the window is blank, retry with `WEBKIT_DISABLE_DMABUF_RENDERER=1` and note which was needed.
    - [ ] Fonts (Archivo, JetBrains Mono), translucent panels, focus rings and scrollbars look like Windows in both themes; "Match system" is the theme label.
    - [ ] `sudo apt install ./CineVault_*_amd64.deb`, start "CineVault" from the app menu (WSLg Start menu entry), check the icon; `sudo apt remove cinevault` (or the package name shown) removes it and leaves `~/.local/share/CineVault`.
    - [ ] Data folder is `~/.local/share/CineVault`; config pointer folder is `~/.config/CineVault`.
    - [ ] Keys: with `gnome-keyring-daemon` running (`dbus-run-session -- bash`, then `echo -n test | gnome-keyring-daemon --unlock`, then start the app), save a TMDB key, restart, it is still there; `secret-tool search service CineVault` shows it. Without a keyring daemon, saving says "No system keyring is running…" and `grep -r <key> ~/.local/share/CineVault` finds nothing.
    - [ ] Tools: first-run setup downloads MediaInfo and FFmpeg; a scan of a folder with a sample `.mkv` fills the Technical card (video, audio, file rows); aspect ratio detection runs (FFmpeg).
    - [ ] Library on ext4 in `$HOME`: Scan, Scrape, Rename with Undo work; a folder with `Movie.mkv` and `movie.mkv` is two files everywhere.
    - [ ] Delete on ext4 in `$HOME`: the title is in `~/.local/share/Trash/files/` with an info file; it shows in the desktop's Trash if one is available (`gio trash --list`); Undo restores it; Drives › Empty erases only CineVault's items (put another file in the Trash first and check it stays).
    - [ ] Second ext4 volume: create `truncate -s 2G ~/vol.img && mkfs.ext4 ~/vol.img && sudo mkdir -p /mnt/cvtest && sudo mount -o loop ~/vol.img /mnt/cvtest && sudo chown $USER /mnt/cvtest`; add `/mnt/cvtest/Movies` as a library folder with a small title; Delete sends it to `/mnt/cvtest/.Trash-$(id -u)/`; Undo restores. Then `sudo chown root /mnt/cvtest && sudo chmod 755 /mnt/cvtest`, remove `.Trash-*`, delete another title: it goes to `/mnt/cvtest/Movies/.deletedByCineVault/` with the note, and nothing was copied to `~/.local/share/Trash` (`du -sh` before and after).
    - [ ] Windows drive through WSL (`/mnt/c/...`, 9p): the drive shows as a network drive; Delete goes to the library folder's `.deletedByCineVault`; Undo restores.
    - [ ] Move to another drive (`$HOME` library → `/mnt/cvtest`): copy, verify (uncached read-back), originals to the Trash; Undo moves back.
    - [ ] Unmounted drive: `sudo umount /mnt/cvtest`; the drive shows Offline; a scan does not mark its titles missing; remount, it comes back Online.
    - [ ] Play with mpv (`/usr/bin/mpv`) and with the system default player; "Open folder" opens the file manager or reports clearly when none is installed.
    - [ ] Second launch of the AppImage while running focuses the first window; in `env -u DBUS_SESSION_BUS_ADDRESS ./CineVault*.AppImage` twice, the second says "CineVault is already running." and exits.
    - [ ] Update: install an older AppImage build (or edit the version down in a local build), point Settings › Updates at a test channel or the real one when a newer release exists, press "Update now": the AppImage file is replaced (same path, new size), the new version starts, library and keys unchanged, no `.CineVault-update-*.part` left. In the `.deb` install, "Update now" opens the release page.
    - [ ] Torrent search (optional feature on): the indexer service downloads, starts and answers; quitting CineVault ends its process (`pgrep -f jackett` is empty).
    - [ ] `cinevault` CLI from a source build (`cargo run -p cinevault-cli -- movie --help`) talks to the running app over its socket.
  - Accept: every line ticked or a note with the failure, filed as a follow-up task.

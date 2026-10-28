# Linux support: final review (task 9.2)

Scope: `git diff main...linux`, read with data safety first: every Linux path that sends, restores,
erases or moves user files, path identity (case), drives and recovery, AppImage self-update,
keyring, instance lock, tool downloads, the publish script, Tauri config, website and UI wording.
Checked against design D1-D9.

Severity: **blocker** (must be fixed before a Linux beta), **should** (fix soon; no data loss),
**note** (accepted, or for later).

## What holds

- **D1 Trash contract.** `xdg_trash::move_into` never copies: info file by `create_new`, a
  placeholder in `files/`, one `rename`; every error removes both and returns, `EXDEV` included.
  `xdg_dirs::trash_choice_with` refuses whenever a fact is missing or disagrees (device of the
  parent vs. mount list, hidden mount points, network or read-only mounts, linked or foreign
  `.Trash`/`.Trash-$uid`). Refused paths go to `.deletedByCineVault` through `send_or_fallback`.
  The `trash` crate only reads: its `list` scans the home Trash and `.Trash/$uid`/`.Trash-$uid` of
  every mount (the same folders CineVault writes), its `restore_all` reserves the old path with
  `create_new`/`create_dir` before renaming, and `purge_all` touches only the recorded ids, matched
  again by original path (`trash_bin.rs` `purge`).
- **Restore never replaces** (`cv_fs::rename_no_replace`, `RENAME_NOREPLACE`); the policy bin and
  trash-folder restores now use it on every platform (Windows keeps check-then-rename).
- **Uncached read-back** with `O_DIRECT`, fallback at open and on `EINVAL`.
- **AppImage update** (`updates/install.rs`): re-verified handle, `create_new` part file in the
  target folder, size and SHA-256 checked on the copy, one same-folder `rename`, folder `fsync`,
  stale parts removed only by exact name and only regular files. The relaunch helper passes the
  path as an argument (`exec "$@"`), not through the shell text. `.deb`/`.rpm` copies open the
  release page.
- **Keyring**: no plain-text fallback; missing Secret Service maps to a clear message.
- **Instance lock**: `fs4` exclusive lock in the data folder, released by the OS on crash.
- **Tools**: tar.xz goes through the existing extractor (regular files only, no absolute or `..`
  names, size limits); pinned URLs with SHA-256 and size.
- No `unsafe`, no `set_var` in the diff; the indexer's `DOTNET_SYSTEM_GLOBALIZATION_INVARIANT`
  is set on its `Command` only.
- Windows SQL is textually unchanged (`path_case::fold` gives `lower(x)` there); Windows strings
  differ only where the bin is named (`{bin}` filled with "Recycle Bin").

## Findings

1. `crates/cv-fs/src/lib.rs:298` (`link_then_unlink`) — open point (a). Scenario: a title folder
   deleted on an NFS share, sshfs, or an NTFS/exFAT drive mounted through FUSE (ntfs-3g,
   exfat-fuse) goes to `.deletedByCineVault`; Undo calls `rename_no_replace`, which gets `EINVAL`
   (no `RENAME_NOREPLACE`) and refused every folder → Undo of a delete failed on most Linux NAS
   libraries (files stayed in the trash folder, nothing lost). The premise that kernel FAT/exFAT
   lack the flag is wrong: the kernel's FAT, exFAT, NTFS3, ext4, Btrfs and XFS drivers all accept
   it; NFS and FUSE do not. Fix: a folder now moves over an empty folder made at the old place first
   (`folder_into_placeholder`, line 325): the `mkdir` fails if anything is there and a rename
   replaces only an empty folder. Files keep the hard-link fallback; a file on a volume with
   neither is still refused (stays in the trash folder, restorable by hand) — accepted for the
   beta. **should**, fixed in cc4f3432.
2. `crates/cv-fs/src/paths.rs:137` (`mount_root_answers`, was `is_mount_point` in
   `drive_is_mounted` and `organize/startup.rs`). Scenario: library at `/mnt/media` that is a
   bind mount of a folder on the same file system, a link, or a folder made by hand → same
   `st_dev` as `/mnt` → the source read as offline forever: never scanned, every title
   unavailable, rename recovery deferred forever. Fix: answers when it is a mount point by device,
   when `/proc/self/mountinfo` names it (bind mounts, automounts), or, below `/mnt` only, when no
   `/etc/fstab` line mounts a drive on it. An unmounted `/media`/`/run/media` folder or fstab
   mount stays offline even with stray files in it (a first attempt keyed on "folder not empty"
   was replaced for that reason). **should**, fixed in 1c85b660 and 48dcd924.
3. `crates/cv-app/src/library_ops/mod.rs:168` (`parts_below`). Scenario on Linux: a path in
   `/data/films` counted as inside the library folder `/data/Films`. It guards Delete, permanent
   delete (`erase.rs` "not inside its library folder") and the trash-folder fallback. Paths come
   from the library, so a mismatch needs a damaged row, but this is a case-folded comparison
   deciding what is erased. Fix: `cv_fs::paths::fold_name` (Windows/macOS unchanged).
   **should**, fixed in 0feb7102.
4. `crates/cv-db/src/schema.sql:484`, `:470`, `:507` — open point (b), the NOCASE columns left on
   Linux. `fingerprint.path` (PK): two case twins in one folder share one row, so each rescan sees
   one of them as changed and reads it again; `delete_fingerprints`/`_of_source` remove a twin's
   row too. Derived data, never referenced by an item: costs work, loses nothing.
   `data_source (kind, path NOCASE)`: two library folders that differ only by case cannot both be
   added (an error, not a merge). `media_unreadable`: an unreadable mark covers a case twin.
   Read-only display queries (`lifecycle.rs:110`, `query.rs:134`, `library_review.rs:54`,
   `media_info.rs:96`) fold for labels and dates only. Real-world risk: case twins of whole library
   folders or of video files are rare on Linux media libraries. Accepted for the beta; a later
   migration can make these columns `BINARY` on Linux. **note**, open.
5. `crates/cv-scan/src/sources.rs:97` — open point (c). The data-source guard folds case on Linux:
   `/media/a/films` is refused as "already added" when `/media/a/Films` exists, and nesting is
   judged ignoring case. Every effect is a refusal (never an acceptance), and it agrees with the
   unique index in finding 4. Accepted. **note**, open.
6. `crates/cv-artwork/src/download/engine.rs:355` (`trash_superseded`) and `set.rs:171`
   (destination dedupe) still fold case on Linux. Folding only makes more files "the same", so
   fewer files are moved to the trash folder and a case-twin destination is skipped: conservative,
   nothing is lost. Other remaining folds (`cv-rename` `order_items`, `undo.rs` trash-folder name
   checks, `trash_folders.rs` `TRASH_DIR` name) only order work or protect. **note**, open.
7. `crates/cv-app/src/library_ops/trash_bin.rs:196` (`send_tracked`). The `trash` crate's `list`
   unwraps a non-UTF-8 mount point and a trash info line without `=`; such a system would panic in
   Delete. Before the send nothing has moved; after it the item is in the Trash (restorable from
   any file manager) but not in History. Third-party; noted for the manual check. **note**, open.
8. `crates/cv-fs/src/paths.rs:23` (`key_with`) turns `\` into `/` on Linux too, where `\` is a legal
   name character: `a\b` and `a/b` share a key. Unlikely in media libraries. **note**, open.
9. `scripts/release-publish.mjs:391` — local mode writes the manifest from the folders given only.
   Publishing Linux alone for a version whose Windows/macOS builds are already public drops their
   entries, so those copies stop being offered that update (they are not harmed). It warns; the
   CI path (no `--from-dir`) downloads every asset of the version and is complete. Suggest merging
   the live manifest's other platforms in local mode. **should**, open.
10. `site/index.html:296` — the Linux card and the Linux hero link to
    `releases/latest/download/CineVault-x86_64.AppImage`, which 404s until a release carries
    Linux files. Deploy the site only after the first Linux publish. **note**, open.
11. `crates/cv-app/src/updates/install.rs:234` — a `.deb`/`.rpm` copy still downloads the
    AppImage before "Update now" opens the release page; wasted download, nothing else. **note**,
    open.
12. `crates/cv-app/src/library_ops/move_engine.rs:2062` — pre-existing (on `main` too): the message
    has a run of spaces inside it ("its verified copy is            in the new place") from a lost
    line continuation. Cosmetic. **note**, open.
13. Under an AppImage, external programs (file manager, players) inherit the AppImage runtime's
    GTK/GIO variables, a known source of failures when opening folders. Added to the 9.3 manual
    check scope ("Open folder"). **note**, open.
14. `crates/cv-fs/src/tests.rs` `a_folder_held_by_an_open_file_below_it_is_renamed_once_the_file_is_closed`
    (Windows, pre-existing) failed once under load from a parallel Linux build and passed on two
    reruns; `src-tauri/src/artwork_stream_ipc/tests.rs:321` (provider order of the artwork
    stream, untouched by this branch) failed the same way once and passed in the quiet full rerun.
    Timing tests. **note**, open.

## Checks after the fixes

Windows: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` green (the full test run on a quiet machine). Linux (WSL Ubuntu 24.04,
TMPDIR on the Linux disk): clippy and `cargo test --workspace` green.
`openspec validate linux-support --strict` passes.

## Verdict

No blocker is open. Three should-level problems were fixed with tests (1-3); one should (9) is a
release-process gap with a warning in place. Data safety on Linux holds: nothing reaches a Trash
by copy, every failure leaves the item in place or in `.deletedByCineVault`, restores never
replace, and the AppImage is replaced only by a verified same-folder rename. Fit for a Linux beta
after the 9.3 manual checks.

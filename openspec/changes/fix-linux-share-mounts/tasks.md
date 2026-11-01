## 1. Linux mount points

- [x] 1.1 `linux_mount_root` takes the deepest folder the mount list names between the top folder and the usual mount point, and the usual mount point when there is none; the pure `linux_mount_root_with` holds the rule. Verified by `cargo test -p cv-fs` (`a_drive_mounted_higher_up_holds_the_path` and the existing mount-root tests).

## 2. Integration

- [ ] 2.1 The issue's setup on Linux (a separate file system mounted at `/media/nas` as the library folder, beside a control folder on `/srv`): every title is online after two scans and the movie page shows no missing notice, where 1.3.0 marked them all missing. `CHANGELOG.md` under `## [1.3.1] - unreleased` and `bun run site:changelog`; `cargo fmt --all`, `cargo lint`, `cargo test --workspace`, `openspec validate fix-linux-share-mounts --strict`. Verified by those commands and CI.

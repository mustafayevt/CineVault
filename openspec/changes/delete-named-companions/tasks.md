## 1. Name rules in the scanner

- [x] 1.1 Public `cv_scan::companion` module: `stem`, `normalized_base` and `is_sibling` moved from the movie walker (which re-exports them), plus `video_bases`, `is_plain_video` and `owner_by_name` (longest base wins, `Ambiguous` on a tie between owners). Verified by `cargo test -p cv-scan` (the walkers' tests unchanged, new unit tests for the collision names).

## 2. Library query

- [ ] 2.1 `cv_db::repo::library_ops::files_in_folder`: every file the library records directly in a folder, by the platform's case rule, not below it. Verified by `cargo test -p cv-db --lib library_ops`.

## 3. Delete

- [ ] 3.1 `library_ops/companions.rs` finds the files named after a title in the folders of its videos; `load` keeps them for titles without a folder of their own, `row_of` counts them and `paths_to_trash` adds them, so the delete to the bin, its History entry, Undo and the permanent delete take them. Verified by `cargo test -p cv-app --lib library_ops` (shared folder, library folder root, episode, the collision names, a sub-folder, the preview count, Undo, permanent delete, an own folder unchanged).

## 4. Integration

- [ ] 4.1 `CHANGELOG.md` under Unreleased and `bun run site:changelog`; `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, the delete and changelog UI tests, `openspec validate delete-named-companions --strict` and `openspec validate --specs`. Verified by those commands and CI.

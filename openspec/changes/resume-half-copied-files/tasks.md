## 1. Continue a half-written copy

- [x] 1.1 `MoveFs::open_append` (real disk and the test shims), and `MoveEngine::copy_file` checks a leftover `.cvpart` against the original's first bytes and appends the rest, or removes it and copies from the start. Verified by `cargo test -p cv-app library_ops` (a resume after a stop mid-file writes only the rest; a part that no longer matches is copied again; a part longer than the original is copied again).

## 2. Move dialog size

- [ ] 2.1 `plan_row` lists the folder and fills in the size before it refuses a title an unfinished move blocks. Verified by a `cv-app` test of the move plan for a blocked title.

## 3. Docs and checks

- [ ] 3.1 `CHANGELOG.md` under 1.6.0 and `bun run site:changelog`; `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p cv-app`, `openspec validate resume-half-copied-files --strict`.

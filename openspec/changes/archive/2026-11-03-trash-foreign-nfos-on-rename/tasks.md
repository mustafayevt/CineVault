## 1. Rename

- [x] 1.1 `plan_file()` marks an NFO the writer may not overwrite as unreadable for the planner, and every app rename plans with NFO clean-up on for movies and TV (`TvSettings::nfo_cleanup` added). Verified by `cargo test -p cv-app` (the info-file tests for a movie, a newer info file and an episode).
- [x] 1.2 The executor's trash step sets aside a foreign NFO and leaves a pointer file. Verified by `cargo test -p cv-rename` and `cargo test -p cv-compat`.

## 2. Docs

- [x] 2.1 `CHANGELOG.md` and `bun run site:changelog`. Verified by `openspec validate trash-foreign-nfos-on-rename --strict`.

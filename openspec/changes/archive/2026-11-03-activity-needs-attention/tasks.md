## 1. Backend: what an unfinished batch says

- [x] 1.1 `UnfinishedBatch` carries the titles' ids, the destination folder, the bytes copied and to copy, and `auto_resume` (a move cut short with no stop reason, its drives reachable and its folders there); `UnfinishedBatchDto` sends them. Verified by `cargo test -p cv-app --lib library_ops::tests::recovery` (a killed move is `auto_resume` with its sizes; a full destination is not; a moved original folder is not) and the DTO test in `src-tauri`.
- [x] 1.2 Start-up sends no warning for a batch the window resumes. Verified by reading `recover_at_startup` and the recovery tests above.

## 2. UI: Activity and Needs attention

- [x] 2.1 A recovery backend (list, resume, roll back) and the actions Resume, Roll back and the start-up resume, with the move's progress state, toasts and refresh. Verified by `src/features/activity/needsAttention.test.tsx` ("the start-up resume").
- [x] 2.2 Needs attention on the Activity page: unfinished batches with Resume and Roll back and live progress, warnings and errors with their fix, Dismiss and Dismiss all; `/activity` with `?batch=`, `/history` leading there. Verified by `needsAttention.test.tsx`, `router.test.tsx` and `historyPage.test.tsx`.
- [x] 2.3 The sidebar's Activity icon with the warning dot. Verified by `sidebar.test.tsx` ("marks Activity with a warning dot").
- [x] 2.4 The Move to menu's fix for a held title, and the toasts of a stopped move and of the start-up warning lead to Activity. Verified by `moveToMenu.test.tsx` ("a title held by a move that did not finish").

## 3. Integration

- [x] 3.1 `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the bindings test, `bun run lint`, `bun run typecheck`, `bun run format:check`, `bun run test`, `openspec validate activity-needs-attention --strict`, and the changelog with `bun run site:changelog`.

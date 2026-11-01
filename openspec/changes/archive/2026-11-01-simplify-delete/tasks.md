## 1. Backend

- [x] 1.1 `erase_titles` and `empty_drives_trash` drop the typed-word argument and the `ERASE_CONFIRMATION` and `EMPTY_CONFIRMATION` constants, in `cv-app`, `cv-ipc`, `src-tauri` and `cv-web` (the browser keeps the password and one-time intent); `ui/src/bindings.ts` regenerated. Verified by `cargo test --workspace` (the erase and empty tests, the browser intent tests) and `generated_bindings_are_up_to_date`.

## 2. One Delete

- [x] 2.1 Remove "Delete permanently…" from the selection bar's More, the right-click menu and the title page's ⋯ menu; label the one entry "Delete"; Shift+Delete acts as Delete. Verified by `bun run test` (library and title action tests, the delete keys test).
- [x] 2.2 The permanent-delete dialog asks Yes or No: no DELETE field and no "Send to Recycle Bin instead"; in a browser it keeps the password. Choosing "Delete permanently" in Settings › Deleted files and the Drives "Empty…" dialogs ask Yes or No too. Verified by `bun run test` (`delete.test.tsx`, the settings and drives tests).

## 3. Docs and integration

- [ ] 3.1 AGENTS.md and CONTRIBUTING.md rules on Delete, the settings wording, `CHANGELOG.md` and `bun run site:changelog`; the README, landing page and screenshots that show a delete menu (`bun run shots`). Verified by `bun run lint`, `bun run typecheck`, `bun run test`, `bun run format:check`, `cargo lint`, `openspec validate simplify-delete --strict`, and the app: Delete under each of the four settings.

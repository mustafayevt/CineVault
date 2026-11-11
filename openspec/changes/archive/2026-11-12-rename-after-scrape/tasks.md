## 1. Backend

- [x] 1.1 `rename_settings`: `RENAME_AFTER_SCRAPE_KEY` and `rename_after_scrape(values)` (off by default, false while Metadata only is on). Verify: `cargo test -p cv-app --lib rename_after_scrape`.
- [x] 1.2 `actions::rename_after::RenameQueue`: gathers titles until 3 seconds of quiet and hands them to the runner as one list. The after-scrape steps push a title once its steps and its artwork download are over. Verify: the queue tests with paused time.
- [x] 1.3 `automation::after_scrape_rename`: the runner (one rename plan per kind of title through `CommandBackend::submit_plan`), its notices (`NoticeAction::UndoRename`), installed by `CommandBackend::start`. Verify: the notice tests; `a_scrape_renames_its_title_when_rename_after_scrape_is_on` in `tests/automation_actions.rs`.

## 2. UI

- [x] 2.1 Settings › Naming: the Rename after Scrape switch under Metadata only, held off while Metadata only is on. Verify: `settingsPage.test.tsx`.
- [x] 2.2 `useBackendNotices.ts`: an `undo_rename` notice is a toast with Undo (the shared `undoRename`), not kept in the notification centre. Verify: `useBackendNotices.test.ts`.

## 3. Integration

- [x] 3.1 `cargo fmt`, `clippy`, `cargo test -p cv-app` and the bindings test; `bun run lint`, `typecheck`, `test`, `format:check`; `openspec validate rename-after-scrape --strict`; the changelog line and `bun run site:changelog`.

## 1. Move to menu

- [x] 1.1 Destination rows (name, folder label and free space; the path shortened in the middle with the whole path as its tooltip; the space bar after the move or the reason in the warning colour), placeholder rows while measuring, the empty and error states, a list that scrolls on its own, and the heading's size taken from a plan that knows it. Verified by `bun run test` (`moveToMenu.test.tsx`, `drivesTitles.test.tsx`).
- [x] 1.2 Opening and closing: a non-modal menu that follows the control that opened it (the toolbar's top edge for the selection bar), closes when that control is pressed again, lets an outside click through, ignores focus moving by itself, returns the focus on Esc, and focuses the first destination when opened from the keyboard. Verified by `bun run test` (`moveToMenu.test.tsx`, `moveToStore.test.ts`) and in the desktop app with a throwaway library.

## 2. Integration

- [x] 2.1 `CHANGELOG.md` under `## [Unreleased]` and `bun run site:changelog`; `bun run typecheck`, `bun run lint`, `bun run test`, `bun run format:check`, `openspec validate fix-move-to-menu --strict`. Verified by those commands and CI.

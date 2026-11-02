## 1. Copy tabs

- [x] 1.1 `CopyTabs` in `ui/src/features/copies`: the group's copies in a stable order (best, larger, lower id), each a link to its Movie page that replaces the history entry, the shown copy current, the best marked. Verified by `bun run test` (`copyTabs.test.tsx`).
- [x] 1.2 The Copies row at the end of the Technical card: Keep only this one (left out when two copies may be one file), Delete this copy (then the first remaining copy's page opens) and Compare. Verified by `bun run test`.
- [x] 1.3 Remove the copies line from the Movie page with its test, and update the messages. Verified by `bun run lint`, `bun run typecheck`, `bun run test`, `bun run format:check`.

## 2. Checks and docs

- [x] 2.1 `bun run check:facts`, `bun run shots -- --routes /movies/5` and `bun run shots:compact` pass; `CHANGELOG.md` and `bun run site:changelog`; `openspec validate movie-copy-tabs --strict`.

## 1. UI

- [x] 1.1 `FilesNotice`: reason line and "Remove from library (keep files)" button for missing files only, on the Movie and TV show pages. Verify: `filesNotice.test.tsx`.
- [x] 1.2 `LibrarySelectionBar` and `TitlesView`: "Remove from library" on the bar in the "Missing files or offline" view. Verify: `titleActions.test.tsx`.

## 2. Integration

- [x] 2.1 `bun run lint`, `typecheck`, `test`, `format:check`, `bun run shots`, `openspec validate remove-missing-titles --strict`, the changelog line and `bun run site:changelog`. Verify: all pass.

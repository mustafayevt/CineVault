## 1. Matcher

- [x] 1.1 `releaseFilter.ts`: parse the typed text into required and excluded words and test a release name against them (lower case, letters and digits only). Verify: unit tests for order, case, punctuation, `-` exclusion and a lone `-`.

## 2. Page

- [x] 2.1 `TorrentsPage.tsx`: the "Filter results" field at the right of the chip row (disabled before the first release, × and Esc clear it, kept per title), the text narrowing before the chips so their counts follow it, and the "No releases contain" state with "Clear filters". i18n messages. Verify: page tests for narrowing, chip counts, Esc, and the empty state.

## 3. Integration

- [x] 3.1 `bun run lint`, `typecheck`, `test`, `format:check` in `ui/`; `bun run shots -- --routes /torrents`; `openspec validate torrent-results-filter --strict`; the changelog line and `bun run site:changelog`. Verify: all pass.

## 1. Catalogue

- [x] 1.1 `cv-db` facet `missing_metadata`: add "Title" (`trim(title) = ''`). Verify: `cargo test -p cv-db facet` (empty and blank titles, negation, counts, a show counted by its own artwork, Clear logo and combined cases for movies and shows).

## 2. Library UI

- [x] 2.1 `libraryStore.ts`: `LibraryFilters.fix`, `fixFacets` and a `facets` argument on `titleQuery`; `TitlesView.tsx` sends them and shows "Nothing to fix here". The synthetic backend understands the two filters. Verify: `needsFixing.test.tsx`.
- [x] 2.2 `LibraryFilterRow.tsx`: the Needs fixing menu with counts, Not scraped as the New stage, and Clear. i18n messages. Verify: menu, count, combine, TV and batch tests in `needsFixing.test.tsx`.

## 3. Integration

- [x] 3.1 `cargo fmt`, `clippy`, `cargo test -p cv-db`; `bun run lint`, `typecheck`, `test`, `format:check` in `ui/`; `bun run shots` for `/library`, `/library?type=tv`; `openspec validate needs-fixing-filters --strict`; the changelog line and `bun run site:changelog`. Verify: all pass.

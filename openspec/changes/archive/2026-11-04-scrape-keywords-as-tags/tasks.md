## 1. Backend

- [x] 1.1 `cv-domain`: Tags are scrapable; applying a scrape adds the scraped tags to the stored ones. Verify: unit test for the merge, a change of match and "write only empty fields".
- [x] 1.2 `cv-app` scrape settings and `cv-providers` universal fields: Tags in the default field lists. Verify: scrape settings tests; the scrape follow-up test writes the NFO with the NFO's tag and the keywords.

## 2. UI

- [x] 2.1 `schema.ts` field lists and the `scrape.field.tags` label. Verify: `bun run test`, `lint`, `typecheck`, `format:check`.

## 3. Integration

- [x] 3.1 `cargo fmt`, clippy and the tests of the touched crates, the compat runner, `openspec validate scrape-keywords-as-tags --strict`, the changelog line and `bun run site:changelog`. Verify: all pass.

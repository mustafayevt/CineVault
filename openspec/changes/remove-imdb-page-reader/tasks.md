## 1. Providers

- [x] 1.1 Remove `crates/cv-providers/src/imdb/`, its exports, fixtures and tests, and `Top250Source` with `Top250List` when nothing else uses them. Verified by `cargo test -p cv-providers` and `cargo lint`.
- [x] 1.2 Drop IMDb from the metadata registration and the artwork order in `cv-app` (with the order tests), remove `top250.rs` and the IMDb cases of `tests/unavailable_providers.rs`, and the `fetch_imdb_top250` command from `cv-ipc` and `src-tauri`; regenerate `ui/src/bindings.ts`. Verified by `cargo test --workspace` (the provider order tests, `generated_bindings_are_up_to_date`) and `cargo test -p cv-compat` (golden files unchanged).

## 2. Wording

- [ ] 2.1 The OMDb hint in Settings › Providers and the onboarding Ratings keys step; any IMDb provider in the UI's synthetic data. Verified by `bun run test` and `bun run typecheck`.

## 3. Integration

- [ ] 3.1 README and landing page wording on IMDb, `CHANGELOG.md` and `bun run site:changelog`; `bun run lint`, `bun run format:check`, `openspec validate remove-imdb-page-reader --strict`; in the app a Scrape search with an OMDb key lists OMDb results and makes no request to imdb.com.

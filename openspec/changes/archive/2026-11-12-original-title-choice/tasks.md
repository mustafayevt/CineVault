## 1. Domain and storage

- [x] 1.1 `ItemCore`: `use_original_title`, `localized_title` and the methods that keep them consistent (`localized_title`, `set_localized_title`, `set_original_title`, `offers_original_title`, `choose_original_title`, `settle_title_choice`); `Item::has_title_choice`. Verify: `cargo test -p cv-domain item::tests`.
- [x] 1.2 Scrape write path: the Title field reads and writes the localized title, Original title keeps the title in use in step. Verify: `cargo test -p cv-domain a_rescrape_keeps_the_original_title`.
- [x] 1.3 Migration 109 (`use_original_title`, `localized_title`), the item repository, the search keys and the FTS title column. Verify: `cargo test -p cv-db` (`migration_109_*`, `the_title_choice_round_trips_*`, `a_title_using_its_original_title_is_still_found_*`).

## 2. Services

- [x] 2.1 Edit details field `title_choice`, applied last in a save, refused without another original title. Verify: `cargo test -p cv-app edit::tests` (`the_title_choice_is_a_field_*`, `the_choice_saved_with_new_titles_*`, `the_original_title_cannot_be_chosen_without_one`).
- [x] 2.2 `EditService::set_title_choice` (one transaction, NFO rewrite of the changed titles) and `titles_in_languages`. Verify: `the_bulk_choice_changes_only_titles_*`, `a_locked_title_still_follows_*`, `the_titles_of_listed_languages_*`.
- [x] 2.3 `metadata.original-title-languages` in `ScrapeSettings` and the first-Scrape rule in `apply_scraped_in`. Verify: `cargo test -p cv-app scrape_settings matching::tests` (`a_first_scrape_in_a_listed_language_*`, `a_rescrape_honours_the_choice_*`).
- [x] 2.4 NFO: `<localized_title>` among CineVault's own elements while the original title is used (movie and show writers, readers, known elements); read-back keeps, restores or ends the choice. Verify: `cargo test -p cv-app nfo::merge` (`reading_back_its_own_nfo_*`, `the_localized_title_in_the_file_restores_*`), `cargo test -p cv-app --test nfo_contract`, `cargo test -p cv-nfo`.
- [x] 2.5 `${title}` follows the choice in the naming engine. Verify: `cargo test -p cv-template the_title_follows_the_title_choice`.

## 3. Commands

- [x] 3.1 `set_title_choice` and `use_original_title_for_languages` in `cv-ipc`, the Tauri commands and the browser server's dispatch; regenerate `ui/src/bindings.ts`. Verify: `cargo test -p cinevault-app generated_bindings`, `cargo test -p cv-web`.

## 4. UI

- [x] 4.1 Edit details: "Title to use" (Localized | Original) and the "Localized title" label when there is a choice. Verify: `editDetailsPage.test.tsx`.
- [x] 4.2 "Use original title" and "Use localized title" under More and in the right-click menu, with the toast and Undo. Verify: `titleActions.test.tsx`.
- [x] 4.3 Settings › Providers "Keep the original title" with "Use for titles I have". Verify: `originalTitleLanguages.test.tsx`.
- [x] 4.4 Settings › Naming: {OriginalTitle} and {ShowOriginalTitle} buttons and the explaining line. Verify: `namingModel.test.ts`.

## 5. Integration

- [x] 5.1 `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the tests of the touched crates and `cargo test -p cv-compat` (golden files unchanged); `bun run lint`, `typecheck`, `test`, `format:check`; `bun run shots` for `/movies`, a movie page and its Edit page; `openspec validate original-title-choice --strict`; the changelog lines and `bun run site:changelog`. Verify: all pass.

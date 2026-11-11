## 1. Catalogue

- [x] 1.1 `cv-artwork` `naming/catalog.rs`: the choices of every movie and TV type (common or not), their labels and example files built with the naming functions, and the lists of every style; the test that writes and checks `ui/src/features/settings/artworkNameCatalog.ts`. Verify: `cargo test -p cv-artwork` (every listed choice is valid, the examples match the writer, every style lists every type with offered choices, a style written out as per-type lists reads back unchanged, the page catalogue is current).
- [x] 1.2 Name resolution from per-type settings in a movie folder and a multi-movie folder. Verify: `per_type_settings_name_the_files_of_a_movie_folder_and_keep_a_shared_folder_apart`.

## 2. Rename

- [x] 2.1 `cv-rename` `plan.rs`: `keep_names_already_held`, called by the movie and TV planners. Verify: `artwork_already_under_every_name_stays_and_is_not_copied_again`, `a_missing_name_of_the_type_is_still_copied`, `cargo test -p cv-rename`, `cargo test -p cv-compat` unchanged.
- [x] 2.2 `cv-app` organize: list the copies an apply made as files of their title; drop files gone after an undo. Verify: `new_artwork_names_rename_the_artwork_a_tidy_title_already_has` (preview, apply, tidy afterwards, an unused name set aside in the trash folder, Undo) and `new_artwork_names_make_every_title_checked_again`.

## 3. Settings page

- [x] 3.1 `artworkNames.ts`: style and custom lists from the stored values, the Custom fill, style changes, toggling with the last name kept, visible choices and examples; `settingsStore.setMany`. Verify: the model tests in `artworkNames.test.tsx`.
- [x] 3.2 `ArtworkNamesRow.tsx` in the Naming section: the style control with its example, the Custom table, the notes, the Rename line and the Undo toast; schema entry moved to Naming with every style as an option; messages. Verify: the component tests in `artworkNames.test.tsx`.

## 4. Integration

- [x] 4.1 `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test` for `cv-artwork`, `cv-rename`, `cv-app` and `cv-compat`; `bun run lint`, `typecheck`, `test`, `format:check` and `bun run shots -- --routes /settings` in `ui/`; the changelog line and `bun run site:changelog`; `openspec validate artwork-names-per-type --strict`. Verify: all pass.

## 1. Release tokens

- [x] 1.1 `cv-template` `tokens::release`: read the group, the release tags and the quality from an original file name, with unit tests for every scenario of "Release tokens" and the edge cases in design.md. Verified by `cargo test -p cv-template`.
- [x] 1.2 Expose `releaseGroup`, `releaseQuality` and `releaseTags` in the movie and episode models and aliases, so validation accepts them and they never count as stream tokens. Verified by `cargo test -p cv-template` and `cargo test -p cv-rename`.

## 2. Release info preset

- [x] 2.1 `cv-rename` `Preset::ReleaseInfo` (`release-info`, "Release info") with its movie and TV patterns, listed after Media-server safe. Verified by `cargo test -p cv-rename`.
- [x] 2.2 Compatibility fixtures for the tokens and the preset (movie and episode, with and without release information, after an earlier rename); existing expected files unchanged. Verified by `cargo test -p cv-compat`.

## 3. Metadata-only mode, backend

- [x] 3.1 `cv-app`: the `renamer.metadata-only` setting (`rename_settings::metadata_only`, the message) and the automation rename step, which renames nothing while it is on. Verified by `cargo test -p cv-app --test automation_batch` (step refused with the message, nothing moved, renames again once off).
- [x] 3.2 `cv-ipc`: `organize_apply` and `apply_change_source` refuse with `metadata_only` before anything is planned; the desktop and browser wrappers pass the settings. Verified by `cargo test -p cv-ipc metadata_only` and `cargo test -p cinevault-app` (bindings current).

## 4. UI

- [x] 4.1 Settings › Naming: the Metadata only switch at the top and the {Quality}, {ReleaseTags} and {ReleaseGroup} buttons; the Release info style in Settings and the first run, with its description. Verified by `bun run test` (namingModel, settings sections, onboarding).
- [x] 4.2 Metadata only hides the sidebar's Rename, the Rename and "Move to…" actions of the library, title, copies and Drives menus, and the Rename page shows the notice with Open Settings. Verified by `bun run test` (shell, library actions, title actions, Rename page).
- [x] 4.3 `CHANGELOG.md` under 1.6.0 and `bun run site:changelog`. Verified by `bun run site:changelog -- --check`.

## 5. Integration checks

- [x] 5.1 `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `bun run lint`, `bun run typecheck`, `bun run test`, `bun run format:check`, `openspec validate release-info-and-metadata-only --strict`.
- [x] 5.2 `bun run shots -- --routes /settings,/rename` in both themes shows the switch, the new token buttons and the notice without console errors or overflow.

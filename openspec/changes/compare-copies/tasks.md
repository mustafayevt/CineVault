## 1. Copy groups in the library database

- [x] 1.1 Migration after `104` adding `copies_kept (set_id, item_id, kept_ms)` with the cascade on `item`. Verified by `cargo test -p cv-db` (the migration tests: a fresh library and an upgraded one both have the table, and removing an item removes its rows).
- [x] 1.2 `cv_db::repo::copies::copy_groups`: online, not deleted movies with a video, linked by a shared TMDB or IMDb ID, or by title and known year when one has neither ID, and only within the same edition; transitive. Verified by `cargo test -p cv-db --lib copies` (shared ID, unscraped title and year, different IDs, unknown year, Extended vs Theatrical, edition case, offline, missing, deleted, shows ignored, a chain of three) and a test over 5,000 generated movies that stays one query.
- [x] 1.3 `keep_set`, `remove_kept_set` and the kept test (all current members in one kept set). Verified by `cargo test -p cv-db --lib copies` (kept group closed, a new member reopens it, a member leaving keeps it kept, ask again reopens it).
- [x] 1.4 Update the comment in `library_review::duplicate_groups` to say that rows which both have their files are copies (`movie-copies`), and check the review is unchanged. Verified by `cargo test -p cv-db --lib library_review`.

## 2. Copies service and best quality

- [x] 2.1 The best-quality rank as a pure function over resolution label, best HDR format, lead audio track (channels, then codec rank) and video bitrate, with no mark on a tie or on an unread top copy. Verified by `cargo test -p cv-app --lib copies::rank` (the spec's scenarios and each step of the order).
- [x] 2.2 `CopiesService`: `counts` (added to `LifecycleCounts` as `copies`), `group_of`, `compare` (the rows of "What the compare rows show", "Not read yet" when media info is missing, the opened copy first), `keep_all` and `ask_again`. Verified by `cargo test -p cv-app --lib copies` and `cargo test -p cv-app --lib lifecycle`.
- [x] 2.3 The Library list query takes `copies = true` (open groups only, copies together, best first, groups in the chosen sort) and its rows carry the open group size. Verified by `cargo test -p cv-db --lib` list tests for the filter, its combination with Drive and search, and the order.

## 3. Copy labels in Rename

- [x] 3.1 `cv_rename::conflict::copies::label_copies`: find clashing members of one copy group (same folder or video destination per library folder root), choose the labels (resolution, then HDR, source, lead audio; "copy n" by id for identical copies, the lowest id plain), rewrite the folder and base name without sharing a folder, then `detect` as before. Verified by `cargo test -p cv-rename --lib conflict::copies` (2160p vs 1080p, DV vs HDR10, BluRay vs WEB-DL, identical, three copies with one repeated label, copies in different roots unlabelled, copies sharing one folder labelled on the video only, a different movie still raises the duplicate conflict, the same input twice gives the same names).
- [ ] 3.2 Organize service: pass the copy groups and each member's label attributes from stored media info, render out-of-batch members as fixed participants, hold a clashing group back while a member's media info is unread, and add `plan_plain(id)`. Verified by `cargo test -p cv-app --lib organize` (one copy renamed alone next to its plain-named twin, a labelled copy planned as tidy, a group that lost a member offers the plain name, Undo of a labelled rename restores both copies).
- [ ] 3.3 New compat family `rename-copies` (`tests/compat/fixtures/rename/copies.json`, `expected/rename/copies.json`, a handler in `crates/cv-compat/src/handlers.rs`, a row in `tests/compat/README.md`) with the six spec cases. Verified by `cargo test -p cv-compat` with every existing golden file unchanged (`git diff --stat tests/compat/expected` shows only the new file).

## 4. Commands

- [ ] 4.1 Typed `tauri-specta` commands for `copies_of`, `compare_copies`, `keep_all_copies`, `ask_again_copies` and `copy_label_to_drop` (the label a kept copy could drop, from `plan_plain`), the same routes in `cv-web`, and `ui/src/bindings.ts` regenerated. Verified by `cargo test -p cinevault-app` (`generated_bindings_are_up_to_date`) and the `cv-web` route tests.

## 5. Library and Movie page

- [ ] 5.1 The "Copies <n>" chip after Problems for movies, shown while the count is above zero or it is on, wired to the list filter, with its English message. Verified by `bun run test` (chip shown and hidden, TV shows without it, the list filtered, "No titles match" after the last group is resolved) and `bun run lint` and `bun run typecheck`.
- [ ] 5.2 The "<n> copies" poster badge for open groups. Verified by `bun run test` (badge on both copies, gone after Keep both) and `bun run shots -- --routes /library`.
- [ ] 5.3 The Movie page line "You have <n> copies · Compare". Verified by `bun run test` (line shown for kept and open groups, absent without copies) and `bun run check:facts`.
- [ ] 5.4 "Compare <n> copies" in the title right-click menu with the title actions, single movie with copies only. Verified by `bun run test` (`titleActions.test.tsx`: shown for a movie with two copies, absent for several titles and for a movie without copies, opens the page).

## 6. Compare copies page

- [ ] 6.1 Route `/movies/$movieId/copies` with the cards, the rows, the "Best quality" tag (a suggestion; every card keeps Keep this one), the "These two copies look the same." note, Esc and Back, and "This movie has no other copies". Built after the owner approves the mock-up. Verified by `bun run test` (two and three copies, no copies left, Esc returns) and `bun run shots -- --routes /movies` with the page open in both themes and both sizes.
- [ ] 6.2 "Keep this one" through `requestDelete` with the other copies, opening the kept copy's Movie page only after a delete that went through. Verified by `bun run test` (Recycle Bin with Undo toast, Ask before deleting off, Delete permanently opens its confirmation, cancel deletes nothing, "Keep this one" on the card that is not marked) and `cargo test -p cv-app --lib library_ops` with a test where two copies share one folder and only the other copy's files go.
- [ ] 6.3 "Keep both" / "Keep all" with an Undo toast, and "Ask again" on a kept group. Verified by `bun run test` (count and badges drop, Undo and Ask again bring them back).
- [ ] 6.4 After Keep this one, the toast's "Rename to drop the '- <label>' label?" when the kept copy carries a label that no longer clashes, opening the Rename preview for that movie. Verified by `bun run test` (offered for "- copy 2" and "- 2160p", absent for a plain name, opens the preview with the plain name, renames nothing by itself).
- [ ] 6.5 `CHANGELOG.md` line under the unreleased version and `bun run site:changelog`; README and landing page wording; screenshots in `.github/assets/` and `site/img/` (dark and light) showing the chip, the Compare copies page and a labelled Rename preview. Verified by `bun run site:changelog -- --check` and the refreshed images.

## 7. Integration

- [ ] 7.1 Full checks: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test -p cv-compat`, and in `ui/` `bun run lint`, `bun run typecheck`, `bun run test`, `bun run format:check`, `bun run build`; `openspec validate compare-copies --strict` and `openspec validate --specs --strict`. Verified by those commands and CI.
- [ ] 7.2 Data-safety review of Keep this one and labelled renames: interrupted delete, interrupted labelled rename with Resume and Roll back, Undo after a partial send, copies in one shared folder, a copy on an offline drive; each finding covered by a test. Verified by the review notes and the added tests.
- [ ] 7.3 Human check in the desktop app with a library holding two real copies of one film on two drives: chip, badge, page, Keep this one with Undo, Keep both and a later third copy reopening the group, Rename of both copies with their labels and Undo, and the drop-the-label offer after Keep this one. Needs the owner's machine. Verified by the owner.

## Workflow follow-up

- Archive the change with `openspec archive compare-copies` after it is merged.

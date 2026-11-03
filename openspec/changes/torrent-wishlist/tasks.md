## 1. Backend

- [x] 1.1 `cv-db`: migration 107 (`torrent_wishlist`, `torrent_wishlist_match`) and the `torrent_wishlist` repository (add or replace, list, update criteria, remove, record a check, store matches keeping dismissed ones out, mark seen, sent and dismissed, mark done, count new). Verify: repository unit tests, the migration tests.
- [x] 1.2 `cv-app` `torrents::wishlist`: criteria on a release and on a library copy, the done rule (movie, season, episode), the next episode, the check (due only or every entry, one at a time, the collecting sink), the list with done judged. Verify: unit tests for the criteria, defaults, next episode and done rule; a service test with fake sources for a check, dismissed releases and the 6-hour rule.
- [x] 1.3 `cv-ipc`, `src-tauri`, `cv-web`: the `torrent_wishlist`, `torrent_wishlist_add`, `torrent_wishlist_update`, `torrent_wishlist_remove`, `torrent_wishlist_check`, `torrent_wishlist_match_state`, `torrent_wishlist_seen` and `torrent_wishlist_new_count` commands with their DTOs; regenerate `ui/src/bindings.ts`. Verify: `cargo test -p cv-ipc -p cv-web -p cinevault-app`, clippy, fmt.

## 2. UI

- [ ] 2.1 Wishlist model: criteria text, defaults from the copy held, the editor's state, the backend calls (desktop, browser and the synthetic backend). Verify: unit tests.
- [ ] 2.2 The Wishlist view at `/torrents/wishlist` with the Search · Wishlist switch, entries, matches (Send, Dismiss, New, Sent), Done, the empty state, Check now and the editor in the page. Verify: page tests.
- [ ] 2.3 Entry points: "Add to Wishlist" / "On your Wishlist" on the search page, the collection pane, the Movie and TV show ⋯ menus. Verify: tests.
- [ ] 2.4 The due check when the app opens and every hour, the toast with Show, the Torrents count in the sidebar and on the switch, marking seen. Verify: tests.

## 3. Integration

- [ ] 3.1 Rust checks; `bun run lint`, `typecheck`, `test`, `format:check` in `ui/`; `openspec validate torrent-wishlist --strict`; `bun run shots` for `/torrents/wishlist` in both themes and the compact sizes; changelog lines and `bun run site:changelog`.

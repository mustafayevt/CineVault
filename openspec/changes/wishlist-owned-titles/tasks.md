## 1. Backend

- [x] 1.1 `cv-db`: migration 108 adds `held_copies` to `torrent_wishlist`; `add` and `update` take it, `keep_looking` clears done and the last check, drops new and seen matches and sets it, `lower_held` lowers it. Verify: repository unit tests, the migration tests.
- [x] 1.2 `cv-app` `torrents::wishlist`: `min_size` in the criteria; the held copy and the count of copies meeting the criteria per scope; the done rule against the count saved; `held_copies` written on add and update; `wishlist_keep_looking` and `wishlist_held`. Verify: unit tests for the size range; service tests for a title already held staying active, a second copy closing it, a copy leaving lowering the count, and Keep looking.
- [x] 1.3 `cv-ipc`, `src-tauri`, `cv-web`: `minSize` (refused when negative or above `maxSize`) and `held` in the DTOs; the `torrent_wishlist_held` and `torrent_wishlist_keep_looking` commands; regenerate `ui/src/bindings.ts`. Verify: `cargo test -p cv-ipc -p cv-web -p cinevault-app`, clippy, fmt.

## 2. UI

- [x] 2.1 Wishlist model: the smallest size in the draft, its checks and the criteria text ("20–60 GB", "from 20 GB"), the held copy's words; the backend calls (desktop, browser and the synthetic backend). Verify: unit tests.
- [x] 2.2 The editor's smallest size and "You already have this" line, the card's held line and Keep looking on a done entry. Verify: page tests.

## 3. Integration

- [ ] 3.1 Rust checks; `bun run lint`, `typecheck`, `test`, `format:check` in `ui/`; `openspec validate wishlist-owned-titles --strict`; `bun run shots` for `/torrents/wishlist`; the changelog lines and `bun run site:changelog`.

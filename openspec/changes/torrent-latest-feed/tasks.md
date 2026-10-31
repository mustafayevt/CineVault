## 1. Sources

- [x] 1.1 `cv_torrent::latest`: feed kinds, `LatestAnswer`, `FeedSession` (one merge, a cursor per source, the page limit, "no new row ends the source", a drop guard for cancelled reads), `assess_feed` and `feed_kind_of`; `TorrentSource::latest_support` and `latest` with "not offered" defaults. Verified by `cargo test -p cv-torrent --lib latest`.
- [x] 1.2 The `latest` of YTS, EZTV, The Pirate Bay, Knaben (v1 POST, `SourceHttp::post_json_raw`), Nyaa and Torznab (indexers through it), with the empty-query refusal read as "unsupported". Verified by `cargo test -p cv-torrent --test latest` over the fixtures in `tests/fixtures/latest/`.

## 2. Library and app

- [x] 2.1 `cv_db::repo::torrent_landing::library_titles`. Verified by `cargo test -p cv-db --lib torrent_landing`.
- [x] 2.2 `cv_app::torrents::feed`: `LibraryIndex` (title, original title, kind, year) and `TorrentService::feed_sources`; `enabled_sources`. Verified by `cargo test -p cv-app --lib torrents::feed`.

## 3. IPC

- [x] 3.1 `cv_ipc::torrent_feed`: `torrent_latest` (the session per kind, the five-minute first page, `more`, `refresh`, `feed_expired`, the `sourceState` item) and `torrent_feed_lookup` (IMDb ID first, kept answers, `weak`); the browser server's command list and dispatch; the two Tauri commands and `ui/src/bindings.ts`. Verified by `cargo test -p cv-ipc --lib torrent_feed`, `cargo test -p cinevault-app generated_bindings` and `cargo test -p cv-ipc` (command list).

## 4. Page

- [x] 4.1 The release table's actions column (sized, not resizable, "Send" with its full accessible name; Details in the list) and the Quality sort. Verified by `ui/src/features/torrents/latestFeed.test.tsx` ("the release table's actions") and `torrentsPage.test.tsx`.
- [x] 4.2 `useLatestFeed`, `LatestFeed`, `FeedPanel`, the synthetic feed, and the Torrents page wiring (the list before a title, the count, the responsibility line). Verified by `latestFeed.test.tsx` (streaming rows, tabs, sort, the CAM filter, paging, the panel and its lookup once, weak match, no key, Esc, no accidental hand-off) and the page tests.

## 5. Integration

- [x] 5.1 `CHANGELOG.md` under `## [Unreleased]` and `bun run site:changelog`; `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `bun run typecheck`, `bun run lint`, `bun run test`, `bun run format:check`, `openspec validate torrent-latest-feed --strict` and `openspec validate --specs`; the desktop app in development with a throwaway data folder against the live sources. Verified by those commands, the run and CI.

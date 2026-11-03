## Why

Torrent search only answers when the user asks. A title that has no good release yet, a better copy of a movie the library holds, or the next episode of a show means coming back and searching again and again (#48).

## What Changes

- A **Wishlist** on the Torrents page, beside the search (`/torrents/wishlist`, "Search · Wishlist" switch at the top of both views).
- **Entries:** a movie, or a show's season, single episode or "next new episode". Each has criteria: the lowest resolution (Any, 720p, 1080p, 2160p), the accepted sources (Remux, Blu-ray, WEB-DL, WEBRip; none chosen takes any), HDR only, a largest size and the fewest seeders.
- **Adding:** "Add to Wishlist" on a title picked in the torrent search, on a collection film the library does not hold, and in the ⋯ menu of the Movie page (to look for a better copy) and the TV show page. Each opens the Wishlist with the entry's editor filled with sensible defaults (one step above the copy the library holds, for a movie it holds).
- **Checking:** when the app opens and while it stays open, each entry is checked at most once every 6 hours; **Check now** checks every entry at once. A check runs every switched-on source for the entry and keeps the releases that are the title and scope asked for, carry no warning (cam, screener, no seeders, payload, too small) and meet the criteria.
- **Telling:** new matches show as a count on the sidebar's Torrents item and as a toast with "Show". Each entry lists its matches with **Send to torrent app** and **Dismiss**; a dismissed release never comes back for that entry.
- **Done:** when the library holds a copy that meets the criteria (a movie, every aired episode of the season, the episode), the entry is marked done and is no longer checked. "Next new episode" never ends: it follows the show, looking for the first aired episode after the newest one the library holds.
- Nothing is ever sent by itself; the user chooses every release.

## Capabilities

### New Capabilities

- `torrent-wishlist`: the Wishlist view, its entries and criteria, the checks, the matches and their actions, the done rule and the entry points.

### Modified Capabilities

- `title-pages`: "Title page actions" gains "Add to Wishlist" in the ⋯ menu of a title with a TMDB ID.
- `app-shell`: "Sidebar destinations and counts" gains the Torrents count of new Wishlist matches.

## Impact

- `cv-db`: migration 107 adds `torrent_wishlist` and `torrent_wishlist_match`; repository `torrent_wishlist`.
- `cv-app`: `torrents::wishlist` (criteria, the check, the done rule, the next episode).
- `cv-ipc`, `src-tauri`, `cv-web`: the `torrent_wishlist*` commands; regenerated `ui/src/bindings.ts`.
- UI: `features/torrents` (the Wishlist view, the editor, the auto check, the toast), the sidebar count, the collection pane, the title actions menu, i18n, the synthetic backend and tests.

## Out of scope

- Sending a release without the user choosing it, or any download by CineVault.
- System notifications outside the app window; checks while the app is closed.
- Criteria for languages, audio or release groups.
- A whole-show entry (every missing episode): "Find missing episodes" on the search page covers it.

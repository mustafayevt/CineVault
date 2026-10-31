## Why

The Torrents page is empty until a title is typed and picked. People who open it often want to see what is new: the releases that came out today on the sites they already use, the ones that are better than the copy they hold, a show's new episode. Today they have to guess a title first. The sources already know their newest releases; the page can list them.

## What Changes

1. **Latest releases on opening the page.** While no title is picked, the Torrents page lists the newest releases of every switched-on source that can list them, in the same release table as the search. Rows stream in as each source answers. Typing and picking a title switches to the search as today; clearing the field brings the list back.
2. **Controls.** All / Movies / TV; a sort (Newest by default, Seeders, Size, Quality); the resolution chips of the search; "Hide CAM and TS" (on by default) for camera, telesync, telecine and screener copies; Refresh. The first page of each kind is kept for five minutes so switching tabs or leaving the page asks nobody again.
3. **More near the end.** Scrolling near the end reads the next page of every source that has one; a source that has no more says so quietly ("No more from YTS"). A release that comes back on a later page or from another source stays one row.
4. **Library marks without TMDB.** Rows of a title the library holds say "In your library", and "Better than your copy" when the release beats the movie's copy, matched by title and year from the library alone.
5. **Details panel.** One click on a row (or its Details button, Enter or a double-click) opens a side panel that slides in from the right; the table hides the columns the panel repeats until the rest fits. Only then is the title looked up on TMDB, once per title: poster, title, year, plot, TMDB rating, and links to TMDB, IMDb, Letterboxd (movies) and Trakt that open in the browser. A weak or missing match says so and offers "Pick another"; without a TMDB key the panel shows what the release name says. The panel lists the title's other releases in the list, keeps "Send to torrent app", and offers "Search all releases" for the matched title.
6. **The Send button never clips.** The release table's last column is now an actions column sized to fit its buttons ("Send", and in the list "Details" too) at every window width; the button keeps "Send to torrent app" as its accessible name and tooltip.
7. **The responsibility notice stays visible** as a quiet line at the foot of the page.

Sources: YTS (newest movies, paged), EZTV (newest TV, paged), The Pirate Bay (the newest of each video category, one page), Knaben (newest movies and TV, paged, adult and unsafe hidden), Nyaa (newest English-translated anime, one page, with TV), each indexer of the managed service and the user's own Torznab instance (`t=search` without words in the movie and TV categories, paged with `offset`; one that refuses a search without words is left out with a note). Torrentio searches one title at a time and takes no part.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `torrent-search`: the Latest releases list, its details panel, the result state before a title is picked, the actions column and the visible responsibility line.
- `torrent-sources`: each source's newest releases, page by page.

## Out of scope

- A poster or card grid for the list; it is the release table.
- Notifications or a background watch for new releases.
- Filtering the list by language, genre or source.
- Remembering a "picked another" title across restarts.
- Downloading anything: the hand-off stays the only action.

## Impact

- `crates/cv-torrent`: `latest` module (feed kinds, the page answer, the feed session with a cursor per source, the feed assessment), `TorrentSource::latest_support` and `latest` with default "not offered", and the `latest` of YTS, EZTV, The Pirate Bay, Knaben, Nyaa and Torznab (indexers through it); `SourceHttp::post_json_raw` for Knaben's v1 interface.
- `crates/cv-db`: `torrent_landing::library_titles`.
- `crates/cv-app`: `torrents/feed.rs` (the feed's sources and the library index); `TorrentService::enabled_sources`.
- `crates/cv-ipc`: `torrent_feed` (`torrent_latest`, `torrent_feed_lookup`), the command list of the browser server; `crates/cv-web` dispatch; `src-tauri` two thin commands; `ui/src/bindings.ts`.
- `ui`: `LatestFeed`, `FeedPanel`, `useLatestFeed`, `feedView`, the synthetic feed, `ReleaseTable` (actions column, Details), `releaseSort` (Quality, the feed's own sort), `TorrentsPage`.
- `CHANGELOG.md` and `site/changelog.html`.

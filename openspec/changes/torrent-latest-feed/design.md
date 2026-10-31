## Context

The Torrents page searches one confirmed title at a time: `torrent_release_search` builds a query from TMDB and runs every source that applies as one provider of a streamed search that shares one merge. Nothing on the page works before a title is picked. Most of the sources the page already uses can also list their newest releases without a title, but none of that is exposed.

## Goals / Non-Goals

**Goals:** a feed of the newest releases in the existing table, streaming like the search, paged on scroll, with library marks that need no network, and a details panel that asks TMDB only for the title the person opens. Desktop and browser mode behave the same.

**Non-Goals:** a poster grid, background watching, per-source or language filters.

## Decisions

### What each source offers (checked against the live sites on 2026-10-31)

| Source | Newest releases | Paging | Notes |
|---|---|---|---|
| YTS | `list_movies.json?sort_by=date_added&order_by=desc&limit=50&page=N` | `page` while `movie_count` says more | Movies only; each movie's torrents are rows, with the IMDb ID. |
| EZTV | `get-torrents?limit=100&page=N` without `imdb_id` | `page` while `torrents_count` says more | TV only; an `imdb_id` of `0` is no ID. |
| The Pirate Bay | `q.php?q=category:<id>&page=0` for 201/207/211 (movies) and 205/208/212 (TV) | none: every page number gives the same 50 | `page=0` is needed: without it the site sometimes answers "No results". No adult category is asked. |
| Knaben | `POST /v1` with `order_by: date`, categories 3000000 / 2000000, `hide_xxx`, `hide_unsafe` | `from`/`size` (100) | The v2 GET interface refuses an empty query, so the v1 POST is used for the list only. |
| Nyaa | `?page=rss&c=1_2&f=0` | none (the RSS has one page of 75) | Listed with TV (most rows are episodes), not with Movies. The site answered 504 on the day of the check; the shape is the one the search already reads. |
| Torrentio | none | none | Per title (by IMDb ID) only; takes no part. |
| Indexers and own Torznab | `t=search` without `q`, `cat=2000` / `5000`, `limit=100&offset=N` | `offset` | Only the movie and TV categories are asked, so adult (6000) stays out as in provisioning. A feed that refuses (HTTP 400, error 200–299, or a 900 whose text is about the query) is "can't list new releases", not a failure. |

The trait gets `latest_support(kind)` and `latest(kind, page, context)` with defaults that say "not offered", so a source that does nothing new keeps compiling and stays out.

### One session per kind, a cursor per source

`cv_torrent::latest::FeedSession` holds one `MergeState` for every page and source of a kind, so a release seen again (on a later page, or from another source) is one row with every source named; the merge, its keys and its tie rules are the search's own. Each source has a cursor (next page, and Open, Loading, Exhausted or Unsupported). A page that adds no new release ends the source: a site that ignores the page number would otherwise be asked for the same rows forever. A source reads at most 10 pages. A failed page leaves the cursor where it was, so the next "more" asks for it again. A cancelled read (the page is left, the tab is switched) puts the cursor back to Open through a drop guard.

The per-source deadline, the per-request timeout, the answer limit and the one-request-per-second lane per site are the search's, unchanged: the feed runs through the same `SourceHttp` and the same provider lanes. Each source is one provider of the generic engine, so `cancel_search` drops its requests.

### The IPC shape

`torrent_latest(request, onEvent)` with `{kind, more, refresh, generation}`. The start reply carries every source's state and, for a first page, the rows the session already holds (the five-minute cache: a first page within five minutes of the last refresh asks only the sources not read yet). Rows stream as `ProviderEvent<TorrentFeedItemDto>`, where an item is a row or, last in a source's answer, `sourceState` (`more`, `done`, `unsupported`). This keeps the generic engine untouched and still tells the page "No more from YTS" as soon as YTS says so. `more` carries the generation; a feed refreshed meanwhile (another browser window on the same server) answers `feed_expired` and the page starts over.

The sessions live in `TorrentState` (shared by the desktop app and the browser server through `cv-ipc`), so both transports run the same code; the browser server dispatches the two new commands like `torrent_release_search`.

### Rows

A feed row is the search's `TorrentReleaseDto` (so the table, sort and hand-off code are the same) plus what the name says: kind, title, year, season, episode, the source's IMDb ID, a title key (kind, match key, and the year for a movie) and the library title. Kind comes from the parsed name (an episode or pack is a show), then the source's category (Torznab 5xxx, The Pirate Bay 205/208/212, Knaben "TV / ..."), else movie. The relevance check is skipped (there is no title); the warnings (payload, cam/telesync/telecine/screener, no seeders) are the search's.

### Library marks without TMDB

`library_titles` reads every movie and show once per feed request (one query; the largest video's frame size, HDR list and the item's source). The index keys them by the same match key the relevance check uses, for the title and the original title. A movie matches with the same year (one either way); a show matches any episode. "Better than your copy" is `rank::upgrade` against the movie's copy. No provider is asked.

### Details on demand

`torrent_feed_lookup` runs the title search the page already has (`TorrentService::title_hits`): the IMDb ID when a source gave one (an exact hit, confidence 1), else `Title (Year)` for a movie or the title for a show. The best hit comes back with `weak` when its confidence is below 0.75. Answers are kept in `FeedState` (500 at most), and the page keeps them in its query cache, so a title is looked up once. The panel then reads `torrent_title_details` (poster, plot, rating, IMDb ID) like the search header does. "Pick another" is the page's own title search (`useTitleMatch`) inside the panel; the pick is kept for the session.

Links: TMDB `/movie/<id>` or `/tv/<id>`; IMDb `/title/<tt>/`; Letterboxd `https://letterboxd.com/tmdb/<id>/` for a movie (Letterboxd's documented TMDB redirect; it refuses scripted requests behind a browser check, so it was checked by its documentation, not by a request); Trakt `https://trakt.tv/search/tmdb/<id>?id_type=movie|show`, which redirects to the title (checked: 301 to `app.trakt.tv/search/tmdb/...`). They open through `openRatingPage`, the app's existing opener: the desktop opener command, or a new tab in a browser.

### One click opens the details (UX choice, changed after the user tried it)

Revised on the user's request: a plain click selects the row and opens the details panel, so browsing takes one click; Ctrl/Shift-clicks still only select, and Send stays its own button, so a click never hands anything off. The text below is the original reasoning, kept for history: a row click selects, like every table in CineVault; it does not open anything. Each feed row has an actions group: a Details icon button (Info, with its tooltip) and Send. Enter and a double-click open Details in the list, never Send, so a hand-off is always its own deliberate click. This keeps the table keyboard-first (arrows to move, Enter for details), avoids an accidental hand-off on a double-click in a list the person is only browsing, and keeps the panel open as the selection moves (it follows the selected row). In the title search, Enter and double-click still send, as specified; there the title is known and the hand-off is what the person came for.

### The clipped Send button

The old last column was 120 px with the label "Send to torrent app", which the 12 + 24 px cell padding cut to "Send to t…". The column is now "actions", not resizable, sized from the button sizes (`SEND_COLUMN_WIDTH` 116 px for Send alone, `ACTIONS_COLUMN_WIDTH` 148 px with Details): a 28 px icon button, a 4 px gap and the compact "Send" button (24 px padding, 16 px icon, 6 px gap, the word). The button keeps "Send to torrent app" as its accessible name and tooltip. Its id changed, so a width saved for the old column does not apply.

### Responsibility notice

The first-visit notice still gates the page. After it, its sentence stays as a quiet line at the foot of the page.

## Risks / Trade-offs

- A site that changes its listing shape breaks only its feed, and says so as a failed source; the search is not touched.
- Nyaa's RSS is one page; anime fills the TV tab only as far as that page goes.
- The Pirate Bay asks six category listings for All (one per second on its lane); its rows arrive over a few seconds, as the search's text queries already do.
- An indexer that ignores `offset` repeats its first page; the "no new row ends the source" rule stops it after one extra request.

## Migration

None: no setting, no stored data. The release table's saved column widths keep working; the new actions column starts at its own width.

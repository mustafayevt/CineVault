## Why

When a searched title is already in the library, the Torrents page only says so in a thin "Yours: …" line and a Same / Better / Worse tag. It is easy to miss, it does not say why a release is better, it names one copy even when the library holds several, and for a show it does not know which episodes are held or missing (#47).

## What Changes

- A **"You have this" card** replaces the "Yours:" line when the library holds the searched title: poster, title and year linking to the Movie or Show page, and what the library holds. For a movie, one line per copy (resolution, HDR, video codec, audio, source, edition, size). For a show, the seasons held and the episodes missing ("You have S01–S03 · missing S04E05–E08"). Actions: **Open in library**, **Look for a better copy** (movies) or **Find missing episodes** (shows), which switch on the chip below, and **Search anyway**, which folds the card into one line.
- **"Compare with yours"**: the Same / Better / Worse tag opens a popover with the copy and the release in two columns, row by row (resolution, HDR, codec, audio, source, edition, size), the differences marked, and a plain verdict ("Better picture, same audio, 2.3× the size"). The same block shows in the Latest releases details panel when the release's title is a library movie. The tag's tooltip says why ("Better: 2160p Dolby Vision vs your 1080p SDR"). Tag, tooltip and block read one comparison: the backend's verdict against the library's best copy.
- An **"Only better than mine"** chip after the resolution chips while a copy is held, hiding releases that are the same or worse.
- For a show, each release that names its episodes gets **Fills a gap** (has an episode the library is missing), **In your library** (every episode in it is held) or **Better** (every episode held, better quality), and a **Fills a gap** chip filters to the first.
- **Entry points:** the Movie page's ⋯ menu gets **Find a better copy** and the Show page's **Find missing episodes**; each opens Torrents with the title picked and the matching chip on.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `torrent-search`: "Same, better or worse than yours" (the card, the compare popover, the reason, the best of several copies), "Resolution chips with counts" (the two library chips), "Release details actions" (the compare block in the details panel); new requirements for the show episode tags and the chip in the address.
- `title-pages`: "Title page actions" gains Find a better copy (movie) and Find missing episodes (show) in the ⋯ menu.

## Impact

- `cv-app` torrents library (every copy of a movie with codec, audio, edition and languages, best first; the compared copy is the best one; a show's missing episodes), `cv-ipc` torrent DTOs (`libraryCopies`, `missingEpisodes`, each release's `episodes`), the regenerated `ui/src/bindings.ts`.
- UI: `features/torrents` (card, compare, chips, tags, `?only=` in the address), the title actions menu, i18n, the synthetic backend and tests.
- No provider lookups are added: what the library holds comes from the library alone. The hand-off is unchanged; CineVault downloads nothing.

## Out of scope

- A "tell me when a better copy appears" watch (the Wishlist, #48).
- Comparing subtitles or bit rates.

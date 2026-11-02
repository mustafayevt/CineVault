## Why

A movie in the library twice is two titles. Its Movie page shows only the copy that was opened, with a thin "You have 2 copies · Compare" line under the facts. To look at the other copy's file, NFO or rename history, to play it, or to delete just that copy, the user has to go back to the Library or through the Compare copies page, which is easy to miss and is all or nothing (issue #50).

## What Changes

- The Movie page of a movie with copies, kept or not, shows a row of copy tabs at the top of the Technical card, one tab per copy, labelled by what tells the copies apart ("2160p · HDR10 · BluRay · 51.2 GB"), with the best-quality copy marked as on the Compare page.
- Choosing a tab opens that copy's Movie page in place of the current one, so everything that belongs to a copy follows: the Technical card, File, NFO, Renamed and Undo, and the header's Play, Open folder, Move, Rename and Delete. What the copies share (title, plot, cast, ratings, artwork) reads the same.
- The card ends with a Copies row: **Keep only this one** (the Compare page's Keep this one for the shown copy), **Delete this copy** (the usual Delete with its confirmation, then the next copy's page opens) and **Compare** (the Compare copies page, unchanged).
- The "You have <n> copies · Compare" line under the facts is removed; the tabs take its place.

Out of scope: one poster per film in the Library (each copy keeps its poster and badge), Scrape and Change artwork for all copies at once, and showing other editions on the page (different editions stay apart, as today).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `movie-copies`: "Copies line on the Movie page" is replaced by "Copy tabs on the Movie page", "Switching copies" and "Copy actions on the Movie page".

## Impact

- UI only: `ui/src/features/copies` (a `CopyTabs` component and its tests; the copies line removed), `ui/src/features/movie/TechnicalCard.tsx` and `MoviePage.tsx`, the copies messages.
- No backend, IPC or database change: the tabs read the existing Compare copies data, and the actions reuse Keep this one and Delete.
- `CHANGELOG.md` and `site/changelog.html`.

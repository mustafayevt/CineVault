## Why

Finding the movies and shows that still miss a poster, fan art, clear logo, plot or title means opening them one by one (#97). The catalogue already has "Missing artwork" and "Missing metadata" filters, but they are not offered anywhere in the Library, so nobody finds them.

## What Changes

- **Needs fixing chip.** The Library's filter row gets a "Needs fixing" dropdown chip after Problems, with checkable entries: Not scraped, No poster, No fan art, No clear logo, No plot and No title. Each entry shows how many titles of the current view it would list. Ticking entries keeps the titles missing all of them, and the chip shows how many are ticked.
- **Same filter engine.** The entries are the catalogue's own `missing_artwork` and `missing_metadata` filters (and, for Not scraped, the New stage), so they combine with the stage, Drive, Quality, Problems, the search and each other, for movies and for TV shows.
- **Title in Missing metadata.** The catalogue's "Missing metadata" filter gains "Title": a title that is empty. A title that was never scraped has the name read from its folder, so that case is covered by Not scraped.
- **TV shows.** A show counts by its own artwork and facts, never by its episodes' or seasons'.
- **Batches.** With an entry ticked, Select all and the selection bar's Rename, Scrape and Artwork act on every listed title.
- **Nothing to fix.** When no title is missing what was ticked, the list says "Nothing to fix here" with a button to clear the choices.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `library-browsing`: the Filter row lists the Needs fixing chip; new requirements *Needs fixing filter* and *Nothing to fix*.

## Impact

- `cv-db`: one more value ("Title") in the Missing metadata facet, with tests. No schema, IPC or stored-data change; no bindings change.
- UI: `LibraryFilterRow.tsx`, `TitlesView.tsx`, `libraryStore.ts`, the synthetic backend for the dev build, i18n messages, tests.

## Out of scope

- No new bulk picker: the Artwork page already takes several titles.
- No new filter system: nothing beyond the catalogue's filters is added to the backend.
- The Needs fixing choices are not remembered across restarts, like the other filters.
- Other artwork types (banner, disc art, thumb, key art) and other metadata (year, runtime, cast) stay in the catalogue for the advanced filters; the menu offers the ones the issue asked for.

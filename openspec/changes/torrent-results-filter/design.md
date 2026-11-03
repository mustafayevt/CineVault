## Context

The Torrents page lists a title's releases under a row of resolution chips and, when the library holds the title, the "Only better than mine" and "Fills a gap" chips. All filtering is done in the page over the releases already received (`TorrentsPage.tsx`).

## Decisions

- **Client-side, over the received releases.** The filter is a pure function over `TorrentRelease.name`; nothing is sent to the sources, so it is instant and works while sources are still answering.
- **Normalised words.** The name and each word are lower-cased and stripped to letters and digits before a substring test. That makes separators irrelevant (`WEB-DL`, `WEB.DL`, `WEBDL`) at the cost of rare matches across word boundaries, which a filter can afford.
- **Exclusion with `-`.** A word that starts with `-` (and has more after it) must not appear. A lone `-` is ignored.
- **Order of narrowing.** Text first, then chips: the chip counts are taken from the releases the text leaves, so a count always says what choosing that chip would show.
- **Per title.** The text is kept with the title it was typed for, like the library chips, so changing the title starts with an empty filter. It is not put in the page address.
- **Layout.** The field takes the right end of the chip row (`ml-auto`, a fixed width), and on a narrow pane wraps to its own full-width line. The row grows from 24px to the field height.

## Risks

- A very long list re-filters on each key press; the work is a few string tests per release, well under a frame for the few hundred releases a search returns.

## Safety

The change writes nothing to media folders or settings.

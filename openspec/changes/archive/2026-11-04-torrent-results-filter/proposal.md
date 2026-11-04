## Why

A popular title brings back hundreds of releases. The resolution and library chips narrow them by kind, but there is no way to find "the remux with Atmos" or to leave out x265 copies short of scrolling and sorting (#80).

## What Changes

- **Filter field.** Once a title's releases are listed, a "Filter results" field sits at the right end of the chip row. It narrows the listed releases by words in the release name, as typed, without searching again.
- **Words.** Every word must appear in the name, in any order and case; a word starting with `-` must not. Punctuation is ignored on both sides, so `web-dl` matches `WEB.DL` and `5.1` matches `DD5.1`.
- **With the chips.** The filter narrows on top of the chosen resolution and library chips, and the chip counts follow it.
- **Clearing.** Esc in the field or its × clears it; a new title starts with it empty. Nothing left by it says "No releases contain “<text>”." with "Clear filters", which clears the chips and the text.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `torrent-search`: new requirements *Filter the results by text* and *Results filter with the chips*.

## Impact

- UI only: `TorrentsPage.tsx`, a small matcher next to `releaseSort.ts`, i18n messages, page tests.
- No backend, IPC or stored-data change.

## Out of scope

- The Latest releases list keeps its own controls; the filter is for a title's results.
- No saved filters, no regular expressions, no filter on fields other than the release name.

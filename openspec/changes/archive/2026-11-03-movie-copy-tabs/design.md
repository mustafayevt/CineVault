## Context

Each copy is its own movie title with its own id, page (`/movies/<id>`), file, NFO, rename journal and actions. The Compare copies data (`compareCopies`) already gives, for every copy of a group, its resolution, HDR, lead audio, source, size, folder, the best-quality mark and whether two copies may be one file.

## Goals / Non-Goals

**Goals:** switch the Movie page between copies in one click; delete one copy or keep only one from where the user already is; keep the first screen of the Movie page as it is (`bun run check:facts`).

**Non-Goals:** merging copies into one library title, one poster per film, acting on all copies at once.

## Decisions

- **A tab is a link to the other copy's page.** Switching replaces the current history entry (`replace: true`), so Back still returns to where the movie was opened from, and every copy-specific part of the page switches by itself because it already belongs to that title. A page that holds one title's data and swaps parts of it for another title's was rejected: it would duplicate every per-title query and action for no gain.
- **The tabs sit on the title line of the Technical card,** the card that differs between copies, in place of its title, instead of a row of their own: at 1366×768 any extra row pushes the File row off the first screen. The old copies line goes, so the left column keeps its height.
- **Stable order:** the best-quality copy first, then the larger, then the lower library id. The Compare page's own order (opened copy first) would shuffle the tabs on every switch.
- **Label:** the resolution and first HDR format ("2160p DV"), then the source and the size only where two tabs would read the same, as the rename copy labels do; the full description is in the tooltip.
- **Actions reuse what exists.** Keep only this one is `keepThisOne` with the same check, confirmation, toast, History and Undo, and is left out when two copies may be one file, as on the Compare page. Delete this copy is `requestDelete` for the shown copy; when it is done, the first remaining copy's page opens.

## Risks / Trade-offs

- [Long labels would not fit beside "Show all streams and details"] → short labels that grow only where tabs clash; the tabs wrap when there are many; `bun run check:facts` passes on the copy fixtures.
- [A copy whose media info is not read yet has no resolution] → its tab reads "Not read yet", the same words as the Compare page.

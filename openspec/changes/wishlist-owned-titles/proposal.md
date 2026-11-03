## Why

Adding a title the library already holds to the Wishlist puts it straight under Done when the copy held meets the criteria, so it is never checked. That makes the Wishlist useless for finding a second or better release of something the user owns (#68). The limits row also has a largest size but no smallest, so a check cannot skip small re-encodes that pass the resolution filter.

## What Changes

- **Done means it arrived.** An entry is done only when the library gains a copy that meets its criteria **after** the entry was added or last edited. A copy that was already there does not close it; the entry stays active and is checked like any other.
- **You already have this.** The editor and the entry card of a title whose scope the library holds say "You already have this: 1080p · WEB-DL" (the copy's resolution, source and HDR), linking to the Movie or TV show page.
- **Keep looking.** A done entry offers "Keep looking", which makes it active again from that moment (the copy that closed it no longer counts). Editing a done entry does the same.
- **Smallest size.** The limits row gets a smallest size next to the largest, in the same GB or GiB unit, so the size works as a range. Empty means no limit, the smallest must not be above the largest, and a release whose size is unknown fails a set limit. The criteria line reads "20–60 GB", "from 20 GB" or "up to 60 GB".

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `torrent-wishlist`: *Wishlist entries and criteria* (smallest size, the held copy in the editor), *Wishlist view* (the held copy on the card), *What a check keeps* (the size range) and *Done when the library has it* (only a copy that arrives closes an entry; Keep looking).

## Impact

- `cv-db`: migration 108 adds `held_copies` to `torrent_wishlist`; the repository writes it on add and update and lowers it when copies leave.
- `cv-app` `torrents::wishlist`: `min_size` in `WishCriteria`, the held copy and the number of copies meeting the criteria, the done rule, Keep looking, the held copy for a title and scope.
- `cv-ipc`, `src-tauri`, `cv-web`: `minSize` and `held` in the DTOs, the `torrent_wishlist_held` and `torrent_wishlist_keep_looking` commands; regenerated `ui/src/bindings.ts`.
- UI: the editor (smallest size, held line), the entry card (held line, Keep looking), the criteria text, i18n, the synthetic backend and tests.

## Out of scope

- Telling two copies of the same quality apart: replacing a held file with another one of the same quality, without the number of meeting copies going up, does not close an entry.
- A per-entry "Stop when my library has it" option: Keep looking and Remove cover it.
- Size limits per episode for season packs; the size applies to the release as listed, as today.

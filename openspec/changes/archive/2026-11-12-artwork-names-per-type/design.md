## Context

`cv_artwork::naming` names every artwork type from a list of choices (`FILENAME_POSTER`, `POSTER`, `FOLDER`, ...). `metadata.artwork-names` picks the starting lists (a style), and `artwork.names.movie.<type>` / `artwork.names.tv.<type>` replace one list; the settings service refuses an empty list. The rename profile copies these lists (`rename_settings::apply_artwork_choices`), so Rename moves artwork to the names a scrape writes, and the organize-state cache key includes them. The NFO writers only write artwork addresses (URLs), never local file names, so they need no change.

## Goals / Non-Goals

**Goals:** choose the names per type in Settings › Naming with a live preview; keep the existing keys; make a type with several names settle after one rename.

**Non-Goals:** collection artwork names, trailer and NFO names, free-text names, renaming files when the setting changes.

## Decisions

### Custom is "any per-type key is stored"

No new stored value. The style key keeps the style the custom lists started from, so a type added in a later version falls back to that style. Starting Custom writes the style's list of every type (the backend reads them back as the same names, checked by a test), so the page is fully described by what is stored and nothing changes on disk. Choosing a style writes the style and removes every per-type key in one store change; Undo puts the previous values back.

### The catalogue is generated, not copied

The choices of each type, a label (`{name}-poster`), the example file for "Example Movie (2020)" and the lists of every style come from `cv_artwork::naming::catalog`, which builds the examples with the same functions that name the files (`MovieNames::targets`, `TvNames::show_targets`, ...). A test writes it as a typed TypeScript constant (`CINEVAULT_UPDATE_ARTWORK_NAMES=1 cargo test -p cv-artwork`) and fails when the file is stale, like the IPC bindings. An IPC command was the alternative; it would add a command to the desktop app, the server and the synthetic backend for data that only changes with a release.

Rare choices (`<name>.fanart`, `background`, ...) are marked not common: a row shows them only while they are in use, so stored lists still read correctly. TV types with a single common choice (banner, clear art, character art, key art, square art) get no row.

### Several names per type settle

Two fixes in the rename path, both data-safe:

1. **Copies are listed.** After an apply, for every title that finished, each extra destination of a carried file that is on disk becomes a `media_file` of the title that owns the moved file, with the same type, unless another title already lists it. After an undo the titles of the batch drop files that are gone, so the removed copies do not linger.
2. **Names already held stay.** The planner still sends the newest file of a type to every name. A file of the same type and owner that already sits at another of those names (at its place in the new folder) is kept there and the newest file is not copied over it; files at names the type no longer uses still go to the trash. The artwork writer and Rename always write every name of a type together, so a file at one of the names is the same image. Without this the title would plan the same copy and trash after every rename.

### Interruption and undo

Nothing new writes to media folders: renames still go through plan, preview and journal, so an interrupted rename is recovered by the existing journal and every rename is undoable. Old names go to the deleted-files destination as rename leftovers, never erased. Listing copies and dropping missing files are single database writes after the disk work; if one fails it is logged, and the next scan corrects the list.

## Risks / Trade-offs

- An older, different image that sits at a second name of a type is kept rather than replaced by the newest one. This only happens when the files were changed outside CineVault; the artwork picker writes every name.
- The generated catalogue is a large TypeScript file; it is excluded from Prettier and checked by a Rust test instead.

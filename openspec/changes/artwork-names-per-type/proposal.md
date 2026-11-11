## Why

People who run two media centres on one library need artwork under the names each of them reads: one wants `poster.jpg`, the other `Example Movie (2020)-poster.jpg` or `folder.jpg` (issue #98). The backend already names every artwork type from a list of choices and accepts per-type lists in the settings, but the app only offers a five-way preset in a part of the settings that no page shows, so nobody can choose per type, and nothing tells them what happens to the artwork they already have.

Two gaps also stop several names per type from settling. After a rename that copies a poster to a second name, the library does not list the copy, so the next preview calls it a stranger and reports a conflict; and once both names are listed, the planner keeps only the newest file of the type, trashes the other and copies over it again, so the title is never tidy.

## What Changes

- Settings › Naming gets "Artwork file names": the styles as the quick choice (Movie name in front, Plain names, Both names, Jellyfin; older stored styles are still shown by name) plus **Custom**. Custom shows a compact table with a row per artwork type, movies first, then the TV types that offer a choice (show, season, episode). Each row switches names on and off (`<name>-poster`, `poster`, `folder`, `cover`, ...; extra fanart picks one), never leaving a type without a name, and previews the files for "Example Movie (2020)".
- Starting Custom writes the lists of the style in use for every type, so nothing changes until a row is edited. Choosing a style again removes the custom lists, says so in one line under the control while Custom is on, and offers Undo in a toast.
- A line under the table says that a folder holding several movies always uses the names with the movie name in front.
- After a change, a line says Rename moves the existing artwork to the new names and the next Scrape writes them. The organize-state cache already follows the artwork names, so titles whose artwork has the old names read as pending.
- The choices and their example files come from a catalogue the backend generates from the code that names the files (`ui/src/features/settings/artworkNameCatalog.ts`, checked by a test), so the preview is what a scrape writes.
- Rename lists the copies it makes as files of their title, and a file of a type that already sits at another name of its type stays there instead of being trashed and copied over. Names a type no longer uses still go to the deleted-files destination.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `settings`: Naming section gains the artwork file names; a new requirement describes the control, Custom and its table.
- `artwork`: Artwork file name lists. The lists are chosen in Settings › Naming; `both` is one of the styles.
- `renamer`: Companion files move together. Copies are listed as the title's files, and a type with several names settles.

## Out of scope

- Collection (movie set) artwork names: they keep their own settings.
- Trailer and NFO file names.
- A free-text name per type: only the choices the backend knows are offered.
- Renaming the existing artwork by itself when the setting changes: that stays the job of Rename, with its preview and Undo.
- NFOs with several names keep today's newest-wins rule.

## Impact

`cv-artwork` (the catalogue and its generated file), `cv-rename` (the planner keeps names already held), `cv-app` (copies listed after an apply, dropped after an undo), the UI (`ArtworkNamesRow`, `artworkNames.ts`, schema and messages), tests. No IPC, settings-key or NFO change; the existing keys keep working.

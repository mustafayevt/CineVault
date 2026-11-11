## Why

A title whose folder was deleted outside CineVault stays in the library with "The files for this title are missing", on purpose: the drive may be off or the files moved. The only way out was "Remove from library (keep files)" in the ⋯ menu, which the notice never mentioned, so users could not find it (#100).

## What Changes

- **The notice offers the way out.** On the Movie page and the TV show page, the "files are missing" notice says why the title is kept (the drive may be offline, or the files only moved) and has a "Remove from library (keep files)" button. It is the same action as the ⋯ menu item and opens the Library afterwards.
- **Not for an offline drive.** The "library folder cannot be reached" notice gets no button: that title comes back by itself.
- **Bulk in the problem view.** While the Library lists "Missing files or offline", the floating selection bar shows "Remove from library" as a button before Delete, so the whole list can be cleared in one action. In every other view it stays under More.
- No backend change: removal already takes many titles in one call, and it has no Undo (the rows are deleted and the next scan adds the title back), so the confirmation stays the plain toast, "Removed N titles from the library. Its files stay; the next scan adds it back."

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `title-pages`: *Files that cannot be reached* names the button and the reason.
- `library-browsing`: *Floating selection bar* promotes "Remove from library" in the missing-files view.

## Impact

- UI only: `FilesNotice`, `LibrarySelectionBar`, `TitlesView`, the English messages. No IPC, schema or file-output change.

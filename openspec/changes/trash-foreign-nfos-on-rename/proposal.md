## Why

Downloads often come with a small `.nfo` info file named like the video: plain text about the video, audio and subtitles, not movie metadata. Rename treated it as the title's NFO and moved it onto the NFO name, after which CineVault could never write its own NFO there (issue #49). Keeping the info file under its old name is not enough either: when the folder is renamed it leaves the old folder behind with only that file in it. After a rename the title's folder should hold CineVault's NFO and nothing that looks like one.

## What Changes

- Rename tells CineVault's NFOs from other files with an `.nfo` extension using the same ownership test as the NFO writer (managed, empty or pointer may be carried; foreign may not).
- A foreign NFO of a renamed title (movie, show, season or episode) is set aside in the deleted-files destination like other rename leftovers, journaled so Undo brings it back. It never takes an NFO name, and it never beats CineVault's NFO as the newest one.
- A pointer file (only a URL or an ID) is still never removed by a rename.
- Writing is unchanged: a write never replaces a foreign file.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `nfo-files`: NFO ownership. A rename now sets a foreign NFO aside instead of leaving it.
- `renamer`: Companion files move together. Only readable NFOs are carried; other `.nfo` files are set aside.

## Impact

`cv-app` (the planner input and the rename profile), `cv-rename` (the TV planner's NFO clean-up and the executor's trash step), tests. No UI, IPC or settings change.

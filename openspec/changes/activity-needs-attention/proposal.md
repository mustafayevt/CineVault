## Why

When a move between library folders is cut short, the Move to menu blocks the title with "Resume it or roll it back in Activity first", but no such page exists. Nothing in the app lists the unfinished move or offers Resume or Roll back, so the title cannot be moved again and a hidden staging folder keeps its space on the destination (issue #70). Every warning and error that background work reports with `notify()` (moves, artwork, deletes, rejected provider keys, the IMDb dataset) is stored but never shown. A move the app's close cut short also waits for the user, although nothing went wrong (issue #69).

## What Changes

- History becomes **Activity** (`/activity`; `/history` leads there). Its top part, **Needs attention**, shows only while something needs the user:
  - Unfinished moves and renames, with where they were going, their titles, how far they got ("8 of 10 files copied, 15 GB of 86 GB"), why they stopped, and **Resume** and **Roll back**. While one is resumed, its row shows the live progress with Pause and Cancel.
  - Warnings and errors from background work, newest first, with their fix (open the provider's key, retry the IMDb dataset) and **Dismiss**, plus **Dismiss all**.
  - History stays below it as it is.
- The sidebar's Activity icon shows a warning dot while Needs attention has entries.
- **Automatic resume at start-up:** a move that the app's close or a crash cut short (no row says why it stopped), with its drives and folders all there, carries on by itself once the window is up. A toast says so and leads to Activity. A move that stopped on an error (disk full, drive unplugged, access denied) waits in Needs attention.
- **Fix from the Move to menu:** a title held by an unfinished move shows what that move is and how far it got, with "Resume that move", "Roll it back" and "Open in Activity" in the menu.
- The start-up warning about an unfinished move and the "move stopped" message come with a toast that opens Activity at that move.

## Out of scope

- Keeping warnings across restarts: plain warnings live for the session; unfinished moves and renames come back from the journal at every start.
- Resuming renames automatically (start-up recovery already settles them from the disk).
- Information messages in Needs attention: they are said in toasts.
- A separate notification centre: Activity is the one place.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `drives`: Unfinished moves are recovered. Moves cut short by a close resume by themselves; the others are listed in Activity's Needs attention and in the Move to menu with Resume and Roll back.
- `operation-history`: History is shown on the Activity page under Needs attention.
- `app-shell`: the sidebar's icon row has Activity with a warning dot.

## Impact

`cv-app` (`library_ops::recovery`: title ids, destination, bytes and the start-up resume flag of an unfinished batch), `cv-ipc` (`UnfinishedBatchDto`), `src-tauri` (no start-up warning for a batch the window resumes), the UI (new `features/activity`, the History page, the Move to menu, the sidebar, the router, the notice stream), `ui/src/bindings.ts`, tests and the changelog.

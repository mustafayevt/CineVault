## Why

Clicking a drive in the sidebar opens the Drives page but leaves another drive chosen there, so the cards and the titles show a different drive than the one clicked (#52). And the page's numbers can only wait for the next automatic read: after copying files in Explorer or plugging a drive in there is no way to ask for fresh numbers now.

## What Changes

- The chosen drive lives in the address (`/drives?drive=<id>`). Clicking a sidebar drive chooses that drive on the Drives page, also while the page is already open; choosing a card updates the address, so Back and Forward follow.
- While the Drives page is open the sidebar shows the chosen drive as selected, so the sidebar and the cards always agree.
- The Drives page header gets a **Refresh** icon button next to "Updated …" that reads every drive again at once, shows that it is working, and then says "Updated just now". The sidebar meters refresh with it.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `drives`: "Drives in the sidebar" (clicking a drive chooses it, the chosen drive is marked), "Drives page header" (Refresh), "Space numbers stay current" (read again on Refresh).

## Impact

- UI only: `ui/src/router.tsx` (search of `/drives`), `ui/src/features/shell/DriveMeters.tsx`, `ui/src/features/drives/DrivesPage.tsx`, the i18n catalog and their tests.
- No backend, IPC or file changes: the overview command already reads the disks on every call.

## Out of scope

- Keyboard shortcuts for Refresh (Ctrl+R and F5 reload the window in the desktop app).
- Refreshing a single drive, or changing how often the numbers are read on their own.

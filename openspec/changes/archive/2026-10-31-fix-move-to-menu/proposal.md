## Why

The Move to menu that opens from the selection bar, the right-click menu and the title page felt broken. A destination that could not take the titles showed its reason in a floating tip inside the menu, which overflowed the menu and gave it a vertical and a horizontal scrollbar even with one destination; the drive name and the path shared one line, so the path was cut at its end; a disabled destination looked only greyed out, with no reason in sight and no hint what to do. The menu was modal, so the first click outside only closed it and never reached the list (the user had to click a title twice), and pressing "Move to…" again closed and reopened it instead of closing it. The menu was placed from a snapshot of the button taken while the selection bar was still sliding in, so it could cover the bar, and the size in its heading read "0 B" when the first library folder was the one the titles are in already.

## What Changes

- Each destination is a row of its own: the drive's name, the folder's label and the free space on the first line, the library folder's path shortened in the middle on the second (the whole path is its tooltip), and either the space bar with the free space after the move or the reason it cannot be chosen.
- A destination that cannot be chosen says why on its row in the warning colour, with what to do where there is something to do: "Not enough space: needs 71.4 GB, 1.02 GB free.", "The drive is offline. Connect it to move here.", and "Another move is running. Wait for it to finish." while a move runs. It stays reachable with the arrow keys and ignores Enter and clicks.
- While the free space is measured the menu shows placeholder rows (after 150 ms, like every skeleton); with no other folder of the type it says so and offers Open Settings (Settings › Library folders); when the folders cannot be read it says so and offers Try again.
- Only the list scrolls, only vertically and only when it is longer than the window allows; the heading and the line about the source drive stay in view. The menu stays inside the window at small sizes and opens above the selection bar without covering it.
- The menu is no longer modal: a click outside closes it and still does what it was meant to do. Pressing the control that opened it closes it. Esc closes it, keeps the selection and gives the focus back to that control. Opened from the keyboard, the first destination that can be chosen takes the focus.

Out of scope: what a move does once a destination is chosen (planning, copying, verifying, Undo), the other menus and their disabled-item tips, and the selection bar's own actions.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `drives`: "Move to with space before and after" (the reasons a destination is disabled, the empty state, and how the menu opens and closes).
- `ui-design-system`: "Disabled controls explain themselves" (a multi-line menu row such as a Move to destination shows its reason on the row instead of a floating line).

## Impact

UI only: `ui/src/features/move-to/` (the menu, its anchor and the options it reads), the shared `Menu` (two optional outside-interaction handlers), the English messages of the Move to menu, and their tests. No backend, IPC or file change.

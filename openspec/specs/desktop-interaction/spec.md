# desktop-interaction Specification

## Purpose
Makes every CineVault page behave like a native desktop program rather than a web page: pointer shapes, text selection, the app's own right-click menus, the key model (no single-key shortcuts), the Esc stack, desktop multi-select, the Delete keys, drag and drop, a keyboard-only focus ring, tooltips, instant feedback, scrollbars and remembered window state.

## Requirements

### Requirement: Pointers match what a control does
Every clickable element (buttons, posters, table rows, chips, tabs, menu items, links, nav items) SHALL show the hand pointer. Text fields SHALL show the text cursor, column dividers and pane splitters the resize cursor, and a disabled control the not-allowed pointer. Everything else SHALL show the arrow, and the wait cursor SHALL never cover the window while work runs in the background.

#### Scenario: Hovering a poster
- **WHEN** the pointer moves over a poster in the Library grid
- **THEN** the pointer is a hand

#### Scenario: Disabled control
- **WHEN** the pointer moves over a disabled button
- **THEN** the pointer is not-allowed and the button keeps its disabled colours

#### Scenario: Long work running
- **WHEN** a scrape of 25 titles runs
- **THEN** the pointer stays an arrow or a hand as usual, and progress shows in the sidebar progress card

### Requirement: No browser leftovers
Interface text (labels, buttons, headings, nav, chips, menus) SHALL NOT be selectable. Titles, values, paths, release names, NFO text and log lines SHALL stay selectable so they can be copied, and text fields SHALL stay selectable. Images and links SHALL NOT start a ghost drag unless they sit inside a drag source. Buttons SHALL NOT underline on hover. The browser's own context menu SHALL never show.

#### Scenario: Dragging across a toolbar
- **WHEN** the user drags the mouse across the Library toolbar and filter chips
- **THEN** nothing is highlighted

#### Scenario: Copying a path
- **WHEN** the user drags over a folder path on the Movie page
- **THEN** the path is selected and Ctrl+C copies it

#### Scenario: Right-click on empty space
- **WHEN** the user right-clicks an empty area of a page
- **THEN** no browser menu appears

### Requirement: Browser keys are off
Reload (F5, Ctrl+R, Ctrl+Shift+R), print (Ctrl+P), view source (Ctrl+U), browser find (Ctrl+G, F3), the browser's refresh, search, home, stop and favourites keys and, in release builds, the developer tools keys (F12, Ctrl+Shift+I, J and C) SHALL do nothing of their own. Ctrl+F (⌘F on macOS) SHALL focus the title-bar library search and select its text. The browser's zoom SHALL stay off; the zoom keys drive the app zoom defined in `app-shell`.

#### Scenario: F5 in a release build
- **WHEN** the user presses F5 on the Rename page
- **THEN** the page does not reload and nothing is lost

#### Scenario: Ctrl+F
- **WHEN** the user presses Ctrl+F on the Movie page
- **THEN** the title-bar library search gets focus with its text selected

### Requirement: No single-key shortcuts
Keyboard shortcuts SHALL be Ctrl or Alt chords (⌘ or ⌥ on macOS), Delete, Shift+Delete, Esc, F2, Enter and the arrow keys only. A plain letter or digit key SHALL never trigger an action, so typing can never change the library by accident. F2 SHALL rename the selected titles, Ctrl+A SHALL select all in the focused list, and Ctrl+Z SHALL undo the most recent action whose toast offers Undo.

#### Scenario: Typing a letter on the Library
- **WHEN** a title is selected in the Library and the user presses "r"
- **THEN** nothing happens to the title

#### Scenario: F2
- **WHEN** three titles are selected in the Library and the user presses F2
- **THEN** the Rename page opens for those three titles

### Requirement: Own right-click menus
Posters, table rows, episode rows, files, drives and artwork tiles SHALL have the app's own right-click menus listing the same actions, in the same order and with the same names, as the selection bar or page header for that thing, with key hints. Right-clicking an unselected item SHALL select it alone first; right-clicking inside a selection SHALL act on the whole selection. Shift+F10 and the context-menu key SHALL open the menu of the focused item.

#### Scenario: Right-click inside a selection
- **WHEN** four movies are selected and the user right-clicks one of them
- **THEN** the menu acts on all four, with items such as "Rename 4 titles", "Scrape 4 titles", "Move to…", "Delete to Recycle Bin" and "Delete permanently…"

#### Scenario: Keyboard opens the menu
- **WHEN** a table row has focus and the user presses Shift+F10
- **THEN** that row's menu opens under it

### Requirement: Esc closes the innermost thing
One Esc press SHALL close exactly one thing, in this order: an open menu or tooltip, an open dialog, an open picker page (returning to where the user was), and then the current selection. A field that holds text SHALL keep the key: Esc SHALL never discard typed text without first clearing that field.

#### Scenario: Esc in the match picker
- **WHEN** the match picker is open from a Movie page and no menu is open
- **THEN** Esc closes the picker and the Movie page shows again

#### Scenario: Esc clears a selection
- **WHEN** three titles are selected in the Library and nothing is open
- **THEN** Esc clears the selection and the selection bar goes away

### Requirement: Desktop multi-select in grids and tables
In every multi-select grid and table, a click SHALL select one item, Ctrl-click (⌘-click) SHALL add or remove an item, Shift-click SHALL select the range from the last clicked item, Ctrl+A SHALL select every item in the current list, and the arrow keys SHALL move the current item, with Shift extending the selection. Double-click or Enter SHALL open the current item where the list has an Open action.

#### Scenario: Range select in the table
- **WHEN** the user clicks the second row and Shift-clicks the fifth row
- **THEN** rows two to five are selected and the selection bar says "4 selected"

#### Scenario: Ctrl+A in a drive's table
- **WHEN** a drive's title table has focus and the user presses Ctrl+A
- **THEN** every title of that drive is selected

### Requirement: Delete keys
In the Library and Drives lists, Delete SHALL start Delete to Recycle Bin for the selection and Shift+Delete SHALL open the permanent-delete confirmation, as defined by `title-deletion`. Neither key SHALL act on titles while focus is in a text field.

#### Scenario: Delete key in a search field
- **WHEN** the library search has focus and the user presses Delete
- **THEN** a character of the search text is deleted and no title is touched

### Requirement: Drag and drop where it is natural
Picture files SHALL be droppable onto the artwork picker's drop targets. Folders SHALL be droppable onto the first-run Folders step and onto Settings › Library folders to add them. While something droppable is dragged over the window, the drop target SHALL show that it accepts the drop; a drop outside a target SHALL change nothing.

#### Scenario: Drop a poster file
- **WHEN** the user drops `poster.jpg` from Explorer onto the poster slot of the artwork picker
- **THEN** the picker takes that image as the poster candidate

#### Scenario: Drop a folder in Settings
- **WHEN** the user drops `E:\TV` onto Settings › Library folders
- **THEN** the folder is added and a toast says "Added 1 folder"

### Requirement: Focus ring only for keyboard use
A focus ring SHALL show only when focus came from the keyboard; clicking a control SHALL NOT draw a ring. Every control SHALL stay reachable with Tab. A text field SHALL mark focus with its gold border whether it was clicked or tabbed into.

#### Scenario: Click a button
- **WHEN** the user clicks Rename in the selection bar
- **THEN** no focus ring appears around it

#### Scenario: Tab to a button
- **WHEN** the user presses Tab until the Rename button has focus
- **THEN** the focus ring shows around it

### Requirement: Hover and pressed states
Every interactive control SHALL change its look on hover and while pressed, one step up the surface ladder and then the pressed surface, so the user sees it respond before anything happens.

#### Scenario: Pressing a table row
- **WHEN** the user presses the mouse on a Library table row
- **THEN** the row shows its pressed state until the button is released

### Requirement: Tooltips after a short delay
Icon-only buttons and text cut off by an ellipsis SHALL show a tooltip with the full name or text after the pointer rests on them for 500 ms, or at once when focus arrives from the keyboard. Tooltips SHALL use the floating recipe and hide when the pointer leaves, on click, on scroll and on Esc.

#### Scenario: Truncated title
- **WHEN** the pointer rests on a table cell showing a title cut off with an ellipsis
- **THEN** after half a second a tooltip shows the whole title

### Requirement: Instant feedback without full-page spinners
Pressing a control SHALL show its effect within one frame (pressed state, selection or an optimistic change). A loading area SHALL show a skeleton shaped like its content only after 150 ms without data, content arriving SHALL NOT shift the layout, and no page SHALL show a full-page spinner. Confirmations SHALL come as toasts at the bottom right, with Undo for reversible actions.

#### Scenario: Fast data
- **WHEN** a page's data arrives within 100 ms
- **THEN** no skeleton is shown at all

#### Scenario: Slow data
- **WHEN** the Drives page's data takes 600 ms
- **THEN** skeleton cards appear after 150 ms and are replaced by the cards without the layout jumping

### Requirement: Solid under load
Long lists and grids SHALL be virtualised so scrolling 20,000 titles stays smooth, sizes SHALL be reserved so late data never moves what is below it, and a page change SHALL be a short crossfade with no blank frame. Mouse and keyboard SHALL both reach every action.

#### Scenario: Ratings arrive late
- **WHEN** the Movie page opens and its ratings arrive 300 ms later
- **THEN** the ratings fill their reserved space and nothing below them moves

### Requirement: Thin scrollbars
Scrollable areas SHALL use thin scrollbars drawn from the design tokens in both themes and SHALL NOT show the browser's default scrollbar.

#### Scenario: Scrolling the Library table
- **WHEN** the Library table scrolls
- **THEN** a thin token-coloured scrollbar shows at its right edge

### Requirement: Window state is remembered
The main window's size, position and maximised state SHALL be restored at the next start. The widths the user gives to resizable table columns SHALL be remembered. Windows snap layouts and macOS native full screen SHALL work.

#### Scenario: Restart restores the window
- **WHEN** the user resizes the window to 1500×950, quits and starts CineVault again
- **THEN** the window opens at 1500×950 in the same place

#### Scenario: Column width kept
- **WHEN** the user widens the Library table's Title column and restarts
- **THEN** the Title column keeps the width the user gave it

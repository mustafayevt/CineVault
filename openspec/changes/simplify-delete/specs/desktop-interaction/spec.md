## MODIFIED Requirements

### Requirement: Browser keys are off
In the desktop app, reload (F5, Ctrl+R, Ctrl+Shift+R), print (Ctrl+P), view source (Ctrl+U), browser find (Ctrl+G, F3), the browser's refresh, search, home, stop and favourites keys and, in release builds, the developer tools keys (F12, Ctrl+Shift+I, J and C) SHALL do nothing of their own. Ctrl+F (⌘F on macOS) SHALL focus the title-bar library search and select its text. The browser's zoom SHALL stay off; the zoom keys drive the app zoom defined in `app-shell`. In a browser, the page SHALL take Ctrl+F for the library search, Esc, Delete and the arrow keys, and SHALL leave reload, the browser's zoom and the browser's own developer tools to the browser; a reload SHALL keep the page's filters and scroll in the address and the stored view state.

#### Scenario: F5 in a release build
- **WHEN** the user presses F5 on the Rename page in the desktop app
- **THEN** the page does not reload and nothing is lost

#### Scenario: Ctrl+F
- **WHEN** the user presses Ctrl+F on the Movie page
- **THEN** the title-bar library search gets focus with its text selected

#### Scenario: Reload in a browser
- **WHEN** the user presses F5 in a browser on the Library page
- **THEN** the page reloads and returns with the same filters and scroll position

### Requirement: No single-key shortcuts
Keyboard shortcuts SHALL be Ctrl or Alt chords (⌘ or ⌥ on macOS), Delete, Esc, F2, Enter and the arrow keys only. A plain letter or digit key SHALL never trigger an action, so typing can never change the library by accident. F2 SHALL rename the selected titles, Ctrl+A SHALL select all in the focused list, and Ctrl+Z SHALL undo the most recent action whose toast offers Undo.

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
- **THEN** the menu acts on all four, with items such as "Rename 4 titles", "Scrape 4 titles", "Move to…" and "Delete"

#### Scenario: Keyboard opens the menu
- **WHEN** a table row has focus and the user presses Shift+F10
- **THEN** that row's menu opens under it

### Requirement: Delete keys
In the Library and Drives lists, Delete SHALL delete the selection as Settings › Deleted files says, as defined by `title-deletion`. Shift+Delete SHALL do the same as Delete; there is no separate permanent-delete key. Neither key SHALL act on titles while focus is in a text field.

#### Scenario: Delete key in a search field
- **WHEN** the library search has focus and the user presses Delete
- **THEN** a character of the search text is deleted and no title is touched

#### Scenario: Shift+Delete is Delete
- **WHEN** a title is selected, Deleted files is "Recycle Bin" and the user presses Shift+Delete
- **THEN** the title goes to the Recycle Bin with Undo, as with Delete, and nothing is erased

# Spec Delta

## MODIFIED Requirements

### Requirement: No browser leftovers
Interface text (labels, buttons, headings, nav, chips, menus) SHALL NOT be selectable. Titles, values, paths, release names, NFO text and log lines SHALL stay selectable so they can be copied, and text fields SHALL stay selectable. Images and links SHALL NOT start a ghost drag unless they sit inside a drag source. Buttons SHALL NOT underline on hover. In the desktop app the browser's own context menu SHALL never show. In a browser it SHALL be suppressed where the app shows its own menu, and left to the browser elsewhere.

#### Scenario: Dragging across a toolbar
- **WHEN** the user drags the mouse across the Library toolbar and filter chips
- **THEN** nothing is highlighted

#### Scenario: Copying a path
- **WHEN** the user drags over a folder path on the Movie page
- **THEN** the path is selected and Ctrl+C copies it

#### Scenario: Right-click on empty space
- **WHEN** the user right-clicks an empty area of a page in the desktop app
- **THEN** no browser menu appears

#### Scenario: Right-click on a poster in a browser
- **WHEN** the user right-clicks a poster in a browser
- **THEN** the app's title menu opens and the browser's own menu does not

### Requirement: Browser keys are off
In the desktop app, reload (F5, Ctrl+R, Ctrl+Shift+R), print (Ctrl+P), view source (Ctrl+U), browser find (Ctrl+G, F3), the browser's refresh, search, home, stop and favourites keys and, in release builds, the developer tools keys (F12, Ctrl+Shift+I, J and C) SHALL do nothing of their own. Ctrl+F (⌘F on macOS) SHALL focus the title-bar library search and select its text. The browser's zoom SHALL stay off; the zoom keys drive the app zoom defined in `app-shell`. In a browser, the page SHALL take Ctrl+F for the library search, Esc, Delete, Shift+Delete and the arrow keys, and SHALL leave reload, the browser's zoom and the browser's own developer tools to the browser; a reload SHALL keep the page's filters and scroll in the address and the stored view state.

#### Scenario: F5 in a release build
- **WHEN** the user presses F5 on the Rename page in the desktop app
- **THEN** the page does not reload and nothing is lost

#### Scenario: Ctrl+F
- **WHEN** the user presses Ctrl+F on the Movie page
- **THEN** the title-bar library search gets focus with its text selected

#### Scenario: Reload in a browser
- **WHEN** the user presses F5 in a browser on the Library page
- **THEN** the page reloads and returns with the same filters and scroll position

### Requirement: Drag and drop where it is natural
Picture files SHALL be droppable onto the artwork picker's drop targets in the desktop app and chosen or dropped on the upload control in a browser. Folders SHALL be droppable onto the first-run Folders step and onto Settings › Library folders in the desktop app. In a browser a dropped folder SHALL NOT be accepted, because the browser provides no absolute path, and the page SHALL say "Use Add folder…". While something droppable is dragged over the window, the drop target SHALL show that it accepts the drop; a drop outside a target SHALL change nothing.

#### Scenario: Drop a poster file
- **WHEN** the user drops `poster.jpg` from Explorer onto the poster slot of the artwork picker in the desktop app
- **THEN** the picker takes that image as the poster candidate

#### Scenario: Drop a folder in Settings
- **WHEN** the user drops `E:\TV` onto Settings › Library folders in the desktop app
- **THEN** the folder is added and a toast says "Added 1 folder"

#### Scenario: Folder drop in a browser
- **WHEN** the user drops a folder onto Settings › Library folders in a browser
- **THEN** the page says "Use Add folder…" and no folder is added

### Requirement: Window state is remembered
In the desktop app, the main window's size, position and maximised state SHALL be restored at the next start. The widths the user gives to resizable table columns SHALL be remembered in both the desktop app and a browser. Windows snap layouts and macOS native full screen SHALL work in the desktop app; a browser uses the browser's own window and has no state for the app to restore.

#### Scenario: Restart restores the window
- **WHEN** the user resizes the desktop window to 1500×950, quits and starts CineVault again
- **THEN** the window opens at 1500×950 in the same place

#### Scenario: Column width kept
- **WHEN** the user widens the Library table's Title column and restarts
- **THEN** the Title column keeps the width the user gave it

#### Scenario: Browser reload
- **WHEN** the user reloads a browser page
- **THEN** the column widths are kept and no window size or position is applied

## ADDED Requirements

### Requirement: Browser interaction
When the interface is served to a browser, the pointer, right-click menu, Esc stack, multi-select, Delete keys, focus ring, tooltip, scrollbar and instant-feedback rules SHALL apply as they do in the desktop app, except where "Browser keys are off" and "Drag and drop where it is natural" say otherwise. Single-key shortcuts SHALL stay off. The browser's own back and forward buttons SHALL navigate the app's pages, and a page opened by address SHALL behave as a deep link. Window state, snap layouts and native full screen SHALL NOT apply. In a browser the page SHALL NOT trap the browser's reload, zoom or developer tools.

#### Scenario: Browser back
- **WHEN** the user opens the Library, opens a movie, and presses the browser's back button
- **THEN** the Library returns with its filters and scroll as they were

#### Scenario: Browser keys
- **WHEN** the user presses a letter outside a text field
- **THEN** nothing is triggered, as in the desktop app

#### Scenario: Browser zoom
- **WHEN** the user presses Ctrl+plus in a browser
- **THEN** the browser zooms the page and the app does not intercept it

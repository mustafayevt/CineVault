# Spec Delta

## MODIFIED Requirements

### Requirement: Custom window chrome
In the desktop app, the main window SHALL draw its own 38px title bar over the content column, with the library search in its centre and the "Ctrl F" hint ("⌘ F" on macOS). On Windows and Linux the window SHALL have no native frame and SHALL draw minimize, maximize and close at the top right, with the native snap-layout flyout on maximize on Windows 11; on macOS the native traffic lights SHALL sit in the sidebar panel's top row. Dragging empty title-bar space SHALL move the window, dragging a window edge SHALL resize it, and the window SHALL NOT shrink below 1100×680. A browser has no app window; its shell follows the web-server capability's "Browser shell" and shows no window controls.

#### Scenario: Windows snap layouts
- **WHEN** the user hovers the maximize button on Windows 11
- **THEN** the native snap-layout flyout appears

#### Scenario: Linux caption buttons
- **WHEN** CineVault runs on a Linux desktop
- **THEN** no system title bar is shown, and the window's own minimize, maximize and close buttons work

#### Scenario: Search from the title bar
- **WHEN** the user types "example" into the title-bar search on the Drives page and presses Enter
- **THEN** the Library opens filtered by that text, and Esc or the clear button empties the search

#### Scenario: Minimum size
- **WHEN** the user drags the window edge to make it 800px wide
- **THEN** the window stops at 1100px wide

#### Scenario: Browser chrome
- **WHEN** the interface is served to a browser
- **THEN** the title-bar search and page header are shown without minimize, maximize or close controls

### Requirement: Single instance
In the desktop app, launching CineVault while it is already running SHALL hand over to the running copy and bring its window to the front (shown, restored if minimised and focused) instead of starting a second process that could damage the library database. On Linux, where the hand-over needs the D-Bus session bus, a second process that cannot reach the running copy SHALL find the data folder's `instance.lock` or `library-owner.json` held, say "CineVault is already running." and exit without opening the library. A desktop app started against a data folder a server holds SHALL refuse it and name the server.

#### Scenario: Second launch
- **WHEN** the user starts CineVault while it is already running
- **THEN** the existing window comes to the front and no second process keeps running

#### Scenario: No session bus on Linux
- **WHEN** CineVault is started a second time on Linux in a session without a D-Bus session bus
- **THEN** the second process says "CineVault is already running." and exits, and the first keeps the library

#### Scenario: Server holds the folder
- **WHEN** the desktop app starts with the data folder a server holds
- **THEN** it says the library is in use by the server and opens no library

### Requirement: App zoom
In the desktop app, the app SHALL have its own zoom, Settings › Appearance › Size, in the steps 80, 90, 100, 110, 125 and 150 percent (100 by default, kept in `appearance.zoom`). Ctrl plus and Ctrl minus (⌘ on macOS) SHALL step it and Ctrl 0 SHALL reset it, applied as the native webview zoom; Ctrl+wheel and pinch SHALL NOT zoom. In a browser the browser's own zoom SHALL be used, the app zoom control SHALL NOT be shown, and Ctrl plus/minus SHALL be left to the browser.

#### Scenario: Zoom in with the keyboard
- **WHEN** the size is 100% and the user presses Ctrl plus in the desktop app
- **THEN** the whole app draws at 110% and Settings › Appearance shows 110%

#### Scenario: Browser zoom
- **WHEN** the user presses Ctrl plus in a browser
- **THEN** the browser zooms the page and the app does not intercept it

## ADDED Requirements

### Requirement: Browser surface
When CineVault is served to a browser, the shell SHALL keep the sidebar with its destinations, counts, drives and progress card, the start page, back and forward navigation, themes, localisation-ready text, accessibility and failure placement. The sign-in and welcome pages SHALL replace the shell like the first-run setup does. The browser SHALL use its own history, and the shell SHALL NOT install a tray icon, taskbar progress or window-state plugin.

#### Scenario: Browser start
- **WHEN** a signed-in user opens the server's address
- **THEN** the Library shows inside the sidebar shell, with the same destinations and counts as the desktop app

#### Scenario: Sign-in
- **WHEN** a signed-out user opens the address
- **THEN** the sign-in page replaces the shell, and no sidebar is shown

#### Scenario: No tray
- **WHEN** the server runs in a container
- **THEN** no tray icon or taskbar button is created

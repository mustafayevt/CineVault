# app-shell Specification

## Purpose
Defines the CineVault application frame on Windows and macOS: the native window with its custom chrome and title bar, the floating sidebar panel with its destinations, counts, drives and progress card, back and forward navigation, the start page, single-instance behaviour, startup speed, themes, localisation-ready text, accessibility and how failures are shown. Every page of the app is drawn inside this shell; the first-run setup is the only screen that replaces it.

## Requirements

### Requirement: Native desktop application
CineVault SHALL ship as an installable desktop application for Windows 10/11 (x64), macOS (Apple Silicon and Intel) and, as a beta, Linux (x86_64, desktops with WebKitGTK 4.1 such as Ubuntu 22.04 or newer), that needs no separately installed language runtime. Browsing the library, its artwork and its details SHALL work without a network connection.

#### Scenario: Launch on a clean machine
- **WHEN** a user installs CineVault on a Windows 11 machine with no extra runtime installed
- **THEN** the application starts and shows the Library or the first-run setup

#### Scenario: Launch on Ubuntu
- **WHEN** a user runs the CineVault AppImage on Ubuntu 24.04 with `libfuse2t64` installed
- **THEN** the application starts and shows the Library or the first-run setup

#### Scenario: Offline browsing
- **WHEN** the machine has no network connection
- **THEN** the library, its posters and its title pages stay browsable, and only online actions report that they cannot reach the network

### Requirement: Fast startup
The app SHALL show an interactive Library within 1.5 seconds of launch for a library of 5,000 movies on an SSD, measured on a release build. Work that the first screen does not need SHALL run later as background jobs.

#### Scenario: Cold start with a large library
- **WHEN** the release app is launched with a 5,000-movie library already stored
- **THEN** the poster grid is visible and scrollable within 1.5 seconds, and no rescan blocks input

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

### Requirement: Sidebar destinations and counts
The sidebar panel SHALL hold the brand mark with Back and Forward; the destinations Library, Scrape, Rename, Drives and Torrents; the Drives section with each drive's free space; and at its foot the progress card above the icon row History, Theme, Settings and collapse. Scrape and Rename SHALL show waiting counts as pills, Drives SHALL show "N low" in the warning tone, and a zero count SHALL show no pill. The active destination SHALL have a gold icon; title and picker pages count as the Library.

#### Scenario: Counts on the nav
- **WHEN** 12 titles wait to be scraped and 71 renames are pending
- **THEN** Scrape shows "12" and Rename shows "71" as pills

#### Scenario: Active page
- **WHEN** the user is on the Rename page
- **THEN** only the Rename item has the active fill and the gold icon

#### Scenario: A movie page belongs to the Library
- **WHEN** the user opens a movie from the grid
- **THEN** the Library item stays active

### Requirement: Floating sidebar panel
The sidebar SHALL be a full-height panel floating on the window ground with an 8px gutter above, left of and below it and 8px between it and the content, drawn with the floating recipe (floating surface, 1px edge, floating shadow, 14px radius). It SHALL be 200px wide expanded. Page content SHALL stay flat on the window ground with no card around it.

#### Scenario: Gutters
- **WHEN** the Library page is shown at 1440×900
- **THEN** the panel sits 8px from the window's top, left and bottom edges and 8px from the content, and the content has no frame of its own

### Requirement: Sidebar collapses to icons
The collapse button SHALL switch the sidebar between the full panel and an icon-only panel (50px wide, 76px on macOS), and the choice SHALL be kept in the `shell.sidebar` setting across restarts. Collapsed, each destination SHALL show its icon with its name and count as a tooltip, drives SHALL show as small meters and the progress card SHALL become a ring.

#### Scenario: Collapse and restart
- **WHEN** the user collapses the sidebar and restarts the app
- **THEN** the sidebar opens collapsed, and hovering the Scrape icon shows its name and count

### Requirement: Progress card
While background work runs, the foot of the sidebar SHALL show a progress card naming the job that started first in bold ("Scraping 3 of 25"), the percentage in muted text and a thin gold bar, followed by "+N more" when several jobs run. Choosing the card SHALL open a floating list of the running jobs, each with Pause or Resume and Cancel. When nothing runs, the card SHALL disappear and the icon row SHALL stay in place.

#### Scenario: Two jobs
- **WHEN** a scrape and a move run at once
- **THEN** the card names the one started first with "+1 more", and choosing it lists both with Pause and Cancel

#### Scenario: Idle
- **WHEN** no job runs
- **THEN** no progress card is shown

### Requirement: Starts on Library
After the first-run setup, the app SHALL open on the Library page, on the library type and in the view the user used last, and back and forward history SHALL start there. The root address SHALL redirect to the Library.

#### Scenario: Normal start
- **WHEN** the user starts CineVault with a library and last used the TV shows table
- **THEN** the Library opens on TV shows in the table view

### Requirement: Back and forward
The Back and Forward buttons in the sidebar panel SHALL move through the pages and picker pages the user visited. Going back or forward to a page SHALL return to its scroll position, while a new page SHALL open at its top. Alt+Left, Alt+Right, the keyboard's browser back and forward keys and the mouse's back and forward buttons SHALL do the same; Alt with an arrow SHALL stay with a focused text field.

#### Scenario: Back to the scrolled grid
- **WHEN** the user scrolls the Library grid, opens a movie and presses Back
- **THEN** the grid shows at the same scroll position with the same filters

#### Scenario: Nothing to go back to
- **WHEN** the app has just started on the Library
- **THEN** the Back button is disabled

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

### Requirement: Themes
The app SHALL offer Dark (the default), Light and "Match Windows" ("Match macOS" on macOS, "Match system" on Linux), which follows the operating system. The Theme button in the sidebar's icon row SHALL switch between dark and light, and Settings › Appearance SHALL offer all three; either way the change SHALL apply at once without a restart and be kept in the `appearance.theme` setting.

#### Scenario: Switch theme live
- **WHEN** the user presses the Theme button in the dark theme
- **THEN** the whole window turns light at once and Settings › Appearance shows Light

#### Scenario: Follow the system
- **WHEN** the theme is "Match Windows" and Windows switches to its light mode
- **THEN** CineVault turns light without a restart

### Requirement: Localisation-ready text
Every user-facing string SHALL come from the string catalogue under `ui/src/i18n`, keyed by a stable id, with parameters and plural forms; English SHALL be the shipped language. Dates, times and numbers SHALL follow the regional format (`general.locale`, the system's by default). Every file size SHALL follow the size-units setting (`advanced.size-units`), which SHALL default to base 2 (KiB, MiB, GiB, as Explorer counts) on Windows and to base 10 (kB, MB, GB, as Finder counts) elsewhere.

#### Scenario: Sizes match Explorer on Windows
- **WHEN** the size units were never changed on Windows and a file holds 5,046,586,573 bytes
- **THEN** its size reads "4.7 GiB"

#### Scenario: Sizes match Finder on macOS
- **WHEN** the size units were never changed on macOS and a file holds 5,046,586,573 bytes
- **THEN** its size reads "5.05 GB"

### Requirement: Accessibility
Text SHALL meet WCAG 2.1 AA contrast in both themes. Every control, including posters, rows, chips and tabs, SHALL be keyboard reachable with a visible focus ring for keyboard focus, and every icon-only control SHALL have an accessible name. The UI SHALL honour reduced motion and Windows high-contrast mode, and a page change SHALL move focus to the page's main content. Job completions and failures SHALL be announced politely to screen readers, without progress ticks.

#### Scenario: Job finished
- **WHEN** a scan finishes while a screen reader runs
- **THEN** it announces the scan's end once, and said nothing during its progress

#### Scenario: High contrast
- **WHEN** Windows high-contrast mode is on
- **THEN** controls, focus rings and text use the system's high-contrast colours

### Requirement: Failures shown where they happen
A failed action SHALL be reported as an error toast that says what failed, on the page where the user is, and the affected titles SHALL stay where they can be fixed (titles that could not be scraped stay in the Scrape list; the Library's Problems filter finds titles that need attention). There SHALL be no separate notification or activity pane, and no failure SHALL open a dialog.

#### Scenario: Scrape failed for some titles
- **WHEN** a scrape of 25 titles fails for 3 because the provider was unreachable
- **THEN** a toast says "3 titles could not be scraped and stay in the list", and the three stay in the Scrape list

### Requirement: Legible at common screen sizes
Every page SHALL show its content without clipped labels or horizontal page scrolling at 1366×768, 1440×900, 1920×1080 and 3840×2160, at 100%, 125% and 150% display scaling. Grids and tables SHALL use the full width of the content column, and reading text SHALL stay narrow enough to read.

#### Scenario: Small laptop screen
- **WHEN** the app runs at 1366×768 with 125% scaling
- **THEN** the Movie page header actions fit and no page scrolls horizontally

### Requirement: App zoom
In the desktop app, the app SHALL have its own zoom, Settings › Appearance › Size, in the steps 80, 90, 100, 110, 125 and 150 percent (100 by default, kept in `appearance.zoom`). Ctrl plus and Ctrl minus (⌘ on macOS) SHALL step it and Ctrl 0 SHALL reset it, applied as the native webview zoom; Ctrl+wheel and pinch SHALL NOT zoom. In a browser the browser's own zoom SHALL be used, the app zoom control SHALL NOT be shown, and Ctrl plus/minus SHALL be left to the browser.

#### Scenario: Zoom in with the keyboard
- **WHEN** the size is 100% and the user presses Ctrl plus in the desktop app
- **THEN** the whole app draws at 110% and Settings › Appearance shows 110%

#### Scenario: Browser zoom
- **WHEN** the user presses Ctrl plus in a browser
- **THEN** the browser zooms the page and the app does not intercept it

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

### Requirement: Keep running in the tray
While "Keep running in the tray when the window is closed" and Browser access are both on, the desktop app SHALL show a tray icon with "Open CineVault" and "Quit CineVault", and closing the main window SHALL hide it instead of quitting; the tray or a new launch SHALL show it again. "Quit CineVault" SHALL quit as closing does otherwise, including a pending install-on-close update. Otherwise there SHALL be no tray icon and closing SHALL quit.

#### Scenario: Close with Browser access on
- **WHEN** both options are on and the user closes the main window
- **THEN** the window disappears, the tray icon stays, and a browser signed in on the phone keeps working

#### Scenario: Open again
- **WHEN** the window is hidden and the user chooses "Open CineVault" in the tray or starts CineVault again
- **THEN** the same window comes back with the library

#### Scenario: Option off
- **WHEN** "Keep running in the tray" is off and the user closes the window
- **THEN** CineVault quits, Browser access stops and there is no tray icon

### Requirement: Start at sign-in
"Start CineVault when I sign in" SHALL register CineVault to start when the user signs in to the computer (a per-user Run entry on Windows, an autostart `.desktop` file on Linux, a login item on macOS) and SHALL remove that registration when turned off. The switch SHALL show whether the registration exists. A start at sign-in SHALL open no window when Browser access and "Keep running in the tray" are both on, and SHALL open the window otherwise.

#### Scenario: Turn on
- **WHEN** the user turns "Start CineVault when I sign in" on and signs in to Windows again
- **THEN** CineVault starts in the tray, Browser access runs, and no window opens

#### Scenario: Turn off
- **WHEN** the user turns it off
- **THEN** the registration is removed and the next sign-in does not start CineVault

## MODIFIED Requirements

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

### Requirement: Custom window chrome
The main window SHALL draw its own 38px title bar over the content column, with the library search in its centre and the "Ctrl F" hint ("⌘ F" on macOS). On Windows and Linux the window SHALL have no native frame and SHALL draw minimize, maximize and close at the top right, with the native snap-layout flyout on maximize on Windows 11; on macOS the native traffic lights SHALL sit in the sidebar panel's top row. Dragging empty title-bar space SHALL move the window, dragging a window edge SHALL resize it, and the window SHALL NOT shrink below 1100×680.

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

### Requirement: Single instance
Launching CineVault while it is already running SHALL hand over to the running copy and bring its window to the front (shown, restored if minimised and focused) instead of starting a second process that could damage the library database. On Linux, where the hand-over needs the D-Bus session bus, a second process that cannot reach the running copy SHALL find the data folder's `instance.lock` held, say "CineVault is already running." and exit without opening the library.

#### Scenario: Second launch
- **WHEN** the user starts CineVault while it is already running
- **THEN** the existing window comes to the front and no second process keeps running

#### Scenario: No session bus on Linux
- **WHEN** CineVault is started a second time on Linux in a session without a D-Bus session bus
- **THEN** the second process says "CineVault is already running." and exits, and the first keeps the library

### Requirement: Themes
The app SHALL offer Dark (the default), Light and "Match Windows" ("Match macOS" on macOS, "Match system" on Linux), which follows the operating system. The Theme button in the sidebar's icon row SHALL switch between dark and light, and Settings › Appearance SHALL offer all three; either way the change SHALL apply at once without a restart and be kept in the `appearance.theme` setting.

#### Scenario: Switch theme live
- **WHEN** the user presses the Theme button in the dark theme
- **THEN** the whole window turns light at once and Settings › Appearance shows Light

#### Scenario: Follow the system
- **WHEN** the theme is "Match Windows" and Windows switches to its light mode
- **THEN** CineVault turns light without a restart

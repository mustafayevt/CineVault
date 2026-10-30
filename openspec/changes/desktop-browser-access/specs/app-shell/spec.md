## ADDED Requirements

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

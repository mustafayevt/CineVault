## ADDED Requirements

### Requirement: Browser access section
The desktop app's Settings page SHALL have a Browser access section, shown only in the desktop app, with a "Browser access" switch that is off by default and cannot be turned on until a password exists, and the password: "Create password" (a new password and its confirmation, at least 12 characters) when none exists, otherwise "Change password", which signs every browser out. Its values SHALL be kept in the data folder's `browser-access.json`, written atomically, so a browser cannot change them.

#### Scenario: First switch-on
- **WHEN** the user opens Settings › Browser access for the first time
- **THEN** the switch is off and disabled, the password fields are shown, and creating a password enables the switch

#### Scenario: In a browser
- **WHEN** Settings is opened in a browser
- **THEN** no Browser access section is shown

### Requirement: Browser access addresses and state
While Browser access runs, the section SHALL show the addresses to open (`http://<address>:<port>` for each home-network address of the computer, then `http://localhost:<port>`), each with a Copy button, and a QR code of the first. It SHALL show the port (1024 to 65535, 8787 by default) with an Apply button that restarts the server, and the state: "Running", the port-in-use sentence, or the error with a plain fix.

#### Scenario: Running
- **WHEN** Browser access is on and running on port 8787 on a computer at 192.168.1.20
- **THEN** the section shows "Running", `http://192.168.1.20:8787` with a Copy button and a QR code, and `http://localhost:8787`

#### Scenario: Copy an address
- **WHEN** the user presses Copy beside an address
- **THEN** the address is on the clipboard and the button says it was copied

### Requirement: Browser access sessions
The section SHALL list the signed-in browsers with their address, browser and last use, with a "Sign out" button for each and "Sign out all browsers".

#### Scenario: Sign out one browser
- **WHEN** two browsers are signed in and the user presses Sign out beside one of them
- **THEN** that browser's next request is answered as signed out and the other stays signed in

### Requirement: Browser access options and help
The section SHALL offer "Only devices on my home network" (on by default; turning it off warns that anyone who can reach the port can try the password), "Keep running in the tray when the window is closed" and "Start CineVault when I sign in". On Windows it SHALL say "Windows will ask to allow CineVault on private networks. Choose Allow." A short help text SHALL explain how to open an address from another device and that the connection is not encrypted.

#### Scenario: Firewall sentence
- **WHEN** the section is opened on Windows
- **THEN** it says Windows will ask to allow CineVault on private networks and to choose Allow

#### Scenario: Home network off
- **WHEN** the user turns "Only devices on my home network" off
- **THEN** the section warns that anyone who can reach the port can try the password

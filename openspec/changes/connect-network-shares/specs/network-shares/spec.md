## Purpose

Lets the user add library folders that live on an SMB network share needing its own user name and password: connecting the share, keeping the password in the operating system credential store, reconnecting at start, explaining why a share is offline, forgetting a share that is no longer used, and setting up shares for the server through its installer.

## ADDED Requirements

### Requirement: Connect a network share
In the desktop app, "Network share…" SHALL open the Network share page with the fields Address, User, Password and "Remember the password", which is on by default. Address SHALL take `\\server\share`, optionally followed by a folder inside the share, in the `\\nas\movies\Films` or `smb://nas/movies/Films` form, and the page SHALL show the library folder it gives. "Connect and add" SHALL connect the share with that login and add the folder as a library folder.

#### Scenario: Share added
- **WHEN** the user enters the Address `\\nas\movies\Films`, User `alex` and the right password, and presses "Connect and add"
- **THEN** the share is connected, the folder is listed in Settings › Library folders, and a scan of it starts

#### Scenario: Pasted address
- **WHEN** the user pastes `smb://nas/movies/Films` into Address
- **THEN** the page shows the library folder `\\nas\movies\Films` on Windows, and "Connect and add" connects the share `movies` on `nas`

### Requirement: Share folder added like any folder
A folder on a connected share SHALL be added with the same rules, type detection and messages as a folder chosen with "Add folder…". When the folder inside the share does not exist or cannot be read, nothing SHALL be added and the page SHALL say so.

#### Scenario: Folder not on the share
- **WHEN** the user asks for Folder `Films` on `\\nas\movies` and the share has no such folder
- **THEN** no library folder is added and the page says "Films was not found on \\nas\movies."

#### Scenario: Nested folder refused
- **WHEN** `\\nas\movies` is a movie folder and the user connects `\\nas\movies` with Folder `Action`
- **THEN** the folder is not added and the page shows the usual "already covered" message

### Requirement: Connection errors in plain words
When a share cannot be connected, the Network share page SHALL keep what was typed and say why next to the field concerned: a wrong user name or password, a server that cannot be reached, a share that does not exist on the server, or, on Windows, a connection to the same server under another user. Any other failure SHALL be shown with the system's own message.

#### Scenario: Wrong password
- **WHEN** the user connects with a wrong password
- **THEN** the Password field says "Wrong user name or password." and the other fields keep their values

#### Scenario: Server unreachable
- **WHEN** the server `nas` does not answer
- **THEN** the Address field says that `nas` could not be reached and to check the name and that it is switched on

### Requirement: Where a share folder lives
On Windows a share folder SHALL keep its network path (`\\nas\movies\Films`) and no drive letter SHALL be assigned. On macOS the share SHALL be mounted in a folder of CineVault's own mount folder, and on a Linux desktop in the desktop's own place for user-mounted shares; the library folder's path SHALL be the folder inside that mount.

#### Scenario: Windows path
- **WHEN** the user connects `\\nas\movies` with Folder `Films` on Windows
- **THEN** the library folder is `\\nas\movies\Films` and no new drive letter appears in File Explorer

### Requirement: Linux desktop without user-level share mounting
On a Linux desktop that offers no user-level share mounting, the Network share page SHALL say that this desktop cannot connect network shares for apps and SHALL explain how to mount the share with the system's own tools and then use "Add folder…". It SHALL NOT ask for a password there.

#### Scenario: No share mounting
- **WHEN** the user opens "Network share…" on a Linux desktop without user-level share mounting
- **THEN** the page explains how to mount the share with the system's tools and use "Add folder…", and shows no password field

### Requirement: Share passwords in the credential store
A remembered share password SHALL be stored only in the operating system credential store, under CineVault's service name. It SHALL NOT be written to the settings file, the library database or any other file. A password that is not remembered SHALL be kept in memory only, until CineVault quits.

#### Scenario: Remembered password
- **WHEN** the user connects `\\nas\movies` with "Remember the password" on
- **THEN** the password is in the credential store, and neither `settings.json` nor the library database contains it

#### Scenario: Not remembered
- **WHEN** the user connects a share with "Remember the password" off and restarts CineVault after the computer was restarted
- **THEN** the share's folders read offline with "Password needed" and "Connect…"

### Requirement: Remembering without a keyring
When the password cannot be stored because no credential store is reachable, the share SHALL NOT be connected silently without it: the page SHALL show the credential-store message and offer to connect without remembering the password.

#### Scenario: Linux without a keyring
- **WHEN** the user connects a share with "Remember the password" on and no Secret Service is running
- **THEN** the page says no system keyring is running and offers "Connect without remembering"; no file contains the password

### Requirement: Share passwords never shown or logged
A share password SHALL NOT appear in logs, error messages, the process list or any command line. It SHALL be masked in log output whether or not it is remembered. The Network share page SHALL show it only while the user chooses Show.

#### Scenario: Detailed log after a connect
- **WHEN** detailed logging is on and the user connects a share with the password `Example-Pass-42`
- **THEN** no log line contains `Example-Pass-42`

#### Scenario: Process list during a connect
- **WHEN** another program lists the running processes and their command lines while CineVault connects a share
- **THEN** no command line contains the password

### Requirement: Remembered shares reconnect at start
When the app starts, it SHALL connect every remembered share that is not already connected before the folder health check and before scanning that share's folders, so their titles are online. A share that does not connect within 15 seconds SHALL be treated as unreachable for that start, and other folders SHALL NOT wait for it.

#### Scenario: Restart with a remembered share
- **WHEN** the computer restarts and the user opens CineVault while `\\nas\movies` is reachable
- **THEN** the share is connected, its titles are online, and none of them showed as offline first

### Requirement: Offline share folders say why
A library folder on a share that cannot be connected SHALL read offline, never Missing, and its titles SHALL keep everything they have. Its row in Settings › Library folders SHALL add the reason ("Wrong password", "Server not reachable", "Share not found", "Password needed") and offer "Connect…".

#### Scenario: Password changed on the NAS
- **WHEN** the NAS password for `alex` was changed and CineVault starts
- **THEN** the folder row reads "offline · Wrong password · 120 titles kept" with "Connect…", and no title is marked Missing

### Requirement: Connect again
"Connect…" SHALL open the Network share page for that share with the Address fixed and User filled in, asking for the password. Connecting SHALL bring every library folder of that share online and scan them.

#### Scenario: Password entered again
- **WHEN** the user chooses "Connect…" on an offline share folder and enters the new password
- **THEN** every folder on that share is online again and a scan of them starts

### Requirement: Shares stay connected
A share SHALL stay connected when CineVault closes, so a player started from CineVault keeps working, until the user signs out, the computer restarts or the share is forgotten.

#### Scenario: Closing the app
- **WHEN** the user closes CineVault while a film from `\\nas\movies` plays in an external player
- **THEN** playback continues

### Requirement: Unused shares are forgotten
When the last library folder on a share is removed and its Undo has passed, CineVault SHALL disconnect the share and delete its stored password. The titles of the removed folders SHALL stay in the library as offline. A share whose files are still open SHALL be forgotten without being disconnected.

#### Scenario: Last folder removed
- **WHEN** the user removes `\\nas\movies\Films`, the only library folder on `\\nas\movies`, and does not choose Undo
- **THEN** the share is disconnected, its password is gone from the credential store, and the titles stay offline

#### Scenario: Undo
- **WHEN** the user removes the share's only folder and chooses Undo in the toast
- **THEN** the folder is back, online, and the share keeps its stored password

### Requirement: Network shares in a browser
In a browser, "Network share…" SHALL NOT ask for a password. It SHALL explain that network shares are connected on the server by the installer, because the server cannot connect them itself, and SHALL show the installer command with a Copy button and say to add the folder with "Add folder…" afterwards. The share commands SHALL answer `not_applicable`.

#### Scenario: Browser user asks for a share
- **WHEN** a signed-in browser user chooses "Network share…" in Settings › Library folders
- **THEN** the explanation and the installer command are shown, and no password field

### Requirement: Shares for the Linux service set up by the installer
When a library folder given to the installer for the Linux service is a share address (`//nas/movies`, `\\nas\movies` or `smb://nas/movies`), the installer SHALL ask for its user and password without showing the password, store them in a credentials file only root can read, mount the share at start-up for the service's account, and add the mounted folder to the server's folder roots.

#### Scenario: Service with a share
- **WHEN** the user gives the installer `//nas/movies/Films` with the user `alex` and a password
- **THEN** after the install the share is mounted, the service can write to the folder, the folder can be added in the browser, and the credentials file has mode `0600` and belongs to root

#### Scenario: Share unreachable after a restart
- **WHEN** the server restarts while the NAS is off
- **THEN** the service starts, the share's folders read offline, and they come online once the share is mounted again

### Requirement: Shares for containers set up by the installer
When a library folder given to the installer for a container is a share address, the installer SHALL ask for its user and password without showing the password, make the share available inside the container under `/media/<name>` writable by the container's user, and SHALL NOT pass the password on any command line.

#### Scenario: Docker Desktop with a share
- **WHEN** the user gives `install.ps1` the folder `\\nas\movies` with a user and password
- **THEN** the container sees the share at `/media/movies`, and a rename in it succeeds

### Requirement: Installer removes unused share setups
When the installer changes the library folders or removes CineVault, it SHALL remove the mount, the credentials and the container volume of every share that is no longer a library folder, and SHALL never delete anything on the share itself.

#### Scenario: Share dropped from the folders
- **WHEN** the user runs the installer again and drops `//nas/movies` from the folders
- **THEN** its mount and credentials file are gone, and the files on the NAS are untouched

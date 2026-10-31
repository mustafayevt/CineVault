# settings Specification

## Purpose
Defines the Settings page at `/settings` and how settings are stored. The page is one scrolling page of sections (Providers, Naming, Library folders, Deleted files, Torrent sources, Appearance, Updates) with an index at its left; every change applies at once. Settings are kept in one `settings.json` file in the data folder, written atomically and never reset silently; provider keys live in the operating system's credential store. The behaviour each setting controls is defined by the capability that owns it.

## Requirements

### Requirement: One scrolling Settings page
Settings SHALL be one page at `/settings`, opened from the Settings button in the sidebar, with the sections Providers, Naming, Library folders, Deleted files, Torrent sources, Appearance and Updates in that order, followed by a Server section when the page is served by the server. An index at the page's left SHALL list them; choosing an entry SHALL scroll to its section, and the entry being read SHALL show a 2px gold indicator that follows the scroll. `/settings?section=<id>` (such as `naming`, `folders` or `server`) SHALL open the page at that section.

#### Scenario: Jump to Naming
- **WHEN** the user chooses Naming in the index
- **THEN** the page scrolls to the Naming section and Naming gets the gold indicator

#### Scenario: Link from the Rename page
- **WHEN** the user follows the Rename page's link to the naming settings
- **THEN** Settings opens at the Naming section

#### Scenario: Browser Settings
- **WHEN** Settings is opened in a browser served by CineVault
- **THEN** the index ends with Server, and `/settings?section=server` opens at it

#### Scenario: Desktop Settings
- **WHEN** Settings is opened in the desktop app
- **THEN** the index has no Server entry and `/settings?section=server` opens the page at the top

### Requirement: Changes apply at once
Every change on the Settings page SHALL apply and be saved at once, with no Save button and no restart. Saves SHALL run one after another so the file always ends with the last change. When a save fails, the change SHALL be undone in the page so it shows what is stored.

#### Scenario: Theme change
- **WHEN** the user picks Light in Settings › Appearance
- **THEN** the window turns light at once and the choice is in `settings.json`

#### Scenario: Save fails
- **WHEN** the settings file cannot be written while the user changes a setting
- **THEN** the control goes back to the stored value

### Requirement: Providers section
The Providers section SHALL hold the TMDB key ("Needed for Scrape."), the OMDb key (IMDb, Rotten Tomatoes and Metacritic ratings), the MDBList key (more ratings) and the optional Fanart.tv and TheTVDB keys, each with a "Get a free key" link, its state ("Works" or "Rejected"), Show or Hide, Test and Remove, plus the metadata language and the ratings country. Keys SHALL be stored in the operating system's credential store, never in a file. How keys and providers are used is defined by `metadata-providers` and `ratings`.

#### Scenario: Testing a key
- **WHEN** the user pastes a TMDB key and presses Test
- **THEN** the key is checked against TMDB and its state reads "Works" or "Rejected" with the reason

### Requirement: Naming section
The Naming section SHALL hold the Movie folder, Movie file and TV episode templates as mono text fields, each with its tokens and a live example of the result, and the "Colon in titles" choice (Remove, Use " - ", or a look-alike colon). These templates are the ones Rename uses, as defined by the `renamer` capability; changing them SHALL rename nothing by itself.

#### Scenario: Live example
- **WHEN** the user edits the Movie folder template
- **THEN** the example under the field shows the new folder name, and no folder on disk changes

### Requirement: Library folders section
The Library folders section SHALL list every library folder with its full path, how many titles or shows it holds and whether it is watched for changes or offline, a Movies | TV shows switch and Remove, and SHALL offer "Add folder…" and accept folders dropped on the window. Removing a folder SHALL keep its titles in the library as offline and confirm with an Undo toast. Scanning these folders is defined by `library-scanning`.

#### Scenario: Remove a folder
- **WHEN** the user removes `D:\Movies`, which holds 120 titles
- **THEN** a toast says "Removed D:\Movies · 120 titles stay, offline" with Undo

#### Scenario: Add a folder
- **WHEN** the user chooses "Add folder…" and picks `E:\Movies`
- **THEN** the folder is listed and a toast says "Added 1 folder"

### Requirement: Deleted files section
The Deleted files section SHALL hold where deleted and replaced files go ("Recycle Bin", the default; "Trash folder on each drive"; "Custom trash folder…"; "Delete permanently"), the custom trash folder with "Choose folder…", and "Empty trash folders automatically" (Off, the default, or after 7, 30 or 90 days). Choosing "Delete permanently" SHALL first ask "Erase deleted files at once?" and change nothing unless confirmed. The behaviour of each choice is defined by `title-deletion`.

#### Scenario: Erase needs confirmation
- **WHEN** the user picks "Delete permanently"
- **THEN** the confirmation asks "Erase deleted files at once?", and the choice is kept only after "Delete permanently" is pressed

#### Scenario: Custom folder not chosen
- **WHEN** "Custom trash folder…" is selected and no folder is chosen yet
- **THEN** the section says files go to each drive's trash folder until a folder is chosen

### Requirement: Torrent sources section
The Torrent sources section SHALL list the sites Torrent search asks, each with an on/off switch (on unless switched off) and its health, and the cards for the indexer service, an own Jackett and Torznab addresses. How sources are searched is defined by `torrent-sources` and `torrent-search`.

#### Scenario: Switch a site off
- **WHEN** the user switches off one built-in site
- **THEN** its tag shows it as off and Torrent search stops asking it

### Requirement: Appearance section
The Appearance section SHALL hold Theme (Light, Dark, and "Match Windows" or "Match macOS"; Dark by default) and Size, the app zoom (80% to 150%, 100% by default) with its keyboard hint. Both SHALL apply at once, as defined by `app-shell`.

#### Scenario: Size
- **WHEN** the user picks 125% under Size
- **THEN** the whole app draws at 125% at once

### Requirement: Updates section
The Updates section SHALL show "CineVault <version>", the result of the last check ("Up to date · checked today at 09:12", or "Not checked yet"), "Check now", the download and install state when an update is on its way, and the switch "Install updates when I close the app". How updates are found and installed is defined by `release-and-updates`.

#### Scenario: Version and last check
- **WHEN** the user opens Settings › Updates after a check this morning found nothing new
- **THEN** it reads "CineVault <version>" with "Up to date · checked today at" and the time

### Requirement: Settings file
Settings SHALL be kept in `settings.json` in the data folder as one map of dotted keys to JSON values inside a versioned envelope. Only values that differ from their defaults SHALL be stored: setting a default or resetting removes the key. A key SHALL use only lowercase letters, digits, dots and hyphens, start with a letter, have no `..` and be at most 96 characters; a value SHALL be at most 64 KiB. A write that breaks these rules SHALL be refused and leave the stored settings unchanged.

#### Scenario: Default not stored
- **WHEN** the user sets the theme back to Dark
- **THEN** `appearance.theme` is removed from `settings.json`

#### Scenario: Malformed key
- **WHEN** a write names the key `Library..Folders`
- **THEN** it is refused and nothing is saved

### Requirement: Artwork file names cannot be emptied
A settings write that would leave an artwork type without a file name SHALL be refused with "Every artwork type is written; keep at least one name", and the stored settings SHALL stay unchanged.

#### Scenario: Emptying the poster names
- **WHEN** a write sets the movie poster file names to an empty list
- **THEN** it is refused with that message and the poster names stay as they were

### Requirement: Atomic writes
Every settings save SHALL write the whole file to a temporary file in the same folder, flush it to disk and rename it over `settings.json`, so a crash leaves either the old or the new file, never a half-written one. A change SHALL be saved before it is kept in memory, so memory and file never disagree.

#### Scenario: Crash during a save
- **WHEN** the app stops in the middle of saving a setting
- **THEN** `settings.json` holds either the settings before or after that change, complete

### Requirement: Settings never reset silently
A settings file that cannot be used (invalid JSON, the wrong shape, or written by a newer version) SHALL never be overwritten: it SHALL be renamed to `settings.json.corrupt-<UTC timestamp>`, the app SHALL start with defaults, and the first settings the UI reads SHALL carry a one-time notice that the file was set aside. Start-up steps that would save settings SHALL be skipped in a session that runs on such defaults.

#### Scenario: Damaged file
- **WHEN** `settings.json` holds invalid JSON at start
- **THEN** it is renamed to `settings.json.corrupt-<timestamp>`, the app opens with defaults, and the settings carry a notice that the file was set aside

### Requirement: Unreadable settings file kept
A settings file that exists but cannot be read (held by another program, no permission) SHALL be read again a few times with short pauses. When it still cannot be read, the app SHALL start with defaults and leave the file alone, and before the first save of the session it SHALL rename the file to `settings.json.unreadable-<UTC timestamp>`; when the file cannot be set aside, the save SHALL fail instead of replacing it.

#### Scenario: File held by another program
- **WHEN** `settings.json` cannot be read at start and the user then changes a setting
- **THEN** the old file is renamed to `settings.json.unreadable-<timestamp>` before the new file is written

### Requirement: Settings outside the page
Some settings SHALL have no control on the Settings page and follow the same storage rules: the motion preference (`appearance.motion`, System by default), the regional format (`general.locale`, the system's by default), the file size units (`advanced.size-units`), and state other features keep, such as the sidebar collapse choice (`shell.sidebar`) and whether the first-run setup was finished (`onboarding.completed`).

#### Scenario: Sidebar choice is a setting
- **WHEN** the user collapses the sidebar
- **THEN** `shell.sidebar` is saved in `settings.json`

### Requirement: Settings in a browser
When Settings is served to a browser, the controls that only make sense on the desktop SHALL be hidden or shown as not applicable with a sentence: the video player choice and custom player command, app zoom and window size, "Match Windows"/"Match macOS", install-on-close, the update check and "Check now", the Recycle Bin/Trash destination and custom trash folder when the server's deleted-files destination is `.deletedByCineVault`, and the indexer manager and own-Jackett addresses that point at the client's loopback. The Server section SHALL show the address and port (read-only), the running version, the session lifetime, "Change password", "Sign out every session", "Allow permanent delete", the Kodi connection and the trusted-proxy state. File-operation labels SHALL follow the server's operating system, while text size, date format and theme SHALL follow the client's settings.

#### Scenario: Browser providers
- **WHEN** the user opens Settings in a browser
- **THEN** the Providers, Naming, Library folders, Deleted files, Appearance and Updates sections work, and the player choice is not shown

#### Scenario: Server labels
- **WHEN** a Windows browser is used against a Linux server and the user deletes a title
- **THEN** the destination is described with the server's Linux name and the configured server folder

#### Scenario: Client appearance
- **WHEN** the client's browser prefers a light theme
- **THEN** the interface follows the client's theme choice, not the server's

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

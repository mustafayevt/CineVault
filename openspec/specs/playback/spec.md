# playback Specification

## Purpose
How CineVault plays a title: the Play action on movies and episodes, which starts the external video player chosen in Settings (the system default, mpv, VLC, MPC-HC / MPC-BE, IINA or a custom command) with the title's files in order, and says plainly when it cannot.

## Requirements

### Requirement: Play action
Play SHALL be offered on the Movie page, on the TV show page (the first numbered episode on disk not marked watched, else the first on disk), on an opened episode and in the Library's right-click menu for one movie. Only movies and episodes SHALL play; in the Library, Play on a show SHALL be off with "Open the show to play an episode". Play SHALL be off with the reason when the files are missing or offline. CineVault SHALL NOT play video itself.

#### Scenario: Play a movie
- **WHEN** the user presses Play on a movie whose file is on disk
- **THEN** the configured player starts with that file

#### Scenario: Play from the show page
- **WHEN** a show has S01E01 to S01E03 on disk and S01E01 is marked watched
- **THEN** Play on the show page starts S01E02

### Requirement: Player choice
Settings SHALL offer "Video player" (`player.kind`) with "System default" (`system`, the default), "mpv", "VLC", "MPC-HC / MPC-BE", "IINA" and "Custom command", "Player program" (`player.path`) for a player installed in an unusual place, and "Custom player command" (`player.custom-command`). A missing or unknown setting SHALL fall back to the system default, so a hand-edited settings file never stops playback.

#### Scenario: Unknown value
- **WHEN** the settings file names a player kind CineVault does not know
- **THEN** Play uses the system default player

### Requirement: Finding the player
When "Player program" is empty, CineVault SHALL look for the chosen player in its usual install places (on Windows under the Program Files folders, on macOS in `/Applications`, on Linux in `/usr/bin`, `/usr/local/bin`, `/snap/bin` and the system and per-user Flatpak export folders) and then on the `PATH`. A configured macOS application bundle (`VLC.app`) SHALL stand for the program inside it. When the player cannot be found, Play SHALL say "The player <name> was not found. Check its location in the settings."

#### Scenario: VLC in its usual place
- **WHEN** the player is VLC, "Player program" is empty and VLC is installed in its default folder on Windows
- **THEN** Play starts that VLC with the file

#### Scenario: mpv from the distribution on Linux
- **WHEN** the player is mpv, "Player program" is empty and mpv is installed as `/usr/bin/mpv` on Linux
- **THEN** Play starts `/usr/bin/mpv` with the file

### Requirement: Parts and discs in order
Mpv, VLC, MPC-HC / MPC-BE and IINA SHALL receive every part of a stacked movie on their command line in part order, so the player queues them as one playlist. A title kept as a disc folder (`VIDEO_TS`, `BDMV`) SHALL be passed as the disc folder: mpv SHALL open a DVD folder with `--dvd-device=<folder> dvd://` and a Blu-ray folder with `--bluray-device=<folder> bd://`, and MPC-HC / MPC-BE SHALL get `/dvd` for a DVD folder. A disc image SHALL be passed as a plain file.

#### Scenario: Play a stacked movie with mpv
- **WHEN** the player is mpv and the user presses Play on a movie stored as `Example Movie CD1.avi` and `Example Movie CD2.avi`
- **THEN** mpv starts with both parts queued in order

### Requirement: System default player
The system default player SHALL open the first file through the operating system, one file at a time. For a stacked title it SHALL show "Opened the first of N parts of <title>. The system player opens one file at a time; choose another video player in Settings to queue every part." For a disc folder it SHALL open the disc's main index file when there is one, and otherwise say "The system default player cannot open a disc folder. Choose mpv, VLC, MPC-HC or IINA in the settings."

#### Scenario: Stacked movie with the system player
- **WHEN** the player is the system default and the movie has two parts
- **THEN** the first part opens and a toast says only the first of 2 parts was opened

### Requirement: Custom command
A custom command SHALL be a template whose first word is the program and whose other words are its arguments, split on spaces, with double or single quotes grouping words that contain spaces. `{file}` SHALL stand for the first file (the disc folder for a disc), `{files}` for every file in play order as separate arguments and `{folder}` for the folder that holds the first file. A command with no placeholder SHALL get the files added at the end.

#### Scenario: No placeholder
- **WHEN** the custom command is `player.exe --fullscreen`
- **THEN** the title's files are added after `--fullscreen`

#### Scenario: Placeholders
- **WHEN** the custom command is `player.exe --fullscreen {files}` and a movie has two parts
- **THEN** `player.exe` starts with `--fullscreen` and the two part paths as separate arguments

### Requirement: Safe player launch
An empty custom command, an unclosed quote, or `{files}` inside a longer word SHALL be refused with a plain message. No shell SHALL be involved in starting any player: the program and every argument are handed to the operating system separately, so a file name is never read as a command.

#### Scenario: Bad command
- **WHEN** the custom command is `player.exe "--title=x`
- **THEN** Play says "The custom player command has a quote that is never closed."

### Requirement: Failures are said plainly
When a title cannot be played, CineVault SHALL show a toast "Could not play <title>. <reason>" and keep the same message in the notification centre. Reasons SHALL include "There is no video file to play.", "<file> is not on disk.", a player that was not found and a player that could not be started.

#### Scenario: File moved away
- **WHEN** a movie's video file was deleted outside CineVault and the user presses Play
- **THEN** a toast says "Could not play Example Movie" with the file name and "is not on disk."

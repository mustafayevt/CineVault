# Spec Delta

## MODIFIED Requirements

### Requirement: Play action
In the desktop app, Play SHALL be offered on the Movie page, on the TV show page (the first numbered episode on disk not marked watched, else the first on disk), on an opened episode and in the Library's right-click menu for one movie. Only movies and episodes SHALL play; in the Library, Play on a show SHALL be off with "Open the show to play an episode". Play SHALL be off with the reason when the files are missing or offline. CineVault SHALL NOT decode video itself. In a browser, Play SHALL follow "Play in a browser" and SHALL use the browser's player, a stream link or Kodi instead of starting a player on the server.

#### Scenario: Play a movie
- **WHEN** the user presses Play on a movie whose file is on disk in the desktop app
- **THEN** the configured player starts with that file

#### Scenario: Play from the show page
- **WHEN** a show has S01E01 to S01E03 on disk and S01E01 is marked watched
- **THEN** Play on the show page starts S01E02

#### Scenario: Browser Play
- **WHEN** the user presses Play on a movie in a browser
- **THEN** the browser's own player, the stream link or Kodi is used, and no player starts on the server

### Requirement: Player choice
In the desktop app, Settings SHALL offer "Video player" (`player.kind`) with "System default" (`system`, the default), "mpv", "VLC", "MPC-HC / MPC-BE", "IINA" and "Custom command", "Player program" (`player.path`) for a player installed in an unusual place, and "Custom player command" (`player.custom-command`). A missing or unknown setting SHALL fall back to the system default, so a hand-edited settings file never stops playback. In a browser the external-player choice SHALL NOT be shown; "Play in browser", "Copy stream link" and "Open in Kodi" replace it.

#### Scenario: Unknown value
- **WHEN** the settings file names a player kind CineVault does not know
- **THEN** Play uses the system default player

#### Scenario: Browser Settings
- **WHEN** Settings is opened in a browser
- **THEN** no video-player choice is shown

## ADDED Requirements

### Requirement: Play in a browser
When the interface is served to a browser, Play SHALL offer "Play in browser", "Copy stream link" and, when Kodi is configured, "Open in Kodi". The browser player SHALL stream the file through a route that answers range requests so seeking works, SHALL send a video content type from an allowlist and `application/octet-stream` for anything else, and SHALL refuse a file outside the library folders. A stacked title SHALL be offered as its parts in order. A disc folder SHALL open its main index file when it is readable, else Play SHALL be off with a sentence. When the browser cannot decode the file, the page SHALL say so and offer the stream link and a download instead of a failing player. The download SHALL always be an attachment. No player SHALL be started on the server.

#### Scenario: Play a movie
- **WHEN** the user presses Play in a browser on a file the browser can decode
- **THEN** the browser's player starts, seeking works, and no process starts on the server

#### Scenario: A format the browser cannot play
- **WHEN** the file is a DTS-in-MKV the browser cannot decode
- **THEN** the page says the browser cannot play it and offers the stream link and the download

#### Scenario: Stacked movie
- **WHEN** a movie has two parts
- **THEN** the browser receives both parts in order and plays them one after the other

#### Scenario: Disc folder
- **WHEN** a title is a `VIDEO_TS` or `BDMV` folder
- **THEN** Play opens the main index file when it is readable, else it is off with a sentence

#### Scenario: File moved away
- **WHEN** a movie's video file was deleted outside CineVault and the user presses Play in a browser
- **THEN** Play is off and says the file is not on disk

#### Scenario: Outside the library
- **WHEN** a media address names a file outside every library folder
- **THEN** the server answers 404 and sends no bytes

#### Scenario: Open in Kodi
- **WHEN** Kodi is configured and the user chooses "Open in Kodi"
- **THEN** Kodi opens the file at the configured path mapping, and a failure says Kodi is unreachable

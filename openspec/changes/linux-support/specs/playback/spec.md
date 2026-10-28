## MODIFIED Requirements

### Requirement: Finding the player
When "Player program" is empty, CineVault SHALL look for the chosen player in its usual install places (on Windows under the Program Files folders, on macOS in `/Applications`, on Linux in `/usr/bin`, `/usr/local/bin`, `/snap/bin` and the system and per-user Flatpak export folders) and then on the `PATH`. A configured macOS application bundle (`VLC.app`) SHALL stand for the program inside it. When the player cannot be found, Play SHALL say "The player <name> was not found. Check its location in the settings."

#### Scenario: VLC in its usual place
- **WHEN** the player is VLC, "Player program" is empty and VLC is installed in its default folder on Windows
- **THEN** Play starts that VLC with the file

#### Scenario: mpv from the distribution on Linux
- **WHEN** the player is mpv, "Player program" is empty and mpv is installed as `/usr/bin/mpv` on Linux
- **THEN** Play starts `/usr/bin/mpv` with the file

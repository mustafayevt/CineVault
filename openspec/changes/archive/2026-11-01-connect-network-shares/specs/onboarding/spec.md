## MODIFIED Requirements

### Requirement: Folders step
The Folders step SHALL list each added folder with its full path, what was found ("146 videos found", or "found so far" when the look stopped early) and a Movies | TV shows switch set from the detected type, with Remove, and SHALL offer "Add folder…" and "Network share…". In the desktop app it SHALL accept folders dropped on the window; in a browser a dropped folder SHALL NOT be accepted, because a browser provides no absolute path, and the page SHALL say "Use Add folder…". A folder that cannot be read SHALL say so on its row. A folder that cannot be written SHALL say "This folder is not writable by the server." Two folders of the same type where one lies inside the other SHALL be flagged, and Continue SHALL stay disabled until one is removed.

#### Scenario: Detected type
- **WHEN** the user adds `E:\TV`, whose videos are named like `S01E01`
- **THEN** its row reads "18 videos found · looks like TV shows" with TV shows chosen

#### Scenario: Nested folders of one type
- **WHEN** the user adds `D:\Movies` and `D:\Movies\Action` as movie folders
- **THEN** the inner one says it is inside `D:\Movies`, which is in the list already, and Continue says "A folder lies inside another one of the same type. Remove one of them."

#### Scenario: Read-only folder in a browser
- **WHEN** a browser adds a folder the server cannot write
- **THEN** the row says "This folder is not writable by the server." and Continue stays disabled

#### Scenario: Network share during setup
- **WHEN** the user chooses "Network share…" in the Folders step and connects `\\nas\movies` with Folder `TV`
- **THEN** the step lists `\\nas\movies\TV` with what was found and the detected type, as for a folder from "Add folder…"

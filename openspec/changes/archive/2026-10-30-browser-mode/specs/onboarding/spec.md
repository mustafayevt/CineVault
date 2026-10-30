# Spec Delta

## MODIFIED Requirements

### Requirement: Four-step setup frame
In the desktop app, the setup SHALL show four numbered steps, Folders, TMDB key, Ratings keys and Naming, on the window ground with its own title row (the brand mark, a drag region and, on Windows, the window buttons). In a browser the four steps SHALL show without a window title row, drag region or window buttons, under the browser shell's own header. Every step SHALL offer "Skip setup", "Skip this step" and Continue, and the footer SHALL say "Every step can be changed later in Settings." Moving to another step SHALL put focus on its heading. A disabled Continue SHALL explain why and what enables it.

#### Scenario: Continue disabled on the folders step
- **WHEN** the user is on the Folders step with no folder added
- **THEN** Continue is disabled and says "Add a folder first." with "Add a folder, or skip this step."

#### Scenario: Browser frame
- **WHEN** the setup runs in a browser
- **THEN** no drag region or window buttons are shown, and the four steps and their footer are the same

### Requirement: Folders step
The Folders step SHALL list each added folder with its full path, what was found ("146 videos found", or "found so far" when the look stopped early) and a Movies | TV shows switch set from the detected type, with Remove, and SHALL offer "Add folder…". In the desktop app it SHALL accept folders dropped on the window; in a browser a dropped folder SHALL NOT be accepted, because a browser provides no absolute path, and the page SHALL say "Use Add folder…". A folder that cannot be read SHALL say so on its row. A folder that cannot be written SHALL say "This folder is not writable by the server." Two folders of the same type where one lies inside the other SHALL be flagged, and Continue SHALL stay disabled until one is removed.

#### Scenario: Detected type
- **WHEN** the user adds `E:\TV`, whose videos are named like `S01E01`
- **THEN** its row reads "18 videos found · looks like TV shows" with TV shows chosen

#### Scenario: Nested folders of one type
- **WHEN** the user adds `D:\Movies` and `D:\Movies\Action` as movie folders
- **THEN** the inner one says it is inside `D:\Movies`, which is in the list already, and Continue says "A folder lies inside another one of the same type. Remove one of them."

#### Scenario: Read-only folder in a browser
- **WHEN** a browser adds a folder the server cannot write
- **THEN** the row says "This folder is not writable by the server." and Continue stays disabled

## ADDED Requirements

### Requirement: First-run setup in a browser
When the interface is served to a browser and the library has no folders and `onboarding.completed` is not true, the welcome password page SHALL run first, then the four-step setup. "Add folder…" SHALL open the server folder browser instead of the native picker. Folder analysis, type detection, the key step, the naming step and the tools download SHALL run on the server, and keys SHALL be saved in the server's secret store. Adding a folder SHALL write and remove a probe file and refuse a folder the server cannot write. Finishing or skipping SHALL set `onboarding.completed` and open the Library.

#### Scenario: Fresh server
- **WHEN** a browser opens a server with an empty data folder
- **THEN** the welcome password page shows first, then the four-step setup with the server folder browser

#### Scenario: Folder added from the browser
- **WHEN** the user adds `/media/movies` and the analysis finds 146 videos
- **THEN** the row reads "146 videos found" and the folder type switch is set from the detection

#### Scenario: Finish
- **WHEN** the user finishes the setup in a browser
- **THEN** the Library opens with the scanned titles and the setup does not show again

# onboarding Specification

## Purpose
Defines the first-run setup at `/welcome`: a full-window flow of four steps (Folders, TMDB key, Ratings keys, Naming) that takes a new user from installation to a scanned library with smart defaults, automatic folder-type detection and no change on disk. It also defines when the setup shows, what finishing or skipping it does, and the library that appears from local NFO and artwork files before any online lookup.

## Requirements

### Requirement: When the setup shows
The first-run setup SHALL replace the whole window, instead of the app shell, when the library has no folders and the `onboarding.completed` setting is not true. Finishing or skipping the setup SHALL set `onboarding.completed`, so it does not show again. The `/welcome` address SHALL show the setup at any time. When the library cannot be read to decide, the app SHALL open normally rather than lock the user out.

#### Scenario: New install
- **WHEN** CineVault starts for the first time with no library folders
- **THEN** the setup fills the window on its first step

#### Scenario: Setup left before
- **WHEN** the user skipped the setup earlier and still has no folders
- **THEN** the app opens on the Library with a way to add a folder

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

### Requirement: Folder type detection
Each added folder SHALL be analysed by walking it up to 6 levels deep and at most 30,000 entries, honouring the scanner's skip rules. A video SHALL count as a TV episode when its name carries an episode marker (such as `S01E02` or `1x02`), when it sits in a season folder (such as `Season 2` or `Specials`), or when it belongs to a numbered series in one folder. The user's choice on the folder's switch SHALL win over the detected type.

#### Scenario: Season folders
- **WHEN** a folder holds `Example Show/Season 1/Episode 01.mkv` and similar files without episode markers in their names
- **THEN** those videos count as episodes

### Requirement: Suggested folder type
A folder SHALL be suggested as TV shows when at least 85% of its videos are episodes and as Movies when fewer than 15% are. Between the two it SHALL say "holds movies and shows, choose one" and start as Movies. A folder with no videos SHALL be suggested from its name.

#### Scenario: Mixed folder
- **WHEN** a folder holds 40 movies and 60 episodes
- **THEN** its row says it holds movies and shows, choose one, with Movies chosen

### Requirement: TMDB key step
The TMDB key step SHALL explain that titles, posters and plots come from TMDB and that the free key stays on this computer, list how to get one, and offer the key field with Test. Continue SHALL stay disabled ("Test your TMDB key first.") until the key passes Test; "Skip this step" SHALL move on without a key. A key that passes Test SHALL be saved to the operating system's credential store.

#### Scenario: Key tested
- **WHEN** the user pastes a TMDB key and Test passes
- **THEN** the key shows as working and Continue is enabled

#### Scenario: TMDB key skipped
- **WHEN** the user skips the TMDB key step and finishes the setup
- **THEN** the Scrape page later says "Add your TMDB key to start scraping"

### Requirement: Ratings keys step
The Ratings keys step ("Add your optional keys") SHALL offer the optional OMDb key ("Adds IMDb, Rotten Tomatoes and Metacritic ratings"), MDBList key, Fanart.tv key ("Adds more artwork") and TheTVDB key ("Adds TV artwork and episode data"), each marked Optional with Test, and SHALL say all of them can be added later in Settings.

#### Scenario: Ratings keys skipped
- **WHEN** the user presses "Skip this step" on the Ratings keys step
- **THEN** the Naming step opens and no ratings key is stored

### Requirement: Naming step
The Naming step SHALL offer the built-in naming styles, each with a one-line description and the folder and file name it gives, and an example that shows the chosen style on the first movie the folder analysis found ("Your file"), or on a built-in sample labelled "Built-in sample" when none was found. It SHALL say "Nothing is renamed until you ask on the Rename page." The chosen style SHALL become the active naming style only when the setup is finished with Continue.

#### Scenario: Example from the user's own file
- **WHEN** the folder analysis found `Example.Movie.2020.1080p.BluRay.mkv` and the user picks a style
- **THEN** the example, marked "Your file", shows that file's current name and its new folder and file name

#### Scenario: Example without files
- **WHEN** no video was found before the Naming step
- **THEN** the example uses the built-in sample and is marked "Built-in sample"

### Requirement: Finishing and skipping
Nothing SHALL be saved by the setup until it ends, except a key that passed Test. Continue on the last step SHALL add the folders, make the chosen naming style active, start a scan of the added folders and open the Library on Movies (on TV shows when only TV folders were added). "Skip this step" on the last step SHALL do the same without changing the style, and "Skip setup" SHALL leave at once, adding no folder and changing no style.

#### Scenario: Minimal setup
- **WHEN** a new user adds one movie folder, enters a TMDB key that passes Test, skips the ratings keys, keeps the suggested style and presses Continue on the last step
- **THEN** the Library opens on Movies and the scan of that folder starts

#### Scenario: Skip setup
- **WHEN** the user presses "Skip setup" on the first step
- **THEN** the Library opens empty with "Add a folder", and every setup choice is in Settings

### Requirement: Refused folder returns to the first step
When the library refuses a folder as the setup finishes, the setup SHALL return to the Folders step with the reason on that folder's row ("This folder could not be added." and why), and SHALL NOT open the Library.

#### Scenario: Folder refused
- **WHEN** a folder cannot be added as the user finishes the setup
- **THEN** the Folders step shows again with the reason on its row

### Requirement: Tools download after setup
When the setup ends in any way, the app SHALL download MediaInfo and FFmpeg in the background when they are not installed, one after the other, and SHALL tell the scan they are on their way so it waits for them instead of reporting them missing. A failed download SHALL show a toast ("MediaInfo could not be downloaded.") and SHALL stop nothing else.

#### Scenario: Tools missing
- **WHEN** the setup finishes on a machine without MediaInfo
- **THEN** the progress card shows the download, and video details are read once it is done

### Requirement: Library from local files
After the setup, titles SHALL appear in the Library as soon as the first scan stores them, with their local posters, and a "Reading NFO files" job SHALL then read the title, year, IDs and plot from each new title's existing NFO, honouring field locks, with no provider key and no network. Only NFO files that parse with a title SHALL be read. The import SHALL only read: no NFO or image file SHALL be created, changed or moved.

#### Scenario: Posters before scraping
- **WHEN** the first scan finds movies with local `poster.jpg` and NFO files carrying TMDB IDs
- **THEN** they appear with their posters, titles and years, at the Scraped stage, before any online lookup

### Requirement: Stages from local files
After the first scan, a title whose NFO carries a provider ID SHALL be Scraped, or Renamed when its folder and file names already match the naming style; every other title SHALL be New.

#### Scenario: Names already match
- **WHEN** a movie's NFO has a TMDB ID and its folder and file already follow the naming style
- **THEN** the title is at the Renamed stage after the first scan

### Requirement: Nothing changes on disk
The first run SHALL NOT scrape, rename, move or delete anything. After the first scan, the sidebar's Scrape and Rename pills SHALL show what waits, and nothing SHALL be scraped or renamed until the user asks.

#### Scenario: After the first scan
- **WHEN** the first scan found 40 new titles and 12 scraped ones whose folders differ from the naming style
- **THEN** the Scrape pill shows 40, the Rename pill shows 12, and no file in the media folders has changed

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

## MODIFIED Requirements

### Requirement: Scans are resumable jobs
A scan SHALL run as a persistent job on the disk lane, one library folder (or, for Scan again, one title's folder) per job item. A scan of every library folder SHALL be named "Scanning N folders", a scan of one library folder "Scanning <folder label>" and a scan of one title "Scanning <title (year)>". After a quit or crash, the next start SHALL resume the folders it had not finished instead of starting a new scan. After a scan, the app SHALL read the pending media info and prepare the thumbnails of new and changed titles in the background.

#### Scenario: Quit during a scan
- **WHEN** the user quits while a scan of 4 folders has finished 2
- **THEN** the next start scans the other 2 folders and does not start a full scan of its own

#### Scenario: Name of a folder scan
- **WHEN** the user scans the library folder labelled "Movies 4K" on its own
- **THEN** the job is named "Scanning Movies 4K"

## ADDED Requirements

### Requirement: Scan one library folder
Each library folder on the Drives page and in Settings › Library folders SHALL offer "Scan this folder", which scans that library folder only, with the same rules as a scan of every folder, and confirms with the toast "Scanning <folder label>". Titles in other library folders SHALL NOT be read or changed. "Scan folders now" SHALL stay and scan every library folder.

#### Scenario: One folder on a network share
- **WHEN** eight library folders are on a network share and the user chooses "Scan this folder" for `\\nas\media\Movies 4K`
- **THEN** only that folder is read, its new titles appear, and the titles of the other seven folders are not touched

### Requirement: Scan one title again
The Movie page and the TV show page SHALL offer "Scan again", which reads only that title's own folder: its files, its NFO and artwork files, and the media info of new or changed videos. It SHALL notice that the title's files are gone (the title becomes Missing) or back (it becomes available again).

#### Scenario: Files back
- **WHEN** a movie shows "The files for this title are missing", the user puts its folder back and chooses "Scan again"
- **THEN** the movie is available again and keeps its metadata

#### Scenario: New subtitle
- **WHEN** the user adds `Example Movie (2020).en.srt` to a movie's folder and chooses "Scan again" on that movie
- **THEN** the movie lists the subtitle

### Requirement: Scan again leaves other titles alone
Scan again SHALL NOT read, compare, add or mark missing any other title, in the same library folder or another. A movie that sits directly in its library folder SHALL scan that library folder instead. A title whose library folder was removed from the library SHALL say that it cannot be scanned on its own and offer nothing to scan.

#### Scenario: Other titles untouched
- **WHEN** the folders of two other movies in the same library folder were deleted and the user chooses "Scan again" on a third
- **THEN** only the third is read, and the other two do not become Missing until their folder or library is scanned

#### Scenario: A new folder next to it
- **WHEN** a new movie folder was copied next to a movie and the user chooses "Scan again" on that movie
- **THEN** the new folder is not added by this scan, and the next scan of the library folder adds it

### Requirement: Scan progress in titles
A scan SHALL report its progress in titles, not library folders. First it SHALL count the titles of the folders it will read, showing "Finding titles… N found" with the folder being listed; this phase SHALL end within 15 seconds, and a folder that has not answered by then SHALL be counted while it is read. Then it SHALL show "N of M titles" with the title being read. The total SHALL never drop while the scan runs, and a finished scan SHALL show every title done.

#### Scenario: Eight folders on a share
- **WHEN** a scan of eight library folders holding 707 titles has read 312 of them
- **THEN** the progress reads "Scanning 312 of 707 titles" with the title being read, at 44%

#### Scenario: Counting first
- **WHEN** the scan has just started and has listed two of the eight folders
- **THEN** the progress reads "Finding titles… 312 found" and names the folder being listed

### Requirement: What a scan counts as a title
For its progress, a scan SHALL count as one title each folder directly in a library folder and, in a movie library, each video directly in it. A folder that groups titles (a letter, year, decade or genre folder of movies, a one-character folder of shows) SHALL count its own folders (and, in a movie library, its videos) instead. Folders the scan skips SHALL NOT count.

#### Scenario: A to Z folders
- **WHEN** a TV library folder holds the folders `A` with 12 shows and `B` with 9
- **THEN** the scan counts 21 titles

### Requirement: Time left of a scan
Once at least 10 titles were read over at least 5 seconds, the progress SHALL add the time left at the pace so far ("about 3 min left", "less than a minute left"). Before that, when nothing is left, and when it would be more than a day, it SHALL show no time.

#### Scenario: Too early to tell
- **WHEN** a scan has read 4 titles in its first two seconds
- **THEN** its progress shows no time left

#### Scenario: Pace known
- **WHEN** a scan has read 100 titles in one minute and 300 are left
- **THEN** its progress reads "about 3 min left"

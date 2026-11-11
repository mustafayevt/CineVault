## MODIFIED Requirements

### Requirement: Progress card
The sidebar's progress card SHALL show the work the user started, oldest first, leaving out the name check and thumbnails. Its bold line SHALL be the job's verb with its count ("Scraping 3 of 25") when the size is known, otherwise the job's name, with "+N more" for further jobs and "{done} of {total} · {percent}%" under a progress bar. A scan SHALL count titles, not library folders: "Finding titles… N found" while it counts, then "Scanning N of M titles" with "{percent}% · about N min left" once the time left is known, and the folder or title being read. It SHALL open the list "Running work" with each job's controls. Moves between library folders and renames SHALL appear on it too; a rename SHALL read "Renaming 3 of 12" with the title just done, without Pause or Cancel.

#### Scenario: Card shows a scrape
- **WHEN** a scrape of 25 titles has finished 3
- **THEN** the card reads "Scraping 3 of 25" and "3 of 25 · 12%", with its bar at 12%

#### Scenario: Card shows a scan
- **WHEN** a scan of eight folders has read 312 of 707 titles and three minutes are left at its pace
- **THEN** the card reads "Scanning 312 of 707 titles", the list shows "44% · about 3 min left" and the title being read

#### Scenario: Card shows a rename
- **WHEN** a rename of 12 titles has done 3
- **THEN** the card reads "Renaming 3 of 12", with its bar at 25%

### Requirement: Throttled progress feed
Job progress SHALL reach the UI over one channel, carrying only the jobs that changed, at most 10 times a second per job; a new subscriber SHALL first get every job. A job's progress in its own units (the titles of a scan) SHALL change at most four times a second, except that a change of phase and the end of a library folder are sent at once. While no job is active the feed SHALL send nothing and sleep until a job is submitted.

#### Scenario: Idle
- **WHEN** no job is running
- **THEN** no progress update is sent until a job starts

#### Scenario: A fast scan
- **WHEN** a scan reads a thousand unchanged titles in one second
- **THEN** its title count reaches the UI at most four times in that second, and the last report shows all thousand done

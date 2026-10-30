# job-system Specification

## Purpose
Covers how CineVault runs long work in the background (scans, reading NFO files, media info, thumbnails, name checks, scrapes, artwork downloads and command-line batches): the resource lanes jobs run on, the order of work on one title, pause, resume and cancel, failures, persistence across a quit, the end of external programs, progress reporting in the sidebar, the taskbar and the dock, and the limits that keep the app responsive and its memory bounded.

## Requirements

### Requirement: Resource lanes
Every job SHALL run on the lane of the resource it uses: the **disk** lane (scans, reading NFO files, media info), the **CPU** lane (thumbnails, FFmpeg work, image processing) or one **network** lane per provider, named by its id (such as `tmdb`). Lanes SHALL run side by side, first come first served, with 2 disk workers, one CPU worker less than the CPU cores and 4 workers per provider by default, so a slow provider never holds back a scan or another provider.

#### Scenario: Scan during a scrape
- **WHEN** a scrape of 500 movies is running on TMDB and a scan of the library folders starts
- **THEN** the scan starts at once and both make progress at the same time

### Requirement: Order of work on one title
Work on one title SHALL keep a logical order: a title is scanned before its NFO files and media info are read; its details are saved before its NFO is written; and a rename is planned only for a scraped title, held back on the Rename page ("Held back until the video details are read") while the media info its names need is still pending. Different titles SHALL proceed independently.

#### Scenario: Rename waits for the video details
- **WHEN** the naming style uses the video resolution and a scraped title's media info has not been read yet
- **THEN** the Rename page holds the title back with "Read now", and includes it once the details are read

### Requirement: Pause and resume
The user SHALL be able to pause and resume each running job from the sidebar's progress card ("Pause {name}", "Resume {name}"). A paused job SHALL start no new item while running items finish, and paused time SHALL NOT count towards its speed or remaining time. At most 8 jobs SHALL run at once; waiting jobs SHALL start by priority, then in order of arrival, and paused jobs SHALL NOT take a place.

#### Scenario: Pause
- **WHEN** the user pauses a media-info job
- **THEN** its line ends with "(paused)" and no new file is read until it is resumed

### Requirement: Cancel within a second
The user SHALL be able to cancel each running job from the progress card ("Cancel {name}"). A cancelled job SHALL start no further item and drop items still waiting. A running item SHALL get 500 ms to finish or stop by itself; after that its work SHALL be dropped and rolled back, so the job is over within one second and no item is left half-processed.

#### Scenario: Cancel during a scrape
- **WHEN** the user cancels a scrape while title 30 of 100 is being saved
- **THEN** title 30 either completes or is rolled back, titles 31 to 100 are not touched, and the job ends as cancelled within a second

### Requirement: Failures do not stop a job
A failed item SHALL NOT stop its job; the job SHALL end as failed only when every item failed or its worker broke. Failed items SHALL be counted and grouped by their reason. When a job ends, the app SHALL tell screen readers politely "{name}: finished", "{name}: finished, N items failed" or "{name}: failed.", without repeating a cancel the user asked for and without speaking on every progress tick.

#### Scenario: One unreadable file
- **WHEN** one file in a media-info job of 200 files cannot be read
- **THEN** the other 199 are read and the job ends as finished with 1 item failed

### Requirement: Quit pauses and the next start resumes
On quit the app SHALL pause every job. Scans, scrape batches, thumbnail jobs and command-line batches SHALL be persistent: each item's outcome is committed before it counts, so the next start SHALL resume every unfinished job with only the items that had not finished, and nothing finished is done twice. Stored scrape batches SHALL wait until a metadata provider is ready.

#### Scenario: Resume after a restart
- **WHEN** the user quits during a scrape of 1,000 titles after 400 are saved
- **THEN** the next start scrapes the other 600 and does not scrape the first 400 again

### Requirement: A job that cannot resume is reported
A job kind that cannot be resumed SHALL be reported as a notice ("Unfinished {what} could not be resumed"), and the other kinds SHALL still resume.

#### Scenario: Unreadable stored job
- **WHEN** the stored thumbnail jobs cannot be read on start
- **THEN** a notice says so and the unfinished scan still resumes

### Requirement: External programs end with their job
When a job running FFmpeg (aspect ratio, picture hash, frame grabs) is cancelled, or an FFmpeg run passes its 120-second limit, the app SHALL stop that FFmpeg process and write nothing for it into the media folder. A cancelled MediaInfo read SHALL stop being waited for. The indexer service the app starts for torrent search SHALL end, with every process it started, when the app ends.

#### Scenario: Cancelled aspect ratio detection
- **WHEN** the user cancels a job while FFmpeg is measuring a file's aspect ratio
- **THEN** FFmpeg stops and nothing is written into the media folder

### Requirement: Job names in lifecycle words
Each job SHALL report its state (queued, running, paused, completed, failed or cancelled), the items done, failed and in total, the items being processed now, its speed and its estimated time remaining. Its name SHALL use the app's verbs and plain words, never an internal step name: "Scanning 4 folders", "Scraping 12 titles (TMDB)", "Downloading artwork for 8 titles", "Reading media info of 5 files", "Reading NFO files", "Preparing 40 thumbnails".

#### Scenario: Job named in plain words
- **WHEN** an artwork download runs for 8 titles
- **THEN** its name is "Downloading artwork for 8 titles"

### Requirement: Progress card
The sidebar's progress card SHALL show the work the user started, oldest first, leaving out the name check and thumbnails. Its bold line SHALL be the job's verb with its count ("Scraping 3 of 25") when the size is known, otherwise the job's name, with "+N more" for further jobs and "{done} of {total} · {percent}%" under a progress bar. It SHALL open the list "Running work" with each job's controls. Moves between library folders SHALL appear on it too.

#### Scenario: Card shows a scrape
- **WHEN** a scrape of 25 titles has finished 3
- **THEN** the card reads "Scraping 3 of 25" and "3 of 25 · 12%", with its bar at 12%

### Requirement: Taskbar and dock progress
In the desktop app, the combined progress of all active jobs SHALL be mirrored to the Windows taskbar button and the macOS dock tile: nothing while idle, an indeterminate bar while sizes are unknown, the fraction while running, and the paused style when every active job is paused. A browser has no taskbar or dock; its progress SHALL follow "Progress in a browser", and no taskbar or dock on the server SHALL be touched.

#### Scenario: Everything paused
- **WHEN** every active job is paused in the desktop app
- **THEN** the taskbar button shows the progress in the paused style

#### Scenario: A browser
- **WHEN** the interface is served to a browser and a job runs
- **THEN** no taskbar or dock is touched on the server and the sidebar's progress card shows the job

### Requirement: Throttled progress feed
Job progress SHALL reach the UI over one channel, carrying only the jobs that changed, at most 10 times a second per job; a new subscriber SHALL first get every job. While no job is active the feed SHALL send nothing and sleep until a job is submitted.

#### Scenario: Idle
- **WHEN** no job is running
- **THEN** no progress update is sent until a job starts

### Requirement: Finished jobs are bounded
The app SHALL keep at most the 50 newest finished jobs in memory; older finished jobs SHALL be dropped as new ones are submitted, while running and waiting jobs always stay.

#### Scenario: A long session
- **WHEN** the app runs all day and a few hundred small jobs finish
- **THEN** only the newest 50 finished jobs are kept and memory does not grow with the older ones

### Requirement: Responsive while busy
Background work SHALL NOT make the UI unresponsive: while scanning, media info and scraping run together, a key press SHALL paint within 50 ms at the 99th percentile and the poster grid SHALL scroll smoothly.

#### Scenario: Typing during heavy work
- **WHEN** a scan, a media-info job and a scrape are all running
- **THEN** typing in the search field and scrolling the grid stay smooth

### Requirement: Progress in a browser
In a browser, job progress SHALL arrive over the server's one stream with the same jobs, order, throttling and completion as the desktop app, and the sidebar's progress card and the Activity list SHALL work as they do in the desktop app. When the connection drops, the page SHALL reconnect and receive the events it missed; if the server cannot replay them, it SHALL send a reset and the page SHALL reload the job state. While no job is active the stream SHALL carry no progress messages. Every job SHALL record the session and client that started it, and the interface SHALL say "Started from another session" for a job started elsewhere.

#### Scenario: A job started in the browser
- **WHEN** the user starts a scan in a browser
- **THEN** the progress card shows it with the same counts and percentage as the desktop app, and it finishes with the same result

#### Scenario: Reconnect
- **WHEN** the connection drops while a scrape is at 3 of 25 and reconnects
- **THEN** the page first shows 3 of 25 and then continues from there

#### Scenario: Two sessions
- **WHEN** a job started in one tab is shown in another tab
- **THEN** it says "Started from another session", and cancelling it from the second tab asks first

#### Scenario: Idle
- **WHEN** no job is running
- **THEN** no progress message is sent to the browser

# library-scanning Specification

## Purpose
Covers how CineVault finds media on disk and keeps the library in step with it: the library folders (data sources) of the Movies and TV shows libraries, the scans that run on start, on a folder change and on request, the skip rules, file classification, movie folder and disc detection, stacking, title, year and ID parsing, episode detection, the reading of existing NFO and artwork files for new titles, incremental rescans, titles whose files are missing or were moved outside the app, and how the library pages refresh while a scan runs. A scan only reads: it never contacts a provider, never scrapes and never renames.

## Requirements

### Requirement: Library folders per library type
The user SHALL be able to add folders to the Movies or TV shows library from Settings › Library folders ("Add folder…", or by dropping folders on the window), switch a folder between "Movies" and "TV shows", and remove it. Adding a folder or switching its type SHALL start a scan at once. Removing a folder SHALL keep its titles in the library as offline and SHALL offer Undo.

#### Scenario: Folder removed
- **WHEN** the user removes the movie folder `D:\Movies`, which holds 12 titles
- **THEN** the toast reads "Removed D:\Movies · 12 titles stay, offline" with Undo, and the 12 titles stay in the library

#### Scenario: New folder scanned at once
- **WHEN** the user drops the folder `E:\Movies` on the Library folders section
- **THEN** it is added and a scan starts without another click

### Requirement: Folders that cannot be library folders
The app SHALL refuse, with a message that says why:
- a path that is not a full folder path, or that cannot be reached;
- an operating-system or system-critical folder, also when reached through a link;
- a folder that is already a library folder, lies inside a library folder of the same library, or contains one.

Switching a folder's type SHALL be refused when the other library has a folder that equals, contains or lies inside it.

#### Scenario: Nested folder refused
- **WHEN** the user adds `D:\Movies\Action` while `D:\Movies` is already a movie folder
- **THEN** the folder is not added and the message reads "D:\Movies\Action is already covered by the library folder D:\Movies."

### Requirement: Folder labels
Each library folder SHALL have a label that the sidebar shows in place of its path. A new folder's label SHALL be the folder's short name; when another folder of the same library type uses it, the parent folder's name SHALL be added ("Movies (E:)"), and then a number. Labels SHALL be unique within a library type, ignoring case, and SHALL NOT be empty. Changing a label SHALL touch no file and start no scan.

#### Scenario: Default label
- **WHEN** the user adds `\\nas\media\Movies 4K` as a movie folder
- **THEN** its label is "Movies 4K"

#### Scenario: Duplicate label refused
- **WHEN** a second movie folder is given the label of the first
- **THEN** the change is refused with "Another movie source already uses this label"

### Requirement: Folder health
Settings › Library folders SHALL show, under each folder, how many titles (or shows) it holds and "watched for changes", or "offline" with the number of titles kept when the folder cannot be reached. A folder that does not answer within 3 seconds SHALL count as offline.

#### Scenario: Disconnected drive
- **WHEN** the drive of a TV folder holding 40 shows is unplugged
- **THEN** the folder row reads "offline · 40 shows kept" and the shows stay in the library

### Requirement: Scan on start and on request
The app SHALL scan every library folder when it starts, unless the previous session's unfinished scan is resumed instead. Scanning on start SHALL have no on/off setting. The Library page SHALL offer "Scan folders now", which confirms with the toast "Scanning N folders", or says "Add a library folder first, then scan." when there is none.

#### Scenario: Startup scan
- **WHEN** the app starts and a folder was added to a library folder while it was closed
- **THEN** a scan runs by itself and the new title appears without the user pressing anything

### Requirement: Watched folders
The app SHALL watch every library folder, with no on/off setting, and queue an incremental scan of a changed folder once it has been quiet for the debounce period (`library.watch-debounce-seconds`, default 5 seconds, never less than 5). Every change event SHALL push the deadline back, so a folder being copied in is scanned once, after the copy ends. Reads and noise files (`Thumbs.db`, `desktop.ini`, `.DS_Store`, dot-files) SHALL NOT trigger a scan.

#### Scenario: New folder copied in
- **WHEN** the user copies a new movie folder without an NFO into a library folder
- **THEN** once the copy has been quiet for 5 seconds the title appears in stage New, and nothing is scraped or renamed

### Requirement: A scan only reads
A scan, and the steps that follow it (reading NFO files, media info and thumbnails), SHALL NOT contact a metadata provider, start a scrape, rename or move anything, or create, change or delete any file in the media folders.

#### Scenario: Twelve new titles
- **WHEN** a scan finds 12 new movie folders without NFO files
- **THEN** all 12 are New, no provider is contacted for them, and no file in the media folders changed

### Requirement: Existing NFO and artwork files read first
For every title a scan adds, the app SHALL record the artwork files next to the video and read the title's NFO file, without network access, in a job named "Reading NFO files". Only NFO files that parse with a title SHALL be read; a file that holds only a provider ID gives that ID. The read SHALL never write a file and SHALL honour field locks.

#### Scenario: NFO without a provider ID
- **WHEN** a movie folder has an NFO with a title and year but no provider ID
- **THEN** the title and year from the NFO are used and the title is New

#### Scenario: Broken NFO
- **WHEN** an NFO is malformed beyond what the tolerant reader can recover
- **THEN** the title is New, the NFO file is not changed, and the title can be scraped as usual

### Requirement: Stage of a title read from local files
A title whose NFO holds a provider ID SHALL become Scraped, or Renamed when its folder and file names already match the active naming style. A title without a usable NFO SHALL keep the title and year parsed from its names and stay New.

#### Scenario: Fresh library built from NFO files
- **WHEN** the first scan of an empty library finds 900 movies, 870 with Kodi NFO files holding a TMDB ID, 600 of those in folders that match the naming style
- **THEN** 600 titles are Renamed, 270 are Scraped and 30 are New, their posters and plots show, and no file in the media folders changed

### Requirement: Built-in skip rules
A scan SHALL NOT enter:
- the built-in system and junk folders, compared without regard to case (among them `$RECYCLE.BIN`, `System Volume Information`, `RECYCLER`, `@eaDir`, `lost.dir` for movies, `extrathumb` for TV, and the `.deletedByCineVault` trash folder);
- hidden folders whose name starts with `.` (for movies also `@`) followed by a word character; names such as `.45` are titles and are kept;
- a folder of the other library type nested inside the folder being scanned.

#### Scenario: Recycle bin
- **WHEN** a library folder holds a `$RECYCLE.BIN` folder with videos in it
- **THEN** none of those videos becomes a title

### Requirement: Marker files and user skip patterns
A folder that holds a `.cinevaultignore` file, or a `.nomedia` file while `library.skip-nomedia` is on (default on), SHALL be skipped with everything below it. A folder whose whole name matches a pattern in `library.skip-patterns` (`;`-separated, default `MAKEMKV`), or whose full path a pattern names, SHALL be skipped. Patterns SHALL be regular expressions, case-sensitive unless they start with `(?i)`; an invalid one SHALL be matched literally.

#### Scenario: Ignore marker
- **WHEN** a movie folder contains a `.cinevaultignore` file
- **THEN** that folder and its sub-folders are not added to the library

#### Scenario: Invalid pattern
- **WHEN** the user adds the skip pattern `[Samples`, which is not a valid regular expression
- **THEN** folders named `[Samples` are skipped

### Requirement: File classification
Each file SHALL be classified from its path relative to the library folder, without reading it, as one of: video, trailer, sample, video extra, extra, audio, subtitle, NFO, poster, fanart, banner, clearart, disc art, clearlogo, thumb, character art, key art, square art, season poster, season fanart, season banner, season thumb, extrafanart, extrathumb, theme, graphic, MediaInfo XML, `.vsmeta`, text, BIF, double extension or unknown.

#### Scenario: Trailer by suffix
- **WHEN** a movie folder contains `Example Movie (2020)-trailer.mkv`
- **THEN** the file is a trailer, not a second video

#### Scenario: Artwork by name
- **WHEN** a movie folder contains `poster.jpg`, `Example Movie (2020)-fanart.jpg` and `clearlogo.png`
- **THEN** they are a poster, a fanart and a clearlogo

### Requirement: Extras, trailers and NAS artwork
A file below a folder named like `extras`, `featurettes`, `behind the scenes`, `deleted scenes`, `interviews`, `scenes`, `shorts`, `bloopers` or `other` SHALL be an extra of its title. A video below a `trailer` or `trailers` folder SHALL be a trailer. The artwork a NAS stores in `.vsmeta` files SHALL be used only while `library.vsmeta-artwork` is on (default off).

#### Scenario: Extras folder
- **WHEN** a file sits at `Example Movie (2020)/Featurettes/Making of.mkv`
- **THEN** it is an extra of Example Movie

### Requirement: Single-movie and multi-movie folders
A folder with one distinct video (stacked parts count once) SHALL be one movie that every file below it joins. Several distinct videos in one folder, files directly in a library folder, and bucket folders named by one letter or digit, a year, a decade, a year range or a genre SHALL be multi-movie folders, where companion files join the movie whose base name they start with, longest base name first.

#### Scenario: Two movies in one folder
- **WHEN** a folder holds `Example Movie (2020).mkv` and `Another Movie (2018).mkv`, each with a matching `-poster.jpg`
- **THEN** two movies are created, each with its own video and poster

### Requirement: Disc structures and editions
A disc structure (`VIDEO_TS`, `BDMV`, `HVDVD_TS`) SHALL make one movie for the folder above it, and the disc's stream files SHALL NOT become movies of their own. An `.iso` image SHALL be a video like any other. A movie's title, year, edition and IDs SHALL come from its folder name; when the folder names no edition, the video's name SHALL give it.

#### Scenario: Blu-ray folder
- **WHEN** the folder `Example Movie (2020)/BDMV/` holds a Blu-ray structure
- **THEN** one disc movie is created at `Example Movie (2020)` and no `.m2ts` file becomes a movie

#### Scenario: Edition in the video's name
- **WHEN** the folder `Example Movie (2020)` holds `Example Movie - Extended Edition (2020).mkv`
- **THEN** the movie's edition is Extended Edition

### Requirement: Stacked multi-part files
Files with part markers in their names (`cd1`, `dvd2`, `part3`, `pt4`, `disc1`, `disk2`, `-a` to `-d`, `1of2`) and folders named like `CD1` and `CD2` SHALL be grouped into one movie or episode, with the parts in order.

#### Scenario: Two-part movie
- **WHEN** a folder holds `Example Movie.cd1.avi` and `Example Movie.cd2.avi`
- **THEN** one movie is created with two video parts in that order

### Requirement: Title and year parsing
For a title without NFO data the scan SHALL derive a clean title and year from its folder and file names. It SHALL strip the extension, resolution and release tags, the words in `library.bad-words` (`,`-separated, default empty) and edition names. The year SHALL be the last plausible four-digit year (after 1800 and before the current year plus 5) that is not the first word. The media source, such as WEB-DL or BluRay, SHALL be parsed too.

#### Scenario: Release-style folder name
- **WHEN** a folder is named `Example.Movie.Part.Two.2024.2160p.WEB-DL.DV.HDR10.TrueHD.Atmos.7.1-GRP`
- **THEN** the title is "Example Movie Part Two", the year 2024 and the media source WEB-DL

### Requirement: Provider IDs in names
IMDb IDs (`tt` and at least six digits), TMDB IDs (`tmdb-123`, `tmdbid=123`) and TVDB IDs SHALL be read from folder and file names before any online lookup.

#### Scenario: ID in the folder name
- **WHEN** a folder is named `Example Movie (1972) {tmdb-593}`
- **THEN** the movie has TMDB ID 593 before any online lookup

### Requirement: Shows and seasons
In a TV library folder, each top-level folder SHALL be a show, and a one-character folder SHALL be an A to Z bucket whose folders are shows. Season artwork SHALL attach to the season its file name or folder names, with season 0 for specials. Episode NFO files SHALL take precedence over names.

#### Scenario: A to Z bucket
- **WHEN** a TV folder holds `E/Example Show/Season 1/Example Show S01E01.mkv`
- **THEN** "Example Show" is a show and `E` is not

### Requirement: Episode detection
Episodes SHALL be found from file and folder names, including `S01E02`, `S01E02-E04` and other multi-episode ranges, `1x02`, season words in many languages (`Season 3`, `Staffel 2`, `Temporada 1`), `Episode 5`, dates, specials as season 0, and anime names with a `[CRC32]` hash, whose last number is the absolute episode. A multi-episode file SHALL be one file shared by several episodes. A date-based episode without a season SHALL take the year as its season.

#### Scenario: Multi-episode file
- **WHEN** a show folder holds `Example Show.S02E05-E07.mkv`
- **THEN** episodes S02E05, S02E06 and S02E07 are created sharing that one file

#### Scenario: Date-based episode
- **WHEN** a file is named `Example Show 2024-03-14.mkv`
- **THEN** an episode is created with the aired date 2024-03-14 in season 2024

### Requirement: Incremental rescans
A rescan SHALL read folders in parallel and treat a file as unchanged when its size, modification time (and inode on Unix) match the stored fingerprint, keeping its stored row and media info. Only differences SHALL be written, in one transaction; an unchanged library SHALL cause no write. A rescan of an unchanged 5,000-movie library SHALL finish within 10 seconds.

#### Scenario: Unchanged library
- **WHEN** the user scans a library where nothing changed on disk
- **THEN** no media info is read again and no title is modified

### Requirement: Replaced videos and the NEW flag
A video replaced by a different file SHALL keep its title and all its metadata, get its media info read again, and be flagged NEW. With `library.reset-new-on-update` on (default off), every NEW flag SHALL be cleared when an update of the library folders starts.

#### Scenario: Video replaced with a better version
- **WHEN** a movie's 1080p file is replaced by a 2160p file with another name
- **THEN** the movie keeps its metadata, the new video's media info is read, and the movie is flagged NEW

### Requirement: Folder reads per drive
A scan SHALL list at most four folders at a time on one drive or network share, counted across every scan running at once, while folders on different drives SHALL still be read side by side.

#### Scenario: Start-up scan on three hard disks
- **WHEN** the app starts and scans three libraries on three hard disks
- **THEN** each disk has at most four folder listings in progress at any moment, and the three disks are read at the same time

### Requirement: Unreachable folders are never compared
A library folder that is missing, unreadable or empty SHALL NOT be compared at all, so a disconnected drive never looks like a deleted library. The titles of an unreachable folder SHALL become offline and keep everything they have.

#### Scenario: Drive unplugged during a scan
- **WHEN** a library folder's drive is disconnected and a scan runs
- **THEN** its titles become offline, keep their metadata, and none of them is removed

### Requirement: Missing titles are kept
A title whose folder or video is gone while its library folder is online SHALL become Missing. Missing and offline titles SHALL keep their metadata, carry the Missing flag, show "Files missing" under the poster, and be listed by the problem filter "Missing files or offline". No title SHALL ever be removed from the library by a scan.

#### Scenario: Folder deleted outside the app
- **WHEN** the user deletes a movie folder in the file manager and the app scans
- **THEN** the movie stays in the library, flagged Missing, and appears under "Missing files or offline"

### Requirement: Titles moved outside the app are found again
When a scan finds a movie or show folder that no stored title holds, and exactly one stored title whose videos are gone fits it, the scan SHALL point that title at the new place instead of adding it again. A fit SHALL be the same video sizes (none empty) plus a shared file name stem or the same parsed title and year. The title, and for a show its seasons and episodes, SHALL keep its metadata, IDs and edits and take the artwork found at the new place.

#### Scenario: Folder renamed in the file manager
- **WHEN** the user renames "Example Movie (2020)" to "Example Movie moved" and the app scans
- **THEN** the title points at the new folder, keeps its plot and IDs, still has its poster, and no second title appears

#### Scenario: Show folder renamed
- **WHEN** the user renames the show folder "Example Show (2022)" to "Example Show moved" and the app scans
- **THEN** the show, its seasons and its episodes point at the new folder and keep their plot and IDs

#### Scenario: Different file
- **WHEN** a new folder has the same title as a missing one but a file of another size
- **THEN** it is a new title and the old one stays Missing

### Requirement: An uncertain move is never guessed
When a new folder fits more than one gone title, or a gone title fits more than one new folder, the scan SHALL relink nothing: the new folder SHALL become its own title and the gone titles SHALL stay. No title SHALL be deleted.

#### Scenario: Two gone titles fit
- **WHEN** two missing titles hold files of the same size and name and one new folder appears with that file
- **THEN** the new folder becomes its own title and both old titles stay

### Requirement: Calm library refresh during a scan
While a scan job advances, the library pages, the counts and the folder health SHALL be read again at most once every two seconds (the first change at once), and once more when any background job ends.

#### Scenario: A scan adds many titles
- **WHEN** a scan finishes hundreds of folders a second
- **THEN** the visible library page refreshes about every two seconds, not on every folder, and shows every new title once the scan ends

### Requirement: Scans are resumable jobs
A scan SHALL run as a persistent job on the disk lane, one library folder per job item, named "Scanning N folders". After a quit or crash, the next start SHALL resume the folders it had not finished instead of starting a new scan. After a scan, the app SHALL read the pending media info and prepare the thumbnails of new and changed titles in the background.

#### Scenario: Quit during a scan
- **WHEN** the user quits while a scan of 4 folders has finished 2
- **THEN** the next start scans the other 2 folders and does not start a full scan of its own

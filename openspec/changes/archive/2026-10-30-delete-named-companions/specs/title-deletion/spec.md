## ADDED Requirements

### Requirement: Delete takes the files named after the title
When a title shares its folder or sits in a library folder's root, and when an episode is deleted, Delete and permanent delete SHALL also take every file directly in the folder of each of its videos whose name is the video's base name or continues it after `.`, `-`, `_` or a space (subtitles, NFO, artwork, trailers and other sidecar files), as found on disk at the time of the delete. A title with its own folder SHALL go as that folder, as before.

#### Scenario: Subtitle and poster added after the last Scan
- **WHEN** `D:\Movies\Mixed` holds `Example Movie (2020).mkv`, `Example Movie (2020).en.srt` and `Example Movie (2020)-poster.jpg`, the library knows only the video, and the user deletes Example Movie (2020)
- **THEN** all three files are in the Recycle Bin, and Undo puts all three back

#### Scenario: Library folder root
- **WHEN** `D:\Movies\Example Movie (2020).mkv` sits in the library folder's root with `Example Movie (2020).nfo` and `Example Movie (2020)-fanart.jpg` next to it
- **THEN** a delete of Example Movie (2020) takes the three files and leaves `D:\Movies` and everything else in it

#### Scenario: Episode subtitles
- **WHEN** an episode `Example Show S01E01.mkv` is deleted and `Example Show S01E01.en.forced.srt` and `Example Show S01E02.en.srt` are next to it
- **THEN** only `Example Show S01E01.en.forced.srt` goes with it

#### Scenario: Sub-folders stay
- **WHEN** a shared folder holds a `Subs` folder with `Example Movie (2020).en.srt` in it
- **THEN** a delete of Example Movie (2020) leaves the `Subs` folder and its files

### Requirement: Files named after the title are counted and undoable
The files a delete takes by name SHALL count in the preview's files and size of their title, be part of the same History entry as the title's other files, and come back with Undo. Only a permanent delete SHALL erase them.

#### Scenario: Preview counts them
- **WHEN** the permanent-delete dialog lists a title in a shared folder with one known video and two subtitles named after it
- **THEN** its row counts three files and their size

#### Scenario: Undo
- **WHEN** the user undoes the delete of a title whose subtitle went with it by name
- **THEN** the subtitle is back at its old path with the video

## MODIFIED Requirements

### Requirement: Deleting never touches other titles
Delete and permanent delete SHALL act only on the selected title's own files in a folder that other titles also use, and SHALL do nothing to a title whose drive is offline. A file the library records for another title SHALL never be taken. A file whose name continues the base names of several videos SHALL belong to the longest of them; when the longest is shared with a video of another title, or another video not of this title, the file SHALL stay where it is and the delete SHALL go on. A video SHALL never be taken by its name alone.

#### Scenario: Offline drive
- **WHEN** the user deletes a title whose drive is offline
- **THEN** nothing happens to it and the message says "The drive of <library folder> is offline, so nothing was deleted."

#### Scenario: Longer name of another title
- **WHEN** a shared folder holds `Example Movie (2020).mkv` and `Example Movie (2020) Director's Cut.mkv`, each with an `.en.srt` subtitle, and the user deletes Example Movie (2020)
- **THEN** `Example Movie (2020).en.srt` goes with it, and `Example Movie (2020) Director's Cut.mkv` and its subtitle stay

#### Scenario: Name that continues after a space or a dot
- **WHEN** a shared folder holds `Heat.mkv`, `Heat 2.mkv`, `Movie.mkv` and `Movie.1080p.mkv`, each with a subtitle named after it, and the user deletes Heat and Movie
- **THEN** `Heat 2.mkv`, `Movie.1080p.mkv` and their subtitles stay

#### Scenario: Same name as another title's video
- **WHEN** another title's video in the same folder has the same base name as the deleted title's video
- **THEN** a file named after that base name stays, and the deleted title's own files go

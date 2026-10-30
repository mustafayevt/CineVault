# title-deletion Specification

## Purpose
Covers how the user deletes titles from disk: by default to the Recycle Bin (Windows) or Trash (macOS) with Undo, or to CineVault's own trash folders, and erased for good only through a typed confirmation or an explicit choice in Settings › Deleted files. It also covers where every file CineVault sets aside goes (deleted titles, a move's originals, rename leftovers, replaced artwork and NFO files), the automatic emptying of the trash folders, and the safety limits that keep a delete from touching anything else.

## Requirements

### Requirement: Delete sends titles to the Recycle Bin
Delete (the selection bar, the right-click menu item "Delete to Recycle Bin" ("Delete to Trash" on macOS and Linux), the title page header and the Delete key outside text fields) SHALL send each selected title's folder, or only its own files when it shares a folder or sits in a library folder's root, to the Recycle Bin on Windows or the Trash on macOS and Linux, then take the title out of the library, with no confirmation dialog. The titles SHALL leave the lists at once. Every message that names the system bin SHALL call it "Recycle Bin" on Windows and "Trash" on macOS and Linux.

#### Scenario: Delete one movie
- **WHEN** the user presses Delete with Example Movie (2020) selected
- **THEN** its folder is in the Recycle Bin, the title leaves the library at once, and a toast says "Deleted Example Movie (2020) to the Recycle Bin" with Undo

#### Scenario: Several titles
- **WHEN** the user deletes four selected titles
- **THEN** the toast says "Deleted 4 titles to the Recycle Bin" with Undo

#### Scenario: Nothing on disk
- **WHEN** the deleted title has no files on disk
- **THEN** it leaves the library and the toast says "Removed <title>: it had no files on disk"

#### Scenario: Delete on Linux
- **WHEN** the user presses Delete with Example Movie (2020) selected in `/home/alex/Movies` on Linux
- **THEN** its folder is in the user's Trash, and the toast says "Deleted Example Movie (2020) to the Trash" with Undo

### Requirement: Delete never erases by itself
Where the system bin cannot take a path (a network share, a mapped network drive, a removable drive on Windows, a drive whose bin is off, an item larger than the bin; on Linux a network or read-only file system, or a volume with no Trash folder on the same file system), Delete SHALL move the files to the library folder's trash folder `.deletedByCineVault/<batch>/` instead. No file SHALL be erased by Delete unless Settings › Deleted files is "Delete permanently". An item that left its place but is not in the bin SHALL be reported.

#### Scenario: Network share
- **WHEN** the title is on `\\nas\movies`
- **THEN** its folder goes to `\\nas\movies\.deletedByCineVault\<batch>\` and the toast says "Deleted <title> to the library folder's trash folder", with Undo

#### Scenario: NFS mount on Linux
- **WHEN** the title is in `/mnt/nas/movies`, an NFS mount, on Linux
- **THEN** its folder goes to `/mnt/nas/movies/.deletedByCineVault/<batch>/` and the toast says "Deleted <title> to the library folder's trash folder", with Undo

### Requirement: Where deleted files go
Settings › Deleted files (`files.deleted-mode`, written atomically) SHALL choose where Delete, a move's originals, rename leftovers and artwork or NFO files replaced by an artwork or match change go: "Recycle Bin" (`bin`, the default), "Trash folder on each drive" (`folder`), "Custom trash folder…" (`custom`) or "Delete permanently" (`erase`). An unknown value SHALL count as "Recycle Bin". Shift+Delete SHALL stay a permanent delete whatever the choice.

#### Scenario: Recycle Bin choice
- **WHEN** the choice is "Recycle Bin"
- **THEN** Delete and a move's originals go to the system bin (the drive's trash folder where there is none), and rename leftovers and replaced files go to the drive's trash folder

#### Scenario: Trash folder chosen
- **WHEN** the choice is "Trash folder on each drive" and the user deletes Example Movie (2020) from `D:\Movies`
- **THEN** its folder is in `D:\Movies\.deletedByCineVault\<batch>\Example Movie (2020)`, nothing is in the Recycle Bin, and Undo brings it back

#### Scenario: Choice changed later
- **WHEN** a title was deleted to the Recycle Bin and the choice is then changed
- **THEN** its Undo still finds it in the Recycle Bin and restores it

### Requirement: Custom trash folder
With "Custom trash folder…" and a folder chosen with "Choose folder…" (`files.trash-folder`), files on that folder's volume SHALL be renamed into `<folder>/.deletedByCineVault/<batch>/<drive>/<path below the volume root>`, never into the chosen folder's top, and SHALL come back by Undo. Files on another volume SHALL go to their own drive's trash folder, and the help text SHALL say so.

#### Scenario: Custom folder on another drive
- **WHEN** the custom trash folder is `D:\Trash` and a title on `E:\Movies` is deleted
- **THEN** its folder goes to `E:\Movies\.deletedByCineVault\<batch>\`, not to D:

#### Scenario: No folder chosen yet
- **WHEN** the choice is "Custom trash folder…" and no folder is chosen
- **THEN** the section says "Choose a folder; until then files go to each drive's trash folder." and files go there

### Requirement: Delete permanently as the choice
Choosing "Delete permanently" SHALL first ask for the typed DELETE confirmation ("Erase deleted files at once?") and SHALL show the red warning "Deleted and replaced files are erased at once. Undo can't bring them back." With it on, a Delete SHALL be a permanent delete without Undo, and renames and moves SHALL stay undoable (a rename without its erased leftovers, a move by copying back). In a browser, permanent delete SHALL be off unless it is enabled in Settings › Server, and a bin/trash delete SHALL show no dialog in either surface.

#### Scenario: Cancelled confirmation
- **WHEN** the user picks "Delete permanently" and cancels the confirmation
- **THEN** the choice stays as it was

#### Scenario: Delete with the choice on
- **WHEN** the choice is "Delete permanently" and the user deletes a title in the desktop app
- **THEN** its files are erased, the toast says "Erased <title>: Deleted files are set to Delete permanently", and a match or artwork entry whose replaced files were erased has no Undo

#### Scenario: Move with the choice on
- **WHEN** the choice is "Delete permanently" and a title moves to another drive
- **THEN** each original is erased only after its copy is verified

#### Scenario: Browser permanent delete off
- **WHEN** permanent delete has not been enabled in Settings › Server and a browser asks to erase a title
- **THEN** the request is refused and the UI offers the configured deleted-files destination

#### Scenario: Bin delete
- **WHEN** the user deletes a title to the configured bin or trash folder
- **THEN** no confirmation dialog is shown and the delete is undoable

### Requirement: Empty trash folders automatically
Settings › Deleted files SHALL offer "Empty trash folders automatically" (`files.auto-empty-days`) with "Off" (the default), "After 7 days", "After 30 days" and "After 90 days". When on, at start-up and then once a day, CineVault SHALL erase each batch of its own trash folders (the custom folder's included) whose newest file is older than the chosen days, and SHALL never touch the system Recycle Bin or Trash.

#### Scenario: Thirty days
- **WHEN** the setting is 30 days and a batch in `D:\Movies\.deletedByCineVault` was last changed 40 days ago
- **THEN** that batch is erased at the next start, a batch from last week stays, and the History entries of the erased files lose their Undo with the reason

### Requirement: Delete is undoable
Each Delete SHALL be one History entry. Undo (from the toast or Restore in History) SHALL put every file back at its old path and bring the title back to the library with its details, artwork, stage and history, without a rescan or a new scrape. When a file cannot be put back, that title SHALL be left as it is and reported with the reason, and nothing SHALL be overwritten.

#### Scenario: Bin emptied
- **WHEN** the user emptied the Recycle Bin and then chooses Restore on a delete entry
- **THEN** a message says the files are not in the Recycle Bin and nothing was restored, and the entry stays as it was

#### Scenario: Old path taken
- **WHEN** a new folder with the same name was created at the old path
- **THEN** Restore leaves both the new folder and the bin untouched and reports that something is at that path

### Requirement: Shift+Delete asks for a typed confirmation
Shift+Delete and "Delete permanently…" SHALL open the one confirmation dialog: "Delete N titles permanently?", a line saying the folders and files are erased and that "This skips the Recycle Bin and cannot be undone", the list "What will be erased" with each title's path and size, a field for DELETE, and "Cancel", "Send to Recycle Bin instead" and "Delete permanently". The last SHALL stay disabled until the field holds exactly DELETE. In a browser the dialog SHALL also ask for the current password, and the server SHALL mint a one-time intent bound to the listed item IDs before the request is sent; a missing, reused, expired or mismatched intent, or a wrong password, SHALL be refused before any file is touched.

#### Scenario: Confirm button stays off
- **WHEN** the dialog is open and the user has typed "delet"
- **THEN** "Delete permanently" is disabled, and the backend also refuses any word but DELETE

#### Scenario: Shared folder in the list
- **WHEN** a listed title shares its folder or sits in a library folder's root
- **THEN** its row says "Only its own files in this shared folder" or "Only its own files; the library folder stays"

#### Scenario: Change of mind
- **WHEN** the user clicks "Send to Recycle Bin instead"
- **THEN** the titles are deleted as with Delete, and the dialog closes

#### Scenario: Browser confirmation
- **WHEN** a browser confirms a permanent delete of two titles
- **THEN** it sends DELETE, the current password and the server's one-time intent, and the two titles are erased

#### Scenario: Replayed intent
- **WHEN** the same intent is sent again
- **THEN** the server refuses it and no file is touched

### Requirement: Permanent delete erases only what it listed
Confirming a permanent delete SHALL write its History entry first with every path it is about to erase, then erase the titles one by one, file by file, with short retries for files another program holds. It SHALL refuse a path outside its library folder and a path that is or holds a library folder's root. In a browser it SHALL also check the one-time intent and the password before the History entry is written. Only titles erased whole SHALL leave the library, and the entry SHALL list what was erased and what was left.

#### Scenario: A file in use
- **WHEN** the second of two titles has a file open in a player
- **THEN** the first title is erased, the second stops at that file and stays in the library with its remaining files, and a message names the file in use

#### Scenario: A library folder root
- **WHEN** a title's files sit directly in `D:\Movies`
- **THEN** only that title's own files are erased and `D:\Movies` itself is never removed

#### Scenario: Intent checked first
- **WHEN** a browser sends an erase request with a wrong password
- **THEN** no History entry is written and no file is erased

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

### Requirement: Linux Trash folder on the same file system
On Linux the Trash SHALL take a path only when a Trash folder on the same file system can be used, in this order: the home Trash (`~/.local/share/Trash` by default) when it is on the same device; else the volume's shared `.Trash/<uid>` folder when `.Trash` at the mount point is a real folder with the sticky bit; else the volume's `.Trash-<uid>` folder, created when the mount point is writable. Otherwise the path SHALL go to the library folder's trash folder, with a note naming the reason.

#### Scenario: Second disk with its own Trash
- **WHEN** Example Movie (2020) in `/media/alex/Films/Movies` (an ext4 disk mounted for the user) is deleted on Linux
- **THEN** its folder is in `/media/alex/Films/.Trash-1000/files/`, its info file in `/media/alex/Films/.Trash-1000/info/`, and Undo puts it back

#### Scenario: Disk without a usable Trash
- **WHEN** a title is deleted from a disk whose mount point the user cannot write and that has no `.Trash` folder
- **THEN** its folder goes to the library folder's `.deletedByCineVault/<batch>/`, the toast says so with the reason "This drive has no Trash folder CineVault can use", and nothing is copied to the home Trash

### Requirement: Linux Trash send is one rename
On Linux a path SHALL reach the Trash by one rename, after its `.trashinfo` file (`Path`, `DeletionDate`) is written, so file managers list and restore it. A path SHALL never be copied into a Trash. When the rename fails, the path SHALL stay in place, its info file SHALL be removed, and the path SHALL go to the library folder's trash folder.

#### Scenario: Rename refused after the check
- **WHEN** the rename into the Trash fails after the Trash folder was chosen
- **THEN** the title's folder is in the library folder's trash folder, no partial copy exists in any Trash, and the info file written for it is gone

### Requirement: Trash items matched by exact path on Linux
On Linux, finding a sent item in the Trash, restoring it, and erasing it with the Drives page's Empty SHALL compare original paths exactly, with letter case, so `Movie.mkv` and `movie.mkv` are two different items. On Windows and macOS the comparison SHALL ignore letter case as before.

#### Scenario: Two names that differ by case
- **WHEN** `/home/alex/Movies/Example/movie.mkv` and `/home/alex/Movies/Example/Movie.mkv` were both sent to the Trash and the user undoes only the delete of `Movie.mkv`
- **THEN** only `Movie.mkv` is restored and `movie.mkv` stays in the Trash

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

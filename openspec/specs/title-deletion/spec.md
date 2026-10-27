# title-deletion Specification

## Purpose
Covers how the user deletes titles from disk: by default to the Recycle Bin (Windows) or Trash (macOS) with Undo, or to CineVault's own trash folders, and erased for good only through a typed confirmation or an explicit choice in Settings › Deleted files. It also covers where every file CineVault sets aside goes (deleted titles, a move's originals, rename leftovers, replaced artwork and NFO files), the automatic emptying of the trash folders, and the safety limits that keep a delete from touching anything else.

## Requirements

### Requirement: Delete sends titles to the Recycle Bin
Delete (the selection bar, the right-click menu item "Delete to Recycle Bin", the title page header and the Delete key outside text fields) SHALL send each selected title's folder, or only its own files when it shares a folder or sits in a library folder's root, to the Recycle Bin on Windows or the Trash on macOS, then take the title out of the library, with no confirmation dialog. The titles SHALL leave the lists at once.

#### Scenario: Delete one movie
- **WHEN** the user presses Delete with Example Movie (2020) selected
- **THEN** its folder is in the Recycle Bin, the title leaves the library at once, and a toast says "Deleted Example Movie (2020) to the Recycle Bin" with Undo

#### Scenario: Several titles
- **WHEN** the user deletes four selected titles
- **THEN** the toast says "Deleted 4 titles to the Recycle Bin" with Undo

#### Scenario: Nothing on disk
- **WHEN** the deleted title has no files on disk
- **THEN** it leaves the library and the toast says "Removed <title>: it had no files on disk"

### Requirement: Delete never erases by itself
Where the system bin cannot take a path (a network share, a mapped network drive, a removable drive, a drive whose bin is off, an item larger than the bin), Delete SHALL move the files to the library folder's trash folder `.deletedByCineVault/<batch>/` instead. No file SHALL be erased by Delete unless Settings › Deleted files is "Delete permanently". An item that left its place but is not in the bin SHALL be reported.

#### Scenario: Network share
- **WHEN** the title is on `\\nas\movies`
- **THEN** its folder goes to `\\nas\movies\.deletedByCineVault\<batch>\` and the toast says "Deleted <title> to the library folder's trash folder", with Undo

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
Choosing "Delete permanently" SHALL first ask for the typed DELETE confirmation ("Erase deleted files at once?") and SHALL show the red warning "Deleted and replaced files are erased at once. Undo can't bring them back." With it on, a Delete SHALL be a permanent delete without Undo, and renames and moves SHALL stay undoable (a rename without its erased leftovers, a move by copying back).

#### Scenario: Cancelled confirmation
- **WHEN** the user picks "Delete permanently" and cancels the confirmation
- **THEN** the choice stays as it was

#### Scenario: Delete with the choice on
- **WHEN** the choice is "Delete permanently" and the user deletes a title
- **THEN** its files are erased, the toast says "Erased <title>: Deleted files are set to Delete permanently", and a match or artwork entry whose replaced files were erased has no Undo

#### Scenario: Move with the choice on
- **WHEN** the choice is "Delete permanently" and a title moves to another drive
- **THEN** each original is erased only after its copy is verified

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
Shift+Delete and "Delete permanently…" SHALL open the one confirmation dialog: "Delete N titles permanently?", a line saying the folders and files are erased and that "This skips the Recycle Bin and cannot be undone", the list "What will be erased" with each title's path and size, a field for DELETE, and "Cancel", "Send to Recycle Bin instead" and "Delete permanently". The last SHALL stay disabled until the field holds exactly DELETE.

#### Scenario: Confirm button stays off
- **WHEN** the dialog is open and the user has typed "delet"
- **THEN** "Delete permanently" is disabled, and the backend also refuses any word but DELETE

#### Scenario: Shared folder in the list
- **WHEN** a listed title shares its folder or sits in a library folder's root
- **THEN** its row says "Only its own files in this shared folder" or "Only its own files; the library folder stays"

#### Scenario: Change of mind
- **WHEN** the user clicks "Send to Recycle Bin instead"
- **THEN** the titles are deleted as with Delete, and the dialog closes

### Requirement: Permanent delete erases only what it listed
Confirming a permanent delete SHALL write its History entry first with every path it is about to erase, then erase the titles one by one, file by file, with short retries for files another program holds. It SHALL refuse a path outside its library folder and a path that is or holds a library folder's root. Only titles erased whole SHALL leave the library, and the entry SHALL list what was erased and what was left.

#### Scenario: A file in use
- **WHEN** the second of two titles has a file open in a player
- **THEN** the first title is erased, the second stops at that file and stays in the library with its remaining files, and a message names the file in use

#### Scenario: A library folder root
- **WHEN** a title's files sit directly in `D:\Movies`
- **THEN** only that title's own files are erased and `D:\Movies` itself is never removed

### Requirement: Deleting never touches other titles
Delete and permanent delete SHALL act only on the selected title's own files in a folder that other titles also use, and SHALL do nothing to a title whose drive is offline.

#### Scenario: Offline drive
- **WHEN** the user deletes a title whose drive is offline
- **THEN** nothing happens to it and the message says "The drive of <library folder> is offline, so nothing was deleted."

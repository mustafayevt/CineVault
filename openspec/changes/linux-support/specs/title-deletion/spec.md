## MODIFIED Requirements

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

## ADDED Requirements

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

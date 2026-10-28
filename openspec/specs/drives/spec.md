# drives Specification

## Purpose
Shows where the library lives on disk and moves titles between library folders safely. It covers the drives in the sidebar, the Drives page with a card per drive (free space, library folders, quality mix, trash folders, online state) and the titles of a drive, emptying CineVault's trash, and "Move to…": choosing a destination with the space before and after in view, then moving whole title folders through the journal with a verified copy, progress, cancel, interruption recovery and Undo.

## Requirements

### Requirement: Drives in the sidebar
The sidebar SHALL list each drive that holds a library folder with its name, a thin used-space bar and "<free> free". A low drive SHALL show its bar and note in the warning colour; an offline drive SHALL be greyed with "Offline". The tooltip SHALL read "<name> · <free> free of <total>". Clicking a drive SHALL open it on the Drives page, and the Drives item SHALL show a warning pill "N low" while any drive is low.

#### Scenario: Low drive
- **WHEN** drive C: has 41 GB free of 931 GB
- **THEN** the sidebar shows C: with a warning-coloured bar and "41 GB free", and the Drives item shows "1 low"

#### Scenario: Offline drive
- **WHEN** the network drive holding `Z:\Shows` does not answer
- **THEN** the sidebar shows that drive greyed with "Offline"

### Requirement: Which drive a folder is on
A library folder SHALL belong to the volume that holds it: a Windows drive letter, a network share, a macOS mount point or a Linux mount point (such as `/media/<user>/<name>`, `/run/media/<user>/<name>`, `/mnt/<name>` or `/`). Two folders on one volume SHALL be one drive with two folders. A Linux drive with no volume label SHALL be named after its mount point's last part. A drive SHALL be online when at least one of its library folders answers the health check, and every folder that answers SHALL record when it was last seen. On Linux a library folder below `/media/<user>/<name>`, `/run/media/<user>/<name>` or `/mnt/<name>` SHALL NOT answer while that folder exists but is not a mount point (the drive is unmounted).

#### Scenario: Two folders on one drive
- **WHEN** `E:\Movies` and `E:\TV` are library folders
- **THEN** one drive E: is listed with both folders

#### Scenario: Two folders on one Linux disk
- **WHEN** `/media/alex/Films/Movies` and `/media/alex/Films/TV` are library folders on Linux
- **THEN** one drive "Films" is listed with both folders

#### Scenario: Unmounted Linux disk
- **WHEN** the disk mounted at `/mnt/films` is unmounted, leaving the empty folder `/mnt/films`, and `/mnt/films` is a library folder
- **THEN** the drive is offline, its titles keep their state, and a scan does not mark them missing

### Requirement: Low space rule
A drive SHALL count as low when less than 5% of it or less than 50 GB is free (either is enough). An offline drive SHALL NOT be judged.

#### Scenario: Big drive
- **WHEN** a 10.9 TB drive has 400 GB free
- **THEN** it is low, because 400 GB is under 5% of 10.9 TB

#### Scenario: Small drive with room
- **WHEN** a 931 GB drive has 60 GB free
- **THEN** it is not low

### Requirement: Drive cards
The Drives page SHALL show a card per drive holding a library folder: the drive name with its size and kind ("HDD", "SSD", "Network" or "Removable"), a "<free> free of <total>" bar ("Low: <free> free" when low), each library folder on it with its title or show count and size, the space of other files as "not in library" from 1 GB, and a 2160p, 1080p, 720p and SD breakdown.

#### Scenario: Two folders on one card
- **WHEN** drive E: holds `E:\Archive\Movies` with 39 titles and `E:\TV` with 18 shows
- **THEN** its card lists both folders with "39 titles" and "18 shows" and their sizes

#### Scenario: Offline card
- **WHEN** `Z:` was last reachable two days ago
- **THEN** its card is greyed, reads "Offline · last seen 2 days ago", still lists its folders, and its titles stay in the library

### Requirement: Drives page header
The Drives page header SHALL say how many drives there are, how many are offline, and when the numbers were last read. With no library folder the page SHALL say "No drives to show yet" with "Open Settings".

#### Scenario: One drive offline
- **WHEN** four drives hold library folders and one does not answer
- **THEN** the header reads "4 drives · 1 offline" with "Updated just now"

### Requirement: Space numbers stay current
Space numbers SHALL be read again when the Drives page opens, when the window comes back to the front, after a scan, move or delete changes files (announced at most once in ten seconds), when a drive comes or goes, and every minute while the app is in front. Reading space SHALL NOT wake network drives more often than that.

#### Scenario: After a move
- **WHEN** a move of 198.7 GB from D: to E: finishes
- **THEN** the sidebar and Drives page show D: with 198.7 GB more free and E: with 198.7 GB less, without a restart

### Requirement: Titles of a drive
Choosing a drive card SHALL list that drive's titles in a table beside the cards with the columns Title, Year, Quality, Size and Folder, "Biggest first" by default, sortable by any column. The table SHALL support desktop multi-select, and a selection SHALL show the floating selection bar with the count, the total size, "Move to…" and "Delete".

#### Scenario: Biggest first
- **WHEN** the user opens drive D:
- **THEN** its titles are listed with the largest at the top, under "142 titles in D:\Movies"

#### Scenario: Other sorts
- **WHEN** the user opens the sort menu
- **THEN** it offers "Biggest first", "Smallest first", "Title A to Z" and "Newest first"

### Requirement: Trash on each drive
Each online drive card SHALL show, when not empty, "Trash folder" with the bytes in its library folders' `.deletedByCineVault` folders, "In the Recycle Bin from CineVault" with the bytes CineVault sent to the bin from that drive that are still there, and "Custom trash folder · <path>" when that folder is on the drive. Sizes SHALL come from walking only those folders, never following a link or junction.

#### Scenario: Originals of a move in the bin
- **WHEN** a 3.5 GB movie moved from D: to F: and its originals went to D:'s Recycle Bin
- **THEN** D:'s card shows "In the Recycle Bin from CineVault" with their size, artwork included

#### Scenario: Only recorded bin items
- **WHEN** the Recycle Bin also holds files the user deleted elsewhere
- **THEN** only the items History recorded with the bin's id are counted, and nothing else in the bin is listed or touched

### Requirement: Empty a drive's trash
A drive card with trash SHALL offer "Empty…", and the Drives page menu "Empty all trash folders…". Both SHALL open the typed DELETE confirmation listing each trash-folder batch and bin item with its size, with the button "Empty for good". Confirming SHALL erase only top entries of a `.deletedByCineVault` folder (never the custom folder's other contents; links removed, never followed) and only the recorded bin items, then refresh the drives and History.

#### Scenario: Empty D:
- **WHEN** the user chooses Empty… on D:, types DELETE and confirms
- **THEN** D:'s trash folder batches and the items CineVault sent to the Recycle Bin from D: are erased, the toast says "Emptied the trash: <size> freed", and the delete entries of those files show why their Undo is off

#### Scenario: Wrong word
- **WHEN** the field holds "delete"
- **THEN** "Empty for good" stays disabled and nothing is erased

### Requirement: Move to with space before and after
"Move to…" in the Library selection bar, the right-click menu, the title page header ("Move") and the Drives selection bar SHALL open the same menu: "Move N titles · <size> to", then each other library folder of the same type with its drive, a used-space bar and "<free> free → <free after> after", and how much the source drive frees. Nothing SHALL touch the disk until the user chooses a destination.

#### Scenario: Space before and after
- **WHEN** the user moves 3 titles totalling 198.7 GB from D: to `E:\Archive\Movies`
- **THEN** the menu shows "5.81 TB free → 5.61 TB after" for it and "D: goes from 1.21 TB to 1.40 TB free"

#### Scenario: Not enough room
- **WHEN** the selection is 2.1 TB and a destination has 41 GB free
- **THEN** that destination is disabled with "Needs 2.1 TB and has 41 GB."

#### Scenario: Offline or no other folder
- **WHEN** a destination is offline, or no other folder of the type exists
- **THEN** the destination is disabled with "Offline", or the menu says "There is no other movie folder. Add one in Settings."

### Requirement: Move the whole title folder
A move SHALL take a movie's or show's whole title folder (videos, NFOs, artwork, subtitles, trailers, extras and every other file and sub-folder) to the chosen library folder of the same type, keeping its place below the library folder. Library rows SHALL be re-pointed, never removed and added again.

#### Scenario: Everything moves
- **WHEN** the user moves Example Movie (2020), whose folder holds the video, an NFO, five images, two subtitles, a trailer and an `extras` folder
- **THEN** all of them arrive in the destination and nothing is left in the old folder

#### Scenario: Shared folder refused
- **WHEN** the movie shares its folder with another movie
- **THEN** it is not moved and the reason is "This folder holds other titles too."

#### Scenario: Other refusals
- **WHEN** a title's files sit in the library folder's root, some files are outside its folder, its folder is a link, its library folder cannot be reached, or an earlier move of it did not finish
- **THEN** it is not moved and its reason is shown

### Requirement: Move conflicts never overwrite
When the destination already has a folder of the title's name, the title SHALL NOT be moved, overwritten or merged, and SHALL be shown as unable to go there with its reason.

#### Scenario: Destination exists
- **WHEN** the destination already has a folder `Example Movie (2020)`
- **THEN** that title is not moved and the reason reads "A folder with this name already exists on <destination>."

### Requirement: Journal before the disk
Before the disk is touched, a move batch and every file's source and destination SHALL be written to the journal. A move within one volume SHALL be one rename of the title folder.

#### Scenario: Same drive
- **WHEN** a title moves from `D:\Movies` to `D:\Archive`
- **THEN** its folder is renamed in one step and the library points at the new place

### Requirement: Verified copy across volumes
A move across volumes SHALL copy one file at a time as `<name>.cvpart` into a hidden staging folder next to the destination (`.cvmove-<batch>-<id>`, which scans skip), hashing (SHA-256) 8 MiB chunks on a reader thread while the writer writes the one before. Each copy SHALL be flushed, read back and matched by size and hash. Then the staging folder SHALL become the destination folder in one rename, and the library SHALL be re-pointed in the same transaction.

#### Scenario: The disk kept something else
- **WHEN** the copy of `Example Movie.mkv` reads back with one byte different from the original
- **THEN** the copy is removed, the title is taken back to how it was, and the batch stops with "The copy of Example Movie.mkv does not match the original"

#### Scenario: Uncached read-back
- **WHEN** a copy is read back on Windows or Linux
- **THEN** the read bypasses the file cache with aligned whole blocks (`FILE_FLAG_NO_BUFFERING` on Windows, `O_DIRECT` on Linux), falling back to a normal read where that is refused; on macOS it is a normal read

#### Scenario: Last block of an odd size
- **WHEN** a file of 3 × 8 MiB + 5 bytes is read back without the cache
- **THEN** all its bytes are read, the last 5 included, and the hash matches

### Requirement: Originals go to the deleted-files destination
Only after the title is in place SHALL the originals go as Settings › Deleted files says: by default to the Recycle Bin or Trash, to the library folder's `.deletedByCineVault/<batch>/` where the bin cannot take them, to the trash folder or custom trash folder by choice, and erased only with "Delete permanently". Empty folders the move leaves behind SHALL be removed.

#### Scenario: Cross-drive move
- **WHEN** a title moves from D: to E:
- **THEN** every file is copied and verified on E:, the library points at E:, and the originals are in the Recycle Bin

#### Scenario: Network source
- **WHEN** the source is a network share the Recycle Bin cannot take
- **THEN** the originals go to that library folder's trash folder

### Requirement: Move progress, pause and cancel
Only one move SHALL run at a time. A running move SHALL show on the Drives page and in the sidebar progress card with its name, percentage, the speed of the last few seconds and the time left, with Pause and Cancel. Cancel before the title is in place SHALL leave it wholly in its old place, with nothing recorded in History for it; cancel after that SHALL let the title finish in its new place and stop the batch.

#### Scenario: Progress line
- **WHEN** a move of Example Movie (2020) to E: is running
- **THEN** it reads "Moving Example Movie (2020) to E:" with its percentage, the speed per second and "N min left" (or "under a minute left", or "H h M min left")

#### Scenario: Cancel during copy
- **WHEN** the user cancels a move while files are still being copied
- **THEN** the copies are removed, the title stays wholly in its old place, the toast says "Move cancelled: <title> is where it was", and History lists nothing for it

#### Scenario: Finished
- **WHEN** a move finishes
- **THEN** a toast says "Moved <title> to <destination>" with Undo

### Requirement: Interrupted moves are safe
A full disk, access denied, a file in use or a missing original SHALL stop the batch without removing anything, and the originals SHALL stay intact. A title that finished before the stop SHALL stay in its new place.

#### Scenario: Disk full
- **WHEN** the destination fills up during the copy
- **THEN** the move stops with "Destination is full" and the originals are untouched

### Requirement: Unfinished moves are recovered
A move left unfinished by a stop, a quit or a crash SHALL be found in the journal at the next start, before any new file work, and reported with a warning naming its titles and reason. It SHALL be resumable (copied files checked again by size and hash, the rest copied) or able to be rolled back (removing only the `.cvpart` files, copies and staging folder the batch made). A new move of such a title SHALL be refused until then.

#### Scenario: App closed mid-copy
- **WHEN** the app is closed after copying 3 of 9 files across volumes
- **THEN** all 9 originals are intact, and at the next start a warning says the move did not finish and the originals are untouched

### Requirement: Moves are undoable
Every finished move SHALL be one History entry. Its Undo (from the toast or History) SHALL check every file first and bring the titles back the same safe way: the originals return from the bin or trash folder when they are all there and the copies go to the deleted-files destination; otherwise the titles are copied back with verification. A title that cannot be restored whole SHALL stay where it is and be reported.

#### Scenario: Undo after emptying the bin
- **WHEN** the user emptied the Recycle Bin and then undoes a move from D: to E:
- **THEN** the files are copied back to D: and verified, the library points at D:, and the copies on E: go to the Recycle Bin

### Requirement: Recovery waits for an unmounted Linux drive
On Linux, recovering an interrupted rename or move batch SHALL treat a drive as reachable only when its mount root (`/media/<user>/<name>`, `/run/media/<user>/<name>`, `/mnt/<name>`, or `/` for other paths) is a mount point. A batch on an unmounted drive SHALL wait until the drive is back, as for a disconnected drive on Windows.

#### Scenario: Batch on an unmounted disk
- **WHEN** CineVault starts after a rename batch on `/media/alex/Films` was interrupted and that disk is not mounted
- **THEN** the batch is left waiting, nothing is written below `/media/alex/Films`, and it is settled once the disk is mounted again

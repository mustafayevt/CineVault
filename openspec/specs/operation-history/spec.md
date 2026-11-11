# operation-history Specification

## Purpose
Keeps a lasting record of every change CineVault makes to the user's files and titles (renames, moves, deletes, permanent deletes, artwork changes and match changes) and lets the user take each one back from the History page (`/history`), from the Undo of the toast shown right after it, or, for a rename, from the title's own page.

## Requirements

### Requirement: Every file-changing operation is recorded
Each finished rename batch, move batch, delete, permanent delete, artwork change and match change (Scrape with another match) SHALL be recorded as one History entry with its time, kind, the titles it touched with their old and new names or paths, a one-line summary, a muted detail line and what Undo needs. Entries SHALL survive restarts and SHALL never be removed automatically.

#### Scenario: Entry after a rename
- **WHEN** the user renames 12 titles at 14:02
- **THEN** History lists "Renamed 12 titles" at 14:02 with "12 folders, 41 files" and the library folders involved

#### Scenario: Kept across a restart
- **WHEN** the user changes the poster of Example Movie (2020), quits and starts CineVault again
- **THEN** History still lists "Changed poster · Example Movie (2020)" with Undo

#### Scenario: Finished batch without an entry
- **WHEN** the app stopped between the end of a rename or move batch and its History entry
- **THEN** the entry is written at the next start

### Requirement: Entry summaries
Summaries SHALL read "Renamed <title>" or "Renamed N titles"; "Moved <title or N titles> from <drive> to <drive>" (or "Moved … to <destination>" when the titles came from several places); "Deleted <title or N titles> to the Recycle Bin" (the Trash on macOS) or "… to the trash folder"; "Permanently deleted <title or N titles>"; "Changed <artwork type> · <title>"; and "Changed match · <old title>" with "<old title> → <new title>" as its detail.

#### Scenario: Delete summary
- **WHEN** the user deletes Example Movie (2020) to the Recycle Bin
- **THEN** History lists "Deleted Example Movie (2020) to the Recycle Bin" with its folder and size

### Requirement: History page
The History page SHALL list entries newest first, grouped by day ("Today", "Yesterday", then dates), with the tabs "All", "Renames", "Moves", "Deletes" and "Artwork and matches". Each entry SHALL show a neutral icon for its kind (a permanent delete in the danger colour), the summary, the detail line, the time and one action. The header SHALL show "<size> in Recycle Bin": what CineVault sent to the bin that is still there.

#### Scenario: Filter to deletes
- **WHEN** the user chooses the Deletes tab
- **THEN** only delete entries are listed, the ones in the bin with Restore and the permanent ones with "No undo"

#### Scenario: Bin total
- **WHEN** 214.6 GB of what CineVault deleted is still in the Recycle Bin
- **THEN** the header reads "214.6 GB in Recycle Bin"

#### Scenario: Nothing yet
- **WHEN** no entry exists
- **THEN** the page reads "Nothing here yet" and says that renames, moves, deletes, artwork changes and new matches are listed there

#### Scenario: Entry actions
- **WHEN** entries are listed
- **THEN** each shows one of "Undo", "Undo all" (several titles), "Restore" (a delete), "Undo the rest" with "N of M undone" (partly undone), "Undone", or "No undo" with its reason

#### Scenario: Older entries
- **WHEN** more than 40 entries exist
- **THEN** the first 40 are listed and "Show older entries" reads the next ones, and a change made anywhere in the app shows at once

### Requirement: A batch entry expands to its titles
An entry that touched more than one title SHALL expand to its first five titles with their old → new names or paths, and a "Show N more" link SHALL list the rest in place ("Show less" folds them).

#### Scenario: Expand a rename batch
- **WHEN** a rename entry covers 12 titles and the user expands it
- **THEN** five old → new pairs show, and "Show 7 more" lists the other seven under them

### Requirement: Undo a batch
Undo SHALL take the whole entry back as one batch: renamed titles return to their old names, moved titles to their old place, artwork to the image that was there before, a match change to the old match with its NFO and artwork, and deleted titles from the Recycle Bin, Trash or trash folder to their old paths, back in the library with their details, artwork, stage and history and without a rescan. An undone entry SHALL stay listed as "Undone at <time>".

#### Scenario: Undo a move
- **WHEN** the user undoes "Moved 3 titles from D: to E:"
- **THEN** the three titles are back in their folders on D:, the library points at them, and the entry reads "Undone at <time>"

#### Scenario: Restore a deleted title
- **WHEN** the user chooses Restore on "Deleted Example Movie (2020) to the Recycle Bin"
- **THEN** its folder returns to its old path and the title is back in the library with its details, artwork and stage

### Requirement: Undo checks each title first
Undo SHALL check every title of the entry before changing anything for it. A title that changed since, or whose files cannot all be put back (gone from the bin, the old path taken, the drive offline), SHALL be left as it is and reported with its reason, nothing SHALL be overwritten, and the other titles SHALL be undone. The entry SHALL then read partly undone.

#### Scenario: One title changed since
- **WHEN** the user undoes a rename batch of 12 and one of the folders was renamed by hand afterwards
- **THEN** 11 titles are taken back, a message names the twelfth with its reason, and the entry offers "Undo the rest"

### Requirement: Undo of an older entry respects newer ones
Undoing an entry SHALL NOT silently undo a later change to the same title. When a later entry changed the same title, Undo SHALL leave that title as it is with the reason `Undo "<later entry>" first.`

#### Scenario: Rename after a move
- **WHEN** a title was moved at 13:40 and renamed at 14:02, and the user undoes the move
- **THEN** that title is left in place and the message says `Undo "Renamed 12 titles" first.`

### Requirement: Permanent deletes have no Undo
A permanent delete SHALL be recorded before anything is erased, listed with its titles and size, drawn with the danger colour, and SHALL show "No undo"; an Undo request for it SHALL be refused with "A permanent delete cannot be undone."

#### Scenario: Permanent delete entry
- **WHEN** the user permanently deleted two titles
- **THEN** History lists "Permanently deleted 2 titles" with their size and "No undo"

### Requirement: Undo after files were erased
A delete, match or artwork entry whose files were erased afterwards SHALL show "No undo" with the reason, and an Undo request for it SHALL be refused with that reason. A rename whose leftovers were erased SHALL stay undoable without them, and a move whose originals are gone SHALL stay undoable by copying the title back from its new place.

#### Scenario: Delete emptied from the trash folder
- **WHEN** a delete went to `D:\Movies\.deletedByCineVault` and the user emptied the trash of D:
- **THEN** its entry reads "No undo" with "Its files were emptied from the trash folder"

#### Scenario: Reasons
- **WHEN** an entry lost its Undo
- **THEN** its reason is "Its files were emptied from the trash folder", "Its files were emptied from the Recycle Bin" or "Its files were erased: Deleted files are set to Delete permanently"

### Requirement: Renamed row on the title page
The Technical card of a movie that CineVault renamed SHALL show a "Renamed" row with "from" and the old name, the time, and an "Undo" button, while that rename is the newest change of the title and still stands. That Undo SHALL undo only this title's part of the batch, and the batch entry in History SHALL then say "1 of 12 undone".

#### Scenario: Undo one title's rename
- **WHEN** the Movie page of Example Movie (2020) shows "Renamed from Example.Movie.2020.2160p" with Undo, and the user clicks Undo
- **THEN** only that title's folder and files take their old names back, and the batch entry in History says "1 of 12 undone"

### Requirement: Undo toasts point to the same record
The Undo in the toast right after a rename, move or delete SHALL undo the same History entry, so undoing from the toast or from History has the same result and marks the entry undone.

#### Scenario: Undo from the toast
- **WHEN** the user deletes a title and clicks Undo in the toast
- **THEN** the title is restored and its History entry reads "Undone at <time>"

### Requirement: Activity page
History SHALL be shown on the Activity page (`/activity`), and the address `/history` SHALL lead there. While work runs, the top of the page SHALL show "Running now" with each job, move and rename as the sidebar's Running work list shows it (its line, its progress, the item being read, and its controls). While something needs the user, the page SHALL show "Needs attention" with how many items it holds, above History; it SHALL scroll on its own when it is long, so History stays in view.

#### Scenario: Nothing needs the user
- **WHEN** nothing runs, no batch is unfinished and no warning or error waits
- **THEN** the Activity page shows History only

#### Scenario: A scan runs
- **WHEN** a scan has read 312 of 707 titles and the user opens Activity
- **THEN** "Running now" shows "Scanning 312 of 707 titles" with the title being read and the scan's Pause and Cancel

#### Scenario: Old address
- **WHEN** the user opens `/history`
- **THEN** the Activity page opens

### Requirement: Needs attention lists unfinished batches
Needs attention SHALL list first the unfinished moves and renames read from the journal, each with where it was going, its titles, how far it got, when it started, why it stopped, and Resume and Roll back. A batch being resumed SHALL show its progress with Pause and Cancel instead.

#### Scenario: Resume from Activity
- **WHEN** Needs attention lists "Move to F:\Movie did not finish" with "8 of 10 files copied, 15 GB of 86 GB" and the user chooses Resume
- **THEN** the row shows the move's progress with Pause and Cancel, and when it finishes the row goes and a toast offers Undo

#### Scenario: Open in Activity
- **WHEN** the user chooses "Open Activity" on the toast of a stopped move
- **THEN** Activity opens with that move's row in view and the focus on its Resume

### Requirement: Needs attention lists warnings and errors
After the unfinished batches, Needs attention SHALL list the warnings and errors that background work reported, newest first, each with its detail, source, time, its fix when it has one (open the provider's key, retry the IMDb dataset, open the titles that could not be renamed in Rename) and Dismiss. "Dismiss all" SHALL clear them and leave the unfinished batches. Information messages SHALL NOT be listed. Warnings and errors SHALL last for the session.

#### Scenario: Dismiss a warning
- **WHEN** the user dismisses the warning "Artwork could not be saved"
- **THEN** it leaves Needs attention, and Needs attention goes when nothing else is left

#### Scenario: Dismiss all keeps unfinished moves
- **WHEN** Needs attention lists an unfinished move and two warnings, and the user chooses "Dismiss all"
- **THEN** the warnings go and the move stays with Resume and Roll back

#### Scenario: Titles that could not be renamed
- **WHEN** a rename could not rename two titles
- **THEN** Needs attention lists "2 titles could not be renamed" with each title and its reason, and "Open in Rename" opens the rename preview of those two titles

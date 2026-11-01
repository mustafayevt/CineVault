## MODIFIED Requirements

### Requirement: Where deleted files go
Settings › Deleted files (`files.deleted-mode`, written atomically) SHALL choose where Delete, a move's originals, rename leftovers and artwork or NFO files replaced by an artwork or match change go: "Recycle Bin" (`bin`, the default), "Trash folder on each drive" (`folder`), "Custom trash folder…" (`custom`) or "Delete permanently" (`erase`). An unknown value SHALL count as "Recycle Bin". There SHALL be one Delete, and it SHALL follow this choice: no menu, button or key offers a permanent delete of its own.

#### Scenario: Recycle Bin choice
- **WHEN** the choice is "Recycle Bin"
- **THEN** Delete and a move's originals go to the system bin (the drive's trash folder where there is none), and rename leftovers and replaced files go to the drive's trash folder

#### Scenario: Trash folder chosen
- **WHEN** the choice is "Trash folder on each drive" and the user deletes Example Movie (2020) from `D:\Movies`
- **THEN** its folder is in `D:\Movies\.deletedByCineVault\<batch>\Example Movie (2020)`, nothing is in the Recycle Bin, and Undo brings it back

#### Scenario: Choice changed later
- **WHEN** a title was deleted to the Recycle Bin and the choice is then changed
- **THEN** its Undo still finds it in the Recycle Bin and restores it

### Requirement: Delete permanently as the choice
Choosing "Delete permanently" SHALL first ask "Erase deleted files at once?" with "Cancel" and "Erase at once", and SHALL show the red warning "Deleted and replaced files are erased at once. Undo can't bring them back." With it on, a Delete SHALL be a permanent delete without Undo, and renames and moves SHALL stay undoable (a rename without its erased leftovers, a move by copying back). In a browser, permanent delete SHALL be off unless it is enabled in Settings › Server, and a bin/trash delete SHALL show no permanent-delete dialog in either surface.

#### Scenario: Cancelled confirmation
- **WHEN** the user picks "Delete permanently" and cancels the confirmation
- **THEN** the choice stays as it was

#### Scenario: Delete with the choice on
- **WHEN** the choice is "Delete permanently" and the user deletes a title in the desktop app and confirms
- **THEN** its files are erased, the toast says "Erased <title>: Deleted files are set to Delete permanently", and a match or artwork entry whose replaced files were erased has no Undo

#### Scenario: Move with the choice on
- **WHEN** the choice is "Delete permanently" and a title moves to another drive
- **THEN** each original is erased only after its copy is verified

#### Scenario: Browser permanent delete off
- **WHEN** permanent delete has not been enabled in Settings › Server and a browser asks to erase a title
- **THEN** the request is refused and the UI offers the configured deleted-files destination

#### Scenario: Bin delete
- **WHEN** the user deletes a title to the configured bin or trash folder
- **THEN** no permanent-delete dialog is shown and the delete is undoable

## REMOVED Requirements

### Requirement: Shift+Delete asks for a typed confirmation
**Reason**: There is one Delete, which follows Settings › Deleted files; Shift+Delete and the "Delete permanently…" item are gone, and a permanent delete asks Yes or No instead of a typed word.
**Migration**: Choose "Delete permanently" in Settings › Deleted files to erase on Delete; see "A permanent delete asks Yes or No".

## ADDED Requirements

### Requirement: A permanent delete asks Yes or No
A Delete while Settings › Deleted files is "Delete permanently" SHALL open the one confirmation dialog: "Delete N titles permanently?", a line saying the folders and files are erased and that "This cannot be undone", the list "What will be erased" with each title's path and size, and the buttons "Cancel" and "Delete permanently". There SHALL be nothing to type. Esc and Cancel SHALL erase nothing.

#### Scenario: Confirm
- **WHEN** the dialog lists two titles and the user clicks "Delete permanently"
- **THEN** the two titles are erased

#### Scenario: Cancel
- **WHEN** the dialog is open and the user presses Esc
- **THEN** the dialog closes and nothing is erased

#### Scenario: Shared folder in the list
- **WHEN** a listed title shares its folder or sits in a library folder's root
- **THEN** its row says "Only its own files in this shared folder" or "Only its own files; the library folder stays"

### Requirement: A browser permanent delete needs the password
In a browser the permanent-delete dialog SHALL also ask for the current password, and the server SHALL mint a one-time intent bound to the listed item IDs before the request is sent. A missing, reused, expired or mismatched intent, or a wrong password, SHALL be refused before any file is touched.

#### Scenario: Browser confirmation
- **WHEN** a browser confirms a permanent delete of two titles
- **THEN** it sends the current password and the server's one-time intent, and the two titles are erased

#### Scenario: Replayed intent
- **WHEN** the same intent is sent again
- **THEN** the server refuses it and no file is touched

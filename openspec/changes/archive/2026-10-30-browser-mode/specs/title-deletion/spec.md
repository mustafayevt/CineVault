# Spec Delta

## MODIFIED Requirements

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

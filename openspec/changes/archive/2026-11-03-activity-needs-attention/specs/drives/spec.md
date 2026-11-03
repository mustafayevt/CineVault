## MODIFIED Requirements

### Requirement: Unfinished moves are recovered
A move left unfinished by a stop, a quit or a crash SHALL be found in the journal at the next start, before any new file work. It SHALL be resumable (copied files checked again by size and hash, the rest copied) or able to be rolled back (removing only the `.cvpart` files, copies and staging folder the batch made). A resume SHALL continue a half-written `.cvpart` from where it ended when its bytes match the same bytes of the original, and copy the file from the start otherwise; the finished file SHALL be read back and checked whole. A new move of such a title SHALL be refused until then.

A move that the app's close or a crash cut short, with no error recorded, whose destination drive is reachable and whose original folder is there, SHALL resume by itself once the window is up, one batch after the other, with a toast that leads to Activity; its progress, Pause and Cancel SHALL show in the sidebar's progress card and in Activity. Every other unfinished move or rename SHALL be reported with a warning naming its titles and reason, with a toast that opens Activity at it, and SHALL wait in Activity's Needs attention with Resume and Roll back.

The Move to menu of a title held by an unfinished move SHALL say where that move was going and how far it got, and offer "Resume that move", "Roll it back" and "Open in Activity". After Roll back the menu SHALL stay open and read its destinations again.

#### Scenario: App closed mid-copy
- **WHEN** the app is closed after copying 3 of 9 files across volumes, and started again with both drives connected
- **THEN** all 9 originals are intact, a toast says "Resuming the move of <title>", the move carries on from the fourth file, and when it finishes the toast offers Undo

#### Scenario: Move stopped on an error
- **WHEN** a move stopped because the destination was full
- **THEN** it is not resumed by itself; a toast says the move stopped with "Open Activity", and Needs attention lists "Move to <folder> did not finish" with its titles, "3 of 9 files copied", the reason and Resume and Roll back

#### Scenario: Drive not connected at start-up
- **WHEN** the app is started after a move was cut short and the destination drive is not connected
- **THEN** nothing is resumed or written, and the move waits in Needs attention

#### Scenario: Fix from the Move to menu
- **WHEN** the user opens Move to for a title an unfinished move holds and chooses "Roll it back"
- **THEN** only what that move made is removed, the menu stays open, and the destinations can be chosen again

#### Scenario: Resume continues a half-copied file
- **WHEN** the app was closed with 15 GB of an 85 GB file written to its `.cvpart`, and the move is resumed
- **THEN** the 15 GB are checked against the original, only the remaining 70 GB are copied, and the whole file is read back and matched before it takes its name

#### Scenario: Half-copied file no longer matches
- **WHEN** the original changed after the app was closed mid-copy, and the move is resumed
- **THEN** the part is removed and the file is copied from the start

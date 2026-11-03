## MODIFIED Requirements

### Requirement: Unfinished moves are recovered
A move left unfinished by a stop, a quit or a crash SHALL be found in the journal at the next start, before any new file work, and reported with a warning naming its titles and reason. It SHALL be resumable (copied files checked again by size and hash, the rest copied) or able to be rolled back (removing only the `.cvpart` files, copies and staging folder the batch made). A resume SHALL continue a half-written `.cvpart` from where it ended when its bytes match the same bytes of the original, and copy the file from the start otherwise; the finished file SHALL be read back and checked whole. A new move of such a title SHALL be refused until then.

#### Scenario: App closed mid-copy
- **WHEN** the app is closed after copying 3 of 9 files across volumes
- **THEN** all 9 originals are intact, and at the next start a warning says the move did not finish and the originals are untouched

#### Scenario: Resume continues a half-copied file
- **WHEN** the app was closed with 15 GB of an 85 GB file written to its `.cvpart`, and the move is resumed
- **THEN** the 15 GB are checked against the original, only the remaining 70 GB are copied, and the whole file is read back and matched before it takes its name

#### Scenario: Half-copied file no longer matches
- **WHEN** the original changed after the app was closed mid-copy, and the move is resumed
- **THEN** the part is removed and the file is copied from the start

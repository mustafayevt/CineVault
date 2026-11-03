## ADDED Requirements

### Requirement: Moves are off in metadata-only mode
While Settings › Naming › "Metadata only" is on, "Move to…" SHALL be hidden from the Library selection bar, the right-click menu, the title page and the Drives selection bar, and a move asked for anyway SHALL be refused with "Metadata only is on in Settings › Naming, so files and folders are not renamed or moved." before anything touches the disk. Resuming or rolling back a move that did not finish, and the Undo of a finished move, SHALL keep working.

#### Scenario: Move refused
- **WHEN** Metadata only is on and a move of a title to another library folder is sent from a browser
- **THEN** the move is refused with the reason and no file is copied

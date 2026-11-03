## ADDED Requirements

### Requirement: Activity page
History SHALL be shown on the Activity page (`/activity`), and the address `/history` SHALL lead there. While something needs the user, the top of the page SHALL show "Needs attention" with how many items it holds, above History; it SHALL scroll on its own when it is long, so History stays in view.

#### Scenario: Nothing needs the user
- **WHEN** no batch is unfinished and no warning or error waits
- **THEN** the Activity page shows History only

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
After the unfinished batches, Needs attention SHALL list the warnings and errors that background work reported, newest first, each with its detail, source, time, its fix when it has one (open the provider's key, retry the IMDb dataset) and Dismiss. "Dismiss all" SHALL clear them and leave the unfinished batches. Information messages SHALL NOT be listed. Warnings and errors SHALL last for the session.

#### Scenario: Dismiss a warning
- **WHEN** the user dismisses the warning "Artwork could not be saved"
- **THEN** it leaves Needs attention, and Needs attention goes when nothing else is left

#### Scenario: Dismiss all keeps unfinished moves
- **WHEN** Needs attention lists an unfinished move and two warnings, and the user chooses "Dismiss all"
- **THEN** the warnings go and the move stays with Resume and Roll back

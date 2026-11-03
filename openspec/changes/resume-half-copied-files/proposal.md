## Why

When CineVault is closed while a large file is being copied to another drive, Resume throws away the part already written and copies the whole file again (issue #69): 15 GB of an 85 GB file written to a USB drive are lost and the full 85 GB are copied again. The Move dialog also shows "0 B" for a title an unfinished move blocks, so the size line reads "Move 1 title · 0 B to …".

## What Changes

- Resume continues a half-written `.cvpart` from where it ended: the bytes already written are checked against the same bytes of the original (SHA-256), and only the rest is copied. A part that does not match, or is longer than the original, is removed and the file is copied from the start. The finished copy is read back and checked whole as before.
- The Move dialog shows the size of a title an unfinished move blocks.

## Out of scope

- Resuming interrupted moves automatically at start-up, and showing unfinished moves with Resume and Roll back in the app: that needs the Activity page with Needs attention (issue #70), so a background copy at start-up can be seen and cancelled. Issue #69 stays open for it.
- Same-volume moves and renames (they write no part files).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `drives`: Unfinished moves are recovered. Resume continues a half-written file instead of copying it again.

## Impact

`cv-app` (`library_ops::move_engine` copy step and the `MoveFs` disk trait, `library_ops::change_source` move plan), tests. No IPC, settings or UI code change.

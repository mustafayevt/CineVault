## Context

`MoveEngine::copy_file` writes each file to `<name>.cvpart` in the batch's hidden staging folder, hashing the original as it reads it, then flushes the part, reads it back past the cache and compares size and hash before renaming it into place. A part left over from an earlier run is deleted before the copy starts.

## Decisions

**Continue the part, check its bytes first.** When a part exists and is not longer than the original, the engine reads the part from its start and the original's first `n` bytes (`n` = the part's length), hashing both. The original's bytes go into the same SHA-256 state the copy uses, so the original is still read only once. When the two prefix hashes agree, the rest of the original is piped and appended to the part (`MoveFs::open_append`), and the copy finishes as before: flush, read back the whole file, compare its size and hash with the whole original's.

When they differ (the original changed, or the disk kept something else), the part is removed and the file is copied from the start, as today. A part longer than the original, or a folder with the part's name, is handled the same way.

**Why not trust the part's length alone.** A part's bytes were never verified: the run may have stopped in the middle of a write, or the original may have changed since. Hashing the prefix costs one read of each, which is far less than writing it again to a slow drive, and the read-back check of the whole file stays.

**Disk trait.** `MoveFs::open_append` opens an existing file to write at its end. Its default returns `Unsupported`, which the engine treats like a part that does not match (copy again), so a test disk that does not add it keeps today's behaviour.

**Progress.** The bytes kept count as done for the move's progress and time left; the prefix reads show as checking.

## Interruption and undo

Nothing about the original changes: it is only read. A stop while the prefix is checked or the rest is appended leaves a part in the staging folder, which the next Resume continues or the Roll back removes, as before. A mismatching part is only ever a file of this batch in its own staging folder. Undo of the finished move is unchanged.

## Move dialog size

`plan_row` returns as soon as it finds an unfinished move of the title, before it lists the folder, so the row's size stays 0. The folder is listed first and the size set before the check, so the dialog shows the real size next to the reason the title is blocked.

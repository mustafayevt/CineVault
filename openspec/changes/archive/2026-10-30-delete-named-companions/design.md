## Context

`RemovalService::delete_with_files` and `RemovalService::erase` share `paths_to_trash`: the folder of a title with its own folder, otherwise the files the library records for the title (and everything below it). The library only knows what the last Scan found, so files added since then are missed in shared folders, library-folder roots and season folders.

A Scan already decides which files of a shared folder belong to which title by name: in a multi-movie folder each video takes the files whose name equals or continues its base name after a separator, longest base name first (`walk/movies.rs`), and in a season folder a file goes to the episode with the longest matching video name (`walk/tv.rs`). Files in sub-folders of a multi-movie folder belong to no movie by name.

## Goals / Non-Goals

**Goals:** take the files named after a title at delete time, with the Scan's name rules, never a file of another title; count them in the preview; undo them with the rest.

**Non-Goals:** a second, looser matcher; sub-folders of a shared folder; changing what a Scan groups.

## Decisions

### One name rule, shared with the Scan

`cv_scan::companion` holds `stem`, `normalized_base` and `is_sibling` (moved from `walk/movies.rs`, which re-exports them for the walkers) and adds:

- `video_bases(name)`: the lower-case base names of a video (its stem, and the stem without a part marker), the names the walkers match companions against;
- `is_plain_video(name)`: `classify` says video and not a disc file (a trailer or sample by name is a companion, not a video);
- `owner_by_name(name, bases)`: among `(base, owner)` pairs, the owner of the longest base the file's stem equals or continues; `Ambiguous` when different owners hold that longest base.

### Who competes for a name

For each folder that holds one of the title's videos (only that folder, no sub-folders; for a title that has a folder, only folders inside it), the bases are:

- the title's own videos (from the library) → the title;
- every other video the library records in that folder, for any other item → another title;
- every video on disk in that folder the title does not own (even if the library does not know it) → another title.

A file on disk is taken when it is a regular file (not a folder, not a link), not a video, not recorded by the library for any other item, and `owner_by_name` gives the title. `Ambiguous` and another owner leave it. Comparing names folds case the way a Scan does (always), and comparing with the library's records follows the platform (`cv_fs::paths::fold_name`).

The library side is one query, `cv_db::repo::library_ops::files_in_folder`, run while the titles are loaded; the folder is listed right after, outside the read.

### Where it plugs in

`load` keeps the found files on `Loaded::companions` (path and size) for titles whose folder use is not `Own`. `row_of` adds them to the row's file count and bytes (the preview), and `paths_to_trash` appends them after the known files, with the same "inside the library folder" check. The delete to the bin, its History entry and Undo, and the permanent delete with its record-first entry all work on that list, so nothing else changes.

## Risks / Trade-offs

- **A leftover named after a title that is gone** (`Heat 2.en.srt` with no `Heat 2` video on disk or in the library) continues `Heat` after a space and goes with `Heat`. It goes to the Recycle Bin or Trash with Undo, as everything a delete takes; a permanent delete lists it in the dialog's count first.
- **Files appear between the preview and the delete:** the delete lists the folder again, so a file added in between goes too; its History entry has it, so Undo brings it back.

## Safety and Undo

Nothing new writes, moves or erases: the extra paths join the list the existing send-to-bin and erase steps already handle. A delete to the bin sends path by path and records every sent path in the one History entry, also when it stops part way, so Undo restores each extra file to its old path and never overwrites. A permanent delete records every path before erasing, as before. An interrupted delete leaves the files not yet sent in place.

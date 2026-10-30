## Why

When a title shares its folder with other titles, or sits directly in a library folder, Delete sends only the files the library knows for it. Files added after the last Scan stay behind: a subtitle (`Example Movie (2020).en.srt`), a poster (`Example Movie (2020)-poster.jpg`) or an NFO dropped next to the video and deleted before a rescan are left in the folder as orphans. People expect Delete to remove everything about the title, as it already does for a title with its own folder.

## What Changes

1. **Files named after the title go with it.** For a title in a shared folder or in a library folder's root, and for an episode, Delete and permanent delete also take every file in the folder of each of its videos whose name is the video's base name or continues it after `.`, `-`, `_` or a space: subtitles in any language or variant (`.en.srt`, `.en.forced.srt`, `.sdh.srt`, `.ass`, `.sup`, `.idx` and `.sub`), NFO, artwork (`-poster.jpg`, `-fanart.jpg`, `-clearlogo.png`, `.jpg`), trailers (`-trailer.mkv`), chapter and other sidecar files. They are found on disk at the time of the delete, with the same name rules a Scan uses to give the files of a shared folder to its titles.
2. **Never another title's file.** A file the library records for another title is never taken. A name that also continues another video's base name goes to the longest base name (`Example Movie (2020) Director's Cut.en.srt` belongs to the Director's Cut, `Heat 2.en.srt` to `Heat 2.mkv`, `Movie.1080p.en.srt` to `Movie.1080p.mkv`); when another title's video has the same base name, the file is left where it is and the delete goes on. Another video is never taken by name. Only the files directly in the video's folder are looked at; no sub-folder is entered.
3. **Counted and undoable.** The extra files are part of the delete's preview (the file count and size of each title), of the same History entry, and of Undo, which puts them back. Delete still goes to the Recycle Bin or Trash (or the library folder's trash folder) by default; only a permanent delete erases them.

A title with its own folder is unchanged: its whole folder already goes.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `title-deletion`: a delete of a title in a shared folder, in a library folder's root, or of an episode also takes the files named after its videos, never another title's.

## Out of scope

- Sub-folders of a shared folder (a `Subs` folder next to several movies): a Scan gives them to no title in a shared folder, so a delete leaves them too.
- Files whose name does not start with the video's base name (`poster.jpg`, `folder.jpg`, a subtitle named differently from its video): they belong to the folder, not to one title.
- Remove from library (without files) is unchanged.
- The wording of the Delete and permanent-delete dialogs; only their counts and sizes grow.

## Impact

- `crates/cv-scan`: a public `companion` module with the name rules the walkers already use (base name, separator, longest base name first) and the owner of a file name among several videos.
- `crates/cv-db`: `files_in_folder` (every file the library records directly in a folder).
- `crates/cv-app`: `library_ops/removal.rs` (the files named after a title are found when titles are loaded and added to the preview row, the delete and the permanent delete), a new `library_ops/companions.rs`.
- Tests: `cv-scan` unit tests of the name owner, `cv-app` tests over temporary folders for a shared folder, a library folder's root and an episode, the collision cases, the preview, Undo and permanent delete.
- `CHANGELOG.md` and `site/changelog.html`.

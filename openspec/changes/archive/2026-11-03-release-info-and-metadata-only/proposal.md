## Why

People who let a download manager fetch and upgrade their files want CineVault for metadata, NFO files and artwork, but a rename today drops what the download name said about the release: its quality, its tags and the group that made it (issue #61). A download manager that reads the file again may then score it differently and fetch an "upgrade" it does not need. The release group cannot be recovered from the video once it is gone from the name, so the renamer has to carry it over from the name the file came with. Others want CineVault never to rename or move their files at all, and today there is no switch that guarantees it.

## What Changes

- Three new naming tokens for movies and episodes, read from the name the video had when it was first scanned (the original file name, which every title keeps): `releaseGroup` (the group after the last `-`, or in a leading `[Group]`, only when the name clearly has one), `releaseQuality` (the source and resolution in the common `Source-Resolution` form, such as `Bluray-1080p` or `WEBDL-2160p`, with `Proper` or `Repack` when the name says so) and `releaseTags` (the release part of the name as written, such as `Bluray-1080p.German.DL.x265`). A token is empty when the name does not say it; nothing is guessed.
- A fourth naming preset, **Release info**: Clean folders, and file names that keep the release tags and group (`Example Movie (2020) [Bluray-1080p.x265]-GROUP`). It is offered in Settings › Naming and as a naming style of the first run.
- Settings › Naming offers the tokens as {Quality}, {ReleaseTags} and {ReleaseGroup} buttons.
- A **Metadata only** switch at the top of Settings › Naming (`renamer.metadata-only`, off by default). While it is on, nothing renames or moves a title's files or folders: the Rename page, the Rename and "Move to…" actions are hidden or explain why they are off, and the backend refuses a rename or a move from any front end (desktop, browser, command line, automations). Scan, Scrape, NFO and artwork writing, Delete and Undo keep working.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `renamer`: Movie and TV naming tokens (the release tokens), Naming presets (Release info), Naming templates in Settings (the new token buttons), and a new requirement for the metadata-only mode.
- `drives`: a new requirement that moves are off in metadata-only mode.
- `settings`: Naming section (the Metadata only switch).

## Out of scope

- A warning on the Rename page when a new name drops release information; the Release info preset and the tokens cover keeping it, and the metadata-only mode covers not renaming at all.
- An overwrite policy for NFO and artwork files. Replaced NFO and artwork files already go to the deleted-files destination, and the NFO stays a full, current file on every change.
- Reading languages, streaming services or other tags as tokens of their own; `releaseTags` keeps them as written.
- Undo of earlier renames and moves stays available while metadata-only is on: it only puts files back where they were.

## Impact

`cv-template` (the release-name reader and the tokens), `cv-rename` (the preset), `cv-app` (the metadata-only guard for rename, move and the automation rename step), `cv-ipc` (the guard's error), the UI (Settings › Naming, the token buttons, the sidebar, the library, title, copies and Rename page actions, the first-run naming styles), messages, `CHANGELOG.md`. New compatibility fixtures for the tokens and the preset; existing expected outputs do not change. No database migration: the original file name is stored already.

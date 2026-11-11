## Why

After a Scrape, every title still has to go through the Rename page, even when its new names plainly fit and nothing stands in the way (#101, asked for in #99). On a large library that is a second pass over hundreds of titles for no decision at all.

## What Changes

- **Rename after Scrape.** Settings › Naming gets a switch, `renamer.after-scrape`, off by default. While it is on, a title a scrape was written into is renamed by itself once its NFO and artwork are written.
- **Only plain changes.** The rename is the batch engine's rename step over those titles: titles whose plan is a plain change are renamed; titles in a conflict, locked titles, titles whose video details are not read yet and titles that are already tidy are left alone.
- **One batch, one Undo.** The titles scraped close together (no new one for 3 seconds) are renamed as one job per kind of title, journaled like any rename. A toast says "Renamed 12 titles after Scrape" with Undo, wherever the user is; History lists the batch.
- **What waits.** Titles held back are listed in Activity › Needs attention ("2 titles were not renamed after Scrape") with each reason, to settle on the Rename page. A rename step that cannot run at all (a naming template with a problem) says why there too.
- **Metadata only wins.** While Metadata only is on, the switch is held off and shown as such, and nothing is renamed.
- The rule that no background step ever renames, and that no setting makes renaming automatic, gains this one opt-in exception.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `renamer`: new requirement *Rename after Scrape*.
- `title-lifecycle`: *Rename only when asked* allows the opt-in Rename after Scrape.
- `title-matching`: *Scrape is the one verb for matching* keeps the folder unless Rename after Scrape is on.

## Impact

- `cv-app`: `rename_settings` (the key and `rename_after_scrape`), `actions::rename_after` (the queue), the after-scrape steps push into it, `automation::after_scrape_rename` (the runner and its notices, installed by `CommandBackend::start`), `NoticeAction::UndoRename`. Tests: the queue (paused time), the notices, the setting, and an end-to-end scrape that renames.
- UI: the switch in `NamingSection.tsx`; `useBackendNotices.ts` turns the notice into a toast with Undo (the shared `undoRename` of `runRename.ts`); i18n; tests.
- No IPC command, DTO or schema change; the notice's action is a new value of an existing string.

## Out of scope

- Renaming after a scrape run by the command line without a running app (there is no batch runner there).
- A per-title or per-library choice; it is one switch.

## Why

With 700 titles in eight library folders on a network share, Scan is slow and says little (#99). Changing one title means scanning every drive again. The progress counts library folders ("Scanning 0 of 8 · 0%"), so it sits at zero for minutes and looks frozen, with no idea of the time left. After a rename it is not clear whether it worked or is still running, and titles that could not be renamed only show up as a generic error.

## What Changes

- **Scan one place.** Each library folder on a Drives card gets "Scan this folder", and Settings › Library folders gets the same for each folder. The Movie page and the TV show page get "Scan again" in the ⋯ menu and in the files notice (missing files or unreachable folder). Scan again reads only the title's own folder (its files, NFO, artwork and the media info of new or changed videos) and notices that its files are gone or back. Titles in other folders, and other titles in the same library folder, are never read, compared or marked missing. The full "Scan folders now" stays.
- **Progress in titles.** A scan first counts the titles of the folders it will read ("Finding titles… 312 found", with the folder being listed), then reports each title as it is read ("Scanning 312 of 707 titles", with the title being read). Once enough titles were timed it adds "about 3 min left"; before that, and when the estimate would be absurd, it says nothing about time. The sidebar's progress card, its Running work list and Activity show the same. These reports reach the UI at most four times a second.
- **Activity shows running work.** While jobs run, the Activity page lists them at the top under "Running now", with the same lines and controls as the sidebar's list.
- **Rename feedback.** While a rename runs, the sidebar's progress card shows "Renaming 3 of 12" with the title just done. When it ends, a toast says "Renamed 12 titles" with Undo, wherever the user is by then. When some titles could not be renamed, the toast says "Renamed 10 of 12 titles; 2 need attention" with Undo and "Open Activity", and Activity › Needs attention lists them with each reason and "Open in Rename", which opens the rename preview of those titles to try again.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `library-scanning`: *Scans are resumable jobs* (folder and title items, names); new requirements *Scan one library folder*, *Scan one title again* and *Scan progress in titles*.
- `drives`: *Drive cards* (Scan this folder per library folder).
- `title-pages`: *Title page actions* (Scan again in the ⋯ menu), *Files that cannot be reached* (Scan again in the notice; builds on the text of the `remove-missing-titles` change).
- `job-system`: *Progress card* (titles of a scan, rename progress), *Throttled progress feed* (unit reports at most four a second).
- `renamer`: *Rename page footer and result* (toast wherever the user is, partial result, sidebar progress).
- `operation-history`: *Activity page* (Running now), *Needs attention lists warnings and errors* (failed renames with Open in Rename).

## Impact

- `cv-jobs`: a job can report progress in its own unit (`UnitProgress`, `ItemControl::set_units`) and count its work before the first item (`ItemWork::prepare`); `JobSnapshot.units`.
- `cv-scan`: the walk can read one folder of a library folder (`WalkScope::focus`) and tells its titles to an observer (`WalkObserver`); `count_titles`; `rescan_folder`; `RescanTarget::Folder`; the rescan job's progress in titles (`rescan::progress`). Availability after a scan is probed only for the titles in scope.
- `cv-db`: `plan_availability_of`, availability for chosen titles only.
- `cv-app`: `ScanService::scan_folder`, `ScanService::rescan_title`; `OrganizeService::apply_with` reports progress.
- IPC: new commands `rescan_folder`, `rescan_title`; `organize_apply` takes a progress channel; `JobProgress.units`. Bindings regenerated; the browser server dispatches the same.
- UI: Drives card, Settings › Library folders, title pages, the files notice, the sidebar progress, Activity, the rename flow and its toasts, i18n messages, tests.
- Stored jobs: a rescan job item can now be a folder; older stored scan jobs still resume.

## Out of scope

- Renaming right after a scrape (#101).
- Scanning several selected titles at once from the Library; Scan again is on the title page.
- A command-line "scan one folder"; the CLI keeps its full scan.
- Pausing or cancelling a rename from the sidebar: a rename runs title by title and is short; it shows its progress only. Undo stays journal based.
- Faster walking of network shares; this change makes the walk visible and lets the user scan less of it.

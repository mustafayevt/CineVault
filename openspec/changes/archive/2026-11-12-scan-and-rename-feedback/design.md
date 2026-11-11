## Context

A scan is a persistent job on the disk lane with one item per library folder (`cv-scan::rescan::job`). Each item walks its whole folder (`cv-scan::walk`), plans the differences against the stored titles and fingerprints, writes them in one transaction and recomputes availability. The job's progress is its items, so eight folders read "0 of 8" until the first whole folder is done, which on a network share takes minutes. The walk is where the time goes; comparing and writing are quick. After each folder, availability was recomputed by probing the files of every title in the library, not only those of the folder.

`ScanService::rescan_source` already scanned one library folder (the folder watcher uses it), but nothing in the UI offered it, and there was no way to read one title's folder only.

A rename (`OrganizeService::apply`) runs inside the `organize_apply` command, title by title through the rename executor and its journal. The Rename page awaited it and showed a toast; failures went to the notification centre as one error. Nothing showed while it ran.

## Goals / Non-Goals

**Goals:** scan one library folder or one title without reading the rest; progress that moves, counted in titles, with an honest estimate; a rename that says when it is done and what failed, wherever the user is; nothing that works today changes.

**Non-Goals:** a second scanner; scanning a selection of titles; pausing or cancelling a rename; a faster network walk.

## Decisions

### One scanner, narrowed by a focus

"Scan again" goes through the same walk and the same planner as a full scan. `WalkScope::focus` names one folder inside the library folder. The walk lists the library folder and each folder on the way down as usual (skip rules, marker files and nested library folders apply), but keeps only the way down at those levels, without their own files; the focused folder and everything below it are read whole. The grouping rules then see exactly the folder tree they would see in a full walk, so a title is grouped, named and paired as a full scan would do it (a movie in a collection folder, a show inside an A to Z folder).

The planner compares only what the walk read: the stored movies and shows whose folder is inside the focus (with their seasons and episodes) and the fingerprints below it (`load_state` with a focus). Titles outside the focus are not in the snapshot, so they are not paired, not relinked, and their fingerprints are not dropped. Availability is recomputed for the items of the plan only, so nothing outside the focus can become Missing.

Emptiness is judged on the library folder's own listing, not on the narrowed tree: a library folder that still holds other titles is not "empty" because the focused folder is gone, so the title becomes Missing; a library folder that is really empty (a drive that is not mounted) is still not compared at all.

A movie that sits directly in its library folder has no folder of its own; Scan again then scans the library folder. An episode or season scans its show.

Alternatives: walking the title folder as if it were a library folder would group differently (its files would be "loose files in a root") and could rename titles; a separate small scanner would duplicate the rules. Both were rejected.

### Availability is probed for the titles in scope only

`cv-db::plan_availability_of` takes a filter and probes only the wanted items. A folder scan probes its folder's titles; a title scan its own. This keeps "Scan again" from touching the disk of every title, and makes a full scan probe each title once instead of once per library folder. The changes it writes are the same as before, since the earlier code already filtered the changes to the scope.

### Progress in titles: count, then read

`cv-jobs` gets two general additions: `UnitProgress` (phase, done, total, current, estimate) set through `ItemControl::set_units` and carried in `JobSnapshot.units`, and `ItemWork::prepare`, run once before the first item with the items the run will process.

The rescan job's `prepare` lists every library folder of the job side by side (`count_titles`, one listing of the folder and of its grouping folders) and reports "Finding titles… N found" with the folder still being listed. It waits at most 15 seconds; a folder that has not answered (a dead share) is counted by its own walk later. Then each walk reports through a `WalkObserver`: a folder directly in the library folder is one title, a grouping folder (movie bucket names such as `A`, `1999` or `Horror`; one-character TV folders) counts its folders, and in a movie library each loose video counts. The count and the walk use the same rules, so the total rarely moves; when it does, it only grows, and a finished library folder counts all its titles as done, so the bar never moves back and ends at 100%.

"Titles" here are folders as the walk sees them, which is what a title is in the usual layouts. A library folder that is itself one show counts its season folders; this is accepted.

The estimate is shown once 10 titles were done over at least 5 seconds of reading, from the pace since the reading began; it is hidden when nothing is left or when it would exceed a day. The UI rounds it ("about 3 min left", "less than a minute left").

Reports are throttled at the source (`REPORT_INTERVAL`, 250 ms; a phase change and the end of a folder are sent at once), so at most four a second reach the job; the progress feed's own 10 Hz limit stays.

### Rename progress and feedback

`OrganizeService::apply_with` calls back before the first title and after each, from the executor's existing per-item hook. `organize_apply` takes a progress channel, like `apply_change_source`. The UI runs a rename in a module-level function (`runRename`), not in the Rename page's hook, so its progress (a "running rename" entry in the sidebar's progress card, "Renaming 3 of 12") and its result toast do not depend on the page being open. Pause and Cancel are not offered for it.

The toast: "Renamed 12 titles" with Undo; with failures "Renamed 10 of 12 titles; 2 need attention" with Undo and "Open Activity"; nothing renamed: "12 titles could not be renamed" as an error toast with "Open Activity". Failures go to Needs attention as one error notice listing each title with its reason and the action "Open in Rename", which opens `/rename?ids=…` for exactly those titles; the user retries from the preview, which shows the current plan and conflicts.

### Running work in Activity

The sidebar's list of running work is extracted into a shared list and shown at the top of the Activity page ("Running now") while something runs, so scans and renames show the same lines in both places.

## Safety

- **Scans** only read the media folders; they write only the library database, in one transaction per folder, as before. A focused scan changes only the stored titles inside its folder. No title is removed by a scan; a folder that is gone makes its titles Missing, a library folder that cannot be reached makes nothing Missing. An interrupted folder or title scan is a persistent job item like a library folder: it runs again on the next start.
- **Renames** still go through the planner, preview and journal; `apply_with` only adds a callback. Undo stays journal based, from the toast and from History. An interrupted rename is recovered as before (Activity › Needs attention, Resume or Roll back).
- Metadata only mode still refuses `organize_apply`, and the UI keeps Rename hidden; Scan again and Scan this folder are not renames and stay.

## Risks / Trade-offs

- Counting lists each library folder once more before the walk. On a network share this is one listing per folder (and per grouping folder), small next to the walk; it is cut off after 15 seconds.
- Stored jobs: a rescan item can now be `{"target":"folder",…}`. An older version of the app reading it would mark that job unresumable; nothing else is affected.
- The title count is folder based, so a library with unusual layouts shows a slightly different number than the Library's title count.

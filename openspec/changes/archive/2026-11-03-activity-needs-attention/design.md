## Context

The journal and `RecoveryService` already find, resume and roll back unfinished moves and renames, and the IPC commands `list_unfinished_batches`, `resume_unfinished_batch` and `roll_back_unfinished_batch` exist for both the desktop app and the browser. The UI never called them, and `useNotificationStore` was never rendered.

## Decisions

**One page, Activity.** History is renamed Activity and keeps its list; Needs attention sits above it in its own scroll area (at most 45% of the page), so History stays in view. A separate notification centre was rejected: two lists would each tell half the story.

**The journal is the truth about unfinished batches.** Needs attention lists the batches from `list_unfinished_batches`. Once that list is read, notices about unfinished batches (`action: resolve_unfinished`) are not listed separately: they would repeat the row, or be out of date. A batch being resumed is not listed by the backend while it runs (`is_running`), so the window keeps the batch it resumes and shows that row with its progress.

**Start-up resume runs in the window, not in the backend.** A resume needs somewhere to show its progress, Pause, Cancel and Roll back; that is the window. `UnfinishedBatch::auto_resume` says a batch may carry on unattended: a move whose open titles have no `stopped:` reason (the app was closed or crashed), whose destination drive is reachable (on Linux, mounted) and whose original folder is there (or, past the commit, whose new folder is there). The window lists the batches once after it opens and resumes those, one after the other, with a toast. The start-up warning is not sent for them. A batch that stopped on an error keeps its warning and waits.

**Progress reuses the move's state.** A resumed move fills `useMoveToStore.running` (with its batch id), so the sidebar's progress card, the Drives page and the Needs attention row all show the same progress, Pause and Cancel, and another move is refused while it runs, as before.

**Sizes come from the originals.** `bytes_total` is the sum of the originals' sizes of the open titles (`None` when one cannot be read); `bytes_done` is the copied and verified part. They are read when the batches are listed, from the source drive only.

**Move to menu.** The menu finds the unfinished move that holds a title of the request by the batch's `item_ids` and shows its fix above the destinations. Roll back keeps the menu open and reads the plans again, so the move can be chosen next.

## Safety

Nothing new touches the disk: Resume and Roll back are the existing, tested operations (copies checked by size and hash, only what the batch made removed, originals untouched until the copy is verified). The start-up resume only starts when the batch was cut short and every folder it reads and writes is reachable; a drive that is not there leaves the batch waiting. Cancel during a resume takes the title back as for a move (before the copy is verified) or finishes it (after). A resume or roll back that stops again leaves the batch unfinished and listed. A resumed move gets its History entry from the journal, so it can be undone from the toast or History.

## Risks

- A browser window opened while the desktop app is closed could start the resume of a move cut short on the server; that is the same "carry on" the desktop window does, and only one move runs at a time across surfaces.
- Warnings are not kept across restarts; the unfinished batches that matter come back from the journal.

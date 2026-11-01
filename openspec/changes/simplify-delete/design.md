## Context

The desktop has two paths into a permanent delete: Shift+Delete or "Delete permanently…" (always erases, after a typed DELETE), and Delete while Settings › Deleted files is "Delete permanently" (the same dialog). The backend repeats the typed word as a guard: `erase_titles(ids, confirm)` and `empty_drives_trash(drive, confirm)` refuse anything but `DELETE`. In a browser the server adds the current password and a one-time intent bound to the item IDs.

## Goals / Non-Goals

**Goals:** one Delete that follows the setting; a Yes or No wherever files are erased; no change to what is erased, in what order, or how it is recorded.

**Non-Goals:** the deleted-files destinations, Undo, automatic emptying, and the browser's password and intent.

## Decisions

- **Shift+Delete does what Delete does** rather than nothing, so a habit from other apps never surprises: it follows the setting, and under "Recycle Bin" it still goes to the bin with Undo.
- **The typed word leaves the backend too.** With no field in the UI, a fixed `"DELETE"` sent by the frontend would guard nothing. `erase_titles` and `empty_drives_trash` lose the `confirm` argument and the two constants. The real guards stay: the dialog's explicit "Delete permanently" / "Empty for good" button on the desktop, and in a browser the setting, password and one-time intent checked before the History entry is written.
- **One label, "Delete".** The destination depends on the setting, so the menu no longer names the Recycle Bin; the toast and History say where the files went, as they do today.

## Risks / Trade-offs

- A permanent delete is one click less hard to reach. It is only reachable when the user chose "Delete permanently" in Settings (itself confirmed), and the dialog still lists every path and size before anything is erased.

## Safety and undo

Nothing changes in how files are erased: the History entry is written first with every path, titles are erased one by one with retries for files in use, paths outside a library folder or holding a library folder's root are refused, and only titles erased whole leave the library. A bin or trash delete keeps its Undo. An interrupted permanent delete leaves the History entry listing what was erased and what was left, as today.

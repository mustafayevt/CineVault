## Why

Deleting a title offers two entries everywhere: Delete, which follows Settings › Deleted files, and a second "Delete permanently…" (also Shift+Delete) that always erases after the user types DELETE. The setting already decides where deleted files go, including "Delete permanently", so the second entry repeats it in a harsher form, doubles every menu and makes the user type a word where a Yes or No does the job.

## What Changes

- **BREAKING** One Delete everywhere (the selection bar, the right-click menu, the title page's ⋯ menu and the Delete key). It does what Settings › Deleted files says. The "Delete permanently…" entries are gone, and Shift+Delete does the same as Delete.
- When Deleted files is "Delete permanently", Delete opens the permanent-delete dialog as today (the list of what will be erased, with sizes), but it asks Yes or No: "Cancel" and "Delete permanently", with nothing to type. "Send to Recycle Bin instead" goes away with the second entry.
- Choosing "Delete permanently" in Settings, and "Empty…" / "Empty all trash folders…" on the Drives page, ask Yes or No in the same way.
- In a browser nothing is weakened: a permanent delete still needs Settings › Server to allow it, the current password and the server's one-time intent.
- The menu label is "Delete" instead of "Delete to Recycle Bin", because where the files go depends on the setting; the toast still says where they went.

Out of scope: where deleted files go and the four choices themselves, Undo and History, the automatic emptying of trash folders, the "Ask before deleting" option for bin and trash deletes, and the browser's password and intent checks.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `title-deletion`: "Where deleted files go", "Delete permanently as the choice"; "Shift+Delete asks for a typed confirmation" is replaced by "A permanent delete asks Yes or No" and "A browser permanent delete needs the password".
- `desktop-interaction`: "Browser keys are off", "No single-key shortcuts", "Own right-click menus", "Delete keys".
- `library-browsing`: "Keys on the selection", "Floating selection bar", "Title right-click menu".
- `title-pages`: "Title page actions".
- `ui-design-system`: "One verb glossary".
- `drives`: "Empty a drive's trash" is replaced by "Empty trash folders with a Yes or No".

## Impact

- UI: `ui/src/features/delete/` (the actions, the permanent-delete dialog and its tests), the library and title-page action lists (`features/library/titleActions.ts`, `TitlesView.tsx`, `useLibraryActions.ts`, `features/title/titleActions.tsx`), the Drives empty dialog, Settings › Deleted files and the English messages.
- Backend: `erase_titles` and `empty_drives_trash` drop their typed-word argument (`ERASE_CONFIRMATION`, `EMPTY_CONFIRMATION`); `ui/src/bindings.ts` is regenerated. The browser's intent and password checks in `cv-web` stay.
- Docs: AGENTS.md (the Shift+Delete rule and the list of dialogs), CONTRIBUTING.md if it repeats the rule, the settings wording, `CHANGELOG.md`, and the README, landing page and screenshots where a menu with "Delete permanently…" is shown.

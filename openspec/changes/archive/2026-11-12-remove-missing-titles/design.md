## Context

`removeTitlesFromLibrary` (`ui/src/features/title/removeTitles.ts`) already calls `remove_from_library` with any number of ids and answers with a toast. The ⋯ menu, the right-click menu and the selection bar's More all use it. In the backend (`cv-app` `library_ops/removal.rs`) it forgets the title's rows; unlike Delete, nothing is hidden under a History entry, so there is nothing to undo. Files stay on disk and a scan re-adds the title.

## Decisions

- **Reuse the action, add entry points.** The notice calls `removeTitlesFromLibrary([id], false, title)` and, on success, navigates to the Library, exactly as the menu item does. No new command.
- **Missing, not offline.** Only the `missing` state shows the button. An offline title is expected to return, so the notice does not invite removal.
- **One reason line.** The body text of the missing notice replaces "Point it at the folder the files moved to, or review every title that lost its files" (which named things the page does not offer) with why the title is kept and what removal does.
- **Selection bar.** `LibrarySelectionBar` gets a `promoteRemove` flag that the Library sets when the Problems filter is "Missing files or offline". The action then shows on the bar, before Delete, as "Remove from library" (the full "(keep files)" name stays in menus), and is left out of More so it is not listed twice. Elsewhere the bar is unchanged.
- **No Undo.** AGENTS.md asks for an Undo toast for reversible actions. Removal has no backend undo and building one is out of scope; the toast keeps confirming with its existing words, which say the next scan adds the title back.

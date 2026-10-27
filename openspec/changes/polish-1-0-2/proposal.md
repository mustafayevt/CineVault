## Why

Version 1.0.1 is the first public release, and daily use shows a set of rough edges: table columns that do not move or resize smoothly, a title-bar search that only works on Enter, a Stage column that is hard to read, a Date added column that shows the first-scan time instead of when the file was created, and a few features the Settings page promises but the app does not deliver (release notes, optional artwork-provider keys, an aspect ratio wider than the frame). This change collects them into one 1.0.2 polish release.

## What Changes

1. Library table: columns reorder by a pointer drag with a lifted header, sliding neighbours and a drop marker (Esc cancels), and resize from a wide edge with a live width; double-clicking an edge fits the column to its content; "Move left" and "Move right" in the header menu are the keyboard path; Title stays first.
2. Spotlight-style search: typing in the title-bar search shows grouped instant results (titles, shows, people), with keyboard selection, recent searches and "See all results".
3. Stage column: a labelled header and readable "New", "Scraped" and "Renamed" tags, no pointer cursor, a problem colour with a tooltip; the grid's tick gets the app tooltip.
4. Title page header: Delete moves into the last place of the ⋯ menu (red, after a divider); "Change artwork" joins the header buttons.
5. Date added is the main file's creation time on insert, with a one-time repair for rows that share the first-scan time.
6. Poster size: a persisted size control in the grid view.
7. Artwork picker: a big preview opened from an expand button, a double-click or Space; any click or Esc closes it.
8. TV episode rows open and close with a short height and fade animation (instant under reduced motion).
9. Settings › Providers and onboarding step 3 take optional Fanart.tv and TheTVDB keys, so the "needs a key" link in search leads somewhere.
10. The update prompt and Settings › Updates show the release notes of the offered version.
11. Aspect ratio detection reports ratios wider than the frame correctly.

Item 4 of the original batch (the permanent-delete confirmation) already landed and is not part of this change.

## Out of scope

- New providers, new columns or new views beyond the Stage column labels.
- Changing the NFO or artwork file formats.
- Any change to how files are renamed, moved or deleted.
- Auto-install of updates, release channels other than the current one.

## Impact

- UI: `ui/src/components/Table.tsx`, `tableLayout.ts`, `ui/src/features/library/*`, title-bar search, title page header, artwork picker, TV show page, Settings and onboarding.
- Rust: `cv-db` (date added repair, migration), `cv-scan` (creation time), `cv-media` (aspect ratio), `cv-providers` (optional keys), the updater command (release notes).
- Specs: `library-browsing`, `title-pages`, `artwork`, `tv-shows`, `metadata-providers`, `settings`, `release-and-updates`, `media-info`.

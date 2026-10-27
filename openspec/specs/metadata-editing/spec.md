# metadata-editing Specification

## Purpose
How users correct and curate a title's metadata: the Edit details page of a movie (`/movies/$movieId/edit`) or a TV show (`/tv/$showId/edit`) with its save bar and Undo, the field and title locks that keep values from being changed by scraping, the NEW marker, and taking titles out of the library without touching their files.

## Requirements

### Requirement: Edit details page
"Edit details" in a title page's ⋯ menu, or in the Library's selection bar and right-click menu for one title, SHALL open a page, not a dialog, titled "Edit details" with the title's name and a back button "Back to the title". It SHALL edit the stored fields of the movie or show in sections, each shown only when the title has fields for it. Tags and trailer addresses SHALL NOT be shown or edited, and saving SHALL keep them unchanged.

#### Scenario: Tags kept
- **WHEN** the user edits a movie whose NFO holds tags and a trailer address, and saves
- **THEN** no section offers tags or trailers, and both stay unchanged in the NFO

### Requirement: Edit details sections
The sections SHALL be "Titles" (title, original title, sort title, tagline), "Plot" (plot, outline), "Release and file" (year, dates, status, runtime, certification, edition, media source, 3D), "Genres and places" (genres, studios, countries, languages), "IDs, ratings and notes" (IDs, ratings with votes, your rating, Top 250 rank, collection, note), "Cast and crew" (add, remove, reorder), "Artwork addresses" (the NFO's image links) and, for a show, "Seasons" (each season's name and overview).

#### Scenario: Fields of a movie
- **WHEN** the user opens Edit details on a movie
- **THEN** the page shows the Titles, Plot, Release and file, Genres and places, IDs ratings and notes, Cast and crew and Artwork addresses sections

#### Scenario: Seasons of a show
- **WHEN** the user opens Edit details on a show with two seasons and specials
- **THEN** a Seasons section offers the name and overview of Specials, Season 1 and Season 2

### Requirement: Field input
Fields SHALL take values in their own form: lists of genres, studios, countries and languages as chips with suggestions ("Add and press Enter"), countries and languages by name, dates as year-month-day, and numbers within their range. A movie's collection SHALL be chosen from the existing collections or "None". Cast rows SHALL hold a name and a character, crew rows a name and a job.

#### Scenario: Add a genre
- **WHEN** the user types "Drama" in Genres and presses Enter
- **THEN** a Drama chip is added to the draft and counted as one change

### Requirement: Field checks
The year SHALL be from 1800 to 2200 or empty, the runtime whole minutes, the Top 250 rank from 0 to 250 or empty, and your rating from 0.5 to 10 in half steps; each rating source SHALL be listed once. Every field that cannot be saved SHALL be marked with a plain message at once and nothing SHALL be written; an empty title SHALL be refused with "Give it a title. A title cannot be empty."

#### Scenario: Bad year
- **WHEN** the user types 3000 as the year and presses Save
- **THEN** the field says "Enter a year from 1800 to 2200, or leave it empty." and nothing is saved

#### Scenario: Rating listed twice
- **WHEN** two rating rows name the same source
- **THEN** the page says that source is listed twice and nothing is saved

### Requirement: Save bar and leaving with changes
Nothing SHALL be written until Save. While the page has unsaved changes, a bar docked at its foot SHALL count them ("3 changes") with "Discard" and "Save" (Ctrl+S, ⌘S on macOS), and say how many fields need attention when some are refused. Leaving the page with unsaved changes SHALL keep the page and ask in the same bar, "Leave with 3 unsaved changes?", with "Keep editing", "Discard and leave" and "Save and leave"; Esc SHALL mean Keep editing.

#### Scenario: Back with changes
- **WHEN** the user edits a field and presses Back without saving
- **THEN** the page stays and the bar asks to keep editing, discard and leave, or save and leave

### Requirement: Saving rewrites the NFO and offers Undo
Saving SHALL write every changed field in one transaction, rewrite the title's NFO files with the NFO settings, return to the title page and show a toast "Saved N changes to <title>" with Undo, which saves the previous values back. A failed NFO write SHALL NOT undo the save: the app SHALL report "Saved <title>, but its NFO files could not be written" with the reason, and a later Rewrite NFO fixes it.

#### Scenario: Edit and save
- **WHEN** the user changes the sort title on Edit details and presses Ctrl+S
- **THEN** the sort title is saved, the NFO is rewritten, the Movie page shows again and a toast "Saved 1 change to Example Movie" offers Undo

#### Scenario: Undo a save
- **WHEN** the user presses Undo on that toast
- **THEN** the old sort title is saved back and the NFO is rewritten again

### Requirement: Field and title locks
A field SHALL be lockable when a scrape can write it, and a whole title SHALL be lockable. A locked field SHALL NOT be changed by Scrape, or by reading the NFO again, and a locked title SHALL be left unchanged by Scrape with "This title is locked, so it was not changed." Saving on Edit details SHALL never be blocked by a lock: a lock protects values from machines, not from the user. Locked titles SHALL be found with the library search term `is:locked`.

#### Scenario: Locked genres
- **WHEN** the genres of a movie are locked and the user scrapes it again
- **THEN** the genres stay as they were and the other fields are updated

#### Scenario: Locked title in a batch
- **WHEN** a Scrape of several titles includes a locked title
- **THEN** that title is skipped with the locked reason and the others are scraped

### Requirement: Watched state is kept, not edited
The app SHALL NOT show or change whether a title or episode was watched in any button, menu, filter, sort, column or form. The stored watched flag and play count SHALL be kept and written back to the NFO unchanged.

#### Scenario: Watched round-trips untouched
- **WHEN** a movie whose NFO has a play count of 3 is edited and its NFO rewritten
- **THEN** the NFO still holds the play count 3 and its watched flag

### Requirement: NEW marker
A title SHALL be marked NEW when a scan adds it, and opening its Movie or TV show page SHALL clear the marker. With the setting "Reset NEW when updating folders" (`library.reset-new-on-update`, off by default), every NEW marker SHALL be cleared when an update of the library folders starts, so only what that update finds is marked. Titles marked NEW SHALL be found with the library search term `is:new`.

#### Scenario: Cleared on open
- **WHEN** a scan adds "Example Movie (2020)" and the user opens its page
- **THEN** the search `is:new` does not list the title

#### Scenario: Reset on update
- **WHEN** the setting is on and the user starts a scan of the library folders
- **THEN** every earlier NEW marker is cleared and only titles the scan adds are marked

### Requirement: Remove from library without touching files
"Remove from library (keep files)" SHALL take the chosen titles out of the library and leave every file on disk, so a later scan adds them back, and say "Removed <title> from the library. Its files stay; the next scan adds it back." After a removal from a title page, the Library SHALL open.

#### Scenario: Remove from library only
- **WHEN** the user chooses "Remove from library (keep files)" on a movie
- **THEN** it leaves the library, its files stay on disk, and the next scan adds it back

### Requirement: Ignore this folder
"Ignore this folder" SHALL remove the titles as "Remove from library (keep files)" does and also write a `.cinevaultignore` marker into each title's own folder, so scans skip it. A folder that is not the title's own (shared with other titles) SHALL NOT get a marker, and the app SHALL say "Removed <title>, but its folder could not be marked, so a scan may add it back."

#### Scenario: Shared folder
- **WHEN** the user chooses "Ignore this folder" on a movie whose folder holds other movies too
- **THEN** the movie leaves the library, no marker is written, and the message says a scan may add it back

#### Scenario: Ignore this folder
- **WHEN** the user chooses "Ignore this folder" on a home video in its own folder
- **THEN** it leaves the library, a `.cinevaultignore` marker is written in its folder, and later scans skip it

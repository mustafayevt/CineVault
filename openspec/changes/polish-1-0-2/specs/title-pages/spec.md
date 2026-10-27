## MODIFIED Requirements

### Requirement: Title page actions
The header of the Movie page and the TV show page SHALL show the buttons Play, Scrape, Rename, Move, Open folder and "Change artwork", and a "More actions" menu (⋯) with "Edit details", "Show NFO", "Rewrite NFO", "Reload media info", "Remove from library (keep files)", "Ignore this folder" and, after a divider and in red, "Delete permanently…" and "Delete…" as the last item, with the names, icons and order of the Library's selection bar and right-click menu.

#### Scenario: More actions
- **WHEN** the user opens the ⋯ menu of a movie
- **THEN** it lists Edit details, Show NFO, Rewrite NFO, Reload media info, Remove from library (keep files), Ignore this folder, a divider, a red Delete permanently… and a red Delete… last

### Requirement: What the header actions do
Scrape SHALL open the Scrape match page of the title, Rename the rename preview, Move the "Move to" menu and "Change artwork" the artwork picker. "Delete…" in the ⋯ menu SHALL open the deletion confirmation of `title-deletion`. When the title's files are missing or its library folder cannot be reached, Play SHALL be off with the reason, and Open folder SHALL be off with "This title has no folder" for a title without one.

#### Scenario: Scrape from the page
- **WHEN** the user clicks Scrape on a scraped movie
- **THEN** the Scrape match page of that movie opens

#### Scenario: Offline title
- **WHEN** the library folder of a movie is on a drive that is not connected
- **THEN** Play is off with "Play is off: the library folder cannot be reached."

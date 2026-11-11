## MODIFIED Requirements

### Requirement: Title page actions
The header of the Movie page and the TV show page SHALL show the buttons Play, Scrape, Rename, Move, Open folder and "Change artwork", and a "More actions" menu (⋯) with "Edit details", "Show NFO", "Rewrite NFO", "Reload media info", "Scan again", "Remove from library (keep files)", "Ignore this folder" and, after a divider and in red, "Delete…" as the last item, with the names, icons and order of the Library's selection bar and right-click menu. "Scan again" SHALL scan that title's own folder only (see `library-scanning` "Scan one title again") and confirm with the toast "Scanning <title>". A title with a TMDB ID SHALL also have, before "Remove from library (keep files)", "Find a better copy" on the Movie page and "Find missing episodes" on the TV show page, which open the Torrents page with the title picked and the "Only better than mine" or "Fills a gap" chip on, followed by "Add to Wishlist", which opens the Torrents Wishlist with the editor filled in for the title.

#### Scenario: More actions
- **WHEN** the user opens the ⋯ menu of a movie with a TMDB ID
- **THEN** it lists Edit details, Show NFO, Rewrite NFO, Reload media info, Scan again, Find a better copy, Add to Wishlist, Remove from library (keep files), Ignore this folder, a divider and a red Delete… last

#### Scenario: Scan again from the menu
- **WHEN** the user chooses "Scan again" in the ⋯ menu of "Example Movie (2020)"
- **THEN** the toast says "Scanning Example Movie (2020)", only that movie's folder is read, and the page shows what was found when the scan ends

#### Scenario: Find missing episodes
- **WHEN** the user chooses "Find missing episodes" in the ⋯ menu of a show with a TMDB ID
- **THEN** the Torrents page opens with that show picked and the "Fills a gap" chip on

#### Scenario: Add to Wishlist
- **WHEN** the user chooses "Add to Wishlist" in the ⋯ menu of a movie with a TMDB ID
- **THEN** the Torrents Wishlist opens with the editor for that movie

### Requirement: Files that cannot be reached
A title page whose files are missing or whose library folder is offline SHALL show one calm notice with an icon and words: "The files for this title are missing" or "This title's library folder cannot be reached", with what to do and the last known folder. Everything known about the title SHALL stay on the page. Both notices SHALL have a "Scan again" button that scans the title's folder (see `library-scanning` "Scan one title again"), so files that are back are found at once. The notice for missing files SHALL say why the title is kept (the drive may be offline or the files only moved) and SHALL also have a "Remove from library (keep files)" button that does what the ⋯ menu item does, then opens the Library. The notice for an unreachable library folder SHALL NOT have the remove button.

#### Scenario: Drive away
- **WHEN** a movie's drive is disconnected and its page opens
- **THEN** the page says "This title's library folder cannot be reached", shows the last known place, and the facts, plot and artwork are still shown, with "Scan again" and no "Remove from library (keep files)" button

#### Scenario: Remove a title whose files are gone
- **WHEN** a movie or a TV show whose files are missing opens and the user chooses "Remove from library (keep files)" in the notice
- **THEN** the title leaves the library, its files stay, the toast says so, and the Library opens

#### Scenario: Files put back
- **WHEN** the files of a movie that shows the missing-files notice are put back and the user chooses "Scan again" in the notice
- **THEN** the movie is available again and the notice goes

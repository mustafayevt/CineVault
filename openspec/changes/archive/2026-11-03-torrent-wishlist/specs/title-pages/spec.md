## MODIFIED Requirements

### Requirement: Title page actions
The header of the Movie page and the TV show page SHALL show the buttons Play, Scrape, Rename, Move, Open folder and "Change artwork", and a "More actions" menu (⋯) with "Edit details", "Show NFO", "Rewrite NFO", "Reload media info", "Remove from library (keep files)", "Ignore this folder" and, after a divider and in red, "Delete…" as the last item, with the names, icons and order of the Library's selection bar and right-click menu. A title with a TMDB ID SHALL also have, before "Remove from library (keep files)", "Find a better copy" on the Movie page and "Find missing episodes" on the TV show page, which open the Torrents page with the title picked and the "Only better than mine" or "Fills a gap" chip on, followed by "Add to Wishlist", which opens the Torrents Wishlist with the editor filled in for the title.

#### Scenario: More actions
- **WHEN** the user opens the ⋯ menu of a movie with a TMDB ID
- **THEN** it lists Edit details, Show NFO, Rewrite NFO, Reload media info, Find a better copy, Add to Wishlist, Remove from library (keep files), Ignore this folder, a divider and a red Delete… last

#### Scenario: Find missing episodes
- **WHEN** the user chooses "Find missing episodes" in the ⋯ menu of a show with a TMDB ID
- **THEN** the Torrents page opens with that show picked and the "Fills a gap" chip on

#### Scenario: Add to Wishlist
- **WHEN** the user chooses "Add to Wishlist" in the ⋯ menu of a movie with a TMDB ID
- **THEN** the Torrents Wishlist opens with the editor for that movie

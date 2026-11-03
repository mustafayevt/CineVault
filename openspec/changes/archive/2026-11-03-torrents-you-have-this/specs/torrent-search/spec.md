## MODIFIED Requirements

### Requirement: Resolution chips with counts
Above the results the page SHALL show the chips All, 2160p, 1080p, 720p and SD (480p and unknown resolutions count as SD), each with the number of releases it holds, and choosing one SHALL list only those releases. Before the first release arrives the chips SHALL show no counts and SHALL NOT be selectable. While the library holds a copy the releases are compared with, an "Only better than mine" chip SHALL follow them, and for a show the library holds episodes of, a "Fills a gap" chip; each switches on and off on its own, with its count, and narrows the chosen resolution further. Nothing left by them SHALL say "No releases left by this filter." with "Clear filters".

#### Scenario: Only 2160p
- **WHEN** the user clicks the 2160p chip showing 18
- **THEN** 18 releases are listed, all 2160p

#### Scenario: Only better than mine
- **WHEN** the library holds the movie at 1080p WEB-DL and the user switches on "Only better than mine"
- **THEN** only the releases tagged "Better" are listed

### Requirement: Same, better or worse than yours
When the library holds the searched title (a movie with a video file, or a show with at least one episode on disk), a "You have this" card SHALL show above the results with the poster, the title and year linking to its Movie or TV show page, and what the library holds. For a movie it SHALL list each copy on one line (resolution, HDR, video codec, audio, source, edition and size, as far as the media info knows them), best first, and with several copies say that releases are compared with the first. For a show it SHALL say which seasons are held and which episodes are missing ("You have S01–S03 · missing S04E05–E08"). It SHALL offer "Open in library", "Look for a better copy" (a movie) or "Find missing episodes" (a show), which switch on the "Only better than mine" or "Fills a gap" chip, and "Search anyway", which folds the card into one line for that title. Each release compared with a copy SHALL carry a tag "Same as yours", "Better" or "Worse" after its name; its tooltip SHALL say why ("Better: 2160p Dolby Vision vs your 1080p SDR"), and choosing it SHALL open "Compare with yours": the copy and the release in two columns, row by row (resolution, HDR, codec, audio, source, edition, size), the rows that differ marked, ending in a plain verdict ("Better picture, same audio, 2.3× the size"). The tag, the tooltip and the verdict SHALL come from one comparison, and what the library holds SHALL come from the library alone.

#### Scenario: A better release
- **WHEN** the owned copy is 1080p SDR and a release is 2160p Dolby Vision
- **THEN** that release shows "Better" with the tooltip "Better: 2160p Dolby Vision vs your 1080p SDR"

#### Scenario: Two copies
- **WHEN** the library holds the movie as a 1080p Theatrical and a 2160p Extended copy
- **THEN** the card lists the 2160p Extended copy first, then the 1080p one, and releases are compared with the 2160p copy

#### Scenario: Compare with yours
- **WHEN** the user chooses the "Better" tag of "Example.2020.2160p.UHD.BluRay.REMUX.DV.TrueHD.Atmos" while the copy is a 1080p WEB-DL with DD+ 5.1 of 6 GB and the release is 14 GB
- **THEN** a popover lists both sides row by row with resolution, HDR, source, audio and size marked as different and ends "Better picture, better audio, 2.3× the size"

#### Scenario: A show with gaps
- **WHEN** the library holds S01 to S03 of a show and its missing episodes are S04E05 to S04E08
- **THEN** the card says "You have S01–S03 · missing S04E05–E08"

### Requirement: Release details actions
The panel SHALL link the library title when there is one, list the title's other releases in the list, and offer "Send to torrent app" and "Search all releases", which shows the search of the matched title. When the matched title is a library movie with a copy, the panel SHALL show the "Compare with yours" block of the search for the release.

#### Scenario: Search the title
- **WHEN** the user chooses "Search all releases" in the panel of a matched title
- **THEN** the page shows the search of that title with its releases from every source

#### Scenario: Compare in the panel
- **WHEN** the user opens the details of a 2160p release of a movie the library holds at 1080p
- **THEN** the panel shows "Compare with yours" with both sides and the verdict

## ADDED Requirements

### Requirement: Show releases against the episodes held
For a show the library holds episodes of, a release whose name gives its episodes (an episode, an episode range, a season, a range of seasons or the whole show) SHALL carry "Fills a gap" when it covers an episode the library is missing, "In your library" when every episode it covers is held, and "Better" when every episode it covers is held and it is better than the quality most held episodes in the searched scope share. A season covers the episodes the library knows for it, held and missing.

#### Scenario: A pack fills a gap
- **WHEN** the library is missing S04E05 to S04E08 and a release is "Example.Show.S04.1080p.WEB-DL"
- **THEN** that release carries "Fills a gap" and is listed under the "Fills a gap" chip

#### Scenario: Already held
- **WHEN** the library holds every episode of season 2 at 1080p and a release is "Example.Show.S02.720p.HDTV"
- **THEN** that release carries "In your library"

#### Scenario: A season the library knows nothing of
- **WHEN** the library knows no episode of season 7 and a release is "Example.Show.S07.1080p.WEB-DL"
- **THEN** that release carries none of the three tags

### Requirement: Library chip in the address
The chip turned on by "Look for a better copy", "Find missing episodes" or the title pages SHALL be part of the page address (`only=better` or `only=gaps` next to `title=`), so a reload or a copied link opens the search with it on, and switching the chip SHALL update the address.

#### Scenario: Opened from the Movie page
- **WHEN** the user chooses "Find a better copy" on a movie's page
- **THEN** the Torrents page opens at `/torrents?title=movie:<id>&only=better` with that movie picked and "Only better than mine" on

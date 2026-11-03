## MODIFIED Requirements

### Requirement: Sidebar destinations and counts
The sidebar panel SHALL hold the brand mark with Back and Forward; the destinations Library, Scrape, Rename, Drives and Torrents; the Drives section with each drive's free space; and at its foot the progress card above the icon row Activity, Theme, Settings and collapse. Scrape and Rename SHALL show waiting counts as pills, Drives SHALL show "N low" in the warning tone, Torrents SHALL show the number of new Wishlist matches, and a zero count SHALL show no pill. Activity SHALL show a warning dot while its Needs attention has entries, and its name SHALL then say how many things need attention. The active destination SHALL have a gold icon; title and picker pages count as the Library.

#### Scenario: Counts on the nav
- **WHEN** 12 titles wait to be scraped and 71 renames are pending
- **THEN** Scrape shows "12" and Rename shows "71" as pills

#### Scenario: New Wishlist matches
- **WHEN** a Wishlist check found 3 new matches that have not been looked at
- **THEN** Torrents shows "3" as a pill, and the pill goes once the Wishlist is opened

#### Scenario: Something needs attention
- **WHEN** a move did not finish
- **THEN** the Activity icon shows a warning dot and is named "Activity, 1 thing needs attention", and the dot goes once the move is resumed or rolled back

#### Scenario: Active page
- **WHEN** the user is on the Rename page
- **THEN** only the Rename item has the active fill and the gold icon

#### Scenario: A movie page belongs to the Library
- **WHEN** the user opens a movie from the grid
- **THEN** the Library item stays active

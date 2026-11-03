## MODIFIED Requirements

### Requirement: Sidebar destinations and counts
The sidebar panel SHALL hold the brand mark with Back and Forward; the destinations Library, Scrape, Rename, Drives and Torrents; the Drives section with each drive's free space; and at its foot the progress card above the icon row History, Theme, Settings and collapse. Scrape and Rename SHALL show waiting counts as pills, Drives SHALL show "N low" in the warning tone, Torrents SHALL show the number of new Wishlist matches, and a zero count SHALL show no pill. The active destination SHALL have a gold icon; title and picker pages count as the Library.

#### Scenario: Counts on the nav
- **WHEN** 12 titles wait to be scraped and 71 renames are pending
- **THEN** Scrape shows "12" and Rename shows "71" as pills

#### Scenario: New Wishlist matches
- **WHEN** a Wishlist check found 3 new matches that have not been looked at
- **THEN** Torrents shows "3" as a pill, and the pill goes once the Wishlist is opened

#### Scenario: Active page
- **WHEN** the user is on the Rename page
- **THEN** only the Rename item has the active fill and the gold icon

#### Scenario: A movie page belongs to the Library
- **WHEN** the user opens a movie from the grid
- **THEN** the Library item stays active

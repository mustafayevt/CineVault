## MODIFIED Requirements

### Requirement: Filter row
Under the toolbar a filter row SHALL show the stage chips "All", "New", "Scraped" and "Renamed" with their counts, then the dropdown chips "Drive: <value>" (All, or a library folder by its label with its title count), "Quality: <value>" (Any, 2160p, 1080p, 720p, SD), "Problems <count>" in the warning tone and "Needs fixing", then the search chip, and at the right the count and total size of what is listed ("181 movies · 7.06 TB"). All of them SHALL combine.

#### Scenario: Drive filter
- **WHEN** the user picks the library folder labelled "Archive" in the Drive menu
- **THEN** only titles in that folder are listed and the footer counts and sizes them only

## ADDED Requirements

### Requirement: Needs fixing filter
The "Needs fixing" menu SHALL offer the checkable entries "Not scraped", "No poster", "No fan art", "No clear logo", "No plot" and "No title", each with the number of titles of the current view it would list. Ticking entries SHALL list the titles missing all of them, keep the menu open, and combine with the other chips and the search. "Not scraped" SHALL be the New stage. The chip SHALL show how many entries are ticked, and "Clear Needs fixing" SHALL untick them.

#### Scenario: One entry
- **WHEN** 12 of 181 movies have no poster and the user ticks "No poster 12"
- **THEN** only those 12 movies are listed, the footer says "12 movies" and the chip shows 1

#### Scenario: Entries together
- **WHEN** the user ticks "No poster" and "No clear logo"
- **THEN** only the titles missing both are listed

#### Scenario: With the search
- **WHEN** the search holds "the" and the user ticks "No poster"
- **THEN** only titles with "the" in a word of their name and no poster are listed, and the search chip stays

#### Scenario: Fixing in a batch
- **WHEN** an entry is ticked, the user presses Ctrl+A and chooses Artwork or Scrape on the selection bar
- **THEN** the action opens for every listed title, not only the ones scrolled into view

### Requirement: Needs fixing for TV shows
The Needs fixing menu SHALL be offered in the TV shows library too, and a show SHALL count by its own facts: it is missing a poster, fan art or clear logo when the show itself has none, whatever its episodes and seasons have, and it is missing a plot or a title when its own is empty. A title that was never scraped has the name read from its folder or file, so "No title" lists only titles whose title is empty.

#### Scenario: Show with its own poster
- **WHEN** a show has a poster of its own and none of its episodes has a thumb, and the user ticks "No poster" in the TV shows library
- **THEN** the show is not listed

#### Scenario: Show without a poster of its own
- **WHEN** a show has no poster of its own but its episodes have thumbs, and the user ticks "No poster"
- **THEN** the show is listed

### Requirement: Nothing to fix
When an entry of the Needs fixing menu is ticked and no title is listed, the list SHALL say "Nothing to fix here" with "Clear Needs fixing", which unticks the entries and keeps the other filters and the search.

#### Scenario: Nothing missing
- **WHEN** no movie has an empty title and the user ticks "No title"
- **THEN** the list says "Nothing to fix here" and "Clear Needs fixing" lists the movies again

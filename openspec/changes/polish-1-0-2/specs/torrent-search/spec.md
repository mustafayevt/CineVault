## MODIFIED Requirements

### Requirement: One search field with results as you type
The Torrents page SHALL show one search field ("Type a movie or show") that has focus when the page opens. After a pause in typing, with at least 2 characters typed, it SHALL look the text up on TMDB and drop a list of title suggestions under the field: movies and shows (documentaries included), each with its poster, name, year and type, and the titles the library holds marked "In your library". No release SHALL be searched while the user types. A release search SHALL start only when the user picks a suggestion with a click or Enter (Enter with none highlighted picks the first), and it SHALL search exactly that title, with "N results · M sources" in the field, where M counts the sources that were asked. ID shortcuts and IMDb or TMDB links SHALL work in the field.

#### Scenario: Type a title
- **WHEN** the user types "example movie" and pauses
- **THEN** a list of titles drops under the field and no release has been searched yet

#### Scenario: Pick a title
- **WHEN** the user clicks "Example Movie (2020)" in the suggestions
- **THEN** releases of Example Movie (2020) start listing, and the field shows the result and source counts

#### Scenario: Clear the search
- **WHEN** the field holds text and the user presses Esc or the clear button
- **THEN** the field is emptied and the results go

### Requirement: Result states
The results area SHALL never be an empty page. It SHALL show placeholder rows while searching and a plain state for: no title picked yet ("Type a movie or show to see its releases."), no title found ("Nothing found for “<query>”", shown in the suggestions), no releases found ("No releases found for <title>." with "Search again"), nothing left by the chosen chip ("No <tier> releases." with "Clear filters"), and every source failed ("No source answered. Check your connection and try again." with "Retry all").

#### Scenario: Nothing left by the chip
- **WHEN** the sources answered but nothing is 2160p and the 2160p chip is chosen
- **THEN** the area says "No 2160p releases." and "Clear filters" shows all results again

#### Scenario: Every source failed
- **WHEN** every asked source failed or was rejected
- **THEN** the page says "No source answered. Check your connection and try again." and "Retry all" asks the failed sources again

## ADDED Requirements

### Requirement: Picked title chip
Above the results the page SHALL name the picked title as "Releases for <title> · Movie" (or "· Show") in a chip with a "Change title" × button. Pressing it, or editing the field, SHALL drop the title and its results and leave the field ready for another title. The confirmed title SHALL be kept in the page address (`?title=movie:<tmdb id>`), so a reload opens it again.

#### Scenario: Change the title
- **WHEN** releases are listed and the user presses the × on the title chip
- **THEN** the results go, the field is empty and has focus, and nothing is searched until another title is picked

## REMOVED Requirements

### Requirement: Matched title line
**Reason**: The page no longer picks a best match by itself; the user picks a title from the suggestions, and the picked title is shown as a chip (see "Picked title chip").
**Migration**: None; the "Pick another match" menu is replaced by the suggestions list.

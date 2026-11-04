## MODIFIED Requirements

### Requirement: What a scrape writes
A scrape SHALL write the fields of the kind's scrape field selection, by default every field a scrape may write: title, original title, tagline, plot, year, release date, status (shows), ratings, Top 250 rank, runtime, certification, genres, spoken languages (movies), country, studios, tags, collection (movies), cast, crew, trailer and each artwork type. A field lock SHALL keep its field, and with "write only empty fields" on a filled field SHALL be kept. Provider IDs SHALL only be added.

#### Scenario: Only empty fields
- **WHEN** "write only empty fields" is on and a movie with a plot but no tagline is scraped
- **THEN** the tagline is filled and the plot is unchanged

## ADDED Requirements

### Requirement: Provider keywords become tags
A scrape whose field selection includes tags SHALL add the provider's keywords to the title's tags. Tags the title already has, from its NFO or earlier scrapes, SHALL be kept, and a keyword equal to one of them, ignoring case, SHALL NOT be added twice. A change of match SHALL keep the tags.

#### Scenario: Keywords added
- **WHEN** a movie whose TMDB entry has 20 keywords and whose NFO has the tag "Favourites" is scraped with the default field selection
- **THEN** it has the tag "Favourites" and the 20 keywords, and its NFO lists all 21

#### Scenario: Tags switched off
- **WHEN** Tags is cleared in "Movie fields to scrape" and a movie is scraped
- **THEN** its tags are unchanged

## REMOVED Requirements

### Requirement: Provider keywords are never tags
**Reason**: Newly scraped titles were left without tags while older titles had them from their NFOs.
**Migration**: Tags is a scrape field, on by default; clear it in the scrape field selection to keep tags as they are.

## MODIFIED Requirements

### Requirement: NFO ownership
The app SHALL classify an NFO path as missing, empty, managed (it parses with a non-empty movie, collection, show, season or episode title), pointer (only a URL or an ID) or foreign (anything else). A write SHALL never replace a foreign file. A rename SHALL set a foreign NFO of the renamed title aside in the deleted-files destination, journaled so Undo brings it back, and SHALL never carry it to an NFO name; a pointer file SHALL never be removed by a rename.

#### Scenario: Foreign NFO kept
- **WHEN** a movie folder contains a release-group `info.nfo` with no movie title
- **THEN** it is never overwritten by NFO writing

#### Scenario: Downloaded info file set aside on rename
- **WHEN** a movie folder holds the video and a plain-text `.nfo` named like it, and the movie is renamed
- **THEN** the info file goes to the deleted-files destination, the renamed folder holds no `.nfo` but CineVault's, and no old folder is left behind

#### Scenario: Pointer file written over
- **WHEN** a movie's `movie.nfo` holds only an IMDb ID and the movie is scraped
- **THEN** the NFO is written over it, and Undo brings the old text back

# movie-collections Specification

## Purpose
Covers movie collections (movie sets): grouping movies of one franchise, showing which films of a collection the library has and which it lacks, assigning movies to collections when they are scraped or edited, and storing collection artwork and NFO files in the locations the Kodi, Jellyfin and Emby formats read. It also covers the collection tokens of naming templates.

## Requirements

### Requirement: A movie belongs to at most one collection
A collection SHALL be a library entry of its own, and a movie SHALL belong to at most one collection. Only movies SHALL join collections. The films of a collection SHALL be ordered by release date.

#### Scenario: Release order
- **WHEN** a collection holds films from 1986, 1979 and 1992
- **THEN** its films are listed 1979, 1986, 1992

### Requirement: Collections view
The Library SHALL offer a Movies | TV shows | Collections switch. Collections SHALL be shown as a grid sorted by name ("Name A–Z"), each card with the collection's artwork, one short bar per film with the owned ones lit, and "<owned> of <total> owned". With no collection to show, the view SHALL say "No collections yet" and "A collection appears once you own two or more films of the same set."

#### Scenario: Empty view
- **WHEN** the library holds no two films of the same collection
- **THEN** the Collections view says "No collections yet"

### Requirement: Collection pane
Choosing a collection SHALL open a docked pane with the collection's artwork and "Change artwork" (which opens the artwork picker for the collection), its name, "<owned> of <total> owned · <size>", its overview (or "No overview yet."), and "Films in this set". Owned films SHALL be marked "In library" and open their movie page. Films the library lacks SHALL be listed muted as "Not in library" with year and a "Search torrents" link that opens torrent search for the title and year.

#### Scenario: Missing film
- **WHEN** the user owns 3 of the 5 released films of Example Collection and opens it
- **THEN** the pane reads "3 of 5 owned", the two others are muted "Not in library" with Search torrents

### Requirement: A collection is shown from two owned movies
A collection SHALL be listed only while the library holds two or more of its movies. With fewer it SHALL be hidden but kept, so it returns when a second movie arrives, and its movie SHALL keep its collection.

#### Scenario: Second film arrives
- **WHEN** the library holds one film of Example Collection and the user scrapes a second one of it
- **THEN** "Example Collection" appears under Collections

#### Scenario: One film left
- **WHEN** one of two films of a collection leaves the library
- **THEN** the collection is hidden from the Collections view and the remaining film keeps its collection

### Requirement: Automatic assignment on scrape
When a movie is scraped with the collection field among the fields the scrape writes and "Group movies into collections" on (setting `movies.collections.auto-group`, default on), the movie SHALL be put into the TMDB collection the provider names, and the collection SHALL be created from TMDB's data (name, overview, poster and fanart) when the library has none. A failure here SHALL be logged and SHALL never fail the scrape. A locked collection field SHALL keep the movie where it is.

#### Scenario: Collection created
- **WHEN** the first movie of a TMDB collection is scraped
- **THEN** the collection is created from TMDB's data and the movie joins it

### Requirement: Missing movies of a collection
When a TMDB collection is fetched, its films that the library lacks SHALL be stored with it. A film SHALL count as owned when a movie of the collection has its TMDB or IMDb ID or, without IDs, its title and year. Films not out yet (a release date after today, or neither date nor year) SHALL be left out, so counts read owned of released. The missing list SHALL be checked against the library on every read.

#### Scenario: Unreleased film
- **WHEN** a collection has 4 released films and one announced for next year
- **THEN** its total is 4 and the announced film is not listed

### Requirement: Collection on the movie page and in Edit details
A movie page SHALL show the movie's collection with "<name> · <owned> of <total> owned" in its header and a Collection fact that opens the Collections view. The Edit details page of a movie SHALL offer a Collection choice listing the library's collections and "None", so the user can move a movie into another collection or out of any.

#### Scenario: Remove from a collection
- **WHEN** the user sets a movie's Collection to "None" in Edit details and saves
- **THEN** the movie belongs to no collection

### Requirement: Collection artwork locations
Collection artwork SHALL be written in the style set by `artwork.collection-locations`: `movieset-<type>.<ext>` in each movie folder of the collection (default), Kodi style `<collection folder>/<name>/<type>.<ext>`, flat style `<collection folder>/<name>-<type>.<ext>`, or the movie-folder and Kodi styles both. The types SHALL be poster, fanart, banner, clearart, clearlogo, discart and thumb (`landscape` in the folder styles). The folder styles SHALL write nothing without a collection folder.

#### Scenario: Default location
- **WHEN** Example Collection has a poster and the default location style is used
- **THEN** `movieset-poster.jpg` is written in the folder of each of its movies

#### Scenario: Kodi style
- **WHEN** the collection folder is `D:\Collections` and the Kodi style is chosen
- **THEN** the Example Collection poster is written to `D:\Collections\Example Collection\poster.jpg`

### Requirement: Writing collection artwork safely
Movies that share one folder with other movies SHALL be left out of the movie-folder style. A collection image that exists under any image extension SHALL be kept unless the user asks to download it again, and a replaced file SHALL go to the trash folder of its data source, never be deleted. After a movie is scraped into a collection, the collection's missing artwork SHALL be filled.

#### Scenario: Existing image kept
- **WHEN** a movie folder already has `movieset-poster.png` and the collection's artwork is filled
- **THEN** the file is kept and no poster is downloaded for that folder

### Requirement: Collection NFO files
A collection NFO SHALL be written only in the collection folder (`metadata.nfo-collection-folder`, empty by default), never in a movie folder, with the file name chosen by `metadata.nfo-set-names`: "Collection name (Name.nfo)" (default), "set.nfo", "collection.nfo", or "All three". Without a collection folder, collection NFOs SHALL be skipped with the note "Set the collection folder in Settings to write collection NFOs".

#### Scenario: No collection folder
- **WHEN** no collection folder is set and a collection's NFO would be written
- **THEN** nothing is written and the note asks for the collection folder

### Requirement: Collection names on disk
The collection name in folder and file names SHALL replace each character a file name cannot hold (`\ / : * ? " < > |` and control characters) with `metadata.collection-name-replacement` (default `_`; empty removes them), drop trailing dots and spaces, and append `-tmdb-<id>` when "Add the TMDB ID to collection folders" is on. A name with nothing left SHALL get no folder-style files and no NFO. Artwork and NFO SHALL use the same name.

#### Scenario: Colon replaced
- **WHEN** a collection is named "Example: The Collection"
- **THEN** its folder is named `Example_ The Collection`

### Requirement: Collection tokens in naming
Naming templates SHALL offer `${movieSet.title}`, `${movieSet.titleSortable}`, `${movieSetIndex}` ("Place in the collection") and `${movieSetIndex2}` ("Place in the collection, missing counted"), the indexes counting from 0 in release order, the second also counting released films the library lacks. A movie in no collection SHALL print nothing for them and an empty folder level SHALL be dropped. A collection of one movie SHALL be left out of naming unless enabled in the naming options.

#### Scenario: Collection subfolder
- **WHEN** the folder template is `${movieSet.title}/${title} (${year})` and a movie belongs to a collection with two owned films
- **THEN** the movie is placed inside a folder named after its collection

#### Scenario: Movie without a collection
- **WHEN** the same template is used for a movie in no collection
- **THEN** the movie's folder sits directly in the library folder

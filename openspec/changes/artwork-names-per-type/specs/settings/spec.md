## MODIFIED Requirements

### Requirement: Naming section
The Naming section SHALL start with the "Metadata only" switch, then hold the Movie folder, Movie file and TV episode templates as mono text fields, each with its tokens and a live example of the result, the "Colon in titles" choice (Remove, Use " - ", or a look-alike colon), and the "Artwork file names" setting. These templates are the ones Rename uses, as defined by the `renamer` capability; changing them SHALL rename nothing by itself. While Metadata only is on, the templates SHALL stay editable.

#### Scenario: Live example
- **WHEN** the user edits the Movie folder template
- **THEN** the example under the field shows the new folder name, and no folder on disk changes

#### Scenario: Metadata only switch
- **WHEN** the user turns on "Metadata only"
- **THEN** the setting is saved at once and Rename and "Move to…" disappear from the app

## ADDED Requirements

### Requirement: Artwork file names in the Naming section
"Artwork file names" SHALL offer the styles Movie name in front (the default), Plain names, Both names and Jellyfin, a stored older style by its name, and Custom, with an example line of the style's poster and fanart files. Custom SHALL be shown while any per-type list is stored. Choosing Custom SHALL store the lists of the style in use for every type, so no file name changes until a row is edited.

#### Scenario: Custom starts from the style
- **WHEN** the style is Jellyfin and the user chooses Custom
- **THEN** the table shows `backdrop` on for fanart and `poster` on for the poster, and no artwork file name changes

### Requirement: Custom artwork names table
Custom SHALL show a table with a row per movie artwork type and per TV artwork type that offers a choice. Each row SHALL switch its names on and off (the name with the movie name in front, the plain name and the type's alternatives), offer a single choice for extra fanart, refuse to switch off a type's last name, and show the files its names give for the example title. Under the table a line SHALL say that a folder holding several movies always uses the names with the movie name in front.

#### Scenario: Two names for one type
- **WHEN** the user switches on `folder` next to `poster` in the Poster row
- **THEN** the setting holds both names and the row's files read `poster.jpg` and `folder.jpg`

#### Scenario: The last name stays
- **WHEN** `<name>-banner` is the only name on in the Banner row and the user clicks it
- **THEN** it stays on and nothing is saved

### Requirement: Leaving Custom and renaming existing artwork
While Custom is on, a line SHALL say that choosing a style replaces these names; choosing a style SHALL remove every per-type list and offer Undo in a toast. After a change of the names, a line SHALL say that Rename moves the existing artwork to the new names and that the next Scrape writes them.

#### Scenario: Back to a style
- **WHEN** custom names are stored and the user chooses Plain names
- **THEN** the per-type lists are removed, the table disappears and a toast offers Undo, which brings the custom names back

#### Scenario: Rename line
- **WHEN** the user changes the names of a type
- **THEN** a line says that Rename moves the artwork already on disk to the new names

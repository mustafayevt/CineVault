# title-pages Specification

## Purpose
The pages of one title: the Movie page (`/movies/$movieId`) and the TV show page (`/tv/$showId`). It covers the shared header with its artwork, ratings and actions, the facts and plot, the Technical card with its video, audio, subtitle and file rows, the NFO and "Renamed from" lines, the cast, the artwork strip, the seasons and the episodes opened in place.

## Requirements

### Requirement: Movie page layout
Opening a movie SHALL show one scrolling page with no tabs: the header, then on the left the plot, the facts, the tags and the cast, on the right the Technical card, and under them the artwork strip. On a 1366×768 or 1440×900 window, in both themes, the plot, the director and release facts and the Video, Audio and File rows of the Technical card SHALL be wholly visible without scrolling. Opening the page SHALL clear the title's NEW marker.

#### Scenario: First screen at 1366×768
- **WHEN** the Movie page of a fully scraped movie opens at 1366×768
- **THEN** the title, ratings, actions, the plot, the Director and Released facts and the Video, Audio and File rows are visible without scrolling

#### Scenario: Movie not in the library
- **WHEN** the page is opened for a movie that was removed
- **THEN** it says "That movie is gone" with a link back to the Library

### Requirement: Title header
The header of a title page SHALL show the backdrop band fading into the window, the poster, the stage tag (a warning flag when the title needs attention) with a context line, the title, one meta line, the tagline, the ratings and the actions. For a movie the context line SHALL name its collection ("Example Collection · 3 of 5 owned") and the meta line SHALL hold the year, certification, runtime and genres joined by " · ". For a show the context line SHALL read "TV show · <status>".

#### Scenario: Movie meta line
- **WHEN** a movie from 2020, rated PG-13, 2 hours 8 minutes long, with the genres Drama and Thriller opens
- **THEN** the meta line reads "2020 · PG-13 · 2h 8m · Drama, Thriller"

#### Scenario: Show status
- **WHEN** a show's status is ended
- **THEN** the context line reads "TV show · ended"

### Requirement: Change artwork from the header
Hovering or focusing the poster or the backdrop of a title page SHALL reveal a small "Change poster" or "Change backdrop" button, which opens the artwork picker on that type.

#### Scenario: Change the poster
- **WHEN** the user points at the poster and clicks "Change poster"
- **THEN** the artwork picker opens on the poster of that title

### Requirement: Ratings strip
The header SHALL show each rating that has a value, the preferred source first, as the value with the source's mark beside it. A rating whose source has a page for the title SHALL be a link that opens that page in the browser. Sources with no value SHALL be left out.

#### Scenario: Open a rating source
- **WHEN** the user clicks the IMDb rating of a movie that has an IMDb ID
- **THEN** the browser opens the movie's IMDb page

### Requirement: Title page actions
The header of the Movie page and the TV show page SHALL show the buttons Play, Scrape, Rename, Move, Open folder and "Change artwork", and a "More actions" menu (⋯) with "Edit details", "Show NFO", "Rewrite NFO", "Reload media info", "Remove from library (keep files)", "Ignore this folder" and, after a divider and in red, "Delete permanently…" and "Delete…" as the last item, with the names, icons and order of the Library's selection bar and right-click menu.

#### Scenario: More actions
- **WHEN** the user opens the ⋯ menu of a movie
- **THEN** it lists Edit details, Show NFO, Rewrite NFO, Reload media info, Remove from library (keep files), Ignore this folder, a divider, a red Delete permanently… and a red Delete… last

### Requirement: What the header actions do
Scrape SHALL open the Scrape match page of the title, Rename the rename preview, Move the "Move to" menu and "Change artwork" the artwork picker. "Delete…" in the ⋯ menu SHALL open the deletion confirmation of `title-deletion`. When the title's files are missing or its library folder cannot be reached, Play SHALL be off with the reason, and Open folder SHALL be off with "This title has no folder" for a title without one.

#### Scenario: Scrape from the page
- **WHEN** the user clicks Scrape on a scraped movie
- **THEN** the Scrape match page of that movie opens

#### Scenario: Offline title
- **WHEN** the library folder of a movie is on a drive that is not connected
- **THEN** Play is off with "Play is off: the library folder cannot be reached."

### Requirement: Files that cannot be reached
A title page whose files are missing or whose library folder is offline SHALL show one calm notice with an icon and words: "The files for this title are missing" or "This title's library folder cannot be reached", with what to do and the last known folder. Everything known about the title SHALL stay on the page.

#### Scenario: Drive away
- **WHEN** a movie's drive is disconnected and its page opens
- **THEN** the page says "This title's library folder cannot be reached", shows the last known place, and the facts, plot and artwork are still shown

### Requirement: Plot and facts
The Movie page SHALL show the plot ("No plot yet." when there is none) and then the facts in a two-pair grid, in this order and each only when the movie has it: Director(s), Writer(s), Released, Language (the original language, else the spoken languages), Studio(s), Country/Countries, Collection (a link to the Collections view) and IDs (each a link that opens the title on that provider). The title's tags SHALL show as quiet chips under the facts when it has any.

#### Scenario: Missing fact
- **WHEN** a movie has no studio
- **THEN** the facts have no Studio row and the other facts close up

### Requirement: Technical card
The Movie page's Technical card SHALL show short rows: Video (the resolution and HDR formats as the headline, such as "2160p · Dolby Vision Profile 8.1 · HDR10", then dimensions, frame rate, codec, bitrate and source), Audio with its track count (up to 3 lines), Subtitles with its count (up to 4 lines), File (the main file name, total video size, container and "N files in folder", then the folder path), NFO and, after a rename, Renamed.

#### Scenario: Dolby Vision with an HDR10 base
- **WHEN** the main video is 2160p Dolby Vision profile 8 with an HDR10 base layer
- **THEN** the Video row reads "2160p · Dolby Vision Profile 8.1 · HDR10"

#### Scenario: External subtitle
- **WHEN** a movie has an SRT file next to its video
- **THEN** the Subtitles row lists it with its format and "external file"

### Requirement: All streams and details
"Show all streams and details" SHALL open in place, under the Technical card, the container, video, audio and subtitle tables, an HDR section per video stream (Dolby Vision profile, base layer, mastering display, MaxCLL and MaxFALL when the file has them) and the files in the folder, with "Copy as text" and "Hide details". Nothing in this view SHALL change a file.

#### Scenario: All streams
- **WHEN** the user chooses "Show all streams and details"
- **THEN** the full tables open under the card, and "Copy as text" copies the same content as plain text

### Requirement: Read now never fails silently
When a title's video details were never read, the Video row SHALL say why and offer "Read now". When MediaInfo is not installed it SHALL say "MediaInfo is not installed, so the video details cannot be read yet." with "Download MediaInfo", which downloads it with its progress and then reads again; any other failure SHALL be written next to the button.

#### Scenario: MediaInfo missing
- **WHEN** the user clicks "Read now" and MediaInfo is not installed
- **THEN** the row says MediaInfo is needed and offers "Download MediaInfo"

#### Scenario: Unreadable file
- **WHEN** MediaInfo cannot read the video
- **THEN** the row says "MediaInfo could not read this video" and keeps "Read now"

### Requirement: NFO line and NFO text
The NFO row SHALL name the NFO file and its state: "up to date", "differs from the library", "no NFO file yet", "another file is in the way" or "no folder to put an NFO in", with "Show NFO". "Show NFO" (also in the ⋯ menu) SHALL open in place the text a Rewrite NFO would write, in a monospace font, with "Copy NFO" and Hide; when the file on disk differs, the panel SHALL say so and offer "Rewrite from library".

#### Scenario: NFO up to date
- **WHEN** the movie's NFO matches the library
- **THEN** the row reads "movie.nfo up to date" with "Show NFO"

#### Scenario: NFO differs
- **WHEN** the NFO on disk holds another plot than the library
- **THEN** the row says it differs from the library, and the NFO panel offers "Rewrite from library"

### Requirement: Renamed from line
When the newest change of a title is a rename by CineVault that still stands, the Technical card SHALL show a Renamed row with the old name ("from <old folder name>") and "Undo", which puts the title back to its old name. When that rename cannot be undone from the page, the row SHALL say so and point to History instead.

#### Scenario: Undo a rename
- **WHEN** a movie was renamed from `Example.Movie.2020.1080p` and the user clicks Undo on its Renamed row
- **THEN** the folder gets its old name back and a toast says "Put Example Movie back to its old name."

### Requirement: Cast and crew
The Movie page SHALL show the first seven cast members with photo, name and character, and "All N" (N distinct people) SHALL expand in place the rest of the cast and the crew, one line per department (Directing, Writing, Production, Camera, Editing, Music, Crew), and change to "Show less". A person SHALL be listed once per department with their jobs joined ("Story, Screenplay"); cast members SHALL NOT be merged. A person with an IMDb ID SHALL link to "Open <name> on IMDb".

#### Scenario: Show all cast
- **WHEN** a movie lists 42 people and the user clicks "All 42"
- **THEN** all of them are listed in place and the link reads "Show less"

#### Scenario: Credited twice
- **WHEN** a provider credits one writer for Story and for Screenplay
- **THEN** the Writing line lists that person once, with "Story, Screenplay"

### Requirement: Artwork strip
The Movie page SHALL show a tile for every artwork type the library writes (Poster, Fanart, Logo, Clear art, Disc, Banner, Key art, Thumb), each at the real shape of its type with its size under it, all tiles in a row the same height, and logos, clear art and discs shown whole, not cropped. The strip header SHALL count the filled types ("7 of 8 types") and offer "Change artwork"; an empty type SHALL say "Missing" with "Choose". Both SHALL open the artwork picker on that type.

#### Scenario: Missing key art
- **WHEN** the movie has no key art
- **THEN** its tile says "Missing" with "Choose", which opens the artwork picker on Key art

### Requirement: TV show page
Opening a show SHALL show one scrolling page with the shared header, the plot and the show facts: "Created by", "On disk" ("17 of 19 episodes · 128.6 GB", aired episodes without specials), Quality, Folder and NFO ("tvshow.nfo up to date" or "tvshow.nfo differs from the library") with Show NFO. Then each season SHALL be a section, newest first, with the specials last. Play SHALL start the first numbered episode on disk not marked watched, else the first on disk.

#### Scenario: Show facts
- **WHEN** a show has 19 aired episodes and 17 of them on disk totalling 128.6 GB
- **THEN** the On disk fact reads "17 of 19 episodes · 128.6 GB"

### Requirement: Season sections
Each season header SHALL read "Season <n>" with its year and episode count, then "N missing" in the warning colour, "Complete" or "Not on disk", its size and an "Artwork" action for the season. The latest season that has files SHALL open with the page, and a header click SHALL open or close its season. An open season SHALL list its episodes as table rows with #, Title, Aired, Runtime, Quality, Size and Status.

#### Scenario: Season header with missing episodes
- **WHEN** season 2 from 2025 has 10 episodes and 2 are not on disk
- **THEN** its header reads "Season 2", "2025", "10 episodes" and "2 missing" in the warning colour, with its size

### Requirement: Missing and unaired episodes
Episodes the provider lists and the library lacks SHALL be listed after the owned ones as muted rows. A missing episode SHALL say "Missing" in the warning colour, never in the error colour; one not yet aired SHALL say "Not aired yet". An owned episode SHALL say "On disk", or "No file" when its file is gone.

#### Scenario: Missing episode
- **WHEN** the provider lists S02E03 and the library does not have it
- **THEN** its row is muted and says "Missing" in the warning colour

#### Scenario: Unaired episode
- **WHEN** the provider lists S03E08 with an air date next month
- **THEN** its row is muted and its status is "Not aired yet"

### Requirement: Episodes open in place
Clicking an owned episode row SHALL open it in place under the row with its still, plot ("No plot yet." when none), "Directed by", "Written by", a Video, Audio and Subtitles summary, its NFO state and its full path, with Play, Open folder, "Scrape episode" and "All streams" (the full stream tables in place). Only one episode SHALL be open at a time, and Esc SHALL close it. "Scrape episode" SHALL say "Scraped <title>", "No match for <title>" or "<title> could not be scraped".

#### Scenario: Open an episode
- **WHEN** the user clicks S02E04
- **THEN** its details open under the row and any other open episode closes

#### Scenario: Episode without a file
- **WHEN** an opened episode has no file on disk
- **THEN** Play is off with "This episode has no file on disk"

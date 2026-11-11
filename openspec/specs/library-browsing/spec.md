# library-browsing Specification

## Purpose
The Library page (`/library`, which `/` opens): the Movies | TV shows | Collections switch, the poster grid and the table, sorting, the forgiving library search with its field syntax, the filter row, selection with the floating selection bar and the right-click menu, the size totals, and the Collections view.

## Requirements

### Requirement: Library page and type switch
The address `/` SHALL open the Library page at `/library`. The page SHALL start with a segmented switch "Movies", "TV shows" and "Collections", each with its count, and SHALL open on the type the user last chose unless the address names one (`?type=`). The filters, view and sort of movies and of TV shows SHALL be kept separately.

#### Scenario: Switch to TV shows
- **WHEN** the library holds 23 shows and the user chooses "TV shows 23"
- **THEN** the page lists the 23 shows with the view and sort last used for shows

#### Scenario: Last type remembered
- **WHEN** the user leaves the Library on Collections, restarts the app and opens the Library
- **THEN** the Collections view opens

### Requirement: Grid and table views
Movies and TV shows SHALL each be viewable as a poster grid or a table, switched with "Grid view" and "Table view" in the toolbar. The choice SHALL be remembered per type across restarts, and the selection, filters and sort SHALL be kept when the view switches. Both views SHALL be virtualised and load rows page by page, so a library of 20,000 titles scrolls without loading every row, and both SHALL leave room under the last row while the selection bar is shown.

#### Scenario: Remembered view
- **WHEN** the user switches Movies to the table and restarts the app
- **THEN** Movies opens as the table and TV shows keeps its own view

#### Scenario: Selection kept
- **WHEN** three movies are selected in the grid and the user switches to the table
- **THEN** the same three rows are selected in the table

### Requirement: Poster cards
A poster card SHALL show the poster, or a generated typographic poster when the title has none, at a 2:3 shape with the stage badge in its top-right corner, then the title on one line and a muted line with the year (`????` when unknown), the quality tier and, for a title with a problem, a warning note ("Not matched", "Uncertain", "Files missing"). A title not yet scraped SHALL use the title parsed from its folder or file name.

#### Scenario: Unscraped title
- **WHEN** a folder `example.movie.2022.2160p` has not been scraped
- **THEN** its card reads "Example Movie" with "2022" and the 2160p tier, and the New badge on the poster

#### Scenario: Not matched
- **WHEN** a scrape found no match for a title
- **THEN** its badge is the warning triangle and its meta line ends with "Not matched"

### Requirement: Poster grid layout and states
Posters SHALL be at least 148px wide with 18px between them, so a 1440px-wide window shows seven across. On hover a poster SHALL lift and brighten slightly and show no action buttons. A selected poster SHALL show a gold outline and a gold check. The current poster while moving with the arrow keys SHALL show the keyboard focus ring; the arrow keys SHALL move one poster sideways and one row up or down.

#### Scenario: Select two posters
- **WHEN** the user clicks one poster and Ctrl-clicks another
- **THEN** both show the gold outline and check, and the selection bar says "2 selected"

### Requirement: Library table columns
The Library table SHALL show a select-all checkbox, then Title (with its thumbnail and any warning), Year, Quality, Size, "Drive and folder", Rating and Runtime by default, with "Stage", "Date added" and "File modified" available. The Stage column SHALL have a labelled header and show each title's stage as a tag reading "New", "Scraped" or "Renamed", with a warning colour and a tooltip naming the problem for a title that needs attention; the tag SHALL be informational and show no pointer cursor. The "Columns" menu in the toolbar and a right-click on the header row SHALL switch every column but Title on and off. The header row SHALL stay in view while scrolling.

#### Scenario: Show the date added
- **WHEN** the user ticks "Date added" in the Columns menu
- **THEN** the column appears with each title's short date, and the full date and time on hover

#### Scenario: Stage tag
- **WHEN** the Stage column is on and a title has been scraped but not renamed
- **THEN** its cell reads "Scraped" and the pointer stays an arrow over it

### Requirement: Table layout is remembered
Columns SHALL be resizable by dragging the header edge, which has a hit area at least 8px wide with a column-resize cursor, or with the arrow keys on its handle; double-clicking the edge SHALL fit the column to its widest visible content and header. Columns SHALL be reorderable by dragging a header, which lifts and follows the pointer while the other headers slide aside and a marker shows the drop place, or with "Move left" and "Move right" in the header's right-click menu and Alt+Left/Right. Esc during a drag SHALL cancel it. Title SHALL stay the first column. Rows SHALL follow a resize live and a reorder when the header is dropped, without stutter in a library of 5,000 titles. The switched-off columns, the widths and the order SHALL be remembered per type across restarts.

#### Scenario: Hide a column
- **WHEN** the user unticks Runtime in the Columns menu and restarts the app
- **THEN** the table shows no Runtime column

#### Scenario: Drag a header
- **WHEN** the user drags the Size header left past the middle of the Year header and releases
- **THEN** Size sits before Year in the header and in every row, and stays there after a restart

#### Scenario: Cancel a drag
- **WHEN** the user presses Esc while dragging a header
- **THEN** the header returns to its place and the order is unchanged

#### Scenario: Fit a column
- **WHEN** the user double-clicks the edge of the "Drive and folder" header
- **THEN** the column takes the width of its longest visible folder text, within its minimum and maximum

#### Scenario: Move with the menu
- **WHEN** the user right-clicks the Year header and chooses "Move right"
- **THEN** Year swaps places with the column after it

### Requirement: Sorting
Every table column SHALL sort the whole library in the backend, not only the loaded rows: a click on a header sorts by it and a second click flips the direction. Quality, Size, Rating, Runtime, Date added and File modified SHALL sort largest or newest first on the first click. Ties SHALL always fall back to the title. The sort SHALL be shared by both views and remembered per type across restarts.

#### Scenario: Sort by size
- **WHEN** the user clicks the Size header once
- **THEN** the largest titles of the whole library come first, titles of equal size ordered by title

### Requirement: Sort menu
In grid view a sort menu at the right of the toolbar SHALL offer "Title A–Z" (the default), "Title Z–A", "Recently added", "Oldest added", "File modified, newest first", "File modified, oldest first", "Year, newest first", "Year, oldest first", "Rating, highest first" and "Runtime, longest first". It SHALL read "Custom sort" for a sort made from a table header.

#### Scenario: Header sort seen from the grid
- **WHEN** the table is sorted by the Drive and folder column and the user switches to the grid
- **THEN** the grid keeps that order and the sort menu reads "Custom sort"

### Requirement: Library search
The search field in the centre of the title bar ("Search movies, shows, people", Ctrl+F, ⌘F on macOS) SHALL search the Library when the user presses Enter, opening `/library?q=<text>` and keeping the Library's type, view and filters when already there. The active search SHALL show as a chip "Search: <text>" in the filter row, and clicking the chip SHALL clear it.

#### Scenario: Clear the search chip
- **WHEN** a search is active and the user clicks its "Search: example" chip
- **THEN** the search is cleared and the full list returns with the other filters kept

### Requirement: Forgiving title matching
Matching SHALL ignore case, diacritics and apostrophes, match `&` with "and", read other punctuation as a space, match Roman numerals II to XX with numbers and allow a leading article to be left out. Every typed word SHALL start a word of the title, English title, original title or sort title, or be found in the plot, people, tags or file names, or be the year. Results SHALL be ranked: exact title, title prefix, a word of a title, original or sort title, people, other text, typo.

#### Scenario: Punctuation and accents
- **WHEN** the library holds "Café Example: Part II" and the user searches `cafe example part 2`
- **THEN** that title is listed

### Requirement: Typo tolerance
Only when the exact search finds nothing in the library SHALL a typo be tolerated: one edit (an insertion, deletion, substitution or swap of neighbours) for a typed word of 4 to 7 characters, two for 8 or more. Words shorter than 4 characters SHALL never match by typo.

#### Scenario: Typo
- **WHEN** the library holds "Example Planet" and the user searches `exmaple`
- **THEN** no title matches exactly, so "Example Planet" is listed as a typo match

### Requirement: Field syntax in the search
The search SHALL understand `field:value` terms and a leading `-` for "not". Words that are not recognised SHALL be searched as title text. A known field with a value that makes no sense (`year:abc`) SHALL restrict nothing and SHALL NOT be searched as a title.

#### Scenario: Negation
- **WHEN** the user searches `-genre:horror`
- **THEN** every title except those with the Horror genre is listed

#### Scenario: Refused value
- **WHEN** the user searches `year:abc example`
- **THEN** the year term restricts nothing and only "example" is searched

### Requirement: Search fields
Number fields (`year`, `runtime`, `rating`, `imdb`, `tmdb`, `rt`, `metacritic`, `myrating`, `top250`) SHALL take `2015`, `2010..2019`, `2010..`, `..2000`, `>2010`, `>=2010`, `<2000` and `<=2000`. List fields (such as `resolution`, `hdr`, `codec`, `audio`, `subs`, `genre`, `actor`, `director`, `writer`, `certification`, `edition`, `source`, `stage`, `title`) SHALL take one value or several separated by commas, any of which matches, with double quotes around a value that has spaces.

#### Scenario: Field terms narrow the list
- **WHEN** the user searches `hdr:dv year:2010..2019 actor:"jane example"`
- **THEN** only Dolby Vision titles from 2010 to 2019 with that actor are listed

### Requirement: Yes-or-no search terms
The yes-or-no fields `new`, `review`, `notmatched`, `renamepending`, `missing` and `locked` SHALL be written `is:<field>`, `-<field>` or `<field>:no`. A bare word such as `new` SHALL stay a title word, so a title that contains it is still found.

#### Scenario: Title that is a field word
- **WHEN** the user searches `new example`
- **THEN** titles with words starting with "new" and "example" are listed, and the NEW marker is not used as a filter

#### Scenario: Flag term
- **WHEN** the user searches `is:missing`
- **THEN** only titles whose files are missing or offline are listed

### Requirement: Filter row
Under the toolbar a filter row SHALL show the stage chips "All", "New", "Scraped" and "Renamed" with their counts, then the dropdown chips "Drive: <value>" (All, or a library folder by its label with its title count), "Quality: <value>" (Any, 2160p, 1080p, 720p, SD), "Problems <count>" in the warning tone and "Needs fixing", then the search chip, and at the right the count and total size of what is listed ("181 movies · 7.06 TB"). All of them SHALL combine.

#### Scenario: Drive filter
- **WHEN** the user picks the library folder labelled "Archive" in the Drive menu
- **THEN** only titles in that folder are listed and the footer counts and sizes them only

### Requirement: Problems filter
The Problems menu SHALL offer "No filter", "All problems", "Not matched", "Uncertain match" and "Missing files or offline", and its chip SHALL count every title with one of these problems.

#### Scenario: Problems
- **WHEN** 7 movies are not matched, uncertain or have missing files and the user picks "All problems" in "Problems 7"
- **THEN** only those 7 are listed, each with its warning badge and note

### Requirement: Empty lists
When the filters and search leave nothing, the list SHALL say "No titles match" with "Clear filters and search", which resets both. An empty library SHALL say "Nothing here yet" with "Add a folder", which opens Settings.

#### Scenario: Nothing matches
- **WHEN** the stage chip New and the quality 2160p leave no titles
- **THEN** the list says "No titles match" and "Clear filters and search" resets both

### Requirement: One size rule for every total
The Library footer, the Size column, the selection bar total and every other size total SHALL use one rule: the sum of the distinct files of online titles, each file path counted once (compared without regard to case on Windows), so a file shared by several episodes counts once. Offline, missing and superseded rows SHALL NOT be counted.

#### Scenario: Double episode
- **WHEN** one 2 GB file holds episodes 1 and 2 of a show
- **THEN** the show's size counts that file once, 2 GB

### Requirement: Selection
A click on a poster or table row SHALL select it, Ctrl-click (⌘-click on macOS) SHALL add or remove one title, Shift-click SHALL select a range, Ctrl+A SHALL select every listed title and Esc SHALL clear the selection. The table's row checkbox SHALL act as a Ctrl-click. A double-click or Enter SHALL open the title's page: `/movies/<id>` for a movie, `/tv/<id>` for a show.

#### Scenario: Open with double-click
- **WHEN** the user double-clicks "Example Movie (2020)" in the grid
- **THEN** the Movie page of that movie opens

#### Scenario: Clear with Esc
- **WHEN** four titles are selected, no menu is open and the user presses Esc
- **THEN** the selection is cleared and the selection bar goes away

### Requirement: Keys on the selection
Outside a text field, F2 SHALL rename the selection and Delete SHALL delete it as Settings › Deleted files says.

#### Scenario: Rename with F2
- **WHEN** two movies are selected and the user presses F2
- **THEN** the rename preview opens for those two movies

### Requirement: Floating selection bar
While titles are selected, a floating bar SHALL appear at the bottom centre of the content with the count and total size ("4 selected · 186.2 GB"), then Rename, Scrape, Artwork, Move, Delete and More, with "Rewrite NFO", "Edit details", "Remove from library (keep files)" and "Ignore this folder" under More, and a way to clear the selection. Actions for one title (Edit details) SHALL say "Select one title" when several are selected. While the Problems filter is "Missing files or offline", the bar SHALL show "Remove from library" as a button before Delete, and no longer list it under More.

#### Scenario: Bar for four titles
- **WHEN** four movies totalling 186.2 GB are selected
- **THEN** the bar reads "4 selected · 186.2 GB" with Rename, Scrape, Artwork, Move, Delete and More

#### Scenario: Remove titles that lost their files
- **WHEN** the Problems filter is "Missing files or offline", four titles are selected and the user chooses "Remove from library" on the bar
- **THEN** all four leave the library in one action, their files stay, and the toast says "Removed 4 titles from the library"

### Requirement: Title right-click menu
The right-click menu of a title (or the menu key on the current one) SHALL list three groups: "Open" (Enter), "Play" and "Open folder"; then "Rename" (F2), "Scrape", "Artwork…", "Move to…", "Rewrite NFO", "Edit details", "Remove from library (keep files)" and "Ignore this folder"; then "Delete" (Del). With several titles selected the labels SHALL carry the count ("Rename 4 titles").

#### Scenario: Menu labels for a selection
- **WHEN** the user right-clicks inside a selection of four movies
- **THEN** the menu reads "Rename 4 titles" and "Scrape 4 titles" with their key hints

#### Scenario: Right-click outside the selection
- **WHEN** two titles are selected and the user right-clicks a third
- **THEN** the third becomes the selection and the menu acts on it alone

#### Scenario: One Delete
- **WHEN** the user right-clicks a title
- **THEN** the menu has one Delete item and no separate permanent delete

### Requirement: One set of title actions
The right-click menu, the selection bar with its More menu and the title page header SHALL read their names, icons, key hints and order from one list, so they always say the same thing. Play on a TV show in the Library SHALL be off with "Open the show to play an episode".

#### Scenario: Same words everywhere
- **WHEN** the user compares the selection bar and the right-click menu
- **THEN** both name "Remove from library (keep files)" and "Ignore this folder" with the same icons

### Requirement: Collections view
The Collections switch SHALL show a grid of collections ordered by name ("Name A–Z"), each tile with its artwork, name and "N of M owned". Collections SHALL form by themselves once two films of the same set are owned; with none the view SHALL say "No collections yet" and "A collection appears once you own two or more films of the same set."

#### Scenario: No collections
- **WHEN** no two owned films share a set
- **THEN** the view says "No collections yet"

### Requirement: Collection pane
Choosing a collection SHALL open a docked pane with its artwork and "Change artwork", its name with "3 of 5 owned · 103.6 GB", its overview ("No overview yet." when none) and "Films in this set". Owned films ("In library") SHALL open their Movie page. Films not owned ("Not in library") SHALL be muted with "Search torrents", which opens the Torrents page searching for the film's title and year.

#### Scenario: Missing film in a set
- **WHEN** the "Example Collection" pane lists "Another Movie (2018)", which the user does not own
- **THEN** the film is muted with "Search torrents", which opens Torrents searching "Another Movie 2018"

#### Scenario: Owned film
- **WHEN** the user chooses an owned film in "Films in this set"
- **THEN** that film's Movie page opens

### Requirement: Spotlight search
Typing in the title-bar search SHALL open a panel under the field that updates about 80 ms after the last keystroke and keeps the previous rows on screen until the new ones arrive. It SHALL list the best match as "Top hit", then only the groups that have hits among "Movies", "TV shows", "Collections" and "People", about five rows each, and end with "See all N results →". The panel SHALL appear at once under reduced motion.

#### Scenario: Instant results
- **WHEN** the user types "blade"
- **THEN** matching movies and shows appear grouped under "Top hit", "Movies" and "TV shows", with "See all N results →" last

### Requirement: Spotlight rows
A title row of the panel SHALL show a small poster, the title with the typed letters marked, the type and year, and its stage. A title that matches only through typo forgiveness SHALL be labelled "Similar".

#### Scenario: Typo result
- **WHEN** the user types "blde" and the search forgives it to find "Blade Runner"
- **THEN** the row reads "Blade Runner" with the subtle label "Similar"

### Requirement: Spotlight keys
The Up and Down arrows SHALL move through the rows of the panel and wrap. Enter SHALL open the highlighted row (a title page, a collection, or the Library filtered to a person), and with no row highlighted SHALL search the Library as before. The first Esc SHALL close the panel and the second SHALL clear the field, and a press outside SHALL close it. The field SHALL be a combobox and the panel its listbox for screen readers.

#### Scenario: Open the highlighted row
- **WHEN** the user types "blade", presses Down and then Enter
- **THEN** the page of the first row opens

#### Scenario: Enter without a highlighted row
- **WHEN** the user types "blade" and presses Enter without using the arrow keys
- **THEN** the Library opens with the search "blade"

### Requirement: Spotlight recents
With the field empty and focused the panel SHALL list up to six recent searches and up to six recently opened titles, each list with a "Clear" button.

#### Scenario: Recent searches
- **WHEN** the user focuses the empty field after searching for "heat"
- **THEN** "heat" is listed under "Recent searches", and choosing it searches again

### Requirement: Date added is the file's creation time
A title's "Date added" SHALL be the creation time of its main file when it is first inserted, falling back to the scan time when the file system gives none. A show SHALL take the date of its oldest episode file, and a date SHALL never be later than the scan that found the title. Titles that share one identical date (at least ten) SHALL be dated once from their files at the next scan, and titles with their own dates SHALL be left alone.

#### Scenario: Old file added today
- **WHEN** a movie file created last year is scanned for the first time today
- **THEN** its Date added is last year's date

### Requirement: Poster size
The poster grid SHALL offer a size control in the toolbar that changes the minimum poster width, and SHALL remember it across restarts. The default SHALL keep the layout of "Poster grid layout and states".

#### Scenario: Bigger posters
- **WHEN** the user raises the poster size and restarts the app
- **THEN** the grid opens with the larger posters

### Requirement: Reload media info from the Library
The Library toolbar SHALL show a "Reload media info" icon button right beside "Scan folders now" for Movies and TV shows. With titles selected it SHALL reload those titles; with none selected it SHALL ask "Reload media info for N movies? This reads every file again and can take a few minutes." under the button, with Cancel and Reload, and reload every title of the current list on Reload.

#### Scenario: Whole list
- **WHEN** nothing is selected, the Movies list holds 180 titles and the user presses "Reload media info" and then Reload
- **THEN** a background job re-reads the details of the 180 movies, shown in the sidebar progress card

#### Scenario: Selection
- **WHEN** two titles are selected and the user presses "Reload media info"
- **THEN** those two titles are reloaded at once, with no question

### Requirement: Reload media info in the menus
"Reload media info" SHALL also be in the right-click menu and the More menu of the selection bar, acting on the selected titles. Scan folders now SHALL keep reading changed files only.

#### Scenario: Right-click
- **WHEN** the user right-clicks a title and chooses "Reload media info"
- **THEN** that title's file details are read again as a background job

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

### Requirement: Switch selected titles between localized and original title
The selection bar's More and the right-click menu SHALL offer "Use original title" and "Use localized title" for the selected movies or TV shows. Choosing one SHALL set the choice of every selected title that has another original title and rewrite their NFO files; titles whose names now differ SHALL show Rename pending, and nothing SHALL be renamed on its own. A toast SHALL say what changed and offer Undo, which switches back exactly the titles that changed. Locks SHALL NOT stop it.

#### Scenario: Four titles, two with another original title
- **WHEN** four movies are selected, two of them have an original title in another language, and the user chooses More › "Use original title for 4 titles"
- **THEN** those two use their original title, their NFO files are rewritten, the toast says "Now using the original title for 2 titles. 2 titles had no other title to switch to." and Undo switches the two back

#### Scenario: Nothing to switch
- **WHEN** no selected title has another original title
- **THEN** the toast says nothing changed and offers no Undo

#### Scenario: Names follow through Rename
- **WHEN** a scraped movie whose folder is named from `${title} (${year})` is switched to its original title
- **THEN** it shows Rename pending, and its folder is renamed only when the user runs Rename

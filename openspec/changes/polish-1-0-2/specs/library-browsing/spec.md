## MODIFIED Requirements

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

## ADDED Requirements

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

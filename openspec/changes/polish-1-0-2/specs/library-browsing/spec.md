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
Typing in the title-bar search SHALL open a dropdown under it with instant results grouped as Movies, TV shows and People, a "See all results for <text>" row and, while the field is empty, the recent searches. The Up and Down arrows SHALL move through the rows, Enter SHALL open the chosen row (a title page, a person's page or the Library search), and Esc SHALL close the dropdown.

#### Scenario: Instant results
- **WHEN** the user types "blade"
- **THEN** matching movies and shows appear grouped, and Enter on the first opens its page

### Requirement: Date added is the file's creation time
A title's "Date added" SHALL be the creation time of its main file when it is first inserted, falling back to the scan time when the file system gives none. Titles whose date added equals the library's first-scan time SHALL be repaired once from their file's creation time.

#### Scenario: Old file added today
- **WHEN** a movie file created last year is scanned for the first time today
- **THEN** its Date added is last year's date

### Requirement: Poster size
The poster grid SHALL offer a size control in the toolbar that changes the minimum poster width, and SHALL remember it across restarts. The default SHALL keep the layout of "Poster grid layout and states".

#### Scenario: Bigger posters
- **WHEN** the user raises the poster size and restarts the app
- **THEN** the grid opens with the larger posters

## ADDED Requirements

### Requirement: Latest releases list
While no title is picked, the Torrents page SHALL list the newest releases of every switched-on source that can list them, in the release table of the search, with rows appearing as each source answers. Picking a title in the field SHALL show its search instead, and clearing the field SHALL bring the list back. The field SHALL show "N releases" while the list is shown.

#### Scenario: Opening the page
- **WHEN** the user opens the Torrents page with no title in the address
- **THEN** the newest releases of the switched-on sources start listing in the release table, newest first, and the search field is ready

#### Scenario: Back from a search
- **WHEN** the user picks a title, looks at its releases and clears the field
- **THEN** the list is shown again

### Requirement: Latest releases controls
The list SHALL offer All, Movies and TV; a sort with Newest (the default), Seeders, Size and Quality, also sortable by the table headers and remembered across restarts; the resolution chips of the search with their counts; "Hide CAM and TS", on by default, which hides camera, telesync, telecine and screener copies; and Refresh, which reads every source's first page again.

#### Scenario: Movies only
- **WHEN** the user chooses Movies
- **THEN** only movie releases are listed, and only sources that list movies are asked

#### Scenario: Camera copies
- **WHEN** a source lists "Example.Movie.2026.HDCAM.x264"
- **THEN** it is not shown until the user switches "Hide CAM and TS" off

### Requirement: Latest releases kept for a while
The rows of each kind (All, Movies, TV) SHALL be kept for five minutes after its first page was read, so switching kinds or leaving for a search and coming back SHALL ask no source again in that time; Refresh SHALL start over at once.

#### Scenario: Switching tabs
- **WHEN** the user switches from All to TV and back within five minutes
- **THEN** the All rows are shown at once and no source is asked again for them

### Requirement: More releases near the end
When the user scrolls near the end of the list, the page SHALL read the next page of every source that has one and add its rows; a release already listed SHALL stay one row. "Load more" SHALL do the same by hand.

#### Scenario: Scrolling on
- **WHEN** the user scrolls to the last rows and YTS and EZTV have more
- **THEN** their next pages are added below, and no release appears twice

### Requirement: Latest releases foot line
Under the list a quiet line SHALL name the sources with no further page ("No more from YTS, The Pirate Bay"), the indexers that cannot list their newest releases ("1337x can't list new releases", not as a failure), and the sources that failed, with "Try again"; when no source has more it SHALL say "That is everything the sources list right now."

#### Scenario: A source that ends
- **WHEN** The Pirate Bay has given its one page
- **THEN** the line says "No more from The Pirate Bay" and The Pirate Bay is not asked again until Refresh

### Requirement: In your library, without a lookup
A listed release whose title (or original title) and kind, and for a movie whose year within one, match a library title SHALL carry "In your library", or "Better than your copy" when it is better than the movie's copy by the comparison of the search. This SHALL use the library alone; no provider is asked.

#### Scenario: A better copy is out
- **WHEN** the library holds Example Movie (2020) at 1080p and the list shows "Example.Movie.2020.2160p.WEB-DL.DV"
- **THEN** that row says "Better than your copy" and nothing was looked up online

### Requirement: Release details panel
A plain click on a release SHALL select it and open a side panel, also after the panel was closed; Ctrl- or Shift-clicks SHALL only select. Details, Enter or a double-click SHALL open it too. It SHALL slide in from the right on opening, not on switching titles, follow the selected release, close on Esc, and show at once what the release name gives.

#### Scenario: One click opens the details, never sends
- **WHEN** the user clicks a listed release once
- **THEN** its details open and nothing is sent to the torrent app

### Requirement: Columns beside the details panel
Beside the open details panel the table SHALL hide Source, Age, Size, Seeders and then Quality, in that order, until the remaining columns fit, so no column is under the panel. The release name and the row's buttons SHALL always stay.

#### Scenario: Every column stays visible beside the panel
- **WHEN** the details panel is open in a window too narrow for every column
- **THEN** the table hides the columns the panel repeats until the rest fits, and none sits under the panel

### Requirement: Release details from TMDB
Opening the details SHALL look the title up on TMDB once per title for the session: by the IMDb ID a source gave, otherwise by the title and year from the name, with the title search's confidence. The panel SHALL then show the poster, title, year, plot, certification, genres and TMDB rating, and links to the title on TMDB, IMDb, Letterboxd (movies only) and Trakt that open in the browser.

#### Scenario: Open the details
- **WHEN** the user presses Details on "Example.Movie.2020.1080p.WEB" and a source gave its IMDb ID
- **THEN** TMDB is asked once by that ID, and the poster, plot and the TMDB, IMDb, Letterboxd and Trakt links appear

#### Scenario: Opened again
- **WHEN** the user closes the panel and opens the same title's details again
- **THEN** no second lookup is made

### Requirement: Weak or missing match
A weak match SHALL say "This may not be the right title." and a missing one "TMDB has no title that fits this release.", each with "Pick another", which searches titles in the panel and uses the one picked. Without a TMDB key the panel SHALL show only what the name says and how to add a key.

#### Scenario: Weak match
- **WHEN** the best TMDB hit fits the release name poorly
- **THEN** the panel says "This may not be the right title." and offers "Pick another"

#### Scenario: No TMDB key
- **WHEN** no TMDB key is set and the user opens a release's details
- **THEN** the panel shows the title, year and episode read from the name and says how to add a key

### Requirement: Release details actions
The panel SHALL link the library title when there is one, list the title's other releases in the list, and offer "Send to torrent app" and "Search all releases", which shows the search of the matched title.

#### Scenario: Search the title
- **WHEN** the user chooses "Search all releases" in the panel of a matched title
- **THEN** the page shows the search of that title with its releases from every source

### Requirement: Responsibility line
After the notice was accepted, the Torrents page SHALL keep its sentence "Torrent search finds releases on public sites. You are responsible for what you download and share." as a quiet line at the foot of the page, for the list and the search alike.

#### Scenario: Always in view
- **WHEN** the user has accepted the notice and opens the Torrents page
- **THEN** the responsibility sentence is shown at the foot of the page

### Requirement: Latest releases states
The Latest releases list SHALL show placeholder rows until the first source answers, "No releases came back. Try Refresh in a moment." with Refresh when nothing came back, "None of your sources lists its newest releases. Switch one on in Settings, Torrent sources." when no source can list them, and "Every release here is hidden by the filters." with "Clear filters" when the filters leave nothing.

#### Scenario: Everything filtered out
- **WHEN** every listed release is 720p and the 2160p chip is chosen
- **THEN** the area says "Every release here is hidden by the filters." and "Clear filters" shows them again

## MODIFIED Requirements

### Requirement: Release table columns and sort
Results SHALL be a table with the columns Quality (the shared quality badge), Release name, Size, Seeders, Source and Age, plus an actions column with a "Send" button per row whose accessible name and tooltip are "Send to torrent app" (and, in the Latest releases list, a Details button before it). The actions column SHALL fit its buttons and SHALL NOT be resizable. A release found by several sources SHALL show the first source name with "+N". Size, Seeders, Source and Age SHALL be sortable by their header, the default sort SHALL be Seeders, most first, releases missing the sorted figure SHALL go last, and the chosen sort SHALL be remembered across restarts.

#### Scenario: Sort remembered
- **WHEN** the user sorts by Size and restarts the app
- **THEN** the next search is sorted by Size

#### Scenario: Release from several sources
- **WHEN** a release was returned by YTS and two other sources
- **THEN** its Source cell reads "YTS +2"

#### Scenario: Narrow window
- **WHEN** the window is at its smallest width
- **THEN** the row's Send button is shown whole

### Requirement: Result states
The results area SHALL never be an empty page. It SHALL show placeholder rows while searching and a plain state for: no title picked yet (the Latest releases list), no title found ("Nothing found for “<query>”", shown in the suggestions), no releases found ("No releases found for <title>." with "Search again"), nothing left by the chosen chip ("No <tier> releases." with "Clear filters"), and every source failed ("No source answered. Check your connection and try again." with "Retry all").

#### Scenario: Nothing left by the chip
- **WHEN** the sources answered but nothing is 2160p and the 2160p chip is chosen
- **THEN** the area says "No 2160p releases." and "Clear filters" shows all results again

#### Scenario: Every source failed
- **WHEN** every asked source failed or was rejected
- **THEN** the page says "No source answered. Check your connection and try again." and "Retry all" asks the failed sources again

#### Scenario: Before a title is picked
- **WHEN** the page is open and no title is picked
- **THEN** the Latest releases list is shown instead of an empty area

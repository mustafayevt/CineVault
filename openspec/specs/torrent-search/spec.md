# torrent-search Specification

## Purpose
Torrent search as the user sees it: the Torrents page with its one search field, the responsibility notice that gates it, the matched title line, resolution chips, the release table with its comparison against the owned copy, the result states, the hand-off of a release to the user's own torrent app, the entry from a collection page, and the Settings › Torrent sources section. CineVault never downloads, tracks or imports a torrent itself.

## Requirements

### Requirement: Always available behind a one-time notice
Torrent search SHALL always be available, with no switch in Settings › Features: the "Torrents" sidebar entry, the `/torrents` page and the "Torrent sources" Settings section are present from the first start. Until the user accepts the notice "Torrent search finds releases on public sites. You are responsible for what you download and share." with "Got it", the page SHALL show only that notice and the app SHALL contact no torrent source, download no indexer service and hand off no release.

#### Scenario: Fresh install
- **WHEN** a new user finishes setup
- **THEN** "Torrents" is in the sidebar, and no torrent source or indexer service has been contacted

#### Scenario: First visit
- **WHEN** the user opens the Torrents page for the first time
- **THEN** the page shows the notice under "Before you search" with "Got it", and after "Got it" the search field appears and a search works at once

#### Scenario: Notice left open
- **WHEN** the user leaves the page without pressing "Got it"
- **THEN** no torrent source or indexer service is contacted, and the notice is shown again on the next visit

#### Scenario: Accepted once
- **WHEN** the user has pressed "Got it" and restarts the app
- **THEN** the setting `torrents.notice-accepted` is on and the notice does not show again

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

### Requirement: The year in a title search ranks
A year in the typed text SHALL rank the title hits with the same rule as the Scrape search: the title fit less a penalty for another year, with popularity breaking near ties.

#### Scenario: Year in the query
- **WHEN** the user types "example title 2005"
- **THEN** the 2005 movie Example Title is the first hit, above the 2015 show of the same name

### Requirement: Search from a collection page
A film of a collection that the library does not hold SHALL offer "Search torrents", which opens the Torrents page with that film as the confirmed title and its releases searched.

#### Scenario: Missing film of a set
- **WHEN** the user chooses "Search torrents" on a missing film of Example Collection
- **THEN** the Torrents page opens with the film's name in the field and its releases listing

### Requirement: Resolution chips with counts
Above the results the page SHALL show the chips All, 2160p, 1080p, 720p and SD (480p and unknown resolutions count as SD), each with the number of releases it holds, and choosing one SHALL list only those releases. Before the first release arrives the chips SHALL show no counts and SHALL NOT be selectable.

#### Scenario: Only 2160p
- **WHEN** the user clicks the 2160p chip showing 18
- **THEN** 18 releases are listed, all 2160p

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

### Requirement: Same, better or worse than yours
When the library holds the searched title with a video file, the page SHALL show the line "Yours: <title> · <quality> · <size>", the title linking to its Movie page for a movie, and each release SHALL carry a tag "Same as yours", "Better" or "Worse" after its name, with a tooltip naming the owned copy's quality and size.

#### Scenario: A better release
- **WHEN** the owned copy is 1080p SDR and a release is 2160p Dolby Vision
- **THEN** that release shows "Better"

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

### Requirement: Offline line
When the system reports no network, the Torrents page SHALL show the line "You are offline. Search again when the connection is back." with "Retry all" above the results.

#### Scenario: Offline
- **WHEN** the network goes away while the page is open
- **THEN** the offline line appears, and "Retry all" asks the failed sources again once the connection is back

### Requirement: Hand-off only
Choosing a release (double-click, Enter, "Send to torrent app" in its row or its right-click menu) SHALL hand its magnet link to the system's default torrent app, or, when the release has no magnet link, download its `.torrent` file and open it with the default app, and confirm with the toast "Sent to your torrent app". In a browser, the magnet SHALL open through the browser's own handler, and a `.torrent` SHALL download from an authenticated route so the browser can open it; the toast SHALL say "Opening your torrent app…" and SHALL always offer "Copy magnet link", with a selection fallback when the clipboard API is unavailable on a plain-HTTP page. CineVault SHALL NOT download the release's content, track it or import it. "Copy magnet link" SHALL be in the right-click menu.

#### Scenario: Send a release
- **WHEN** the user double-clicks a release in the desktop app
- **THEN** the default torrent app receives its magnet link and a toast says "Sent to your torrent app"

#### Scenario: No torrent app
- **WHEN** no app is set up for magnet links in the desktop app
- **THEN** a toast says "No torrent app is set up for magnet links" and offers "Copy magnet link"

#### Scenario: Notice not accepted
- **WHEN** a hand-off is requested before the notice was accepted
- **THEN** nothing is opened and a toast says "Accept the Torrent search notice first"

#### Scenario: Browser hand-off
- **WHEN** the user sends a release from a browser
- **THEN** the magnet opens through the browser, the toast says "Opening your torrent app…" and "Copy magnet link" is offered

#### Scenario: Copy without the clipboard API
- **WHEN** the page is served over plain HTTP on the LAN and the browser has no clipboard API
- **THEN** "Copy magnet link" selects the link so the user can copy it

### Requirement: Settings › Torrent sources
The Settings page SHALL have a section "Torrent sources" in its index that lists, under "Sites", each built-in source with its switch, its health (Working, Needs a challenge solver, Unreachable, Error, Off, Checking or Not checked yet) and when it was last checked ("Checked 5 min ago"), with "Check sources now". The health of a built-in source SHALL come from its last search. The section SHALL also hold the indexer service card, the own Torznab instance card and the challenge solver address.

#### Scenario: Re-check
- **WHEN** the user chooses "Check sources now" while the indexer service runs
- **THEN** the indexers are re-tested in the background job "Checking torrent sources" and the list shows the new health when it ends

### Requirement: Source addresses
Each built-in source's web address SHALL be an advanced setting of the Torrent sources section, with a reset to its default address, for a site that has moved.

#### Scenario: A site that moved
- **WHEN** the user changes the YTS address to a mirror
- **THEN** the next search asks YTS at that address, and Reset restores the default

### Requirement: Indexer service card
The Torrent sources section SHALL show an "Indexer service" card with its switch, its state (Running, Downloading, Starting, Not running, Stopped, Failed, Not downloaded), its version and how many indexers work, with "Download" (or "Try again"), "Update indexer service" and "Open indexer manager". A newer version, once found, SHALL stay announced ("Version <version> is available") across restarts until it is installed. The card SHALL list each provisioned indexer with its switch and health.

#### Scenario: Update notice survives a restart
- **WHEN** a check found a newer version and the app is restarted before it is installed
- **THEN** the card still says "Version <version> is available" with "Update indexer service"

#### Scenario: Switched off
- **WHEN** the user switches the indexer service off
- **THEN** the card says "Off. The sites above still work." and the built-in sources keep answering searches

### Requirement: Indexer manager and challenge solver help
"Open indexer manager" SHALL open the service's own pages in the browser on its loopback address and show the admin password with "Copy the password". When sites need a browser check, the card SHALL explain how to install the FlareSolverr challenge solver and where to enter its address ("Challenge solver address").

#### Scenario: Open the manager
- **WHEN** the user chooses "Open indexer manager"
- **THEN** the browser opens the service's pages and the card offers "Copy the password"

#### Scenario: Sites behind a browser check
- **WHEN** the last check left 3 sites needing a challenge solver
- **THEN** the card says "3 sites ask for a browser check." with the steps to set up a challenge solver

### Requirement: Tracker languages
The indexer service card SHALL have "Tracker languages": by default English plus every language set as the metadata language, the metadata fallback language or the NFO language; any other language can be switched on. A change SHALL start a check after a short pause. Only indexers in a chosen language SHALL be provisioned (an indexer that names no language always is), and provisioned indexers that do not qualify SHALL be removed at the next check.

#### Scenario: Default languages
- **WHEN** the metadata language is English and the user never changed Tracker languages
- **THEN** the next check provisions only English indexers and removes indexers in other languages that an earlier check added

#### Scenario: Another language switched on
- **WHEN** the user switches Russian on
- **THEN** a check starts and provisions the Russian indexers too

#### Scenario: Languages set elsewhere count
- **WHEN** the metadata fallback language is French and Tracker languages were never changed
- **THEN** English and French indexers are provisioned

### Requirement: Tracker language counts
Each language that is not chosen SHALL show how many indexers it would add, as the last check found them, and a line SHALL say how many indexers in other languages are off, with "Turn them on" to switch all those languages on.

#### Scenario: Counts of what other languages add
- **WHEN** the last check left 7 Russian and 4 Chinese indexers out for their language
- **THEN** the card says "11 indexers in other languages are off." with "Turn them on", and the Russian chip says it holds 7

### Requirement: Use your own Jackett
When a Jackett that is not CineVault's own service answers at the saved Torznab instance's address or at `http://127.0.0.1:9117`, the Torrent sources section SHALL show a "Your Jackett" card with "Use your Jackett", which asks only for its API key and sets the own Torznab instance to that Jackett's all-indexers feed once the key passes the test, then offers "Switch the indexer service off". The app SHALL NOT read another program's files for the key.

#### Scenario: Use your Jackett
- **WHEN** a Jackett answers at 127.0.0.1:9117 and the user pastes its key in "Use your Jackett"
- **THEN** the own Torznab instance points at its all-indexers feed, the card says "Searches ask your Jackett too.", and no file of that program was read

### Requirement: Picked title chip
Above the results the page SHALL name the picked title as "Releases for <title> · Movie" (or "· Show") in a chip with a "Change title" × button. Pressing it, or editing the field, SHALL drop the title and its results and leave the field ready for another title. The confirmed title SHALL be kept in the page address (`?title=movie:<tmdb id>`), so a reload opens it again.

#### Scenario: Change the title
- **WHEN** releases are listed and the user presses the × on the title chip
- **THEN** the results go, the field is empty and has focus, and nothing is searched until another title is picked

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

# torrent-wishlist Specification

## Purpose
The torrent Wishlist: titles the user wants a release of with the kind of release they are after, the checks that look for one (when the app opens, at most every 6 hours per entry, or on demand), the matches with Send to torrent app and Dismiss, the sidebar count and toast that announce new ones, the done rule against the library, and the ways a title is added. Nothing is sent to the torrent app without the user choosing it.

## Requirements

### Requirement: Wishlist view
The Torrents page SHALL have two views, Search (the one the Torrents item opens) and Wishlist, switched by a "Search · Wishlist" control at the top of both that shows the number of new matches. The Wishlist view (`/torrents/wishlist`), behind the responsibility notice like the search, SHALL list every entry with its poster, title, year, what it looks for and its criteria in one line, with Edit and Remove; the newest entries first, done entries last under "Done". An entry that is not done and whose scope the library holds SHALL say "You already have this: " followed by the held copy's resolution, source and HDR, as far as they are known, linking to its Movie or TV show page.

#### Scenario: Open the Wishlist
- **WHEN** the user chooses Wishlist on the Torrents page
- **THEN** the address is `/torrents/wishlist` and the entries are listed, the newest first

#### Scenario: Empty Wishlist
- **WHEN** the Wishlist has no entry
- **THEN** the view says "Your Wishlist is empty. Choose Add, or add a title from a torrent search, a collection or a title page."

#### Scenario: What an entry looks for
- **WHEN** the Wishlist holds Season 2 of Example Show and the next new episode of Another Show
- **THEN** they read "Season 2" and "Next new episode · S03E06"

#### Scenario: A title the library holds
- **WHEN** an entry is a movie the library holds
- **THEN** its title links to its Movie page

#### Scenario: The copy held
- **WHEN** an entry asks for Example Movie (2020) in 2160p and the library holds it in 1080p WEB-DL
- **THEN** the entry says "You already have this: 1080p · WEB-DL", linking to the Movie page

### Requirement: Wishlist entries and criteria
An entry SHALL be a movie, or for a show a season, one episode or "Next new episode". Its criteria SHALL be the lowest resolution (Any, 720p, 1080p or 2160p), the accepted sources (Remux, Blu-ray, WEB-DL, WEBRip; none chosen accepts any), "HDR only", and an optional smallest size and largest size in GB (GiB when sizes are shown in base 2) and fewest seeders. The smallest size SHALL NOT be above the largest when both are set. The editor SHALL be part of the page (no dialog), and while the library holds the title and scope being edited it SHALL say "You already have this: " with the held copy's resolution, source and HDR. Adding a title and scope already on the Wishlist SHALL replace that entry's criteria instead of adding a second entry. Saving an entry for a title the library already holds SHALL add an active entry that is checked like any other.

#### Scenario: Default criteria
- **WHEN** the user adds a movie the library does not hold
- **THEN** the editor opens with 1080p as the lowest resolution and no source, size or seeder limit

#### Scenario: Add with criteria
- **WHEN** the user adds Example Movie (2020) with 2160p, Remux and HDR only, and at most 80 GB
- **THEN** the entry lists "2160p or better · Remux · HDR · up to 80 GB"

#### Scenario: A size range
- **WHEN** the user adds Example Movie (2020) with 2160p, Remux, at least 20 GB, at most 60 GB and 10 seeders
- **THEN** the entry lists "2160p or better · Remux · 20–60 GB · 10+ seeders"

#### Scenario: Smallest above largest
- **WHEN** the user types 60 as the smallest size and 20 as the largest and saves
- **THEN** the editor says the smallest size must not be above the largest and nothing is saved

#### Scenario: Defaults from the copy held
- **WHEN** the user adds a movie the library holds in 1080p
- **THEN** the editor opens with "2160p" as the lowest resolution; for a 2160p copy without HDR it asks for HDR, and for a 2160p HDR copy for a Remux

#### Scenario: Adding a title the library holds
- **WHEN** the user adds Example Movie (2020) with 1080p while the library holds it in 1080p WEB-DL
- **THEN** the editor says "You already have this: 1080p · WEB-DL", and the saved entry is listed among the active entries and checked

#### Scenario: Added twice
- **WHEN** the user adds Season 2 of Example Show while it is already on the Wishlist
- **THEN** the Wishlist still holds one Season 2 entry, with the new criteria

### Requirement: Add on the Wishlist
The Wishlist view SHALL have an **Add** button that shows a title field with suggestions as the user types (movies and shows, the owned ones marked) and Cancel; picking a title SHALL open the editor for it at the top of the Wishlist.

#### Scenario: From the Wishlist
- **WHEN** the user chooses Add on the Wishlist, types "example" and picks Example Movie (2020)
- **THEN** the editor for Example Movie (2020) opens at the top of the Wishlist

### Requirement: Adding to the Wishlist
"Add to Wishlist" SHALL be offered on a title picked in the torrent search, on a film of a collection that the library does not hold (beside "Search torrents"), and in the ⋯ menu of the Movie page and the TV show page of a title with a TMDB ID. Each SHALL open the Wishlist with the editor filled in for that title; a show SHALL start from the season or episode chosen on the search page, otherwise from "Next new episode".

#### Scenario: Already on the Wishlist
- **WHEN** the user picks a title in the torrent search that is on the Wishlist
- **THEN** the page shows "On your Wishlist", which opens the Wishlist

#### Scenario: From a search
- **WHEN** the user picks Example Movie (2020) in the torrent search and chooses "Add to Wishlist"
- **THEN** the Wishlist opens with the editor for Example Movie (2020)

#### Scenario: From a collection
- **WHEN** the user chooses "Add to Wishlist" on a missing film of Example Collection
- **THEN** the Wishlist opens with the editor for that film

### Requirement: Wishlist checks
Each entry that is not done SHALL be checked when the app opens and while it stays open, at most once every 6 hours per entry, and "Check now" SHALL check every entry that is not done at once and then show "Checked 5 min ago". One check SHALL run at a time. Nothing SHALL be checked before the responsibility notice is accepted, and an empty Wishlist SHALL contact nothing. When no source answered, the entry SHALL say "No source answered at the last check."

#### Scenario: Check now
- **WHEN** the user chooses "Check now" with three entries, one of them done
- **THEN** the two others are checked and their matches listed

#### Scenario: Not yet due
- **WHEN** the app opens two hours after an entry was checked
- **THEN** that entry is not checked again until six hours have passed

### Requirement: What a check keeps
A check SHALL search every switched-on source that applies to the entry and keep the releases of the title and scope asked for that carry no warning (cam, telesync, telecine, screener, no seeders, payload, too small) and meet every criterion. A release whose size or seeders are not known SHALL NOT meet a set limit.

#### Scenario: A release over the size limit
- **WHEN** an entry asks for at most 40 GB and a matching 2160p release is 58 GB
- **THEN** that release is not listed

#### Scenario: A release under the smallest size
- **WHEN** an entry asks for at least 20 GB and a matching 2160p release is 12 GB
- **THEN** that release is not listed

### Requirement: Wishlist matches
Each entry SHALL list its matches, most seeders first, each with the quality badge, name, size, seeders, "New" while it is new, **Send to torrent app** and **Dismiss**. Sending SHALL use the hand-off of the torrent search and mark the match "Sent". A dismissed match SHALL be hidden and SHALL NOT be listed again for that entry by a later check. CineVault SHALL NOT send a release by itself.

#### Scenario: Dismiss
- **WHEN** the user dismisses a match and the entry is checked again
- **THEN** that release is not listed again

#### Scenario: Send
- **WHEN** the user chooses "Send to torrent app" on a match
- **THEN** the release goes to the torrent app as from the search, and the match shows "Sent"

### Requirement: New matches are announced
New matches SHALL be counted on the sidebar's Torrents item and on the Wishlist switch until the Wishlist is opened, and a check that finds new matches SHALL show the toast "<n> new Wishlist matches" with "Show", which opens the Wishlist.

#### Scenario: New matches found
- **WHEN** a check finds two new releases for the Wishlist
- **THEN** the Torrents item shows "2", and a toast says "2 new Wishlist matches" with "Show"

#### Scenario: Looked at
- **WHEN** the user opens the Wishlist
- **THEN** the count goes, and the matches found by that check still show "New" until the view is left

### Requirement: Done when the library has it
An entry SHALL be marked done, and no longer checked, when the library gains a copy that meets its resolution, source and HDR criteria after the entry was added, last edited or told to keep looking: for a movie, when more of its copies (each with its TMDB ID) meet the criteria than did then; for a season, when every aired episode is held with the quality most of them share and that quality meets them while it did not then; for an episode, when that episode is held and meets them while it did not then. When fewer copies meet the criteria than did then, the lower number SHALL count from then on. A Blu-ray copy SHALL count as a Remux. Done entries SHALL show "Done: your library has it" under "Done" with **Keep looking** and Remove; Keep looking, and editing the criteria, SHALL make the entry active again, counting from that moment.

#### Scenario: The copy arrives
- **WHEN** an entry asks for 2160p and a 2160p copy of the movie is scanned into the library
- **THEN** the entry shows under Done and is not checked any more

#### Scenario: A copy that was already there
- **WHEN** the user adds Example Movie (2020) with 1080p while the library holds a 1080p copy
- **THEN** the entry stays active and is checked; once a second copy of 1080p or better is scanned in, it shows under Done

#### Scenario: Keep looking
- **WHEN** the user chooses Keep looking on a done entry
- **THEN** the entry is listed among the active entries again and is checked

### Requirement: Next new episode
A "Next new episode" entry SHALL look for the first aired episode after the newest episode of the show the library holds, or for the newest aired episode when the library holds none, leaving specials out. When that episode has not aired yet, the entry SHALL say "Next episode airs <date>" (or "No new episode announced") and contact no source. When the episode looked for changes, the matches of the earlier one SHALL be dropped. Such an entry SHALL never be done.

#### Scenario: Following a show
- **WHEN** the library holds Example Show up to S03E05 and S03E06 has aired
- **THEN** the entry looks for S03E06, and once S03E06 is in the library, for S03E07

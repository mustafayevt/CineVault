## MODIFIED Requirements

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

### Requirement: What a check keeps
A check SHALL search every switched-on source that applies to the entry and keep the releases of the title and scope asked for that carry no warning (cam, telesync, telecine, screener, no seeders, payload, too small) and meet every criterion. A release whose size or seeders are not known SHALL NOT meet a set limit.

#### Scenario: A release over the size limit
- **WHEN** an entry asks for at most 40 GB and a matching 2160p release is 58 GB
- **THEN** that release is not listed

#### Scenario: A release under the smallest size
- **WHEN** an entry asks for at least 20 GB and a matching 2160p release is 12 GB
- **THEN** that release is not listed

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

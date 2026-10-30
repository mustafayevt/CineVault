# torrent-sources Specification

## Purpose
The sources behind Torrent search: the six built-in direct sources, the managed indexer service (Jackett) with its automatic provisioning, and the user's own Torznab instance; how a title is searched in several spellings and scopes, and how results are merged, checked for relevance, grouped and compared with the copy the library holds.

## Requirements

### Requirement: Built-in sources work without setup
The app SHALL ship with six built-in sources, each on by default and ready to use with no account, key or setting: Torrentio (movies and episodes by IMDb ID), YTS (movies by IMDb ID), EZTV (TV by IMDb ID), The Pirate Bay (text search on its JSON interface), Knaben (text search over many public trackers) and Nyaa (text search, anime titles only). Each source SHALL have its own on/off switch (`torrents.source.<id>`) and an editable web address (`torrents.url.<id>`).

#### Scenario: First search
- **WHEN** a new user accepts the Torrent search notice and searches a movie that has an IMDb ID
- **THEN** Torrentio, YTS, The Pirate Bay and Knaben are asked at once, with no other setting needed

#### Scenario: Source switched off
- **WHEN** the user switches a source off in Settings and searches again
- **THEN** that source is not contacted

### Requirement: Sources that do not apply are skipped
A source that does not apply to the search SHALL be skipped with its reason and not contacted: a movie-only source for a show, a TV-only source for a movie, an anime-only source for a title that is not anime, an ID-based source for a title without an IMDb ID, a source that cannot search a whole show, and a switched-off source. Skipped sources SHALL NOT count in the page's source count.

#### Scenario: Movie search
- **WHEN** releases are searched for a movie that is not anime
- **THEN** EZTV is skipped as TV only and Nyaa as anime only

#### Scenario: No IMDb ID
- **WHEN** the chosen title has no IMDb ID
- **THEN** the ID-based sources are skipped and the text sources still run

### Requirement: Sources answer independently
Results SHALL appear as each source answers. Each source SHALL have its own deadline of 25 seconds, each request a time limit of 20 seconds and an answer limit of 8 MiB, and the app SHALL send at most one request per second to any one built-in site. A source that fails, is rate limited (HTTP 429), answers with a challenge page or times out SHALL be reported with that reason and SHALL NOT stop the others.

#### Scenario: Slow source
- **WHEN** one source has not answered after 25 seconds
- **THEN** the list already shows the other sources' results and that source is reported as timed out

#### Scenario: Rate limited
- **WHEN** a source answers HTTP 429
- **THEN** that source is reported as rate limited and the rest of the list is unaffected

#### Scenario: Retry after failures
- **WHEN** two sources failed and the user chooses "Retry all"
- **THEN** only those two are asked again, and the rows already listed stay

### Requirement: Managed indexer service
While the Torrent search notice is accepted and `torrents.indexer-service.enabled` is on (the default), the app SHALL download the indexer service by itself through the external tool manager (pinned version and SHA-256) as the Activity job "Downloading the indexer service". It SHALL run without a window, bound to `127.0.0.1` on a free port, with its own updater disabled, and its API key and admin password SHALL be kept out of the log. The built-in sources SHALL keep working meanwhile.

#### Scenario: Download in the background
- **WHEN** the user accepts the notice and the indexer service is not installed
- **THEN** the job "Downloading the indexer service" starts in Activity, and a search started meanwhile returns results from the built-in sources

#### Scenario: Checksum mismatch
- **WHEN** the downloaded archive does not match the pinned SHA-256
- **THEN** nothing is installed, the card shows the failure with "Try again", and searches still run on the built-in sources

#### Scenario: Loopback only
- **WHEN** the indexer service is running
- **THEN** it accepts connections only on `127.0.0.1`

### Requirement: Indexer service lifecycle
The indexer service SHALL run while the app runs, with no idle timer. It SHALL start in the background at app start (once the notice is accepted) and right after it is installed, never waiting for the first search, and stop at once when it is switched off. If the app exits or crashes, the service SHALL end with it. At start, a leftover from an earlier run SHALL be stopped only when its program file is inside CineVault's install folder, never by name.

#### Scenario: Ready for the first search
- **WHEN** the app starts with the notice accepted and the indexer service installed
- **THEN** the service starts in the background without any search

#### Scenario: App killed
- **WHEN** the app is killed while the service runs
- **THEN** the service ends with it, and a leftover found at the next start is stopped before a new one starts

#### Scenario: User's own instance untouched
- **WHEN** the user already runs their own copy of the same program on its default port
- **THEN** the managed service uses another free port, and the user's instance keeps running

### Requirement: Indexer service updates
The app SHALL look up the newest release of the indexer service at most every 7 days and remember a newer version until it is installed. "Update indexer service" SHALL install it as the Activity job "Updating the indexer service" only when the release comes from the service's own GitHub project, is the same kind of file as the pinned one, is at most 300 MiB and publishes a SHA-256 digest; a download that does not match the digest SHALL be discarded.

#### Scenario: No digest published
- **WHEN** the newest release publishes no checksum for its download
- **THEN** nothing is downloaded, and the card says the release was not installed because it has no checksum

### Requirement: Automatic indexer provisioning
On the service's first start, after a version change, when the last check is older than 7 days, and on "Check sources now", the app SHALL run the background job "Checking torrent sources": set up every public indexer in a tracker language that offers movie or TV categories (or only "Other") and is not mainly adult or a built-in site, test each one, keep those that pass and remove those that fail with the reason. Each indexer that passes SHALL become a source at once.

#### Scenario: Zero-config provisioning
- **WHEN** the service starts for the first time
- **THEN** public movie and TV indexers are set up and tested in a background job with no user action, and a search during the job uses the indexers that passed so far

#### Scenario: Challenge-protected indexer
- **WHEN** an indexer's test fails because its site demands a browser check
- **THEN** it is listed as "Needs a challenge solver", searches do not wait for it, and the card explains how to set up a challenge solver

### Requirement: Indexer re-checks and user choices
Indexers that were unreachable or failed for a reason other than a browser check SHALL be tried again after 24 hours, and an indexer the user switched off SHALL stay off through every later check.

#### Scenario: A site that was down
- **WHEN** an indexer was unreachable at a check more than 24 hours ago
- **THEN** the next start checks it again instead of waiting for the 7 days

#### Scenario: User choice survives
- **WHEN** the user switched an indexer off and a later check runs
- **THEN** that indexer stays off

### Requirement: ID and title search on Torznab feeds
A Torznab feed (an indexer of the service or the user's own instance) whose capabilities offer a search by IMDb ID SHALL be asked by ID and by title text (with the year, or the season and episode code) side by side, never one instead of the other, and the answers merged with each release listed once.

#### Scenario: ID search next to the title search
- **WHEN** a movie with an IMDb ID is searched on a feed that offers the IMDb ID search
- **THEN** the feed is asked by ID and by "Title Year", and a release both answers return is one row

### Requirement: Own Torznab instance
In Settings the user SHALL be able to add their own Torznab instance (the feed address in `torrents.torznab.url` plus an API key) as one more source named "Your Torznab instance", with "Test connection", "Save" and "Remove". The key SHALL be stored in the OS keyring and never logged. The instance SHALL be searched next to the other sources and SHALL NOT replace or stop them.

#### Scenario: Own instance added
- **WHEN** the user enters a Torznab address and key and the test passes
- **THEN** the next search asks "Your Torznab instance" too, and its results are merged with the rest

#### Scenario: Wrong key
- **WHEN** the connection test gets an authentication error
- **THEN** Settings shows "The key was rejected"

### Requirement: Time for an aggregate instance
The user's own instance SHALL get up to 75 seconds per request and four times the usual source deadline, because an all-indexers feed asks every site before it answers.

#### Scenario: A slow aggregate instance
- **WHEN** the own instance's all-indexers feed takes 40 seconds to answer
- **THEN** its answer is used, and the source is not reported as timed out

### Requirement: Query variants
For text-search sources the app SHALL search several spellings of the title, in this order: as written; with apostrophes removed; with apostrophes replaced by a space; with punctuation removed; with `&` and "and" swapped; without accents; then the original title and the alternative titles. Duplicates SHALL be dropped and at most 3 text queries sent per source.

#### Scenario: Apostrophe
- **WHEN** releases are searched for "Example's Bay" (2019)
- **THEN** the text sources receive "Example's Bay 2019" and "Examples Bay 2019", and their results are merged

#### Scenario: Accents
- **WHEN** releases are searched for "Café Example"
- **THEN** "Café Example" and "Cafe Example" are both searched, and nothing is searched twice

### Requirement: Scope suffixes on text queries
Movie queries SHALL carry the year when it is known; episode queries SHALL carry the episode code (`S01E02`) plus one query with the season code (`S01`) for packs; season queries SHALL carry the season code; whole-show queries SHALL carry only the title.

#### Scenario: Episode
- **WHEN** releases are searched for S01E02 of a show
- **THEN** each text source receives a query with `S01E02` and one with `S01`

### Requirement: Merged, de-duplicated results
Results from every source and query SHALL be merged into one list. Two results SHALL be the same release when their info-hashes match (hex or base32); when either has none, when their normalised names match and their sizes are within 1 MiB. A merged release SHALL list every source that returned it, keep the highest seeder count and the earliest date, and get a magnet link built from the info-hash and public trackers when no source gave one.

#### Scenario: Same release on three sources
- **WHEN** three sources return the same info-hash
- **THEN** one row is shown naming all three

#### Scenario: Base32 hash
- **WHEN** one source gives the info-hash as 32 base32 characters and another as 40 hex characters
- **THEN** they are one row

### Requirement: Relevance check
Each release name SHALL be parsed for title, year, season and episode. A release SHALL be listed only when its title matches the title, original title or an alternative title (ignoring case, punctuation, accents and spaces); for a movie, when its year is within one year or absent; for an episode or season, when it is that one or a pack that holds it. Releases that do not match SHALL NOT be listed or counted.

#### Scenario: Same words, other film
- **WHEN** searching "Example Movie (2021)" also returns "Example Movie 1984" releases
- **THEN** only the 2021 releases are listed

#### Scenario: Pack for an episode
- **WHEN** searching S02E05 returns a "S02 Complete" pack
- **THEN** the pack is listed

#### Scenario: Other season
- **WHEN** searching S02E05 returns an "S03E05" release
- **THEN** it is not listed

### Requirement: Comparison with the library copy
When the title is in the library with a video file, each release SHALL be compared with that copy: "Better" when its resolution is higher, or equal with a better source (remux, then disc, web download, web rip, TV capture, DVD) or HDR added; "Same as yours" when resolution, source rank and HDR presence match; otherwise "Worse". A cam, telesync, telecine or screener SHALL always be "Worse". When either resolution is unknown, the source and HDR SHALL decide.

#### Scenario: 1080p in the library
- **WHEN** the library copy is 1080p WEB-DL and the list holds a 2160p BluRay release and a 1080p WEB-DL release
- **THEN** the first is "Better" and the second "Same as yours"

### Requirement: Browser access to the indexer service
When the interface is served to a browser, the managed indexer service still binds `127.0.0.1` on the server. The browser SHALL reach its pages through an authenticated server route, or the page SHALL show the server's address and port with a sentence saying it is reachable only from the server or a proxy. "Your Torznab instance" addresses SHALL be interpreted on the server, not on the client, and a loopback address SHALL be labelled as the server's loopback.

#### Scenario: Open the indexer manager
- **WHEN** the user chooses to open the indexer service from a browser
- **THEN** the manager opens through the authenticated route, or the page shows the server's address and says it is server-only

#### Scenario: Own Torznab on loopback
- **WHEN** the user enters `http://127.0.0.1:9117` as their Torznab address in a browser
- **THEN** the page says that address is the server's loopback and the test runs from the server

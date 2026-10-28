# metadata-providers Specification

## Purpose
Covers the online metadata sources CineVault uses and how it talks to them. TMDB is the main provider and needs the user's own key; OMDb and MDBList are optional keys that add ratings. A few providers that need no key also answer searches. This capability covers the Providers section of Settings, how keys are stored, tested and kept secret, which providers take part in a scrape, language and country, what a scrape writes, provider ID completion, and the shared HTTP behaviour: rate limits, retries, caching, answer size limits and sites that ask for a human check.

## Requirements

### Requirement: Keys the user can enter
Settings › Providers SHALL offer the required "TMDB key" (hint "Needed for Scrape.") and the optional "OMDb key" (hint "Adds IMDb, Rotten Tomatoes and Metacritic ratings."), "MDBList key" (hint "Adds Letterboxd and more ratings."), "Fanart.tv key" (hint "Adds more artwork.") and "TheTVDB key" (hint "Adds TV artwork and episode data."), each with a "Get a free key" link to the provider's own key page. The app SHALL ship no keys: every key is the user's own. The section hint SHALL read "Keys are stored in the system credential store, never in files." The Fanart.tv and TheTVDB sources SHALL take part only once their key is saved, and any link in the app that says one of them needs a key SHALL open Settings › Providers at that key.

#### Scenario: Fresh install
- **WHEN** a user opens Settings › Providers on a fresh install
- **THEN** the five key fields are empty and each offers "Get a free key"

#### Scenario: Key link in search
- **WHEN** a search says "TheTVDB needs a key" and the user follows the link
- **THEN** Settings › Providers opens at the TheTVDB key field

### Requirement: Keys live in the system credential store
Keys SHALL be stored in the operating system credential store (Windows Credential Manager, the macOS Keychain, or the Secret Service on Linux), never in the settings file. A portable copy SHALL use a credential service of its own, so it starts with no keys and never changes an installed copy's keys.

#### Scenario: Settings file holds no key
- **WHEN** the user saves a TMDB key and the settings file is inspected
- **THEN** the file does not contain the key

#### Scenario: Key kept in GNOME Keyring
- **WHEN** the user saves a TMDB key on a Linux desktop running GNOME Keyring and restarts CineVault
- **THEN** the key is still configured, and the keyring lists it under the service `CineVault`

### Requirement: Test, save and remove a key
A key field SHALL show a saved key only as dots and offer Show and Hide for a key being typed. Test (or Enter) SHALL check a typed key with the provider and save it only when accepted; a refused key SHALL show the provider's message or "<name> did not accept this key. Check that it is copied whole." A working key SHALL show "Works" and a refused saved key "Rejected". Remove SHALL delete the key and confirm with a toast whose Undo puts it back.

#### Scenario: Valid key
- **WHEN** the user pastes a TMDB key and presses Test
- **THEN** the key is checked with TMDB, saved, and the field shows "Works"

#### Scenario: Remove and undo
- **WHEN** the user removes the OMDb key and clicks Undo in the toast
- **THEN** the OMDb key is saved again and shows "Works"

### Requirement: Keys are never exposed
A key value SHALL never be shown after saving, logged, written to settings or cache files, used in a cache key, or returned by the IPC or the command line. Any saved value SHALL be masked in log lines and error text even where no pattern rule would catch it. Only each provider's status (active, missing or rejected) SHALL leave the credential store.

#### Scenario: Key never exposed
- **WHEN** the settings file, the logs and `cinevault providers --json` are inspected after a TMDB key was saved
- **THEN** none of them contains the key

### Requirement: Provider status on the command line
`cinevault providers` SHALL list each provider with its importance and whether the user's key is active, missing or rejected, with `--json` for machine output. TMDB SHALL be the one required provider.

#### Scenario: Missing key
- **WHEN** no TMDB key is saved and the user runs `cinevault providers`
- **THEN** TMDB is listed as required with its key missing

### Requirement: Providers that take part in a scrape
A scrape search SHALL ask every ready metadata provider: TMDB and OMDb once their keys are saved, and the providers that need no key (IMDb, TVmaze for TV shows, OFDb, fernsehserien.de, TheSportsDB, and Kodi XML scraper add-ons found in the app's `kodi_scraper` folder or an installed Kodi). A provider without its key SHALL send no request. TMDB SHALL be the default provider while its key is saved, else the first ready provider for the kind. MDBList SHALL be a rating source only.

#### Scenario: Default provider
- **WHEN** a TMDB key is saved and a New movie is searched on the Scrape page
- **THEN** TMDB is asked as the default provider

#### Scenario: Provider without a key
- **WHEN** no OMDb key is saved and a title is searched
- **THEN** OMDb sends no request

### Requirement: Search, lookup and details
Every metadata provider SHALL support search by text with an optional year, direct lookup by a provider ID it understands, and full details for a result. A TMDB lookup SHALL resolve an IMDb ID without a text search.

#### Scenario: Lookup by IMDb ID
- **WHEN** a movie has IMDb ID `tt0000001` and no TMDB ID
- **THEN** TMDB resolves it by the IMDb ID and returns its details without a text search

### Requirement: Language and country
Scraping SHALL use the "Metadata language" (default English) for titles, plots and artwork text, and the certification country chosen in Settings › Providers (shown as "Ratings: <country>", default United States). When the metadata language has no title or plot, the fallback language (default English) SHALL fill the gap. The release date SHALL be the title's original worldwide release date.

#### Scenario: Missing translation
- **WHEN** the metadata language is Turkish and TMDB has no Turkish plot for a film
- **THEN** the English plot is used and the Turkish title is kept if one exists

### Requirement: What a scrape writes
A scrape SHALL write the fields of the kind's scrape field selection, by default every field a scrape may write: title, original title, tagline, plot, year, release date, status (shows), ratings, Top 250 rank, runtime, certification, genres, spoken languages (movies), country, studios, collection (movies), cast, crew, trailer and each artwork type. A field lock SHALL keep its field, and with "write only empty fields" on a filled field SHALL be kept. Provider IDs SHALL only be added.

#### Scenario: Only empty fields
- **WHEN** "write only empty fields" is on and a movie with a plot but no tagline is scraped
- **THEN** the tagline is filled and the plot is unchanged

### Requirement: Provider keywords are never tags
A scrape SHALL NOT import a provider's keywords as tags, and no field selection SHALL offer tags. Tags a title already has from its NFO SHALL be kept unchanged by a scrape.

#### Scenario: Keywords ignored
- **WHEN** a movie whose TMDB entry has 20 keywords and whose NFO has the tag "Favourites" is scraped
- **THEN** it still has exactly the tag "Favourites" and none of the keywords

### Requirement: Trailers come with the scrape
A TMDB scrape SHALL store the provider's trailers and mark the preferred one (one in the request language, else the first), so the NFO references it. A YouTube trailer SHALL be written as `plugin://plugin.video.youtube/play/?video_id=<id>`.

#### Scenario: Trailer in the NFO
- **WHEN** a movie is scraped from TMDB and TMDB lists the YouTube trailer `abc123`
- **THEN** the movie's NFO has `<trailer>plugin://plugin.video.youtube/play/?video_id=abc123</trailer>`

### Requirement: Every scrape writes NFO and artwork
Details fetched from any metadata provider SHALL be followed by the same steps in the background: the NFO is written by the NFO settings, then the empty artwork types are filled from the artwork providers by the IDs the title has. When no artwork provider knows the title, the artwork step SHALL say "No artwork provider knows this title." and Activity SHALL show it. These steps SHALL never delay or fail the scrape.

#### Scenario: No artwork source
- **WHEN** a title scraped from OFDb has no ID any artwork provider understands
- **THEN** the NFO is still written and Activity shows that no artwork provider knows the title

### Requirement: Provider ID completion
After a scrape the app SHALL fill missing IMDb, TMDB, TVDB and Trakt IDs, and for movies the collection ID, from the providers that can resolve the title by an ID it already has, skipping the provider that just scraped it. For a title with an anime-site ID it SHALL first consult the offline anime ID cross-reference, loaded on the first such scrape and refreshed weekly. Completion SHALL only add IDs, and a provider that fails SHALL be skipped without failing the scrape.

#### Scenario: IDs completed
- **WHEN** a movie is scraped from TMDB
- **THEN** its IMDb ID is filled from TMDB's external IDs

### Requirement: A person's IMDb page
The Cast & crew panel SHALL open a person's own IMDb page (`https://www.imdb.com/name/<id>/`) when their IMDb ID is known. The ID SHALL be asked of TMDB by the stored TMDB person ID when needed and stored with the person, so it is asked once. When TMDB cannot give it, the panel SHALL fall back to a name search on IMDb.

#### Scenario: Open on IMDb
- **WHEN** the user chooses to open a cast member with TMDB person ID 10 on IMDb, and TMDB gives `nm0000001`
- **THEN** the browser opens `https://www.imdb.com/name/nm0000001/`

### Requirement: Rate limiting and retries
All providers SHALL share one HTTP client that gives each provider its own lane with a concurrency and rate cap (TMDB 4 at a time and 40 requests a second; OMDb 2 at a time and 5 a second), so a slow or throttled provider never holds up another. An HTTP 429 answer SHALL be retried up to 3 times, each waiting `Retry-After` but at most 30 seconds. A provider outage SHALL fail only that provider's requests.

#### Scenario: Rate-limited provider
- **WHEN** a provider answers 429 with `Retry-After: 2`
- **THEN** the request is retried after 2 seconds and other providers keep working

### Requirement: Response caching
Provider answers SHALL be cached on disk under the data location's cache folder: 15 minutes for API answers and one day for web pages. Identical concurrent requests SHALL be coalesced into one network call. A refresh by stored IDs SHALL bypass the cache. A cache folder that cannot be used SHALL cost only the cache.

#### Scenario: Repeat within the cache window
- **WHEN** the same title is searched twice within 10 minutes
- **THEN** the second search makes no network request for unchanged API calls

### Requirement: Answer size limits
A provider answer SHALL be refused once it passes 16 MiB, counted after decompression, and an answer that announces a larger size SHALL be refused before it is read. The IMDb ratings dataset download SHALL have a limit of 256 MiB and the anime ID database download 512 MiB. A refused answer SHALL fail only that request.

#### Scenario: A compressed bomb
- **WHEN** a provider answers with a small gzip body that expands past 16 MiB
- **THEN** the request fails with a size error, memory stays bounded, and other requests continue

### Requirement: A human check is unavailable, not a failure
When a web site (IMDb, OFDb or fernsehserien.de) answers with a human check (HTTP 202 or 403), the search SHALL show "<provider> is not available right now." in muted text without a Retry action, and the app SHALL not ask that site again for ten minutes. The other providers' results SHALL still be listed.

#### Scenario: IMDb asks for a human check
- **WHEN** IMDb answers HTTP 202 during a scrape search
- **THEN** the line reads "IMDb is not available right now.", other results are listed, and searches in the next ten minutes do not contact IMDb

### Requirement: Rejected and limited keys say so
When a provider rejects the user's key, a search SHALL say "<provider> turned the key down." with Set up, which opens the provider keys in Settings. An error from a rate limit or a rejected key SHALL say which of the two it is.

#### Scenario: Rejected key
- **WHEN** TMDB rejects the saved key during a search
- **THEN** the line says "TMDB turned the key down." and Set up opens the keys in Settings

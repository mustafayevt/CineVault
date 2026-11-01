## MODIFIED Requirements

### Requirement: Keys the user can enter
Settings › Providers SHALL offer the required "TMDB key" (hint "Needed for Scrape.") and the optional "OMDb key" (hint "Adds IMDb data to Scrape, and IMDb, Rotten Tomatoes and Metacritic ratings."), "MDBList key" (hint "Adds Letterboxd and more ratings."), "Fanart.tv key" (hint "Adds more artwork.") and "TheTVDB key" (hint "Adds TV artwork and episode data."), each with a "Get a free key" link to the provider's own key page. The app SHALL ship no keys: every key is the user's own. The section hint SHALL read "Keys are stored in the system credential store, never in files." The Fanart.tv and TheTVDB sources SHALL take part only once their key is saved, and any link in the app that says one of them needs a key SHALL open Settings › Providers at that key.

#### Scenario: Fresh install
- **WHEN** a user opens Settings › Providers on a fresh install
- **THEN** the five key fields are empty and each offers "Get a free key"

#### Scenario: Key link in search
- **WHEN** a search says "TheTVDB needs a key" and the user follows the link
- **THEN** Settings › Providers opens at the TheTVDB key field

### Requirement: Providers that take part in a scrape
A scrape search SHALL ask every ready metadata provider: TMDB and OMDb once their keys are saved, and the providers that need no key (TVmaze for TV shows, OFDb, fernsehserien.de, TheSportsDB, and Kodi XML scraper add-ons found in the app's `kodi_scraper` folder or an installed Kodi). A provider without its key SHALL send no request. TMDB SHALL be the default provider while its key is saved, else the first ready provider for the kind. MDBList SHALL be a rating source only.

#### Scenario: Default provider
- **WHEN** a TMDB key is saved and a New movie is searched on the Scrape page
- **THEN** TMDB is asked as the default provider

#### Scenario: Provider without a key
- **WHEN** no OMDb key is saved and a title is searched
- **THEN** OMDb sends no request

#### Scenario: IMDb data through OMDb
- **WHEN** an OMDb key is saved and a movie is searched on the Scrape page
- **THEN** OMDb is asked along with TMDB, and no request goes to the IMDb web site

## REMOVED Requirements

### Requirement: A human check is unavailable, not a failure
**Reason**: IMDb is no longer read from its web site, so the rule names only the web sites that remain. Replaced by "A web site's human check is unavailable, not a failure".
**Migration**: None.

## ADDED Requirements

### Requirement: A web site's human check is unavailable, not a failure
When a web site (OFDb or fernsehserien.de) answers with a human check (HTTP 202 or 403), the search SHALL show "<provider> is not available right now." in muted text without a Retry action, and the app SHALL not ask that site again for ten minutes. The other providers' results SHALL still be listed.

#### Scenario: OFDb asks for a human check
- **WHEN** OFDb answers HTTP 202 during a scrape search
- **THEN** the line reads "OFDb is not available right now.", other results are listed, and searches in the next ten minutes do not contact OFDb

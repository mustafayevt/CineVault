# title-lifecycle Specification

## Purpose
Covers what CineVault has done, is doing and will do to every title, in three stages (New → Scraped → Renamed) and three verbs (Scan, Scrape, Rename): how the stage and the flags of a title are derived, how a scrape attempt is remembered, how the stage reads on posters, in the table and on title pages, the pipeline counts behind the sidebar, the Library chips and the Scrape page, the live refresh of open pages, and which work runs on its own and which only when the user asks.

## Requirements

### Requirement: Three lifecycle stages
Every movie, show, season and episode SHALL be in exactly one stage:
- **New**: found by a scan; it has no provider ID, or its details are not in place;
- **Scraped**: it has a provider ID and its details came from a provider or from an NFO file;
- **Renamed**: scraped, and its folder and file names match the active naming style.

A rating source that found nothing, or an after-scrape step that is off, SHALL NOT keep a title from being Scraped.

#### Scenario: New file found
- **WHEN** a scan finds a new video and no NFO with a provider ID exists
- **THEN** the title is New

#### Scenario: Scraped
- **WHEN** a title is matched, its details are saved and its NFO is written
- **THEN** the title is Scraped, even if one rating source reported "not found"

#### Scenario: Renamed
- **WHEN** a scraped title's folder and file already match the naming style, or a rename to it was applied
- **THEN** the title is Renamed

### Requirement: Lifecycle flags
On top of its stage a title SHALL carry zero or more flags, in this order of importance: Working (a job is touching it now), Needs review (an uncertain match, with its reason), Not matched (with its reason), Missing (its files are gone or its library folder cannot be reached), Locked, and Rename pending.

#### Scenario: Offline folder
- **WHEN** the library folder of a title cannot be reached
- **THEN** the title carries the Missing flag with the reason "The library folder cannot be reached."

### Requirement: Rename pending
A scraped, unlocked title whose folder or file names differ from the active naming style, or whose rename would conflict, SHALL carry the flag Rename pending with the number of changes or the conflict. A locked title and a title that is not scraped SHALL never be Rename pending.

#### Scenario: Locked title
- **WHEN** the user locks a scraped title whose names differ from the naming style
- **THEN** it carries the flag Locked and not Rename pending

### Requirement: Not matched only after a scrape
Not matched SHALL mean that a scrape ran for the title and found no confident match: the search found nothing, the review was set aside, the provider had no key, or the provider failed. The title SHALL stay New with the flag and that reason. A title no scrape was tried for SHALL carry no Not matched flag: it is New, "Not scraped yet", and counted neither by Not matched nor by Needs review.

#### Scenario: Never scraped
- **WHEN** a scan has just found a title and no scrape has been tried for it
- **THEN** it is New with no flag and is not counted by Not matched

#### Scenario: A scrape found nothing
- **WHEN** a scrape of a title finds no candidates
- **THEN** it stays New with the flag Not matched and the reason "No candidates found"

### Requirement: Scrape attempt stored per title
Whether a scrape was tried for a title, and how it ended, SHALL be stored with the title, so posters, the table, the filters and every count read the same fact and agree after a restart. A successful scrape SHALL clear a stored miss, and the Undo of a scrape SHALL put the stored outcomes back as they were before it.

#### Scenario: Not matched survives a restart
- **WHEN** a scrape found no confident match for a title, the user set its review aside and restarts CineVault
- **THEN** the title still shows Not matched with its reason, on the poster, in the table and in the Not matched count

#### Scenario: Undo of an accept
- **WHEN** the user accepts a candidate for a Not matched title and then presses Undo
- **THEN** the title is Not matched again with its earlier reason

### Requirement: Stage tag on title pages
The stage tag of the movie and show pages SHALL name the stage in words ("New", "Scraped", "Renamed"). A title with the Not matched or Needs review flag SHALL show that flag's word ("Not matched", "Needs review") in its place.

#### Scenario: Not matched on the title page
- **WHEN** the user opens a movie whose last scrape found nothing
- **THEN** its stage tag reads "Not matched"

### Requirement: Stage glyphs on posters and in the table
Posters and the library table's Stage column SHALL show a stage glyph: a hollow dot for New, a check for Scraped and Renamed, and a warning triangle for a title with a problem (Not matched, an uncertain match, Missing). Every glyph SHALL carry its word as tooltip and accessible name ("Scraped", or "Not matched", "Uncertain", "Files missing"), so the stage is never shown by colour alone.

#### Scenario: Same stage in grid and table
- **WHEN** the user switches the Library from grid to table
- **THEN** each title shows the same glyph with the same name in both

### Requirement: One vocabulary of three verbs
User-facing text SHALL describe the app's work with three verbs: Scan (find files), Scrape (match the title and fetch its details, artwork and NFO) and Rename (tidy folder and file names, only when the user asks). The words Identify, Enrich, Rescrape, Scrape again, Change match, Organize and Search & scrape SHALL NOT appear in any UI message, and a test of the message catalogue SHALL fail when one does.

#### Scenario: Same verb everywhere
- **WHEN** titles are matched from the Scrape page and from a title page
- **THEN** both places and the progress card call it "Scrape"

### Requirement: Scrape only when asked
A scan SHALL never start a scrape: titles a scan finds stay New until the user scrapes them (from a title, a selection, the Scrape page or the command line). There SHALL be no setting that scrapes after a scan. A scrape that is accepted SHALL save the title's details, write its NFO and download its artwork.

#### Scenario: New files wait for the user
- **WHEN** a scan finds 12 new movies without NFO files
- **THEN** all 12 are New and wait on the Scrape page

### Requirement: Rename only when asked
Folder and file renames SHALL happen only when the user applies them on the Rename page or runs a rename step on the command line. No scan, scrape or other background step SHALL rename anything, and there SHALL be no setting that makes renaming automatic.

#### Scenario: Scraping does not rename
- **WHEN** a title whose folder differs from the naming style is scraped
- **THEN** its files keep their names and it carries the flag Rename pending

### Requirement: What runs on its own
The library work the app SHALL do without being asked is limited to: scan the library folders on start and when they change; read the NFO and artwork files of the titles a scan adds; read pending media info (and measure the aspect ratio while `media.detect-aspect-ratio` is on); prepare thumbnails; check which titles already have the naming style's names ("Checking names"); and run the after-scrape steps after each scrape.

#### Scenario: Idle library
- **WHEN** the app is open, no scrape is running and nothing changes on disk
- **THEN** no metadata provider is contacted and no file in the media folders is written

### Requirement: After-scrape steps
After each scrape the app SHALL download the title's artwork and run the steps that are switched on: ratings (`ratings.fetch-after-scrape-movie` and `ratings.fetch-after-scrape-show`, default on), the movie's TMDB collection (`movies.collections.auto-group`, default on) and checksums (`media.checksums-after-scrape`: Off, CRC-32, Picture hash or Both, default Off). Artwork, ratings and checksums SHALL run in the background and never delay or fail the scrape.

#### Scenario: Ratings after a scrape
- **WHEN** a movie is scraped while ratings after a scrape are on
- **THEN** its ratings are fetched in the background after the scrape is saved

### Requirement: Step outcomes stored per title
The outcome of the artwork, ratings and collection steps SHALL be stored per title (done, unchanged, skipped, needs a key, not found or failed) with one plain reason and the time, the ratings step also per rating source. A step that is switched off SHALL store nothing.

#### Scenario: Missing key
- **WHEN** ratings are fetched after a scrape and the OMDb source has no key
- **THEN** the scrape still succeeds, the title is Scraped, and the OMDb outcome is stored as needing a key

### Requirement: Pipeline counts
The library SHALL provide, per library type and from one grouped query, the number of titles in each stage and with each condition: New waiting for a first scrape, Uncertain, Not matched, Needs review, Rename pending, Locked, Missing and Working. Every count shown SHALL come from this one result, so no two places show different numbers for the same set.

#### Scenario: Stage chips
- **WHEN** the movie library holds 40 New, 300 Scraped and 900 Renamed titles
- **THEN** the Library's chips read All 1,240, New 40, Scraped 300 and Renamed 900, and each opens exactly those titles

### Requirement: Sidebar counts
The sidebar's Scrape count SHALL be the titles New waiting for a first scrape plus Uncertain plus Not matched, over movies and shows, matching the Scrape page's tabs (All, New, Uncertain, Not matched). Its Rename count SHALL be the titles that are Rename pending.

#### Scenario: Scrape count
- **WHEN** 7 titles wait for a first scrape, 3 are Uncertain and 2 are Not matched
- **THEN** the sidebar's Scrape count is 12

### Requirement: Live stage updates
When a write changes a title (a scrape applied, ratings stored, artwork placed, an edit saved, media info read, a rename applied or undone), the app SHALL announce which parts of which titles changed, coalescing a burst within 150 ms into one event. Open pages, lists and counts SHALL refresh only the parts that changed, in place, without the user reloading the page.

#### Scenario: Ratings land while the page is open
- **WHEN** the page of a newly scraped movie is open and its ratings fetch finishes
- **THEN** the ratings and the stage tag update without a reload

#### Scenario: Batch scrape
- **WHEN** a scrape of 200 titles saves them one after another
- **THEN** the Library and the counts follow along without the user refreshing anything

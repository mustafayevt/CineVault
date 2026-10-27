# tv-shows Specification

## Purpose
Defines the TV-specific behaviour of CineVault: the show, season and episode hierarchy, episode orderings, episode scraping by number and by title, missing and unaired episodes, new and duplicate episodes, show status and air information, season artwork and its fallback, and the show page (`/tv/$showId`) with its season sections and inline episode panels.

## Requirements

### Requirement: Show, season and episode hierarchy
Each TV show SHALL hold seasons, and each season its episodes; season 0 SHALL hold the specials. A season SHALL exist for every season number that has episodes, and a season SHALL carry its name and overview (kept with the show) and its own artwork and IDs. A file that holds several episodes SHALL be one file shared by those episodes.

#### Scenario: Season metadata
- **WHEN** a show is scraped from TMDB
- **THEN** each season receives its name, overview and poster address

### Requirement: Show page
The show page (`/tv/$showId`) SHALL show a header with the show's status ("TV show · returning series", "ended" or "cancelled") and its facts: "Created by", "On disk" as "<owned> of <total> episodes" (specials and unaired episodes left out), "Quality", "Folder", and "NFO" with "tvshow.nfo up to date" or "tvshow.nfo differs from the library". The addresses `/tv/$showId/season/$seasonNumber` and `/tv/$showId/episode/$episodeId` SHALL redirect to the show page.

#### Scenario: On-disk count
- **WHEN** a show has 17 episodes on disk and the provider lists 2 more aired regular episodes
- **THEN** "On disk" reads "17 of 19 episodes"

#### Scenario: An old season address
- **WHEN** the address `/tv/42/season/2` is opened
- **THEN** the show page of show 42 opens

### Requirement: Season sections
The show page SHALL list one collapsible section per season, newest first, with the specials last, and SHALL open the latest season that has files. Each section SHALL show the season's year and episode count, "<n> missing", "Complete" or "Not on disk", the size of its distinct files, and an "Artwork" button for numbered seasons.

#### Scenario: Opening a show
- **WHEN** the user opens a show with seasons 1 to 3 on disk and season 4 announced without files
- **THEN** the sections run 4, 3, 2, 1, and season 3 is open

### Requirement: Episode rows and panels
A season section SHALL list its episodes in number order with Title, Aired, Runtime, Quality, Size and Status ("On disk", "No file", "Missing", "Not aired yet"), including the provider's episodes the library lacks. Opening an episode SHALL show an inline panel with its plot, "Directed by", "Written by", the video, audio and subtitle summary, its NFO state, Play, Open folder, "Scrape episode" and "All streams".

#### Scenario: A missing episode row
- **WHEN** the provider lists S02E04 and the library has no file for it
- **THEN** season 2 lists S02E04 with the status "Missing"

### Requirement: Episode orderings
A show SHALL use one episode ordering: aired, DVD, absolute, or a named alternate ordering the provider offers. A new show SHALL start in the ordering of "Episode order" (`tv.episode-order`: Aired (default), DVD, Absolute) when the provider offers it. Every episode SHALL keep its numbers in every ordering it was matched in, and the season list, episode rows and NFOs SHALL show the number in the chosen ordering, falling back to the aired number.

#### Scenario: DVD order by default
- **WHEN** "Episode order" is DVD and a new show is scraped from a provider that has a DVD list for it
- **THEN** the show uses DVD order and its season list follows the DVD numbers

### Requirement: Switching an ordering
Switching a show's ordering SHALL fetch the provider's episode list in that ordering and match local episodes by provider episode ID, then by the number already stored in that ordering, then, in the absolute ordering, by the number in the file name. When the provider has no list in that ordering, the switch SHALL fail with `ordering_not_available` and the show SHALL keep its ordering. Locked episodes SHALL keep their numbers.

#### Scenario: Absolute numbers in file names
- **WHEN** a show uses the absolute ordering and a file is named `Example Show - 137.mkv`
- **THEN** it is matched to absolute episode 137 and gets that episode's aired season and number

#### Scenario: Ordering not offered
- **WHEN** a show is switched to DVD order and the provider has no DVD list for it
- **THEN** the switch reports `ordering_not_available` and the show keeps its ordering

### Requirement: Episode scraping
Scraping a show SHALL also scrape its episodes by number in the show's ordering, after the show itself was written; a provider without an episode list, or a failure there, SHALL NOT fail the show's scrape. Numbers and provider IDs SHALL always be recorded on a match, entity and field locks SHALL be honoured, and a whole-show scrape SHALL replace the show's missing episodes with the list entries no local episode matched.

#### Scenario: Locked episode
- **WHEN** an episode is locked and its show is scraped
- **THEN** the episode's fields and numbers stay as they were

### Requirement: Episode fields
The fields an episode scrape writes SHALL be those of the episode field selection (`metadata.scrape-fields-episode`): title, original title, plot, aired date, rating, runtime, studio, tags, guest actors, directors and writers, and thumb, with the do-not-overwrite setting applied.

#### Scenario: Plot left out of the selection
- **WHEN** the plot is not in the episode field selection and a show is scraped
- **THEN** the episodes keep their plots and receive the other selected fields

### Requirement: Scrape one episode
"Scrape episode" in an episode panel SHALL scrape that episode by its number. It SHALL confirm with "Scraped <title>", and report "No match for <title>" when the provider's list has nothing for its number.

#### Scenario: Scrape one episode
- **WHEN** the user chooses "Scrape episode" on S01E03 and the provider lists that number
- **THEN** the episode gets the selected fields and a toast says "Scraped <title>"

### Requirement: Episode search by title
An episode whose number is unknown SHALL be findable in the show's episode list by title: hits ranked by whole title, whole words, word starts, then similarity, with a minimum score of 0.5 and at most 20 hits. Assigning a hit SHALL give the episode the provider's numbers and IDs and scrape the selected fields. A hit that left the provider's list SHALL fail with `stale_pick`, and a locked episode SHALL not change.

#### Scenario: Assign an unnumbered episode
- **WHEN** an episode file has no number and the hit "A Quiet Harbour" is assigned to it
- **THEN** the episode gets that entry's season, number and IDs, and the entry leaves the show's missing list

### Requirement: Missing and unaired episodes
The app SHALL derive missing episodes from the provider's episode list: entries no local episode matched. An entry that airs after today SHALL be unaired: shown as "Not aired yet" and not counted as missing. Each season SHALL count its missing and unaired episodes, and the library SHALL offer the show filter "Missing episodes".

#### Scenario: Unaired episodes are not missing
- **WHEN** S05E10 airs next week and the library has no file for it
- **THEN** it is listed as "Not aired yet" and the season's missing count leaves it out

### Requirement: New episodes
A scan SHALL flag every episode it adds as New, without flagging an existing show again, and the library SHALL offer the show filter "New episodes".

#### Scenario: Weekly episode added
- **WHEN** a new episode file appears in a show folder and is scanned
- **THEN** the episode is flagged New and the show matches "New episodes"

### Requirement: Duplicate and uncategorised episodes
Two files that map to the same season and episode in the show's ordering SHALL be duplicates: both stay episodes, each row is marked, and the library SHALL offer "Duplicate episodes". A multi-episode file SHALL clash only when another file covers one of its numbers, and an episode whose file is gone SHALL not count. Episodes without valid numbering SHALL be listed as uncategorised, with the filter "Uncategorised episodes".

#### Scenario: A repack beside the original
- **WHEN** both `S01E03.mkv` and `S01E03.REPACK.mkv` exist
- **THEN** both are episodes, both rows are marked duplicate, and the show matches "Duplicate episodes"

#### Scenario: A deleted repack
- **WHEN** the repack file is deleted
- **THEN** the remaining episode is not a duplicate, and the gone episode's row is kept

### Requirement: Show status and air information
A show SHALL store its status (continuing, ended, cancelled, unknown), first aired date, end year, studios and runtime. The app SHALL derive from every known episode, owned and missing, the last aired date, the next air date, and for an ended or cancelled show the end date: the latest episode date, ignoring specials while the show has regular episodes. The library SHALL offer the "Status" filter.

#### Scenario: End date of an ended show
- **WHEN** a show has status Ended and its last known episode aired on 2018-06-10
- **THEN** its end date is 2018-06-10 and its end year 2018

### Requirement: Season artwork names
Season poster, fanart, banner and thumb SHALL be written to the season names of the artwork settings: `season01-<type>` in the show folder (`season-specials-<type>` for season 0, `season-all-<type>` for the all-seasons image), the same names inside the season folder, or `folder`, `poster` or `cover` in the season folder for posters.

#### Scenario: Specials poster naming
- **WHEN** a poster is written for season 0 with the default names
- **THEN** it is `season-specials-poster.jpg` in the show folder

### Requirement: Season artwork after scrapes
With a show's artwork, and after the episodes of a show, a season or one episode are scraped, the app SHALL fill each affected season's missing images from the providers that serve season images (TheTVDB, Fanart.tv, TMDB) by the best-image selection, else from the address the season recorded, and each episode's missing thumb from its recorded address. These runs SHALL only fill gaps, never replacing an image that is there.

#### Scenario: Season poster after the season's scrape
- **WHEN** episodes of season 1 are scraped and the season has no poster
- **THEN** `season01-poster.jpg` is written in the show folder, and no other season's images are touched

#### Scenario: Automatic runs keep a chosen image
- **WHEN** the user chose a season poster and later scrapes the show again
- **THEN** the chosen poster stays

### Requirement: Changing a season's artwork
A season section's "Artwork" button SHALL open the artwork picker on that season, with the candidates of the providers that serve season images, the same order and the same Undo as for a movie. The chosen image SHALL be written to the season names of the settings; the season's previous image of that type, under any name, SHALL move to the trash folder, and Undo SHALL bring it back.

#### Scenario: Change a season poster
- **WHEN** season 1 has a poster at `Season 1/folder.jpg` and the user picks a TMDB season poster
- **THEN** the new poster is written as `season01-poster.jpg`, `Season 1/folder.jpg` moves to the trash folder, and Undo puts the old file back

### Requirement: Show artwork fallback for seasons
When "Use the show's artwork for seasons without any" (`tv.season-art-fallback`, on by default) is on, a season without its own poster or fanart SHALL be given the show's in the show's data, marked as borrowed. No file SHALL be copied.

#### Scenario: A season without a poster
- **WHEN** season 2 has no poster and the fallback is on
- **THEN** season 2 carries the show's poster, marked as the show's, and no file is written

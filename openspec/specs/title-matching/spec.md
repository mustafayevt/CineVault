# title-matching Specification

## Purpose
Covers how CineVault decides which online title a local movie or TV show is, and how the user confirms it. Scrape is the one verb for choosing a match and fetching its details. This capability covers query parsing and ID shortcuts, confidence scoring with local evidence, the confident rule and automatic acceptance, the Scrape page (`/scrape`) with its queue, the match picker page (`/scrape/match/$itemId`), streaming search results from several providers, batch scrapes run from the command line or automation, locks, and undoing a change of match.

## Requirements

### Requirement: Scrape is the one verb for matching
Scrape SHALL be the only action that chooses a match and fetches its details. Scraping one title from the Library, its title page or its menu SHALL open the match picker page for that title. Scraping a selection of several titles SHALL open the Scrape page limited to those titles. A scrape SHALL move a New title to the Scraped stage and SHALL NOT rename or move its folder.

#### Scenario: One title
- **WHEN** the user chooses Scrape on one movie in the Library
- **THEN** the match picker page `/scrape/match/<id>` opens for that movie

#### Scenario: A selection
- **WHEN** the user chooses Scrape with 12 titles selected
- **THEN** the Scrape page opens listing those 12 titles

#### Scenario: Folder stays
- **WHEN** a New title is scraped
- **THEN** its stage becomes Scraped and its folder keeps its name until the user renames it

### Requirement: Query parsing and ID shortcuts
A search SHALL accept free text with an optional `(yyyy)` year between 1880 and 2100. It SHALL also accept ID shortcuts: `tt1234567`, `imdb:tt1234567`, `tmdb:123`, `tvdb:123`, and IMDb, TMDB (`/movie/` or `/tv/`) and TheTVDB web addresses. An ID shortcut SHALL be looked up directly with no text search. When a text query finds nothing and starts with a 4-digit number, the search SHALL be retried once without that number.

#### Scenario: Pasted IMDb link
- **WHEN** the user pastes `https://www.imdb.com/title/tt0000001/` into the search field
- **THEN** the title with IMDb ID `tt0000001` is looked up directly and listed as the exact result

#### Scenario: Year in the query
- **WHEN** the user searches `Example Movie (2004)`
- **THEN** the search text is "Example Movie" and the search year is 2004

#### Scenario: Leading number retried
- **WHEN** the query `2049 Example` finds nothing
- **THEN** the search is retried once with `Example`

### Requirement: Confidence scoring
Each candidate SHALL get a confidence between 0 and 1. The base score SHALL be the best title similarity (word letter-pair Sørensen–Dice or Jaro-Winkler) between the query and the candidate's title or original title, minus a year penalty and minus 0.01 when the candidate has no poster. A candidate found by an ID shortcut or a stored ID SHALL score 1.0.

#### Scenario: Exact ID
- **WHEN** the user searches `tmdb:12345`
- **THEN** the title with that TMDB ID is listed at 100%

### Requirement: Year penalty
The search year SHALL be the year typed in the query, else the year of the local title. The year penalty SHALL be 0 for the same year or no search year, 0.01 plus a thousandth per year for a year one off, 0.15 plus a thousandth per year (counted up to 30 years) for two or more years off, and 0.11 for an unknown candidate year or more than 100 years off.

#### Scenario: Year penalty orders equal titles
- **WHEN** the search year is 2004 and two candidates with equal title similarity are from 2004 and 1996
- **THEN** the 2004 candidate scores higher

### Requirement: A year that is off caps the score
A candidate whose year is one year off a known search year SHALL show at most 99%. A candidate whose year is two or more years off SHALL show at most 74%, so it is never confident and never accepted automatically.

#### Scenario: Years off
- **WHEN** the folder says 2020 and the only candidate with the same title is from 2000
- **THEN** it shows at most 74% and waits for the user

### Requirement: Local evidence refines confidence
When the local title has the facts, the score SHALL be adjusted by signed evidence terms: runtime within 5% of the file's runtime adds 0.05 and a larger difference subtracts 0.08; an original language that agrees with the primary audio language adds 0.03 and one that disagrees subtracts 0.03; agreement with a provider ID the title already has adds 0.10 and a conflict subtracts 0.10. The shown score SHALL be clamped to 0..1, while ranking and tie checks SHALL use the unclamped value.

#### Scenario: Runtime separates same-named titles
- **WHEN** a folder `Example Movie` without a year holds a 167-minute file with Russian audio, and two candidates of that name run 167 minutes in Russian and 99 minutes in English
- **THEN** the 167-minute Russian candidate scores higher

### Requirement: Ranking among equal scores
Candidates SHALL be ordered by their unclamped score plus a ranking bonus, then year, then the order they arrived in. The bonus SHALL favour the better known title (its vote count on a log scale, worth up to 0.10, with the provider's popularity figure used at half weight only when there are no votes) and a title that equals the searched text exactly (0.08). The bonus SHALL only order candidates and SHALL NOT change a score, so two candidates that score the same still tie.

#### Scenario: Well-known title leads
- **WHEN** neither the query nor the local title has a year and two candidates named "Example Movie" score the same, one with 15,000 votes and one with 20
- **THEN** the one with 15,000 votes is listed first and the two still count as tied

### Requirement: Confident means 90 percent and clear
A title SHALL count as confident when its top candidate scores 90% or more (unrounded) and is not tied with the second candidate. The rule SHALL be fixed, with no setting. The score badge of a candidate SHALL use the confident tone at 90% or more, the uncertain tone from 40%, and the unlikely tone below.

#### Scenario: Just below
- **WHEN** a title's top candidate scores 89.6%
- **THEN** it shows 90% but is not confident

#### Scenario: Tie
- **WHEN** the top two candidates both score 94%
- **THEN** the title is not confident

### Requirement: Scrape page queue
The Scrape page SHALL list on the left every title that waits for a scrape decision: New titles, Uncertain titles (a batch scrape found candidates it could not accept) and Not matched titles (a batch scrape found nothing), with the tabs All, New, Uncertain and Not matched and their counts. A scraped title sent from the Library SHALL be listed under All only. Choosing a row SHALL show its search on the right. When nothing waits, the page SHALL say "Nothing waits for a scrape".

#### Scenario: Tabs
- **WHEN** 7 New, 2 Uncertain and 1 Not matched titles wait
- **THEN** the tabs read All 10, New 7, Uncertain 2 and Not matched 1

### Requirement: New titles are searched while the Scrape page is open
While the Scrape page is open, the app SHALL search candidates for the New titles near the visible rows automatically, three at a time, with the default provider. This search SHALL write nothing: no title, NFO, artwork, stage or review change. Results SHALL be kept for the session, so returning to the page does not search the same titles again. Leaving the page SHALL stop starting new searches.

#### Scenario: Scores appear
- **WHEN** the Scrape page opens with 7 New titles never searched
- **THEN** their candidates and scores appear one after another and no file or title changes

#### Scenario: Leaving the page
- **WHEN** the user leaves the Scrape page while 4 New titles are still unsearched
- **THEN** no further searches start for them

### Requirement: Scrape page evidence and what will change
For the current title the Scrape page SHALL show the file evidence (full path, size, runtime and quality when known), the search bar, the candidates, and for the chosen candidate its details and a "What will change" summary: the title old and new, the stage New → Scraped, the files written (`movie.nfo` or `tvshow.nfo` and the number of artwork files) and that the folder "stays as is until you Rename". For the current match it SHALL say that nothing else changes.

#### Scenario: What will change for a new title
- **WHEN** the chosen candidate for `example.movie.2022.2160p.x265` is Example Movie (2022) with 8 artwork files
- **THEN** "What will change" shows the title becoming "Example Movie (2022)", the stage New → Scraped, "movie.nfo and 8 artwork files", and the folder staying as is until Rename

### Requirement: Scrape page actions
On the Scrape page the user SHALL be able to apply the chosen candidate with "Scrape with this" or Enter, go to the next title with "Skip" or the Down key (the skipped title stays queued), and search again by typing. A scraped title SHALL leave the queue at once and the next title SHALL be shown; a refused scrape SHALL put the title back.

#### Scenario: Skip
- **WHEN** the user presses Skip on the current title
- **THEN** the next title is shown and the skipped one stays in the queue

### Requirement: Accept all confident
The Scrape page SHALL offer "Accept all confident (N)", whose tooltip states the confident rule, to scrape every confident title with its top candidate. It SHALL be disabled with a reason while no title is confident, and SHALL record one History entry whose Undo takes every accepted title back.

#### Scenario: Accept all confident
- **WHEN** 7 titles in the queue have a confident top candidate and the user clicks "Accept all confident (7)"
- **THEN** the 7 titles are scraped with their top candidates, leave the queue, and one History entry records them

### Requirement: Match picker page
The match picker page SHALL show the title's poster, name, full path, size and runtime, Cancel (Esc), the search bar focused on open, the current match pinned first under "Current match" with its score, the other results under "N more results from <providers>", and for the chosen result its details, its artwork and a Now / After comparison. "Scrape with this" or Enter SHALL scrape the title with the chosen result and close the picker.

#### Scenario: Pasted link
- **WHEN** the user pastes `https://www.themoviedb.org/movie/12345` into the match picker
- **THEN** that title is the single exact result and is chosen

### Requirement: Now and After comparison
The match picker's comparison SHALL show, now and after, the title, director, runtime, NFO and artwork files ("Replaced (old files to Recycle Bin)") and folder ("at next Rename"). When the chosen result's runtime differs from the file, it SHALL warn "Runtime differs from your file by N minutes".

#### Scenario: Runtime warning
- **WHEN** the chosen result's runtime differs from the file by 69 minutes
- **THEN** the comparison warns "Runtime differs from your file by 69 minutes"

#### Scenario: Current match chosen
- **WHEN** the chosen result is the current match
- **THEN** the comparison says scraping again refreshes its details and nothing else changes

### Requirement: Search bar
The search bar of the Scrape page and of the match picker SHALL offer a Movie | TV show switch, one field prefilled with the parsed title (labelled "Title, or an IMDb or TMDB link or ID", hint "or paste a link or ID") and a Year field prefilled with the local year. Typing SHALL search again after a short pause. A pasted link or ID SHALL be searched on its own, without the year.

#### Scenario: Search as a TV show
- **WHEN** the user switches the search to TV show
- **THEN** the providers are asked again for TV shows with the same text

### Requirement: Search results appear as they are found
The search SHALL ask every ready metadata provider that handles the kind at once and list each provider's results (up to 25 per provider) as soon as it answers. Results that share an IMDb, TMDB or TVDB ID SHALL be merged into one entry that lists every provider and keeps the highest confidence. The list SHALL stay ordered by rank while it grows, a skeleton SHALL show while nothing has arrived, and the heading SHALL read "N results from <providers>".

#### Scenario: Same title from two providers
- **WHEN** TMDB and OMDb both return Example Movie (1995) with IMDb ID `tt0000001`
- **THEN** one entry is listed, naming both providers

### Requirement: Provider notes in a search
Below the results, the search SHALL show one quiet line for each provider that did not answer: "<provider> could not answer." with Retry, "<provider> needs a key." or "<provider> turned the key down." with Set up (which opens the provider keys), and "<provider> is not available right now." in muted text with no action. Retry SHALL ask only that provider again.

#### Scenario: Retry one provider
- **WHEN** TMDB could not answer and the user clicks Retry on its line
- **THEN** only TMDB is asked again and its results join the list

### Requirement: Nothing found explains why
When every provider has answered and the search found nothing, the search SHALL say why: no provider is set up, a key is missing, the providers cannot be reached, or nothing matches the query. Without a reason it SHALL say "No title found for this search. Change the title or paste an ID."

#### Scenario: Offline
- **WHEN** no provider can be reached
- **THEN** the search says the providers cannot be reached instead of claiming nothing exists

### Requirement: Search work stops with the search
Choosing a result before the search has finished SHALL work. Typing a new query or leaving the page SHALL cancel the outstanding provider work of the old search, and results that arrive after that SHALL be dropped.

#### Scenario: A new query
- **WHEN** the user types a new query while providers are still answering the old one
- **THEN** only results of the new query are listed

### Requirement: The best match is chosen for you
When candidates are listed, the best-ranked candidate (other than the current match, unless it is the only one) SHALL be chosen as soon as it arrives, so Enter applies it. While the user has not moved the choice, a better candidate that arrives later SHALL become the choice. Once the user clicks a candidate or moves with the Up and Down keys, the choice SHALL stay. Changing the text, year or kind SHALL start again with the best of the new results.

#### Scenario: A better result arrives late
- **WHEN** the choice is the best result so far, the user has not moved it, and a better-ranked result arrives
- **THEN** the choice moves to the new result

#### Scenario: The user has chosen
- **WHEN** the user clicks a result and a better-ranked result arrives
- **THEN** the user's result stays chosen

### Requirement: Title already in the library
When a candidate is already linked to another library item, its details SHALL say "Already in your library as <name>. It is scraped as a second version." Scraping it SHALL be allowed.

#### Scenario: Second copy
- **WHEN** the chosen candidate is the match of another movie in the library
- **THEN** the details name that movie and scraping makes this title a second version of it

### Requirement: Scrape writes through the scrape rules
Applying a match SHALL link the title to the chosen result, write the kind's selected scrape fields, complete missing provider IDs, then write the NFO and fetch missing artwork in the background. A locked title SHALL NOT be changed ("This title is locked, so it was not changed."), and a field lock SHALL keep its field. A value the provider lacks SHALL never blank a stored value. The scrape SHALL confirm with a toast "Scraped <title>" that offers Undo.

#### Scenario: Locked plot
- **WHEN** a movie's plot is locked and the user scrapes it
- **THEN** every other selected field updates and the plot stays

#### Scenario: Locked title
- **WHEN** the user scrapes a locked title
- **THEN** nothing is written and the error says the title is locked

### Requirement: A match change is undoable
Scraping a scraped title with a different match, and Accept all confident, SHALL be recorded in History. Before the new match is written, the title's fields, IDs, ratings, people, collection and step outcomes SHALL be kept, and its NFO and artwork files SHALL go to the Recycle Bin / Trash or the library folder's trash folder. Undo SHALL restore the old match, details and files and send the files the new match wrote to the bin.

#### Scenario: Undo a match change
- **WHEN** a movie was scraped as the wrong title and the user undoes it in History
- **THEN** its old match is back with its NFO and artwork files

### Requirement: Batch scrapes
The app SHALL offer these batch scrape modes, used by the command line and automation: scrape New titles; scrape titles no provider has scraped; scrape a selection; refresh a selection by its stored IDs with no search and no review (asking for fresh data); and scrape every selected title with its top candidate whatever its score. Locked titles SHALL be skipped before any request.

#### Scenario: Refresh by IDs
- **WHEN** a batch refreshes 50 scraped movies by their IDs
- **THEN** each is refreshed by its stored ID and no search or review happens

### Requirement: Batch scrapes run in the background
Batch scrapes SHALL run as background jobs with progress and Cancel, one job per provider, and SHALL never wait for user input. A provider or database error SHALL fail only its title, with failures grouped by reason in Activity. A cancelled batch SHALL leave each title either fully scraped or untouched. Unfinished batches SHALL resume after a restart.

#### Scenario: Cancel
- **WHEN** the user cancels a batch halfway
- **THEN** every title is either fully scraped or unchanged

### Requirement: ID-first identification in batches
In a batch scrape, a title that already has an ID the provider can look up (from its NFO, folder or file name, or an earlier scrape) SHALL be looked up by that ID directly, scored 100%, with no text search.

#### Scenario: NFO with a TMDB ID
- **WHEN** a batch scrapes a New movie whose NFO has `<uniqueid type="tmdb">12345</uniqueid>`
- **THEN** it is matched to TMDB title 12345 at 100% without a text search

### Requirement: Automatic acceptance in batches
A batch scrape SHALL accept the top candidate only when it scores at or above the auto-accept threshold (`matching.auto-accept-threshold`, default 75%) and is not tied with the second, even for a single result. When this cannot decide and the file's runtime is known, the runtimes of the top three candidates SHALL be fetched and the ranking repeated once. Otherwise the title SHALL wait as Uncertain or Not matched with its reason and up to five candidates.

#### Scenario: Tie goes to the queue
- **WHEN** the top two candidates score the same
- **THEN** the title is not scraped and waits as Uncertain with the reason "2 strong candidates"

#### Scenario: Nothing found
- **WHEN** the search finds no candidate
- **THEN** the title waits as Not matched with the reason "No candidates found"

#### Scenario: Clear match accepted
- **WHEN** the best candidate scores 94% and the second 41%
- **THEN** the title is scraped automatically

### Requirement: Scraping needs the TMDB key
While no TMDB key is saved, batch scrapes SHALL NOT start and SHALL fail with a message that asks for the key; titles keep their local NFO and artwork data. When the Scrape page has nothing to list and the key is missing, it SHALL say "Add your TMDB key to start scraping" with "Open provider keys". Saving the key SHALL let held-back batches run again. Searching on the Scrape page and in the match picker SHALL still use the providers that are ready.

#### Scenario: Key added
- **WHEN** the user saves a TMDB key while a batch was held back for lack of it
- **THEN** the batch runs

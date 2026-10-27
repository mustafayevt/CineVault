# ratings Specification

## Purpose
Covers how CineVault collects, stores and shows a title's ratings from several sources, the user's own rating and the IMDb Top 250 rank. This includes the rating sources and their scales, where ratings are fetched from (the scrape itself, the free IMDb ratings dataset, OMDb and MDBList), what is recorded when a source cannot answer, the preferred source, the ratings strip on title pages with each source's mark and link, filters and sorting, live refresh, and the library-wide fetch offered when a new key is saved.

## Requirements

### Requirement: Ratings per source
Each title SHALL store any number of ratings, one per source, with value, scale top, votes, fetch time and, when named, the title's page address there. Known sources SHALL include IMDb, TMDB, Trakt, Letterboxd (out of 5), Rotten Tomatoes critics and audience (percentages), Metacritic (out of 100) and Metacritic users, MyAnimeList, Roger Ebert (out of 4), TheTVDB and TVmaze. Values SHALL keep the source's own scale and be written as the source writes them.

#### Scenario: Letterboxd scale
- **WHEN** a Letterboxd rating of 4.4 is fetched
- **THEN** it is stored with a maximum of 5 and shown as "4.4"

#### Scenario: Rotten Tomatoes percentage
- **WHEN** a Rotten Tomatoes critics score of 92 is stored
- **THEN** it is shown as "92%"

### Requirement: Rating sources to fetch from
A rating fetch SHALL ask the selected sources in this order: the free IMDb ratings dataset, the scraping provider's ratings, OMDb (IMDb, Rotten Tomatoes, Metacritic) and MDBList (Letterboxd, Rotten Tomatoes, Metacritic, MyAnimeList, Roger Ebert and more). By default it SHALL ask the IMDb dataset, OMDb and MDBList; the scraping provider's rating comes with the scrape. When two sources give the same rating, the one with more votes SHALL win, and on a tie the earlier source.

#### Scenario: Ratings with an MDBList key
- **WHEN** ratings are fetched for a movie and an MDBList key is saved
- **THEN** its Letterboxd, Rotten Tomatoes and Metacritic ratings are added or updated

### Requirement: Ratings are fetched after a scrape
After a movie or show is scraped, its ratings SHALL be fetched in the background by default. The fetch SHALL never delay or fail the scrape.

#### Scenario: A new show
- **WHEN** a new show is scraped with default settings
- **THEN** its ratings are fetched after the scrape

### Requirement: The free IMDb ratings dataset
The IMDb ratings dataset SHALL be downloaded on first use into the app's cache folder, never shipped with the app, and need no key. A copy younger than one day SHALL be used without a request; an older copy SHALL be checked with one conditional request and kept when unchanged. When the server cannot be reached, the old copy SHALL be used. An interrupted download SHALL leave the previous copy intact.

#### Scenario: Offline after a first download
- **WHEN** the dataset was downloaded yesterday and the network is down
- **THEN** IMDb ratings are still read from the stored copy

### Requirement: Rating fetch outcomes
A source whose key is not saved SHALL be skipped without a request, and the other sources SHALL still run. The outcome of every source (fetched, skipped for a missing key, nothing found, or failed) SHALL be recorded per title. When the IMDb dataset cannot be downloaded, Activity SHALL say "IMDb ratings: dataset download failed" once, with Retry. When OMDb, MDBList or the scraper fails to give ratings, Activity SHALL say "<source> ratings: the request failed" once.

#### Scenario: Source without a key
- **WHEN** OMDb is a selected source but no OMDb key is saved
- **THEN** OMDb sends no request, its outcome records the missing key, and the other sources still run

### Requirement: Storing ratings keeps locks and the user's rating
Fetched ratings SHALL be written through the scrape write path: a locked title and a locked ratings field SHALL not change, a source that answered SHALL replace only its own ratings, and the user's own rating SHALL never be touched.

#### Scenario: Locked ratings
- **WHEN** a movie's ratings field is locked and its ratings are fetched
- **THEN** its stored ratings stay as they were

### Requirement: Preferred rating source
The library's Rating column, the "Rating, highest first" sort and the `rating` filter SHALL use the preferred rating source (setting `ratings.preferred-source`, default TMDB). A title without a rating from it SHALL use the first available source in the fallback order (IMDb, TMDB, Trakt, TVmaze, TheTVDB, MyAnimeList, then the others). When a preferred source is stored in the settings, the default rating of movie and show NFOs SHALL follow it too.

#### Scenario: Preferred rating missing
- **WHEN** the preferred source is IMDb and a movie has no IMDb rating but a TMDB one
- **THEN** the Rating column shows the TMDB rating

### Requirement: Ratings strip on title pages
The movie and show pages SHALL show a ratings strip labelled "Ratings" with each rating that has a value, the preferred source first and then the fallback order. Each rating SHALL show its value in bold with its source's mark beside it. Hovering a rating SHALL show its votes and "Fetched <date>". Sources without a value SHALL be left out, and a title with no rating SHALL show no strip.

#### Scenario: Strip order
- **WHEN** the preferred source is TMDB and a movie has IMDb, TMDB and Letterboxd ratings
- **THEN** the strip lists TMDB first, then IMDb, then Letterboxd

### Requirement: Rating marks
Each rating SHALL show its source's logo, or a monogram of the same size for a source without one, in the source's brand colours. The critics' and the audience or user scores of one site SHALL each have their own mark. Each rating SHALL be named for screen readers by source and value, with "out of <max>" for values on a decimal scale.

#### Scenario: Two scores of one site
- **WHEN** a movie has the Rotten Tomatoes critics' and audience scores
- **THEN** both show, each with its own mark, and a screen reader names them apart

### Requirement: Rating links
A rating SHALL open the title's page on the source's site in the default browser: the page the source named when one is stored with the rating, else an address built from the title's IDs, else a search on the site by title and year. A source with none of these SHALL not be a link. Only absolute `http` and `https` addresses SHALL be opened. A link's tooltip SHALL say "Open the title on <source>".

#### Scenario: Open a rating's page
- **WHEN** the user clicks the IMDb rating of a movie whose IMDb ID is `tt0000001`
- **THEN** `https://www.imdb.com/title/tt0000001/` opens in the default browser

#### Scenario: A stored page wins
- **WHEN** MDBList named the Rotten Tomatoes page of a movie and the user clicks its Rotten Tomatoes rating
- **THEN** that page opens, not a search

### Requirement: Ratings refresh live
When ratings of a title are stored, the app SHALL announce that the title's ratings changed, and every open page that shows them (the title page and the library rows) SHALL refresh its ratings in place without touching its artwork or files.

#### Scenario: Ratings land after the scrape
- **WHEN** a movie's page is open and its after-scrape fetch stores an IMDb rating
- **THEN** the IMDb rating appears in the strip without a reload

### Requirement: User rating
The user SHALL be able to set their own rating, "Your rating", from 0.5 to 10 in half steps, on the Edit details page. It SHALL be stored apart from the source ratings, SHALL never be changed by a scrape or a rating fetch, and SHALL be filterable as "my rating".

#### Scenario: User rating survives a scrape
- **WHEN** the user rates a movie 9 and later scrapes it again
- **THEN** its user rating is still 9

### Requirement: Ratings in Edit details
The Edit details page SHALL let the user add, change and remove source ratings ("Add rating"), one row per source with its value, scale ("out of <max>") and votes. It SHALL refuse a source listed twice, a value outside 0 to the scale's top, a scale that is not a whole number above 0, and votes that are not a whole number.

#### Scenario: Source listed twice
- **WHEN** the user adds a second IMDb row
- **THEN** the page says "IMDb is listed twice. Keep one rating per source." and does not save

### Requirement: IMDb Top 250 rank
Each movie and show SHALL be able to hold an IMDb Top 250 rank from 1 to 250. The rank SHALL come from a scrape or the NFO, and the user SHALL be able to set it on the Edit details page. It SHALL be available as the `top250` filter, as a renamer token ("Top 250 place") and in the NFO. When the IMDb Top 250 chart is fetched, a rank SHALL be stored only when the whole chart was read; titles that left the chart SHALL lose their rank and locked titles SHALL keep theirs.

#### Scenario: Top 250 filter
- **WHEN** the user filters with `top250` from 1 to 50
- **THEN** only titles ranked in the top 50 are listed

### Requirement: Rating filters by source
Library search SHALL offer filters on the preferred rating (`rating`), on IMDb, TMDB, Rotten Tomatoes and Metacritic ratings, and on the user's own rating, each as a range on a 0 to 10 scale.

#### Scenario: Filter by IMDb
- **WHEN** the user filters IMDb rating from 8 to 10
- **THEN** only titles with a stored IMDb rating of at least 8 are listed

### Requirement: Fetch ratings for a new key
When the user saves an OMDb or MDBList key and scraped titles lack that source's ratings, the app SHALL offer "Fetch the new <source> ratings for N titles?" with a Fetch action, and say nothing when no title lacks them.

#### Scenario: New key offer
- **WHEN** the user saves an OMDb key and 24 scraped titles lack OMDb's ratings
- **THEN** a notice asks "Fetch the new OMDb ratings for 24 titles?" with Fetch

### Requirement: Library-wide ratings fetch
Fetch SHALL start one background job, "Fetching ratings", with progress and Cancel, over the scraped movies and shows that lack the chosen source's ratings, one title at a time with a short pause between titles. Cancel SHALL stop between titles and keep what was stored. When a source says its request limit is reached, the job SHALL stop asking without failing. The summary SHALL say how many titles got ratings, how many failed and, after a limit, how many are left.

#### Scenario: Ratings after adding a key
- **WHEN** a library was scraped without an MDBList key and the user saves the key and accepts the offer
- **THEN** the scraped titles get their MDBList ratings, and a later offer finds nothing missing

#### Scenario: Daily limit reached
- **WHEN** OMDb answers that its daily request limit is reached during the job
- **THEN** the job stops asking, the summary says "Stopped at the OMDb request limit: 40 titles are left. Run it again later to fetch the rest.", and the titles already fetched keep their ratings

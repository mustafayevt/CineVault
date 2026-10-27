# nfo-files Specification

## Purpose
Defines how CineVault writes and reads NFO metadata files for movies, collections, shows, seasons and episodes: the Kodi, Jellyfin, Emby and MediaPortal formats, their exact element content and order, CineVault's own elements, what a rewrite keeps from the old file, change-only atomic writes, file naming, tolerant reading, which files CineVault may overwrite or clean up, the NFO options, and the rule that the NFO of a scraped title is a complete record that is rewritten after every change.

## Requirements

### Requirement: NFO formats
The app SHALL write NFO files in the format chosen by `metadata.nfo-format`, one setting for movies, collections and TV: Kodi (the default), Jellyfin, Emby, XBMC (Kodi before 17), a preset that writes the Kodi format, and MediaPortal. Collection NFOs SHALL use the Emby layout when the format is Emby and the Kodi layout otherwise. A missing or unknown value SHALL fall back to Kodi, so no value turns NFO writing off.

#### Scenario: Jellyfin collection number
- **WHEN** the NFO format is Jellyfin and a movie belongs to a TMDB collection
- **THEN** its NFO holds the Kodi elements without `epbookmark`, `top250`, `status` and `code`, plus `<collectionnumber>` with the TMDB collection ID

#### Scenario: Unknown format value
- **WHEN** the settings file holds `metadata.nfo-format` = `bogus`
- **THEN** NFO files are written in the Kodi format

### Requirement: MediaPortal layouts
With the MediaPortal format, movie NFOs SHALL use the layout chosen by `metadata.nfo-mediaportal-style`: the original MediaPortal layout (the default), Moving Pictures or MyVideo. TV NFOs SHALL use the XBMC show layout with Kodi-layout episodes.

#### Scenario: Moving Pictures style
- **WHEN** the format is MediaPortal with the Moving Pictures style
- **THEN** movie NFOs write the certification as `mpaa` and repeat `<genre>` once per genre

### Requirement: Exact element content and order
For the same title data and options, a written NFO SHALL contain exactly the elements, attributes, value formats and element order of the expected files in `tests/compat/expected`, checked byte for byte by the compatibility fixtures. A file SHALL start with one XML declaration and the header comment `created on <time> by CineVault <version> for <format>`, which is ignored when files are compared.

#### Scenario: Golden-file equivalence
- **WHEN** a fixture movie is written with the fixture's settings
- **THEN** the NFO equals the expected file, apart from the header comment

#### Scenario: Element order of a Kodi movie
- **WHEN** a movie is written in the Kodi format
- **THEN** its elements run `title`, `originaltitle`, `sorttitle`, `epbookmark`, `year`, `ratings`, `userrating`, ... `fileinfo`, then the elements CineVault does not manage, then the `CineVault meta data` comment and CineVault's own elements

### Requirement: CineVault's own elements come last
CineVault's own elements SHALL come after the comment `<!--CineVault meta data-->` at the end of their root, and its additions inside `<fileinfo>` SHALL follow the standard elements of their parent. An addition without a value SHALL be left out. `<cv_locked>` SHALL list the title's locked fields, comma-joined. The compatibility fixtures SHALL be written without these additions.

#### Scenario: Movie additions
- **WHEN** a movie with a source, an edition, a note, an original language and 3D video is written
- **THEN** `source`, `edition`, `original_filename`, `user_note`, `english_title`, `original_language`, `video3d`, the crew and `cv_locked` follow the `CineVault meta data` comment, in that order

#### Scenario: Field locks in the NFO
- **WHEN** the plot and genres of a scraped movie are locked
- **THEN** its NFO holds `<cv_locked>plot,genres</cv_locked>`

#### Scenario: Empty additions are left out
- **WHEN** a movie has no media info
- **THEN** its `<fileinfo>` holds only the standard elements and none of CineVault's stream additions

### Requirement: Value formats
Written values SHALL follow fixed formats: ratings with one decimal on their own scale, rounding half up, with the default rating taken from the rating order (`ratings.preferred-source` when set); countries stored as codes written as English names and read back as codes; no `<role>` element for a person without a role; and the display aspect ratio snapped to the nearest common ratio with two decimals.

#### Scenario: Country names
- **WHEN** a movie's countries are stored as `JP` and `GB`
- **THEN** its NFO holds `<country>Japan</country>` and `<country>United Kingdom</country>`

#### Scenario: Rating rounding
- **WHEN** a rating is 6.85 out of 10
- **THEN** it is written as `6.9`

### Requirement: Stream value formats
Stream values SHALL be written in the spellings media centres read: the resolution as `4K` for 2160 lines, `8K` for 4320, else the line count; audio codecs in Kodi's spelling (`truehd`, `truehd_atmos`, `dtshd_ma`, `dca`, `eac3`, `eac3_ddp_atmos`, `ac3`, else lower case); the HDR type as `dolbyvision`, else `hdr10plus`, else the first format in lower case; and stream languages as ISO 639-2 codes.

#### Scenario: Stream details codec mapping
- **WHEN** a movie's audio is DTS-HD MA and its video HEVC 2160p with Dolby Vision
- **THEN** `<fileinfo>` holds audio codec `dtshd_ma`, video codec `hevc`, resolution `4K` and hdrtype `dolbyvision`

#### Scenario: YouTube trailer
- **WHEN** a movie's NFO trailer is a YouTube link
- **THEN** `<trailer>` holds `plugin://plugin.video.youtube/play/?video_id=<id>`

### Requirement: Preserve unknown content
When rewriting an existing NFO, the app SHALL read the first managed NFO of the title and keep, verbatim and in their old order, every direct child of the root that no CineVault writer manages, placed after the managed elements and before the `CineVault meta data` comment. With "Write clean NFO files" (`metadata.nfo-clean`, off by default) on, nothing SHALL be kept from the old file.

#### Scenario: Unknown element survives
- **WHEN** an existing NFO contains `<customfield>abc</customfield>` and the movie is scraped again
- **THEN** the rewritten NFO still contains `<customfield>abc</customfield>`

#### Scenario: Clean NFO
- **WHEN** "Write clean NFO files" is on and the old NFO holds `<customfield>abc</customfield>`
- **THEN** the rewritten NFO holds only what the library says, without `customfield`

### Requirement: Carried-over values
Unless "Write clean NFO files" is on, a rewrite SHALL carry over values the library does not hold: for movies `epbookmark`, `status`, `code`, `outline`, play count, last played and the other `<set>` entries of the Emby format; for shows `outline`, the old `episodeguide` and play count; for episodes `epbookmark`, `code`, `outline`, `trailer`, play count and last played. Play count SHALL be the higher of the stored and the old value, and last played the later one.

#### Scenario: Play count from a media centre
- **WHEN** the library says a movie was played once and its old NFO says `<playcount>3</playcount>`
- **THEN** the rewritten NFO holds `<playcount>3</playcount>`

### Requirement: Write only when content changes
The app SHALL compare the new NFO with the file on disk, ignoring XML comments, and write it only when the content differs. A write SHALL go through a temporary file renamed over the target, so a crash never leaves a half-written NFO, and a file another program holds briefly SHALL be retried.

#### Scenario: No-op rewrite
- **WHEN** Rewrite NFO runs on a movie whose data has not changed
- **THEN** the NFO file is not modified on disk and keeps its modification time

### Requirement: Encoding and layout
NFO files SHALL be UTF-8 XML with two-space indentation, CRLF line ends and attributes sorted by name. A file that holds several episodes SHALL get one `<episodedetails>` block per episode in a single NFO with one XML declaration; each block's runtime and duration SHALL be the file's duration divided by the number of episodes, and the Emby format SHALL write `<episodenumberend>` in every block.

#### Scenario: Multi-episode NFO
- **WHEN** a video file holds episodes S02E05 to S02E07
- **THEN** its NFO contains three consecutive `<episodedetails>` blocks and one XML declaration

### Requirement: Movie and collection NFO names
Movie NFO names SHALL follow `metadata.nfo-movie-names`: `<video base>.nfo` (default), `movie.nfo`, or both, with identical content. A folder holding several movies SHALL always use `<video base>.nfo`; a disc movie SHALL use `movie.nfo`, or `<main disc file>.nfo` in the disc folder with "NFO inside disc folders". Collection NFOs SHALL follow `metadata.nfo-set-names` (`<set>.nfo` by default, `set.nfo`, `collection.nfo`, or all three) in the collection folder, and are skipped without one.

#### Scenario: Both movie NFO names
- **WHEN** both `<video base>.nfo` and `movie.nfo` are chosen for a movie in its own folder
- **THEN** both files are written with identical content

#### Scenario: Shared folder
- **WHEN** `movie.nfo` is chosen and a movie sits in a folder with other movies
- **THEN** its NFO is written as `<video base>.nfo`

### Requirement: TV NFO names
A show's NFO SHALL be `tvshow.nfo` and an episode's `<video base>.nfo`. Season NFO names SHALL follow `metadata.nfo-season-names`: `season01.nfo` in the show folder with `season-specials.nfo` for season 0 (the default), `season.nfo` in the season folder, or both.

#### Scenario: Season NFO in the season folder
- **WHEN** season NFOs go in the season folder and the episodes of season 2 sit in `Season 2`
- **THEN** the season NFO is `Season 2/season.nfo`

### Requirement: Tolerant NFO reading
The NFO readers SHALL read files written by Kodi, XBMC, Jellyfin, Emby, MediaPortal, NextPVR (`<recording>`) and other tools without failing the whole file on one bad value. They SHALL keep what was read before a truncation or a mismatched tag, match names without regard to case, fold alternative spellings (`original_title`, `imdb_id`), and read several `<episodedetails>` blocks in a row or in a wrapper. The NFOs in `tests/nfo-read/` SHALL read to their expected values.

#### Scenario: Malformed XML
- **WHEN** an NFO has an unclosed tag after valid title and year elements
- **THEN** the title and year are read, and the file is not rejected

#### Scenario: Old encoding
- **WHEN** an NFO without a byte-order mark holds bytes that are not valid UTF-8
- **THEN** it is read as Windows-1252

### Requirement: IDs from any NFO
The readers SHALL take provider IDs from `<uniqueid>` elements, the common ID elements (`imdbid`, `tmdbid`, `tvdbid`, `collectionnumber`) and, when still missing, from IMDb numbers and provider URLs in the raw text. A file that holds only a provider URL or an ID SHALL yield that ID and no title.

#### Scenario: URL-only NFO
- **WHEN** an NFO contains only `https://www.themoviedb.org/movie/12345`
- **THEN** the reader yields TMDB ID 12345 and no title

### Requirement: NFO ownership
The app SHALL classify an NFO path as missing, empty, managed (it parses with a non-empty movie, collection, show, season or episode title), pointer (only a URL or an ID) or foreign (anything else). A write SHALL never replace a foreign file.

#### Scenario: Foreign NFO kept
- **WHEN** a movie folder contains a release-group `info.nfo` with no movie title
- **THEN** it is never overwritten or moved by NFO writing or renaming

#### Scenario: Pointer file written over
- **WHEN** a movie's `movie.nfo` holds only an IMDb ID and the movie is scraped
- **THEN** the NFO is written over it, and Undo brings the old text back

### Requirement: Stale NFO cleanup
Cleanup SHALL look only at the paths a title could have written itself: every naming choice, the names before a rename, the disc and season forms and the collection forms. It SHALL move a candidate to the trash folder only when it is managed and outside the configured naming set, and only after the title's NFO is in place under the current names.

#### Scenario: Stale managed NFO
- **WHEN** the movie naming changes from `movie.nfo` to `<video base>.nfo` and the movie's NFO is rewritten
- **THEN** `<video base>.nfo` is written first and the old `movie.nfo` then moves to the trash folder

### Requirement: Content options
The app SHALL apply these settings to every format: `metadata.nfo-language` (genre and spoken-language names; English, German and French, others fall back to English); `metadata.nfo-certification` (default `short`, `PG-13`); `metadata.nfo-create-outline` (off) with `metadata.nfo-outline-first-sentence` (on); `metadata.nfo-single-studio` (off); and `metadata.nfo-date-added` (on) with `metadata.nfo-date-added-source`.

#### Scenario: Date added from file creation
- **WHEN** date added is on with the source "File creation date"
- **THEN** the NFO's `<dateadded>` holds the video file's creation time as `yyyy-MM-dd HH:mm:ss` in local time

#### Scenario: Outline from the plot
- **WHEN** outline creation is on and a movie has a plot but no outline
- **THEN** its `<outline>` holds the plot's first sentence

### Requirement: Element options
The app SHALL leave out an element whose option is off: `metadata.nfo-artwork-urls` (on), `metadata.nfo-trailer` (on), `metadata.nfo-fileinfo` (on), `metadata.nfo-external-audio` (off), `metadata.nfo-lockdata` (off), `metadata.nfo-all-actors` (off, the episodes' guest actors in the show NFO), `metadata.nfo-end-date` (on, `<enddate>` of an ended show for Emby and Jellyfin) and `metadata.nfo-disc-folder` (off).

#### Scenario: No artwork addresses
- **WHEN** `metadata.nfo-artwork-urls` is off
- **THEN** movie NFOs hold no `<thumb>` or `<fanart>` addresses

### Requirement: Episode guide style
`metadata.nfo-episodeguide` SHALL choose the show's `<episodeguide>`: JSON with the show IDs (the default), a web address, or no element.

#### Scenario: Episode guide as JSON
- **WHEN** a show with TMDB, IMDb and TVDB IDs is written with the default style
- **THEN** its `<episodeguide>` holds a JSON object of those three IDs

### Requirement: Rewrite NFO action
The app SHALL offer "Rewrite NFO" from a title's page, its More actions menu and the Library selection bar, and from the command line (`--rewrite-nfo`). It SHALL write the NFOs of the scope from the library: a show with its seasons and episode files, a season with its episodes, an episode with its whole file. A change SHALL confirm with an Undo toast, and titles left out SHALL be listed with a reason.

#### Scenario: Rewrite with Undo
- **WHEN** the user chooses Rewrite NFO for three selected movies and two NFOs change
- **THEN** a toast says "Rewrote NFO files for 2 titles" with Undo, and Undo puts the previous files back

#### Scenario: Nothing to write
- **WHEN** Rewrite NFO runs on titles whose NFOs already match the library
- **THEN** the toast says "The NFO files are already up to date"

### Requirement: Read NFO action
Read NFO SHALL be available as the `read-nfo` automation step. It SHALL read the first managed NFO of each title and let the file win where it holds a value: lists with entries replace the library's, IDs and ratings merge one by one, and new trailer links are added. Locked fields, the date added, folders, files and locks SHALL stay unchanged, and an NFO that is not managed SHALL not be read.

#### Scenario: Read after an external edit
- **WHEN** a plot is edited in an NFO with a text editor and Read NFO runs for that title
- **THEN** the library holds the edited plot, and Undo restores the previous value

### Requirement: NFO preview
A movie, show or episode page SHALL show whether the NFO on disk matches what Rewrite NFO would write ("up to date", "differs from the library", "no NFO yet"). "Show NFO" SHALL show the exact text Rewrite NFO would write, without writing anything, with "Copy NFO".

#### Scenario: Preview a movie NFO
- **WHEN** the user chooses Show NFO on a movie whose NFO was edited outside CineVault
- **THEN** the page shows the text a rewrite would write and marks the NFO on disk as differing

### Requirement: NFO undo records
Every changing NFO action SHALL keep an undo record in the data folder with a copy of each file it replaced. Undo SHALL restore replaced files, move created files to the trash folder, bring trashed files back and restore changed library values. A file edited since, a path taken again or a title edited since SHALL be skipped and listed, never overwritten.

#### Scenario: A file edited after the write
- **WHEN** an NFO is edited in a text editor after a rewrite and the user then undoes the rewrite
- **THEN** that file is left as it is and listed as skipped

### Requirement: Undo record pruning
Undo records SHALL be pruned at start-up and after an action, at most every ten minutes: records older than 30 days, records beyond the newest 200, and the oldest records while all of them take more than 512 MB. A record from the last hour or among the newest 20 SHALL never be pruned.

#### Scenario: Old records go
- **WHEN** the undo folder holds 260 records and the oldest are two months old
- **THEN** after the next start the records older than 30 days and those beyond the newest 200 are gone

#### Scenario: A fresh Undo still works
- **WHEN** the user rewrites every NFO of a large library and the records pass 512 MB
- **THEN** the records of that rewrite are kept and its Undo restores the files

### Requirement: Complete NFO content
The NFO of a title SHALL carry every field the library stores as metadata of that title: all provider IDs, every rating with its votes and scale, the original title and language, the collection, all credits with their IDs and pictures, the dates, the trailer, the edition, the source, the user's note, the watched state, the field locks and the artwork addresses. Working state of the library (availability, review state, folders, rename records) SHALL be left out.

#### Scenario: Round trip
- **WHEN** a movie, a show and an episode with every written field filled are written and read back
- **THEN** each field read equals the library's, stream details included

#### Scenario: A field without a mapping
- **WHEN** a field is added to the library model without an NFO element or a stated reason
- **THEN** the contract test fails

### Requirement: Full stream details
`<fileinfo>` SHALL describe the video (codec, size, aspect, duration, HDR type and every HDR format, stereo mode, container, bitrates, frame rate, bit depth, file size), every audio stream (codec, language, channels, title, bitrate, bit depth, sample rate, flags) and every subtitle, embedded or in a file next to the video (language, format, title, flags, and `external` for a file). A stacked movie SHALL report all its parts together; a disc folder its main title.

#### Scenario: Subtitle file next to the video
- **WHEN** `Example Movie (2020).eng.forced.srt` sits next to the video
- **THEN** the NFO lists a subtitle with language `eng`, the `forced` flag and `external`

#### Scenario: Stacked movie
- **WHEN** a movie is split into `CD1` and `CD2` of 50 and 45 minutes
- **THEN** its `<durationinseconds>` is 5700

### Requirement: NFO files stay current
After every change of a scraped title's data that its NFO holds (a scrape, an edit, a field lock, ratings, artwork, media info, subtitles, watched state, a rename, a collection, a tag, a sync), the title's NFO SHALL be written again the way Rewrite NFO writes it. A burst of changes SHALL be written once, about 300 ms after the last. Only titles a scraper wrote SHALL be followed; a season or episode counts as its show. No setting SHALL stop NFO writing.

#### Scenario: A field lock reaches the NFO
- **WHEN** the user locks the plot of a scraped movie
- **THEN** the movie's NFO soon lists `plot` in `<cv_locked>`

#### Scenario: Unscraped title
- **WHEN** a title no scraper has written changes
- **THEN** its NFO file is not touched

#### Scenario: Tags and trailer survive a rewrite
- **WHEN** a scraped movie whose NFO has two `<tag>` elements and a `<trailer>` is edited
- **THEN** the new NFO has the same two tags and the same trailer, in the same places

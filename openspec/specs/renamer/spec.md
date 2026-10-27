# renamer Specification

## Purpose
Covers how CineVault names and reorganises title folders and files: the `${token}` template language and its renderers, the naming presets and profiles, name clean-up, the files that move with a title, multi-movie folders and TV naming, the Rename page (`/rename`) that previews every change before it happens, the journal that makes every rename undoable and recoverable after a crash, and the Naming section of Settings where the templates are edited.

## Requirements

### Requirement: Template syntax
The template engine SHALL accept `${token}`, dotted and indexed access (`${movie.title}`, `${actors[0].name}`, `${title[0,2]}`, `[last]`), optional segments `${prefix,token,suffix}` that disappear with an empty value, `${if token}…${else}…${end}` (also `!`, `=`, `==` and `!=`), `${foreach list item [separator]}…${end}`, the `${@regexp "pattern" token "replacement $1"}` processor, and named renderers chained with `;` with parameters in brackets. In text, `\${` SHALL be a literal `${`.

#### Scenario: Optional segment without a value
- **WHEN** `${title}${ - ,edition,} (${year})` is rendered for "Example Movie" from 2020 with no edition
- **THEN** the output is `Example Movie (2020)`, with no dangling separator

#### Scenario: Loop over a list
- **WHEN** `${foreach movies movie}${movie.title};${end}` is rendered with the movies "Example Movie" and "Another Movie"
- **THEN** the output is `Example Movie;Another Movie;`

#### Scenario: Chained renderers
- **WHEN** `${title;lower;first}` is rendered for "Example Movie"
- **THEN** the renderers apply left to right

### Requirement: Named renderers
The engine SHALL provide the renderers `first`, `upper`, `lower`, `title`, `array`, `uniqueArray`, `date`, `number`, `bitrate`, `filesize`, `framerate`, `split` and `replace`. The naming engine SHALL add its own `first` (with a configurable replacement for a leading digit), `replace(file.csv)` (a table read from the app's data folder) and the collection index renderers. Their output SHALL match the compatibility fixtures in `tests/compat/`.

#### Scenario: First-letter bucket
- **WHEN** `${title;first}` is rendered for "The Example" with sortable titles off
- **THEN** the output is `T`, and for a title starting with a digit it is `#` unless another replacement is configured

#### Scenario: Zero-padded number
- **WHEN** `${seasonNr;number(%02d)}` is rendered for season 3
- **THEN** the output is `03`

### Requirement: Movie naming tokens
For file and folder names the engine SHALL expose short movie token names, among them the title tokens, `year`, `edition`, `certification`, the people, genre and tag tokens, `rating`, `releaseDate`, the decade tokens, `movieSetIndex`, `imdb`, `tmdb`, the original file and folder name tokens, `filesize` and `crc32`, and the stream tokens. Export templates SHALL use the full model paths instead.

#### Scenario: Token names offered
- **WHEN** a movie template is written
- **THEN** it may use, among others, `title`, `originalTitle`, `englishTitle`, `titleSortable`, `year`, `edition`, `directors`, `writers`, `actors`, `genres`, `tags`, `rating`, `top250`, `releaseDate`, `decadeShort`, `decadeLong`, `movieSetIndex`, `imdb`, `tmdb`, `originalFilename`, `originalBasename`, `parent`, `folderName`, `filesize` and `crc32`

#### Scenario: Stream tokens
- **WHEN** a movie template needs quality tags
- **THEN** it may use `videoFormat`, `videoResolution`, `videoCodec`, `videoBitDepth`, `videoBitRate`, `framerate`, `aspectRatio`, `hdrformat` (also `hdrFormat`), `3Dformat`, `mediaSource`, `audioCodec`, `audioChannels`, `audioChannelsDot`, `audioLanguage` and the per-track audio and subtitle lists

#### Scenario: Detailed tags render in order
- **WHEN** `${title} (${year}) [${videoFormat}] [${mediaSource}] [${hdrformat}] [${audioCodec} ${audioChannelsDot}]` is rendered for a 2160p WEB-DL Dolby Vision HDR10 movie with TrueHD Atmos 7.1 audio
- **THEN** the output is `Example Movie (2024) [2160p] [WEB-DL] [Dolby Vision, HDR10] [TrueHD Atmos 7.1]`

### Requirement: TV naming tokens
For TV names the engine SHALL expose the show tokens, the season and episode number tokens with their two-digit, aired-order and DVD-order variants, the absolute number, the aired date and the episode IDs, besides the stream tokens.

#### Scenario: Token names offered
- **WHEN** a TV template is written
- **THEN** it may use, among others, `showTitle`, `showYear`, `showYearEnd`, `showImdb`, `showTmdb`, `showTvdb`, `seasonNr`, `seasonNr2`, `seasonName`, `episodeNr`, `episodeNr2`, `episodeNrAired`, `episodeNrDvd`, `absoluteNr`, `absoluteNr2`, `airedDate` and `episodeTvdb`

### Requirement: Empty values render empty
In names, numeric zero, missing values, empty text and empty lists SHALL render as empty text, so optional segments and the bracket clean-up remove them.

#### Scenario: Missing year
- **WHEN** `${title} (${year})` is rendered for a movie with no year
- **THEN** the raw output is `Example Movie ()`, which the clean-up turns into `Example Movie`

### Requirement: Template validation
The engine SHALL report syntax errors with their position, unknown tokens and renderers with suggestions, and, for TV, whether the episode file pattern has exactly one episode token and a season token in it or in the season-folder pattern. Validation SHALL never render or read files.

#### Scenario: Typo in a token
- **WHEN** a template contains `${titel}`
- **THEN** validation marks `titel` as unknown at its position and suggests `title`

### Requirement: Safe and bounded evaluation
Rendering SHALL NOT run code, use the network or read files, except `replace(file.csv)`, which reads CSV tables only from the app's data folder. A name render SHALL stop at 256 KiB of output, 2 seconds or one million steps; an export render at 256 MiB, 60 seconds or 100 million steps.

#### Scenario: Runaway template stopped
- **WHEN** a naming template would produce more than 256 KiB of text
- **THEN** the render stops with an error and no name is produced from it

### Requirement: Naming presets
The app SHALL ship three naming presets, offered as the naming styles of the first run: Clean, Detailed and Media-server safe. Each SHALL define the movie folder, movie file, show folder, season folder (`Season ${seasonNr2}`) and episode file templates.

#### Scenario: Clean
- **WHEN** the Clean preset is active
- **THEN** the movie folder is `${title} (${year})`, the movie file `${title}${ - ,edition,} (${year})`, the show folder `${showTitle} (${showYear})` and the episode file `${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}`

#### Scenario: Detailed preset output
- **WHEN** the Detailed preset, whose movie folder and file are `${title} (${year}) [${videoFormat}] [${mediaSource}] [${hdrformat}] [${audioCodec} ${audioChannelsDot}]`, names a 2160p BluRay HDR10 movie with DTS-HD MA 7.1 audio
- **THEN** the folder is `Example Movie (2017) [2160p] [BluRay] [HDR10] [DTS-HD MA 7.1]`, and episode files carry the same tags

#### Scenario: Media-server safe
- **WHEN** the Media-server safe preset names a movie with a TMDB ID and an edition
- **THEN** the folder is `Example Movie (2020) {tmdb-<id>}`, the file `Example Movie (2020) {edition-<edition>}`, and episode files are `${showTitle} - S${seasonNr2}E${episodeNr2}`

### Requirement: Naming profiles
The first change to a template in Settings SHALL copy the active preset into the user's own profile ("Custom") and make it active; later changes SHALL change that profile. The presets SHALL stay unchanged.

#### Scenario: Editing a preset makes a custom profile
- **WHEN** the active style is Clean and the user changes the Movie file template
- **THEN** a Custom profile holding the change becomes active and the Clean preset stays as shipped

### Requirement: Name clean-up
Every rendered name SHALL be cleaned before use: empty `()`, `[]` and `{}` removed, line breaks turned into spaces, path separators inside a file name turned into spaces (a folder pattern is split at `/` and each part cleaned on its own), leading and trailing separators trimmed, characters the file system cannot hold replaced or removed, and runs of spaces collapsed. A profile SHALL also offer space substitution, ASCII folding and Unicode look-alikes for invalid characters.

#### Scenario: Empty HDR bracket removed
- **WHEN** a 1080p SDR movie is named with the Detailed preset
- **THEN** the folder holds no `[]` where the HDR tag would be

#### Scenario: ASCII folding
- **WHEN** ASCII folding is on and the title holds `é` and `Ü`
- **THEN** the name holds `e` and `Ue`

### Requirement: Colon in titles option
Settings › Naming SHALL offer "Colon in titles" (`naming.colon`) with "Remove", "Use “ - ”" and "Use “꞉”" (the look-alike character U+A789). The default SHALL be " - ". The live examples SHALL follow the choice at once.

#### Scenario: Use a dash
- **WHEN** the option is "Use “ - ”" and the title is "Example: Part Two" from 2024
- **THEN** the folder example reads `Example - Part Two (2024)`

#### Scenario: Look-alike colon
- **WHEN** the option is "Use “꞉”"
- **THEN** the folder example reads `Example꞉ Part Two (2024)`

### Requirement: Naming templates in Settings
Settings › Naming SHALL hold the templates "Movie folder", "Movie file" and "TV episode" as text fields in the mono face, each with a live "Example:" line from the library under it, and a row of token buttons that insert at the cursor of the field last used. Any other engine expression SHALL be shown and kept as written.

#### Scenario: Token buttons
- **WHEN** the user looks at the token buttons
- **THEN** the movie fields offer {Title}, {Year}, {Resolution}, {HDR}, {VideoCodec}, {AudioCodec}, {Channels}, {Edition}, {Source}, {IMDb} and {TMDB}, and the episode field offers {Show}, {ShowYear}, {Season}, {Season00}, {Episode}, {Episode00}, {EpisodeTitle}, {Resolution}, {HDR}, {VideoCodec}, {AudioCodec} and {Source}

#### Scenario: Insert a token
- **WHEN** the cursor is after `{Year})` in Movie file and the user clicks {Resolution}
- **THEN** `{Resolution}` is inserted at the cursor and the example updates

### Requirement: Invalid templates are not saved
A template SHALL be saved only when its braces match, every `{Name}` is a token and the engine accepts it. Otherwise the field SHALL say why and the last valid template SHALL stay in use.

#### Scenario: Unclosed brace
- **WHEN** the user types `{Title` without the closing brace
- **THEN** the field says the brace in "{Title" is not closed, and the last valid template stays in use

### Requirement: Templates are edited only in Settings
The Rename page SHALL show the active folder and file templates in its header with an "Edit in Settings" link, and SHALL NOT edit them itself.

#### Scenario: Edit link
- **WHEN** the user clicks "Edit in Settings" on the Rename page
- **THEN** Settings opens at Naming

### Requirement: Companion files move together
Applying a rename SHALL move or rename every file of the title together and the library paths SHALL follow without a rescan. Videos SHALL keep their stacking markers; NFOs and artwork SHALL take every name of the profile; subtitles, trailers, samples and sidecars SHALL be named after the new base name; every other file and sub-folder SHALL move with the folder.

#### Scenario: Subtitle suffix kept
- **WHEN** `old.eng.forced.srt` belongs to a movie renamed to `Example Movie (2020)`
- **THEN** it becomes `Example Movie (2020).eng.forced.srt`, and a `.sub` takes its `.idx` along

#### Scenario: Newest NFO and artwork win
- **WHEN** a movie folder holds two NFOs and two posters under different names
- **THEN** the newest readable NFO is written to every NFO name of the profile, the newest poster to every poster name, and the older ones are set aside

#### Scenario: Sidecars follow the video
- **WHEN** a movie with a sample, a media-info XML, a BIF file and an extra outside a sub-folder is renamed
- **THEN** they become `<base>-sample`, `<base>-mediainfo.xml`, `<base>.bif` and `<base>` plus what followed the old base name, while files in sub-folders keep their names

### Requirement: Leftovers go to the deleted-files destination
Files a rename makes redundant SHALL go where Settings › Deleted files says: the library folder's `.deletedByCineVault/<batch>/` with "Recycle Bin" (the default) or "Trash folder on each drive", the custom trash folder for files on its drive with "Custom trash folder…", and erased only with "Delete permanently". Empty folders left behind SHALL be removed. A library folder root and a folder holding another title's files SHALL never be removed.

#### Scenario: Old artwork name set aside
- **WHEN** the artwork names change from `poster.jpg` to `<base>-poster.jpg` and the movie is renamed
- **THEN** the new file exists and the old `poster.jpg` is in the library folder's trash folder, not erased

### Requirement: Unwanted files clean-up
With "Move unwanted files to the trash" (`renamer.cleanup-unwanted`, off by default), files matching "Unwanted file types" (`renamer.unwanted-patterns`, default `.txt, .url, .html, .sfv`, extensions or patterns separated by commas) SHALL be set aside after a rename like other leftovers, never erased.

#### Scenario: Text file set aside
- **WHEN** the clean-up is on and a renamed movie folder holds `info.txt`
- **THEN** `info.txt` goes to the trash folder of the library folder

### Requirement: Keep sub-folders
Settings › Naming SHALL offer "Keep sub-folders" (`renamer.keep-subfolders`, on by default), applied to every rename. When it is on, a one-segment movie or show folder pattern SHALL render inside the folder that holds the title; a movie in a folder shared with other movies SHALL get its own folder inside it. When it is off, folder patterns SHALL render under the library folder. Seasons and episodes SHALL stay inside the show folder.

#### Scenario: A movie in a category folder stays in it
- **WHEN** the library folder is `D:\Movies`, Keep sub-folders is on, and the movie in `D:\Movies\Drama\example.movie.2020` is renamed with `${title} (${year})`
- **THEN** the folder becomes `D:\Movies\Drama\Example Movie (2020)`

#### Scenario: The setting off
- **WHEN** Keep sub-folders is off and the same movie is renamed
- **THEN** the folder becomes `D:\Movies\Example Movie (2020)`

### Requirement: Folder patterns with sub-folders start at the library folder
A folder pattern that renders more than one path segment (it contains `/`) SHALL render under the library folder whatever Keep sub-folders says, so a structure such as letter folders is never nested again on the next rename.

#### Scenario: Letter folders
- **WHEN** Keep sub-folders is on and the movie in `Drama/example.movie.2020` is renamed with `${title;first}/${title} (${year})`
- **THEN** the folder becomes `E/Example Movie (2020)` under the library folder, and renaming it again changes nothing

### Requirement: Multi-movie folders
A movie folder pattern that does not name every movie differently (it lacks title and year, or an ID) SHALL make the destination a shared folder: each movie's files are named after its video, and files shared by the folder (such as a plain `poster.jpg`) SHALL NOT be planned. A rename SHALL never merge into an existing folder that holds another title.

#### Scenario: Letter-only folder pattern
- **WHEN** the folder pattern is `${title;first}`
- **THEN** movies are placed in letter folders shared by several movies, and each movie's files are named from the file pattern

### Requirement: TV renaming rules
TV renaming SHALL render the show, season and episode patterns and name a file holding several episodes in the profile's style: repeated (`S01E01 S01E02`) or a range (`S01E01-E02`). It SHALL refuse an episode file pattern without an episode token. An episode without season and episode numbers SHALL keep its files' names and place.

#### Scenario: Multi-episode range naming
- **WHEN** a file holding S01E01 and S01E02 is renamed with the range style
- **THEN** its name contains `S01E01-E02` and both episodes point to the renamed file

#### Scenario: Specials and empty season folders
- **WHEN** the profile names season 0 `Specials`, or the season pattern renders empty
- **THEN** season 0 goes to `Specials`, and an episode with an empty season folder goes to the show folder when the pattern pair is valid, else to `Season N`

### Requirement: Rename page preview
No rename SHALL touch the disk until the user applies it. The Rename page (`/rename`, or `/rename?ids=…` for titles selected elsewhere) SHALL list, under the tabs "Movies" and "TV episodes" with their counts, every title whose folder or main file would change, each with a checkbox (all ticked at first). Renames SHALL stay inside each title's library folder; moves between drives belong to Drives.

#### Scenario: Tidy titles not listed
- **WHEN** a movie's folder and file already match the templates
- **THEN** it is not listed and not counted

#### Scenario: Nothing to rename
- **WHEN** every title is named as the templates say
- **THEN** the page reads "Everything is named as your templates say"

### Requirement: Rename page footer and result
The footer SHALL show "N of M selected", the conflict count and the primary "Rename N titles". After a rename a toast SHALL report the result ("Renamed N titles", or how many could not be renamed) with Undo.

#### Scenario: Exclude one title
- **WHEN** 64 titles are listed, the user unticks one and clicks "Rename 63 titles"
- **THEN** that title is not renamed, the other 63 are, and the toast says "Renamed 63 titles" with Undo

### Requirement: Word-level rename diffs
Each Rename row SHALL show the title with its year and library folder, then the folder and the main file as now → after pairs, with removed words struck through and added words highlighted. Whole words with their brackets SHALL change together, never split. "Show all N files" SHALL list every file as now → after pairs in place (set-aside files marked "to the trash"), and "Hide files" SHALL fold them.

#### Scenario: Word-level diff
- **WHEN** `Example.Movie.2020` becomes `Example Movie (2020)`
- **THEN** the diff keeps the words, shows the changed separators and the added `(2020)`, and splits no word

### Requirement: Inline rename conflicts
The preview SHALL detect and block in the row, until resolved: two titles or files ending at one destination; a destination that exists and belongs to another title, or a non-empty folder; a path or name over the file-system limit; and a change of case alone that would overwrite itself on a case-insensitive volume. A blocked title SHALL NOT be counted in Rename. A case-only rename of a single file SHALL go through a temporary name and is no conflict.

#### Scenario: Folder already exists
- **WHEN** `example.movie.2020.1080p` would become `Example Movie (2020)` and `D:\Movies\Example Movie (2020)` exists for another title
- **THEN** the row says "A folder named Example Movie (2020) already exists in D:\Movies." with "Use Example Movie (2020) (2)", "Open both folders" and "Skip"

#### Scenario: Path too long
- **WHEN** the new path would be longer than 260 characters on Windows without long-path support (1024 on macOS), or one name longer than 255
- **THEN** the row gives the length and the limit and offers "Skip"

### Requirement: Rename waits for video details
When a pattern a rename renders uses a token read from the video, audio or subtitle streams, a title whose stream details are not read (pending, timed out, unreadable, or missing the fields the tokens need) SHALL NOT be planned or renamed by the Rename page, the CLI rename step or the background name check. Patterns without such tokens SHALL never wait.

#### Scenario: A title with unread details keeps its name
- **WHEN** the folder pattern is `${title} (${year}) [${videoFormat}] [${hdrFormat}]` and the video of `Example Movie (2020) [2160p] [HDR10]` has no details yet
- **THEN** its folder and files keep their names

#### Scenario: Patterns without stream tokens
- **WHEN** the patterns are `${title} (${year})` and the title's video details are not read
- **THEN** the title is planned and renamed as usual

### Requirement: Held-back titles on the Rename page
The Rename page SHALL list a title held back for its video details with "Held back until the video details are read" as the reason in a tooltip and a "Read now" action, without a checkbox that includes it.

#### Scenario: Read now
- **WHEN** the user clicks "Read now" on a held-back row
- **THEN** the video details are read and the plan is made again, offering the rename with the quality tags

### Requirement: Rename journal
Before the disk is touched, every applied rename batch SHALL be written to a persistent journal with each title's plan and every move, copy and set-aside step. A finished batch SHALL write one History entry, and the Undo of the toast and of History SHALL undo that same entry.

#### Scenario: Undo from the toast
- **WHEN** the user renames 12 titles and clicks Undo in the toast
- **THEN** the titles take their old names back and the History entry "Renamed 12 titles" reads "Undone at <time>"

### Requirement: Rename undo
Undo SHALL restore the old folder and file names of each title whose files are as the rename left them, and SHALL leave a title that changed since as it is and report it with the reason. A rename whose set-aside leftovers were erased SHALL stay undoable without them.

#### Scenario: Partial undo
- **WHEN** one of the renamed folders was renamed by hand afterwards
- **THEN** the other titles are restored, and that title is reported as not restorable with the reason

### Requirement: Recovery of interrupted renames
Renames a crash or power loss left half done SHALL be settled from the journal at start-up on a background thread, so the window opens without waiting for the drives. A rename whose drive does not answer SHALL wait, its title marked as having an open rename, and be tried again every 30 seconds. One that cannot be settled from the disk SHALL be reported with a warning and SHALL be resumable or able to be rolled back.

#### Scenario: A share that does not answer
- **WHEN** the app starts after a crash and the network share of a half-done rename does not answer
- **THEN** the window opens at once, and the rename is settled once the share answers

#### Scenario: An unplugged drive
- **WHEN** a half-done rename's drive is unplugged at the next start
- **THEN** the rename is not settled as failed; it is completed or rolled back once the drive is back

#### Scenario: Finished batch without an entry
- **WHEN** the app stopped between the end of a rename batch and its History entry
- **THEN** the entry is written at the next start, so History can undo the batch

# artwork Specification

## Purpose
Defines how CineVault obtains, chooses, names, writes and shows artwork for movies, collections, shows, seasons, episodes and actors: the artwork types and providers, the automatic best-image choice and its quality ranking, the Kodi, Jellyfin and Emby file names, extra fanart, extra thumbs and actor images, the artwork picker page (`/artwork`) with frame grabs, the content-keyed thumbnail cache and the `cv://img` image protocol.

## Requirements

### Requirement: Artwork types
The app SHALL handle these artwork types: poster, fanart, banner, clearart, clearlogo, disc art, thumb (landscape), keyart, squareart and characterart; extra fanart and extra thumbs; season poster, season fanart, season banner and season thumb; episode thumb; and actor images. A movie page SHALL show a tile for every type the library writes, at its real shape, with "Missing" and Choose for a type without an image.

#### Scenario: Present and missing types
- **WHEN** a movie has poster, fanart, clearlogo and keyart files
- **THEN** its page shows those four tiles filled and the other types as missing, each opening the picker on its type

### Requirement: Artwork providers
Artwork SHALL come from TMDB, Fanart.tv, TheTVDB, IMDb and MPDb.tv, asked in a fixed order per library type, and from frame grabs of the video in the picker. A provider that is not set up SHALL be skipped by the automatic choice, and a provider that does not offer a type SHALL not be asked for it.

#### Scenario: Provider order
- **WHEN** artwork is looked up for a movie and for a show
- **THEN** a movie asks TMDB, Fanart.tv, IMDb, MPDb.tv, TheTVDB in that order, and a show TheTVDB, Fanart.tv, TMDB, IMDb

#### Scenario: Clearlogo from two providers
- **WHEN** TMDB and Fanart.tv are set up and a movie needs a clearlogo
- **THEN** clearlogo candidates from both providers are considered

### Requirement: Automatic best-image selection
With `artwork.auto-select` on (default), the app SHALL choose per type, in order: an image in the preferred languages (`artwork.languages`, default the metadata language then English; `none` means without text) at the preferred size (`artwork.poster-size`, `artwork.fanart-size`, default `large`); other sizes (`artwork.other-resolutions`); for fanart a textless image first (`artwork.fanart-without-text`); any language last (`artwork.any-language`). Those three switches SHALL default to on.

#### Scenario: Prefer textless fanart
- **WHEN** "prefer fanart without text" is on and both a text and a textless backdrop exist
- **THEN** the textless backdrop is selected

#### Scenario: Poster language preference
- **WHEN** the preferred languages are Turkish then English and only English and Japanese posters exist
- **THEN** the best English poster is selected

#### Scenario: Automatic selection off
- **WHEN** `artwork.auto-select` is off and a title is scraped
- **THEN** only the image addresses already recorded for the title are used

### Requirement: Image quality ranking
Within each step of the selection, candidates SHALL be ranked by quality = 0.75 × popularity + 0.25 × resolution. Popularity SHALL be a Bayesian average of votes, with likes on a log scale comparable with votes, so a few high votes do not outweigh many good ones. Resolution SHALL be the width against the width the type needs to look sharp (posters 2000, fanart 3840, thumbs 1920, banners 1000, clearlogos 800 pixels), at most 1.

#### Scenario: Many votes beat one high vote
- **WHEN** two English posters at the preferred size have one vote of 10 and fifty votes averaging 5.6
- **THEN** the poster with fifty votes is selected

#### Scenario: The bigger backdrop wins a near tie
- **WHEN** two textless backdrops, 2048 and 3840 pixels wide, have about the same votes
- **THEN** the 3840-pixel backdrop is selected

### Requirement: Low-quality images come last
An image narrower than its type's minimum width (posters 500, fanart 1280 pixels) or far from its type's shape SHALL be low quality. The automatic choice SHALL take a low-quality image only after every other image, and SHALL never drop it.

#### Scenario: Small images come last
- **WHEN** a 342-pixel-wide poster has the most likes and a 1000-pixel-wide poster has none
- **THEN** the 1000-pixel poster is selected, and the small one is still taken when it is the only poster

### Requirement: Artwork file name lists
Artwork files SHALL be named from per-type lists of name choices, and every name in a list SHALL receive the same image. `metadata.artwork-names` SHALL pick the starting lists: `prefixed` (the default), the short fixed names, or the Emby, Jellyfin or Kodi-format presets. Per-type lists (`artwork.names.movie.<type>`, `artwork.names.tv.<type>`) SHALL replace a starting list. Every type SHALL keep at least one name.

#### Scenario: A type cannot be emptied
- **WHEN** the user removes the last name of the fanart type
- **THEN** the change is refused with "Every artwork type is written; keep at least one name"

### Requirement: Movie artwork file names
With the default `prefixed` names, movie artwork SHALL be written as `<base>-poster`, `<base>-fanart`, `<base>-fanart1` and up for extra fanart, `<base>-banner`, `<base>-clearart`, `<base>-landscape`, `<base>-clearlogo`, `<base>-discart`, `<base>-keyart` and `<base>-squareart`. The Emby style SHALL write `poster`, `fanart`, `extrafanart/fanartN`, `landscape` and the other fixed names; the Jellyfin style `backdrop` and `backdropN` for fanart and extra fanart.

#### Scenario: Default naming
- **WHEN** artwork is saved for `Example Movie (2020).mkv` with the default names
- **THEN** the files are `Example Movie (2020)-poster.jpg`, `Example Movie (2020)-fanart.jpg`, `Example Movie (2020)-clearlogo.png` and `Example Movie (2020)-landscape.jpg`

### Requirement: Folder restrictions on names
A folder holding several movies SHALL keep only names built on the video name, using `<base>-<type>` when a list has only fixed names, and SHALL get no extra fanart, extra thumbs or `.actors` folder. A disc folder (`VIDEO_TS`, `BDMV`) SHALL use the fixed names.

#### Scenario: Shared folder forces video-based names
- **WHEN** a movie is in a folder with other movies and the poster list is `poster`
- **THEN** the poster is written as `<base>-poster.jpg`

### Requirement: TV artwork file names
Show artwork SHALL be written in the show folder as `poster`, `fanart` (`backdrop` for Jellyfin), `banner`, `clearart`, `clearlogo`, `discart`, `characterart`, `keyart`, `landscape` and `squareart`; season artwork as `season01-poster`, `-fanart`, `-banner` and `-thumb` (`season-specials-` for season 0); and episode thumbs as `<video base>-thumb` next to the video.

#### Scenario: Episode thumb
- **WHEN** a thumb is written for `Example Show S01E02.mkv` with the default names
- **THEN** it is `Example Show S01E02-thumb.jpg`

### Requirement: Artwork with every scrape
A scrape SHALL always download and write the title's missing artwork types afterwards, with the configured names; no setting SHALL skip it. Download missing artwork (the `artwork` automation step) SHALL fill only the empty types of a selection, and Re-download artwork (the `redownload-artwork` action) SHALL fetch files gone from disk from their recorded addresses. Automatic runs after scrapes SHALL work on at most two titles at a time.

#### Scenario: Artwork comes with every scrape
- **WHEN** a New movie is scraped
- **THEN** its poster, fanart and the other types the providers offer are written under the configured names

#### Scenario: Fill gaps only
- **WHEN** a movie has a poster but no fanart and Download missing artwork runs
- **THEN** only fanart is written, and the poster is untouched

### Requirement: Safe artwork writes
An image already there, under any name of its type or any extension, SHALL never be replaced unless the user chose a replacement. Replaced files, old extras and portraits of actors who left the cast SHALL move to the trash folder of Settings › Deleted files. Files SHALL be written to a temporary name and renamed into place; bytes that are not a picture SHALL never be written; nothing SHALL be created in a folder that is gone.

#### Scenario: Keep the user's poster
- **WHEN** a movie folder already has a hand-picked `poster.jpg` and the movie is scraped
- **THEN** the existing poster is kept

#### Scenario: An error page is not a poster
- **WHEN** a provider answers an image request with an HTML page and status 200
- **THEN** no file is written and the type stays missing

### Requirement: Remembered artwork addresses
For each written image, the app SHALL record the file and the source address on the title, so the image can be fetched again and its address written into the NFO.

#### Scenario: Re-download from the recorded address
- **WHEN** a poster file is deleted on disk and Re-download artwork runs for the movie
- **THEN** the poster is fetched again from its recorded address

### Requirement: Extra fanart and extra thumbs
When `artwork.extra-fanart-enabled` is on, the app SHALL write up to `artwork.extra-fanart` extra fanart images (default 5); when `artwork.extra-thumbs` is on, up to `artwork.extra-thumbs-count` extra thumbs (default 5), scaled to `artwork.extra-thumbs-width` (default 480) when `artwork.extra-thumbs-resize` is on. Both are off by default. A new set SHALL replace the old one, whose files go to the trash folder.

#### Scenario: Extra fanart count
- **WHEN** extra fanart is on with a count of 4
- **THEN** four extra fanart images are written with the extra fanart names

### Requirement: Actor images
When `artwork.actor-images` is on (off by default), actor portraits SHALL be saved as `.actors/<name>.<ext>` in the title's folder, the name with spaces turned into underscores and the characters `" \ : < > | / ? *` removed. Portraits of actors who are not in the cast SHALL move to the trash folder. A `.actors` folder that is a symbolic link SHALL never be written or cleaned.

#### Scenario: Actor image file name
- **WHEN** actor images are on for a movie with the actor "Jane Q. Doe"
- **THEN** the portrait is saved as `.actors/Jane_Q._Doe.jpg`

### Requirement: Artwork picker page
The artwork picker page (`/artwork`, "Artwork · <name>") SHALL show a type strip with each type's image count, the candidates of the chosen type with the current image pinned first, language chips ("All", the languages, "Textless"), a sort menu (Votes, Size), "Use a file on this PC…", "From a link…" and a large preview with the size, language, source and the file name it is saved as.

#### Scenario: Tile opens its type
- **WHEN** the user clicks the empty Logo tile on the movie page
- **THEN** the artwork picker opens with Logo chosen in the type strip

### Requirement: Preview before an image is used
A single click on a candidate SHALL only show it in the preview beside the current image of that type; "Use this" or Enter SHALL apply it. A picture dropped anywhere on the page SHALL join the preview the same way. Nothing SHALL be written before the user applies an image.

#### Scenario: Preview first
- **WHEN** the user clicks a fanart candidate
- **THEN** the preview shows it beside "Current fanart" and nothing is written yet

#### Scenario: Replace a poster with a local file
- **WHEN** the user drops a local JPEG onto the picker and presses "Use this"
- **THEN** it is written under the poster names and the toast "Changed poster · Example Movie (2020)" offers Undo

### Requirement: Applying a chosen image
Applying an image SHALL show it at once from its cached thumbnail, write the full image in the background, move the replaced file to the trash folder, confirm with an Undo toast and record a History entry whose Undo puts the old file back. Choosing a new main image, and undoing that choice, SHALL rewrite the title's NFO so it names the image in place.

#### Scenario: A new backdrop reaches the NFO
- **WHEN** the user chooses another backdrop for a scraped movie whose NFO lists artwork addresses
- **THEN** the NFO's `<fanart>` names the new image, and after Undo the old one again

### Requirement: Candidates stream per provider
The picker SHALL ask each provider on its own and list its candidates in batches as their thumbnails are ready, so a fast provider is never held back by a slow one; a later batch SHALL NOT be listed above a tile the same provider already showed. While providers work, placeholders at the type's shape SHALL follow the tiles and a line SHALL name them ("Still looking on …"). A provider whose key is not set SHALL answer as needing a key without a request.

#### Scenario: Fast provider first
- **WHEN** TMDB answers in 0.5 s and Fanart.tv in 2 s
- **THEN** TMDB's posters appear after 0.5 s and Fanart.tv's join them at 2 s

#### Scenario: No irrelevant providers
- **WHEN** the user opens Clear art, which only Fanart.tv offers
- **THEN** only Fanart.tv is asked

### Requirement: Picker thumbnails load lazily
Candidate thumbnails SHALL load as their tiles come into view and SHALL be served from the thumbnail cache, never over IPC. Closing the picker SHALL cancel the outstanding provider work.

#### Scenario: Closing the picker
- **WHEN** the user closes the picker while a provider is still downloading thumbnails
- **THEN** the provider's work is cancelled and nothing more is listed

### Requirement: Best order in the picker
The picker's best order SHALL be the order the automatic selection uses: the fit to the settings, the preferred language, the size, then the quality, comparable across providers. Each candidate SHALL show its votes or likes at the provider when the provider gives them. Low-quality candidates SHALL come after the others behind "Show N more", unless nothing else is offered.

#### Scenario: Best order across providers
- **WHEN** the picker shows English posters from TMDB with fifty votes averaging 5.6 and from Fanart.tv with 14 likes
- **THEN** the Fanart.tv poster is listed first, its tile shows 14 likes and the TMDB tile shows 50 votes

#### Scenario: Small images held back
- **WHEN** a provider offers a poster 342 pixels wide among larger ones
- **THEN** it is not in the grid at first, "Show 1 more" lists it, and it then comes after the larger posters

### Requirement: Frame grabs in the picker
For movies and episodes with a readable video file, the picker SHALL offer "Frames" as a source for thumb and fanart: eight evenly spaced stills taken with FFmpeg from 10% to 90% of the video's length, made in the background and listed after the online candidates. Frames SHALL be kept in the image cache folder, count in its size, and frames whose newest grab is more than a day old SHALL be removed when the app starts.

#### Scenario: Frame grabs for a movie
- **WHEN** the user opens the picker on a movie's Thumb type
- **THEN** stills from the video join the candidates as "A still from your video" when they are ready

### Requirement: Artwork for several titles
Choosing Artwork for a Library selection SHALL open the picker stepping through the titles one at a time ("Artwork · Title 3 of 12", the title's name, "2 changed · 0 skipped"), with "Use this and go to next" (Enter), "Use this", "Skip title" and Done. Each title's change SHALL be applied as it is chosen, and the chosen type SHALL be kept for the next title.

#### Scenario: Step through a selection
- **WHEN** the user picks a fanart for title 3 of 12 with Enter
- **THEN** the fanart is applied, the picker moves to title 4 on Fanart, and the counter reads "3 changed"

### Requirement: Collection artwork
A collection's poster and fanart SHALL be changeable from the Library's collection pane ("Change artwork") with the same picker, offering only those two types. They SHALL be written to the places `artwork.collection-locations` names: `movieset-<type>` in each movie's folder (the default), `<collection folder>/<set>/<type>`, `<collection folder>/<set>-<type>`, or the movie folders and the first folder form together.

#### Scenario: Change a collection poster
- **WHEN** the user chooses a new poster for "Example Collection" with the default location
- **THEN** `movieset-poster.jpg` is written in each of its movie folders and History lists the change with Undo

### Requirement: Image size limits
An image download SHALL be refused past 40 MB. A `file:` artwork address SHALL name a file on this computer only, with no host and no UNC path, and a local file over 40 MB SHALL be refused before it is read.

#### Scenario: Another machine's file
- **WHEN** an artwork address is `file:////server/share/poster.jpg`
- **THEN** it is refused and nothing is read

### Requirement: Thumbnail cache
The app SHALL keep a cache of resized artwork in its own cache folder, never next to media, keyed by the first 128 bits of the SHA-256 of the image bytes, so a renamed or moved file finds the same entry and identical copies share one. Each entry SHALL hold WebP thumbnails made when first needed (grid 360 × 540, list 120 × 180, hero 1600 × 900, never upscaled), a ThumbHash placeholder, a colour palette and the source size.

#### Scenario: Rename keeps thumbnails
- **WHEN** a movie folder is renamed
- **THEN** its posters show at once from the cache without being made again

#### Scenario: Instant grid render
- **WHEN** the user scrolls quickly through 5,000 posters
- **THEN** every tile shows at least its colour placeholder, never a blank tile

### Requirement: Thumbnail cache size
The thumbnail cache SHALL be capped by `advanced.image-cache-size` (default 1024 MB, accepted from 100 to 51,200 MB) and trimmed least recently used first, down to 90% of the cap. Trimming and clearing SHALL remove only the cache's own folders, never media files or library rows.

#### Scenario: Over the cap
- **WHEN** the cache grows past its cap
- **THEN** the least recently used entries are removed until it is at 90% of the cap

### Requirement: The cv://img protocol
Images SHALL reach the interface only through `cv://img/<key>/<size>` (`grid`, `list`, `hero`, `full`), never over IPC, and only a parsed 32-character key and a size word SHALL reach the file system. A thumbnail SHALL be served as `image/webp` with `Cache-Control: public, max-age=31536000, immutable` and a strong ETag, 304 for a matching `If-None-Match`, 404 for an unknown key or missing source, and 405 for methods other than GET and HEAD.

#### Scenario: A repeated request
- **WHEN** the web view asks again for a grid thumbnail with the ETag it holds
- **THEN** the answer is 304 and no image is sent

### Requirement: Full-resolution and stand-in images
`full` SHALL answer with the original file unchanged, so the title page's hero backdrop stays sharp on large high-density windows; when the original is offline, changed or past the decode limits, the hero thumbnail SHALL answer instead. A size that cannot be made yet SHALL be answered by the largest cached size with `no-store`, so the web view asks again and gets the real size later.

#### Scenario: A 4K backdrop on a large window
- **WHEN** a movie's fanart is 3840 pixels wide and its page is open on a large high-density window
- **THEN** the hero draws the 3840-pixel original, not the 1600-pixel thumbnail

#### Scenario: A chosen backdrop is sharp after the write
- **WHEN** the user chooses a backdrop whose candidate is cached only at the grid size
- **THEN** the hero shows the grid-size stand-in while the file is written, and the full image once it is written

### Requirement: Bounded thumbnail work
The app SHALL make at most one thumbnail per processor core at a time, and at most eight; cached thumbnails SHALL be served without waiting. Every artwork decode SHALL be limited to 16384 pixels a side and 256 MB, and a source file over 64 MB SHALL NOT be made into a thumbnail. After a rescan, a low-priority background job SHALL make the grid and list thumbnails of the poster-like images of new and changed titles, and resume after a restart.

#### Scenario: A decompression bomb
- **WHEN** an artwork file claims to be 100,000 pixels wide
- **THEN** it is refused from its header and no thumbnail is made from it

### Requirement: The grid does not touch the media drives
Showing posters SHALL NOT read the media drives for a poster whose size and modification time match what the scan or the artwork writer recorded; only a new or changed poster SHALL be read and hashed. A title whose folder is offline SHALL never be looked at and SHALL show its remembered poster. Remembered poster keys SHALL be saved at most every 30 seconds, and keys unused for 180 days SHALL be forgotten.

#### Scenario: Sleeping disks
- **WHEN** the user opens the Movies page while the media hard disks are asleep
- **THEN** the known posters show without waking the disks

#### Scenario: Offline share
- **WHEN** a library on an unreachable network share is shown
- **THEN** its titles show their cached posters at once

### Requirement: Big preview in the picker
A double-click on an image of the artwork picker, Space on the chosen image, or the expand button that shows on hover and focus in the corner of the preview pane SHALL open the image large, about 90% of the window over the dimmed page, fitted inside, with its type, size and source under it. It SHALL fade in and grow within 160 ms, and appear at once with reduced motion.

#### Scenario: Look closer
- **WHEN** the user double-clicks a fanart image
- **THEN** the image opens large and nothing is written

### Requirement: The big preview only looks
Any click anywhere or Esc SHALL close the big preview; nothing SHALL be chosen or written by it, and the focus SHALL return to where it was. The Left and Right arrows SHALL show the neighbouring images of the current type.

#### Scenario: Flick through
- **WHEN** the large view is open and the user presses Right
- **THEN** the next image of the type shows, and the choice in the grid is unchanged

## MODIFIED Requirements

### Requirement: Movie naming tokens
For file and folder names the engine SHALL expose short movie token names, among them the title tokens, `year`, `edition`, `certification`, the people, genre and tag tokens, `rating`, `releaseDate`, the decade tokens, `movieSetIndex`, `imdb`, `tmdb`, the original file and folder name tokens, the release tokens, `filesize` and `crc32`, and the stream tokens. Export templates SHALL use the full model paths instead.

#### Scenario: Token names offered
- **WHEN** a movie template is written
- **THEN** it may use, among others, `title`, `originalTitle`, `englishTitle`, `titleSortable`, `year`, `edition`, `directors`, `writers`, `actors`, `genres`, `tags`, `rating`, `top250`, `releaseDate`, `decadeShort`, `decadeLong`, `movieSetIndex`, `imdb`, `tmdb`, `originalFilename`, `originalBasename`, `releaseGroup`, `releaseQuality`, `releaseTags`, `parent`, `folderName`, `filesize` and `crc32`

#### Scenario: Stream tokens
- **WHEN** a movie template needs quality tags
- **THEN** it may use `videoFormat`, `videoResolution`, `videoCodec`, `videoBitDepth`, `videoBitRate`, `framerate`, `aspectRatio`, `hdrformat` (also `hdrFormat`), `3Dformat`, `mediaSource`, `audioCodec`, `audioChannels`, `audioChannelsDot`, `audioLanguage` and the per-track audio and subtitle lists

#### Scenario: Detailed tags render in order
- **WHEN** `${title} (${year}) [${videoFormat}] [${mediaSource}] [${hdrformat}] [${audioCodec} ${audioChannelsDot}]` is rendered for a 2160p WEB-DL Dolby Vision HDR10 movie with TrueHD Atmos 7.1 audio
- **THEN** the output is `Example Movie (2024) [2160p] [WEB-DL] [Dolby Vision, HDR10] [TrueHD Atmos 7.1]`

### Requirement: TV naming tokens
For TV names the engine SHALL expose the show tokens, the season and episode number tokens with their two-digit, aired-order and DVD-order variants, the absolute number, the aired date, the episode IDs and the episode's release tokens, besides the stream tokens.

#### Scenario: Token names offered
- **WHEN** a TV template is written
- **THEN** it may use, among others, `showTitle`, `showYear`, `showYearEnd`, `showImdb`, `showTmdb`, `showTvdb`, `seasonNr`, `seasonNr2`, `seasonName`, `episodeNr`, `episodeNr2`, `episodeNrAired`, `episodeNrDvd`, `absoluteNr`, `absoluteNr2`, `airedDate`, `episodeTvdb`, `releaseGroup`, `releaseQuality` and `releaseTags`

### Requirement: Naming presets
The app SHALL ship four naming presets, offered as the naming styles of the first run and in Settings › Naming: Clean, Detailed, Media-server safe and Release info. Each SHALL define the movie folder, movie file, show folder, season folder (`Season ${seasonNr2}`) and episode file templates.

#### Scenario: Clean
- **WHEN** the Clean preset is active
- **THEN** the movie folder is `${title} (${year})`, the movie file `${title}${ - ,edition,} (${year})`, the show folder `${showTitle} (${showYear})` and the episode file `${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}`

#### Scenario: Detailed preset output
- **WHEN** the Detailed preset, whose movie folder and file are `${title} (${year}) [${videoFormat}] [${mediaSource}] [${hdrformat}] [${audioCodec} ${audioChannelsDot}]`, names a 2160p BluRay HDR10 movie with DTS-HD MA 7.1 audio
- **THEN** the folder is `Example Movie (2017) [2160p] [BluRay] [HDR10] [DTS-HD MA 7.1]`, and episode files carry the same tags

#### Scenario: Media-server safe
- **WHEN** the Media-server safe preset names a movie with a TMDB ID and an edition
- **THEN** the folder is `Example Movie (2020) {tmdb-<id>}`, the file `Example Movie (2020) {edition-<edition>}`, and episode files are `${showTitle} - S${seasonNr2}E${episodeNr2}`

#### Scenario: Release info keeps the release
- **WHEN** the Release info preset names an episode titled "Pilot" first scanned as `Example.Show.2017.S01E01.Episode.1.Bluray-1080p.German.DL.x265-GRP.mkv`
- **THEN** the file is `Example Show - S01E01 - Pilot [Bluray-1080p.German.DL.x265]-GRP.mkv`, and a movie first scanned as `Example.Movie.2020.1080p.WEB-DL.x264-GRP.mkv` becomes `Example Movie (2020)/Example Movie (2020) [1080p.WEB-DL.x264]-GRP.mkv`

#### Scenario: Release info without release information
- **WHEN** the Release info preset names a movie first scanned as `Example Movie (2020).mkv`
- **THEN** the file is `Example Movie (2020).mkv`, with no empty brackets and no group

### Requirement: Naming templates in Settings
Settings › Naming SHALL hold the templates "Movie folder", "Movie file" and "TV episode" as text fields in the mono face, each with a live "Example:" line from the library under it, and a row of token buttons that insert at the cursor of the field last used. Any other engine expression SHALL be shown and kept as written.

#### Scenario: Token buttons
- **WHEN** the user looks at the token buttons
- **THEN** the movie fields offer {Title}, {Year}, {Resolution}, {HDR}, {VideoCodec}, {AudioCodec}, {Channels}, {Edition}, {Source}, {Quality}, {ReleaseTags}, {ReleaseGroup}, {IMDb} and {TMDB}, and the episode field offers {Show}, {ShowYear}, {Season}, {Season00}, {Episode}, {Episode00}, {EpisodeTitle}, {Resolution}, {HDR}, {VideoCodec}, {AudioCodec}, {Source}, {Quality}, {ReleaseTags} and {ReleaseGroup}

#### Scenario: Insert a token
- **WHEN** the cursor is after `{Year})` in Movie file and the user clicks {Resolution}
- **THEN** `{Resolution}` is inserted at the cursor and the example updates

## ADDED Requirements

### Requirement: Release tokens
The release tokens `releaseGroup`, `releaseTags` and `releaseQuality` SHALL be read from the name the title's main video had when it was first scanned, which renames never change, so they describe the release as it was downloaded however often the title is renamed. A token SHALL be empty when that name does not say it clearly; the app SHALL never guess a value. The release tokens SHALL NOT make a rename wait for the video details.

#### Scenario: Name kept after renames
- **WHEN** a movie first scanned as `Example.Movie.2020.1080p.BluRay.x264-GRP.mkv` was renamed with the Clean preset and is then renamed with the Release info preset
- **THEN** its file is `Example Movie (2020) [1080p.BluRay.x264]-GRP.mkv`

#### Scenario: A dash in the title is no group
- **WHEN** a movie was first scanned as `Spider-Man (2002).mkv` or `Example.Movie-Part.Two.2021.mkv`
- **THEN** `releaseGroup`, `releaseTags` and `releaseQuality` are empty

### Requirement: Release group token
`releaseGroup` SHALL be the group after the last `-` of the name, only when a release tag (resolution, source, codec, HDR or audio) comes before that dash and the group is not itself a tag or a number, or else the group of a leading `[Group]`.

#### Scenario: Group of a movie
- **WHEN** a movie was first scanned as `Example.Movie.2020.2160p.UHD.BluRay.REMUX.HDR.HEVC.Atmos-GRP.mkv`
- **THEN** `releaseGroup` is `GRP`

#### Scenario: A tag after the last dash is no group
- **WHEN** a movie was first scanned as `Example.Movie.2020.1080p.WEB-DL.mkv`
- **THEN** `releaseGroup` is empty

#### Scenario: Leading group
- **WHEN** an episode was first scanned as `[GRP] Example Show - 05 [1080p].mkv`
- **THEN** `releaseGroup` is `GRP`

### Requirement: Release tags and quality tokens
`releaseTags` SHALL be the release part of the name as written: from the first release or language tag after the year (movies) or the episode code (episodes) up to the group. `releaseQuality` SHALL be the name's source and resolution in the `Source-Resolution` form (`Bluray-1080p`, `Remux-2160p`, `WEBDL-1080p`, `WEBRip-720p`, `HDTV-720p`, `DVD`), followed by `Proper` or `Repack` when the name says so.

#### Scenario: Tags and quality of a remux
- **WHEN** a movie was first scanned as `Example.Movie.2020.2160p.UHD.BluRay.REMUX.HDR.HEVC.Atmos-GRP.mkv`
- **THEN** `releaseTags` is `2160p.UHD.BluRay.REMUX.HDR.HEVC.Atmos` and `releaseQuality` is `Remux-2160p`

#### Scenario: Quality without a group
- **WHEN** a movie was first scanned as `Example.Movie.2020.1080p.WEB-DL.mkv`
- **THEN** `releaseQuality` is `WEBDL-1080p`

#### Scenario: Proper release
- **WHEN** a movie was first scanned as `Example.Movie.2020.PROPER.1080p.BluRay.x264-GRP.mkv`
- **THEN** `releaseQuality` is `Bluray-1080p Proper`

### Requirement: Metadata-only mode
Settings › Naming SHALL offer "Metadata only" (`renamer.metadata-only`, off by default). While it is on, no rename SHALL change a title's files or folders, whether it is asked for on the Rename page, by a title, library or copies action, by the command line or by an automation; it SHALL be refused with "Metadata only is on in Settings › Naming, so files and folders are not renamed or moved." Scan, Scrape, NFO and artwork writing, Delete and the Undo of earlier renames SHALL keep working.

#### Scenario: Rename refused
- **WHEN** Metadata only is on and a rename is sent by the command line
- **THEN** it is refused with the reason and no file is touched

#### Scenario: Scrape still writes
- **WHEN** Metadata only is on and the user scrapes a movie
- **THEN** its NFO and artwork are written next to the video and its files keep their names

### Requirement: Rename hidden in metadata-only mode
While Metadata only is on, the sidebar's Rename entry and the Rename actions of the library, title and copies menus SHALL be hidden, and the Rename page SHALL say that Metadata only is on, with "Open Settings", instead of its plan. Turning it off SHALL offer Rename again at once.

#### Scenario: Rename hidden
- **WHEN** Metadata only is on
- **THEN** the sidebar has no Rename entry and the title menu has no Rename

#### Scenario: Turned off again
- **WHEN** the user turns Metadata only off
- **THEN** Rename is offered again at once

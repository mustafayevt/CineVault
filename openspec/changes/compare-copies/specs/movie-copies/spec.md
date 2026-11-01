## Purpose

Finds movies the library holds more than once, shows the copies side by side with the best one marked, and lets the user keep one copy and delete the others, or keep them all.

## ADDED Requirements

### Requirement: What counts as copies
Two or more movies SHALL be copies of one film when they share a TMDB ID or an IMDb ID, or, when one of them has neither ID, when their titles match without regard to case and surrounding spaces and their years are the same and known. Copies SHALL be counted transitively, so every movie linked to a group by one of these rules is in that group. TV shows, seasons and episodes SHALL never be copies.

#### Scenario: Same TMDB ID in two folders
- **WHEN** `D:\Movies\Example Movie (2020) 1080p WEB` and `E:\Films\Example.Movie.2020.2160p.BluRay` are both scraped to the same TMDB ID
- **THEN** the two movies are copies of one film

#### Scenario: Not scraped yet
- **WHEN** a scraped Example Movie (2020) and a new, unscraped folder parsed as "example movie" from 2020 are in the library
- **THEN** the two movies are copies

#### Scenario: Different IDs
- **WHEN** two scraped movies are both titled Example Movie from 2020 but have different TMDB and IMDb IDs
- **THEN** they are not copies

#### Scenario: Unknown year
- **WHEN** two unscraped movies are both parsed as "Example Movie" with no year
- **THEN** they are not copies

### Requirement: Different editions are not copies
Movies whose editions differ SHALL NOT be copies of each other, whatever their IDs, titles and years. Editions SHALL be compared without regard to case and surrounding spaces, and a movie with no edition SHALL match only another movie with no edition.

#### Scenario: Extended and Theatrical
- **WHEN** Example Movie (2020) with the edition Extended and Example Movie (2020) with the edition Theatrical share a TMDB ID
- **THEN** they are not copies

#### Scenario: Same edition
- **WHEN** two movies with the edition "extended" and "Extended" share an IMDb ID
- **THEN** they are copies

### Requirement: Only movies that have their files
A movie SHALL count as a copy only while its files are present and its library folder can be reached. A movie whose files are missing or offline, or that was deleted and not undone, SHALL be left out of every group; such rows stay with the missing and duplicate review.

#### Scenario: One copy offline
- **WHEN** two copies share a TMDB ID and the drive of one of them is disconnected
- **THEN** neither counts as a copy until the drive is back

#### Scenario: Copy deleted
- **WHEN** one of two copies is deleted to the Recycle Bin
- **THEN** the other has no copies, and Undo of the delete makes them copies again

### Requirement: Copies follow library changes
The copy groups, their counts and their badges SHALL reflect the library as it is after every Scan, Scrape, edit of details, Delete, Undo and Remove from library, without a restart or a step of their own.

#### Scenario: Scrape joins a group
- **WHEN** an unscraped movie is scraped to the TMDB ID of a movie already in the library
- **THEN** the Copies chip counts both movies as soon as the scrape is saved

### Requirement: Copies chip in the filter row
For movies, the Library's filter row SHALL show the on/off chip "Copies <count>" right after "Problems" while its count is above zero or while it is on. The TV shows and Collections types SHALL never show it.

#### Scenario: Chip shown
- **WHEN** Example Movie (2020) has two copies the user has not kept
- **THEN** the filter row shows "Copies 2" after the Problems chip

#### Scenario: Chip hidden
- **WHEN** no movie has an open copy, or the TV shows type is shown
- **THEN** the filter row has no Copies chip

### Requirement: Copies filter
The Copies chip SHALL list only the movies in open copy groups, the copies of each film next to each other with the best quality first, and the groups in the chosen sort order. Its count SHALL be the number of movies listed. It SHALL combine with the other filters and the search. A group the user kept SHALL NOT be counted or listed.

#### Scenario: Two films with copies
- **WHEN** Example Movie (2020) has two copies, Another Example (2018) has three, and the user turns on "Copies 5"
- **THEN** the five movies are listed, the two Example Movie copies together and the three Another Example copies together

#### Scenario: Last group resolved
- **WHEN** the Copies filter is on and the user keeps one copy of the only open group
- **THEN** the list says "No titles match", and the chip stays until the user turns it off

### Requirement: Copies badge on the poster card
The poster card of a movie in an open copy group SHALL show a small "<n> copies" badge in the poster's bottom-left corner, where n is the number of copies in the group. A kept group's posters SHALL show no badge.

#### Scenario: Badge on both copies
- **WHEN** Example Movie (2020) is in the library twice, as two copies the user has not kept
- **THEN** both posters show "2 copies", and after "Keep both" neither does

### Requirement: Copies line on the Movie page
The Movie page header of a movie with copies, kept or not, SHALL show "You have <n> copies · Compare" under the facts, where Compare opens the Compare copies page. The line SHALL NOT push the plot, the facts or the Technical card's first rows below the first screen.

#### Scenario: Copies line
- **WHEN** the Movie page of Example Movie (2020) opens at 1366×768 and the library holds two copies of it
- **THEN** the header reads "You have 2 copies · Compare", and the plot, the Director and Released facts and the Video, Audio and File rows are still visible without scrolling

#### Scenario: No copies
- **WHEN** a movie has no copy
- **THEN** its header has no copies line

### Requirement: Compare from the right-click menu
For a movie with copies, kept or not, the title right-click menu SHALL also offer "Compare <n> copies", where n is the number of copies in its group, in the group of title actions and never in the Delete group. It SHALL open the Compare copies page of that movie, and SHALL be left out when several titles are selected or the movie has no copy.

#### Scenario: Compare from the menu
- **WHEN** the user right-clicks Example Movie (2020), which has two copies
- **THEN** the menu offers "Compare 2 copies" among the title actions, and choosing it opens the Compare copies page

#### Scenario: Several titles selected
- **WHEN** the user right-clicks inside a selection of four movies
- **THEN** the menu has no Compare entry

### Requirement: Compare copies page
The Compare copies page (`/movies/<id>/copies`) SHALL show every copy of the movie's group side by side under a header with the movie's poster, one card each, and these rows: Resolution, HDR, Audio, Source, Video bitrate, Length, Size and Folder. The movie's own copy SHALL be first. It SHALL be a page like the other pickers: Esc and the back button return to where the user came from.

#### Scenario: Two copies
- **WHEN** the user opens Compare copies for a 1080p WEB-DL copy and a 2160p BluRay copy of Example Movie (2020)
- **THEN** the page shows two cards with "1080p" and "2160p", the sources "WEB-DL" and "BluRay", each copy's size, video bitrate and folder, the opened copy first

#### Scenario: No other copy left
- **WHEN** the page is opened for a movie that no longer has a copy
- **THEN** it says "This movie has no other copies" with a link back to the Movie page

### Requirement: What the compare rows show
Resolution SHALL show the resolution label, HDR the HDR formats or "SDR", Audio the codec and channels of the track the title leads with, Source the media source ("Unknown" when none), Video bitrate in Mb/s, Length the running time, Size the copy's size by the library's size rule, with how much smaller it is on every copy but the largest, and Folder the copy's folder path. A copy whose media info has not been read SHALL show "Not read yet" in its media rows.

#### Scenario: Audio and HDR rows
- **WHEN** a copy's video is 2160p with Dolby Vision and HDR10 and its lead track is TrueHD Atmos 7.1
- **THEN** its rows read "2160p", "Dolby Vision, HDR10" and "TrueHD Atmos 7.1"

### Requirement: Best quality mark
The page SHALL mark one copy "Best quality": the one with the highest resolution label, then the best HDR (Dolby Vision, HDR10+, HDR10, HLG, then SDR), then the lead audio track's most channels and then its best codec, then the highest video bitrate. When the best copies tie on all of these, or the best copy's media info has not been read, no copy SHALL be marked. The mark SHALL be a suggestion only: every card, marked or not, keeps "Keep this one".

#### Scenario: Resolution decides
- **WHEN** a 2160p SDR copy and a 1080p Dolby Vision copy are compared
- **THEN** the 2160p copy is marked "Best quality"

#### Scenario: Bitrate decides
- **WHEN** two 1080p SDR copies have the same audio and video bitrates of 8 Mb/s and 20 Mb/s
- **THEN** the 20 Mb/s copy is marked "Best quality"

#### Scenario: Tie
- **WHEN** two copies are equal in resolution, HDR, audio and video bitrate
- **THEN** no copy is marked

#### Scenario: Keep the copy that is not marked
- **WHEN** the 2160p copy is marked "Best quality" and the user prefers the 1080p one
- **THEN** the 1080p card still offers "Keep this one"

### Requirement: Identical copies on the compare page
When the copies of a group show the same values in Resolution, HDR, Audio and Source, the page SHALL say "These two copies look the same." ("These <n> copies look the same." for more) and suggest keeping one of them.

#### Scenario: Same download twice
- **WHEN** two copies are both 1080p SDR WEB-DL with AAC 5.1
- **THEN** the page says "These two copies look the same." and suggests keeping one

### Requirement: Copies that may be one file
Two copies SHALL count as maybe one file when the file system reports their videos as the same file (one folder reached through a network share, a mapped drive, a link or a substituted drive letter), when their videos have the same file name, size and modified time, or when one of their videos cannot be read.

#### Scenario: One folder through its network share
- **WHEN** `D:\Movies` and `\SERVER\Movies`, the network share of the same folder, are both library folders and Example Movie (2020) shows as two copies, one under each
- **THEN** the two copies count as maybe one file

#### Scenario: Same name, size and time
- **WHEN** two copies' videos are both `Example Movie (2020).mkv` with the same size and modified time, and the file system cannot tell whether they are one file
- **THEN** the two copies count as maybe one file

#### Scenario: Two different files
- **WHEN** two copies' videos are different files with different modified times on two drives
- **THEN** they do not count as maybe one file

### Requirement: No Keep this one for copies that may be one file
When any two copies of a group may be one file, the Compare copies page SHALL offer "Keep this one" on no card, SHALL NOT say that the copies look the same, and SHALL say "These may be the same file reached through two library folders. CineVault won't delete either from here; check the folders under Settings › Library folders." "Keep both" SHALL stay available.

#### Scenario: Copies through a network share
- **WHEN** the user opens Compare copies for the two copies of Example Movie (2020) under `D:\Movies` and `\SERVER\Movies`
- **THEN** no card offers "Keep this one" and the page says "These may be the same file reached through two library folders. CineVault won't delete either from here; check the folders under Settings › Library folders."

### Requirement: Keep this one
Each card SHALL offer "Keep this one", which deletes every other copy of the group exactly as Delete does for those titles: it follows Settings › Deleted files, with the same confirmation, toast, History entry and Undo. The kept copy, its folder and its files SHALL NOT change. Cancelling a confirmation SHALL delete nothing.

#### Scenario: Deleted files go to the Recycle Bin
- **WHEN** Deleted files is the Recycle Bin and the user chooses "Keep this one" on the 2160p copy of Example Movie (2020)
- **THEN** the 1080p copy's files go to the Recycle Bin, a toast with Undo says so, and the app returns to where the page was opened from, or opens the 2160p copy's Movie page when the page was opened from the deleted copy

#### Scenario: Deleted files is Delete permanently
- **WHEN** Deleted files is Delete permanently and the user chooses "Keep this one"
- **THEN** the permanent-delete confirmation lists the other copies, and nothing is erased until the user confirms it

#### Scenario: Copies in one folder
- **WHEN** `D:\Movies\Mixed` holds `Example Movie (2020).1080p.mkv` and `Example Movie (2020).2160p.mkv` as two copies, and the user keeps the 2160p one
- **THEN** only the 1080p video and the files named after it are deleted

### Requirement: Keep this one checks the copies again
Before anything is deleted, "Keep this one" SHALL check the group as the library holds it now and SHALL delete nothing, saying why, when the kept copy is no longer in the library or cannot be reached, when one of the copies to delete is no longer a copy in its group, or when two copies may be one file. It SHALL delete only the copies that check confirmed. While one "Keep this one" is running, no card SHALL start another.

#### Scenario: Deleted in another window
- **WHEN** the Compare copies page of Example Movie (2020) is open in two windows, the user keeps the 2160p copy in the first and then chooses "Keep this one" on the 1080p copy in the second
- **THEN** nothing is deleted and the second window says that the copy to keep is no longer in the library

#### Scenario: Two clicks
- **WHEN** the user chooses "Keep this one" on one card and, while the confirmation is open, tries "Keep this one" on another card
- **THEN** the other card's button is disabled

### Requirement: Keep both
Each group SHALL offer "Keep both" ("Keep all" for three or more copies), which records the decision in the library database. A kept group SHALL leave the Copies count and lose its badges, the page SHALL close, and the decision SHALL be confirmed with an Undo toast. No file SHALL change.

#### Scenario: Keep both
- **WHEN** the user chooses "Keep both" for the two copies of Example Movie (2020)
- **THEN** "Copies" counts two movies fewer, their badges go, and Undo in the toast brings both back

### Requirement: A new copy opens a kept group
A kept group SHALL open again when a movie joins it that was not in it when the user kept it. A kept group that loses a copy SHALL stay kept.

#### Scenario: Third copy found
- **WHEN** the user kept both copies of Example Movie (2020) and a later Scan finds a third copy that is scraped to the same TMDB ID
- **THEN** all three movies count in "Copies" and show "3 copies"

#### Scenario: One kept copy deleted
- **WHEN** the user kept three copies and later deletes one of them
- **THEN** the remaining two stay kept and are not counted

### Requirement: Ask again about a kept group
The Compare copies page of a kept group SHALL say "You chose to keep these copies" with "Ask again", which removes the decision so the group counts again. "Keep this one" SHALL stay available on a kept group.

#### Scenario: Ask again
- **WHEN** the user opens Compare copies for a kept group and chooses "Ask again"
- **THEN** the group's movies count in "Copies" again and show their badges

### Requirement: Copies keep their own folders
Rename SHALL keep each copy in a folder of its own and SHALL never move the files of two copies into one folder. Copies that already share one folder SHALL stay in it, each with its own video name.

#### Scenario: Two copies, two folders
- **WHEN** two copies of Example Movie (2020) in `D:\Movies\dl-1` and `D:\Movies\dl-2` are renamed together
- **THEN** they end in two folders, never in one

### Requirement: Copy label when names clash
When two or more copies of one group would end at the same folder or video path under the naming templates, Rename SHALL add " - <label>" after the name of the folder and of the video, and the files that follow the video's base name (NFO, artwork named after the video, subtitles) SHALL follow it. A movie whose name clashes with no copy SHALL keep its normal name.

#### Scenario: 2160p and 1080p
- **WHEN** a 2160p copy and a 1080p copy of Example Movie (2020) in `D:\Movies` are renamed
- **THEN** they become `Example Movie (2020) - 2160p\Example Movie (2020) - 2160p.mkv` and `Example Movie (2020) - 1080p\Example Movie (2020) - 1080p.mkv`, each with its NFO and subtitles named after its video

#### Scenario: Single movie unaffected
- **WHEN** Another Example (2018) has no copy and is renamed
- **THEN** it becomes `Another Example (2018)\Another Example (2018).mkv` with no label

### Requirement: How the copy label is chosen
The label SHALL be the copy's resolution (2160p, 1080p, 720p, SD) when that tells the clashing copies apart; otherwise the resolution followed by the first of these that differs between the copies of that resolution: HDR (DV, HDR10+, HDR10, HLG, SDR), source (BluRay, Remux, WEB-DL, WEBRip, HDTV, DVD), then the lead audio track (such as Atmos, TrueHD 7.1, DTS-HD MA, AAC 5.1).

#### Scenario: Dolby Vision and HDR10
- **WHEN** two 2160p copies clash, one with Dolby Vision and one with HDR10
- **THEN** their labels are "2160p DV" and "2160p HDR10"

#### Scenario: BluRay and WEB-DL
- **WHEN** two 2160p HDR10 copies clash, one from BluRay and one from WEB-DL
- **THEN** their labels are "2160p BluRay" and "2160p WEB-DL"

### Requirement: Identical copies are numbered
When clashing copies do not differ in resolution, HDR, source or lead audio track, the copy with the lowest library id SHALL keep the plain name and the others SHALL get "copy 2", "copy 3" and so on in id order.

#### Scenario: Identical copies
- **WHEN** two identical 1080p SDR WEB-DL copies of Example Movie (2020) are renamed
- **THEN** the older row becomes `Example Movie (2020)` and the other `Example Movie (2020) - copy 2`

### Requirement: Copy labels are stable
Renaming copies again with the same templates and media info SHALL give the same names, so labelled copies count as already named. When a clash is gone, the next Rename SHALL offer the plain name as a normal row of the preview; a label SHALL never be dropped without a Rename.

#### Scenario: Rename again
- **WHEN** the two labelled copies of Example Movie (2020) are selected and Rename is opened again
- **THEN** neither needs a rename

#### Scenario: Copy deleted
- **WHEN** the 1080p copy is deleted and the user later renames the 2160p copy `Example Movie (2020) - 2160p`
- **THEN** the preview offers `Example Movie (2020)` as its new name, and nothing moves until the user renames

### Requirement: Copy labels in the Rename preview
Copy labels SHALL be shown in the Rename preview before anything moves, and SHALL be applied, journaled and undone like every other rename. Copies that the labels separate SHALL NOT be reported as "Another title would also become …"; that conflict SHALL stay for clashes between different movies.

#### Scenario: No conflict for separated copies
- **WHEN** the preview lists two copies of Example Movie (2020) that the labels separate
- **THEN** neither row shows a conflict, and Undo after the rename puts both back under their old names

#### Scenario: Different movies still clash
- **WHEN** two movies that are not copies would both become `Example Movie (2020)`
- **THEN** the rows say "Another title would also become Example Movie (2020)."

### Requirement: Drop the label after Keep this one
When "Keep this one" has deleted the other copies and the kept copy's folder or video carries a copy label that no longer clashes, the Undo toast SHALL also offer "Rename to drop the '- <label>' label?", which opens the Rename preview for that movie alone with its plain name. Nothing SHALL be renamed without that preview.

#### Scenario: Drop "copy 2"
- **WHEN** the user keeps `Example Movie (2020) - copy 2` and the other copy is deleted
- **THEN** the toast offers Undo and "Rename to drop the '- copy 2' label?", which opens the Rename preview of that movie with `Example Movie (2020)`

## REMOVED Requirements

### Requirement: Copies line on the Movie page
**Reason**: The copy tabs on the title line of the Technical card show the copies and link to Compare.
**Migration**: See "Copy tabs on the Movie page".

## ADDED Requirements

### Requirement: Copy tabs on the Movie page
The title line of the Technical card of the Movie page of a movie with copies, kept or not, SHALL show one tab per copy of its group in place of the card's title, the shown copy's tab current. A movie with no copy SHALL show no tabs.

#### Scenario: Two copies
- **WHEN** the Movie page of the 1080p copy of Example Movie (2020) opens and the library also holds a 2160p copy
- **THEN** the Technical card's title line shows two tabs, the 1080p one current

#### Scenario: No copies
- **WHEN** a movie has no copy
- **THEN** its Technical card has its title, no tabs and no Copies row

### Requirement: Copy tab labels
Each copy tab SHALL read the copy's resolution and first HDR format ("DV" for Dolby Vision; "Not read yet" when its media info has not been read). Where two tabs would read the same, they SHALL add the source and then, if still the same, the size. Each tab's tooltip and accessible name SHALL give the resolution, first HDR format, source and size.

#### Scenario: Different resolutions
- **WHEN** a movie has a 2160p Dolby Vision BluRay copy and a 1080p WEB-DL copy
- **THEN** the tabs read "2160p DV" and "1080p", and the first one's tooltip reads "2160p · Dolby Vision · BluRay · <size>"

#### Scenario: Same resolution
- **WHEN** a movie has a 1080p WEB-DL copy and a 1080p BluRay copy
- **THEN** the tabs read "1080p · WEB-DL" and "1080p · BluRay"

### Requirement: Order and mark of the copy tabs
The copy tabs SHALL be ordered with the copy marked "Best quality" first, then by size from the largest, then by library id, and SHALL keep this order whichever copy is shown. The "Best quality" copy SHALL be marked as on the Compare copies page.

#### Scenario: Same order on both pages
- **WHEN** the 2160p copy is marked "Best quality" and the user switches from it to the 1080p copy
- **THEN** the 2160p tab stays first and keeps its mark

### Requirement: Copy tabs keep the first screen
The copy tabs SHALL NOT push the plot, the facts or the Technical card's Video, Audio and File rows below the first screen at 1366×768.

#### Scenario: First screen with tabs
- **WHEN** the Movie page of a movie with two copies opens at 1366×768
- **THEN** the plot, the Director and Released facts and the Video, Audio and File rows are visible without scrolling

### Requirement: Switching copies
Choosing a copy tab SHALL open that copy's Movie page in place of the current one without adding a step to Back, so the Technical card, File, NFO, Renamed and Undo and the header actions (Play, Open folder, Move, Rename, Delete) are the chosen copy's. The tabs SHALL be reachable with the keyboard.

#### Scenario: Switch to the other copy
- **WHEN** the user opens Example Movie (2020) from the Library, chooses the 2160p tab and then goes Back
- **THEN** the page shows the 2160p copy's file and folder, and Back returns to the Library

### Requirement: Copies row on the Movie page
The Technical card of a movie with copies SHALL end with a Copies row offering "Keep only this one", "Delete this copy" and "Compare", where Compare opens the Compare copies page. "Keep only this one" SHALL be left out when two copies of the group may be one file.

#### Scenario: Copies that may be one file
- **WHEN** the two copies of a movie may be one file reached through two library folders
- **THEN** the Copies row offers "Delete this copy" and "Compare" but no "Keep only this one"

### Requirement: Keep only this one
"Keep only this one" SHALL do what "Keep this one" does on the Compare copies page for the shown copy, with the same checks, confirmation, toast, History entry and Undo.

#### Scenario: Keep the 2160p copy
- **WHEN** the user shows the 2160p copy of Example Movie (2020) and chooses "Keep only this one"
- **THEN** the Delete confirmation lists the 1080p copy with its folder, and once confirmed the 1080p copy's files go where Settings › Deleted files sends them, with Undo in the toast

### Requirement: Delete this copy
"Delete this copy" SHALL delete the shown copy as Delete does, following Settings › Deleted files with its confirmation, and then open the Movie page of the first remaining copy in tab order.

#### Scenario: Delete the 1080p copy
- **WHEN** the user shows the 1080p copy, chooses "Delete this copy" and confirms
- **THEN** the 1080p copy is deleted as Delete does, and the 2160p copy's Movie page opens

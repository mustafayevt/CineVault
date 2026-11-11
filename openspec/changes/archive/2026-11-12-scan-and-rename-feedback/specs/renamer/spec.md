## MODIFIED Requirements

### Requirement: Rename page footer and result
The footer SHALL show "N of M selected", the conflict count and the primary "Rename N titles". While a rename runs, the sidebar's progress card SHALL show "Renaming N of M" with the title just done. When it ends, a toast SHALL report the result wherever the user is by then, also after leaving the Rename page: "Renamed N titles" with Undo when every title was renamed; "Renamed N of M titles; K need attention" with Undo and "Open Activity" when some could not be; "M titles could not be renamed" as an error with "Open Activity" when none could. The titles that could not be renamed SHALL be listed in Activity › Needs attention, each with its reason, with "Open in Rename", which opens the rename preview of those titles.

#### Scenario: Exclude one title
- **WHEN** 64 titles are listed, the user unticks one and clicks "Rename 63 titles"
- **THEN** that title is not renamed, the other 63 are, and the toast says "Renamed 63 titles" with Undo

#### Scenario: Leaving the page during a rename
- **WHEN** the user starts a rename of 12 titles and opens the Library before it ends
- **THEN** the sidebar shows "Renaming 3 of 12" while it runs, and the toast "Renamed 12 titles" with Undo appears on the Library when it ends

#### Scenario: Two titles fail
- **WHEN** a rename of 12 titles cannot rename two because their files are in use by another program
- **THEN** the toast says "Renamed 10 of 12 titles; 2 need attention" with Undo and "Open Activity", and Needs attention lists both titles with "the file is in use by another program" and "Open in Rename"

#### Scenario: Try the failed titles again
- **WHEN** the user chooses "Open in Rename" on that entry
- **THEN** the Rename page opens with the preview of those two titles

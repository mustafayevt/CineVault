## MODIFIED Requirements

### Requirement: Keys on the selection
Outside a text field, F2 SHALL rename the selection and Delete SHALL delete it as Settings › Deleted files says.

#### Scenario: Rename with F2
- **WHEN** two movies are selected and the user presses F2
- **THEN** the rename preview opens for those two movies

### Requirement: Floating selection bar
While titles are selected, a floating bar SHALL appear at the bottom centre of the content with the count and total size ("4 selected · 186.2 GB"), then Rename, Scrape, Artwork, Move, Delete and More, with "Rewrite NFO", "Edit details", "Remove from library (keep files)" and "Ignore this folder" under More, and a way to clear the selection. Actions for one title (Edit details) SHALL say "Select one title" when several are selected.

#### Scenario: Bar for four titles
- **WHEN** four movies totalling 186.2 GB are selected
- **THEN** the bar reads "4 selected · 186.2 GB" with Rename, Scrape, Artwork, Move, Delete and More

### Requirement: Title right-click menu
The right-click menu of a title (or the menu key on the current one) SHALL list three groups: "Open" (Enter), "Play" and "Open folder"; then "Rename" (F2), "Scrape", "Artwork…", "Move to…", "Rewrite NFO", "Edit details", "Remove from library (keep files)" and "Ignore this folder"; then "Delete" (Del). With several titles selected the labels SHALL carry the count ("Rename 4 titles").

#### Scenario: Menu labels for a selection
- **WHEN** the user right-clicks inside a selection of four movies
- **THEN** the menu reads "Rename 4 titles" and "Scrape 4 titles" with their key hints

#### Scenario: Right-click outside the selection
- **WHEN** two titles are selected and the user right-clicks a third
- **THEN** the third becomes the selection and the menu acts on it alone

#### Scenario: One Delete
- **WHEN** the user right-clicks a title
- **THEN** the menu has one Delete item and no separate permanent delete

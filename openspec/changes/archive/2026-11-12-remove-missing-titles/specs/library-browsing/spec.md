## MODIFIED Requirements

### Requirement: Floating selection bar
While titles are selected, a floating bar SHALL appear at the bottom centre of the content with the count and total size ("4 selected · 186.2 GB"), then Rename, Scrape, Artwork, Move, Delete and More, with "Rewrite NFO", "Edit details", "Remove from library (keep files)" and "Ignore this folder" under More, and a way to clear the selection. Actions for one title (Edit details) SHALL say "Select one title" when several are selected. While the Problems filter is "Missing files or offline", the bar SHALL show "Remove from library" as a button before Delete, and no longer list it under More.

#### Scenario: Bar for four titles
- **WHEN** four movies totalling 186.2 GB are selected
- **THEN** the bar reads "4 selected · 186.2 GB" with Rename, Scrape, Artwork, Move, Delete and More

#### Scenario: Remove titles that lost their files
- **WHEN** the Problems filter is "Missing files or offline", four titles are selected and the user chooses "Remove from library" on the bar
- **THEN** all four leave the library in one action, their files stay, and the toast says "Removed 4 titles from the library"

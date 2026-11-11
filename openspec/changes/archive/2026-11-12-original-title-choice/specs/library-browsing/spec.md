## ADDED Requirements

### Requirement: Switch selected titles between localized and original title
The selection bar's More and the right-click menu SHALL offer "Use original title" and "Use localized title" for the selected movies or TV shows. Choosing one SHALL set the choice of every selected title that has another original title and rewrite their NFO files; titles whose names now differ SHALL show Rename pending, and nothing SHALL be renamed on its own. A toast SHALL say what changed and offer Undo, which switches back exactly the titles that changed. Locks SHALL NOT stop it.

#### Scenario: Four titles, two with another original title
- **WHEN** four movies are selected, two of them have an original title in another language, and the user chooses More › "Use original title for 4 titles"
- **THEN** those two use their original title, their NFO files are rewritten, the toast says "Now using the original title for 2 titles. 2 titles had no other title to switch to." and Undo switches the two back

#### Scenario: Nothing to switch
- **WHEN** no selected title has another original title
- **THEN** the toast says nothing changed and offers no Undo

#### Scenario: Names follow through Rename
- **WHEN** a scraped movie whose folder is named from `${title} (${year})` is switched to its original title
- **THEN** it shows Rename pending, and its folder is renamed only when the user runs Rename

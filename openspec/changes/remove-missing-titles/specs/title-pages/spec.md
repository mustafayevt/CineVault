## MODIFIED Requirements

### Requirement: Files that cannot be reached
A title page whose files are missing or whose library folder is offline SHALL show one calm notice with an icon and words: "The files for this title are missing" or "This title's library folder cannot be reached", with what to do and the last known folder. Everything known about the title SHALL stay on the page. The notice for missing files SHALL say why the title is kept (the drive may be offline or the files only moved) and SHALL have a "Remove from library (keep files)" button that does what the ⋯ menu item does, then opens the Library. The notice for an unreachable library folder SHALL NOT have the button.

#### Scenario: Drive away
- **WHEN** a movie's drive is disconnected and its page opens
- **THEN** the page says "This title's library folder cannot be reached", shows the last known place, and the facts, plot and artwork are still shown, with no "Remove from library (keep files)" button

#### Scenario: Remove a title whose files are gone
- **WHEN** a movie or a TV show whose files are missing opens and the user chooses "Remove from library (keep files)" in the notice
- **THEN** the title leaves the library, its files stay, the toast says so, and the Library opens

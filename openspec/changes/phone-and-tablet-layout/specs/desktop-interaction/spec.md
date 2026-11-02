## MODIFIED Requirements

### Requirement: Own right-click menus
Posters, table rows, episode rows, files, drives and artwork tiles SHALL have the app's own right-click menus listing the same actions, in the same order and with the same names, as the selection bar or page header for that thing, with key hints. Right-clicking an unselected item SHALL select it alone first; right-clicking inside a selection SHALL act on the whole selection. Shift+F10 and the context-menu key SHALL open the menu of the focused item. On a touch screen a long-press SHALL open the same menu, selecting the pressed item first as a right-click does.

#### Scenario: Right-click inside a selection
- **WHEN** four movies are selected and the user right-clicks one of them
- **THEN** the menu acts on all four, with items such as "Rename 4 titles", "Scrape 4 titles", "Move to…" and "Delete"

#### Scenario: Keyboard opens the menu
- **WHEN** a table row has focus and the user presses Shift+F10
- **THEN** that row's menu opens under it

#### Scenario: Long-press on a phone
- **WHEN** no title is selected and a person long-presses a poster on a phone
- **THEN** that title is selected alone and its menu opens at the finger

## ADDED Requirements

### Requirement: Choice of title
Every movie and TV show SHALL have a choice of title, Localized (the default) or Original. With Original, the original title SHALL be the title CineVault uses everywhere: the Library, the title page, the search, the sort, `${title}` in the naming templates and the NFO `<title>`. The localized title SHALL be kept meanwhile, so switching back restores it exactly. A title whose original title is empty or equals its localized title SHALL offer no choice; episodes and collections have none.

#### Scenario: Use the original title
- **WHEN** a movie titled "The Sleeping City" with the original title "La Ciudad Dormida" is set to Original
- **THEN** the Library, the title page and the NFO `<title>` say "La Ciudad Dormida"

#### Scenario: Back to the localized title
- **WHEN** the user sets that movie back to Localized
- **THEN** its title is "The Sleeping City" again and its original title is still "La Ciudad Dormida"

### Requirement: Title to use on Edit details
When a title has a choice, the Titles section of Edit details SHALL show "Title to use" with Localized and Original under the Title and Original title fields, label the Title field "Localized title", and say which title the lists, the file and folder names and the NFO use. The choice SHALL be saved, counted and undone like any other field. Choosing Original without a different original title SHALL be refused with a plain sentence and nothing saved.

#### Scenario: Choose on Edit details
- **WHEN** the user picks Original under "Title to use" and saves
- **THEN** the line says "Lists, file and folder names and the NFO use “La Ciudad Dormida”.", the Title field still holds "The Sleeping City" and the toast offers Undo

#### Scenario: No other original title
- **WHEN** the user opens Edit details on a movie whose original title equals its title
- **THEN** the Titles section shows no "Title to use" choice

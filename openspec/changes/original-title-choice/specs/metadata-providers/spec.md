## ADDED Requirements

### Requirement: Scrape keeps the choice of title
The scrape field Title SHALL be the provider's title in the metadata language: a Scrape of a title that uses its original title SHALL keep the original title as its title and refresh the kept localized title, and a new original title from the provider SHALL become the title in use. A Scrape SHALL never change a choice the title already has.

#### Scenario: Rescrape a title set to Original
- **WHEN** a movie that uses its original title "La Ciudad Dormida" is scraped again and the provider's English title is now "The Sleeping Town"
- **THEN** its title stays "La Ciudad Dormida" and "The Sleeping Town" is kept as its localized title

### Requirement: Original title for chosen languages
Settings › Providers SHALL offer "Keep the original title" beside the metadata language: a list of original languages, empty by default, added from a list and removed with a button on each. A movie or TV show scraped for the first time whose original language, as the provider gives it, is in the list SHALL start with its original title as its title, unless its Title field is locked. A title scraped before SHALL keep its choice.

#### Scenario: A French film scraped for the first time
- **WHEN** French is in the list and a new movie whose original language is French is scraped
- **THEN** it uses its original title in the library, the file names and the NFO, and its title in the metadata language is kept

#### Scenario: A title scraped before
- **WHEN** French is added to the list and a French movie that was scraped before is scraped again
- **THEN** it keeps the title it had chosen

### Requirement: Original title for the titles already in the library
"Use for titles I have" beside the list SHALL switch, once, every movie and show in the library whose original language is listed and that has another original title, rewrite their NFO files and confirm with a toast that offers Undo.

#### Scenario: Titles already in the library
- **WHEN** Spanish is in the list and the user presses "Use for titles I have"
- **THEN** every movie and show whose original language is Spanish uses its original title, and Undo switches them back

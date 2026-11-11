## MODIFIED Requirements

### Requirement: Scrape is the one verb for matching
Scrape SHALL be the only action that chooses a match and fetches its details. Scraping one title from the Library, its title page or its menu SHALL open the match picker page for that title. Scraping a selection of several titles SHALL open the Scrape page limited to those titles. A scrape SHALL move a New title to the Scraped stage and SHALL NOT rename or move its folder; only "Rename after Scrape" (spec `renamer`), when the user turns it on, renames the title afterwards.

#### Scenario: One title
- **WHEN** the user chooses Scrape on one movie in the Library
- **THEN** the match picker page `/scrape/match/<id>` opens for that movie

#### Scenario: A selection
- **WHEN** the user chooses Scrape with 12 titles selected
- **THEN** the Scrape page opens listing those 12 titles

#### Scenario: Folder stays
- **WHEN** a New title is scraped while Rename after Scrape is off
- **THEN** its stage becomes Scraped and its folder keeps its name until the user renames it

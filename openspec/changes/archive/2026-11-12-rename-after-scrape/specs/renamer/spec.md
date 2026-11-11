## ADDED Requirements

### Requirement: Rename after Scrape
Settings › Naming SHALL offer "Rename after Scrape" (`renamer.after-scrape`, off by default). While it is on and Metadata only is off, a movie or TV show a scrape was written into SHALL be renamed by itself once its NFO and artwork are written, but only when its plan is a plain change: conflicts, locked titles and titles whose video details are not read yet stay as they are. While Metadata only is on, the switch SHALL be off and disabled.

#### Scenario: A plain change is renamed after its scrape
- **WHEN** Rename after Scrape is on and the user scrapes `heat.1995.1080p`, whose plan renames it to `Heat (1995)` without a conflict
- **THEN** once its NFO and artwork are written the folder is renamed to `Heat (1995)`

#### Scenario: Off by default
- **WHEN** a title is scraped on a fresh install
- **THEN** nothing is renamed and the title carries Rename pending

#### Scenario: Metadata only wins
- **WHEN** Rename after Scrape and Metadata only are both on and a title is scraped
- **THEN** nothing is renamed, and Settings shows Rename after Scrape off and disabled

### Requirement: Rename after Scrape batches and feedback
Titles scraped close together (until no new one for 3 seconds) SHALL be renamed as one journaled batch per kind of title, shown as a job in Activity. When it is done, a toast SHALL say "Renamed N titles after Scrape" with Undo, on whatever page is open. Titles held back SHALL be listed in Activity › Needs attention ("N titles were not renamed after Scrape") with each reason, and a rename that could not run at all SHALL say why there.

#### Scenario: One batch for a scrape of several titles
- **WHEN** Rename after Scrape is on and the user scrapes 12 titles whose plans are plain changes
- **THEN** one rename batch is journaled for them and one toast says "Renamed 12 titles after Scrape" with Undo

#### Scenario: A conflict waits for the user
- **WHEN** Rename after Scrape is on and a scraped title's new name is taken by another title
- **THEN** that title keeps its names and Needs attention lists it with the conflict, while the other titles of the scrape are renamed

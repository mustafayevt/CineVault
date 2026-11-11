## MODIFIED Requirements

### Requirement: Rename only when asked
Folder and file renames SHALL happen only when the user applies them on the Rename page, runs a rename step on the command line, or has turned on "Rename after Scrape" (spec `renamer`), which renames the titles a scrape was written into. No scan, scrape or other background step SHALL rename anything otherwise, and "Rename after Scrape", off by default, SHALL be the only setting that makes renaming automatic.

#### Scenario: Scraping does not rename
- **WHEN** a title whose folder differs from the naming style is scraped while Rename after Scrape is off
- **THEN** its files keep their names and it carries the flag Rename pending

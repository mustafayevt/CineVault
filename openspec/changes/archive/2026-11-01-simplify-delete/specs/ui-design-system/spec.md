## MODIFIED Requirements

### Requirement: One verb glossary
User-facing text SHALL use one vocabulary on buttons, menus, toasts and History: Scan, Scrape and Rename, the stages New, Scraped and Renamed, and Artwork…, Move to…, Rewrite NFO and Delete. Scrape on a scraped title SHALL open the match picker with the current match first. `ui/src/i18n/verbs.test.ts` SHALL fail when a catalogue message contains Identify, Enrich, Rescrape, Scrape again, Change match, Organize or Search & scrape.

#### Scenario: Banned verb in a message
- **WHEN** a catalogue message reads "Rescrape 3 titles"
- **THEN** the verb test fails and names the message key

#### Scenario: Scrape on a scraped title
- **WHEN** the user clicks Scrape on the page of a scraped movie
- **THEN** the match picker opens with the current match pinned first

## ADDED Requirements

### Requirement: Newest releases of the built-in sources
The built-in sources SHALL list their newest releases a page at a time, newest first: YTS movies (paged), EZTV TV (paged), The Pirate Bay its movie and TV video categories (one page), Knaben movies and TV with adult and unsafe results hidden (paged) and Nyaa English-translated anime (one page, with TV). Torrentio SHALL take no part, and a switched-off source SHALL NOT be asked.

#### Scenario: The first page
- **WHEN** the Latest releases list opens with every built-in source on
- **THEN** YTS, EZTV, The Pirate Bay, Knaben and Nyaa are asked for their newest releases, and Torrentio is not contacted

### Requirement: Newest releases of indexers and Torznab feeds
Every indexer of the managed service and the user's own Torznab instance SHALL be asked a search without words in the movie (2000) and TV (5000) categories only, paged by `limit` and `offset`. A feed that refuses a search without words SHALL take no further part and SHALL be reported as unable to list new releases, not as failed. The per-source deadline, request time limit, answer limit and request rate of the search SHALL apply.

#### Scenario: An indexer that needs words
- **WHEN** an indexer answers a search without words with an error about the missing query
- **THEN** it takes no further part in the list and is named as unable to list new releases, not as failed

#### Scenario: Adult categories stay out
- **WHEN** the user's own Torznab instance is asked for its newest releases
- **THEN** only the movie and TV categories are asked

### Requirement: Paging the newest releases
The newest releases of all sources SHALL be merged like a search (one row per release, naming every source that listed it). Each source SHALL be asked for its next page only while it says it has more; a page that adds no new release, or the tenth page, SHALL end it. A page that fails SHALL be asked again on the next request for more. The first page of each kind SHALL be kept for five minutes and Refresh SHALL start over.

#### Scenario: A site that ignores the page number
- **WHEN** a source answers its second page with the same releases as its first
- **THEN** no new row is added and the source is not asked again

#### Scenario: A failed page
- **WHEN** Knaben times out on its second page
- **THEN** the rows already listed stay, and the next request for more asks Knaben for that page again

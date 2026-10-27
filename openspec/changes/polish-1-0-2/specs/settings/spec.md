## MODIFIED Requirements

### Requirement: Providers section
The Providers section SHALL hold the TMDB key ("Needed for Scrape."), the OMDb key (IMDb, Rotten Tomatoes and Metacritic ratings), the MDBList key (more ratings) and the optional Fanart.tv and TheTVDB keys, each with a "Get a free key" link, its state ("Works" or "Rejected"), Show or Hide, Test and Remove, plus the metadata language and the ratings country. Keys SHALL be stored in the operating system's credential store, never in a file. How keys and providers are used is defined by `metadata-providers` and `ratings`.

#### Scenario: Testing a key
- **WHEN** the user pastes a TMDB key and presses Test
- **THEN** the key is checked against TMDB and its state reads "Works" or "Rejected" with the reason

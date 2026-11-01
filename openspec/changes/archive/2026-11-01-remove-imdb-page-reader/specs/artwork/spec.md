## MODIFIED Requirements

### Requirement: Artwork providers
Artwork SHALL come from TMDB, Fanart.tv, TheTVDB and MPDb.tv, asked in a fixed order per library type, and from frame grabs of the video in the picker. A provider that is not set up SHALL be skipped by the automatic choice, and a provider that does not offer a type SHALL not be asked for it.

#### Scenario: Provider order
- **WHEN** artwork is looked up for a movie and for a show
- **THEN** a movie asks TMDB, Fanart.tv, MPDb.tv, TheTVDB in that order, and a show TheTVDB, Fanart.tv, TMDB

#### Scenario: Clearlogo from two providers
- **WHEN** TMDB and Fanart.tv are set up and a movie needs a clearlogo
- **THEN** clearlogo candidates from both providers are considered


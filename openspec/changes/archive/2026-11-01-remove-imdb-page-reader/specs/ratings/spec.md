## MODIFIED Requirements

### Requirement: IMDb Top 250 rank
Each movie and show SHALL be able to hold an IMDb Top 250 rank from 1 to 250. The rank SHALL come from a scrape or the NFO, and the user SHALL be able to set it on the Edit details page. It SHALL be available as the `top250` filter, as a renamer token ("Top 250 place") and in the NFO. CineVault SHALL not fetch the chart itself, because IMDb publishes it only on its web site.

#### Scenario: Top 250 filter
- **WHEN** the user filters with `top250` from 1 to 50
- **THEN** only titles ranked in the top 50 are listed

#### Scenario: Rank from an NFO
- **WHEN** a movie's NFO holds `<top250>12</top250>` and its folder is read
- **THEN** the movie has rank 12 and the `top250` filter finds it

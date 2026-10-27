## MODIFIED Requirements

### Requirement: Ratings keys step
The Ratings keys step ("Add your optional keys") SHALL offer the optional OMDb key ("Adds IMDb, Rotten Tomatoes and Metacritic ratings"), MDBList key, Fanart.tv key ("Adds more artwork") and TheTVDB key ("Adds TV artwork and episode data"), each marked Optional with Test, and SHALL say all of them can be added later in Settings.

#### Scenario: Ratings keys skipped
- **WHEN** the user presses "Skip this step" on the Ratings keys step
- **THEN** the Naming step opens and no ratings key is stored

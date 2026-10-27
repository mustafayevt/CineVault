## MODIFIED Requirements

### Requirement: Keys the user can enter
Settings › Providers SHALL offer the required "TMDB key" (hint "Needed for Scrape.") and the optional "OMDb key" (hint "Adds IMDb, Rotten Tomatoes and Metacritic ratings."), "MDBList key" (hint "Adds Letterboxd and more ratings."), "Fanart.tv key" (hint "Adds more artwork.") and "TheTVDB key" (hint "Adds TV artwork and episode data."), each with a "Get a free key" link to the provider's own key page. The app SHALL ship no keys: every key is the user's own. The section hint SHALL read "Keys are stored in the system credential store, never in files." The Fanart.tv and TheTVDB sources SHALL take part only once their key is saved, and any link in the app that says one of them needs a key SHALL open Settings › Providers at that key.

#### Scenario: Fresh install
- **WHEN** a user opens Settings › Providers on a fresh install
- **THEN** the five key fields are empty and each offers "Get a free key"

#### Scenario: Key link in search
- **WHEN** a search says "TheTVDB needs a key" and the user follows the link
- **THEN** Settings › Providers opens at the TheTVDB key field

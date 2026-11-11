## MODIFIED Requirements

### Requirement: Naming templates in Settings
Settings › Naming SHALL hold the templates "Movie folder", "Movie file" and "TV episode" as text fields in the mono face, each with a live "Example:" line from the library under it, and a row of token buttons that insert at the cursor of the field last used. Any other engine expression SHALL be shown and kept as written. Under the Movie folder buttons a line SHALL say that {Title} is the title each movie uses (its localized title, or its original title when that was chosen) and that {OriginalTitle} is always the original title, with an example.

#### Scenario: Token buttons
- **WHEN** the user looks at the token buttons
- **THEN** the movie fields offer {Title}, {OriginalTitle}, {Year}, {Resolution}, {HDR}, {VideoCodec}, {AudioCodec}, {Channels}, {Edition}, {Source}, {Quality}, {ReleaseTags}, {ReleaseGroup}, {IMDb} and {TMDB}, and the episode field offers {Show}, {ShowOriginalTitle}, {ShowYear}, {Season}, {Season00}, {Episode}, {Episode00}, {EpisodeTitle}, {Resolution}, {HDR}, {VideoCodec}, {AudioCodec}, {Source}, {Quality}, {ReleaseTags} and {ReleaseGroup}

#### Scenario: Insert a token
- **WHEN** the cursor is after `{Year})` in Movie file and the user clicks {Resolution}
- **THEN** `{Resolution}` is inserted at the cursor and the example updates

#### Scenario: Original title in a template
- **WHEN** the Movie folder template is `{OriginalTitle} ({Year})` and a movie titled "The Sleeping City" has the original title "La Ciudad Dormida" from 2019
- **THEN** the folder is named `La Ciudad Dormida (2019)`, whatever title the movie uses

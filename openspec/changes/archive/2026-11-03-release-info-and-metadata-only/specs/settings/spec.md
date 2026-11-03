## MODIFIED Requirements

### Requirement: Naming section
The Naming section SHALL start with the "Metadata only" switch, then hold the Movie folder, Movie file and TV episode templates as mono text fields, each with its tokens and a live example of the result, and the "Colon in titles" choice (Remove, Use " - ", or a look-alike colon). These templates are the ones Rename uses, as defined by the `renamer` capability; changing them SHALL rename nothing by itself. While Metadata only is on, the templates SHALL stay editable.

#### Scenario: Live example
- **WHEN** the user edits the Movie folder template
- **THEN** the example under the field shows the new folder name, and no folder on disk changes

#### Scenario: Metadata only switch
- **WHEN** the user turns on "Metadata only"
- **THEN** the setting is saved at once and Rename and "Move to…" disappear from the app

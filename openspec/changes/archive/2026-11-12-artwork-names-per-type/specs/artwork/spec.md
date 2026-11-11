## MODIFIED Requirements

### Requirement: Artwork file name lists
Artwork files SHALL be named from per-type lists of name choices, and every name in a list SHALL receive the same image. `metadata.artwork-names` SHALL pick the starting lists: `prefixed` (the default), the short fixed names, both of them, or the Emby, Jellyfin or Kodi-format presets. Per-type lists (`artwork.names.movie.<type>`, `artwork.names.tv.<type>`) SHALL replace a starting list; Settings › Naming edits them. Every type SHALL keep at least one name.

#### Scenario: A type cannot be emptied
- **WHEN** the user removes the last name of the fanart type
- **THEN** the change is refused with "Every artwork type is written; keep at least one name"

#### Scenario: Two names for a poster
- **WHEN** the movie poster list is `poster` and `folder` and a poster is saved for `Example Movie (2020).mkv` in its own folder
- **THEN** the same image is written as `poster.jpg` and `folder.jpg`, and in a folder holding several movies as `Example Movie (2020)-poster.jpg`

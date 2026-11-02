## MODIFIED Requirements

### Requirement: Companion files move together
Applying a rename SHALL move or rename every file of the title together and the library paths SHALL follow without a rescan. Videos SHALL keep their stacking markers; readable NFOs and artwork SHALL take every name of the profile, and an `.nfo` CineVault cannot read as an NFO SHALL be set aside like other leftovers; subtitles, trailers, samples and sidecars SHALL be named after the new base name; every other file and sub-folder SHALL move with the folder.

#### Scenario: Subtitle suffix kept
- **WHEN** `old.eng.forced.srt` belongs to a movie renamed to `Example Movie (2020)`
- **THEN** it becomes `Example Movie (2020).eng.forced.srt`, and a `.sub` takes its `.idx` along

#### Scenario: Newest NFO and artwork win
- **WHEN** a movie folder holds two NFOs and two posters under different names
- **THEN** the newest readable NFO is written to every NFO name of the profile, the newest poster to every poster name, and the older ones are set aside

#### Scenario: Sidecars follow the video
- **WHEN** a movie with a sample, a media-info XML, a BIF file and an extra outside a sub-folder is renamed
- **THEN** they become `<base>-sample`, `<base>-mediainfo.xml`, `<base>.bif` and `<base>` plus what followed the old base name, while files in sub-folders keep their names

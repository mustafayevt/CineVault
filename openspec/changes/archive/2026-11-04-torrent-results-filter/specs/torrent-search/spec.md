## ADDED Requirements

### Requirement: Filter the results by text
Once a title is picked, the page SHALL show a "Filter results" field at the right end of the chip row, not usable before the first release arrives. As the user types it SHALL list only the releases whose name holds every word typed, in any order, ignoring case and punctuation, and none of the words that start with `-`.

#### Scenario: Words in any order
- **WHEN** the releases include `Example.Movie.2020.2160p.UHD.BluRay.REMUX.HDR.TrueHD.Atmos-GRP` and the user types `atmos remux`
- **THEN** that release is listed and releases without both words are not

#### Scenario: Leaving a word out
- **WHEN** the user types `1080p -x265`
- **THEN** only 1080p releases whose name does not hold x265 are listed

#### Scenario: Punctuation does not matter
- **WHEN** the user types `web-dl`
- **THEN** releases named with `WEB-DL`, `WEB.DL` or `WEBDL` are listed

### Requirement: Results filter with the chips
The results filter SHALL narrow on top of the chosen chips, and the chip counts SHALL count only the releases it leaves. Esc in the field, or its × button, SHALL clear it, and picking another title SHALL start with it empty. Nothing left by it SHALL say "No releases contain “<text>”." with "Clear filters", which clears the chips and the text.

#### Scenario: Chip counts follow the filter
- **WHEN** 40 releases are listed, 6 of them hold `remux`, 4 of those are 2160p, and the user types `remux`
- **THEN** the All chip shows 6 and the 2160p chip shows 4

#### Scenario: Clearing with Esc
- **WHEN** the filter holds text and the user presses Esc in it
- **THEN** the field is empty and every release is listed again

#### Scenario: Nothing left
- **WHEN** the user types `xyz` and no release name holds it
- **THEN** the results say "No releases contain “xyz”." with "Clear filters"
